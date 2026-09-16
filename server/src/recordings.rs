//! Gravações de reuniões: upload (webm), biblioteca por utilizador,
//! partilha só-leitura e download.
//!
//! O ficheiro fica no disco (`RECORDINGS_DIR`, por omissão `./recordings`);
//! a base de dados guarda os metadados. Acesso: quem participou na sala
//! (`room_participants`), quem fez o upload, ou com quem foi partilhada
//! (`recording_shares`). Partilha é sempre só-leitura (download). Uma gravação
//! PUBLICADA para a organização (`visibility = 'org'`) é vista também pelos
//! membros activos de uma organização do autor — ver `sql_can_view`.
//!
//! Os sub-recursos do leitor (transcrição, capítulos, legendas, comentários,
//! visualizações, participantes) vivem em `recording_meta.rs` e decidem o
//! acesso SEMPRE por `access` deste módulo.

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

use crate::{
    auth::AuthUser, error::ApiError, media_probe::MediaInfo, rooms::Room, users::UserPublic,
    AppState,
};

pub const MAX_RECORDING_BYTES: usize = 512 * 1024 * 1024;

/// Tipos de sessão de uma gravação (coluna `recordings.kind`, migração 0040).
pub const RECORDING_KINDS: &[&str] = &["meeting", "training", "broadcast", "hybrid"];

fn recordings_dir() -> std::path::PathBuf {
    std::env::var("RECORDINGS_DIR")
        .unwrap_or_else(|_| "recordings".into())
        .into()
}

/// Formato da sala → tipo de sessão da gravação. `normal` é uma reunião.
pub(crate) fn kind_from_room_format(format: &str) -> &'static str {
    match format {
        "training" => "training",
        "broadcast" => "broadcast",
        "hybrid" => "hybrid",
        _ => "meeting",
    }
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
    /// Estado do FICHEIRO: `processing` (a compor), `transcribing` (há
    /// ficheiro; o ai-worker está a transcrever), `ready`, `failed`.
    ///
    /// A entrada falhada existe para ser VISTA: antes, uma gravação que não
    /// compunha desaparecia sem deixar rasto, e quem carregou em «gravar»
    /// ficava a pensar que tinha um ficheiro algures. Ver migração 0036.
    pub status: String,
    /// Causa em linguagem de utilizador. `None` quando não falhou.
    pub failure_reason: Option<String>,
    /// Estado para mostrar: o `status`, com `published` quando está pronta e publicada.
    pub state: String,
    /// Progresso do passo em curso (composição ou transcrição), 0–100.
    pub progress_pct: Option<i16>,
    /// `meeting` | `training` | `broadcast` | `hybrid`.
    pub kind: String,
    /// Medidos com ffprobe; `None` = não foi possível medir.
    pub duration_ms: Option<i64>,
    pub width: Option<i32>,
    pub height: Option<i32>,
    pub fps: Option<f32>,
    pub video_codec: Option<String>,
    pub audio_codec: Option<String>,
    /// Há miniatura em `GET /api/recordings/{id}/thumbnail`.
    pub has_thumbnail: bool,
    /// `none` | `transcribing` | `ready` | `failed`.
    pub transcript_status: String,
    pub transcript_language: Option<String>,
    pub transcribed_at: Option<DateTime<Utc>>,
    pub chapter_count: i64,
    pub comment_count: i64,
    /// Visualizações (uma por pessoa por dia).
    pub view_count: i64,
    pub participant_count: i64,
    /// Línguas com legenda PUBLICADA.
    pub caption_languages: Vec<String>,
    pub description: String,
    pub tags: Vec<String>,
    /// `private` | `org`.
    pub visibility: String,
    pub published_at: Option<DateTime<Utc>>,
    /// Pode editar descrição, etiquetas, capítulos, legendas e publicar.
    pub can_manage: bool,
    /// Organização do autor (a partilhada com quem pede, se houver).
    pub uploader_org_id: Option<Uuid>,
    pub uploader_org_name: Option<String>,
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

