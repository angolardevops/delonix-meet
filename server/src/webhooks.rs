//! Webhooks de saída por organização (Slack / Teams / Mattermost / genérico).
//!
//! Eventos disparados: ver `KNOWN_EVENTS` (`meeting.created`,
//! `meeting.started`, `meeting.mom_ready`, `recording.ready`).
//! Os alvos Slack/Mattermost recebem `{ "text": "..." }`; Teams recebe um
//! MessageCard; o alvo `generic` recebe o JSON estruturado com a assinatura
//! `X-Delonix-Signature: sha256=<hmac>` (chave = `secret`) para verificação.
//!
//! **O segredo em repouso (S5).** `org_webhooks.secret` guarda-se cifrado
//! (`secrets_at_rest`, aad `org_webhooks.secret:<id>`) e abre-se só no envio.
//! Criar um webhook COM segredo sem `DATA_ENCRYPTION_KEYS` é `422`; SEM segredo
//! continua a funcionar sem chaves (não há nada a proteger, e Slack/Teams/
//! Mattermost nunca o usam). Um segredo que não abre não envia a entrega sem
//! assinatura: a entrega fica `failed`.
//!
//! O envio é best-effort e assíncrono (não bloqueia o pedido do utilizador).
//!
//! **Registo de entregas (G7).** Cada envio a um webhook fica numa linha de
//! `webhook_deliveries` (migração 0043): inserida `pending` com o corpo exacto
//! ANTES do envio e fechada `succeeded`/`failed` com o código HTTP, o tempo e
//! um erro curto e limpo DEPOIS. Nem o segredo nem a assinatura se guardam.
//! As regras puras (estados, limpeza do erro, ritmo de reenvio, retenção) estão
//! em `delonix_meet_domain::integration::webhook_delivery`.
//!
//! Rotas novas (contrato do ADR-0004 §4, só administradores da org):
//! - `GET  /api/orgs/{org_id}/webhooks/{hook_id}`                               um webhook
//! - `GET  /api/orgs/{org_id}/webhooks/{hook_id}/deliveries`                    lista paginada, mais recentes primeiro, `?status=`
//! - `GET  /api/orgs/{org_id}/webhooks/{hook_id}/deliveries/{delivery_id}`      uma entrega, com o payload
//! - `POST /api/orgs/{org_id}/webhooks/{hook_id}/deliveries/{delivery_id}/redeliver`  método personalizado: `202` + `Location`
//!
//! Um id de outra org e um id inexistente dão a MESMA resposta (`404`).

