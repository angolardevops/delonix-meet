//! Gravações de reuniões: upload (webm), biblioteca por utilizador,
//! partilha só-leitura e download; metadados e estado (G4), capítulos e
//! comentários (G5), pesquisa na transcrição (G6).
//!
//! O ficheiro fica no disco (`config.recordings_dir`, de `RECORDINGS_DIR`);
//! a base de dados guarda os metadados. Acesso: quem participou na sala
//! (`room_participants`), quem fez o upload, ou com quem foi partilhada
//! (`recording_shares`). Partilha é sempre só-leitura (download). Uma gravação
//! PUBLICADA para a organização (`visibility = 'org'`) é vista também pelos
//! membros activos de uma organização do autor — ver `sql_can_view`.
//!
//! **Uma regra de acesso.** As rotas por id lêem os factos com [`load_item`] e
//! decidem com `delonix_meet_domain::content::recording::AccessFacts` —
//! reproduzir, descarregar, gerir, comentar. Um membro arquivado (S3) perde
//! todas de uma vez, porque deixaram de ser cópias.
//!
//! Os sub-recursos do leitor (transcrição, capítulos, legendas, comentários,
//! visualizações, participantes) vivem em `recording_meta.rs` e decidem o
//! acesso SEMPRE por `access` deste módulo.

use axum::{
    body::Bytes,
    extract::{Path, Query, State},
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use chrono::{DateTime, Utc};
use delonix_meet_core::{
    page::{Page, PageRequest},
    DomainError,
};
use delonix_meet_domain::content::recording as rules;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use uuid::Uuid;

use crate::{
    auth::AuthUser, error::ApiError, media_probe::MediaInfo, rooms::Room, users::UserPublic,
    AppState,
};

pub const MAX_RECORDING_BYTES: usize = 512 * 1024 * 1024;

/// Tipos de sessão de uma gravação (coluna `recordings.kind`, migração 0053).
pub const RECORDING_KINDS: &[&str] = &["meeting", "training", "broadcast", "hybrid"];

/// Formato da sala → tipo de sessão da gravação. `normal` é uma reunião.
pub(crate) fn kind_from_room_format(format: &str) -> &'static str {
    match format {
        "training" => "training",
        "broadcast" => "broadcast",
        "hybrid" => "hybrid",
        _ => "meeting",
    }
}

/// Ficheiro webm em bruto (só para o spec).
#[derive(utoipa::ToSchema)]
#[schema(value_type = String, format = Binary)]
#[allow(dead_code)]
pub struct WebmBytes(Vec<u8>);

/// Documentação OpenAPI das rotas deste módulo (`openapi.rs` junta-as).
#[derive(utoipa::OpenApi)]
#[openapi(
    paths(
        details,
        upload,
        list,
        library,
        download,
        share,
        shares,
        unshare,
        get_link,
        create_link,
        revoke_link,
        public_share,
        public_share_download,
        get_metadata,
        update,
        list_chapters,
        create_chapter,
        get_chapter,
        delete_chapter,
        list_comments,
        create_comment,
        get_comment,
        update_comment,
        delete_comment
    ),
    components(schemas(
        Recording,
        RecordingItem,
        RecordingPage,
        LibraryResponse,
        UpdateRecordingReq,
        Chapter,
        ChapterPage,
        CreateChapterReq,
        Comment,
        CommentPage,
        CreateCommentReq,
        UpdateCommentReq,
        ShareReq,
        ShareLink,
        CreateLinkReq,
        PublicShareResp
    ))
)]
pub struct ApiDoc;

#[derive(Debug, Serialize, sqlx::FromRow, utoipa::ToSchema)]
pub struct Recording {
    pub id: Uuid,
    pub room_id: Uuid,
    pub uploader_id: Uuid,
    pub filename: String,
    pub size_bytes: i64,
    pub created_at: DateTime<Utc>,
}

/// Item da biblioteca, enriquecido para a UI.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct RecordingItem {
    pub id: Uuid,
    pub room_id: Uuid,
    pub room_code: String,
    pub uploader_id: Uuid,
    pub uploader_name: String,
    /// Nome do ficheiro (é o nome com que se descarrega).
    pub filename: String,
    /// Título dado por quem gere a gravação; `null` = a UI mostra o `filename`.
    pub title: Option<String>,
    /// `meeting` | `lecture` | `broadcast` | `other`.
    pub category: String,
    /// Duração em segundos; `null` = não se sabe (gravação carregada pelo browser).
    pub duration_secs: Option<i32>,
    /// Resolução do vídeo; `null` = não se sabe, ou só áudio.
    pub width: Option<i32>,
    pub height: Option<i32>,
    pub size_bytes: i64,
    pub created_at: DateTime<Utc>,
    /// True se o utilizador atual é dono (participou/fez upload); false se só partilhada.
    pub owned: bool,
    /// Nº de utilizadores com quem está partilhada (só relevante para o dono).
    pub share_count: i64,
    /// RBAC de download: só o dono da gravação e admins da org do dono podem
    /// descarregar o ficheiro; os restantes só reproduzem.
    pub can_download: bool,
    /// Pode alterar título, categoria e capítulos (dono ou admin activo da org do dono).
    pub can_manage: bool,
    /// Estado do FICHEIRO: `processing` (a compor), `transcribing` (há
    /// ficheiro; o ai-worker está a transcrever), `ready`, `failed`.
    ///
    /// A entrada falhada existe para ser VISTA: antes, uma gravação que não
    /// compunha desaparecia sem deixar rasto, e quem carregou em «gravar»
    /// ficava a pensar que tinha um ficheiro algures. Ver migração 0036.
    pub status: String,
    /// Causa em linguagem de utilizador. `None` quando não falhou.
    pub failure_reason: Option<String>,
    /// Estado derivado: `ready` | `failed` | `transcribing` | `transcribed` |
    /// `transcription_failed`. Não há `processing`: a linha só nasce depois de
    /// o ffmpeg acabar de compor.
    pub processing_state: String,
    /// Excerto com os termos marcados entre `«` e `»`. Só numa pesquisa (`q`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snippet: Option<String>,
}

/// Página da biblioteca (com `q`, `page_size` ou `page_token`).
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct RecordingPage {
    pub items: Vec<RecordingItem>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_page_token: Option<String>,
}

/// Sem parâmetros: a lista inteira (forma herdada, lida pelo web). Com `q`,
/// `page_size` ou `page_token`: uma página.
#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(untagged)]
pub enum LibraryResponse {
    List(Vec<RecordingItem>),
    Page(RecordingPage),
}

/// Uma linha da biblioteca com os factos de acesso de quem pede.
#[derive(Debug, sqlx::FromRow)]
pub(crate) struct ItemRow {
    id: Uuid,
    room_id: Uuid,
    room_code: String,
    uploader_id: Uuid,
    uploader_name: String,
    filename: String,
    title: Option<String>,
    category: String,
    duration_secs: Option<i32>,
    width: Option<i32>,
    height: Option<i32>,
    size_bytes: i64,
    created_at: DateTime<Utc>,
    status: String,
    failure_reason: Option<String>,
    transcribed: bool,
    transcription_failed: bool,
    lease_active: bool,
    is_uploader: bool,
    participant: bool,
    shared: bool,
    org_admin: bool,
    active_member: bool,
    archived_member: bool,
    share_count: i64,
}

impl ItemRow {
    fn facts(&self) -> rules::AccessFacts {
        rules::AccessFacts {
            is_uploader: self.is_uploader,
            participant: self.participant,
            shared: self.shared,
            org_admin: self.org_admin,
            active_member: self.active_member,
            archived_member: self.archived_member,
        }
    }

