//! Facade de aplicação para gravações: orquestra os metadados em Postgres e
//! a porta `domain::ports::RecordingStorage` para os bytes. Os handlers em
//! `recordings.rs` só extraem o pedido, chamam estas funções e mapeiam o
//! resultado para JSON — nunca correm SQL nem tocam em ficheiros. Ver
//! ADR-0004.
//!
//! `RecordingRow` (mapeamento sqlx) e `RecordingView` (contrato JSON) são
//! tipos separados de propósito: uma migração que mude uma coluna de
//! `recordings` parte a compilação deste ficheiro em vez de partir o
//! contrato de API em silêncio — foi exactamente o que aconteceu em
//! `meetings.rs` com a migração 0022 (ver ADR-0004).

use argon2::{
    password_hash::{rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2,
};
use axum::body::Bytes;
use chrono::{DateTime, Utc};
use serde::Serialize;
use std::sync::Arc;
use uuid::Uuid;

use crate::{error::ApiError, rooms::Room, users::UserPublic, AppState};

// ---------- Linhas de BD (só para o repositório mapear) ----------

#[derive(Debug, Clone, sqlx::FromRow)]
struct RecordingRow {
    id: Uuid,
    room_id: Uuid,
    uploader_id: Uuid,
    filename: String,
    size_bytes: i64,
    created_at: DateTime<Utc>,
}

// ---------- DTOs de resposta (o que a API promete ao cliente) ----------

#[derive(Debug, Serialize)]
pub struct RecordingView {
    pub id: Uuid,
    pub room_id: Uuid,
    pub uploader_id: Uuid,
    pub filename: String,
    pub size_bytes: i64,
    pub created_at: DateTime<Utc>,
}

impl From<RecordingRow> for RecordingView {
    fn from(r: RecordingRow) -> Self {
        Self {
            id: r.id,
            room_id: r.room_id,
            uploader_id: r.uploader_id,
            filename: r.filename,
            size_bytes: r.size_bytes,
            created_at: r.created_at,
        }
    }
}

/// Item da biblioteca — já é um modelo de leitura para a UI (um único ponto
/// de consulta, sem reutilização como entidade); mantém FromRow+Serialize
/// no mesmo tipo por não ter o risco de drift que motivou o split acima.
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
    pub owned: bool,
    pub share_count: i64,
    pub can_download: bool,
    pub status: String,
    pub failure_reason: Option<String>,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct ShareLinkView {
    pub id: Uuid,
    pub recording_id: Uuid,
    pub token: String,
    pub expires_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
pub struct PublicShareInfo {
    pub recording_id: Uuid,
    pub filename: String,
    pub size_bytes: i64,
    pub created_at: DateTime<Utc>,
    pub download_url: String,
    pub has_password: bool,
}

// ---------- Helpers de acesso (repositório + regras) ----------

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

async fn fetch_row(state: &AppState, id: Uuid) -> Result<RecordingRow, ApiError> {
    let row: RecordingRow = sqlx::query_as(
        "SELECT id, room_id, uploader_id, filename, size_bytes, created_at FROM recordings WHERE id = $1",
    )
    .bind(id)
    .fetch_one(&state.db)
    .await?;
    Ok(row)
}

/// Acesso a uma gravação: participou na sala, fez upload, ou foi-lhe partilhada.
async fn can_access(state: &AppState, rec: &RecordingRow, user_id: Uuid) -> Result<bool, ApiError> {
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

/// RBAC de download: dono da gravação, ou admin de uma org a que o dono pertence.
async fn can_download(
    state: &AppState,
    rec: &RecordingRow,
    user_id: Uuid,
) -> Result<bool, ApiError> {
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

async fn require_owner(state: &AppState, id: Uuid, user_id: Uuid) -> Result<(), ApiError> {
    let owner: Option<(Uuid,)> = sqlx::query_as("SELECT uploader_id FROM recordings WHERE id = $1")
        .bind(id)
        .fetch_optional(&state.db)
        .await?;
    match owner {
        Some((uploader,)) if uploader == user_id => Ok(()),
        Some(_) => Err(ApiError::Unauthorized),
        None => Err(ApiError::NotFound),
    }
}

// ---------- Casos de uso ----------

pub async fn upload(
    state: &Arc<AppState>,
    room_code: &str,
    uploader_id: Uuid,
    display_name: Option<String>,
    body: Bytes,
) -> Result<RecordingView, ApiError> {
    let room = room_by_code(state, room_code).await?;
    if !is_participant(state, room.id, uploader_id).await? {
        return Err(ApiError::Unauthorized);
    }

    let stamp = Utc::now().format("%Y%m%d-%H%M%S");
    let display = display_name
        .filter(|n| !n.trim().is_empty())
        .unwrap_or_else(|| format!("{}-{stamp}.webm", room.code));

    let row: RecordingRow = sqlx::query_as(
        "INSERT INTO recordings (room_id, uploader_id, filename, size_bytes)
         VALUES ($1, $2, $3, $4)
         RETURNING id, room_id, uploader_id, filename, size_bytes, created_at",
    )
    .bind(room.id)
    .bind(uploader_id)
    .bind(&display)
    .bind(body.len() as i64)
    .fetch_one(&state.db)
    .await?;

    state
        .storage
        .put(row.id, body.clone())
        .await
        .map_err(ApiError::internal)?;

    tracing::info!(room = %room.code, id = %row.id, size = body.len(), "recording stored");
    Ok(row.into())
}

pub async fn list_for_room(
    state: &AppState,
    room_code: &str,
    user_id: Uuid,
) -> Result<Vec<RecordingView>, ApiError> {
    let room = room_by_code(state, room_code).await?;
    if !is_participant(state, room.id, user_id).await? {
        return Err(ApiError::Unauthorized);
    }
    let rows: Vec<RecordingRow> = sqlx::query_as(
        "SELECT id, room_id, uploader_id, filename, size_bytes, created_at
         FROM recordings WHERE room_id = $1 ORDER BY created_at DESC",
    )
    .bind(room.id)
    .fetch_all(&state.db)
    .await?;
    Ok(rows.into_iter().map(Into::into).collect())
}

/// Biblioteca do utilizador: gravações onde participou + partilhadas consigo.
pub async fn library(state: &AppState, user_id: Uuid) -> Result<Vec<RecordingItem>, ApiError> {
    let items: Vec<RecordingItem> = sqlx::query_as(
        r#"
        SELECT r.id, r.room_id, rm.code AS room_code,
               r.uploader_id, u.username AS uploader_name,
               r.filename, r.size_bytes, r.created_at,
               r.status, r.failure_reason,
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
    .bind(user_id)
    .fetch_all(&state.db)
    .await?;
    Ok(items)
}

/// `?dl=1` (as_download=true) exige RBAC de download; sem isso, basta acesso
/// de reprodução. Devolve a view de metadados + os bytes do ficheiro.
pub async fn fetch_for_download(
    state: &AppState,
    id: Uuid,
    user_id: Uuid,
    as_download: bool,
) -> Result<(RecordingView, Bytes), ApiError> {
    let row = fetch_row(state, id).await?;

    // Uma gravação falhada não tem ficheiro — resposta honesta em vez de um
    // 500 opaco quando o `get` da storage falhar mais abaixo.
    let (status, motivo): (String, Option<String>) =
        sqlx::query_as("SELECT status, failure_reason FROM recordings WHERE id = $1")
            .bind(id)
            .fetch_one(&state.db)
            .await?;
    if status != "ready" {
        return Err(ApiError::BadRequest(motivo.unwrap_or_else(|| {
            "Esta gravação falhou e não tem ficheiro.".into()
        })));
    }

    if as_download {
        if !can_download(state, &row, user_id).await? {
            return Err(ApiError::Unauthorized);
        }
    } else if !can_access(state, &row, user_id).await? {
        return Err(ApiError::Unauthorized);
    }

    let bytes = state.storage.get(row.id).await.map_err(|e| match e {
        crate::domain::ports::StorageError::NotFound => ApiError::NotFound,
        other => ApiError::internal(other),
    })?;
    Ok((row.into(), bytes))
}

/// Partilha só-leitura de uma gravação com outro utilizador (só o dono pode).
pub async fn share(
    state: &AppState,
    id: Uuid,
    owner_id: Uuid,
    target_user_id: Uuid,
) -> Result<(), ApiError> {
    let row = fetch_row(state, id).await?;
    if row.uploader_id != owner_id {
        return Err(ApiError::Unauthorized);
    }
    if target_user_id == owner_id {
        return Err(ApiError::BadRequest("cannot share with yourself".into()));
    }
    sqlx::query(
        "INSERT INTO recording_shares (recording_id, user_id, shared_by) VALUES ($1, $2, $3)
         ON CONFLICT (recording_id, user_id) DO NOTHING",
    )
    .bind(id)
    .bind(target_user_id)
    .bind(owner_id)
    .execute(&state.db)
    .await?;
    Ok(())
}

pub async fn unshare(
    state: &AppState,
    id: Uuid,
    owner_id: Uuid,
    target_user_id: Uuid,
) -> Result<(), ApiError> {
    require_owner(state, id, owner_id).await?;
    sqlx::query("DELETE FROM recording_shares WHERE recording_id = $1 AND user_id = $2")
        .bind(id)
        .bind(target_user_id)
        .execute(&state.db)
        .await?;
    Ok(())
}

pub async fn shares_list(
    state: &AppState,
    id: Uuid,
    owner_id: Uuid,
) -> Result<Vec<UserPublic>, ApiError> {
    require_owner(state, id, owner_id).await?;
    let users = sqlx::query_as::<_, UserPublic>(
        "SELECT u.id, u.email, u.username, u.created_at FROM recording_shares s
         JOIN users u ON u.id = s.user_id
         WHERE s.recording_id = $1 ORDER BY u.username",
    )
    .bind(id)
    .fetch_all(&state.db)
    .await?;
    Ok(users)
}

fn gen_token() -> String {
    Uuid::new_v4().to_string().replace('-', "")
}

pub async fn create_link(
    state: &AppState,
    id: Uuid,
    owner_id: Uuid,
    password: Option<String>,
    expires_at: Option<DateTime<Utc>>,
) -> Result<ShareLinkView, ApiError> {
    require_owner(state, id, owner_id).await?;

    let password_hash = match password {
        Some(ref pw) if !pw.is_empty() => {
            let salt = SaltString::generate(&mut OsRng);
            let hash = Argon2::default()
                .hash_password(pw.as_bytes(), &salt)
                .map_err(ApiError::internal)?
                .to_string();
            Some(hash)
        }
        _ => None,
    };

    let token = gen_token();
    let link: ShareLinkView = sqlx::query_as(
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
    .bind(expires_at)
    .bind(owner_id)
    .fetch_one(&state.db)
    .await?;

    crate::audit::log(
        &state.db,
        None,
        owner_id,
        "recording.link_created",
        &id.to_string(),
    )
    .await;
    Ok(link)
}

pub async fn get_link(
    state: &AppState,
    id: Uuid,
    owner_id: Uuid,
) -> Result<Option<ShareLinkView>, ApiError> {
    require_owner(state, id, owner_id).await?;
    let link: Option<ShareLinkView> = sqlx::query_as(
        "SELECT id, recording_id, token, expires_at, created_at
         FROM recording_share_links WHERE recording_id = $1",
    )
    .bind(id)
    .fetch_optional(&state.db)
    .await?;
    Ok(link)
}

pub async fn revoke_link(state: &AppState, id: Uuid, owner_id: Uuid) -> Result<(), ApiError> {
    require_owner(state, id, owner_id).await?;
    sqlx::query("DELETE FROM recording_share_links WHERE recording_id = $1")
        .bind(id)
        .execute(&state.db)
        .await?;
    crate::audit::log(
        &state.db,
        None,
        owner_id,
        "recording.link_revoked",
        &id.to_string(),
    )
    .await;
    Ok(())
}

fn verify_link_password(
    password_hash: &Option<String>,
    given: Option<&str>,
) -> Result<(), ApiError> {
    if let Some(hash) = password_hash {
        let pw = given.unwrap_or("");
        let parsed = PasswordHash::new(hash).map_err(ApiError::internal)?;
        Argon2::default()
            .verify_password(pw.as_bytes(), &parsed)
            .map_err(|_| ApiError::Unauthorized)?;
    }
    Ok(())
}

pub async fn public_share_info(
    state: &AppState,
    token: &str,
    password: Option<&str>,
) -> Result<PublicShareInfo, ApiError> {
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
    .bind(token)
    .fetch_optional(&state.db)
    .await?;

    let (rec_id, password_hash, expires_at, filename, size_bytes, created_at) =
        row.ok_or(ApiError::NotFound)?;

    if let Some(exp) = expires_at {
        if Utc::now() > exp {
            return Err(ApiError::NotFound);
        }
    }
    let has_password = password_hash.is_some();
    verify_link_password(&password_hash, password)?;

    Ok(PublicShareInfo {
        recording_id: rec_id,
        filename,
        size_bytes,
        created_at,
        download_url: format!("/api/share/{token}/download"),
        has_password,
    })
}

pub async fn public_share_bytes(
    state: &AppState,
    token: &str,
    password: Option<&str>,
) -> Result<(String, Bytes), ApiError> {
    let row: Option<(Uuid, Option<String>, Option<DateTime<Utc>>, String)> = sqlx::query_as(
        "SELECT l.recording_id, l.password_hash, l.expires_at, r.filename
             FROM recording_share_links l JOIN recordings r ON r.id = l.recording_id
             WHERE l.token = $1",
    )
    .bind(token)
    .fetch_optional(&state.db)
    .await?;

    let (rec_id, password_hash, expires_at, filename) = row.ok_or(ApiError::NotFound)?;

    if let Some(exp) = expires_at {
        if Utc::now() > exp {
            return Err(ApiError::NotFound);
        }
    }
    verify_link_password(&password_hash, password)?;

    let bytes = state.storage.get(rec_id).await.map_err(|e| match e {
        crate::domain::ports::StorageError::NotFound => ApiError::NotFound,
        other => ApiError::internal(other),
    })?;
    Ok((filename, bytes))
}