use axum::{
    extract::{Path, Query, State},
    http::{header::LOCATION, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use chrono::{DateTime, Utc};
use delonix_meet_core::{
    page::{Page, PageRequest},
    DomainError,
};
use delonix_meet_domain::integration::webhook_delivery as rules;
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use std::sync::Arc;
use uuid::Uuid;

use crate::{auth::AuthUser, error::ApiError, secrets_at_rest, AppState};

type HmacSha256 = Hmac<Sha256>;

/// Eventos que um webhook pode subscrever. Um nome fora desta lista é um typo
/// que silenciaria o hook sem erro nenhum — recusa-se na criação.
pub const KNOWN_EVENTS: &[&str] = &[
    "meeting.created",
    "meeting.started",
    "meeting.mom_ready",
    "recording.ready",
];

/// Subscrição por omissão. `meeting.mom_ready` TEM de estar aqui: é o evento
/// que diz a um ERP integrado (Odoo) que a ata AI ficou pronta, e um webhook
/// criado sem `events` explícitos nunca o receberia.
pub const DEFAULT_EVENTS: &str =
    "meeting.created,meeting.started,meeting.mom_ready,recording.ready";

#[derive(Debug, Clone, Serialize, sqlx::FromRow, utoipa::ToSchema)]
pub struct Webhook {
    pub id: Uuid,
    pub org_id: Uuid,
    /// `slack` | `teams` | `mattermost` | `generic`.
    pub kind: String,
    pub url: String,
    /// Como está GUARDADO (cifrado, ou herdado em claro) — nunca serializado.
    #[serde(skip_serializing)]
    #[schema(ignore)]
    pub secret: String,
    /// Eventos subscritos, separados por vírgula (ver `KNOWN_EVENTS`).
    pub events: String,
    pub active: bool,
}

/// Lista de colunas que cobre todos os campos de `Webhook` — usar sempre que
/// se hidrata `Webhook`. Estava copiada à mão em três sítios (ver ADR-0004,
/// mesmo padrão de risco de `meetings::MEETING_COLUMNS`/`rooms::ROOM_COLUMNS`).
const WEBHOOK_COLUMNS: &str = "id, org_id, kind, url, secret, events, active";

/// `aad` do segredo de um webhook.
fn secret_aad(id: Uuid) -> String {
    secrets_at_rest::aad("org_webhooks", "secret", id)
}

/// Um evento de webhook: nome + payload estruturado (o corpo do `generic`).
pub struct Event {
    pub name: &'static str,
    pub title: String,
    pub text: String,
    pub payload: serde_json::Value,
}

/// Dispara um evento para todos os webhooks ativos de uma organização que o
/// tenham subscrito. Corre em background — falhas são registadas (no log e em
/// `webhook_deliveries`), não propagadas.
pub fn fire(state: Arc<AppState>, org_id: Uuid, event: Event) {
    tokio::spawn(async move {
        let hooks: Vec<Webhook> = match sqlx::query_as(&format!(
            "SELECT {WEBHOOK_COLUMNS} FROM org_webhooks WHERE org_id = $1 AND active = TRUE"
        ))
        .bind(org_id)
        .fetch_all(&state.db)
        .await
        {
            Ok(h) => h,
            Err(e) => {
                tracing::warn!(error = %e, "webhook query failed");
                return;
            }
        };
        for hook in hooks {
            if !hook.events.split(',').any(|e| e.trim() == event.name) {
                continue;
            }
            let body = body_for(&hook, &event);
            // O registo não é condição do envio: se a base falhar aqui, a
            // entrega segue sem linha (e fica o aviso no log).
            let (delivery_id, body) =
                match record_pending(&state.db, &hook, event.name, &body, 1, None).await {
                    Ok(d) => (Some(d.id), d.payload),
                    Err(e) => {
                        tracing::warn!(hook = %hook.id, error = %e, "registo da entrega falhou");
                        (None, body)
                    }
                };
            attempt(&state, &hook, event.name, &body, delivery_id).await;
        }
    });
}

/// O corpo JSON a enviar a um webhook, conforme o tipo do destino.
fn body_for(hook: &Webhook, event: &Event) -> serde_json::Value {
    match hook.kind.as_str() {
        "slack" | "mattermost" => serde_json::json!({
            "text": format!("*{}*\n{}", event.title, event.text)
        }),
        "teams" => serde_json::json!({
            "@type": "MessageCard",
            "@context": "http://schema.org/extensions",
            "summary": event.title,
            "themeColor": "C8201D",
            "title": event.title,
            "text": event.text,
        }),
        // Genérico: JSON estruturado (assinado no envio).
        _ => serde_json::json!({
            "event": event.name,
            "title": event.title,
            "text": event.text,
            "data": event.payload,
        }),
    }
}

/// Linha acabada de inserir: o id e o payload TAL COMO FICOU GUARDADO.
struct PendingDelivery {
    id: Uuid,
    payload: serde_json::Value,
}

/// Insere a entrega `pending` com o corpo exacto. Devolve o payload relido da
/// base: o primeiro envio e qualquer reenvio serializam o MESMO valor, e por
/// isso mandam os mesmos bytes (a ordem das chaves é a do JSONB, não a de quem
/// construiu o JSON).
async fn record_pending(
    db: &sqlx::PgPool,
    hook: &Webhook,
    event: &str,
    body: &serde_json::Value,
    attempt: i32,
    redelivery_of: Option<Uuid>,
) -> Result<PendingDelivery, sqlx::Error> {
    let (id, payload): (Uuid, String) = sqlx::query_as(
        "INSERT INTO webhook_deliveries (org_id, webhook_id, event, payload, attempt, redelivery_of)
         VALUES ($1, $2, $3, $4::jsonb, $5, $6)
         RETURNING id, payload::text",
    )
    .bind(hook.org_id)
    .bind(hook.id)
    .bind(event)
    .bind(body.to_string())
    .bind(attempt)
    .bind(redelivery_of)
    .fetch_one(db)
    .await?;
    Ok(PendingDelivery {
        id,
        payload: serde_json::from_str(&payload).unwrap_or_else(|_| body.clone()),
    })
}

/// Uma tentativa de entrega: revalida o destino (DNS-rebinding), envia e fecha
/// a linha da entrega com o resultado.
async fn attempt(
    state: &AppState,
    hook: &Webhook,
    event: &str,
    body: &serde_json::Value,
    delivery_id: Option<Uuid>,
) {
    let started = std::time::Instant::now();
    let outcome: Result<u16, String> = match state.outbound.check_tenant_url(&hook.url).await {
        Err(e) => {
            tracing::warn!(hook = %hook.id, error = %e, "webhook destino bloqueado (SSRF)");
            Err(format!("destino bloqueado: {e}"))
        }
        // Um segredo que não abre não se troca por um envio sem assinatura:
        // o receptor aceitaria (ou recusaria) sem saber porquê.
        Ok(_) => {
            match secrets_at_rest::open(&state.config, &hook.secret, &secret_aad(hook.id)) {
                Err(_) => Err("o segredo do webhook não abre neste servidor".to_string()),
                Ok(secret) => send(
                    state.outbound.tenant(),
                    hook,
                    &secret,
                    event,
                    body,
                    delivery_id,
                )
                .await
                // `without_url`: o URL de um webhook do Slack/Teams é a credencial.
                .map_err(|e| e.without_url().to_string()),
            }
        }
    };
    let elapsed_ms = i32::try_from(started.elapsed().as_millis()).unwrap_or(i32::MAX);
    let (status, response_status, response_ms, error) = match outcome {
        Ok(code) => {
            let status = rules::status_for_http(code);
            let error = (status == rules::DeliveryStatus::Failed).then(|| {
                tracing::warn!(hook = %hook.id, code, "webhook delivery failed (HTTP)");
                rules::sanitize_error(&rules::http_status_error(code))
            });
            (status, Some(i32::from(code)), Some(elapsed_ms), error)
        }
        Err(e) => {
            tracing::warn!(hook = %hook.id, error = %e, "webhook delivery failed");
            (
                rules::DeliveryStatus::Failed,
                None,
                None,
                Some(rules::sanitize_error(&e)),
            )
        }
    };
    let Some(id) = delivery_id else { return };
    debug_assert!(rules::DeliveryStatus::Pending.can_transition_to(status));
    // `status = 'pending'` na condição: uma entrega só se fecha uma vez (se o
    // varredor já a deu por abandonada, o resultado tardio não a reescreve).
    if let Err(e) = sqlx::query(
        "UPDATE webhook_deliveries
            SET status = $2, response_status = $3, response_ms = $4, error = $5, delivered_at = now()
          WHERE id = $1 AND status = 'pending'",
    )
    .bind(id)
    .bind(status.as_str())
    .bind(response_status)
    .bind(response_ms)
    .bind(error)
    .execute(&state.db)
    .await
    {
        tracing::warn!(delivery = %id, error = %e, "fecho do registo da entrega falhou");
    }
}

/// Envia o corpo. Devolve o código HTTP da resposta (qualquer que seja); um
/// erro é só falha de transporte (ligação, tempo-limite).
async fn send(
    client: &reqwest::Client,
    hook: &Webhook,
    secret: &str,
    event: &str,
    body: &serde_json::Value,
    delivery_id: Option<Uuid>,
) -> Result<u16, reqwest::Error> {
    let raw = serde_json::to_vec(body).unwrap_or_default();
    let mut rb = client
        .post(&hook.url)
        .header(reqwest::header::CONTENT_TYPE, "application/json");
    if !matches!(hook.kind.as_str(), "slack" | "mattermost" | "teams") {
        rb = rb.header("X-Delonix-Event", event);
        // O id da entrega deixa o receptor reconhecer um reenvio (cada
        // tentativa tem o seu) sem comparar corpos.
        if let Some(id) = delivery_id {
            rb = rb.header("X-Delonix-Delivery", id.to_string());
        }
        // Assinatura HMAC opcional, sempre sobre os bytes que seguem — com o
        // segredo ACTUAL do webhook, já decifrado (nunca guardada no registo).
        if !secret.is_empty() {
            if let Ok(mut mac) = HmacSha256::new_from_slice(secret.as_bytes()) {
                mac.update(&raw);
                let sig = hex::encode(mac.finalize().into_bytes());
                rb = rb.header("X-Delonix-Signature", format!("sha256={sig}"));
            }
        }
    }
    Ok(rb.body(raw).send().await?.status().as_u16())
}

/// Varredor do registo de entregas: dá por falhadas as `pending` abandonadas
/// (o processo morreu a meio) e apaga as mais velhas do que a retenção.
/// Devolve `(abandonadas, apagadas)`.
pub(crate) async fn sweep_deliveries(db: &sqlx::PgPool) -> Result<(u64, u64), sqlx::Error> {
    let abandoned = sqlx::query(
        "UPDATE webhook_deliveries SET status = 'failed', error = $1, delivered_at = now()
          WHERE status = 'pending' AND created_at < now() - make_interval(secs => $2)",
    )
    .bind(rules::ABANDONED_ERROR)
    .bind(rules::STALE_PENDING_SECS as f64)
    .execute(db)
    .await?
    .rows_affected();
    let deleted = sqlx::query(
        "DELETE FROM webhook_deliveries WHERE created_at < now() - make_interval(days => $1)",
    )
    .bind(rules::RETENTION_DAYS as i32)
    .execute(db)
    .await?
    .rows_affected();
    Ok((abandoned, deleted))
}

// ---------- CRUD (admin da organização) ----------

#[derive(Deserialize, utoipa::ToSchema)]
pub struct WebhookReq {
    pub kind: String,
    pub url: String,
    /// Segredo HMAC do alvo `generic`. Guarda-se cifrado e nunca é devolvido.
    /// Vazio = sem assinatura. Não vazio sem `DATA_ENCRYPTION_KEYS` → `422`.
    #[serde(default)]
    pub secret: String,
    #[serde(default)]
    pub events: Option<String>,
}

/// Documentação OpenAPI das rotas deste módulo.
#[derive(utoipa::OpenApi)]
#[openapi(
    paths(
        list,
        create,
        get_one,
        delete,
        list_deliveries,
        get_delivery,
        redeliver
    ),
    components(schemas(
        Webhook,
        WebhookReq,
        WebhookDelivery,
        WebhookDeliveryDetail,
        WebhookDeliveryPage
    ))
)]
pub struct ApiDoc;

