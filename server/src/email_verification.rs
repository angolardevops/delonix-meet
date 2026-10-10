//! Prova do endereço de email (D7, decidido a 2026-10-09; migração 0111).
//!
//! **PORQUE EXISTE.** O `users.email` é afirmado — por quem se regista, ou por um
//! administrador — e nunca confirmado. A reposição de password pela própria
//! pessoa (E3) dá a conta a quem controla o ENDEREÇO, e por isso só pode servir
//! endereços provados: um `joao@gmai.com` mal escrito tornava-se uma porta para
//! uma conta que já tem dados. Este módulo é essa prova, e nada mais.
//!
//! **Duas rotas.** A pessoa, com sessão, pede o email
//! (`POST /api/users/me/email-verification`); abre o link e a web entrega o
//! token SEM sessão (`POST /api/email-verifications/accept`) — o link pode ser
//! aberto noutro aparelho.
//!
//! **O token nunca sai na resposta da API.** Só vai no email: devolvê-lo a quem
//! pede provaria a sessão, não o endereço. E vai no FRAGMENTO do link
//! (`#/verificar-email?token=…`), que o browser não envia a servidor nenhum —
//! nem ao nosso, nem a um proxy pelo caminho, nem a um registo de acessos.
//!
//! **O link sai de `PUBLIC_URL`, nunca do `Host` do pedido** — ver
//! `Config::public_url`.

use std::sync::Arc;

use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use delonix_meet_core::DomainError;

use crate::{auth::AuthUser, error::ApiError, AppState};

/// 24 h: chega para a pessoa ir à caixa de correio no mesmo dia, e não deixa
/// um link esquecido válido por uma semana.
const VALIDADE_HORAS: i64 = 24;

/// Entre dois pedidos da mesma pessoa. Sem isto, uma sessão roubada (ou um
/// clique repetido) enchia a caixa de correio da vítima e gastava o relay.
const INTERVALO_SEGUNDOS: i64 = 60;

#[derive(utoipa::OpenApi)]
#[openapi(
    paths(status, request, accept),
    components(schemas(
        EmailVerificationStatus,
        AcceptEmailVerificationReq,
        AcceptedEmailVerification
    ))
)]
pub struct ApiDoc;

#[derive(Serialize, utoipa::ToSchema)]
pub struct EmailVerificationStatus {
    /// `verified` — o endereço está provado;
    /// `pending` — há um link enviado e ainda válido;
    /// `unverified` — nunca se provou, ou o último link já não vale;
    /// `sent` — (só no POST) saiu agora um email com o link.
    pub status: &'static str,
    pub email: String,
    /// Quando o link enviado deixa de valer (`pending` e `sent`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<DateTime<Utc>>,
}

#[derive(Deserialize, utoipa::ToSchema)]
pub struct AcceptEmailVerificationReq {
    pub token: String,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct AcceptedEmailVerification {
    pub email: String,
    pub verified_at: DateTime<Utc>,
}

fn token_hash(token: &str) -> String {
    delonix_meet_core::crypto::sha256_hex(token.trim())
}

/// O link que vai no email.
fn link(public_url: &str, token: &str) -> String {
    format!("{public_url}/#/verificar-email?token={token}")
}

/// O texto do email, na língua da conta (pt por omissão; en para quem a
/// escolheu). Texto simples: não há HTML a escapar nem imagens a seguir.
fn mensagem(locale: &str, email: &str, link: &str) -> (&'static str, String) {
    if locale.starts_with("en") {
        (
            "Confirm your email address",
            format!(
                "Someone (hopefully you) asked to confirm {email} as the address of a \
                 Delonix Meet account.\n\nOpen this link within {VALIDADE_HORAS} hours to \
                 confirm it:\n\n{link}\n\nIf it wasn't you, ignore this message: nothing \
                 changes until the link is opened.\n"
            ),
        )
    } else {
        (
            "Confirme o seu endereço de email",
            format!(
                "Alguém (esperamos que tenha sido você) pediu para confirmar {email} como \
                 endereço de uma conta do Delonix Meet.\n\nAbra este link nas próximas \
                 {VALIDADE_HORAS} horas para o confirmar:\n\n{link}\n\nSe não foi você, \
                 ignore esta mensagem: nada muda enquanto o link não for aberto.\n"
            ),
        )
    }
}

/// O estado da prova do endereço da própria conta.
///
/// Não vai no `UserPublic` de propósito: a resposta do login constrói-o à mão
/// e o seu JSON é contrato da web (`login_response_serializa_como_antes`).
#[utoipa::path(
    get, path = "/api/users/me/email-verification", tag = "users",
    security(("session" = [])),
    responses(
        (status = 200, body = EmailVerificationStatus),
        (status = 401, body = crate::openapi::ErrorBody),
    )
)]
pub async fn status(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
) -> Result<Json<EmailVerificationStatus>, ApiError> {
    let (email, verified_at, pending_until): (
        String,
        Option<DateTime<Utc>>,
        Option<DateTime<Utc>>,
    ) = sqlx::query_as(
        "SELECT u.email, u.email_verified_at,
                    (SELECT v.expires_at FROM email_verifications v
                      WHERE v.user_id = u.id AND v.status = 'pending' AND v.expires_at > now())
               FROM users u WHERE u.id = $1",
    )
    .bind(auth.user_id)
    .fetch_one(&state.db)
    .await?;
    let status = match (verified_at, pending_until) {
        (Some(_), _) => "verified",
        (None, Some(_)) => "pending",
        (None, None) => "unverified",
    };
    Ok(Json(EmailVerificationStatus {
        status,
        email,
        expires_at: if verified_at.is_none() {
            pending_until
        } else {
            None
        },
    }))
}