/// Sala por código, só para quem nela participou. Um código que não existe e
/// uma sala onde não se esteve dão a mesma resposta.
pub(crate) async fn participated_room(
    state: &AppState,
    code: &str,
    user_id: Uuid,
) -> Result<Room, ApiError> {
    let room = room_by_code(state, code).await?;
    if !is_participant(state, room.id, user_id).await? {
        return Err(ApiError::NotFound);
    }
    Ok(room)
}

// ---------- a regra de acesso, escrita uma vez ----------

/// Predicado SQL «`viewer` pode gerir a gravação `r`»: quem a carregou, ou um
/// admin activo de uma organização do autor (a mesma regra do download).
pub(crate) fn sql_can_manage(viewer: &str) -> String {
    format!(
        "(r.uploader_id = {viewer} OR {})",
        crate::org::sql_active_admin_of(viewer, "r.uploader_id")
    )
}

/// Predicado SQL «`viewer` vê a gravação `r`»: participou na sala, carregou-a,
/// foi-lhe partilhada, pode geri-la, ou está publicada para a organização e
/// `viewer` é membro activo de uma organização do autor.
pub(crate) fn sql_can_view(viewer: &str) -> String {
    format!(
        "(r.uploader_id = {viewer}
          OR EXISTS(SELECT 1 FROM room_participants vp WHERE vp.room_id = r.room_id AND vp.user_id = {viewer})
          OR EXISTS(SELECT 1 FROM recording_shares vs WHERE vs.recording_id = r.id AND vs.user_id = {viewer})
          OR {manage}
          OR (r.visibility = 'org' AND r.published_at IS NOT NULL AND {member}))",
        manage = sql_can_manage(viewer),
        member = crate::org::sql_active_member_with(viewer, "r.uploader_id"),
    )
}

/// O que um pedido pode fazer a uma gravação.
#[derive(Debug)]
pub(crate) struct Access {
    pub id: Uuid,
    pub room_id: Uuid,
    pub status: String,
    pub duration_ms: Option<i64>,
    pub can_manage: bool,
}

impl Access {
    /// Escrever exige gerir. Quem só vê recebe 403 (já sabe que existe).
    pub fn require_manage(&self) -> Result<(), ApiError> {
        if self.can_manage {
            Ok(())
        } else {
            Err(ApiError::Forbidden)
        }
    }

    /// Há ficheiro para ler (pronta, ou pronta e a ser transcrita).
    pub fn has_file(&self) -> bool {
        matches!(self.status.as_str(), "ready" | "transcribing")
    }
}

/// Resolve o acesso de `viewer` à gravação `id`. Quem não a pode ver recebe
/// `404`: não se confirma a outra organização que o id existe.
pub(crate) async fn access(state: &AppState, id: Uuid, viewer: Uuid) -> Result<Access, ApiError> {
    type Row = (Uuid, String, Option<i64>, bool, bool);
    let row: Option<Row> = sqlx::query_as(&format!(
        "SELECT r.room_id, r.status, r.duration_ms, {view}, {manage}
         FROM recordings r WHERE r.id = $1",
        view = sql_can_view("$2"),
        manage = sql_can_manage("$2"),
    ))
    .bind(id)
    .bind(viewer)
    .fetch_optional(&state.db)
    .await?;
    match row {
        Some((room_id, status, duration_ms, true, can_manage)) => Ok(Access {
            id,
            room_id,
            status,
            duration_ms,
            can_manage,
        }),
        _ => Err(ApiError::NotFound),
    }
}

/// Acesso a uma gravação para reproduzir (ver `sql_can_view`).
async fn can_access(state: &AppState, rec: &Recording, user_id: Uuid) -> Result<bool, ApiError> {
    match access(state, rec.id, user_id).await {
        Ok(_) => Ok(true),
        Err(ApiError::NotFound) => Ok(false),
        Err(e) => Err(e),
    }
}

// ---------- recording.ready ----------

/// Uma gravação que ficou pronta, para o `recording.ready`.
pub(crate) struct ReadyRecording<'a> {
    pub id: Uuid,
    pub uploader: Uuid,
    pub filename: &'a str,
    pub size: i64,
    pub room_code: &'a str,
    pub kind: &'a str,
    pub media: &'a MediaInfo,
    /// `server` (gravador do servidor) | `upload`.
    pub source: &'static str,
}