    fn into_item(self, snippet: Option<String>) -> RecordingItem {
        let facts = self.facts();
        let processing_state = rules::processing_state(rules::ProcessingFacts {
            status: &self.status,
            transcribed: self.transcribed,
            transcription_failed: self.transcription_failed,
            lease_active: self.lease_active,
        })
        .as_str()
        .to_string();
        RecordingItem {
            id: self.id,
            room_id: self.room_id,
            room_code: self.room_code,
            uploader_id: self.uploader_id,
            uploader_name: self.uploader_name,
            filename: self.filename,
            title: self.title,
            category: self.category,
            duration_secs: self.duration_secs,
            width: self.width,
            height: self.height,
            size_bytes: self.size_bytes,
            created_at: self.created_at,
            // `owned` foi sempre «participou na sala» (ver o teste de conteúdo).
            owned: self.participant,
            share_count: self.share_count,
            can_download: facts.can_download(),
            can_manage: facts.can_manage(),
            status: self.status,
            failure_reason: self.failure_reason,
            processing_state,
            snippet,
        }
    }
}

/// Uma linha por gravação, com os factos de acesso de `$1` (quem pede).
///
/// A pertença lê-se UMA vez, numa só junção: admin activo, membro activo e
/// membro arquivado de uma organização do dono. O dono (`o`) não se filtra por
/// `archived_at` — a gravação de quem saiu continua da empresa (S3); quem pede
/// (`me`) sim, pela regra do domínio.
const ITEM_SELECT: &str = r#"
SELECT r.id, r.room_id, rm.code AS room_code, r.uploader_id, u.username AS uploader_name,
       r.filename, r.title, r.category, r.duration_secs, r.width, r.height,
       r.size_bytes, r.created_at, r.status, r.failure_reason,
       (r.transcribed_at IS NOT NULL) AS transcribed,
       (r.transcription_failed_at IS NOT NULL) AS transcription_failed,
       (r.transcription_lease_token IS NOT NULL
        AND r.transcription_lease_expires_at >= now()) AS lease_active,
       (r.uploader_id = $1) AS is_uploader,
       EXISTS(SELECT 1 FROM room_participants p
               WHERE p.room_id = r.room_id AND p.user_id = $1) AS participant,
       EXISTS(SELECT 1 FROM recording_shares s
               WHERE s.recording_id = r.id AND s.user_id = $1) AS shared,
       m.org_admin, m.active_member, m.archived_member,
       (SELECT COUNT(*) FROM recording_shares sc WHERE sc.recording_id = r.id) AS share_count
  FROM recordings r
  JOIN rooms rm ON rm.id = r.room_id
  JOIN users u ON u.id = r.uploader_id
  CROSS JOIN LATERAL (
      SELECT COALESCE(bool_or(me.archived_at IS NULL AND EXISTS (
                 SELECT 1 FROM org_role_effective_capabilities vo
                  WHERE vo.role_id = me.role_id AND vo.capability = 'recordings.view_others'
                    AND vo.org_decision = 'allow')), false) AS org_admin,
             COALESCE(bool_or(me.archived_at IS NULL), false) AS active_member,
             COALESCE(bool_or(me.archived_at IS NOT NULL), false) AS archived_member
        FROM org_members me JOIN org_members o ON o.org_id = me.org_id
       WHERE me.user_id = $1 AND o.user_id = r.uploader_id
  ) m
"#;

/// A visibilidade da biblioteca sobre as colunas de [`ITEM_SELECT`] (alias `i`).
/// É `AccessFacts::can_view` escrita em SQL, porque filtra ANTES de paginar;
/// o teste `library_hides_from_archived_member` prova que as duas concordam.
const LIBRARY_VISIBLE: &str =
    "(i.is_uploader OR i.participant OR i.shared) AND NOT (i.archived_member AND NOT i.active_member)";

/// A gravação `id` com os factos de acesso de `user_id`. `None` = não existe.
pub(crate) async fn load_item(
    state: &AppState,
    id: Uuid,
    user_id: Uuid,
) -> Result<Option<ItemRow>, ApiError> {
    Ok(
        sqlx::query_as::<_, ItemRow>(&format!("{ITEM_SELECT} WHERE r.id = $2"))
            .bind(user_id)
            .bind(id)
            .fetch_optional(&state.db)
            .await?,
    )
}

/// A gravação para quem a pode ver de alguma forma. Não existir e não chegar
/// lá dão a MESMA resposta (`404`): não se confirma que existe.
async fn seen_item(state: &AppState, id: Uuid, user_id: Uuid) -> Result<ItemRow, ApiError> {
    match load_item(state, id, user_id).await? {
        Some(row) if row.facts().can_see() => Ok(row),
        _ => Err(ApiError::NotFound),
    }
}

/// Como [`seen_item`], e além disso tem de a poder gerir (`403` se só a vê).
async fn managed_item(state: &AppState, id: Uuid, user_id: Uuid) -> Result<ItemRow, ApiError> {
    let row = seen_item(state, id, user_id).await?;
    if !row.facts().can_manage() {
        return Err(DomainError::forbidden("recording.not_manager")
            .with_message("só o dono da gravação ou um administrador da organização a altera")
            .into());
    }
    Ok(row)
}

/// Como [`seen_item`], e além disso tem de a poder publicar (partilhas e link
/// público): o dono activo (`can_share`), OU quem tem `recordings.publish` numa
/// organização activa do dono (ADR-0008 §1 — poder sobre gravações de OUTROS).
/// Vê a gravação sem nenhum dos dois → `403 authz.missing_capability`; não a vê
/// → `404` (de `seen_item`).
async fn owned_item(state: &AppState, id: Uuid, user_id: Uuid) -> Result<ItemRow, ApiError> {
    let row = seen_item(state, id, user_id).await?;
    if row.facts().can_share() {
        return Ok(row);
    }
    let cap = delonix_meet_domain::identity::authorization::Capability::RecordingsPublish;
    if crate::org::has_capability_over_colleague(state, user_id, row.uploader_id, cap).await? {
        return Ok(row);
    }
    Err(DomainError::forbidden("authz.missing_capability")
        .with_message("só o dono da gravação, ou quem tem recordings.publish, a partilha")
        .with_field("capability", cap.as_str())
        .into())
}

