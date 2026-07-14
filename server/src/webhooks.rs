//! Webhooks de saída por organização (Slack / Teams / Mattermost / genérico).
//!
//! Eventos disparados: `meeting.created`, `meeting.started`, `recording.ready`.
//! Os alvos Slack/Mattermost recebem `{ "text": "..." }`; Teams recebe um
//! MessageCard; o alvo `generic` recebe o JSON estruturado com a assinatura
//! `X-Delonix-Signature: sha256=<hmac>` (chave = `secret`) para verificação.
//!
//! O envio é best-effort e assíncrono (não bloqueia o pedido do utilizador).

use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use std::net::{IpAddr, Ipv6Addr};
use std::sync::Arc;
use uuid::Uuid;

use crate::{error::ApiError, AppState};

type HmacSha256 = Hmac<Sha256>;

/// True se o IP pertence a um intervalo interno/privado que não deve ser
/// alcançável a partir do servidor (anti-SSRF).
fn ip_is_blocked(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            let o = v4.octets();
            v4.is_private()
                || v4.is_loopback()
                || v4.is_link_local()
                || v4.is_unspecified()
                || v4.is_broadcast()
                || v4.is_multicast()
                || v4.is_documentation()
                || o[0] == 0
                || (o[0] == 100 && (o[1] & 0xc0) == 64) // 100.64.0.0/10 CGNAT
        }
        IpAddr::V6(v6) => {
            if let Some(mapped) = v6.to_ipv4_mapped() {
                return ip_is_blocked(IpAddr::V4(mapped));
            }
            let seg0 = v6.segments()[0];
            v6.is_loopback()
                || v6.is_unspecified()
                || v6.is_multicast()
                || (seg0 & 0xfe00) == 0xfc00 // fc00::/7 ULA
                || (seg0 & 0xffc0) == 0xfe80 // fe80::/10 link-local
                || v6 == Ipv6Addr::LOCALHOST
        }
    }
}

/// Valida que um URL de webhook é público e seguro. Chamado na criação E na
/// entrega (esta última defende contra DNS-rebinding entre check e envio).
async fn validate_public_url(raw: &str) -> Result<(), ApiError> {
    let url = reqwest::Url::parse(raw).map_err(|_| ApiError::BadRequest("URL inválido".into()))?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err(ApiError::BadRequest("esquema de URL inválido".into()));
    }
    if url.username() != "" || url.password().is_some() {
        return Err(ApiError::BadRequest(
            "URL não pode conter credenciais".into(),
        ));
    }
    let host = url
        .host_str()
        .ok_or_else(|| ApiError::BadRequest("URL sem host".into()))?;
    let port = url.port_or_known_default().unwrap_or(443);
    let mut any = false;
    for sa in tokio::net::lookup_host((host, port))
        .await
        .map_err(|_| ApiError::BadRequest("host do URL não resolve".into()))?
    {
        any = true;
        if ip_is_blocked(sa.ip()) {
            return Err(ApiError::BadRequest(
                "o URL aponta para um endereço interno/privado".into(),
            ));
        }
    }
    if !any {
        return Err(ApiError::BadRequest("host do URL não resolve".into()));
    }
    Ok(())
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct Webhook {
    pub id: Uuid,
    pub org_id: Uuid,
    pub kind: String,
    pub url: String,
    #[serde(skip_serializing)]
    pub secret: String,
    pub events: String,
    pub active: bool,
}

/// Um evento de webhook: nome + payload estruturado (o corpo do `generic`).
pub struct Event {
    pub name: &'static str,
    pub title: String,
    pub text: String,
    pub payload: serde_json::Value,
}

/// Dispara um evento para todos os webhooks ativos de uma organização que o
/// tenham subscrito. Corre em background — falhas são registadas, não propagadas.
pub fn fire(state: Arc<AppState>, org_id: Uuid, event: Event) {
    tokio::spawn(async move {
        let hooks: Vec<Webhook> = match sqlx::query_as(
            "SELECT id, org_id, kind, url, secret, events, active
             FROM org_webhooks WHERE org_id = $1 AND active = TRUE",
        )
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
            // Revalida o destino no momento do envio (defende contra DNS-rebinding).
            if let Err(e) = validate_public_url(&hook.url).await {
                tracing::warn!(hook = %hook.id, error = %e, "webhook destino bloqueado (SSRF)");
                continue;
            }
            if let Err(e) = deliver(&state.webhook_client, &hook, &event).await {
                tracing::warn!(hook = %hook.id, error = %e, "webhook delivery failed");
            }
        }
    });
}