/// Pede o email de prova do endereço da própria conta.
#[utoipa::path(
    post, path = "/api/users/me/email-verification", tag = "users",
    security(("session" = [])),
    responses(
        (status = 200, body = EmailVerificationStatus, description = "`verified`: o endereço já estava provado e nada foi enviado."),
        (status = 202, body = EmailVerificationStatus, description = "`sent`: o email saiu para a fila. O token só vai no email."),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 422, body = crate::openapi::ErrorBody, description = "`mail.disabled` (sem relay) ou `mail.public_url_missing` (sem `PUBLIC_URL`)"),
        (status = 429, body = crate::openapi::ErrorBody, description = "um pedido há menos de um minuto — `Retry-After` diz quanto falta"),
    )
)]
pub async fn request(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
) -> Result<Response, ApiError> {
    let (email, locale, verified_at): (String, String, Option<DateTime<Utc>>) = sqlx::query_as(
        "SELECT email, COALESCE(locale, 'pt'), email_verified_at FROM users WHERE id = $1",
    )
    .bind(auth.user_id)
    .fetch_one(&state.db)
    .await?;
    if verified_at.is_some() {
        return Ok((
            StatusCode::OK,
            Json(EmailVerificationStatus {
                status: "verified",
                email,
                expires_at: None,
            }),
        )
            .into_response());
    }
    // As duas pré-condições ANTES de criar o token: não se promete um email
    // que não vai sair, nem se deixa um token pendente que ninguém recebe.
    if !crate::mail::enabled(&state) {
        return Err(DomainError::precondition(
            "mail.disabled",
            "o correio não está configurado neste servidor",
        )
        .into());
    }
    let Some(public_url) = state.config.public_url.as_deref() else {
        return Err(DomainError::precondition(
            "mail.public_url_missing",
            "o servidor não sabe o seu endereço público (PUBLIC_URL) e não escreve links",
        )
        .into());
    };

    let token = delonix_meet_core::crypto::prefixed_token("dlxv_");
    let prefix: String = token.chars().take(12).collect();
    let expires_at = Utc::now() + chrono::Duration::hours(VALIDADE_HORAS);

    let mut tx = state.db.begin().await?;
    // O intervalo lê-se E a anterior revoga-se na mesma transacção, com a
    // linha da pessoa bloqueada: dois pedidos simultâneos não passam os dois.
    sqlx::query("SELECT 1 FROM users WHERE id = $1 FOR UPDATE")
        .bind(auth.user_id)
        .execute(&mut *tx)
        .await?;
    let ultimo: Option<DateTime<Utc>> =
        sqlx::query_scalar("SELECT max(created_at) FROM email_verifications WHERE user_id = $1")
            .bind(auth.user_id)
            .fetch_one(&mut *tx)
            .await?;
    if let Some(ultimo) = ultimo {
        let passou = (Utc::now() - ultimo).num_seconds();
        if passou < INTERVALO_SEGUNDOS {
            return Err(ApiError::RateLimited {
                retry_after_secs: (INTERVALO_SEGUNDOS - passou) as u64,
            });
        }
    }
    sqlx::query(
        "UPDATE email_verifications SET status = 'revoked'
          WHERE user_id = $1 AND status = 'pending'",
    )
    .bind(auth.user_id)
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "INSERT INTO email_verifications (user_id, email, token_hash, token_prefix, expires_at)
         VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(auth.user_id)
    .bind(&email)
    .bind(token_hash(&token))
    .bind(&prefix)
    .bind(expires_at)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;

    let (subject, body) = mensagem(&locale, &email, &link(public_url, &token));
    crate::mail::enqueue(
        &state,
        crate::mail::Outgoing {
            org_id: None,
            purpose: crate::mail::Purpose::EmailVerification,
            to: &email,
            subject,
            body: &body,
        },
    )
    .await?;
    crate::audit::log(
        &state.db,
        None,
        auth.user_id,
        "user.email_verification_sent",
        &prefix,
    )
    .await;

    Ok((
        StatusCode::ACCEPTED,
        Json(EmailVerificationStatus {
            status: "sent",
            email,
            expires_at: Some(expires_at),
        }),
    )
        .into_response())
}