async fn room_by_code(state: &AppState, code: &str) -> Result<Room, ApiError> {
    let room: Room = sqlx::query_as(&format!(
        "SELECT {} FROM rooms WHERE code = $1",
        crate::rooms::ROOM_COLUMNS
    ))
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

#[derive(Deserialize, utoipa::IntoParams)]
#[into_params(parameter_in = Query)]
pub struct UploadQuery {
    /// Nome de apresentação; omissão `<código>-<AAAAMMDD-HHMMSS>.webm`.
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

/// Carrega uma gravação da sala. O corpo é o ficheiro **em bruto** (não
/// multipart); o `Content-Type` não é verificado. Máximo 512 MiB. Só quem
/// participou na sala pode carregar — senão **401**, não 403.
#[utoipa::path(
    post, path = "/api/rooms/{room_code}/recordings", tag = "recordings",
    security(("session" = [])),
    params(("room_code" = String, Path, description = "Código da sala."), UploadQuery),
    request_body(content = inline(WebmBytes), content_type = "video/webm", description = "Ficheiro webm em bruto."),
    responses(
        (status = 200, body = Recording),
        (status = 400, description = "Corpo vazio.", body = crate::openapi::ErrorBody),
        (status = 401, description = "Sessão inválida.", body = crate::openapi::ErrorBody),
        (status = 403, description = "`room.not_participant`: não participou na sala.", body = crate::openapi::ErrorBody),
        (status = 404, description = "Sala inexistente.", body = crate::openapi::ErrorBody),
        (status = 422, description = "`storage.quota_exceeded`: a gravação não cabe na quota de armazenamento de uma organização do autor. Nada é escrito.", body = crate::openapi::ErrorBody),
        (status = 413, description = "Corpo acima de 512 MiB (rejeitado pelo axum, texto simples)."),
    )
)]
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
        return Err(DomainError::forbidden("room.not_participant")
            .with_message("só quem participou na sala")
            .into());
    }
    // Quota de armazenamento (G3): antes de escrever a linha ou o ficheiro.
    crate::usage::enforce_recording_quota(&state, auth.user_id, body.len() as i64).await?;
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

    let dir = &state.config.recordings_dir;
    tokio::fs::create_dir_all(dir)
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
/// Só para participantes da sala — senão `403 room.not_participant`.
#[utoipa::path(
    get, path = "/api/rooms/{room_code}/recordings", tag = "recordings",
    security(("session" = [])),
    params(("room_code" = String, Path, description = "Código da sala.")),
    responses(
        (status = 200, body = Vec<Recording>, description = "Mais recentes primeiro."),
        (status = 401, description = "Sessão inválida.", body = crate::openapi::ErrorBody),
        (status = 403, description = "`room.not_participant`: não participou na sala.", body = crate::openapi::ErrorBody),
        (status = 404, body = crate::openapi::ErrorBody),
    )
)]
pub async fn list(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(code): Path<String>,
) -> Result<Json<Vec<Recording>>, ApiError> {
    let room = room_by_code(&state, &code).await?;
    if !is_participant(&state, room.id, auth.user_id).await? {
        return Err(DomainError::forbidden("room.not_participant")
            .with_message("só quem participou na sala")
            .into());
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

// NOTA DE MERGE (integra-ui-template-rebuild): a `LibraryQuery` do lado
// `frontend/ui-template-rebuild` tinha `q` + `scope` ('mine' | 'published',
// com `scope=published` a mostrar gravações publicadas para a organização) e
// usava `item_select_sql()`/`sql_can_view` (schema rico: state, progress_pct,
// kind, dimensões, transcript_status, chapter_count, comment_count,
// view_count, participant_count, caption_languages, tags, visibility,
// uploader_org_*). Fica de fora nesta resolução: o `scope=published`
// (biblioteca publicada da organização) NÃO está implementado — ver o
// relatório do merge.
#[derive(Deserialize, utoipa::IntoParams)]
#[into_params(parameter_in = Query)]
pub struct LibraryQuery {
    /// Pesquisa de texto no título, no nome do ficheiro e na transcrição. Cada
    /// palavra conta como prefixo e todas têm de aparecer. Só letras e dígitos.
    pub q: Option<String>,
    /// 1-100, omissão 50. Com este parâmetro (ou `q`/`page_token`) a resposta é uma página.
    pub page_size: Option<u32>,
    pub page_token: Option<String>,
}

#[derive(Serialize, Deserialize)]
struct LibraryCursor {
    at: DateTime<Utc>,
    id: Uuid,
}

/// Biblioteca do utilizador: gravações onde participou + partilhadas consigo.
///
/// **Duas formas, de propósito.** Sem parâmetros devolve a lista inteira, como
/// sempre — é o que o web lê hoje (`recordingsLibrary`), e trocá-la no mesmo
/// PR partia a página. Com `q`, `page_size` ou `page_token` devolve uma página
/// (`items` + `next_page_token`), por `created_at` descendente — também numa
/// pesquisa, para o cursor ser estável; a relevância só decide o `snippet`.
/// A forma sem limite é dívida e sai quando o web passar a paginar.
///
/// Um membro arquivado (S3) deixa de ver as gravações da ex-organização.
#[utoipa::path(
    get, path = "/api/recordings", tag = "recordings",
    security(("session" = [])),
    params(LibraryQuery),
    responses(
        (status = 200, body = LibraryResponse,
         description = "Sem parâmetros: `RecordingItem[]` (todas). Com `q`/`page_size`/`page_token`: `RecordingPage`. Inclui as falhadas (`status = failed`)."),
        (status = 400, body = crate::openapi::ErrorBody, description = "`page_token` inválido, ou `q` sem nenhuma letra ou dígito (`recording.invalid_query`)."),
        (status = 401, body = crate::openapi::ErrorBody),
    )
)]
pub async fn library(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Query(q): Query<LibraryQuery>,
) -> Result<Json<LibraryResponse>, ApiError> {
    if q.q.is_none() && q.page_size.is_none() && q.page_token.is_none() {
        let rows: Vec<ItemRow> = sqlx::query_as(&format!(
            "SELECT * FROM ({ITEM_SELECT}) i WHERE {LIBRARY_VISIBLE}
              ORDER BY i.created_at DESC, i.id DESC"
        ))
        .bind(auth.user_id)
        .fetch_all(&state.db)
        .await?;
        return Ok(Json(LibraryResponse::List(
            rows.into_iter().map(|r| r.into_item(None)).collect(),
        )));
    }

    // Um `q` vazio não filtra; um `q` só com pontuação é erro do cliente, não
    // «tudo» nem «nada» em silêncio.
    let tsquery = match q.q.as_deref().map(str::trim).filter(|t| !t.is_empty()) {
        None => None,
        Some(text) => Some(rules::search_query(text).ok_or_else(|| {
            DomainError::invalid(
                "recording.invalid_query",
                "a pesquisa tem de ter pelo menos uma letra ou dígito",
            )
            .with_field("q", "letras e dígitos")
        })?),
    };
    let page = PageRequest {
        page_size: q.page_size,
        page_token: q.page_token,
    };
    let size = page.size();
    let cursor: Option<LibraryCursor> = page.cursor()?;
    // Duas instruções distintas (com e sem texto) em vez de `$2 IS NULL OR …`:
    // um plano genérico com o OR deixava de usar o índice GIN.
    let search = if tsquery.is_some() {
        "r.search_vector @@ to_tsquery('simple', $5)"
    } else {
        "$5::text IS NULL"
    };
    let rows: Vec<ItemRow> = sqlx::query_as(&format!(
        "SELECT * FROM ({ITEM_SELECT}
            WHERE {search}
              AND ($2::timestamptz IS NULL OR (r.created_at, r.id) < ($2, $3))
         ) i
         WHERE {LIBRARY_VISIBLE}
         ORDER BY i.created_at DESC, i.id DESC
         LIMIT $4"
    ))
    .bind(auth.user_id)
    .bind(cursor.as_ref().map(|c| c.at))
    .bind(cursor.as_ref().map(|c| c.id).unwrap_or_default())
    .bind(size as i64 + 1)
    .bind(tsquery.as_deref())
    .fetch_all(&state.db)
    .await?;
    let p = Page::from_overfetch(rows, size, |r| LibraryCursor {
        at: r.created_at,
        id: r.id,
    });

    // O excerto só para a página devolvida (≤ 100 linhas): o `ts_headline`
    // relê o texto inteiro, e fazê-lo antes do LIMIT seria por cada candidata.
    let mut snippets: std::collections::HashMap<Uuid, String> = Default::default();
    if let Some(tsq) = &tsquery {
        let ids: Vec<Uuid> = p.items.iter().map(|r| r.id).collect();
        let found: Vec<(Uuid, String)> = sqlx::query_as(
            r#"SELECT r.id, ts_headline('simple',
                        CASE WHEN to_tsvector('simple', r.transcript) @@ q.q
                             THEN r.transcript
                             ELSE coalesce(r.title, '') || ' ' || r.filename END,
                        q.q,
                        'MaxFragments=1, MaxWords=18, MinWords=6, StartSel="«", StopSel="»"')
                 FROM recordings r, to_tsquery('simple', $2) AS q(q)
                WHERE r.id = ANY($1)"#,
        )
        .bind(&ids)
        .bind(tsq)
        .fetch_all(&state.db)
        .await?;
        snippets.extend(found);
    }
    Ok(Json(LibraryResponse::Page(RecordingPage {
        items: p
            .items
            .into_iter()
            .map(|r| {
                let snippet = snippets.remove(&r.id);
                r.into_item(snippet)
            })
            .collect(),
        next_page_token: p.next_page_token,
    })))
}

