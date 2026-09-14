//! Gravações de reuniões — camada HTTP (fina, ADR-0004): extrai o pedido,
//! chama `application::recording_service`, mapeia o resultado para JSON. A
//! lógica de negócio, SQL e acesso ao storage vivem no serviço; nenhuma rota
//! aqui abaixo corre SQL nem toca em ficheiros directamente.
//!
//! Acesso: quem participou na sala (`room_participants`), quem fez o
//! upload, ou com quem foi partilhada (`recording_shares`). Partilha é
//! sempre só-leitura (download).

use axum::{
    body::Bytes,
    extract::{Path, Query, State},
    http::header,
    response::IntoResponse,
    Json,
};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use std::sync::Arc;
use uuid::Uuid;

use crate::{application::recording_service as service, auth::AuthUser, error::ApiError, AppState};

pub use service::{RecordingItem, RecordingView as Recording, ShareLinkView as ShareLink};

pub const MAX_RECORDING_BYTES: usize = 512 * 1024 * 1024;

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
    let rec = service::upload(&state, &code, auth.user_id, q.name, body).await?;
    Ok(Json(rec))
}

/// Gravações de uma sala específica (painel dentro da reunião).
pub async fn list(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(code): Path<String>,
) -> Result<Json<Vec<Recording>>, ApiError> {
    let recs = service::list_for_room(&state, &code, auth.user_id).await?;
    Ok(Json(recs))
}

/// Biblioteca do utilizador: gravações onde participou + partilhadas consigo.
pub async fn library(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
) -> Result<Json<Vec<RecordingItem>>, ApiError> {
    let items = service::library(&state, auth.user_id).await?;
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

pub async fn download(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
    Query(q): Query<DownloadQuery>,
) -> Result<impl IntoResponse, ApiError> {
    let as_download = q.dl.unwrap_or(0) == 1;
    let (rec, data) = service::fetch_for_download(&state, id, auth.user_id, as_download).await?;
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
    service::share(&state, id, auth.user_id, req.user_id).await?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

/// Remove a partilha com um utilizador.
pub async fn unshare(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path((id, user_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<serde_json::Value>, ApiError> {
    service::unshare(&state, id, auth.user_id, user_id).await?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

/// Lista com quem uma gravação está partilhada (só o dono).
pub async fn shares(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> Result<Json<Vec<crate::users::UserPublic>>, ApiError> {
    let users = service::shares_list(&state, id, auth.user_id).await?;
    Ok(Json(users))
}

// ---------- Links públicos de partilha ----------

#[derive(Deserialize)]
pub struct CreateLinkReq {
    #[serde(default)]
    pub password: Option<String>,
    #[serde(default)]
    pub expires_at: Option<DateTime<Utc>>,
}

/// Cria (ou substitui) um link público de partilha.
pub async fn create_link(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
    Json(req): Json<CreateLinkReq>,
) -> Result<Json<ShareLink>, ApiError> {
    let link = service::create_link(&state, id, auth.user_id, req.password, req.expires_at).await?;
    Ok(Json(link))
}

/// Devolve o link público existente de uma gravação (sem expor password_hash).
pub async fn get_link(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> Result<Json<Option<ShareLink>>, ApiError> {
    let link = service::get_link(&state, id, auth.user_id).await?;
    Ok(Json(link))
}

/// Revoga o link público de partilha.
pub async fn revoke_link(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    service::revoke_link(&state, id, auth.user_id).await?;
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
    let info = service::public_share_info(&state, &token, q.password.as_deref()).await?;
    Ok(Json(serde_json::json!({
        "recording_id": info.recording_id,
        "filename": info.filename,
        "size_bytes": info.size_bytes,
        "created_at": info.created_at,
        "download_url": info.download_url,
        "has_password": info.has_password,
    })))
}

/// Download via link público (sem autenticação — token é a credencial).
pub async fn public_share_download(
    State(state): State<Arc<AppState>>,
    Path(token): Path<String>,
    Query(q): Query<PublicShareQuery>,
) -> Result<impl IntoResponse, ApiError> {
    let (filename, data) =
        service::public_share_bytes(&state, &token, q.password.as_deref()).await?;
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
