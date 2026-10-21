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

use axum::extract::{Path, Query, State};
use axum::Json;
use chrono::{DateTime, Utc};
use delonix_meet_core::{
    page::{Page, PageRequest},
    DomainError,
};
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

/* ------------------------------------------------------------------ */
/* A caixa de saída, vista por quem administra a organização           */
/* ------------------------------------------------------------------ */

/// Uma mensagem da caixa de saída. **SEM O CORPO**, e isso é deliberado:
///
/// o corpo de um convite leva o **token do convite**, e o de uma notificação
/// leva o título da reunião. Devolver corpos numa listagem era espalhar
/// credenciais por uma rota de diagnóstico que ninguém precisa de ler assim —
/// quem quer o token de um convite pede o convite, onde a autorização é a
/// própria.
///
/// Pela mesma razão não há rota para UMA mensagem com o corpo: não houve
/// pergunta que a justificasse, e acrescentá-la depois é mais fácil do que
/// retirar uma que já exista.
#[derive(Serialize, utoipa::ToSchema, sqlx::FromRow)]
pub struct MailMessage {
    pub id: Uuid,
    /// `invitation` | `notification` | … (ver `mail::Purpose`).
    pub purpose: String,
    pub to_address: String,
    pub subject: String,
    /// `pending` | `sending` | `sent` | `failed`.
    pub status: String,
    pub attempt: i32,
    /// A razão da última falha, quando houve — é para isto que esta rota serve.
    pub error: Option<String>,
    /// Quando a repetição está agendada. `null` = não há mais.
    pub retry_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub sent_at: Option<DateTime<Utc>>,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct MailMessagePage {
    pub items: Vec<MailMessage>,
    pub next_page_token: Option<String>,
}

#[derive(Deserialize, utoipa::IntoParams)]
pub struct MailListQuery {
    /// 1–100, omissão 50.
    pub page_size: Option<u32>,
    pub page_token: Option<String>,
    /// Só neste estado: `pending` | `sending` | `sent` | `failed`.
    pub status: Option<String>,
    /// Só deste propósito: `invitation` | `notification` | …
    pub purpose: Option<String>,
}

#[derive(Serialize, Deserialize)]
struct MailCursor {
    at: DateTime<Utc>,
    id: Uuid,
}

const COLUNAS_MSG: &str = "id, purpose, to_address, subject, status, attempt, error, retry_at, \
                           created_at, sent_at";

/// A caixa de saída desta organização, mais recentes primeiro.
///
/// **Para que serve:** até aqui, uma mensagem que falhava deixava o erro numa
/// linha que a API não expunha — quem configurou um relay descobria o problema
/// por um email que nunca chegou. A verificação do relay
/// (`POST …/mail-settings/check`) diz se a ligação está boa; isto diz o que
/// aconteceu a cada mensagem.
///
/// **O que NÃO aparece aqui:** o correio que não é da organização. A reposição
/// de password e a prova de endereço enfileiram com `org_id` nulo de propósito
/// (são da pessoa, não da empresa), e por isso nunca entram nesta listagem —
/// nem o seu token de uso único, que vai no corpo.
#[utoipa::path(
    get, path = "/api/orgs/{org_id}/mail-messages", tag = "org",
    security(("session" = [])),
    params(("org_id" = Uuid, Path), MailListQuery),
    responses(
        (status = 200, body = MailMessagePage, description = "Sem corpos: o do convite leva o token."),
        (status = 400, body = crate::openapi::ErrorBody, description = "`mail_messages.unknown_status` ou `mail_messages.unknown_purpose`"),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 403, body = crate::openapi::ErrorBody, description = "não é administrador desta organização"),
    )
)]
pub async fn list_messages(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(org_id): Path<Uuid>,
    Query(q): Query<MailListQuery>,
) -> Result<Json<MailMessagePage>, ApiError> {
    crate::org::require_admin_pub(&state, org_id, auth.user_id).await?;

    // Catálogos fechados: um filtro escrito à toa devolveria uma página vazia
    // e pareceria «não há mensagens», que é a resposta mais enganadora de
    // todas numa rota de diagnóstico.
    const ESTADOS: [&str; 4] = ["pending", "sending", "sent", "failed"];
    if let Some(st) = q.status.as_deref() {
        if !ESTADOS.contains(&st) {
            return Err(DomainError::invalid(
                "mail_messages.unknown_status",
                format!("estado desconhecido: {st}"),
            )
            .with_field("status", ESTADOS.join(" | "))
            .into());
        }
    }
    if let Some(pp) = q.purpose.as_deref() {
        if crate::mail::Purpose::parse(pp).is_none() {
            return Err(DomainError::invalid(
                "mail_messages.unknown_purpose",
                format!("propósito desconhecido: {pp}"),
            )
            .with_field("purpose", crate::mail::Purpose::TODOS.join(" | "))
            .into());
        }
    }

    let page = PageRequest {
        page_size: q.page_size,
        page_token: q.page_token,
    };
    let size = page.size();
    let cursor: Option<MailCursor> = page.cursor()?;
    let rows: Vec<MailMessage> = sqlx::query_as(&format!(
        "SELECT {COLUNAS_MSG} FROM mail_messages
          WHERE org_id = $1
            AND ($2::text IS NULL OR status = $2)
            AND ($3::text IS NULL OR purpose = $3)
            AND ($4::timestamptz IS NULL OR (created_at, id) < ($4, $5))
          ORDER BY created_at DESC, id DESC
          LIMIT $6"
    ))
    .bind(org_id)
    .bind(q.status.as_deref())
    .bind(q.purpose.as_deref())
    .bind(cursor.as_ref().map(|c| c.at))
    .bind(cursor.as_ref().map(|c| c.id).unwrap_or_default())
    .bind(size as i64 + 1)
    .fetch_all(&state.db)
    .await?;
    let p = Page::from_overfetch(rows, size, |m: &MailMessage| MailCursor {
        at: m.created_at,
        id: m.id,
    });
    Ok(Json(MailMessagePage {
        items: p.items,
        next_page_token: p.next_page_token,
    }))
}