/// Uma gravação da biblioteca, para o leitor em página inteira.
#[utoipa::path(
    get, path = "/api/recordings/{recording_id}/details", tag = "recordings",
    security(("session" = [])),
    params(("recording_id" = Uuid, Path, description = "Gravação.")),
    responses(
        (status = 200, body = RecordingItem, description = "Uma gravação da biblioteca, para o leitor em página inteira."),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 404, body = crate::openapi::ErrorBody),
    )
)]
pub async fn details(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> Result<Json<RecordingItem>, ApiError> {
    item_for(&state, id, auth.user_id).await.map(Json)
}

// NOTA DE MERGE (integra-ui-template-rebuild): a versão original desta função
// (lado `frontend/ui-template-rebuild`) fazia a sua própria consulta com
// `item_select_sql()`/`sql_can_view`, um caminho de acesso PARALELO ao de
// `seen_item`/`AccessFacts`. Reescrita para passar pela MESMA regra de acesso
// que o resto do ficheiro usa (`seen_item` + `ItemRow::into_item`) em vez de
// duplicar a verificação — ver a filosofia de resolução no relatório do merge.
pub(crate) async fn item_for(
    state: &AppState,
    id: Uuid,
    viewer: Uuid,
) -> Result<RecordingItem, ApiError> {
    Ok(seen_item(state, id, viewer).await?.into_item(None))
}

/// `?dl=1` pede o ficheiro para DESCARREGAR (attachment); sem isso, é para
/// REPRODUZIR inline. Descarregar exige RBAC (dono + admin da org); reproduzir
/// basta ter acesso (participante/partilhado/dono).
#[derive(Deserialize, utoipa::IntoParams)]
#[into_params(parameter_in = Query)]
pub struct DownloadQuery {
    /// `1` = descarregar (`attachment`, exige dono ou admin da org do dono);
    /// outro valor ou ausente = reproduzir `inline` (basta ter acesso).
    #[serde(default)]
    pub dl: Option<i32>,
}

/// O ficheiro webm de uma gravação, para reproduzir ou descarregar (`?dl=1`).
///
/// Os metadados em JSON estão em `GET /api/recordings/{id}/metadata`: este
/// caminho é o `src` do leitor de vídeo e o link de download do web, e não
/// muda de representação.
#[utoipa::path(
    get, path = "/api/recordings/{recording_id}/content", tag = "recordings",
    security(("session" = [])),
    params(("recording_id" = Uuid, Path), DownloadQuery),
    responses(
        (status = 200, body = inline(WebmBytes), content_type = "video/webm",
         description = "`Content-Disposition: inline`, ou `attachment` com `dl=1`."),
        (status = 400, description = "A gravação falhou e não tem ficheiro (mensagem = causa). Só para quem chega à gravação.", body = crate::openapi::ErrorBody),
        (status = 401, description = "Sessão inválida.", body = crate::openapi::ErrorBody),
        (status = 403, description = "`recording.download_forbidden`: chega à gravação mas não pode descarregar (`dl=1`).", body = crate::openapi::ErrorBody),
        (status = 404, description = "Gravação inexistente, sem acesso (inclui membro arquivado), ou ficheiro em falta no disco.", body = crate::openapi::ErrorBody),
    )
)]
pub async fn download(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
    Query(q): Query<DownloadQuery>,
) -> Result<impl IntoResponse, ApiError> {
    // Quem não chega à gravação recebe o 404 de «não existe» ANTES de qualquer
    // outra resposta: o `400` de gravação falhada levava o motivo da falha a
    // utilizadores de outra organização.
    let rec = seen_item(&state, id, auth.user_id).await?;
    // Uma gravação falhada não tem ficheiro. Sem esta guarda, o pedido descia
    // até ao `File::open` e voltava um 500 opaco — quando a resposta honesta é
    // dizer que não há nada para descarregar, e porquê.
    if rec.status != "ready" {
        return Err(ApiError::BadRequest(rec.failure_reason.unwrap_or_else(
            || "Esta gravação falhou e não tem ficheiro.".into(),
        )));
    }

    let facts = rec.facts();
    let as_download = q.dl.unwrap_or(0) == 1;
    // Ficheiro para guardar: exige a permissão de download (RBAC).
    // Reprodução inline: basta ter acesso à gravação.
    let allowed = if as_download {
        facts.can_download()
    } else {
        facts.can_view()
    };
    if !allowed {
        // Chega a ela (`seen_item`) mas não tem esta permissão.
        return Err(DomainError::forbidden("recording.download_forbidden")
            .with_message("só o dono ou um administrador da organização descarrega o ficheiro")
            .into());
    }

    let path = state.config.recordings_dir.join(format!("{}.webm", rec.id));
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

#[derive(Deserialize, utoipa::ToSchema)]
#[schema(as = RecordingShareReq)]
pub struct ShareReq {
    /// Utilizador com quem partilhar (só-leitura). Não é verificado que exista
    /// nem que seja da mesma organização.
    pub user_id: Uuid,
}

/// Partilha só-leitura de uma gravação com outro utilizador.
/// Apenas quem fez o upload (o "dono") pode partilhar. Idempotente.
#[utoipa::path(
    post, path = "/api/recordings/{recording_id}/shares", tag = "recordings",
    security(("session" = [])),
    params(("recording_id" = Uuid, Path)),
    request_body = ShareReq,
    responses(
        (status = 201, body = crate::users::UserPublic, description = "Partilha criada. `Location: /api/recordings/{recording_id}/shares/{user_id}`."),
        (status = 200, body = crate::users::UserPublic, description = "Já estava partilhada com essa pessoa (idempotente)."),
        (status = 400, description = "Partilhar consigo próprio.", body = crate::openapi::ErrorBody),
        (status = 401, description = "Sessão inválida.", body = crate::openapi::ErrorBody),
        (status = 403, description = "`authz.missing_capability` (`recordings.publish`): vê a gravação mas não é o dono activo nem tem a capacidade.", body = crate::openapi::ErrorBody),
        (status = 404, description = "A gravação não existe ou não lhe chega; ou o utilizador destino não existe.", body = crate::openapi::ErrorBody),
    )
)]
pub async fn share(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
    Json(req): Json<ShareReq>,
) -> Result<Response, ApiError> {
    owned_item(&state, id, auth.user_id).await?;
    if req.user_id == auth.user_id {
        return Err(ApiError::BadRequest("cannot share with yourself".into()));
    }
    // Antes era um 500 (chave estrangeira) para um id que não existe.
    let target = crate::users::fetch_public(&state.db, req.user_id)
        .await
        .map_err(|_| ApiError::NotFound)?;
    let res = sqlx::query(
        "INSERT INTO recording_shares (recording_id, user_id, shared_by) VALUES ($1, $2, $3)
         ON CONFLICT (recording_id, user_id) DO NOTHING",
    )
    .bind(id)
    .bind(req.user_id)
    .bind(auth.user_id)
    .execute(&state.db)
    .await?;
    if res.rows_affected() == 0 {
        return Ok(Json(target).into_response());
    }
    let location = format!("/api/recordings/{id}/shares/{}", req.user_id);
    Ok((
        StatusCode::CREATED,
        [(header::LOCATION, location)],
        Json(target),
    )
        .into_response())
}