async fn deliver(client: &reqwest::Client, hook: &Webhook, event: &Event) -> anyhow::Result<()> {
    let req = match hook.kind.as_str() {
        "slack" | "mattermost" => client.post(&hook.url).json(&serde_json::json!({
            "text": format!("*{}*\n{}", event.title, event.text)
        })),
        "teams" => client.post(&hook.url).json(&serde_json::json!({
            "@type": "MessageCard",
            "@context": "http://schema.org/extensions",
            "summary": event.title,
            "themeColor": "C8201D",
            "title": event.title,
            "text": event.text,
        })),
        _ => {
            // Genérico: JSON estruturado + assinatura HMAC opcional.
            let body = serde_json::json!({
                "event": event.name,
                "title": event.title,
                "text": event.text,
                "data": event.payload,
            });
            let raw = serde_json::to_vec(&body)?;
            let mut rb = client
                .post(&hook.url)
                .header("Content-Type", "application/json")
                .header("X-Delonix-Event", event.name);
            if !hook.secret.is_empty() {
                if let Ok(mut mac) = HmacSha256::new_from_slice(hook.secret.as_bytes()) {
                    mac.update(&raw);
                    let sig = hex::encode(mac.finalize().into_bytes());
                    rb = rb.header("X-Delonix-Signature", format!("sha256={sig}"));
                }
            }
            rb.body(raw)
        }
    };
    req.send().await?.error_for_status()?;
    Ok(())
}

// ---------- CRUD (admin da organização) ----------

#[derive(Deserialize)]
pub struct WebhookReq {
    pub kind: String,
    pub url: String,
    #[serde(default)]
    pub secret: String,
    #[serde(default)]
    pub events: Option<String>,
}

pub async fn list(
    axum::extract::State(state): axum::extract::State<Arc<AppState>>,
    auth: crate::auth::AuthUser,
    axum::extract::Path(org_id): axum::extract::Path<Uuid>,
) -> Result<axum::Json<Vec<Webhook>>, crate::error::ApiError> {
    crate::org::require_admin_pub(&state, org_id, auth.user_id).await?;
    let hooks: Vec<Webhook> = sqlx::query_as(
        "SELECT id, org_id, kind, url, secret, events, active
         FROM org_webhooks WHERE org_id = $1 ORDER BY created_at",
    )
    .bind(org_id)
    .fetch_all(&state.db)
    .await?;
    Ok(axum::Json(hooks))
}

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
    validate_public_url(&req.url).await?;
    let events = req
        .events
        .unwrap_or_else(|| "meeting.created,meeting.started,recording.ready".into());
    let hook: Webhook = sqlx::query_as(
        "INSERT INTO org_webhooks (org_id, kind, url, secret, events, created_by)
         VALUES ($1, $2, $3, $4, $5, $6)
         RETURNING id, org_id, kind, url, secret, events, active",
    )
    .bind(org_id)
    .bind(&req.kind)
    .bind(&req.url)
    .bind(&req.secret)
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

#[cfg(test)]
mod tests {
    use super::ip_is_blocked;
    use std::net::IpAddr;

    fn ip(s: &str) -> IpAddr {
        s.parse().unwrap()
    }

    #[test]
    fn blocks_internal_ranges() {
        for s in [
            "127.0.0.1",        // loopback
            "10.0.0.5",         // privado
            "172.16.3.4",       // privado
            "192.168.1.1",      // privado
            "169.254.169.254",  // link-local / metadata cloud
            "100.64.0.1",       // CGNAT
            "0.0.0.0",          // unspecified
            "::1",              // loopback v6
            "fc00::1",          // ULA v6
            "fe80::1",          // link-local v6
            "::ffff:127.0.0.1", // v4-mapped loopback
            "::ffff:10.0.0.1",  // v4-mapped privado
        ] {
            assert!(ip_is_blocked(ip(s)), "devia bloquear {s}");
        }
    }

    #[test]
    fn allows_public_addresses() {
        for s in [
            "1.1.1.1",
            "8.8.8.8",
            "93.184.216.34",
            "2606:4700:4700::1111",
        ] {
            assert!(!ip_is_blocked(ip(s)), "não devia bloquear {s}");
        }
    }
}

pub async fn delete(
    axum::extract::State(state): axum::extract::State<Arc<AppState>>,
    auth: crate::auth::AuthUser,
    axum::extract::Path((org_id, hook_id)): axum::extract::Path<(Uuid, Uuid)>,
) -> Result<axum::Json<serde_json::Value>, crate::error::ApiError> {
    crate::org::require_admin_pub(&state, org_id, auth.user_id).await?;
    sqlx::query("DELETE FROM org_webhooks WHERE id = $1 AND org_id = $2")
        .bind(hook_id)
        .bind(org_id)
        .execute(&state.db)
        .await?;
    crate::audit::log(
        &state.db,
        Some(org_id),
        auth.user_id,
        "webhook.deleted",
        &hook_id.to_string(),
    )
    .await;
    Ok(axum::Json(serde_json::json!({ "ok": true })))
}
