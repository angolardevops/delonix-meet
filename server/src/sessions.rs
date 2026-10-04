//! O ESTADO das sessões da própria conta e a reautenticação recente.
//!
//! A lista e o «terminar uma» já existiam em [`crate::account`]
//! (`GET /api/users/me/sessions`, `DELETE /api/users/me/sessions/{session_id}`,
//! lidas de `refresh_tokens.session_id`, migração 0065) e ficam lá. Este módulo
//! acrescenta o que faltava (ADR-0011):
//! - `POST /api/users/me/sessions/revoke-others`   método personalizado: termina todas menos a actual
//! - `POST /api/users/me/reauthentication`         prova a identidade de novo (password ou código)
//!
//! **Terminar é imediato.** Uma sessão terminada:
//! 1. revoga os refresh tokens dela (o próximo `/api/auth/refresh` dá `401`);
//! 2. deixa de abrir a API já — o access token leva o `sid` e o extractor
//!    [`crate::auth::AuthUser`] consulta o estado da sessão em cada pedido;
//! 3. fecha os WebSockets dessa sessão (`/rtc` e `/ws` da sala) neste nó, e
//!    publica no Redis para os outros nós fazerem o mesmo ([`KillRegistry`]).
//!
//! **Isolamento.** Tudo é filtrado pelo `user_id` da sessão: uma sessão de
//! outra pessoa dá a mesma resposta que uma inexistente (`404`), também para
//! um administrador da organização — terminar sessões de outros é uma acção de
//! administração (suspender a conta), não desta superfície.

use axum::{extract::State, Json};
use chrono::{DateTime, Utc};
use dashmap::DashMap;
use delonix_meet_core::{DomainError, ErrorKind};
use delonix_meet_domain::identity::session as rules;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use uuid::Uuid;

use crate::{auth::AuthUser, error::ApiError, AppState};

/// Como a sessão foi aberta.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthMethod {
    Password,
    Mfa,
    Passkey,
    Sso,
    Odoo,
    Legacy,
}

impl AuthMethod {
    pub fn as_str(self) -> &'static str {
        match self {
            AuthMethod::Password => "password",
            AuthMethod::Mfa => "mfa",
            AuthMethod::Passkey => "passkey",
            AuthMethod::Sso => "sso",
            AuthMethod::Odoo => "odoo",
            AuthMethod::Legacy => "legacy",
        }
    }
}

/// Regista o estado da sessão `id` (o `refresh_tokens.session_id` da 0065).
///
/// Com `method` (um login): a sessão nasce, e um login acabado de fazer É uma
/// prova de identidade — `reauthenticated_at` nasce preenchido. Sem `method`
/// (um refresh): a sessão já existe e só se toca no «visto por último» e no
/// dispositivo; se não existir (token emitido antes da 0078 correr neste nó),
/// nasce `legacy`, sem prova recente.
pub(crate) async fn upsert(
    state: &AppState,
    id: Uuid,
    user_id: Uuid,
    method: Option<AuthMethod>,
    user_agent: &str,
    ip: &str,
    started_at: DateTime<Utc>,
) -> Result<(), ApiError> {
    let method = method.unwrap_or(AuthMethod::Legacy);
    sqlx::query(
        "INSERT INTO user_sessions
            (id, user_id, auth_method, user_agent, ip, created_at, reauthenticated_at)
         VALUES ($1, $2, $3, $4, $5, $6, CASE WHEN $3 = 'legacy' THEN NULL ELSE now() END)
         ON CONFLICT (id) DO UPDATE
            SET last_seen_at = now(), user_agent = EXCLUDED.user_agent, ip = EXCLUDED.ip
          WHERE user_sessions.user_id = EXCLUDED.user_id",
    )
    .bind(id)
    .bind(user_id)
    .bind(method.as_str())
    .bind(rules::clip_user_agent(user_agent))
    .bind(ip.chars().take(64).collect::<String>())
    .bind(started_at)
    .execute(&state.db)
    .await?;
    Ok(())
}