/// Webhooks da organização. Só administradores.
#[utoipa::path(
    get, path = "/api/orgs/{org_id}/webhooks", tag = "webhooks",
    security(("session" = [])),
    params(("org_id" = Uuid, Path)),
    responses(
        (status = 200, body = Vec<Webhook>),
        (status = 403, body = crate::openapi::ErrorBody),
        (status = 404, body = crate::openapi::ErrorBody),
    )
)]
pub async fn list(
    axum::extract::State(state): axum::extract::State<Arc<AppState>>,
    auth: crate::auth::AuthUser,
    axum::extract::Path(org_id): axum::extract::Path<Uuid>,
) -> Result<axum::Json<Vec<Webhook>>, crate::error::ApiError> {
    crate::org::require_admin_pub(&state, org_id, auth.user_id).await?;
    let hooks: Vec<Webhook> = sqlx::query_as(&format!(
        "SELECT {WEBHOOK_COLUMNS} FROM org_webhooks WHERE org_id = $1 ORDER BY created_at"
    ))
    .bind(org_id)
    .fetch_all(&state.db)
    .await?;
    Ok(axum::Json(hooks))
}

/// Cria um webhook. O URL passa pela guarda anti-SSRF.
#[utoipa::path(
    post, path = "/api/orgs/{org_id}/webhooks", tag = "webhooks",
    security(("session" = [])),
    params(("org_id" = Uuid, Path)),
    request_body = WebhookReq,
    responses(
        (status = 200, body = Webhook),
        (status = 400, body = crate::openapi::ErrorBody),
        (status = 403, body = crate::openapi::ErrorBody),
        (status = 422, body = crate::openapi::ErrorBody, description = "`secret` não vazio sem DATA_ENCRYPTION_KEYS (`secrets.encryption_unconfigured`)"),
    )
)]
pub async fn create(
    axum::extract::State(state): axum::extract::State<Arc<AppState>>,
    auth: crate::auth::AuthUser,
    axum::extract::Path(org_id): axum::extract::Path<Uuid>,
    axum::Json(req): axum::Json<WebhookReq>,
) -> Result<axum::Json<Webhook>, crate::error::ApiError> {
    crate::org::require_admin_pub(&state, org_id, auth.user_id).await?;
    if !matches!(
        req.kind.as_str(),
        "slack" | "teams" | "mattermost" | "generic"
    ) {
        return Err(crate::error::ApiError::BadRequest(
            "tipo de webhook inválido".into(),
        ));
    }
    state.outbound.check_tenant_url(&req.url).await?;
    let events = req.events.unwrap_or_else(|| DEFAULT_EVENTS.into());
    // Um nome desconhecido nunca dispara — recusa-se aqui em vez de deixar o
    // admin com um webhook silenciosamente morto.
    if let Some(bad) = events
        .split(',')
        .map(str::trim)
        .filter(|e| !e.is_empty())
        .find(|e| !KNOWN_EVENTS.contains(e))
    {
        return Err(crate::error::ApiError::BadRequest(format!(
            "evento desconhecido «{bad}» — válidos: {}",
            KNOWN_EVENTS.join(", ")
        )));
    }
    // O id nasce aqui: o segredo cifra-se ligado a ESTA linha.
    let id = Uuid::new_v4();
    let secret = if req.secret.is_empty() {
        String::new()
    } else {
        secrets_at_rest::seal(&state.config, &req.secret, &secret_aad(id))?
    };
    let hook: Webhook = sqlx::query_as(&format!(
        "INSERT INTO org_webhooks (id, org_id, kind, url, secret, events, created_by)
         VALUES ($1, $2, $3, $4, $5, $6, $7)
         RETURNING {WEBHOOK_COLUMNS}"
    ))
    .bind(id)
    .bind(org_id)
    .bind(&req.kind)
    .bind(&req.url)
    .bind(&secret)
    .bind(&events)
    .bind(auth.user_id)
    .fetch_one(&state.db)
    .await?;
    crate::audit::log(
        &state.db,
        Some(org_id),
        auth.user_id,
        "webhook.created",
        &hook.url,
    )
    .await;
    Ok(axum::Json(hook))
}

