//! O PIN de um ramal (plano de produção, item 3.8, lote 1 — R276).
//!
//! Três coisas separadas (decisão do dono, 2026-10-04): o NÚMERO do ramal é a
//! identidade e não é secreto; a PASSWORD SIP é a credencial do aparelho
//! (`ramais.rs`); o PIN é um código secreto de seis dígitos.
//!
//! O que este módulo garante:
//!
//! - o PIN é sorteado pelo servidor (aleatoriedade do SO, sem enviesamento) ou
//!   escolhido por quem tem direito a isso, e nos dois casos passa pelas mesmas
//!   recusas (`delonix_meet_domain::telephony::extension_pin::pin_refusal`);
//! - só o hash (Argon2, `auth::hash_password`) fica na base; o valor em claro
//!   existe UMA vez, na resposta que o gera, e nenhuma leitura o devolve — as
//!   leituras só dizem o estado (`unset` / `set` / `locked`);
//! - **quem vê o PIN de quem.** O PIN de um ramal de PESSOA é dela: só ela o
//!   gera ou escolhe, na sua área (`/my-extension`). O administrador não o vê
//!   nem o define — «forçar a regeneração» é LIMPAR o PIN
//!   (`DELETE …/extensions/{id}/pin`): fica «por definir» e a pessoa gera um
//!   novo. O PIN de um ramal da EMPRESA (sem pessoa) é do administrador: é ele
//!   que o gera ou escolhe, e o vê uma vez;
//! - cinco falhas seguidas bloqueiam o PIN durante quinze minutos, e cada
//!   falha e cada bloqueio ficam na auditoria imutável (`audit::log`).
//!
//! **O que NÃO existe ainda:** nenhum consumidor da verificação. A rota
//! `/internal/v1/voice/ivr/verify-extension-pin` está montada e medida contra
//! Postgres, mas o Lua do IVR não a chama — identificar quem liga de fora por
//! ramal+PIN e o anfitrião por telefone são do lote seguinte. O bloqueio é POR
//! RAMAL: não trava quem experimente o mesmo PIN em muitos ramais; esse travão
//! desenha-se com o fluxo do IVR, que é quem sabe de onde vem a chamada.

use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    Json,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use uuid::Uuid;

use crate::{auth::AuthUser, error::ApiError, voice::check_media_secret, AppState};
use delonix_meet_core::DomainError;
use delonix_meet_domain::telephony::extension_pin as rules;

// ---------- Gerar, validar e guardar ----------

/// Um PIN novo: seis dígitos da aleatoriedade do SO (`crate::crypto`, regra 4
/// do ADR-0004 §5), sorteados de novo enquanto caírem numa das recusas.
fn generate_pin(extension: &str) -> String {
    loop {
        let bytes = crate::crypto::random_bytes(4);
        let random = u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
        if let Some(pin) = rules::pin_candidate(random) {
            if rules::pin_refusal(&pin, extension).is_none() {
                return pin;
            }
        }
    }
}

/// As recusas, para um PIN escolhido por uma pessoa.
fn check_chosen_pin(pin: &str, extension: &str) -> Result<(), ApiError> {
    match rules::pin_refusal(pin, extension) {
        None => Ok(()),
        Some(r) => Err(DomainError::invalid(r.code(), r.message()).into()),
    }
}

/// Guarda o hash do PIN e levanta qualquer bloqueio: um PIN novo começa do zero.
async fn store_pin(state: &AppState, id: Uuid, pin: &str) -> Result<(), ApiError> {
    let hash = crate::auth::hash_password(pin)?;
    sqlx::query(
        "UPDATE voice_extensions
            SET pin_hash = $2, pin_set_at = now(), pin_failed_attempts = 0, pin_locked_until = NULL
          WHERE id = $1",
    )
    .bind(id)
    .bind(hash)
    .execute(&state.db)
    .await?;
    Ok(())
}

// ---------- Tipos ----------