/// Remove a partilha com um utilizador (só o dono).
#[utoipa::path(
    delete, path = "/api/recordings/{recording_id}/shares/{user_id}", tag = "recordings",
    security(("session" = [])),
    params(("recording_id" = Uuid, Path), ("user_id" = Uuid, Path)),
    responses(
        (status = 204, description = "Partilha removida."),
        (status = 401, description = "Sessão inválida.", body = crate::openapi::ErrorBody),
        (status = 403, description = "`authz.missing_capability` (`recordings.publish`).", body = crate::openapi::ErrorBody),
        (status = 404, description = "A gravação não existe/não lhe chega, ou não estava partilhada com essa pessoa.", body = crate::openapi::ErrorBody),
    )
)]
pub async fn unshare(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path((id, user_id)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, ApiError> {
    owned_item(&state, id, auth.user_id).await?;
    let res = sqlx::query("DELETE FROM recording_shares WHERE recording_id = $1 AND user_id = $2")
        .bind(id)
        .bind(user_id)
        .execute(&state.db)
        .await?;
    if res.rows_affected() == 0 {
        return Err(ApiError::NotFound);
    }
    Ok(StatusCode::NO_CONTENT)
}

// ---------- Links públicos de partilha ----------

/// Link público de uma gravação. O hash da password nunca sai.
#[derive(Debug, Serialize, sqlx::FromRow, utoipa::ToSchema)]
#[schema(as = RecordingShareLink)]
pub struct ShareLink {
    pub id: Uuid,
    pub recording_id: Uuid,
    pub token: String,
    pub expires_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

#[derive(Deserialize, utoipa::ToSchema)]
#[schema(as = RecordingLinkReq)]
pub struct CreateLinkReq {
    /// Password opcional; vazia = sem password.
    #[serde(default)]
    pub password: Option<String>,
    #[serde(default)]
    pub expires_at: Option<DateTime<Utc>>,
}

/// 128 bits do SO, em hexadecimal (32 caracteres — a mesma forma do UUID sem
/// hífens que se usava antes, por isso os links já emitidos continuam válidos).
fn gen_token() -> String {
    delonix_meet_core::crypto::random_hex(16)
}

/// Cria (ou substitui) um link público de partilha. Substituir roda o token:
/// o link anterior deixa de funcionar.
#[utoipa::path(
    put, path = "/api/recordings/{recording_id}/public-link", tag = "recordings",
    security(("session" = [])),
    params(("recording_id" = Uuid, Path)),
    request_body = CreateLinkReq,
    responses(
        (status = 200, body = ShareLink),
        (status = 401, description = "Sessão inválida.", body = crate::openapi::ErrorBody),
        (status = 403, description = "`authz.missing_capability` (`recordings.publish`): vê a gravação mas não é o dono activo nem tem a capacidade.", body = crate::openapi::ErrorBody),
        (status = 404, body = crate::openapi::ErrorBody),
    )
)]
pub async fn create_link(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
    Json(req): Json<CreateLinkReq>,
) -> Result<Json<ShareLink>, ApiError> {
    owned_item(&state, id, auth.user_id).await?;

    let password_hash = if let Some(ref pw) = req.password {
        if pw.is_empty() {
            None
        } else {
            Some(crate::auth::hash_password(pw)?)
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
#[utoipa::path(
    get, path = "/api/recordings/{recording_id}/public-link", tag = "recordings",
    security(("session" = [])),
    params(("recording_id" = Uuid, Path)),
    responses(
        (status = 200, body = Option<ShareLink>, description = "`null` se não houver link."),
        (status = 401, description = "Sessão inválida.", body = crate::openapi::ErrorBody),
        (status = 403, description = "`authz.missing_capability` (`recordings.publish`): vê a gravação mas não é o dono activo nem tem a capacidade.", body = crate::openapi::ErrorBody),
        (status = 404, body = crate::openapi::ErrorBody),
    )
)]
pub async fn get_link(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> Result<Json<Option<ShareLink>>, ApiError> {
    owned_item(&state, id, auth.user_id).await?;
    let link: Option<ShareLink> = sqlx::query_as(
        "SELECT id, recording_id, token, expires_at, created_at
         FROM recording_share_links WHERE recording_id = $1",
    )
    .bind(id)
    .fetch_optional(&state.db)
    .await?;
    Ok(Json(link))
}

/// Revoga o link público de partilha. Idempotente.
#[utoipa::path(
    delete, path = "/api/recordings/{recording_id}/public-link", tag = "recordings",
    security(("session" = [])),
    params(("recording_id" = Uuid, Path)),
    responses(
        (status = 204, description = "Link revogado."),
        (status = 401, description = "Sessão inválida.", body = crate::openapi::ErrorBody),
        (status = 403, description = "`authz.missing_capability` (`recordings.publish`).", body = crate::openapi::ErrorBody),
        (status = 404, description = "A gravação não existe/não lhe chega, ou não tinha link.", body = crate::openapi::ErrorBody),
    )
)]
pub async fn revoke_link(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
    owned_item(&state, id, auth.user_id).await?;
    let res = sqlx::query("DELETE FROM recording_share_links WHERE recording_id = $1")
        .bind(id)
        .execute(&state.db)
        .await?;
    if res.rows_affected() == 0 {
        return Err(ApiError::NotFound);
    }
    crate::audit::log(
        &state.db,
        None,
        auth.user_id,
        "recording.link_revoked",
        &id.to_string(),
    )
    .await;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize, utoipa::IntoParams)]
#[into_params(parameter_in = Query)]
pub struct PublicShareQuery {
    /// Password do link, se tiver. Vai na query string.
    #[serde(default)]
    pub password: Option<String>,
}

/// Metadados de uma gravação partilhada por link público.
#[derive(Serialize, utoipa::ToSchema)]
pub struct PublicShareResp {
    pub recording_id: Uuid,
    pub filename: String,
    pub size_bytes: i64,
    pub created_at: DateTime<Utc>,
    /// `/api/share/<token>/download`.
    pub download_url: String,
    pub has_password: bool,
}

/// Acesso público a uma gravação via token (sem autenticação).
///
/// Link expirado responde como inexistente (404).
#[utoipa::path(
    get, path = "/api/public/recordings/{token}", tag = "recordings",
    params(("token" = String, Path, description = "Token do link público."), PublicShareQuery),
    responses(
        (status = 200, body = PublicShareResp),
        (status = 401, description = "O link tem password e a dada (ou a sua falta) não confere.", body = crate::openapi::ErrorBody),
        (status = 404, description = "Token inexistente ou expirado.", body = crate::openapi::ErrorBody),
    )
)]
pub async fn public_share(
    State(state): State<Arc<AppState>>,
    Path(token): Path<String>,
    Query(q): Query<PublicShareQuery>,
) -> Result<Json<PublicShareResp>, ApiError> {
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
        // Um hash ilegível na base conta como password errada (falha fechado).
        let pw = q.password.as_deref().unwrap_or("");
        if !crate::auth::verify_password(pw, hash) {
            return Err(ApiError::Unauthorized);
        }
    }

    Ok(Json(PublicShareResp {
        recording_id: rec_id,
        filename,
        size_bytes,
        created_at,
        download_url: format!("/api/public/recordings/{token}/content"),
        has_password: password_hash.is_some(),
    }))
}

