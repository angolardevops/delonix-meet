//! Biblioteca de quadros brancos: guardados como PNG por organização.
//!
//! Isolamento multi-tenant: um quadro pertence a uma organização e só é
//! listado/acedido por membros dessa org (via [`crate::org::orgs_of_user`]).
//! Partilha só-leitura por link público com token (`share_token`).

use axum::{
    extract::{Path, State},
    http::header,
    response::IntoResponse,
    Json,
};
use base64::{engine::general_purpose, Engine};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use uuid::Uuid;

use crate::{auth::AuthUser, error::ApiError, org::orgs_of_user, AppState};

/// PNG máximo aceite (evita abusos): 8 MB.
const MAX_PNG_BYTES: usize = 8 * 1024 * 1024;

#[derive(Serialize, sqlx::FromRow)]
pub struct WhiteboardMeta {
    pub id: Uuid,
    pub title: String,
    pub room_code: String,
    pub is_public: bool,
    pub share_token: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Deserialize)]
pub struct SaveReq {
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub room_code: String,
    /// PNG em base64 (com ou sem prefixo `data:image/png;base64,`).
    pub png_base64: String,
}

const PNG_MAGIC: &[u8] = &[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];

fn decode_png(s: &str) -> Result<Vec<u8>, ApiError> {
    let raw = s.split(',').next_back().unwrap_or(s).trim();
    // Rejeita ANTES de descodificar (evita alocar ~500MB só para rejeitar).
    if raw.len() > MAX_PNG_BYTES * 4 / 3 + 4 {
        return Err(ApiError::BadRequest(
            "tamanho do quadro fora do limite".into(),
        ));
    }
    let bytes = general_purpose::STANDARD
        .decode(raw)
        .map_err(|_| ApiError::BadRequest("PNG inválido".into()))?;
    if bytes.len() > MAX_PNG_BYTES {
        return Err(ApiError::BadRequest(
            "tamanho do quadro fora do limite".into(),
        ));
    }
    // Valida a assinatura PNG (impede guardar bytes arbitrários servidos como image/png).
    if !bytes.starts_with(PNG_MAGIC) {
        return Err(ApiError::BadRequest(
            "o conteúdo não é um PNG válido".into(),
        ));
    }
    Ok(bytes)
}

/// Não expõe o token de partilha enquanto o quadro não for público.
fn mask_token(mut m: WhiteboardMeta) -> WhiteboardMeta {
    if !m.is_public {
        m.share_token = String::new();
    }
    m
}

/// Guarda um quadro na biblioteca da organização do utilizador.
pub async fn save(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Json(req): Json<SaveReq>,
) -> Result<Json<WhiteboardMeta>, ApiError> {
    let org_id = *orgs_of_user(&state, auth.user_id)
        .await
        .first()
        .ok_or(ApiError::BadRequest("utilizador sem organização".into()))?;
    let png = decode_png(&req.png_base64)?;
    let title = if req.title.trim().is_empty() {
        "Quadro sem título".to_string()
    } else {
        req.title.trim().chars().take(120).collect()
    };
    let meta: WhiteboardMeta = sqlx::query_as(
        "INSERT INTO whiteboards (org_id, owner_id, title, room_code, png)
         VALUES ($1, $2, $3, $4, $5)
         RETURNING id, title, room_code, is_public, share_token, created_at",
    )
    .bind(org_id)
    .bind(auth.user_id)
    .bind(title)
    .bind(req.room_code.chars().take(64).collect::<String>())
    .bind(png)
    .fetch_one(&state.db)
    .await?;
    Ok(Json(mask_token(meta)))
}

/// Lista os quadros das organizações a que o utilizador pertence.
pub async fn list(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
) -> Result<Json<Vec<WhiteboardMeta>>, ApiError> {
    let orgs = orgs_of_user(&state, auth.user_id).await;
    if orgs.is_empty() {
        return Ok(Json(vec![]));
    }
    let items: Vec<WhiteboardMeta> = sqlx::query_as(
        "SELECT id, title, room_code, is_public, share_token, created_at
         FROM whiteboards WHERE org_id = ANY($1) ORDER BY created_at DESC LIMIT 200",
    )
    .bind(&orgs)
    .fetch_all(&state.db)
    .await?;
    Ok(Json(items.into_iter().map(mask_token).collect()))
}

/// Imagem PNG de um quadro — apenas membros da org dona.
pub async fn png(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> Result<impl IntoResponse, ApiError> {
    let orgs = orgs_of_user(&state, auth.user_id).await;
    let row: (Vec<u8>, Uuid) = sqlx::query_as("SELECT png, org_id FROM whiteboards WHERE id = $1")
        .bind(id)
        .fetch_one(&state.db)
        .await?;
    if !orgs.contains(&row.1) {
        return Err(ApiError::Unauthorized);
    }
    Ok(([(header::CONTENT_TYPE, "image/png")], row.0))
}

/// Apaga um quadro — dono ou admin da org.
pub async fn delete(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let row: (Uuid, Uuid) =
        sqlx::query_as("SELECT owner_id, org_id FROM whiteboards WHERE id = $1")
            .bind(id)
            .fetch_one(&state.db)
            .await?;
    let is_owner = row.0 == auth.user_id;
    let is_admin = crate::org::require_admin_pub(&state, row.1, auth.user_id)
        .await
        .is_ok();
    if !is_owner && !is_admin {
        return Err(ApiError::Unauthorized);
    }
    sqlx::query("DELETE FROM whiteboards WHERE id = $1")
        .bind(id)
        .execute(&state.db)
        .await?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

#[derive(Deserialize)]
pub struct ShareReq {
    pub public: bool,
}

/// Ativa/desativa a partilha por link público. Dono ou admin.
pub async fn set_share(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
    Json(req): Json<ShareReq>,
) -> Result<Json<WhiteboardMeta>, ApiError> {
    let row: (Uuid, Uuid) =
        sqlx::query_as("SELECT owner_id, org_id FROM whiteboards WHERE id = $1")
            .bind(id)
            .fetch_one(&state.db)
            .await?;
    let is_owner = row.0 == auth.user_id;
    let is_admin = crate::org::require_admin_pub(&state, row.1, auth.user_id)
        .await
        .is_ok();
    if !is_owner && !is_admin {
        return Err(ApiError::Unauthorized);
    }
    // Ao desativar a partilha, roda o token (o link antigo deixa de funcionar).
    let meta: WhiteboardMeta = sqlx::query_as(
        "UPDATE whiteboards SET is_public = $1,
             share_token = CASE WHEN $1 THEN share_token ELSE encode(gen_random_bytes(12), 'hex') END
         WHERE id = $2
         RETURNING id, title, room_code, is_public, share_token, created_at",
    )
    .bind(req.public)
    .bind(id)
    .fetch_one(&state.db)
    .await?;
    Ok(Json(mask_token(meta)))
}

/// Vista pública só-leitura por token — sem autenticação, se `is_public`.
pub async fn shared_png(
    State(state): State<Arc<AppState>>,
    Path(token): Path<String>,
) -> Result<impl IntoResponse, ApiError> {
    let row: (Vec<u8>, bool) =
        sqlx::query_as("SELECT png, is_public FROM whiteboards WHERE share_token = $1")
            .bind(token)
            .fetch_one(&state.db)
            .await?;
    if !row.1 {
        return Err(ApiError::NotFound);
    }
    Ok(([(header::CONTENT_TYPE, "image/png")], row.0))
}
