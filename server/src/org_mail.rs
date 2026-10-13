//! O SMTP da própria organização: ler, gravar, desligar e **verificar**.
//!
//! **PORQUE EXISTE.** O ADR-0025 decidiu o correio em dois tempos — relay do
//! operador primeiro, SMTP por organização depois — e nomeou a condição para o
//! segundo, que não era o SMTP: era **um guarda de saída que valide
//! `host:porta` antes de abrir a ligação**. Esse guarda passou a existir
//! (`net_guard::Outbound::resolve_tenant_target`), e por isso isto já se pode
//! oferecer.
//!
//! **O que estas rotas guardam:**
//!
//! - o host é escrito por um cliente, logo passa pela guarda **ao gravar**
//!   (`check_tenant_host_port`, erro com razão) **e ao ligar** (o `mail.rs`
//!   resolve e liga-se ao endereço aprovado, nunca ao nome);
//! - a password é selada em repouso com o AAD preso à organização, e **nunca
//!   sai numa resposta**: a leitura devolve só `password_configured`;
//! - desligar o STARTTLS é possível mas **tem de ser dito por escrito**
//!   (`allow_plaintext: true`), porque sem TLS a password do cliente viaja à
//!   vista;
//! - só administradores da organização, e cada organização só vê a sua.
//!
//! Sem linha, ou com `enabled = false`, o correio dessa organização sai pelo
//! relay do operador: isto ACRESCENTA um caminho e não substitui nenhum.

use std::sync::Arc;

use axum::extract::{Path, State};
use axum::Json;
use chrono::{DateTime, Utc};
use delonix_meet_core::DomainError;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{auth::AuthUser, error::ApiError, AppState};

/// A configuração, como a API a conta. **Sem password.**
#[derive(Serialize, utoipa::ToSchema)]
pub struct OrgMailSettings {
    pub host: String,
    pub port: u16,
    pub username: String,
    /// Há password gravada? O valor dela **nunca** sai daqui.
    pub password_configured: bool,
    pub starttls: bool,
    pub from_address: String,
    /// `false` = guardada mas a dormir; o correio sai pelo relay do operador.
    pub enabled: bool,
    pub updated_by: Option<Uuid>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct PutOrgMailSettingsReq {
    pub host: String,
    pub port: u16,
    #[serde(default)]
    pub username: Option<String>,
    /// Omissa numa alteração = **mantém** a que está. `""` apaga-a.
    #[serde(default)]
    pub password: Option<String>,
    pub from_address: String,
    /// Por omissão `true`. Pôr `false` exige `allow_plaintext: true`.
    #[serde(default = "sim")]
    pub starttls: bool,
    /// A confirmação por escrito de que se quer uma ligação em claro. Sem ela,
    /// `starttls: false` é recusado — a password do cliente viajaria à vista e
    /// isso não pode ser um descuido de um campo.
    #[serde(default)]
    pub allow_plaintext: bool,
    #[serde(default = "sim")]
    pub enabled: bool,
}

fn sim() -> bool {
    true
}

/// O resultado de uma verificação: ligou-se ao relay e falou SMTP com ele
/// (autenticação incluída), **sem mandar mensagem nenhuma**.
#[derive(Serialize, utoipa::ToSchema)]
pub struct MailRelayCheck {
    pub ok: bool,
    /// A razão, quando não deu. Vem do relay ou da guarda de saída.
    pub error: Option<String>,
}

const COLUNAS: &str = "host, port, username, (password_sealed <> '') AS password_configured, \
                       starttls, from_address, enabled, updated_by, updated_at";

#[derive(sqlx::FromRow)]
struct Row {
    host: String,
    port: i32,
    username: String,
    password_configured: bool,
    starttls: bool,
    from_address: String,
    enabled: bool,
    updated_by: Option<Uuid>,
    updated_at: DateTime<Utc>,
}

impl From<Row> for OrgMailSettings {
    fn from(r: Row) -> Self {
        OrgMailSettings {
            host: r.host,
            port: r.port.clamp(1, u16::MAX as i32) as u16,
            username: r.username,
            password_configured: r.password_configured,
            starttls: r.starttls,
            from_address: r.from_address,
            enabled: r.enabled,
            updated_by: r.updated_by,
            updated_at: r.updated_at,
        }
    }
}

async fn carregar(state: &AppState, org_id: Uuid) -> Result<Option<OrgMailSettings>, ApiError> {
    let row: Option<Row> = sqlx::query_as(&format!(
        "SELECT {COLUNAS} FROM org_mail_settings WHERE org_id = $1"
    ))
    .bind(org_id)
    .fetch_optional(&state.db)
    .await?;
    Ok(row.map(Into::into))
}

/// O SMTP desta organização. `404` quando ela não configurou nenhum — e aí o
/// correio dela sai pelo relay do operador.
#[utoipa::path(
    get, path = "/api/orgs/{org_id}/mail-settings", tag = "org",
    security(("session" = [])),
    params(("org_id" = Uuid, Path)),
    responses(
        (status = 200, body = OrgMailSettings, description = "A password nunca vem; só `password_configured`."),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 403, body = crate::openapi::ErrorBody, description = "não é administrador desta organização"),
        (status = 404, body = crate::openapi::ErrorBody, description = "a organização não configurou SMTP próprio"),
    )
)]
pub async fn get_settings(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(org_id): Path<Uuid>,
) -> Result<Json<OrgMailSettings>, ApiError> {
    crate::org::require_admin_pub(&state, org_id, auth.user_id).await?;
    carregar(&state, org_id).await?.map(Json).ok_or_else(|| {
        DomainError::not_found("org_mail_settings.not_found")
            .with_message("esta organização não configurou SMTP próprio")
            .into()
    })
}