/// O ramal de quem pede, visto pela própria pessoa.
#[derive(Debug, Serialize, sqlx::FromRow, utoipa::ToSchema)]
pub struct MyExtension {
    pub id: Uuid,
    #[schema(example = "1004")]
    pub extension: String,
    pub label: String,
    pub active: bool,
    /// `unset` (por definir), `set` (definido) ou `locked` (bloqueado).
    #[schema(example = "set")]
    pub pin_state: String,
    /// Número curto que o ramal marca para entrar numa reunião.
    #[sqlx(default)]
    pub meeting_access_number: String,
}

#[derive(Deserialize, utoipa::ToSchema)]
pub struct SetPinReq {
    /// Seis dígitos. Recusados: todos iguais, sequências, e o que contém o
    /// número do ramal.
    #[schema(example = "482913")]
    pub pin: String,
}

/// Um PIN acabado de gerar. Sai UMA vez: só o hash fica guardado.
#[derive(Serialize, utoipa::ToSchema)]
pub struct GeneratedPin {
    #[schema(example = "482913")]
    pub pin: String,
    /// O ramal a que o PIN pertence.
    #[schema(example = "1004")]
    pub extension: String,
}

// ============================================================
//  A própria pessoa — o seu ramal e o seu PIN
// ============================================================

async fn own_extension(
    state: &AppState,
    org_id: Uuid,
    user_id: Uuid,
) -> Result<MyExtension, ApiError> {
    crate::org::require_member_pub(state, org_id, user_id).await?;
    let mut ext: MyExtension = sqlx::query_as(
        "SELECT e.id, e.extension, e.label, e.active,
                CASE WHEN e.pin_hash IS NULL THEN 'unset'
                     WHEN e.pin_locked_until > now() THEN 'locked'
                     ELSE 'set' END AS pin_state
           FROM voice_extensions e
          WHERE e.org_id = $1 AND e.member_id = $2",
    )
    .bind(org_id)
    .bind(user_id)
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(|| DomainError::not_found("ramais.no_extension"))?;
    ext.meeting_access_number = state.config.voice_meeting_access_number.clone();
    Ok(ext)
}

/// O ramal de quem pede nesta organização (qualquer membro activo).
#[utoipa::path(
    get, path = "/api/orgs/{org_id}/my-extension", tag = "voice",
    security(("session" = [])),
    params(("org_id" = Uuid, Path, description = "Organização.")),
    responses(
        (status = 200, body = MyExtension, description = "O número e o ESTADO do PIN — nunca o valor."),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 404, body = crate::openapi::ErrorBody, description = "Não é membro activo, ou ainda não tem ramal (`ramais.no_extension`)."),
    )
)]
pub async fn my_extension(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(org_id): Path<Uuid>,
) -> Result<Json<MyExtension>, ApiError> {
    own_extension(&state, org_id, auth.user_id).await.map(Json)
}

/// A pessoa escolhe o PIN do seu ramal. Substitui o anterior e levanta o
/// bloqueio: quem chegou aqui autenticou-se com a sessão.
#[utoipa::path(
    put, path = "/api/orgs/{org_id}/my-extension/pin", tag = "voice",
    security(("session" = [])),
    params(("org_id" = Uuid, Path, description = "Organização.")),
    request_body = SetPinReq,
    responses(
        (status = 204, description = "PIN guardado (só o hash)."),
        (status = 400, body = crate::openapi::ErrorBody, description = "`ramais.pin_format`, `ramais.pin_repeated`, `ramais.pin_sequence` ou `ramais.pin_contains_extension`."),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 404, body = crate::openapi::ErrorBody),
    )
)]
pub async fn set_my_pin(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(org_id): Path<Uuid>,
    Json(req): Json<SetPinReq>,
) -> Result<StatusCode, ApiError> {
    let ext = own_extension(&state, org_id, auth.user_id).await?;
    check_chosen_pin(&req.pin, &ext.extension)?;
    store_pin(&state, ext.id, &req.pin).await?;
    crate::audit::log(
        &state.db,
        Some(org_id),
        auth.user_id,
        "ramal.pin_alterado",
        &ext.extension,
    )
    .await;
    Ok(StatusCode::NO_CONTENT)
}