pub const SESSION_REVOKED: &str = "auth.session_revoked";

/// O estado de uma sessão que o extractor precisa de saber.
pub(crate) struct ActiveSession {
    pub reauthenticated_at: Option<DateTime<Utc>>,
}

/// `(user_id, revoked_at, last_seen_at, reauthenticated_at)`.
type SessionStateRow = (
    Uuid,
    Option<DateTime<Utc>>,
    DateTime<Utc>,
    Option<DateTime<Utc>>,
);

/// A sessão `sid` de `user_id` está activa? Actualiza o «visto por último» no
/// máximo uma vez por minuto (não é uma escrita por pedido).
pub(crate) async fn ensure_active(
    state: &AppState,
    user_id: Uuid,
    sid: Uuid,
) -> Result<ActiveSession, ApiError> {
    let row: Option<SessionStateRow> = sqlx::query_as(
        "SELECT user_id, revoked_at, last_seen_at, reauthenticated_at
               FROM user_sessions WHERE id = $1",
    )
    .bind(sid)
    .fetch_optional(&state.db)
    .await?;
    let revoked = || {
        ApiError::Domain(DomainError::new(
            ErrorKind::Unauthenticated,
            SESSION_REVOKED,
            "esta sessão foi terminada — entre de novo",
        ))
    };
    let Some((owner, revoked_at, last_seen, reauthenticated_at)) = row else {
        return Err(revoked());
    };
    if owner != user_id || revoked_at.is_some() {
        return Err(revoked());
    }
    if (Utc::now() - last_seen).num_seconds() >= 60 {
        let _ = sqlx::query(
            "UPDATE user_sessions SET last_seen_at = now() WHERE id = $1 AND revoked_at IS NULL",
        )
        .bind(sid)
        .execute(&state.db)
        .await;
    }
    Ok(ActiveSession { reauthenticated_at })
}

// ---------------------------------------------------------------------------
//  Fecho de WebSockets por sessão
// ---------------------------------------------------------------------------

/// Ligações de tempo real de cada sessão NESTE nó. Cada ligação regista o seu
/// `Notify` de encerramento; terminar a sessão acorda-os todos, e o laço da
/// ligação sai pelo mesmo caminho ordenado de sempre (limpeza do peer, aviso
/// de saída aos outros).
#[derive(Default)]
pub struct KillRegistry {
    conns: DashMap<Uuid, Vec<Arc<tokio::sync::Notify>>>,
}

/// Remove o registo quando a ligação termina.
pub struct KillGuard<'a> {
    registry: &'a KillRegistry,
    sid: Uuid,
    notify: Arc<tokio::sync::Notify>,
}

impl Drop for KillGuard<'_> {
    fn drop(&mut self) {
        if let Some(mut v) = self.registry.conns.get_mut(&self.sid) {
            v.retain(|n| !Arc::ptr_eq(n, &self.notify));
        }
        self.registry
            .conns
            .remove_if(&self.sid, |_, v| v.is_empty());
    }
}

impl KillRegistry {
    pub fn register(&self, sid: Uuid, notify: Arc<tokio::sync::Notify>) -> KillGuard<'_> {
        self.conns.entry(sid).or_default().push(notify.clone());
        KillGuard {
            registry: self,
            sid,
            notify,
        }
    }

    /// Acorda as ligações locais da sessão. Devolve quantas eram.
    pub fn kill_local(&self, sid: Uuid) -> usize {
        match self.conns.get(&sid) {
            Some(v) => {
                for n in v.iter() {
                    n.notify_one();
                }
                v.len()
            }
            None => 0,
        }
    }

    pub fn local_count(&self, sid: Uuid) -> usize {
        self.conns.get(&sid).map(|v| v.len()).unwrap_or(0)
    }
}

/// Fecha as ligações da sessão aqui e nos outros nós.
pub(crate) async fn kill_everywhere(state: &AppState, sid: Uuid) {
    let n = state.session_kills.kill_local(sid);
    if n > 0 {
        tracing::info!(%sid, ligações = n, "sessão terminada: a fechar ligações locais");
    }
    if let Some(bus) = &state.redis_bus {
        bus.publish_session_revoked(sid).await;
    }
}