/// O webhook `hook_id` se for da org `org_id`; senão `404` (não se distingue
/// «de outra org» de «não existe»).
async fn fetch_hook(state: &AppState, org_id: Uuid, hook_id: Uuid) -> Result<Webhook, ApiError> {
    sqlx::query_as(&format!(
        "SELECT {WEBHOOK_COLUMNS} FROM org_webhooks WHERE id = $1 AND org_id = $2"
    ))
    .bind(hook_id)
    .bind(org_id)
    .fetch_optional(&state.db)
    .await?
    .ok_or(ApiError::NotFound)
}

/// Um webhook (sem o segredo).
#[utoipa::path(
    get, path = "/api/orgs/{org_id}/webhooks/{hook_id}", tag = "webhooks",
    security(("session" = [])),
    params(("org_id" = Uuid, Path), ("hook_id" = Uuid, Path)),
    responses(
        (status = 200, body = Webhook),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 403, body = crate::openapi::ErrorBody),
        (status = 404, body = crate::openapi::ErrorBody),
    )
)]
pub async fn get_one(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path((org_id, hook_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<Webhook>, ApiError> {
    crate::org::require_admin_pub(&state, org_id, auth.user_id).await?;
    Ok(Json(fetch_hook(&state, org_id, hook_id).await?))
}

// ---------- Registo de entregas e reenvio (G7) ----------

/// Uma entrega (sem o payload — esse vem no `GET` da entrega).
#[derive(Debug, Clone, Serialize, sqlx::FromRow, utoipa::ToSchema)]
pub struct WebhookDelivery {
    pub id: Uuid,
    pub org_id: Uuid,
    pub webhook_id: Uuid,
    /// Evento entregue (ver `KNOWN_EVENTS`).
    pub event: String,
    /// 1 no disparo original; um reenvio é a tentativa seguinte à reenviada.
    pub attempt: i32,
    /// `pending` | `succeeded` | `failed`.
    pub status: String,
    /// Código HTTP devolvido pelo destino, se houve resposta.
    pub response_status: Option<i32>,
    /// Tempo até à resposta, em milissegundos, se houve resposta.
    pub response_ms: Option<i32>,
    /// Razão da falha, curta e sem URLs.
    pub error: Option<String>,
    pub created_at: DateTime<Utc>,
    /// Quando a tentativa terminou (com sucesso ou não). Nulo enquanto `pending`.
    pub delivered_at: Option<DateTime<Utc>>,
    /// A entrega de que esta é um reenvio.
    pub redelivery_of: Option<Uuid>,
}

const DELIVERY_COLUMNS: &str = "id, org_id, webhook_id, event, attempt, status, response_status, \
                                response_ms, error, created_at, delivered_at, redelivery_of";

#[derive(sqlx::FromRow)]
struct DeliveryDetailRow {
    #[sqlx(flatten)]
    delivery: WebhookDelivery,
    payload_text: String,
}

/// Uma entrega com o corpo JSON exacto que foi enviado.
#[derive(Serialize, utoipa::ToSchema)]
pub struct WebhookDeliveryDetail {
    #[serde(flatten)]
    pub delivery: WebhookDelivery,
    #[schema(value_type = Object)]
    pub payload: serde_json::Value,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct WebhookDeliveryPage {
    pub items: Vec<WebhookDelivery>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_page_token: Option<String>,
}

#[derive(Deserialize, utoipa::IntoParams)]
#[into_params(parameter_in = Query)]
pub struct DeliveryListQuery {
    /// 1-100, omissão 50.
    pub page_size: Option<u32>,
    pub page_token: Option<String>,
    /// Só entregas neste estado: `pending` | `succeeded` | `failed`.
    pub status: Option<String>,
}

#[derive(Serialize, Deserialize)]
struct DeliveryCursor {
    at: DateTime<Utc>,
    id: Uuid,
}

/// Entregas de um webhook, mais recentes primeiro.
#[utoipa::path(
    get, path = "/api/orgs/{org_id}/webhooks/{hook_id}/deliveries", tag = "webhooks",
    security(("session" = [])),
    params(("org_id" = Uuid, Path), ("hook_id" = Uuid, Path), DeliveryListQuery),
    responses(
        (status = 200, body = WebhookDeliveryPage),
        (status = 400, body = crate::openapi::ErrorBody, description = "page_token ou status inválido"),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 403, body = crate::openapi::ErrorBody),
        (status = 404, body = crate::openapi::ErrorBody),
    )
)]
pub async fn list_deliveries(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path((org_id, hook_id)): Path<(Uuid, Uuid)>,
    Query(q): Query<DeliveryListQuery>,
) -> Result<Json<WebhookDeliveryPage>, ApiError> {
    crate::org::require_admin_pub(&state, org_id, auth.user_id).await?;
    let status = q
        .status
        .as_deref()
        .map(rules::DeliveryStatus::parse)
        .transpose()?;
    let page = PageRequest {
        page_size: q.page_size,
        page_token: q.page_token,
    };
    let size = page.size();
    let cursor: Option<DeliveryCursor> = page.cursor()?;
    fetch_hook(&state, org_id, hook_id).await?;
    let rows: Vec<WebhookDelivery> = sqlx::query_as(&format!(
        "SELECT {DELIVERY_COLUMNS} FROM webhook_deliveries
          WHERE webhook_id = $1 AND org_id = $2
            AND ($3::text IS NULL OR status = $3)
            AND ($4::timestamptz IS NULL OR (created_at, id) < ($4, $5))
          ORDER BY created_at DESC, id DESC
          LIMIT $6"
    ))
    .bind(hook_id)
    .bind(org_id)
    .bind(status.map(rules::DeliveryStatus::as_str))
    .bind(cursor.as_ref().map(|c| c.at))
    .bind(cursor.as_ref().map(|c| c.id).unwrap_or_default())
    .bind(size as i64 + 1)
    .fetch_all(&state.db)
    .await?;
    let p = Page::from_overfetch(rows, size, |d| DeliveryCursor {
        at: d.created_at,
        id: d.id,
    });
    Ok(Json(WebhookDeliveryPage {
        items: p.items,
        next_page_token: p.next_page_token,
    }))
}

async fn fetch_delivery(
    state: &AppState,
    org_id: Uuid,
    hook_id: Uuid,
    delivery_id: Uuid,
) -> Result<WebhookDeliveryDetail, ApiError> {
    let row: DeliveryDetailRow = sqlx::query_as(&format!(
        "SELECT {DELIVERY_COLUMNS}, payload::text AS payload_text FROM webhook_deliveries
          WHERE id = $1 AND webhook_id = $2 AND org_id = $3"
    ))
    .bind(delivery_id)
    .bind(hook_id)
    .bind(org_id)
    .fetch_optional(&state.db)
    .await?
    .ok_or(ApiError::NotFound)?;
    let payload = serde_json::from_str(&row.payload_text).map_err(ApiError::internal)?;
    Ok(WebhookDeliveryDetail {
        delivery: row.delivery,
        payload,
    })
}

/// Uma entrega, com o payload enviado.
#[utoipa::path(
    get, path = "/api/orgs/{org_id}/webhooks/{hook_id}/deliveries/{delivery_id}", tag = "webhooks",
    security(("session" = [])),
    params(("org_id" = Uuid, Path), ("hook_id" = Uuid, Path), ("delivery_id" = Uuid, Path)),
    responses(
        (status = 200, body = WebhookDeliveryDetail),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 403, body = crate::openapi::ErrorBody),
        (status = 404, body = crate::openapi::ErrorBody),
    )
)]
pub async fn get_delivery(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path((org_id, hook_id, delivery_id)): Path<(Uuid, Uuid, Uuid)>,
) -> Result<Json<WebhookDeliveryDetail>, ApiError> {
    crate::org::require_admin_pub(&state, org_id, auth.user_id).await?;
    Ok(Json(
        fetch_delivery(&state, org_id, hook_id, delivery_id).await?,
    ))
}

