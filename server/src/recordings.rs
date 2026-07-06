//! Gravações de reuniões: upload (webm), biblioteca por utilizador,
//! partilha só-leitura e download.
//!
//! O ficheiro fica no disco (`RECORDINGS_DIR`, por omissão `./recordings`);
//! a base de dados guarda os metadados. Acesso: quem participou na sala
//! (`room_participants`), quem fez o upload, ou com quem foi partilhada
//! (`recording_shares`). Partilha é sempre só-leitura (download).

use axum::{
    body::Bytes,
    extract::{Path, Query, State},
    http::header,
    response::IntoResponse,
    Json,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use uuid::Uuid;

use crate::{auth::AuthUser, error::ApiError, rooms::Room, users::UserPublic, AppState};

pub const MAX_RECORDING_BYTES: usize = 512 * 1024 * 1024;

fn recordings_dir() -> std::path::PathBuf {
    std::env::var("RECORDINGS_DIR")
        .unwrap_or_else(|_| "recordings".into())
        .into()
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct Recording {
    pub id: Uuid,
    pub room_id: Uuid,
    pub uploader_id: Uuid,
    pub filename: String,
    pub size_bytes: i64,
    pub created_at: DateTime<Utc>,
}

/// Item da biblioteca, enriquecido para a UI.
#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct RecordingItem {
    pub id: Uuid,
    pub room_id: Uuid,
    pub room_code: String,
    pub uploader_id: Uuid,
    pub uploader_name: String,
    pub filename: String,
    pub size_bytes: i64,
    pub created_at: DateTime<Utc>,
    /// True se o utilizador atual é dono (participou/fez upload); false se só partilhada.
    pub owned: bool,
    /// Nº de utilizadores com quem está partilhada (só relevante para o dono).
    pub share_count: i64,
}

async fn room_by_code(state: &AppState, code: &str) -> Result<Room, ApiError> {
    let room: Room = sqlx::query_as(
        "SELECT id, code, name, owner_id, topology, waiting_room, e2ee, created_at FROM rooms WHERE code = $1",
    )
    .bind(code.to_lowercase())
    .fetch_one(&state.db)
    .await?;
    Ok(room)
}

async fn is_participant(state: &AppState, room_id: Uuid, user_id: Uuid) -> Result<bool, ApiError> {
    let row: Option<(i32,)> =
        sqlx::query_as("SELECT 1 FROM room_participants WHERE room_id = $1 AND user_id = $2")
            .bind(room_id)
            .bind(user_id)
            .fetch_optional(&state.db)
            .await?;
    Ok(row.is_some())
}

/// Acesso a uma gravação: participou na sala, fez upload, ou foi-lhe partilhada.
async fn can_access(state: &AppState, rec: &Recording, user_id: Uuid) -> Result<bool, ApiError> {
    if rec.uploader_id == user_id {
        return Ok(true);
    }
    if is_participant(state, rec.room_id, user_id).await? {
        return Ok(true);
    }
    let shared: Option<(i32,)> =
        sqlx::query_as("SELECT 1 FROM recording_shares WHERE recording_id = $1 AND user_id = $2")
            .bind(rec.id)
            .bind(user_id)
            .fetch_optional(&state.db)
            .await?;
    Ok(shared.is_some())
}

#[derive(Deserialize)]
pub struct UploadQuery {
    #[serde(default)]
    pub name: Option<String>,
}

pub async fn upload(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(code): Path<String>,
    Query(q): Query<UploadQuery>,
    body: Bytes,
) -> Result<Json<Recording>, ApiError> {
    if body.is_empty() {
        return Err(ApiError::BadRequest("empty recording".into()));
    }
    if body.len() > MAX_RECORDING_BYTES {
        return Err(ApiError::BadRequest("recording too large".into()));
    }
    let room = room_by_code(&state, &code).await?;
    // Só quem participou na sala pode carregar gravações dela.
    if !is_participant(&state, room.id, auth.user_id).await? {
        return Err(ApiError::Unauthorized);
    }

    let stamp = Utc::now().format("%Y%m%d-%H%M%S");
    let display = q
        .name
        .filter(|n| !n.trim().is_empty())
        .unwrap_or_else(|| format!("{}-{stamp}.webm", room.code));

    let rec: Recording = sqlx::query_as(
        "INSERT INTO recordings (room_id, uploader_id, filename, size_bytes)
         VALUES ($1, $2, $3, $4)
         RETURNING id, room_id, uploader_id, filename, size_bytes, created_at",
    )
    .bind(room.id)
    .bind(auth.user_id)
    .bind(&display)
    .bind(body.len() as i64)
    .fetch_one(&state.db)
    .await?;

    let dir = recordings_dir();
    tokio::fs::create_dir_all(&dir).await.map_err(ApiError::internal)?;
    tokio::fs::write(dir.join(format!("{}.webm", rec.id)), &body)
        .await
        .map_err(ApiError::internal)?;

    tracing::info!(room = %room.code, id = %rec.id, size = body.len(), "recording stored");
    Ok(Json(rec))
}

/// Gravações de uma sala específica (painel dentro da reunião).
pub async fn list(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(code): Path<String>,
) -> Result<Json<Vec<Recording>>, ApiError> {
    let room = room_by_code(&state, &code).await?;
    if !is_participant(&state, room.id, auth.user_id).await? {
        return Err(ApiError::Unauthorized);
    }
    let recs: Vec<Recording> = sqlx::query_as(
        "SELECT id, room_id, uploader_id, filename, size_bytes, created_at
         FROM recordings WHERE room_id = $1 ORDER BY created_at DESC",
    )
    .bind(room.id)
    .fetch_all(&state.db)
    .await?;
    Ok(Json(recs))
}

/// Biblioteca do utilizador: gravações onde participou + partilhadas consigo.
pub async fn library(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
) -> Result<Json<Vec<RecordingItem>>, ApiError> {
    let items: Vec<RecordingItem> = sqlx::query_as(
        r#"
        SELECT r.id, r.room_id, rm.code AS room_code,
               r.uploader_id, u.username AS uploader_name,
               r.filename, r.size_bytes, r.created_at,
               (p.user_id IS NOT NULL) AS owned,
               COALESCE(sc.n, 0) AS share_count
        FROM recordings r
        JOIN rooms rm ON rm.id = r.room_id
        JOIN users u ON u.id = r.uploader_id
        LEFT JOIN room_participants p ON p.room_id = r.room_id AND p.user_id = $1
        LEFT JOIN recording_shares s ON s.recording_id = r.id AND s.user_id = $1
        LEFT JOIN (
            SELECT recording_id, COUNT(*) AS n FROM recording_shares GROUP BY recording_id
        ) sc ON sc.recording_id = r.id
        WHERE p.user_id IS NOT NULL OR s.user_id IS NOT NULL OR r.uploader_id = $1
        ORDER BY r.created_at DESC
        "#,
    )
    .bind(auth.user_id)
    .fetch_all(&state.db)
    .await?;
    Ok(Json(items))
}

pub async fn download(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> Result<impl IntoResponse, ApiError> {
    let rec: Recording = sqlx::query_as(
        "SELECT id, room_id, uploader_id, filename, size_bytes, created_at FROM recordings WHERE id = $1",
    )
    .bind(id)
    .fetch_one(&state.db)
    .await?;
    if !can_access(&state, &rec, auth.user_id).await? {
        return Err(ApiError::Unauthorized);
    }

    let path = recordings_dir().join(format!("{}.webm", rec.id));
    let data = tokio::fs::read(&path).await.map_err(|_| ApiError::NotFound)?;
    Ok((
        [
            (header::CONTENT_TYPE, "video/webm".to_string()),
            (
                header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"{}\"", rec.filename.replace('"', "")),
            ),
        ],
        data,
    ))
}

#[derive(Deserialize)]
pub struct ShareReq {
    pub user_id: Uuid,
}

/// Partilha só-leitura de uma gravação com outro utilizador.
/// Apenas quem fez o upload (o "dono") pode partilhar.
pub async fn share(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
    Json(req): Json<ShareReq>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let rec: Recording = sqlx::query_as(
        "SELECT id, room_id, uploader_id, filename, size_bytes, created_at FROM recordings WHERE id = $1",
    )
    .bind(id)
    .fetch_one(&state.db)
    .await?;
    if rec.uploader_id != auth.user_id {
        return Err(ApiError::Unauthorized);
    }
    if req.user_id == auth.user_id {
        return Err(ApiError::BadRequest("cannot share with yourself".into()));
    }
    sqlx::query(
        "INSERT INTO recording_shares (recording_id, user_id, shared_by) VALUES ($1, $2, $3)
         ON CONFLICT (recording_id, user_id) DO NOTHING",
    )
    .bind(id)
    .bind(req.user_id)
    .bind(auth.user_id)
    .execute(&state.db)
    .await?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

/// Remove a partilha com um utilizador.
pub async fn unshare(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path((id, user_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let owner: Option<(Uuid,)> =
        sqlx::query_as("SELECT uploader_id FROM recordings WHERE id = $1")
            .bind(id)
            .fetch_optional(&state.db)
            .await?;
    match owner {
        Some((uploader,)) if uploader == auth.user_id => {}
        Some(_) => return Err(ApiError::Unauthorized),
        None => return Err(ApiError::NotFound),
    }
    sqlx::query("DELETE FROM recording_shares WHERE recording_id = $1 AND user_id = $2")
        .bind(id)
        .bind(user_id)
        .execute(&state.db)
        .await?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

/// Lista com quem uma gravação está partilhada (só o dono).
pub async fn shares(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> Result<Json<Vec<UserPublic>>, ApiError> {
    let owner: Option<(Uuid,)> =
        sqlx::query_as("SELECT uploader_id FROM recordings WHERE id = $1")
            .bind(id)
            .fetch_optional(&state.db)
            .await?;
    match owner {
        Some((uploader,)) if uploader == auth.user_id => {}
        Some(_) => return Err(ApiError::Unauthorized),
        None => return Err(ApiError::NotFound),
    }
    let users = sqlx::query_as::<_, UserPublic>(
        "SELECT u.id, u.email, u.username, u.created_at FROM recording_shares s
         JOIN users u ON u.id = s.user_id
         WHERE s.recording_id = $1 ORDER BY u.username",
    )
    .bind(id)
    .fetch_all(&state.db)
    .await?;
    Ok(Json(users))
}