/// Revoga uma sessão (e os refresh tokens dela) se for de `user_id` e estiver
/// activa. Devolve `false` se não havia nada a revogar.
pub(crate) async fn revoke(
    state: &AppState,
    user_id: Uuid,
    sid: Uuid,
    reason: &str,
) -> Result<bool, ApiError> {
    let mut tx = state.db.begin().await?;
    let n = sqlx::query(
        "UPDATE user_sessions SET revoked_at = now(), revoked_reason = $3
          WHERE id = $1 AND user_id = $2 AND revoked_at IS NULL",
    )
    .bind(sid)
    .bind(user_id)
    .bind(reason)
    .execute(&mut *tx)
    .await?
    .rows_affected();
    // Filtrado pelo dono também aqui: `refresh_tokens.session_id` não tem
    // chave estrangeira (0065), e o id de uma sessão alheia não revoga nada.
    let tokens = sqlx::query(
        "UPDATE refresh_tokens SET revoked = TRUE
          WHERE session_id = $1 AND user_id = $2 AND NOT revoked",
    )
    .bind(sid)
    .bind(user_id)
    .execute(&mut *tx)
    .await?
    .rows_affected();
    tx.commit().await?;
    let any = n > 0 || tokens > 0;
    if any {
        kill_everywhere(state, sid).await;
    }
    Ok(any)
}

/// Termina todas as sessões activas de `user_id` menos `keep` — a regra ÚNICA
/// de «terminar as outras», chamada pelo `revoke-others` e pela mudança de
/// password. Com `keep = None` (access token anterior às sessões, que não diz
/// qual é a actual) termina todas. Devolve quantas terminou.
pub(crate) async fn revoke_all_except(
    state: &AppState,
    user_id: Uuid,
    keep: Option<Uuid>,
) -> Result<u64, ApiError> {
    let ids: Vec<Uuid> = sqlx::query_scalar(
        "SELECT id FROM user_sessions
          WHERE user_id = $1 AND revoked_at IS NULL AND ($2::uuid IS NULL OR id <> $2)",
    )
    .bind(user_id)
    .bind(keep)
    .fetch_all(&state.db)
    .await?;
    let mut revoked = 0;
    for sid in ids {
        if revoke(state, user_id, sid, "user_revoked_others").await? {
            revoked += 1;
        }
    }
    Ok(revoked)
}

/// Varredor: sessões sem refresh token vivo passam a `expired`; sessões
/// terminadas há mais de 90 dias e cerimónias WebAuthn vencidas saem.
pub(crate) async fn sweep(db: &sqlx::PgPool) -> Result<(u64, u64), sqlx::Error> {
    let expired = sqlx::query(
        "UPDATE user_sessions s SET revoked_at = now(), revoked_reason = 'expired'
          WHERE s.revoked_at IS NULL
            AND s.created_at < now() - interval '1 hour'
            AND NOT EXISTS (SELECT 1 FROM refresh_tokens t
                             WHERE t.session_id = s.id AND NOT t.revoked AND t.expires_at > now())",
    )
    .execute(db)
    .await?
    .rows_affected();
    let deleted =
        sqlx::query("DELETE FROM user_sessions WHERE revoked_at < now() - interval '90 days'")
            .execute(db)
            .await?
            .rows_affected();
    sqlx::query("DELETE FROM webauthn_ceremonies WHERE expires_at < now()")
        .execute(db)
        .await?;
    Ok((expired, deleted))
}

// ---------------------------------------------------------------------------
//  Endpoints
// ---------------------------------------------------------------------------