/// Método personalizado: volta a enviar o MESMO payload ao URL ACTUAL do
/// webhook (guarda anti-SSRF reaplicada), numa entrega nova com
/// `redelivery_of`. Responde `202` com a entrega nova em `pending`; o
/// resultado lê-se no `Location`.
#[utoipa::path(
    post, path = "/api/orgs/{org_id}/webhooks/{hook_id}/deliveries/{delivery_id}/redeliver", tag = "webhooks",
    security(("session" = [])),
    params(("org_id" = Uuid, Path), ("hook_id" = Uuid, Path), ("delivery_id" = Uuid, Path)),
    responses(
        (status = 202, body = WebhookDelivery, headers(("Location" = String)), description = "Reenvio aceite; a entrega nova está `pending`."),
        (status = 400, body = crate::openapi::ErrorBody, description = "o URL actual do webhook é recusado pela guarda anti-SSRF"),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 403, body = crate::openapi::ErrorBody),
        (status = 404, body = crate::openapi::ErrorBody),
        (status = 422, body = crate::openapi::ErrorBody, description = "webhook inactivo"),
        (status = 429, body = crate::openapi::ErrorBody, description = "demasiados reenvios para este webhook (máx. 10/min)"),
    )
)]
pub async fn redeliver(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path((org_id, hook_id, delivery_id)): Path<(Uuid, Uuid, Uuid)>,
) -> Result<Response, ApiError> {
    crate::org::require_admin_pub(&state, org_id, auth.user_id).await?;
    let hook = fetch_hook(&state, org_id, hook_id).await?;
    let original = fetch_delivery(&state, org_id, hook_id, delivery_id).await?;
    if !hook.active {
        return Err(DomainError::precondition(
            "webhook.inactive",
            "o webhook está inactivo — não se reenvia para um destino desligado",
        )
        .into());
    }

    // Antes de aceitar: o URL de AGORA tem de passar a guarda (o envio volta a
    // validá-lo, contra DNS-rebinding). Fora da transacção — é uma resolução DNS.
    state.outbound.check_tenant_url(&hook.url).await?;

    // O ritmo conta-se sob o lock da linha do webhook: dois reenvios em
    // simultâneo (dois nós, dois separadores) não passam ambos pelo último lugar.
    let mut tx = state.db.begin().await?;
    sqlx::query("SELECT 1 FROM org_webhooks WHERE id = $1 AND org_id = $2 FOR UPDATE")
        .bind(hook_id)
        .bind(org_id)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or(ApiError::NotFound)?;
    let recent: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM webhook_deliveries
          WHERE webhook_id = $1 AND redelivery_of IS NOT NULL
            AND created_at > now() - make_interval(secs => $2)",
    )
    .bind(hook_id)
    .bind(rules::REDELIVERY_WINDOW_SECS as f64)
    .fetch_one(&mut *tx)
    .await?;
    rules::check_redelivery_rate(recent)?;
    let delivery: WebhookDelivery = sqlx::query_as(&format!(
        "INSERT INTO webhook_deliveries (org_id, webhook_id, event, payload, attempt, redelivery_of)
         SELECT org_id, webhook_id, event, payload, $2, id
           FROM webhook_deliveries WHERE id = $1
         RETURNING {DELIVERY_COLUMNS}"
    ))
    .bind(delivery_id)
    .bind(rules::next_attempt(original.delivery.attempt))
    .fetch_one(&mut *tx)
    .await?;
    tx.commit().await?;

    crate::audit::log(
        &state.db,
        Some(org_id),
        auth.user_id,
        "webhook.redelivered",
        &delivery_id.to_string(),
    )
    .await;

    let new_id = delivery.id;
    let event = original.delivery.event;
    let payload = original.payload;
    let bg = state.clone();
    tokio::spawn(async move {
        attempt(&bg, &hook, &event, &payload, Some(new_id)).await;
    });

    let location = format!("/api/orgs/{org_id}/webhooks/{hook_id}/deliveries/{new_id}");
    Ok((StatusCode::ACCEPTED, [(LOCATION, location)], Json(delivery)).into_response())
}