/// Download via link público (sem autenticação — token é a credencial).
#[utoipa::path(
    get, path = "/api/public/recordings/{token}/content", tag = "recordings",
    params(("token" = String, Path, description = "Token do link público."), PublicShareQuery),
    responses(
        (status = 200, body = inline(WebmBytes), content_type = "video/webm", description = "Sempre `Content-Disposition: attachment`."),
        (status = 401, description = "Password em falta ou errada.", body = crate::openapi::ErrorBody),
        (status = 404, description = "Token inexistente, expirado, ou sem ficheiro (gravação falhada).", body = crate::openapi::ErrorBody),
    )
)]
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
        // Um hash ilegível na base conta como password errada (falha fechado).
        let pw = q.password.as_deref().unwrap_or("");
        if !crate::auth::verify_password(pw, hash) {
            return Err(ApiError::Unauthorized);
        }
    }

    let path = state.config.recordings_dir.join(format!("{rec_id}.webm"));
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
#[utoipa::path(
    get, path = "/api/recordings/{recording_id}/shares", tag = "recordings",
    security(("session" = [])),
    params(("recording_id" = Uuid, Path)),
    responses(
        (status = 200, body = Vec<crate::users::UserPublic>),
        (status = 401, description = "Sessão inválida.", body = crate::openapi::ErrorBody),
        (status = 403, description = "`authz.missing_capability` (`recordings.publish`): vê a gravação mas não é o dono activo nem tem a capacidade.", body = crate::openapi::ErrorBody),
        (status = 404, body = crate::openapi::ErrorBody),
    )
)]
pub async fn shares(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> Result<Json<Vec<UserPublic>>, ApiError> {
    owned_item(&state, id, auth.user_id).await?;
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

// ---------- Metadados (G4) ----------

/// Metadados de uma gravação — o mesmo item da biblioteca.
#[utoipa::path(
    get, path = "/api/recordings/{recording_id}", tag = "recordings",
    security(("session" = [])),
    params(("recording_id" = Uuid, Path)),
    responses(
        (status = 200, body = RecordingItem),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 404, description = "Inexistente, ou sem acesso (inclui membro arquivado).", body = crate::openapi::ErrorBody),
    )
)]
pub async fn get_metadata(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> Result<Json<RecordingItem>, ApiError> {
    Ok(Json(
        seen_item(&state, id, auth.user_id).await?.into_item(None),
    ))
}

#[derive(Deserialize, Default, utoipa::ToSchema)]
pub struct UpdateRecordingReq {
    /// 1-120 caracteres, uma linha. `""` apaga o título (a UI volta ao `filename`).
    pub title: Option<String>,
    /// `meeting` | `lecture` | `broadcast` | `other`.
    pub category: Option<String>,
}

/// Altera título e categoria. Só o dono ou um admin activo da org do dono.
#[utoipa::path(
    patch, path = "/api/recordings/{recording_id}", tag = "recordings",
    security(("session" = [])),
    params(("recording_id" = Uuid, Path)),
    request_body = UpdateRecordingReq,
    responses(
        (status = 200, body = RecordingItem),
        (status = 400, body = crate::openapi::ErrorBody, description = "`recording.invalid_title` / `recording.invalid_category`"),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 403, body = crate::openapi::ErrorBody, description = "Vê a gravação mas não a gere (`recording.not_manager`)."),
        (status = 404, body = crate::openapi::ErrorBody),
    )
)]
pub async fn update(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
    Json(req): Json<UpdateRecordingReq>,
) -> Result<Json<RecordingItem>, ApiError> {
    managed_item(&state, id, auth.user_id).await?;
    // Valida tudo ANTES de escrever (sem escritas parciais).
    let title = req
        .title
        .as_deref()
        .map(rules::validate_title)
        .transpose()?;
    let category = req
        .category
        .as_deref()
        .map(rules::Category::parse)
        .transpose()?;
    sqlx::query(
        "UPDATE recordings
            SET title = CASE WHEN $2 THEN $3 ELSE title END,
                category = COALESCE($4, category)
          WHERE id = $1",
    )
    .bind(id)
    .bind(title.is_some())
    .bind(title.flatten())
    .bind(category.map(|c| c.as_str()))
    .execute(&state.db)
    .await?;
    crate::audit::log(
        &state.db,
        None,
        auth.user_id,
        "recording.updated",
        &id.to_string(),
    )
    .await;
    Ok(Json(
        seen_item(&state, id, auth.user_id).await?.into_item(None),
    ))
}

// ---------- Capítulos (G5) ----------

#[derive(Debug, Serialize, sqlx::FromRow, utoipa::ToSchema)]
#[schema(as = RecordingChapter)]
pub struct Chapter {
    pub id: Uuid,
    pub recording_id: Uuid,
    pub at_secs: i32,
    pub title: String,
    pub created_by: Uuid,
    pub created_at: DateTime<Utc>,
}

const CHAPTER_COLUMNS: &str = "id, recording_id, at_secs, title, created_by, created_at";

#[derive(Serialize, utoipa::ToSchema)]
#[schema(as = RecordingChapterPage)]
pub struct ChapterPage {
    pub items: Vec<Chapter>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_page_token: Option<String>,
}

#[derive(Deserialize, utoipa::ToSchema)]
#[schema(as = RecordingChapterReq)]
pub struct CreateChapterReq {
    /// Segundos desde o início; `0..=duration_secs` (ou `0..=172800` sem duração).
    pub at_secs: i32,
    /// 1-120 caracteres, uma linha.
    pub title: String,
}

#[derive(Deserialize, utoipa::IntoParams)]
#[into_params(parameter_in = Query)]
pub struct PageQuery {
    /// 1-100, omissão 50.
    pub page_size: Option<u32>,
    pub page_token: Option<String>,
}

#[derive(Serialize, Deserialize)]
struct ChapterCursor {
    at: i32,
    id: Uuid,
}

/// Capítulos, por marca temporal. Quem vê a gravação.
#[utoipa::path(
    get, path = "/api/recordings/{recording_id}/chapters", tag = "recordings",
    security(("session" = [])),
    params(("recording_id" = Uuid, Path), PageQuery),
    responses(
        (status = 200, body = ChapterPage),
        (status = 400, body = crate::openapi::ErrorBody, description = "page_token inválido"),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 404, body = crate::openapi::ErrorBody),
    )
)]
pub async fn list_chapters(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
    Query(q): Query<PageQuery>,
) -> Result<Json<ChapterPage>, ApiError> {
    seen_item(&state, id, auth.user_id).await?;
    let page = PageRequest {
        page_size: q.page_size,
        page_token: q.page_token,
    };
    let size = page.size();
    let cursor: Option<ChapterCursor> = page.cursor()?;
    let rows: Vec<Chapter> = sqlx::query_as(&format!(
        "SELECT {CHAPTER_COLUMNS} FROM recording_chapters
          WHERE recording_id = $1
            AND ($2::int IS NULL OR (at_secs, id) > ($2, $3))
          ORDER BY at_secs, id
          LIMIT $4"
    ))
    .bind(id)
    .bind(cursor.as_ref().map(|c| c.at))
    .bind(cursor.as_ref().map(|c| c.id).unwrap_or_default())
    .bind(size as i64 + 1)
    .fetch_all(&state.db)
    .await?;
    let p = Page::from_overfetch(rows, size, |c| ChapterCursor {
        at: c.at_secs,
        id: c.id,
    });
    Ok(Json(ChapterPage {
        items: p.items,
        next_page_token: p.next_page_token,
    }))
}