/// Gera um PIN novo para o ramal de quem pede e mostra-o UMA vez (*custom
/// method*). É também como a pessoa obtém o primeiro PIN, e o caminho depois
/// de o administrador o ter limpo.
#[utoipa::path(
    post, path = "/api/orgs/{org_id}/my-extension/regenerate-pin", tag = "voice",
    security(("session" = [])),
    params(("org_id" = Uuid, Path, description = "Organização.")),
    responses(
        (status = 200, body = GeneratedPin, description = "O PIN sai UMA vez; o anterior deixa de servir."),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 404, body = crate::openapi::ErrorBody),
    )
)]
pub async fn regenerate_my_pin(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(org_id): Path<Uuid>,
) -> Result<Json<GeneratedPin>, ApiError> {
    let ext = own_extension(&state, org_id, auth.user_id).await?;
    let pin = generate_pin(&ext.extension);
    store_pin(&state, ext.id, &pin).await?;
    crate::audit::log(
        &state.db,
        Some(org_id),
        auth.user_id,
        "ramal.pin_gerado",
        &ext.extension,
    )
    .await;
    Ok(Json(GeneratedPin {
        pin,
        extension: ext.extension,
    }))
}

// ============================================================
//  O administrador
// ============================================================

/// `(número, é da empresa)` de um ramal desta organização.
async fn admin_target(
    state: &AppState,
    org_id: Uuid,
    admin: Uuid,
    id: Uuid,
) -> Result<(String, bool), ApiError> {
    crate::org::require_admin_pub(state, org_id, admin).await?;
    sqlx::query_as(
        "SELECT extension, member_id IS NULL FROM voice_extensions WHERE id = $1 AND org_id = $2",
    )
    .bind(id)
    .bind(org_id)
    .fetch_optional(&state.db)
    .await?
    .ok_or(ApiError::NotFound)
}

/// O PIN de um ramal de pessoa não se define nem se vê por terceiros.
fn company_only(is_company: bool) -> Result<(), ApiError> {
    if is_company {
        return Ok(());
    }
    Err(DomainError::conflict(
        "ramais.pin_belongs_to_member",
        "o PIN de um ramal de pessoa é dela: limpa-o, e ela gera um novo na sua área",
    )
    .into())
}

/// O administrador escolhe o PIN de um ramal da EMPRESA.
#[utoipa::path(
    put, path = "/api/orgs/{org_id}/extensions/{id}/pin", tag = "voice",
    security(("session" = [])),
    params(("org_id" = Uuid, Path, description = "Organização."), ("id" = Uuid, Path, description = "Ramal.")),
    request_body = SetPinReq,
    responses(
        (status = 204, description = "PIN guardado (só o hash)."),
        (status = 400, body = crate::openapi::ErrorBody, description = "`ramais.pin_format`, `ramais.pin_repeated`, `ramais.pin_sequence` ou `ramais.pin_contains_extension`."),
        (status = 409, body = crate::openapi::ErrorBody, description = "O ramal é de uma pessoa (`ramais.pin_belongs_to_member`)."),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 403, body = crate::openapi::ErrorBody),
        (status = 404, body = crate::openapi::ErrorBody),
    )
)]
pub async fn set_extension_pin(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path((org_id, id)): Path<(Uuid, Uuid)>,
    Json(req): Json<SetPinReq>,
) -> Result<StatusCode, ApiError> {
    let (extension, is_company) = admin_target(&state, org_id, auth.user_id, id).await?;
    company_only(is_company)?;
    check_chosen_pin(&req.pin, &extension)?;
    store_pin(&state, id, &req.pin).await?;
    crate::audit::log(
        &state.db,
        Some(org_id),
        auth.user_id,
        "ramal.pin_alterado",
        &extension,
    )
    .await;
    Ok(StatusCode::NO_CONTENT)
}