/// Dispara `recording.ready` para as organizações de quem gravou/carregou.
/// Uma só função para o gravador do servidor e para o upload.
pub(crate) async fn fire_recording_ready(state: &Arc<AppState>, r: ReadyRecording<'_>) {
    let ReadyRecording {
        id: rec_id,
        uploader,
        filename,
        size,
        room_code,
        kind,
        media,
        source,
    } = r;
    let orgs = crate::org::orgs_of_user(state, uploader).await;
    if orgs.is_empty() {
        return;
    }
    let mb = size / (1024 * 1024);
    let text = format!("Nova gravação disponível: «{filename}» ({mb} MB)");
    let payload = serde_json::json!({
        "recording_id": rec_id,
        "filename": filename,
        "size_bytes": size,
        "room_code": room_code,
        "kind": kind,
        "source": source,
        "duration_ms": media.duration_ms,
        "width": media.width,
        "height": media.height,
        "fps": media.fps,
        "video_codec": media.video_codec,
        "audio_codec": media.audio_codec,
    });
    for org_id in orgs {
        crate::webhooks::fire(
            state.clone(),
            org_id,
            crate::webhooks::Event {
                name: "recording.ready",
                title: "Delonix Meet".into(),
                text: text.clone(),
                payload: payload.clone(),
            },
        );
    }
}

#[derive(Deserialize)]
pub struct UploadQuery {
    #[serde(default)]
    pub name: Option<String>,
    /// Tipo de sessão declarado por quem carrega (o estúdio envia `broadcast`).
    /// Sem ele, herda o formato da sala.
    #[serde(default)]
    pub kind: Option<String>,
}

/// Resposta do upload: a gravação e o que o servidor mediu no ficheiro.
#[derive(Debug, Serialize)]
pub struct UploadResp {
    #[serde(flatten)]
    pub recording: Recording,
    pub kind: String,
    pub status: String,
    #[serde(flatten)]
    pub media: MediaInfo,
    pub has_thumbnail: bool,
}

