//! Chaves de API por organização + superfície REST pública `/api/v1`.
//!
//! Gestão (admin da org, autenticada por sessão): criar / listar / revogar.
//! A chave completa (`dlx_<32 hex>`) só é mostrada UMA vez na criação; guarda-se
//! o SHA-256. As integrações externas autenticam com
//! `Authorization: Bearer dlx_...` (ou header `X-API-Key`).

use axum::{
    extract::{FromRequestParts, Path, State},
    http::request::Parts,
    Json,
};
use rand::{rngs::OsRng, RngCore};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::sync::Arc;
use uuid::Uuid;

use crate::{auth::AuthUser, error::ApiError, AppState};

fn sha256_hex(s: &str) -> String {
    hex::encode(Sha256::digest(s.as_bytes()))
}

// ---------- Autenticação por chave de API (extractor) ----------

/// Pedido autenticado por chave de API: transporta a organização e o
/// utilizador dono da chave (usado como owner das salas criadas via API).
pub struct ApiKeyAuth {
    pub org_id: Uuid,
    pub owner_id: Uuid,
}

impl FromRequestParts<Arc<AppState>> for ApiKeyAuth {
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &Arc<AppState>,
    ) -> Result<Self, ApiError> {
        let raw = parts
            .headers
            .get("x-api-key")
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string())
            .or_else(|| {
                parts
                    .headers
                    .get(axum::http::header::AUTHORIZATION)
                    .and_then(|v| v.to_str().ok())
                    .and_then(|h| h.strip_prefix("Bearer "))
                    .map(|s| s.to_string())
            })
            .ok_or(ApiError::Unauthorized)?;
        if !raw.starts_with("dlx_") {
            return Err(ApiError::Unauthorized);
        }
        let hash = sha256_hex(&raw);
        let row: Option<(Uuid, Uuid)> =
            sqlx::query_as("SELECT org_id, created_by FROM org_api_keys WHERE key_hash = $1")
                .bind(&hash)
                .fetch_optional(&state.db)
                .await?;
        let (org_id, owner_id) = row.ok_or(ApiError::Unauthorized)?;
        // Regista uso (best-effort) sem falhar o pedido.
        let _ = sqlx::query("UPDATE org_api_keys SET last_used_at = now() WHERE key_hash = $1")
            .bind(&hash)
            .execute(&state.db)
            .await;
        Ok(ApiKeyAuth { org_id, owner_id })
    }
}

// ---------- Gestão das chaves (admin, sessão) ----------

#[derive(Serialize, sqlx::FromRow)]
pub struct ApiKeyInfo {
    pub id: Uuid,
    pub name: String,
    pub prefix: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub last_used_at: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Deserialize)]
pub struct CreateKeyReq {
    #[serde(default)]
    pub name: String,
}

#[derive(Serialize)]
pub struct CreatedKey {
    pub id: Uuid,
    pub name: String,
    pub prefix: String,
    /// A chave completa — só devolvida AGORA, não fica guardada em claro.
    pub key: String,
}

pub async fn list(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(org_id): Path<Uuid>,
) -> Result<Json<Vec<ApiKeyInfo>>, ApiError> {
    crate::org::require_admin_pub(&state, org_id, auth.user_id).await?;
    let keys: Vec<ApiKeyInfo> = sqlx::query_as(
        "SELECT id, name, prefix, created_at, last_used_at FROM org_api_keys
         WHERE org_id = $1 ORDER BY created_at DESC",
    )
    .bind(org_id)
    .fetch_all(&state.db)
    .await?;
    Ok(Json(keys))
}

pub async fn create(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(org_id): Path<Uuid>,
    Json(req): Json<CreateKeyReq>,
) -> Result<Json<CreatedKey>, ApiError> {
    crate::org::require_admin_pub(&state, org_id, auth.user_id).await?;
    let mut bytes = [0u8; 32]; // 256 bits de entropia
    OsRng.fill_bytes(&mut bytes);
    let key = format!("dlx_{}", hex::encode(bytes));
    let prefix = key.chars().take(12).collect::<String>();
    let hash = sha256_hex(&key);
    let name = req.name.trim().chars().take(60).collect::<String>();
    let (id,): (Uuid,) = sqlx::query_as(
        "INSERT INTO org_api_keys (org_id, name, prefix, key_hash, created_by)
         VALUES ($1, $2, $3, $4, $5) RETURNING id",
    )
    .bind(org_id)
    .bind(&name)
    .bind(&prefix)
    .bind(&hash)
    .bind(auth.user_id)
    .fetch_one(&state.db)
    .await?;
    Ok(Json(CreatedKey {
        id,
        name,
        prefix,
        key,
    }))
}