/// Gera o PIN de um ramal da EMPRESA e mostra-o UMA vez ao administrador
/// (*custom method*).
#[utoipa::path(
    post, path = "/api/orgs/{org_id}/extensions/{id}/regenerate-pin", tag = "voice",
    security(("session" = [])),
    params(("org_id" = Uuid, Path, description = "Organização."), ("id" = Uuid, Path, description = "Ramal.")),
    responses(
        (status = 200, body = GeneratedPin, description = "O PIN sai UMA vez; o anterior deixa de servir."),
        (status = 409, body = crate::openapi::ErrorBody, description = "O ramal é de uma pessoa (`ramais.pin_belongs_to_member`): o administrador não vê o PIN dela."),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 403, body = crate::openapi::ErrorBody),
        (status = 404, body = crate::openapi::ErrorBody),
    )
)]
pub async fn regenerate_extension_pin(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path((org_id, id)): Path<(Uuid, Uuid)>,
) -> Result<Json<GeneratedPin>, ApiError> {
    let (extension, is_company) = admin_target(&state, org_id, auth.user_id, id).await?;
    company_only(is_company)?;
    let pin = generate_pin(&extension);
    store_pin(&state, id, &pin).await?;
    crate::audit::log(
        &state.db,
        Some(org_id),
        auth.user_id,
        "ramal.pin_gerado",
        &extension,
    )
    .await;
    Ok(Json(GeneratedPin { pin, extension }))
}