#[cfg(test)]
mod tests {
    #[test]
    fn mom_ready_is_subscribed_by_default() {
        // Sem isto, um Odoo que crie o webhook sem `events` nunca soube que a
        // ata AI ficou pronta — o evento existia e não chegava a ninguém.
        assert!(super::DEFAULT_EVENTS.contains("meeting.mom_ready"));
        for e in super::DEFAULT_EVENTS.split(',') {
            assert!(
                super::KNOWN_EVENTS.contains(&e),
                "{e} não é um evento conhecido"
            );
        }
    }

    /// O varredor (G7) contra Postgres real: a `pending` abandonada fecha-se
    /// `failed`, a que passou da retenção sai, e a recente fica intacta.
    #[sqlx::test(migrations = "./migrations")]
    async fn sweep_closes_abandoned_and_drops_expired(db: sqlx::PgPool) {
        let hook: uuid::Uuid = sqlx::query_scalar(
            "WITH u AS (INSERT INTO users (email, username, password_hash)
                        VALUES ('a@x.test', 'a', 'x') RETURNING id),
                  o AS (INSERT INTO organizations (name, slug, created_by)
                        SELECT 'X', 'x', id FROM u RETURNING id, created_by)
             INSERT INTO org_webhooks (org_id, kind, url, created_by)
             SELECT id, 'generic', 'https://hooks.test/x', created_by FROM o RETURNING id",
        )
        .fetch_one(&db)
        .await
        .unwrap();
        let insert = |age: &'static str, status: &'static str| {
            let db = db.clone();
            async move {
                sqlx::query_scalar::<_, uuid::Uuid>(&format!(
                    "INSERT INTO webhook_deliveries (org_id, webhook_id, event, payload, status, created_at)
                     SELECT org_id, id, 'meeting.created', '{{}}'::jsonb, '{status}', now() - interval '{age}'
                       FROM org_webhooks WHERE id = $1 RETURNING id"
                ))
                .bind(hook)
                .fetch_one(&db)
                .await
                .unwrap()
            }
        };
        let expired = insert("31 days", "succeeded").await;
        let abandoned = insert("10 minutes", "pending").await;
        let in_flight = insert("5 seconds", "pending").await;
        let recent = insert("29 days", "failed").await;