#[derive(Serialize, utoipa::ToSchema)]
pub struct RevokedCount {
    /// Quantas sessões foram terminadas.
    pub revoked: u64,
}

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ReauthReq {
    /// A password da conta (verificada no Odoo numa conta gerida).
    pub password: Option<String>,
    /// Ou um código TOTP / de recuperação, se o MFA estiver activo.
    pub code: Option<String>,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct Reauthenticated {
    /// Até quando vale para alterar factores.
    pub valid_until: DateTime<Utc>,
}

#[derive(utoipa::OpenApi)]
#[openapi(
    paths(revoke_others, reauthenticate),
    components(schemas(RevokedCount, ReauthReq, Reauthenticated))
)]
pub struct ApiDoc;

/// Método personalizado: termina todas as minhas sessões excepto a deste
/// pedido.
#[utoipa::path(
    post, path = "/api/users/me/sessions/revoke-others", tag = "sessions",
    security(("session" = [])),
    responses(
        (status = 200, body = RevokedCount),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 422, description = "O access token é anterior às sessões e não diz qual é a actual (`sessions.current_unknown`): renove a sessão e repita.", body = crate::openapi::ErrorBody),
    )
)]
pub async fn revoke_others(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
) -> Result<Json<RevokedCount>, ApiError> {
    let current = auth.session_id.ok_or_else(|| {
        DomainError::precondition(
            "sessions.current_unknown",
            "esta sessão é anterior à lista de sessões: renove-a (/api/auth/refresh) e repita",
        )
    })?;
    let revoked = revoke_all_except(&state, auth.user_id, Some(current)).await?;
    crate::audit::log(
        &state.db,
        None,
        auth.user_id,
        "session.revoked_others",
        &revoked.to_string(),
    )
    .await;
    Ok(Json(RevokedCount { revoked }))
}

/// Prova a identidade outra vez, para alterar factores (chaves de acesso,
/// códigos de recuperação). Vale `REAUTH_WINDOW_SECS` para ESTA sessão.
///
/// Aceita a password (numa conta gerida pelo Odoo, verificada no Odoo; com o
/// Odoo em baixo, o hash da última entrada, como no login) ou um código TOTP /
/// de recuperação. Uma conta só com SSO e sem MFA não tem como se
/// reautenticar aqui (`reauthentication.no_method`).
#[utoipa::path(
    post, path = "/api/users/me/reauthentication", tag = "sessions",
    security(("session" = [])),
    request_body = ReauthReq,
    responses(
        (status = 200, body = Reauthenticated),
        (status = 400, description = "Nem `password` nem `code` (`reauthentication.missing_proof`).", body = crate::openapi::ErrorBody),
        (status = 401, description = "Prova errada (`reauthentication.failed`) ou sessão terminada.", body = crate::openapi::ErrorBody),
        (status = 422, description = "Access token anterior às sessões (`sessions.current_unknown`), ou conta sem método (`reauthentication.no_method`).", body = crate::openapi::ErrorBody),
        (status = 429, description = "Cinco provas erradas em 5 minutos.", body = crate::openapi::ErrorBody),
    )
)]
pub async fn reauthenticate(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Json(req): Json<ReauthReq>,
) -> Result<Json<Reauthenticated>, ApiError> {
    let sid = auth.session_id.ok_or_else(|| {
        DomainError::precondition(
            "sessions.current_unknown",
            "esta sessão é anterior à lista de sessões: renove-a (/api/auth/refresh) e repita",
        )
    })?;
    let key = format!("reauth:{}", auth.user_id);
    if state.mfa_limiter.is_blocked(&key) {
        return Err(ApiError::TooManyRequests);
    }
    let ok = match (req.password.as_deref(), req.code.as_deref()) {
        (Some(p), _) if !p.is_empty() => {
            match crate::auth::check_password_of(&state, auth.user_id, p).await? {
                crate::auth::PasswordCheck::NoLocalPassword if req.code.is_none() => {
                    return Err(DomainError::precondition(
                        "reauthentication.no_method",
                        "esta conta entra por SSO: use um código de autenticação",
                    )
                    .into())
                }
                crate::auth::PasswordCheck::Valid => true,
                _ => false,
            }
        }
        (_, Some(c)) if !c.is_empty() => {
            crate::mfa::consome_codigo(&state, auth.user_id, c).await?
        }
        _ => {
            return Err(DomainError::invalid(
                "reauthentication.missing_proof",
                "indique a password ou um código",
            )
            .into())
        }
    };
    if !ok {
        return Err(failed_proof(&state, auth.user_id, &key).await);
    }
    let at: DateTime<Utc> = sqlx::query_scalar(
        "UPDATE user_sessions SET reauthenticated_at = now()
          WHERE id = $1 AND user_id = $2 AND revoked_at IS NULL
          RETURNING reauthenticated_at",
    )
    .bind(sid)
    .bind(auth.user_id)
    .fetch_optional(&state.db)
    .await?
    .ok_or(ApiError::Unauthorized)?;
    crate::audit::log(
        &state.db,
        None,
        auth.user_id,
        "auth.reauthenticated",
        &sid.to_string(),
    )
    .await;
    Ok(Json(Reauthenticated {
        valid_until: at + chrono::Duration::seconds(rules::REAUTH_WINDOW_SECS),
    }))
}