/// Usa o token do email. **Pública**: o link pode abrir-se noutro aparelho.
#[utoipa::path(
    post, path = "/api/email-verifications/accept", tag = "users",
    request_body = AcceptEmailVerificationReq,
    responses(
        (status = 200, body = AcceptedEmailVerification),
        (status = 404, body = crate::openapi::ErrorBody, description = "`email_verification.not_found` — token errado, já usado ou revogado"),
        (status = 409, body = crate::openapi::ErrorBody, description = "`email_verification.email_changed` — a conta mudou de email depois do pedido"),
        (status = 422, body = crate::openapi::ErrorBody, description = "`email_verification.expired`"),
        (status = 429, body = crate::openapi::ErrorBody, description = "limite por IP"),
    )
)]
pub async fn accept(
    State(state): State<Arc<AppState>>,
    Json(req): Json<AcceptEmailVerificationReq>,
) -> Result<Json<AcceptedEmailVerification>, ApiError> {
    // Um 404 igual para token errado, usado, revogado e inexistente: distinguir
    // dava um oráculo a quem adivinha.
    let not_found = || ApiError::from(DomainError::not_found("email_verification.not_found"));

    let mut tx = state.db.begin().await?;
    // A busca É a comparação: a linha encontra-se PELO hash (256 bits).
    let row: Option<(Uuid, Uuid, String, DateTime<Utc>)> = sqlx::query_as(
        "SELECT id, user_id, email, expires_at FROM email_verifications
          WHERE token_hash = $1 AND status = 'pending' FOR UPDATE",
    )
    .bind(token_hash(&req.token))
    .fetch_optional(&mut *tx)
    .await?;
    let Some((id, user_id, email, expires_at)) = row else {
        return Err(not_found());
    };
    if expires_at <= Utc::now() {
        sqlx::query("UPDATE email_verifications SET status = 'expired' WHERE id = $1")
            .bind(id)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        return Err(DomainError::precondition(
            "email_verification.expired",
            "o link expirou — peça outro",
        )
        .into());
    }
    // O token prova o endereço de QUANDO foi pedido. Se a conta tiver outro
    // email agora, provar o novo com ele seria provar o que ninguém recebeu.
    let verified_at: Option<DateTime<Utc>> = sqlx::query_scalar(
        "UPDATE users SET email_verified_at = now()
          WHERE id = $1 AND email = $2 RETURNING email_verified_at",
    )
    .bind(user_id)
    .bind(&email)
    .fetch_optional(&mut *tx)
    .await?;
    let Some(verified_at) = verified_at else {
        sqlx::query("UPDATE email_verifications SET status = 'revoked' WHERE id = $1")
            .bind(id)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        return Err(DomainError::conflict(
            "email_verification.email_changed",
            "a conta mudou de email depois deste pedido — peça outro",
        )
        .into());
    };
    sqlx::query("UPDATE email_verifications SET status = 'used', used_at = now() WHERE id = $1")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;

    crate::audit::log(&state.db, None, user_id, "user.email_verified", &email).await;
    Ok(Json(AcceptedEmailVerification { email, verified_at }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn o_token_vai_no_fragmento_e_nunca_na_query() {
        let l = link("https://meet.exemplo.ao", "dlxv_abc");
        assert_eq!(
            l,
            "https://meet.exemplo.ao/#/verificar-email?token=dlxv_abc"
        );
        // Tudo o que vem depois do `#` fica no browser: o `?` é do fragmento.
        let (antes, _) = l.split_once('#').unwrap();
        assert!(
            !antes.contains("dlxv_"),
            "o token não pode ir antes do #: {l}"
        );
    }

    #[test]
    fn a_mensagem_leva_o_link_nas_duas_linguas() {
        for locale in ["pt", "en", "fr"] {
            let (_, corpo) = mensagem(locale, "a@b.ao", "https://x/#/verificar-email?token=t");
            assert!(
                corpo.contains("https://x/#/verificar-email?token=t"),
                "{locale}: {corpo}"
            );
        }
    }
}
