//! Reposição de password pedida pela própria pessoa, por email (E3; migração
//! 0112).
//!
//! **Só para endereços PROVADOS** (`users.email_verified_at`, D7). A reposição
//! por email dá a conta a quem controla o endereço: um endereço afirmado e mal
//! escrito era uma porta para uma conta que já tem dados.
//!
//! **A resposta é a mesma para toda a gente, e sai logo.** Conta inexistente,
//! endereço por provar, conta do Odoo, pedido repetido no mesmo minuto, ou
//! email enfileirado: tudo `202` com o mesmo corpo. E o trabalho da conta corre
//! numa tarefa à parte, DEPOIS da resposta — a conta existente faz escritas e
//! enfileira um email, a inexistente não faz nada, e um `202` que demorasse mais
//! num caso do que no outro dizia quem tem conta só pelo relógio.
//!
//! **O token vive na mesma tabela da reposição por administrador** e é usado
//! pela mesma rota (`POST /api/password-resets/accept`). Pedir uma revoga a
//! pendente do outro canal: dois tokens válidos para a mesma conta eram duas
//! portas.

use std::sync::Arc;

use axum::{extract::State, http::StatusCode, Json};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use delonix_meet_core::DomainError;

use crate::{error::ApiError, AppState};

/// 1 h: um link que viaja por email vale menos tempo do que um token que o
/// administrador entrega à mão (24 h) — é o canal que mais fica esquecido numa
/// caixa de correio.
const VALIDADE_MINUTOS: i64 = 60;

/// Entre dois pedidos para a mesma conta. Sem isto, quem sabe um endereço
/// enchia a caixa de correio do dono e gastava o relay.
const INTERVALO_SEGUNDOS: i64 = 60;

#[derive(utoipa::OpenApi)]
#[openapi(
    paths(request),
    components(schemas(PasswordRecoveryReq, PasswordRecoveryAccepted))
)]
pub struct ApiDoc;

#[derive(Deserialize, utoipa::ToSchema)]
pub struct PasswordRecoveryReq {
    pub email: String,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct PasswordRecoveryAccepted {
    /// Sempre `accepted`, exista a conta ou não.
    pub status: &'static str,
}

/// O link que vai no email: o token no FRAGMENTO, que o browser não envia a
/// servidor nenhum.
fn link(public_url: &str, token: &str) -> String {
    format!("{public_url}/#/repor-password?token={token}")
}

fn mensagem(locale: &str, link: &str) -> (&'static str, String) {
    if locale.starts_with("en") {
        (
            "Reset your password",
            format!(
                "Someone (hopefully you) asked to reset the password of this Delonix Meet \
                 account.\n\nOpen this link within {VALIDADE_MINUTOS} minutes to choose a new \
                 one:\n\n{link}\n\nIf it wasn't you, ignore this message: your password stays \
                 the same until the link is used. Using it signs every session out.\n"
            ),
        )
    } else {
        (
            "Repor a palavra-passe",
            format!(
                "Alguém (esperamos que tenha sido você) pediu para repor a palavra-passe \
                 desta conta do Delonix Meet.\n\nAbra este link nos próximos \
                 {VALIDADE_MINUTOS} minutos para escolher uma nova:\n\n{link}\n\nSe não foi \
                 você, ignore esta mensagem: a palavra-passe não muda enquanto o link não for \
                 usado. Usá-lo termina todas as sessões.\n"
            ),
        )
    }
}

/// Pede um email de reposição. **Pública**: quem a usa está fora da conta.
#[utoipa::path(
    post, path = "/api/password-resets/request", tag = "directory",
    request_body = PasswordRecoveryReq,
    responses(
        (status = 202, body = PasswordRecoveryAccepted, description = "Sempre o mesmo, exista a conta ou não: se o endereço for de uma conta com o email provado, sai um link."),
        (status = 422, body = crate::openapi::ErrorBody, description = "`mail.disabled` ou `mail.public_url_missing` — estado do SERVIDOR, igual para qualquer endereço"),
        (status = 429, body = crate::openapi::ErrorBody, description = "limite por IP"),
    )
)]
pub async fn request(
    State(state): State<Arc<AppState>>,
    Json(req): Json<PasswordRecoveryReq>,
) -> Result<(StatusCode, Json<PasswordRecoveryAccepted>), ApiError> {
    // As pré-condições são do servidor, não da conta: dizê-las não revela quem
    // tem conta, e esconder que o correio está desligado deixava a pessoa à
    // espera de um email que nunca vem.
    if !crate::mail::enabled(&state) {
        return Err(DomainError::precondition(
            "mail.disabled",
            "o correio não está configurado neste servidor",
        )
        .into());
    }
    let Some(public_url) = state.config.public_url.clone() else {
        return Err(DomainError::precondition(
            "mail.public_url_missing",
            "o servidor não sabe o seu endereço público (PUBLIC_URL) e não escreve links",
        )
        .into());
    };
    let email = delonix_meet_domain::identity::validation::normalize_email(&req.email);
    let st = state.clone();
    tokio::spawn(async move {
        if let Err(e) = emitir(&st, &email, &public_url).await {
            tracing::warn!(error = ?e, "reposição por email: o pedido não foi tratado");
        }
    });
    Ok((
        StatusCode::ACCEPTED,
        Json(PasswordRecoveryAccepted { status: "accepted" }),
    ))
}