/// Limpa o PIN de um ramal (admin): fica «por definir» e o bloqueio é
/// levantado. Num ramal de pessoa é assim que o administrador FORÇA a
/// regeneração sem ver o PIN — a pessoa gera um novo na sua área. Idempotente.
#[utoipa::path(
    delete, path = "/api/orgs/{org_id}/extensions/{id}/pin", tag = "voice",
    security(("session" = [])),
    params(("org_id" = Uuid, Path, description = "Organização."), ("id" = Uuid, Path, description = "Ramal.")),
    responses(
        (status = 204, description = "PIN limpo; a resposta não traz PIN nenhum."),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 403, body = crate::openapi::ErrorBody),
        (status = 404, body = crate::openapi::ErrorBody),
    )
)]
pub async fn clear_extension_pin(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path((org_id, id)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, ApiError> {
    let (extension, _) = admin_target(&state, org_id, auth.user_id, id).await?;
    sqlx::query(
        "UPDATE voice_extensions
            SET pin_hash = NULL, pin_set_at = NULL, pin_failed_attempts = 0, pin_locked_until = NULL
          WHERE id = $1 AND org_id = $2",
    )
    .bind(id)
    .bind(org_id)
    .execute(&state.db)
    .await?;
    crate::audit::log(
        &state.db,
        Some(org_id),
        auth.user_id,
        "ramal.pin_limpo",
        &extension,
    )
    .await;
    Ok(StatusCode::NO_CONTENT)
}

// ============================================================
//  Verificação — para o IVR (lote seguinte)
// ============================================================

/// O que a verificação de um PIN conclui.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum PinCheck {
    Valid {
        extension_id: Uuid,
        member_id: Option<Uuid>,
        display_name: String,
    },
    /// PIN errado — e também ramal inexistente, inactivo ou de pessoa
    /// arquivada: quem liga não distingue os casos.
    Invalid,
    /// Bloqueado por falhas seguidas. Não se verifica nada enquanto durar.
    Locked { retry_after_secs: i64 },
    /// O ramal existe mas não tem PIN.
    NotSet,
}

/// Verifica o PIN de um ramal ACTIVO da organização. Conta as falhas, bloqueia
/// à quinta (`rules::MAX_FAILED_ATTEMPTS`) durante `rules::LOCK_SECS`, e
/// regista cada falha e cada bloqueio na auditoria imutável.
///
/// A linha do ramal é lida com `FOR UPDATE`: duas tentativas simultâneas não
/// contam como uma, e cinco pedidos em paralelo não dão cinco palpites grátis.
pub(crate) async fn verify_pin(
    state: &AppState,
    org_id: Uuid,
    extension: &str,
    pin: &str,
) -> Result<PinCheck, ApiError> {
    #[derive(sqlx::FromRow)]
    struct Row {
        id: Uuid,
        member_id: Option<Uuid>,
        label: String,
        username: Option<String>,
        pin_hash: Option<String>,
        pin_failed_attempts: i32,
        locked_secs: Option<i64>,
    }
    // O ramal de uma pessoa arquivada não identifica ninguém. A pertença
    // decide-se em org.rs (regra 1, ADR-0004 §5) — e ANTES de abrir a
    // transacção, para não segurar duas ligações da pool ao mesmo tempo.
    let owner: Option<Option<Uuid>> = sqlx::query_scalar(
        "SELECT member_id FROM voice_extensions
          WHERE org_id = $1 AND extension = $2 AND active",
    )
    .bind(org_id)
    .bind(extension)
    .fetch_optional(&state.db)
    .await?;
    match owner {
        None => return Ok(PinCheck::Invalid),
        Some(Some(member_id)) => {
            if crate::org::role_in_org(state, org_id, member_id)
                .await?
                .is_none()
            {
                return Ok(PinCheck::Invalid);
            }
        }
        Some(None) => {}
    }

    let mut tx = state.db.begin().await?;
    let row: Option<Row> = sqlx::query_as(
        "SELECT e.id, e.member_id, e.label, u.username, e.pin_hash, e.pin_failed_attempts,
                CEIL(EXTRACT(EPOCH FROM (e.pin_locked_until - now())))::BIGINT AS locked_secs
           FROM voice_extensions e LEFT JOIN users u ON u.id = e.member_id
          WHERE e.org_id = $1 AND e.extension = $2 AND e.active
            FOR UPDATE OF e",
    )
    .bind(org_id)
    .bind(extension)
    .fetch_optional(&mut *tx)
    .await?;
    let Some(row) = row else {
        return Ok(PinCheck::Invalid);
    };
    if let Some(secs) = row.locked_secs.filter(|s| *s > 0) {
        return Ok(PinCheck::Locked {
            retry_after_secs: secs,
        });
    }
    let Some(hash) = row.pin_hash else {
        return Ok(PinCheck::NotSet);
    };

    if crate::auth::verify_password(pin, &hash) {
        sqlx::query(
            "UPDATE voice_extensions SET pin_failed_attempts = 0, pin_locked_until = NULL
              WHERE id = $1",
        )
        .bind(row.id)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        return Ok(PinCheck::Valid {
            extension_id: row.id,
            member_id: row.member_id,
            display_name: row.username.unwrap_or(row.label),
        });
    }

    let attempts = row.pin_failed_attempts + 1;
    let lock = attempts >= rules::MAX_FAILED_ATTEMPTS;
    // Ao bloquear, o contador volta a zero: passado o bloqueio, são outra vez
    // cinco tentativas — não uma.
    sqlx::query(
        "UPDATE voice_extensions
            SET pin_failed_attempts = CASE WHEN $2 THEN 0 ELSE $3 END,
                pin_locked_until = CASE WHEN $2 THEN now() + make_interval(secs => $4) ELSE NULL END
          WHERE id = $1",
    )
    .bind(row.id)
    .bind(lock)
    .bind(attempts)
    .bind(rules::LOCK_SECS as f64)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;

    // Sem sessão: o actor é a pessoa do ramal, ou ninguém (ramal da empresa).
    // O PIN tentado NUNCA entra no registo.
    let actor = row.member_id.unwrap_or(Uuid::nil());
    crate::audit::log_com_metricas(
        &state.db,
        Some(&state.metrics),
        Some(org_id),
        actor,
        "ramal.pin_falhado",
        &format!(
            "ramal {extension} — tentativa {attempts} de {}",
            rules::MAX_FAILED_ATTEMPTS
        ),
    )
    .await;
    if lock {
        tracing::warn!(%org_id, ramal = %extension, "PIN de ramal bloqueado por falhas seguidas");
        crate::audit::log_com_metricas(
            &state.db,
            Some(&state.metrics),
            Some(org_id),
            actor,
            "ramal.pin_bloqueado",
            &format!("ramal {extension} — {} min", rules::LOCK_SECS / 60),
        )
        .await;
        return Ok(PinCheck::Locked {
            retry_after_secs: rules::LOCK_SECS,
        });
    }
    Ok(PinCheck::Invalid)
}

#[derive(Deserialize)]
pub struct VerifyExtensionPinReq {
    /// Domínio SIP da organização (`<slug>.<VOICE_RAMAIS_DOMAIN_SUFFIX>`).
    pub domain: String,
    pub extension: String,
    pub pin: String,
}

/// Contrato com o IVR (lote seguinte). Sempre `200`: um PIN errado é uma
/// resposta, não uma falha HTTP.
#[derive(Serialize)]
pub struct VerifyExtensionPinResp {
    pub valid: bool,
    /// `invalid`, `locked` ou `not_set`. Ausente quando `valid`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub retry_after_secs: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extension_id: Option<Uuid>,
    /// A pessoa identificada. Ausente num ramal da empresa.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub member_id: Option<Uuid>,
    /// Nome com que a pessoa (ou o ramal da empresa) entra.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
}

impl VerifyExtensionPinResp {
    fn refused(reason: &'static str, retry_after_secs: Option<i64>) -> Self {
        Self {
            valid: false,
            reason: Some(reason),
            retry_after_secs,
            extension_id: None,
            member_id: None,
            display_name: None,
        }
    }
}

/// `POST /internal/v1/voice/ivr/verify-extension-pin` — no listener interno,
/// com o segredo de voz (`X-Voice-Secret`), como as outras rotas do IVR.
/// **Ainda sem consumidor:** o `dialin_ivr.lua` não a chama neste lote.
pub async fn ivr_verify_extension_pin(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(req): Json<VerifyExtensionPinReq>,
) -> Result<Json<VerifyExtensionPinResp>, ApiError> {
    check_media_secret(&state, &headers)?;
    // Um domínio que não é de nenhuma organização responde como um PIN errado.
    let Some(org_id) = crate::ramais::org_id_by_sip_domain(&state, req.domain.trim()).await else {
        return Ok(Json(VerifyExtensionPinResp::refused("invalid", None)));
    };
    let resp = match verify_pin(&state, org_id, req.extension.trim(), req.pin.trim()).await? {
        PinCheck::Valid {
            extension_id,
            member_id,
            display_name,
        } => VerifyExtensionPinResp {
            valid: true,
            reason: None,
            retry_after_secs: None,
            extension_id: Some(extension_id),
            member_id,
            display_name: Some(display_name),
        },
        PinCheck::Invalid => VerifyExtensionPinResp::refused("invalid", None),
        PinCheck::NotSet => VerifyExtensionPinResp::refused("not_set", None),
        PinCheck::Locked { retry_after_secs } => {
            VerifyExtensionPinResp::refused("locked", Some(retry_after_secs))
        }
    };
    Ok(Json(resp))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn o_pin_gerado_tem_seis_digitos_e_passa_as_recusas() {
        let mut seen = std::collections::HashSet::new();
        for _ in 0..500 {
            let pin = generate_pin("1234");
            assert_eq!(pin.len(), 6);
            assert!(pin.bytes().all(|b| b.is_ascii_digit()));
            assert_eq!(rules::pin_refusal(&pin, "1234"), None, "{pin}");
            seen.insert(pin);
        }
        // 500 sorteios em 10⁶: mais de um punhado de repetições é um gerador
        // partido, não azar.
        assert!(seen.len() > 490, "só {} PIN distintos em 500", seen.len());
    }

    #[test]
    fn um_pin_escolhido_passa_pelas_mesmas_recusas() {
        assert!(check_chosen_pin("482913", "1234").is_ok());
        for bad in ["111111", "123456", "654321", "001234", "12345", "abcdef"] {
            assert!(check_chosen_pin(bad, "1234").is_err(), "{bad}");
        }
    }
}