        let (a, d) = super::sweep_deliveries(&db).await.unwrap();
        assert_eq!((a, d), (1, 1));
        let rows: Vec<(uuid::Uuid, String, Option<String>)> =
            sqlx::query_as("SELECT id, status, error FROM webhook_deliveries")
                .fetch_all(&db)
                .await
                .unwrap();
        let by_id = |id| rows.iter().find(|r| r.0 == id);
        assert!(by_id(expired).is_none(), "passou da retenção");
        let ab = by_id(abandoned).unwrap();
        assert_eq!(ab.1, "failed");
        assert_eq!(
            ab.2.as_deref(),
            Some(delonix_meet_domain::integration::webhook_delivery::ABANDONED_ERROR)
        );
        assert_eq!(by_id(in_flight).unwrap().1, "pending", "ainda em curso");
        assert_eq!(by_id(recent).unwrap().1, "failed");
    }
}

/// Apaga um webhook.
#[utoipa::path(
    delete, path = "/api/orgs/{org_id}/webhooks/{hook_id}", tag = "webhooks",
    security(("session" = [])),
    params(("org_id" = Uuid, Path), ("hook_id" = Uuid, Path)),
    responses(
        (status = 204, description = "Webhook apagado."),
        (status = 403, body = crate::openapi::ErrorBody),
    )
)]
pub async fn delete(
    axum::extract::State(state): axum::extract::State<Arc<AppState>>,
    auth: crate::auth::AuthUser,
    axum::extract::Path((org_id, hook_id)): axum::extract::Path<(Uuid, Uuid)>,
) -> Result<axum::http::StatusCode, crate::error::ApiError> {
    crate::org::require_admin_pub(&state, org_id, auth.user_id).await?;
    let res = sqlx::query("DELETE FROM org_webhooks WHERE id = $1 AND org_id = $2")
        .bind(hook_id)
        .bind(org_id)
        .execute(&state.db)
        .await?;
    if res.rows_affected() == 0 {
        return Err(crate::error::ApiError::NotFound);
    }
    crate::audit::log(
        &state.db,
        Some(org_id),
        auth.user_id,
        "webhook.deleted",
        &hook_id.to_string(),
    )
    .await;
    Ok(axum::http::StatusCode::NO_CONTENT)
}
