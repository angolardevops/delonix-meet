//! Gravações de reuniões: upload (webm), biblioteca por utilizador,
//! partilha só-leitura e download.
//!
//! O ficheiro fica no disco (`RECORDINGS_DIR`, por omissão `./recordings`);
//! a base de dados guarda os metadados. Acesso: quem participou na sala
//! (`room_participants`), quem fez o upload, ou com quem foi partilhada
//! (`recording_shares`). Partilha é sempre só-leitura (download).

use argon2::{
    password_hash::{rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2,
};
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
    /// RBAC de download: só o dono da gravação e admins da org do dono podem
    /// descarregar o ficheiro; os restantes só reproduzem.
    pub can_download: bool,
}

async fn room_by_code(state: &AppState, code: &str) -> Result<Room, ApiError> {
    let room: Room = sqlx::query_as(
        "SELECT id, code, name, owner_id, topology, waiting_room, e2ee, format, created_at FROM rooms WHERE code = $1",
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
    tokio::fs::create_dir_all(&dir)
        .await
        .map_err(ApiError::internal)?;
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
               COALESCE(sc.n, 0) AS share_count,
               (r.uploader_id = $1 OR EXISTS(
                  SELECT 1 FROM org_members me
                  JOIN org_members o ON o.org_id = me.org_id
                  WHERE me.user_id = $1 AND me.role = 'admin' AND o.user_id = r.uploader_id
               )) AS can_download
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

/// `?dl=1` pede o ficheiro para DESCARREGAR (attachment); sem isso, é para
/// REPRODUZIR inline. Descarregar exige RBAC (dono + admin da org); reproduzir
/// basta ter acesso (participante/partilhado/dono).
#[derive(Deserialize)]
pub struct DownloadQuery {
    #[serde(default)]
    pub dl: Option<i32>,
}

/// RBAC de download: dono da gravação, ou admin de uma org a que o dono pertence.
async fn can_download(state: &AppState, rec: &Recording, user_id: Uuid) -> Result<bool, ApiError> {
    if rec.uploader_id == user_id {
        return Ok(true);
    }
    let is_admin: bool = sqlx::query_scalar(
        r#"SELECT EXISTS(
             SELECT 1 FROM org_members me
             JOIN org_members o ON o.org_id = me.org_id
             WHERE me.user_id = $1 AND me.role = 'admin' AND o.user_id = $2
           )"#,
    )
    .bind(user_id)
    .bind(rec.uploader_id)
    .fetch_one(&state.db)
    .await?;
    Ok(is_admin)
}

pub async fn download(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
    Query(q): Query<DownloadQuery>,
) -> Result<impl IntoResponse, ApiError> {
    let rec: Recording = sqlx::query_as(
        "SELECT id, room_id, uploader_id, filename, size_bytes, created_at FROM recordings WHERE id = $1",
    )
    .bind(id)
    .fetch_one(&state.db)
    .await?;
    let as_download = q.dl.unwrap_or(0) == 1;
    if as_download {
        // Ficheiro para guardar: exige a permissão de download (RBAC).
        if !can_download(&state, &rec, auth.user_id).await? {
            return Err(ApiError::Unauthorized);
        }
    } else if !can_access(&state, &rec, auth.user_id).await? {
        // Reprodução inline: basta ter acesso à gravação.
        return Err(ApiError::Unauthorized);
    }

    let path = recordings_dir().join(format!("{}.webm", rec.id));
    let data = tokio::fs::read(&path)
        .await
        .map_err(|_| ApiError::NotFound)?;
    let disposition = if as_download {
        format!("attachment; filename=\"{}\"", rec.filename.replace('"', ""))
    } else {
        "inline".to_string()
    };
    Ok((
        [
            (header::CONTENT_TYPE, "video/webm".to_string()),
            (header::CONTENT_DISPOSITION, disposition),
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
    let owner: Option<(Uuid,)> = sqlx::query_as("SELECT uploader_id FROM recordings WHERE id = $1")
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

// ---------- Links públicos de partilha ----------

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct ShareLink {
    pub id: Uuid,
    pub recording_id: Uuid,
    pub token: String,
    pub expires_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

#[derive(Deserialize)]
pub struct CreateLinkReq {
    #[serde(default)]
    pub password: Option<String>,
    #[serde(default)]
    pub expires_at: Option<DateTime<Utc>>,
}

fn gen_token() -> String {
    Uuid::new_v4().to_string().replace('-', "")
}

/// Cria (ou substitui) um link público de partilha.
pub async fn create_link(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
    Json(req): Json<CreateLinkReq>,
) -> Result<Json<ShareLink>, ApiError> {
    let rec: Option<(Uuid,)> = sqlx::query_as("SELECT uploader_id FROM recordings WHERE id = $1")
        .bind(id)
        .fetch_optional(&state.db)
        .await?;
    match rec {
        Some((uploader,)) if uploader == auth.user_id => {}
        Some(_) => return Err(ApiError::Unauthorized),
        None => return Err(ApiError::NotFound),
    }

    let password_hash = if let Some(ref pw) = req.password {
        if pw.is_empty() {
            None
        } else {
            let salt = SaltString::generate(&mut OsRng);
            let hash = Argon2::default()
                .hash_password(pw.as_bytes(), &salt)
                .map_err(ApiError::internal)?
                .to_string();
            Some(hash)
        }
    } else {
        None
    };

    let token = gen_token();
    let link: ShareLink = sqlx::query_as(
        "INSERT INTO recording_share_links (recording_id, token, password_hash, expires_at, created_by)
         VALUES ($1, $2, $3, $4, $5)
         ON CONFLICT (recording_id) DO UPDATE
           SET token = EXCLUDED.token,
               password_hash = EXCLUDED.password_hash,
               expires_at = EXCLUDED.expires_at,
               created_by = EXCLUDED.created_by,
               created_at = now()
         RETURNING id, recording_id, token, expires_at, created_at",
    )
    .bind(id)
    .bind(&token)
    .bind(&password_hash)
    .bind(req.expires_at)
    .bind(auth.user_id)
    .fetch_one(&state.db)
    .await?;

    crate::audit::log(
        &state.db,
        None,
        auth.user_id,
        "recording.link_created",
        &id.to_string(),
    )
    .await;
    Ok(Json(link))
}

/// Devolve o link público existente de uma gravação (sem expor password_hash).
pub async fn get_link(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> Result<Json<Option<ShareLink>>, ApiError> {
    let rec: Option<(Uuid,)> = sqlx::query_as("SELECT uploader_id FROM recordings WHERE id = $1")
        .bind(id)
        .fetch_optional(&state.db)
        .await?;
    match rec {
        Some((uploader,)) if uploader == auth.user_id => {}
        Some(_) => return Err(ApiError::Unauthorized),
        None => return Err(ApiError::NotFound),
    }
    let link: Option<ShareLink> = sqlx::query_as(
        "SELECT id, recording_id, token, expires_at, created_at
         FROM recording_share_links WHERE recording_id = $1",
    )
    .bind(id)
    .fetch_optional(&state.db)
    .await?;
    Ok(Json(link))
}

/// Revoga o link público de partilha.
pub async fn revoke_link(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let rec: Option<(Uuid,)> = sqlx::query_as("SELECT uploader_id FROM recordings WHERE id = $1")
        .bind(id)
        .fetch_optional(&state.db)
        .await?;
    match rec {
        Some((uploader,)) if uploader == auth.user_id => {}
        Some(_) => return Err(ApiError::Unauthorized),
        None => return Err(ApiError::NotFound),
    }
    sqlx::query("DELETE FROM recording_share_links WHERE recording_id = $1")
        .bind(id)
        .execute(&state.db)
        .await?;
    crate::audit::log(
        &state.db,
        None,
        auth.user_id,
        "recording.link_revoked",
        &id.to_string(),
    )
    .await;
    Ok(Json(serde_json::json!({ "ok": true })))
}

#[derive(Deserialize)]
pub struct PublicShareQuery {
    #[serde(default)]
    pub password: Option<String>,
}

/// Acesso público a uma gravação via token (sem autenticação).
pub async fn public_share(
    State(state): State<Arc<AppState>>,
    Path(token): Path<String>,
    Query(q): Query<PublicShareQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let row: Option<(
        Uuid,
        Option<String>,
        Option<DateTime<Utc>>,
        String,
        i64,
        DateTime<Utc>,
    )> = sqlx::query_as(
        r#"SELECT l.recording_id, l.password_hash, l.expires_at,
                      r.filename, r.size_bytes, r.created_at
               FROM recording_share_links l
               JOIN recordings r ON r.id = l.recording_id
               WHERE l.token = $1"#,
    )
    .bind(&token)
    .fetch_optional(&state.db)
    .await?;

    let (rec_id, password_hash, expires_at, filename, size_bytes, created_at) =
        row.ok_or(ApiError::NotFound)?;

    // Verificar expiração.
    if let Some(exp) = expires_at {
        if Utc::now() > exp {
            return Err(ApiError::NotFound);
        }
    }

    // Verificar password.
    if let Some(ref hash) = password_hash {
        let pw = q.password.as_deref().unwrap_or("");
        let parsed = PasswordHash::new(hash).map_err(ApiError::internal)?;
        Argon2::default()
            .verify_password(pw.as_bytes(), &parsed)
            .map_err(|_| ApiError::Unauthorized)?;
    }

    Ok(Json(serde_json::json!({
        "recording_id": rec_id,
        "filename": filename,
        "size_bytes": size_bytes,
        "created_at": created_at,
        "download_url": format!("/api/share/{token}/download"),
        "has_password": password_hash.is_some(),
    })))
}

/// Download via link público (sem autenticação — token é a credencial).
pub async fn public_share_download(
    State(state): State<Arc<AppState>>,
    Path(token): Path<String>,
    Query(q): Query<PublicShareQuery>,
) -> Result<impl IntoResponse, ApiError> {
    let row: Option<(Uuid, Option<String>, Option<DateTime<Utc>>, String)> = sqlx::query_as(
        "SELECT l.recording_id, l.password_hash, l.expires_at, r.filename
             FROM recording_share_links l JOIN recordings r ON r.id = l.recording_id
             WHERE l.token = $1",
    )
    .bind(&token)
    .fetch_optional(&state.db)
    .await?;

    let (rec_id, password_hash, expires_at, filename) = row.ok_or(ApiError::NotFound)?;

    if let Some(exp) = expires_at {
        if Utc::now() > exp {
            return Err(ApiError::NotFound);
        }
    }
    if let Some(ref hash) = password_hash {
        let pw = q.password.as_deref().unwrap_or("");
        let parsed = PasswordHash::new(hash).map_err(ApiError::internal)?;
        Argon2::default()
            .verify_password(pw.as_bytes(), &parsed)
            .map_err(|_| ApiError::Unauthorized)?;
    }

    let path = recordings_dir().join(format!("{rec_id}.webm"));
    let data = tokio::fs::read(&path)
        .await
        .map_err(|_| ApiError::NotFound)?;

    Ok((
        [
            (header::CONTENT_TYPE, "video/webm".to_string()),
            (
                header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"{}\"", filename.replace('"', "")),
            ),
        ],
        data,
    ))
}

/// Lista com quem uma gravação está partilhada (só o dono).
pub async fn shares(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> Result<Json<Vec<UserPublic>>, ApiError> {
    let owner: Option<(Uuid,)> = sqlx::query_as("SELECT uploader_id FROM recordings WHERE id = $1")
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