/// Uma prova de identidade que não confere: conta para o travão (cinco em 5
/// minutos, partilhado entre a reautenticação e a mudança de password — quem
/// adivinha passwords não ganha tentativas por mudar de rota) e fica na
/// auditoria.
async fn failed_proof(state: &AppState, user_id: Uuid, key: &str) -> ApiError {
    if !state.mfa_limiter.check(key) {
        return ApiError::TooManyRequests;
    }
    crate::audit::log(&state.db, None, user_id, "auth.reauthentication_failed", "").await;
    DomainError::new(
        ErrorKind::Unauthenticated,
        "reauthentication.failed",
        "a prova não confere",
    )
    .into()
}

/// Prova a identidade com a password ACTUAL, no próprio pedido — para a
/// mudança de password, onde pedir uma reautenticação à parte só para depois
/// escrever a password outra vez seria cerimónia. Mesma regra de «esta
/// password é desta conta» do login (`auth::check_password_of`), mesmo travão
/// e mesmo registo da reautenticação. Não abre a janela de reautenticação.
pub(crate) async fn prove_current_password(
    state: &Arc<AppState>,
    user_id: Uuid,
    password: &str,
) -> Result<(), ApiError> {
    let key = format!("reauth:{user_id}");
    if state.mfa_limiter.is_blocked(&key) {
        return Err(ApiError::TooManyRequests);
    }
    match crate::auth::check_password_of(state, user_id, password).await? {
        crate::auth::PasswordCheck::Valid => Ok(()),
        _ => Err(failed_proof(state, user_id, &key).await),
    }
}

/// Exige reautenticação recente NESTA sessão (para alterar factores).
pub(crate) fn require_recent(auth: &AuthUser) -> Result<(), ApiError> {
    rules::require_recent_reauth(auth.reauthenticated_at, Utc::now()).map_err(ApiError::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn kill_registry_wakes_only_that_session_and_cleans_up() {
        let reg = KillRegistry::default();
        let a = Uuid::new_v4();
        let b = Uuid::new_v4();
        let na = Arc::new(tokio::sync::Notify::new());
        let na2 = Arc::new(tokio::sync::Notify::new());
        let nb = Arc::new(tokio::sync::Notify::new());
        {
            let _ga = reg.register(a, na.clone());
            let _ga2 = reg.register(a, na2.clone());
            let _gb = reg.register(b, nb.clone());
            assert_eq!(reg.kill_local(a), 2);
            // o permit fica guardado: quem esperar a seguir acorda já
            tokio::time::timeout(std::time::Duration::from_millis(50), na.notified())
                .await
                .expect("a ligação A acorda");
            tokio::time::timeout(std::time::Duration::from_millis(50), na2.notified())
                .await
                .expect("a segunda ligação de A acorda");
            assert!(
                tokio::time::timeout(std::time::Duration::from_millis(50), nb.notified())
                    .await
                    .is_err(),
                "a sessão B não é tocada"
            );
        }
        assert_eq!(reg.local_count(a), 0, "os guards limpam");
        assert_eq!(reg.kill_local(a), 0);
    }
}