/// Cria um capítulo. Só o dono ou um admin activo da org do dono.
#[utoipa::path(
    post, path = "/api/recordings/{recording_id}/chapters", tag = "recordings",
    security(("session" = [])),
    params(("recording_id" = Uuid, Path)),
    request_body = CreateChapterReq,
    responses(
        (status = 201, body = Chapter, headers(("Location" = String))),
        (status = 400, body = crate::openapi::ErrorBody, description = "`recording.invalid_chapter_title` / `recording.invalid_timestamp`"),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 403, body = crate::openapi::ErrorBody, description = "`recording.not_manager`"),
        (status = 404, body = crate::openapi::ErrorBody),
        (status = 422, body = crate::openapi::ErrorBody, description = "`recording.too_many_chapters` (máx. 100)"),
    )
)]
pub async fn create_chapter(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
    Json(req): Json<CreateChapterReq>,
) -> Result<Response, ApiError> {
    let rec = managed_item(&state, id, auth.user_id).await?;
    let title = rules::validate_chapter_title(&req.title)?;
    let at_secs = rules::validate_at_secs(req.at_secs, rec.duration_secs)?;
    // O tecto e a inserção numa só instrução.
    let chapter: Option<Chapter> = sqlx::query_as(&format!(
        "INSERT INTO recording_chapters (recording_id, at_secs, title, created_by)
         SELECT $1, $2, $3, $4
          WHERE (SELECT COUNT(*) FROM recording_chapters WHERE recording_id = $1) < $5
         RETURNING {CHAPTER_COLUMNS}"
    ))
    .bind(id)
    .bind(at_secs)
    .bind(&title)
    .bind(auth.user_id)
    .bind(rules::MAX_CHAPTERS)
    .fetch_optional(&state.db)
    .await?;
    let chapter = chapter.ok_or_else(|| {
        DomainError::precondition(
            "recording.too_many_chapters",
            format!(
                "uma gravação tem no máximo {} capítulos",
                rules::MAX_CHAPTERS
            ),
        )
    })?;
    crate::audit::log(
        &state.db,
        None,
        auth.user_id,
        "recording.chapter_created",
        &id.to_string(),
    )
    .await;
    let location = format!("/api/recordings/{id}/chapters/{}", chapter.id);
    Ok((
        StatusCode::CREATED,
        [(header::LOCATION, location)],
        Json(chapter),
    )
        .into_response())
}

/// Um capítulo. Quem vê a gravação.
#[utoipa::path(
    get, path = "/api/recordings/{recording_id}/chapters/{chapter_id}", tag = "recordings",
    security(("session" = [])),
    params(("recording_id" = Uuid, Path), ("chapter_id" = Uuid, Path)),
    responses(
        (status = 200, body = Chapter),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 404, body = crate::openapi::ErrorBody),
    )
)]
pub async fn get_chapter(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path((id, chapter_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<Chapter>, ApiError> {
    seen_item(&state, id, auth.user_id).await?;
    let chapter: Option<Chapter> = sqlx::query_as(&format!(
        "SELECT {CHAPTER_COLUMNS} FROM recording_chapters WHERE id = $1 AND recording_id = $2"
    ))
    .bind(chapter_id)
    .bind(id)
    .fetch_optional(&state.db)
    .await?;
    Ok(Json(chapter.ok_or(ApiError::NotFound)?))
}

/// Apaga um capítulo. Só o dono ou um admin activo da org do dono.
#[utoipa::path(
    delete, path = "/api/recordings/{recording_id}/chapters/{chapter_id}", tag = "recordings",
    security(("session" = [])),
    params(("recording_id" = Uuid, Path), ("chapter_id" = Uuid, Path)),
    responses(
        (status = 204, description = "Apagado."),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 403, body = crate::openapi::ErrorBody, description = "`recording.not_manager`"),
        (status = 404, body = crate::openapi::ErrorBody),
    )
)]
pub async fn delete_chapter(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path((id, chapter_id)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, ApiError> {
    managed_item(&state, id, auth.user_id).await?;
    let r = sqlx::query("DELETE FROM recording_chapters WHERE id = $1 AND recording_id = $2")
        .bind(chapter_id)
        .bind(id)
        .execute(&state.db)
        .await?;
    if r.rows_affected() == 0 {
        return Err(ApiError::NotFound);
    }
    crate::audit::log(
        &state.db,
        None,
        auth.user_id,
        "recording.chapter_deleted",
        &chapter_id.to_string(),
    )
    .await;
    Ok(StatusCode::NO_CONTENT)
}

// ---------- Comentários (G5) ----------

#[derive(Debug, Serialize, sqlx::FromRow, utoipa::ToSchema)]
#[schema(as = RecordingComment)]
pub struct Comment {
    pub id: Uuid,
    pub recording_id: Uuid,
    /// Segundos desde o início; `null` = comentário sobre a gravação inteira.
    pub at_secs: Option<i32>,
    /// Já censurado pelo DLP.
    pub body: String,
    pub author_id: Uuid,
    pub author_name: String,
    pub created_at: DateTime<Utc>,
    pub edited_at: Option<DateTime<Utc>>,
}

/// Comentários vivos (os apagados nunca saem), com o nome do autor.
const COMMENT_SELECT: &str = "SELECT c.id, c.recording_id, c.at_secs, c.body, c.author_id,
        u.username AS author_name, c.created_at, c.edited_at
   FROM recording_comments c JOIN users u ON u.id = c.author_id
  WHERE c.deleted_at IS NULL";

/// A chave de ordem das sem marca temporal: no fim.
const NO_TIMESTAMP_KEY: i32 = i32::MAX;

#[derive(Serialize, utoipa::ToSchema)]
#[schema(as = RecordingCommentPage)]
pub struct CommentPage {
    pub items: Vec<Comment>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_page_token: Option<String>,
}

#[derive(Deserialize, utoipa::ToSchema)]
#[schema(as = RecordingCommentReq)]
pub struct CreateCommentReq {
    /// 1-2000 caracteres. Passa pelo DLP antes de ser guardado.
    pub body: String,
    /// Segundos desde o início; omisso = sobre a gravação inteira.
    #[serde(default)]
    pub at_secs: Option<i32>,
}

#[derive(Deserialize, Default, utoipa::ToSchema)]
#[schema(as = RecordingCommentUpdateReq)]
pub struct UpdateCommentReq {
    pub body: Option<String>,
    /// Muda a marca temporal. Não se retira a marca por aqui.
    pub at_secs: Option<i32>,
}

#[derive(Serialize, Deserialize)]
struct CommentCursor {
    k: i32,
    at: DateTime<Utc>,
    id: Uuid,
}

/// Comentários, por marca temporal (os sem marca no fim) e depois por criação.
/// Quem vê ou descarrega a gravação.
#[utoipa::path(
    get, path = "/api/recordings/{recording_id}/comments", tag = "recordings",
    security(("session" = [])),
    params(("recording_id" = Uuid, Path), PageQuery),
    responses(
        (status = 200, body = CommentPage),
        (status = 400, body = crate::openapi::ErrorBody, description = "page_token inválido"),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 404, body = crate::openapi::ErrorBody),
    )
)]
pub async fn list_comments(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
    Query(q): Query<PageQuery>,
) -> Result<Json<CommentPage>, ApiError> {
    seen_item(&state, id, auth.user_id).await?;
    let page = PageRequest {
        page_size: q.page_size,
        page_token: q.page_token,
    };
    let size = page.size();
    let cursor: Option<CommentCursor> = page.cursor()?;
    let rows: Vec<Comment> = sqlx::query_as(&format!(
        "{COMMENT_SELECT}
            AND c.recording_id = $1
            AND ($2::int IS NULL
                 OR (COALESCE(c.at_secs, {NO_TIMESTAMP_KEY}), c.created_at, c.id) > ($2, $3, $4))
          ORDER BY COALESCE(c.at_secs, {NO_TIMESTAMP_KEY}), c.created_at, c.id
          LIMIT $5"
    ))
    .bind(id)
    .bind(cursor.as_ref().map(|c| c.k))
    .bind(cursor.as_ref().map(|c| c.at))
    .bind(cursor.as_ref().map(|c| c.id).unwrap_or_default())
    .bind(size as i64 + 1)
    .fetch_all(&state.db)
    .await?;
    let p = Page::from_overfetch(rows, size, |c| CommentCursor {
        k: c.at_secs.unwrap_or(NO_TIMESTAMP_KEY),
        at: c.created_at,
        id: c.id,
    });
    Ok(Json(CommentPage {
        items: p.items,
        next_page_token: p.next_page_token,
    }))
}