pub async fn revoke(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path((org_id, key_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<serde_json::Value>, ApiError> {
    crate::org::require_admin_pub(&state, org_id, auth.user_id).await?;
    sqlx::query("DELETE FROM org_api_keys WHERE id = $1 AND org_id = $2")
        .bind(key_id)
        .bind(org_id)
        .execute(&state.db)
        .await?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

// ---------- API pública v1 (autenticada por chave) ----------

/// Constrói o link partilhável de uma sala com o domínio de produção da org.
async fn room_link(state: &AppState, org_id: Uuid, code: &str) -> String {
    let domain: Option<(String,)> =
        sqlx::query_as("SELECT domain FROM organizations WHERE id = $1 AND domain <> ''")
            .bind(org_id)
            .fetch_optional(&state.db)
            .await
            .ok()
            .flatten();
    match domain {
        Some((d,)) => format!("https://{d}/#/r/{code}"),
        None => format!("/#/r/{code}"),
    }
}

#[derive(Deserialize)]
pub struct ApiCreateRoomReq {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub e2ee: bool,
    #[serde(default)]
    pub waiting_room: bool,
}

/// `POST /api/v1/rooms` — cria uma sala e devolve o código + link de entrada.
pub async fn v1_create_room(
    State(state): State<Arc<AppState>>,
    key: ApiKeyAuth,
    Json(req): Json<ApiCreateRoomReq>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let name: String = req
        .name
        .unwrap_or_else(|| "Reunião (API)".into())
        .trim()
        .chars()
        .take(120)
        .collect();
    let room = crate::rooms::insert_room(
        &state.db,
        key.owner_id,
        &name,
        "sfu",
        req.waiting_room,
        req.e2ee,
        "normal",
    )
    .await?;
    let link = room_link(&state, key.org_id, &room.code).await;
    Ok(Json(serde_json::json!({
        "code": room.code,
        "name": room.name,
        "e2ee": room.e2ee,
        "waiting_room": room.waiting_room,
        "join_url": link,
    })))
}

/// `GET /api/v1/rooms/{code}` — metadados de uma sala.
pub async fn v1_get_room(
    State(state): State<Arc<AppState>>,
    key: ApiKeyAuth,
    Path(code): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let row: Option<(String, String, bool, bool, Uuid)> = sqlx::query_as(
        "SELECT code, name, e2ee, waiting_room, owner_id FROM rooms WHERE code = $1",
    )
    .bind(&code)
    .fetch_optional(&state.db)
    .await?;
    let (code, name, e2ee, waiting, owner_id) = row.ok_or(ApiError::NotFound)?;

    // Assegura que a sala pertence à organização da chave API.
    crate::org::require_member_pub(&state, key.org_id, owner_id).await?;

    let link = room_link(&state, key.org_id, &code).await;
    Ok(Json(serde_json::json!({
        "code": code, "name": name, "e2ee": e2ee, "waiting_room": waiting, "join_url": link,
    })))
}

#[derive(Deserialize)]
pub struct JoinBotReq {
    pub bot_name: String,
}

/// `POST /api/v1/rooms/{code}/join-bot` — gera um room_token para um bot headless.
pub async fn v1_join_bot_room(
    State(state): State<Arc<AppState>>,
    key: ApiKeyAuth,
    Path(code): Path<String>,
    Json(req): Json<JoinBotReq>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let room: crate::rooms::Room = sqlx::query_as(
        "SELECT id, code, name, owner_id, topology, waiting_room, e2ee, format, created_at FROM rooms WHERE code = $1",
    )
    .bind(code.to_lowercase())
    .fetch_one(&state.db)
    .await?;

    // O bot só pode entrar em salas que pertençam à sua organização.
    crate::org::require_member_pub(&state, key.org_id, room.owner_id).await?;

    // Para bots autorizados pela org (via API key), podemos forçar entrada direta
    // mesmo que a sala tenha waiting_room, dado o consentimento via plano implementado.
    let now = chrono::Utc::now().timestamp();
    let bot_name = if req.bot_name.trim().is_empty() {
        "AI Assistant".into()
    } else {
        req.bot_name.trim().chars().take(40).collect::<String>()
    };

    let room_token = crate::auth::sign_jwt(
        &state.config.jwt_secret,
        &crate::auth::Claims {
            sub: key.owner_id, // Atribui as ações do bot ao criador da chave API
            typ: "room".into(),
            iat: now,
            exp: now + state.config.room_token_ttl_secs,
            room: Some(room.id),
            name: Some(bot_name),
            topo: Some(room.topology.clone()),
            owner: false, // bots não podem gerir a sala
            wait: false,  // Bypass da sala de espera (acordado com o utilizador)
            adm: false,
            is_bot: true,
        },
    )?;

    Ok(Json(serde_json::json!({
        "room": room,
        "room_token": room_token,
        "ws_path": format!("/ws?token={room_token}"),
    })))
}

/// `GET /api/v1/recordings` — gravações da organização (membros).
pub async fn v1_recordings(
    State(state): State<Arc<AppState>>,
    key: ApiKeyAuth,
) -> Result<Json<serde_json::Value>, ApiError> {
    let rows: Vec<(Uuid, String, i64, chrono::DateTime<chrono::Utc>, String)> = sqlx::query_as(
        "SELECT r.id, r.filename, r.size_bytes, r.created_at, rm.code
         FROM recordings r
         JOIN rooms rm ON rm.id = r.room_id
         JOIN org_members m ON m.user_id = r.uploader_id AND m.org_id = $1
         ORDER BY r.created_at DESC LIMIT 200",
    )
    .bind(key.org_id)
    .fetch_all(&state.db)
    .await?;
    let items: Vec<serde_json::Value> = rows
        .into_iter()
        .map(|(id, filename, size, created, code)| {
            serde_json::json!({
                "id": id, "filename": filename, "size_bytes": size,
                "created_at": created, "room_code": code,
                "download_url": format!("/api/recordings/{id}"),
            })
        })
        .collect();
    Ok(Json(serde_json::json!({ "recordings": items })))
}

/// `GET /api/v1/org` — dados da organização da chave.
pub async fn v1_org(
    State(state): State<Arc<AppState>>,
    key: ApiKeyAuth,
) -> Result<Json<serde_json::Value>, ApiError> {
    let row: (String, String, String) =
        sqlx::query_as("SELECT name, email_domain, domain FROM organizations WHERE id = $1")
            .bind(key.org_id)
            .fetch_one(&state.db)
            .await?;
    let (members,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM org_members WHERE org_id = $1")
        .bind(key.org_id)
        .fetch_one(&state.db)
        .await?;
    Ok(Json(serde_json::json!({
        "id": key.org_id, "name": row.0, "email_domain": row.1,
        "domain": row.2, "members": members,
    })))
}