/// Grava (ou substitui) o SMTP desta organização.
#[utoipa::path(
    put, path = "/api/orgs/{org_id}/mail-settings", tag = "org",
    security(("session" = [])),
    params(("org_id" = Uuid, Path)),
    request_body = PutOrgMailSettingsReq,
    responses(
        (status = 200, body = OrgMailSettings),
        (status = 400, body = crate::openapi::ErrorBody, description = "`org_mail_settings.invalid_from`, `org_mail_settings.plaintext_not_confirmed`, `org_mail_settings.host_refused` (a guarda de saída recusou o destino)"),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 403, body = crate::openapi::ErrorBody),
        (status = 422, body = crate::openapi::ErrorBody, description = "`secrets.encryption_unconfigured` — sem `DATA_ENCRYPTION_KEYS` não se guarda uma password"),
    )
)]
pub async fn put_settings(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(org_id): Path<Uuid>,
    Json(req): Json<PutOrgMailSettingsReq>,
) -> Result<Json<OrgMailSettings>, ApiError> {
    crate::org::require_admin_pub(&state, org_id, auth.user_id).await?;

    // Uma ligação em claro não pode ser um descuido de um campo.
    if !req.starttls && !req.allow_plaintext {
        return Err(DomainError::invalid(
            "org_mail_settings.plaintext_not_confirmed",
            "sem STARTTLS a password do relay viaja à vista: confirme com `allow_plaintext`",
        )
        .with_field("starttls", "true, ou allow_plaintext: true")
        .into());
    }
    // O remetente tem de ser um endereço: um relay sério recusa o resto, e o
    // erro dele chegaria tarde e sem razão clara.
    let from = req.from_address.trim();
    if from.parse::<lettre::message::Mailbox>().is_err() {
        return Err(DomainError::invalid(
            "org_mail_settings.invalid_from",
            "o remetente tem de ser um endereço (ex.: Nome <conta@dominio.ao>)",
        )
        .with_field("from_address", "endereço de correio")
        .into());
    }
    // O host é escrito por um cliente: a guarda de saída julga-o AQUI, para o
    // erro ter razão, e julga-o OUTRA VEZ ao ligar (é lá que o rebinding se
    // fecha).
    state
        .outbound
        .check_tenant_host_port(req.host.trim(), req.port)
        .await
        .map_err(|e| -> ApiError {
            // Código estável e próprio, como os troncos SIP fazem
            // (`telephony.trunk_host_refused`): quem configura precisa de
            // distinguir «o host foi recusado pela guarda» de qualquer outro
            // erro de validação, e o `invalid_argument` genérico não o diz.
            DomainError::invalid("org_mail_settings.host_refused", e.to_string())
                .with_field("host", "destino interno ou inválido")
                .into()
        })?;

    let antiga_selada: Option<String> =
        sqlx::query_scalar("SELECT password_sealed FROM org_mail_settings WHERE org_id = $1")
            .bind(org_id)
            .fetch_optional(&state.db)
            .await?;
    let selada = match req.password.as_deref() {
        // Omissa: mantém o que está (ou nada, se não havia).
        None => antiga_selada.unwrap_or_default(),
        Some("") => String::new(),
        Some(p) => crate::secrets_at_rest::seal(&state.config, p, &crate::mail::aad(org_id))?,
    };

    sqlx::query(
        "INSERT INTO org_mail_settings
             (org_id, host, port, username, password_sealed, starttls, from_address, enabled,
              updated_by, updated_at)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, now())
         ON CONFLICT (org_id) DO UPDATE SET
             host = $2, port = $3, username = $4, password_sealed = $5, starttls = $6,
             from_address = $7, enabled = $8, updated_by = $9, updated_at = now()",
    )
    .bind(org_id)
    .bind(req.host.trim())
    .bind(req.port as i32)
    .bind(req.username.as_deref().unwrap_or("").trim())
    .bind(&selada)
    .bind(req.starttls)
    .bind(from)
    .bind(req.enabled)
    .bind(auth.user_id)
    .execute(&state.db)
    .await?;

    crate::audit::log(
        &state.db,
        Some(org_id),
        auth.user_id,
        "org.mail_settings_updated",
        // O host e a porta ficam no registo; a password, nunca.
        &format!("{}:{}", req.host.trim(), req.port),
    )
    .await;

    carregar(&state, org_id)
        .await?
        .map(Json)
        .ok_or(ApiError::NotFound)
}