async fn fetch_comment(
    state: &AppState,
    recording_id: Uuid,
    comment_id: Uuid,
) -> Result<Comment, ApiError> {
    sqlx::query_as(&format!(
        "{COMMENT_SELECT} AND c.id = $1 AND c.recording_id = $2"
    ))
    .bind(comment_id)
    .bind(recording_id)
    .fetch_optional(&state.db)
    .await?
    .ok_or(ApiError::NotFound)
}

/// Um comentário escrito por outra pessoa não se altera nem se apaga.
fn require_author(comment: &Comment, user_id: Uuid) -> Result<(), ApiError> {
    if comment.author_id != user_id {
        return Err(DomainError::forbidden("recording.not_comment_author")
            .with_message("só quem escreveu o comentário o altera ou apaga")
            .into());
    }
    Ok(())
}

/// Comenta a gravação, com ou sem marca temporal. O texto passa pelo DLP.
#[utoipa::path(
    post, path = "/api/recordings/{recording_id}/comments", tag = "recordings",
    security(("session" = [])),
    params(("recording_id" = Uuid, Path)),
    request_body = CreateCommentReq,
    responses(
        (status = 201, body = Comment, headers(("Location" = String))),
        (status = 400, body = crate::openapi::ErrorBody, description = "`recording.invalid_comment` / `recording.invalid_timestamp`"),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 404, body = crate::openapi::ErrorBody),
    )
)]
pub async fn create_comment(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
    Json(req): Json<CreateCommentReq>,
) -> Result<Response, ApiError> {
    let rec = seen_item(&state, id, auth.user_id).await?;
    let body = rules::validate_comment_body(&req.body)?;
    let at_secs = req
        .at_secs
        .map(|a| rules::validate_at_secs(a, rec.duration_secs))
        .transpose()?;
    // DLP antes de qualquer byte chegar à base: um comentário é lido por toda
    // a gente que vê a gravação, e pode sair num export.
    let body = crate::dlp::censor(&body);
    let (comment_id,): (Uuid,) = sqlx::query_as(
        "INSERT INTO recording_comments (recording_id, at_secs, body, author_id)
         VALUES ($1, $2, $3, $4) RETURNING id",
    )
    .bind(id)
    .bind(at_secs)
    .bind(&body)
    .bind(auth.user_id)
    .fetch_one(&state.db)
    .await?;
    let comment = fetch_comment(&state, id, comment_id).await?;
    let location = format!("/api/recordings/{id}/comments/{comment_id}");
    Ok((
        StatusCode::CREATED,
        [(header::LOCATION, location)],
        Json(comment),
    )
        .into_response())
}

/// Um comentário. Quem vê ou descarrega a gravação.
#[utoipa::path(
    get, path = "/api/recordings/{recording_id}/comments/{comment_id}", tag = "recordings",
    security(("session" = [])),
    params(("recording_id" = Uuid, Path), ("comment_id" = Uuid, Path)),
    responses(
        (status = 200, body = Comment),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 404, description = "Inexistente, apagado, ou sem acesso.", body = crate::openapi::ErrorBody),
    )
)]
pub async fn get_comment(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path((id, comment_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<Comment>, ApiError> {
    seen_item(&state, id, auth.user_id).await?;
    Ok(Json(fetch_comment(&state, id, comment_id).await?))
}

/// Altera o texto ou a marca temporal. Só o autor.
#[utoipa::path(
    patch, path = "/api/recordings/{recording_id}/comments/{comment_id}", tag = "recordings",
    security(("session" = [])),
    params(("recording_id" = Uuid, Path), ("comment_id" = Uuid, Path)),
    request_body = UpdateCommentReq,
    responses(
        (status = 200, body = Comment),
        (status = 400, body = crate::openapi::ErrorBody),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 403, body = crate::openapi::ErrorBody, description = "`recording.not_comment_author`"),
        (status = 404, body = crate::openapi::ErrorBody),
    )
)]
pub async fn update_comment(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path((id, comment_id)): Path<(Uuid, Uuid)>,
    Json(req): Json<UpdateCommentReq>,
) -> Result<Json<Comment>, ApiError> {
    let rec = seen_item(&state, id, auth.user_id).await?;
    let comment = fetch_comment(&state, id, comment_id).await?;
    require_author(&comment, auth.user_id)?;
    let body = req
        .body
        .as_deref()
        .map(rules::validate_comment_body)
        .transpose()?
        .map(|b| crate::dlp::censor(&b));
    let at_secs = req
        .at_secs
        .map(|a| rules::validate_at_secs(a, rec.duration_secs))
        .transpose()?;
    sqlx::query(
        "UPDATE recording_comments
            SET body = COALESCE($3, body), at_secs = COALESCE($4, at_secs), edited_at = now()
          WHERE id = $1 AND recording_id = $2 AND deleted_at IS NULL",
    )
    .bind(comment_id)
    .bind(id)
    .bind(body)
    .bind(at_secs)
    .execute(&state.db)
    .await?;
    Ok(Json(fetch_comment(&state, id, comment_id).await?))
}

/// Apaga (logicamente) um comentário. Só o autor. Apagar outra vez dá `404`.
#[utoipa::path(
    delete, path = "/api/recordings/{recording_id}/comments/{comment_id}", tag = "recordings",
    security(("session" = [])),
    params(("recording_id" = Uuid, Path), ("comment_id" = Uuid, Path)),
    responses(
        (status = 204, description = "Apagado (deixa de aparecer)."),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 403, body = crate::openapi::ErrorBody, description = "`recording.not_comment_author`"),
        (status = 404, body = crate::openapi::ErrorBody),
    )
)]
pub async fn delete_comment(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path((id, comment_id)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, ApiError> {
    seen_item(&state, id, auth.user_id).await?;
    let comment = fetch_comment(&state, id, comment_id).await?;
    require_author(&comment, auth.user_id)?;
    let r = sqlx::query(
        "UPDATE recording_comments SET deleted_at = now()
          WHERE id = $1 AND recording_id = $2 AND deleted_at IS NULL",
    )
    .bind(comment_id)
    .bind(id)
    .execute(&state.db)
    .await?;
    if r.rows_affected() == 0 {
        return Err(ApiError::NotFound);
    }
    Ok(StatusCode::NO_CONTENT)
}