pub async fn upload(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(code): Path<String>,
    Query(q): Query<UploadQuery>,
    body: Bytes,
) -> Result<Json<UploadResp>, ApiError> {
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
    let kind = match q.kind.as_deref() {
        None | Some("") => kind_from_room_format(&room.format),
        Some(k) => RECORDING_KINDS
            .iter()
            .copied()
            .find(|x| *x == k)
            .ok_or_else(|| {
                ApiError::BadRequest("kind must be meeting, training, broadcast or hybrid".into())
            })?,
    };

    let stamp = Utc::now().format("%Y%m%d-%H%M%S");
    let display = q
        .name
        .filter(|n| !n.trim().is_empty())
        .unwrap_or_else(|| format!("{}-{stamp}.webm", room.code));

    let rec: Recording = sqlx::query_as(
        "INSERT INTO recordings (room_id, uploader_id, filename, size_bytes, kind)
         VALUES ($1, $2, $3, $4, $5)
         RETURNING id, room_id, uploader_id, filename, size_bytes, created_at",
    )
    .bind(room.id)
    .bind(auth.user_id)
    .bind(&display)
    .bind(body.len() as i64)
    .bind(kind)
    .fetch_one(&state.db)
    .await?;

    let dir = recordings_dir();
    tokio::fs::create_dir_all(&dir)
        .await
        .map_err(ApiError::internal)?;
    let path = dir.join(format!("{}.webm", rec.id));
    tokio::fs::write(&path, &body)
        .await
        .map_err(ApiError::internal)?;

    // Mede antes de responder: quem carrega recebe já a duração e a resolução,
    // e o `recording.ready` sai com elas. Cada passo tem tecto de tempo.
    let media = crate::media_probe::probe_and_store(&state, rec.id, &path).await;
    let has_thumbnail = crate::media_probe::thumbnail_path(&state, rec.id).exists();
    tracing::info!(room = %room.code, id = %rec.id, size = body.len(), "recording stored");
    fire_recording_ready(
        &state,
        ReadyRecording {
            id: rec.id,
            uploader: auth.user_id,
            filename: &rec.filename,
            size: rec.size_bytes,
            room_code: &room.code,
            kind,
            media: &media,
            source: "upload",
        },
    )
    .await;
    Ok(Json(UploadResp {
        recording: rec,
        kind: kind.to_string(),
        status: "ready".into(),
        media,
        has_thumbnail,
    }))
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

/// SELECT de um `RecordingItem` para o utilizador `$1`. O chamador junta o WHERE.
fn item_select_sql() -> String {
    format!(
        r#"
        SELECT r.id, r.room_id, rm.code AS room_code,
               r.uploader_id, u.username AS uploader_name,
               r.filename, r.size_bytes, r.created_at,
               r.status, r.failure_reason,
               CASE WHEN r.status = 'ready' AND r.published_at IS NOT NULL THEN 'published'
                    ELSE r.status END AS state,
               r.progress_pct, r.kind,
               r.duration_ms, r.width, r.height, r.fps, r.video_codec, r.audio_codec,
               r.has_thumbnail,
               CASE WHEN r.status = 'transcribing' THEN 'transcribing'
                    WHEN r.transcribed_at IS NULL THEN 'none'
                    WHEN r.transcript_error IS NOT NULL THEN 'failed'
                    ELSE 'ready' END AS transcript_status,
               r.transcript_language, r.transcribed_at,
               (SELECT COUNT(*) FROM recording_chapters c WHERE c.recording_id = r.id) AS chapter_count,
               (SELECT COUNT(*) FROM recording_comments c WHERE c.recording_id = r.id) AS comment_count,
               (SELECT COUNT(*) FROM recording_views v WHERE v.recording_id = r.id) AS view_count,
               (SELECT COUNT(*) FROM room_participants pp WHERE pp.room_id = r.room_id) AS participant_count,
               COALESCE((SELECT array_agg(cap.lang ORDER BY cap.lang) FROM recording_captions cap
                         WHERE cap.recording_id = r.id AND cap.status = 'published'), '{{}}') AS caption_languages,
               r.description, r.tags, r.visibility, r.published_at,
               (p.user_id IS NOT NULL) AS owned,
               COALESCE(sc.n, 0) AS share_count,
               {manage} AS can_download,
               {manage} AS can_manage,
               uo.id AS uploader_org_id, uo.name AS uploader_org_name
        FROM recordings r
        JOIN rooms rm ON rm.id = r.room_id
        JOIN users u ON u.id = r.uploader_id
        LEFT JOIN room_participants p ON p.room_id = r.room_id AND p.user_id = $1
        LEFT JOIN recording_shares s ON s.recording_id = r.id AND s.user_id = $1
        LEFT JOIN (
            SELECT recording_id, COUNT(*) AS n FROM recording_shares GROUP BY recording_id
        ) sc ON sc.recording_id = r.id
        LEFT JOIN LATERAL ({org}) uo ON true
        "#,
        manage = sql_can_manage("$1"),
        org = crate::org::sql_lateral_org_of("r.uploader_id", "$1"),
    )
}

#[derive(Deserialize, Default)]
pub struct LibraryQuery {
    /// Pesquisa no nome, no autor e na TRANSCRIÇÃO (texto completo).
    #[serde(default)]
    pub q: Option<String>,
    /// `published` = só as publicadas que o utilizador vê (incluindo as da
    /// organização em que não participou). Sem ele, a biblioteca de sempre.
    #[serde(default)]
    pub scope: Option<String>,
}

/// Escapa `%`, `_` e `\` para um `ILIKE` literal.
fn like_pattern(q: &str) -> String {
    let mut out = String::with_capacity(q.len() + 2);
    out.push('%');
    for c in q.chars() {
        if matches!(c, '%' | '_' | '\\') {
            out.push('\\');
        }
        out.push(c);
    }
    out.push('%');
    out
}

/// Biblioteca do utilizador: gravações onde participou + partilhadas consigo.
pub async fn library(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Query(q): Query<LibraryQuery>,
) -> Result<Json<Vec<RecordingItem>>, ApiError> {
    let scope_clause = match q.scope.as_deref() {
        None | Some("") | Some("mine") => {
            "(p.user_id IS NOT NULL OR s.user_id IS NOT NULL OR r.uploader_id = $1)".to_string()
        }
        Some("published") => format!("(r.published_at IS NOT NULL AND {})", sql_can_view("$1")),
        Some(_) => {
            return Err(ApiError::BadRequest(
                "scope must be 'mine' or 'published'".into(),
            ))
        }
    };
    let search: Option<String> =
        q.q.map(|s| s.trim().chars().take(200).collect::<String>())
            .filter(|s| !s.is_empty());
    let sql = format!(
        "{select}
         WHERE {scope_clause}
           AND ($2::text IS NULL
                OR r.search_tsv @@ websearch_to_tsquery('simple', $2)
                OR r.filename ILIKE $3 OR u.username ILIKE $3 OR rm.code ILIKE $3
                OR r.description ILIKE $3 OR $2 = ANY(r.tags))
         ORDER BY r.created_at DESC",
        select = item_select_sql(),
    );
    let items: Vec<RecordingItem> = sqlx::query_as(&sql)
        .bind(auth.user_id)
        .bind(&search)
        .bind(search.as_deref().map(like_pattern))
        .fetch_all(&state.db)
        .await?;
    Ok(Json(items))
}

/// Uma gravação da biblioteca, para o leitor em página inteira.
pub async fn details(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> Result<Json<RecordingItem>, ApiError> {
    item_for(&state, id, auth.user_id).await.map(Json)
}

pub(crate) async fn item_for(
    state: &AppState,
    id: Uuid,
    viewer: Uuid,
) -> Result<RecordingItem, ApiError> {
    let sql = format!(
        "{select} WHERE r.id = $2 AND {view}",
        select = item_select_sql(),
        view = sql_can_view("$1"),
    );
    sqlx::query_as(&sql)
        .bind(viewer)
        .bind(id)
        .fetch_optional(&state.db)
        .await?
        .ok_or(ApiError::NotFound)
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
///
/// O admin que PEDE tem de ser membro activo — um admin arquivado continuava
/// a descarregar as gravações da ex-empresa (auditoria 2026-09-16, S3). O
/// `uploader` NÃO se filtra, de propósito: a gravação de um funcionário que
/// saiu continua a ser da organização, e o admin dela tem de a poder
/// descarregar (retenção, eDiscovery). A regra é `sql_can_manage`, a mesma
/// que a biblioteca aplica.
async fn can_download(state: &AppState, rec: &Recording, user_id: Uuid) -> Result<bool, ApiError> {
    match access(state, rec.id, user_id).await {
        Ok(a) => Ok(a.can_manage),
        Err(ApiError::NotFound) => Ok(false),
        Err(e) => Err(e),
    }
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

    // Uma gravação falhada (ou ainda a compor) não tem ficheiro. Sem esta
    // guarda, o pedido descia até ao `File::open` e voltava um 500 opaco —
    // quando a resposta honesta é dizer que não há nada para descarregar, e
    // porquê. Vem DEPOIS da autorização: o estado de uma gravação alheia
    // também é informação.
    let (status, motivo): (String, Option<String>) =
        sqlx::query_as("SELECT status, failure_reason FROM recordings WHERE id = $1")
            .bind(id)
            .fetch_one(&state.db)
            .await?;
    if status == "processing" {
        return Err(ApiError::Conflict(
            "Esta gravação ainda está a ser processada.".into(),
        ));
    }
    if !matches!(status.as_str(), "ready" | "transcribing") {
        return Err(ApiError::BadRequest(motivo.unwrap_or_else(|| {
            "Esta gravação falhou e não tem ficheiro.".into()
        })));
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
        // `locale` é campo de `UserPublic` — sem ele, sempre 500 (ver users::search).
        "SELECT u.id, u.email, u.username, u.created_at, COALESCE(u.locale, 'pt') AS locale
         FROM recording_shares s
         JOIN users u ON u.id = s.user_id
         WHERE s.recording_id = $1 ORDER BY u.username",
    )
    .bind(id)
    .fetch_all(&state.db)
    .await?;
    Ok(Json(users))
}