/// Apaga o SMTP próprio: o correio desta organização volta ao relay do
/// operador.
#[utoipa::path(
    delete, path = "/api/orgs/{org_id}/mail-settings", tag = "org",
    security(("session" = [])),
    params(("org_id" = Uuid, Path)),
    responses(
        (status = 204, description = "Apagado, ou já não existia."),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 403, body = crate::openapi::ErrorBody),
    )
)]
pub async fn delete_settings(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(org_id): Path<Uuid>,
) -> Result<axum::http::StatusCode, ApiError> {
    crate::org::require_admin_pub(&state, org_id, auth.user_id).await?;
    let n = sqlx::query("DELETE FROM org_mail_settings WHERE org_id = $1")
        .bind(org_id)
        .execute(&state.db)
        .await?
        .rows_affected();
    if n > 0 {
        crate::audit::log(
            &state.db,
            Some(org_id),
            auth.user_id,
            "org.mail_settings_deleted",
            "",
        )
        .await;
    }
    Ok(axum::http::StatusCode::NO_CONTENT)
}

/// Verifica o relay desta organização: liga-se, fala SMTP (autenticação
/// incluída) e **não manda mensagem nenhuma**.
///
/// Existe porque sem isto quem configura um relay só descobre que está errado
/// quando um email se perde — e o erro fica numa linha da caixa de saída que a
/// API não expõe.
///
/// A ligação passa pela MESMA guarda de saída do envio: um host interno é
/// recusado aqui também, e por isso isto não serve de sonda da rede.
#[utoipa::path(
    post, path = "/api/orgs/{org_id}/mail-settings/check", tag = "org",
    security(("session" = [])),
    params(("org_id" = Uuid, Path)),
    responses(
        (status = 200, body = MailRelayCheck, description = "`ok: false` com a razão é uma resposta, não um erro: a configuração é que está má."),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 403, body = crate::openapi::ErrorBody),
        (status = 404, body = crate::openapi::ErrorBody, description = "a organização não configurou SMTP próprio"),
    )
)]
pub async fn check_settings(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(org_id): Path<Uuid>,
) -> Result<Json<MailRelayCheck>, ApiError> {
    crate::org::require_admin_pub(&state, org_id, auth.user_id).await?;
    // De propósito ignora o `enabled`: verifica-se uma configuração a dormir
    // ANTES de a ligar, que é a ordem sensata.
    let relay = crate::mail::org_relay_any(&state, org_id)
        .await
        .ok_or_else(|| {
            ApiError::from(
                DomainError::not_found("org_mail_settings.not_found")
                    .with_message("esta organização não configurou SMTP próprio"),
            )
        })?;
    Ok(Json(match crate::mail::test_relay(&state, &relay).await {
        Ok(()) => MailRelayCheck {
            ok: true,
            error: None,
        },
        Err(e) => MailRelayCheck {
            ok: false,
            error: Some(e),
        },
    }))
}