/// O trabalho da conta, fora do tempo da resposta. Devolve `Ok(false)` quando
/// não há nada a enviar — e isso não é um erro, é o caso normal de um endereço
/// que não é de ninguém.
async fn emitir(state: &AppState, email: &str, public_url: &str) -> Result<bool, ApiError> {
    let conta: Option<(Uuid, String, Option<DateTime<Utc>>)> = sqlx::query_as(
        "SELECT id, COALESCE(locale, 'pt'), email_verified_at FROM users WHERE email = $1",
    )
    .bind(email)
    .fetch_optional(&state.db)
    .await?;
    let Some((user_id, locale, Some(_))) = conta else {
        // Sem conta, ou com o endereço por provar: nada sai.
        return Ok(false);
    };
    // A password de uma conta do Odoo é a do Odoo: um hash local novo seria
    // sobrescrito no próximo login (a mesma razão da reposição por administrador).
    if crate::account::is_odoo_managed(state, user_id).await? {
        return Ok(false);
    }

    let token = delonix_meet_core::crypto::prefixed_token("dlxr_");
    let prefix: String = token.chars().take(12).collect();
    let expires_at = Utc::now() + chrono::Duration::minutes(VALIDADE_MINUTOS);

    let mut tx = state.db.begin().await?;
    // O intervalo lê-se com a linha da pessoa bloqueada: dois pedidos
    // simultâneos para o mesmo endereço não passam os dois.
    sqlx::query("SELECT 1 FROM users WHERE id = $1 FOR UPDATE")
        .bind(user_id)
        .execute(&mut *tx)
        .await?;
    let ultimo: Option<DateTime<Utc>> = sqlx::query_scalar(
        "SELECT max(created_at) FROM password_resets WHERE user_id = $1 AND channel = 'email'",
    )
    .bind(user_id)
    .fetch_one(&mut *tx)
    .await?;
    if ultimo.is_some_and(|u| (Utc::now() - u).num_seconds() < INTERVALO_SEGUNDOS) {
        return Ok(false);
    }
    // Uma pendente por pessoa, em QUALQUER canal (índice da 0106).
    sqlx::query(
        "UPDATE password_resets SET status = 'revoked' WHERE user_id = $1 AND status = 'pending'",
    )
    .bind(user_id)
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "INSERT INTO password_resets (user_id, org_id, token_hash, token_prefix, expires_at, channel)
         VALUES ($1, NULL, $2, $3, $4, 'email')",
    )
    .bind(user_id)
    .bind(crate::directory::reset_token_hash(&token))
    .bind(&prefix)
    .bind(expires_at)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;

    let (subject, body) = mensagem(&locale, &link(public_url, &token));
    crate::mail::enqueue(
        state,
        crate::mail::Outgoing {
            org_id: None,
            purpose: crate::mail::Purpose::PasswordReset,
            to: email,
            subject,
            body: &body,
        },
    )
    .await?;
    crate::audit::log(
        &state.db,
        None,
        user_id,
        "user.password_reset_requested",
        &prefix,
    )
    .await;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn o_token_vai_no_fragmento() {
        let l = link("https://meet.exemplo.ao", "dlxr_abc");
        assert_eq!(l, "https://meet.exemplo.ao/#/repor-password?token=dlxr_abc");
        assert!(!l.split_once('#').unwrap().0.contains("dlxr_"));
    }

    #[test]
    fn a_mensagem_leva_o_link() {
        for locale in ["pt", "en", "zh"] {
            let (_, corpo) = mensagem(locale, "https://x/#/repor-password?token=t");
            assert!(
                corpo.contains("https://x/#/repor-password?token=t"),
                "{locale}"
            );
        }
    }
}
