//! Gravações de reuniões: upload (webm), biblioteca por utilizador,
//! partilha só-leitura e download; metadados e estado (G4), capítulos e
//! comentários (G5), pesquisa na transcrição (G6).
//!
//! O ficheiro fica no disco (`config.recordings_dir`, de `RECORDINGS_DIR`);
//! a base de dados guarda os metadados. Acesso: quem participou na sala
//! (`room_participants`), quem fez o upload, ou com quem foi partilhada
//! (`recording_shares`). Partilha é sempre só-leitura (download). Uma gravação
//! PUBLICADA para a organização (`visibility = 'org'`) é vista também pelos
//! membros activos de uma organização do autor — ver `AccessFacts::can_view`.
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
use std::sync::{Arc, LazyLock};
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
        public_share_access,
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
        PublicShareResp,
        PublicShareAccessReq
    ))
)]
pub struct ApiDoc;

/// A gravação acabada de carregar (`POST /api/rooms/{room_code}/recordings`);
/// a resposta do upload junta-lhe `kind`, `status`, a media medida e
/// `has_thumbnail`. As LEITURAS — a biblioteca, o recurso e a lista da sala —
/// devolvem `RecordingLibraryItem`.
#[derive(Debug, Serialize, sqlx::FromRow, utoipa::ToSchema)]
pub struct Recording {
    pub id: Uuid,
    pub room_id: Uuid,
    pub uploader_id: Uuid,
    pub filename: String,
    pub size_bytes: i64,
    pub created_at: DateTime<Utc>,
}

/// Item da biblioteca com metadados de media, estados, contagens e publicação
/// (`RecordingLibraryItem` da UI — ver `web/src/api.ts`).
///
/// Substituiu o item antigo (`title`, `category`, `duration_secs`): a consola
/// já lia estes nomes e o servidor devolvia os outros, por isso a biblioteca
/// aparecia sem duração e sem estado (R183, R236).
#[derive(Debug, Serialize, utoipa::ToSchema)]
#[schema(as = RecordingLibraryItem)]
pub struct RecordingItem {
    pub id: Uuid,
    pub room_id: Uuid,
    pub room_code: String,
    pub uploader_id: Uuid,
    pub uploader_name: String,
    /// Nome da gravação: o que a UI mostra e o nome com que se descarrega.
    pub filename: String,
    pub size_bytes: i64,
    pub created_at: DateTime<Utc>,
    /// True se quem pede participou na sala; false se só a vê por partilha,
    /// publicação ou papel.
    pub owned: bool,
    /// Nº de utilizadores com quem está partilhada (só relevante para o dono).
    pub share_count: i64,
    /// RBAC de download: só o dono da gravação e admins da org do dono
    /// descarregam o ficheiro; os restantes só reproduzem.
    pub can_download: bool,
    /// Pode editar nome, descrição, etiquetas, capítulos, legendas e publicar.
    pub can_manage: bool,
    /// Estado do FICHEIRO: `processing` | `transcribing` | `ready` | `failed`.
    ///
    /// `processing` é o servidor a compor o ficheiro depois de a gravação
    /// parar: ainda não há nada para reproduzir, o `progress_pct` diz quanto
    /// falta, e não é uma falha (`failure_reason` é `null`).
    ///
    /// A entrada falhada existe para ser VISTA: antes, uma gravação que não
    /// compunha desaparecia sem deixar rasto, e quem carregou em «gravar»
    /// ficava a pensar que tinha um ficheiro algures. Ver migração 0036.
    pub status: String,
    /// Causa em linguagem de utilizador. `null` quando não falhou.
    pub failure_reason: Option<String>,
    /// O `status`, com `published` quando está pronta e publicada.
    pub state: String,
    /// Progresso da composição, 0–100, enquanto o `status` é `processing`.
    /// `null` fora dela (a transcrição não escreve progresso aqui).
    pub progress_pct: Option<i16>,
    /// `meeting` | `training` | `broadcast` | `hybrid`.
    pub kind: String,
    /// Medidos com ffprobe; `null` = não foi possível medir (nunca inventado).
    pub duration_ms: Option<i64>,
    pub width: Option<i32>,
    pub height: Option<i32>,
    pub fps: Option<f32>,
    pub video_codec: Option<String>,
    pub audio_codec: Option<String>,
    /// Há miniatura em `GET /api/recordings/{recording_id}/thumbnail`.
    pub has_thumbnail: bool,
    /// `none` | `transcribing` | `ready` | `failed`.
    pub transcript_status: String,
    pub transcript_language: Option<String>,
    pub transcribed_at: Option<DateTime<Utc>>,
    pub chapter_count: i64,
    /// Comentários vivos (os apagados não contam).
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
    /// Organização do autor (a que partilha com quem pede, se houver).
    pub uploader_org_id: Option<Uuid>,
    pub uploader_org_name: Option<String>,
    /// Excerto com os termos marcados entre `«` e `»`. Só numa pesquisa (`q`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snippet: Option<String>,
}

/// Página da biblioteca (com `page_size`, `page_token`, `filter`, `filters`,
/// `group_by` ou `order_by`): o envelope do ADR-0007
/// (`docs/reference/pesquisa.md` §2.3). Só documentação — a resposta é
/// `search::SearchPage`, com cada item acompanhado de `search: {score,
/// highlight}` quando há `q`. Continua a ter `items` e `next_page_token`, como a
/// página de antes; `total`, `total_kind` e `groups` são acrescento.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct RecordingPage {
    pub items: Vec<RecordingItem>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_page_token: Option<String>,
    pub total: i64,
    /// `exact` | `at_least`.
    pub total_kind: String,
    #[schema(value_type = Option<Vec<Object>>)]
    pub groups: Option<Vec<serde_json::Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_groups_page_token: Option<String>,
    /// Só com `q`: `exact` | `fuzzy`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text_match: Option<String>,
}

/// Sem paginação nem parâmetros de pesquisa de lista: a lista inteira (forma
/// herdada, lida pelo web — também com `q` e `scope`). Com eles: uma página.
#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(untagged)]
pub enum LibraryResponse {
    List(Vec<RecordingItem>),
    /// Só documenta a resposta no OpenAPI; o handler serializa `search::SearchPage`.
    #[allow(dead_code)]
    Page(RecordingPage),
}

/// Uma linha da biblioteca com os factos de acesso de quem pede.
#[derive(Debug, sqlx::FromRow)]
pub(crate) struct ItemRow {
    pub(crate) id: Uuid,
    pub(crate) room_id: Uuid,
    room_code: String,
    pub(crate) uploader_id: Uuid,
    uploader_name: String,
    pub(crate) filename: String,
    size_bytes: i64,
    pub(crate) created_at: DateTime<Utc>,
    pub(crate) status: String,
    pub(crate) failure_reason: Option<String>,
    transcribed: bool,
    transcription_failed: bool,
    lease_active: bool,
    kind: String,
    pub(crate) duration_ms: Option<i64>,
    width: Option<i32>,
    height: Option<i32>,
    fps: Option<f32>,
    video_codec: Option<String>,
    audio_codec: Option<String>,
    has_thumbnail: bool,
    progress_pct: Option<i16>,
    transcript_language: Option<String>,
    transcribed_at: Option<DateTime<Utc>>,
    description: String,
    tags: Vec<String>,
    visibility: String,
    published_at: Option<DateTime<Utc>>,
    published: bool,
    is_uploader: bool,
    participant: bool,
    shared: bool,
    org_admin: bool,
    active_member: bool,
    archived_member: bool,
    published_to_my_org: bool,
    share_count: i64,
    chapter_count: i64,
    comment_count: i64,
    view_count: i64,
    participant_count: i64,
    caption_languages: Vec<String>,
    uploader_org_id: Option<Uuid>,
    uploader_org_name: Option<String>,
}

impl ItemRow {
    pub(crate) fn facts(&self) -> rules::AccessFacts {
        rules::AccessFacts {
            is_uploader: self.is_uploader,
            participant: self.participant,
            shared: self.shared,
            org_admin: self.org_admin,
            active_member: self.active_member,
            archived_member: self.archived_member,
            published_to_my_org: self.published_to_my_org,
        }
    }

    /// Recusa (`409`) o que só faz sentido sobre uma gravação com ficheiro, com
    /// os mesmos dois códigos do [`Access::require_file`]: a regra é uma só
    /// (`content::recording::require_file`) e quem a quer chama-a daqui.
    pub(crate) fn require_file(&self) -> Result<(), ApiError> {
        Ok(rules::require_file(self.processing_facts())?)
    }

    fn processing_facts(&self) -> rules::ProcessingFacts<'_> {
        rules::ProcessingFacts {
            status: &self.status,
            transcribed: self.transcribed,
            transcription_failed: self.transcription_failed,
            lease_active: self.lease_active,
        }
    }

    fn into_item(self, snippet: Option<String>) -> RecordingItem {
        let facts = self.facts();
        let pf = self.processing_facts();
        let file = rules::file_status(pf);
        let state = rules::display_state(file, self.published).to_string();
        let transcript_status = rules::transcript_status(pf).as_str().to_string();
        let status = file.as_str().to_string();
        RecordingItem {
            id: self.id,
            room_id: self.room_id,
            room_code: self.room_code,
            uploader_id: self.uploader_id,
            uploader_name: self.uploader_name,
            filename: self.filename,
            size_bytes: self.size_bytes,
            created_at: self.created_at,
            // `owned` foi sempre «participou na sala» (ver o teste de conteúdo).
            owned: self.participant,
            share_count: self.share_count,
            can_download: facts.can_download(),
            can_manage: facts.can_manage(),
            status,
            failure_reason: self.failure_reason,
            state,
            progress_pct: self.progress_pct,
            kind: self.kind,
            duration_ms: self.duration_ms,
            width: self.width,
            height: self.height,
            fps: self.fps,
            video_codec: self.video_codec,
            audio_codec: self.audio_codec,
            has_thumbnail: self.has_thumbnail,
            transcript_status,
            transcript_language: self.transcript_language,
            transcribed_at: self.transcribed_at,
            chapter_count: self.chapter_count,
            comment_count: self.comment_count,
            view_count: self.view_count,
            participant_count: self.participant_count,
            caption_languages: self.caption_languages,
            description: self.description,
            tags: self.tags,
            visibility: self.visibility,
            published_at: self.published_at,
            uploader_org_id: self.uploader_org_id,
            uploader_org_name: self.uploader_org_name,
            snippet,
        }
    }
}

/// Uma linha por gravação, com os factos de acesso de `$1` (quem pede).
///
/// A pertença lê-se UMA vez, numa só junção: admin activo, membro activo e
/// membro arquivado de uma organização do dono. O dono (`o`) não se filtra por
/// `archived_at` — a gravação de quem saiu continua da empresa (S3); quem pede
/// (`me`) sim, pela regra do domínio. O `published_to_my_org` reusa o
/// `active_member` DESSA junção: publicar não abre uma segunda leitura de
/// pertença que pudesse divergir da primeira.
static ITEM_SELECT: LazyLock<String> = LazyLock::new(|| {
    format!(
        r#"
SELECT r.id, r.room_id, rm.code AS room_code, r.uploader_id, u.username AS uploader_name,
       r.filename, r.size_bytes, r.created_at, r.status, r.failure_reason,
       (r.transcribed_at IS NOT NULL) AS transcribed,
       (r.transcription_failed_at IS NOT NULL) AS transcription_failed,
       (r.transcription_lease_token IS NOT NULL
        AND r.transcription_lease_expires_at >= now()) AS lease_active,
       r.kind, r.duration_ms, r.width, r.height, r.fps, r.video_codec, r.audio_codec,
       r.has_thumbnail, r.progress_pct, r.transcript_language, r.transcribed_at,
       r.description, r.tags, r.visibility, r.published_at,
       (r.published_at IS NOT NULL) AS published,
       (r.uploader_id = $1) AS is_uploader,
       EXISTS(SELECT 1 FROM room_participants p
               WHERE p.room_id = r.room_id AND p.user_id = $1) AS participant,
       EXISTS(SELECT 1 FROM recording_shares s
               WHERE s.recording_id = r.id AND s.user_id = $1) AS shared,
       m.org_admin, m.active_member, m.archived_member,
       (r.visibility = 'org' AND r.published_at IS NOT NULL AND m.active_member)
           AS published_to_my_org,
       (SELECT COUNT(*) FROM recording_shares sc WHERE sc.recording_id = r.id) AS share_count,
       (SELECT COUNT(*) FROM recording_chapters ch WHERE ch.recording_id = r.id) AS chapter_count,
       (SELECT COUNT(*) FROM recording_comments cm
         WHERE cm.recording_id = r.id AND cm.deleted_at IS NULL) AS comment_count,
       (SELECT COUNT(*) FROM recording_views v WHERE v.recording_id = r.id) AS view_count,
       (SELECT COUNT(*) FROM room_participants pp WHERE pp.room_id = r.room_id)
           AS participant_count,
       COALESCE((SELECT array_agg(cap.lang ORDER BY cap.lang) FROM recording_captions cap
                  WHERE cap.recording_id = r.id AND cap.status = 'published'),
                '{{}}'::text[]) AS caption_languages,
       uo.id AS uploader_org_id, uo.name AS uploader_org_name
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
  LEFT JOIN LATERAL ({uploader_org}) uo ON true
"#,
        uploader_org = crate::org::uploader_org_for_viewer_sql("r.uploader_id", "$1"),
    )
});

/// «Saiu da organização» (S3) sobre as colunas de [`ITEM_SELECT`] (alias `i`).
const NOT_DEPARTED: &str = "NOT (i.archived_member AND NOT i.active_member)";

/// A biblioteca `mine` em SQL: `AccessFacts::listed_in(Mine, _)`. Publicar não
/// enche a biblioteca pessoal dos colegas, por isso este predicado NÃO olha
/// para `published_to_my_org`.
static LIBRARY_VISIBLE_MINE: LazyLock<String> =
    LazyLock::new(|| format!("(i.is_uploader OR i.participant OR i.shared) AND {NOT_DEPARTED}"));

/// A biblioteca `published` em SQL: `AccessFacts::listed_in(Published, published)`.
///
/// Escreve-se em SQL porque filtra ANTES de paginar; o teste
/// `library_scopes_agree_with_access_facts` prova que os dois concordam com o
/// domínio linha a linha.
static LIBRARY_VISIBLE_PUBLISHED: LazyLock<String> = LazyLock::new(|| {
    format!(
        "i.published AND (i.is_uploader OR i.participant OR i.shared OR i.published_to_my_org) \
         AND {NOT_DEPARTED}"
    )
});

/// A gravação `id` com os factos de acesso de `user_id`. `None` = não existe.
pub(crate) async fn load_item(
    state: &AppState,
    id: Uuid,
    user_id: Uuid,
) -> Result<Option<ItemRow>, ApiError> {
    Ok(
        sqlx::query_as::<_, ItemRow>(&format!("{} WHERE r.id = $2", *ITEM_SELECT))
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

/// Como [`seen_item`], e além disso tem de a poder MOSTRAR A OUTREM (partilhas,
/// link público e publicar para a organização): o dono activo (`can_share`), OU
/// quem tem `recordings.publish` numa organização activa do dono (ADR-0008 §1 —
/// poder sobre gravações de OUTROS). Vê a gravação sem nenhum dos dois →
/// `403 authz.missing_capability`; não a vê → `404` (de `seen_item`).
///
/// O `publish` chegou aqui a 2026-10-06 (R306): pedia `require_manage`, e desde
/// que o `access()` passou a derivar o `org_admin` da capacidade
/// `recordings.view_others`, um papel a quem a organização NEGAVA
/// `recordings.publish` publicava a gravação de um colega. Quem decide a quem
/// uma gravação é mostrada é esta porta, não a de gerir metadados.
pub(crate) async fn owned_item(
    state: &AppState,
    id: Uuid,
    user_id: Uuid,
) -> Result<ItemRow, ApiError> {
    let row = seen_item(state, id, user_id).await?;
    if row.facts().can_share() {
        return Ok(row);
    }
    let cap = delonix_meet_domain::identity::authorization::Capability::RecordingsPublish;
    if crate::org::has_capability_over_colleague(state, user_id, row.uploader_id, cap).await? {
        return Ok(row);
    }
    Err(DomainError::forbidden("authz.missing_capability")
        .with_message(
            "só o dono da gravação, ou quem tem recordings.publish, a mostra a outras pessoas",
        )
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
//
// E é mesmo uma só: os predicados SQL que aqui viviam
// (`sql_can_manage`/`sql_can_view`/`sql_direct_relation`) eram a SEGUNDA
// derivação do acesso, e o único consumidor deles era o [`access`]. Faltava-lhes
// a condição da S3 e por isso quem saía da organização do dono passava por eles
// (R304). Saíram em vez de serem corrigidos: uma regra de acesso que existe em
// dois sítios volta a divergir, e aqui não havia nada a pedi-la em SQL — nenhum
// destes caminhos pagina, que é a razão por que a BIBLIOTECA tem os seus
// (`LIBRARY_VISIBLE_*`, provados contra o domínio por
// `library_scopes_agree_with_access_facts`).

/// O que um pedido pode fazer a uma gravação.
#[derive(Debug)]
pub(crate) struct Access {
    pub id: Uuid,
    pub room_id: Uuid,
    pub status: String,
    pub duration_ms: Option<i64>,
    pub can_manage: bool,
    /// Ligação directa com a gravação (não só «está publicada»).
    pub direct_relation: bool,
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

    /// Ler a transcrição e a lista de presentes exige ligação DIRECTA.
    ///
    /// Publicar a gravação abre a REPRODUÇÃO a toda a organização; não é
    /// convite para um colega que nunca esteve na reunião ler a transcrição
    /// (que pode ter nomes e decisões que ninguém reviu) nem saber quem
    /// esteve lá. Quem chega por publicação recebe `403`, não `404`: já sabe
    /// que a gravação existe, viu-a na biblioteca publicada.
    pub fn require_direct_relation(&self, code: &'static str) -> Result<(), ApiError> {
        if self.direct_relation {
            return Ok(());
        }
        Err(DomainError::forbidden(code)
            .with_message(
                "a gravação está publicada para reprodução; \
                 a transcrição e os presentes são de quem participou",
            )
            .into())
    }

    /// Recusa (`409`) o que só faz sentido sobre uma gravação com ficheiro:
    /// `recording.processing` a compor, `recording.no_file` falhada. A mesma
    /// regra e os mesmos códigos de partilhar e do link público.
    pub fn require_file(&self) -> Result<(), ApiError> {
        Ok(rules::require_file(self.processing_facts())?)
    }

    /// Só o `status`: é o que decide se há ficheiro. As marcas da transcrição
    /// distinguem `ready` de `transcribing`, e os dois têm ficheiro.
    fn processing_facts(&self) -> rules::ProcessingFacts<'_> {
        rules::ProcessingFacts {
            status: &self.status,
            ..Default::default()
        }
    }
}

/// Resolve o acesso de `viewer` à gravação `id`. Quem não a pode ver recebe
/// `404`: não se confirma a outra organização que o id existe.
///
/// Deriva do MESMO sítio que as rotas por id (`seen_item` → `AccessFacts`), e
/// não de um predicado SQL próprio. Enquanto derivava do seu
/// (`sql_can_view`/`sql_can_manage`/`sql_direct_relation`), faltava-lhe a
/// condição da S3 que o `AccessFacts::departed` tem: quem saía da organização
/// do dono recebia `200` em `/transcript`, `/participants` e `/captions`, e
/// `204` em `POST /views`, da mesma gravação cujo `/details` já lhe respondia
/// `404` (medido a 2026-10-05, escrito na R304). Uma gravação tem UMA regra de
/// acesso; esta era a segunda porta.
pub(crate) async fn access(state: &AppState, id: Uuid, viewer: Uuid) -> Result<Access, ApiError> {
    let row = seen_item(state, id, viewer).await?;
    let facts = row.facts();
    Ok(Access {
        id: row.id,
        room_id: row.room_id,
        status: row.status,
        duration_ms: row.duration_ms,
        can_manage: facts.can_manage(),
        direct_relation: facts.has_direct_relation(),
    })
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
/// **Registra** as entregas do `recording.ready` na transacção de quem chama
/// (trabalho nº3). Devolve o que há a enviar depois do commit; `envia` manda.
///
/// Era um `fire` disparado numa tarefa à parte, com uma janela entre o
/// `COMMIT` que marcou a gravação `ready` e o `INSERT` da entrega em que um
/// SIGTERM apagava o aviso sem deixar rasto.
/// `orgs` vem de FORA: a consulta usa a pool, e pedir uma segunda ligação com
/// uma transacção na mão é a armadilha que a `delonix-meet-backend` avisa pelo
/// nome. Quem chama lê-as antes do `begin`.
pub(crate) async fn enqueue_recording_ready(
    orgs: Vec<Uuid>,
    conn: &mut sqlx::PgConnection,
    r: ReadyRecording<'_>,
) -> Result<Vec<crate::webhooks::Enfileirada>, sqlx::Error> {
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
    if orgs.is_empty() {
        return Ok(Vec::new());
    }
    let _ = uploader;
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
    let mut fila = Vec::new();
    for org_id in orgs {
        fila.extend(
            crate::webhooks::enqueue(
                &mut *conn,
                org_id,
                &crate::webhooks::Event {
                    name: "recording.ready",
                    title: "Delonix Meet".into(),
                    text: text.clone(),
                    payload: payload.clone(),
                },
            )
            .await?,
        );
    }
    Ok(fila)
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
    // O UPLOAD é o caso em que uma transacção NÃO fecha a janela, e vale dizer
    // porquê em vez de a fingir fechada: a linha da gravação já foi commitada
    // `ready` lá atrás (antes de o ficheiro ser escrito e medido), e o payload
    // do `recording.ready` precisa do `media`, que só se sabe depois do
    // `ffprobe`. Embrulhar só o registo das entregas move a janela, não a
    // elimina.
    //
    // O que a fecharia: inserir a gravação em `processing`, e passá-la a
    // `ready` na MESMA transacção do registo das entregas, depois de medida —
    // o que também corrigiria a linha ser anunciada pronta antes de o ficheiro
    // estar validado. É uma mudança ao contrato desta rota (o `status` que a
    // resposta devolve) e não entra neste trabalho.
    //
    // Entretanto: a transacção aqui garante que as entregas das várias
    // organizações do dono nascem todas ou nenhuma, e a rede de segurança
    // (varredor + `retry_due`) vale como para as outras.
    let orgs = crate::org::orgs_of_user(&state, auth.user_id).await;
    let fila = match state.db.begin().await {
        Ok(mut tx) => {
            let r = enqueue_recording_ready(
                orgs,
                &mut tx,
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
            match r {
                Ok(f) => match tx.commit().await {
                    Ok(()) => f,
                    Err(e) => {
                        tracing::warn!(error = %e, "upload: o registo das entregas não ficou");
                        Vec::new()
                    }
                },
                Err(e) => {
                    tracing::warn!(error = %e, "upload: o registo das entregas falhou");
                    Vec::new()
                }
            }
        }
        Err(e) => {
            tracing::warn!(error = %e, "upload: sem transacção para registar as entregas");
            Vec::new()
        }
    };
    crate::webhooks::envia(&state, "recording.ready", fila).await;
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
///
/// Devolve a MESMA representação da biblioteca e do recurso
/// (`RecordingLibraryItem`): o `status` diz se há ficheiro (`processing` e
/// `failed` não têm), e o `can_download` se quem pede o pode descarregar.
///
/// Sem paginação: devolve a lista de uma sala inteira (dívida — a rota já
/// não paginava, e cada linha custa agora o que custa na biblioteca).
// É mais uma VISTA da gravação, lida pela consulta e pela regra das outras.
// Antes devolvia seis campos sem estado, e o painel oferecia «descarregar»
// sobre uma gravação a compor, falhada, ou que o `?dl=1` ia recusar (R59).
#[utoipa::path(
    get, path = "/api/rooms/{room_code}/recordings", tag = "recordings",
    security(("session" = [])),
    params(("room_code" = String, Path, description = "Código da sala.")),
    responses(
        (status = 200, body = Vec<RecordingItem>, description = "Mais recentes primeiro. Inclui as que o servidor ainda está a compor (`status = processing`, com `progress_pct`) e as falhadas (`status = failed`, com `failure_reason`): nenhuma das duas tem ficheiro. Só as gravações a que quem pede ainda chega — um participante que saiu da organização do dono recebe a lista sem elas."),
        (status = 401, description = "Sessão inválida.", body = crate::openapi::ErrorBody),
        (status = 403, description = "`room.not_participant`: não participou na sala.", body = crate::openapi::ErrorBody),
        (status = 404, body = crate::openapi::ErrorBody),
    )
)]
pub async fn list(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(code): Path<String>,
) -> Result<Json<Vec<RecordingItem>>, ApiError> {
    let room = room_by_code(&state, &code).await?;
    if !is_participant(&state, room.id, auth.user_id).await? {
        return Err(DomainError::forbidden("room.not_participant")
            .with_message("só quem participou na sala")
            .into());
    }
    let rows = sqlx::query_as::<_, ItemRow>(&format!(
        "{} WHERE r.room_id = $2 ORDER BY r.created_at DESC, r.id DESC",
        *ITEM_SELECT
    ))
    .bind(auth.user_id)
    .bind(room.id)
    .fetch_all(&state.db)
    .await?;
    // Ter estado na sala não chega: a lista só anuncia o que as rotas por id
    // servem a quem pede (S3 — quem saiu da organização do dono já não chega
    // à gravação, e passaria a ler aqui a descrição e as etiquetas dela).
    // Filtra-se em Rust porque não há página; quem paginar esta rota passa o
    // filtro para SQL, como `LIBRARY_VISIBLE_*`, senão as páginas encolhem.
    Ok(Json(
        rows.into_iter()
            .filter(|row| row.facts().can_see())
            .map(|row| row.into_item(None))
            .collect(),
    ))
}

/// O que a biblioteca tem além dos parâmetros uniformes de `search::SearchParams`.
#[derive(Deserialize)]
pub struct LibraryQuery {
    /// `mine` (omissão) = a biblioteca de sempre: carregou, participou, ou
    /// foram-lhe partilhadas. `published` = as publicadas para a organização
    /// que quem pede vê, INCLUINDO aquelas em cuja sala nunca esteve — é esta
    /// que dá sentido a publicar (R235).
    pub scope: Option<String>,
}

/// Biblioteca do utilizador.
///
/// **Duas bibliotecas, pelo `scope`.** `mine` (omissão) é a de sempre: o que
/// carregou, onde participou, e o que lhe foi partilhado. `published` são as
/// gravações publicadas para a organização que quem pede vê — incluindo as de
/// salas onde nunca esteve. Sem a segunda, publicar escrevia duas colunas e
/// não mostrava a gravação a ninguém (R235).
///
/// **Duas formas, de propósito.** Só com `q` e/ou `scope` devolve a lista
/// inteira, como sempre — é o que o web lê (`recordingsLibraryMeta`). Com
/// `page_size`, `page_token`, `filter`, `filters`, `group_by` ou `order_by`
/// responde a pesquisa de lista do ADR-0007 (`docs/reference/pesquisa.md`
/// §2.3): página keyset com `items` e `next_page_token` como antes, mais
/// `total` e os grupos — é o que o painel de pesquisa do web pede. Com `q` e
/// sem `order_by` continua por `created_at` descendente; a relevância pede-se
/// com `order_by=-_score`, e o `snippet` mantém-se. As duas respeitam o
/// `scope`. A forma sem limite é dívida e sai quando o web passar a paginar.
///
/// Um membro arquivado (S3) deixa de ver as gravações da ex-organização, nas
/// duas bibliotecas.
#[utoipa::path(
    get, path = "/api/recordings", tag = "recordings",
    security(("session" = [])),
    params(("scope" = Option<String>, Query, description = "`mine` (omissão) ou `published`."), crate::search::SearchParams),
    responses(
        (status = 200, body = LibraryResponse,
         description = "Só com `q`/`scope`: `RecordingLibraryItem[]` (todas). Com `page_size`, `page_token` ou parâmetros de pesquisa de lista: `RecordingPage`. Inclui as falhadas (`status = failed`)."),
        (status = 400, body = crate::openapi::ErrorBody, description = "`page.invalid_token`; `recording.invalid_query` (`q` sem nenhuma letra ou dígito); `recording.invalid_scope`; e os `search.*` do contrato de pesquisa."),
        (status = 401, body = crate::openapi::ErrorBody),
    )
)]
pub async fn library(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Query(q): Query<LibraryQuery>,
    Query(params): Query<crate::search::SearchParams>,
) -> Result<Response, ApiError> {
    let scope = rules::LibraryScope::parse(q.scope.as_deref())?;

    // Página ou pesquisa de lista (ADR-0007): com qualquer parâmetro além do
    // `q`. Só `q`/`scope` continua a ser a lista inteira abaixo — é a que o
    // web lê fora do painel de pesquisa.
    if params.beyond_text() {
        let page = crate::search::list_recordings(&state, auth.user_id, scope, &params).await?;
        return Ok(Json(page).into_response());
    }
    let visible: &str = match scope {
        rules::LibraryScope::Mine => &LIBRARY_VISIBLE_MINE,
        rules::LibraryScope::Published => &LIBRARY_VISIBLE_PUBLISHED,
    };

    // Um `q` vazio não filtra; um `q` só com pontuação é erro do cliente, não
    // «tudo» nem «nada» em silêncio.
    let tsquery = match params.q.as_deref().map(str::trim).filter(|t| !t.is_empty()) {
        None => None,
        Some(text) => Some(rules::search_query(text).ok_or_else(|| {
            DomainError::invalid(
                "recording.invalid_query",
                "a pesquisa tem de ter pelo menos uma letra ou dígito",
            )
            .with_field("q", "letras e dígitos")
        })?),
    };

    // Duas instruções distintas (com e sem texto) em vez de `$2 IS NULL OR …`:
    // um plano genérico com o OR deixava de usar o índice GIN.
    let search = if tsquery.is_some() {
        "r.search_vector @@ to_tsquery('dlx_search', $2)"
    } else {
        "$2::text IS NULL"
    };
    let rows: Vec<ItemRow> = sqlx::query_as(&format!(
        "SELECT * FROM ({} WHERE {search}) i
          WHERE {visible}
          ORDER BY i.created_at DESC, i.id DESC",
        *ITEM_SELECT,
    ))
    .bind(auth.user_id)
    .bind(tsquery.as_deref())
    .fetch_all(&state.db)
    .await?;
    let snippets = snippets_for(&state, &rows, tsquery.as_deref()).await?;
    Ok(Json(LibraryResponse::List(
        rows.into_iter()
            .map(|r| {
                let snippet = snippets.get(&r.id).cloned();
                r.into_item(snippet)
            })
            .collect(),
    ))
    .into_response())
}

/// O excerto só para as linhas devolvidas: o `ts_headline` relê o texto
/// inteiro, e fazê-lo antes do LIMIT seria por cada candidata.
async fn snippets_for(
    state: &AppState,
    rows: &[ItemRow],
    tsquery: Option<&str>,
) -> Result<std::collections::HashMap<Uuid, String>, ApiError> {
    let Some(tsq) = tsquery else {
        return Ok(Default::default());
    };
    let ids: Vec<Uuid> = rows.iter().map(|r| r.id).collect();
    let found: Vec<(Uuid, String)> = sqlx::query_as(
        r#"SELECT r.id, ts_headline('dlx_search',
                    CASE WHEN to_tsvector('dlx_search', r.transcript) @@ q.q
                         THEN r.transcript
                         ELSE coalesce(r.title, '') || ' ' || r.filename END,
                    q.q,
                    'MaxFragments=1, MaxWords=18, MinWords=6, StartSel="«", StopSel="»"')
             FROM recordings r, to_tsquery('dlx_search', $2) AS q(q)
            WHERE r.id = ANY($1)"#,
    )
    .bind(&ids)
    .bind(tsq)
    .fetch_all(&state.db)
    .await?;
    Ok(found.into_iter().collect())
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
// `item_select_sql()` e um predicado SQL próprio, um caminho de acesso PARALELO ao de
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
         description = "O ficheiro inteiro. `Content-Disposition: inline`, ou `attachment` com `dl=1`. Sai sempre com `Accept-Ranges: bytes`."),
        (status = 206, body = inline(WebmBytes), content_type = "video/webm",
         description = "Resposta a um `Range: bytes=…` de uma só faixa, com `Content-Range: bytes <início>-<fim>/<total>`. É o que o leitor usa para procurar sem puxar o ficheiro todo."),
        (status = 400, description = "A gravação falhou e não tem ficheiro (mensagem = causa). Só para quem chega à gravação.", body = crate::openapi::ErrorBody),
        (status = 401, description = "Sessão inválida.", body = crate::openapi::ErrorBody),
        (status = 403, description = "`recording.download_forbidden`: chega à gravação mas não pode descarregar (`dl=1`).", body = crate::openapi::ErrorBody),
        (status = 404, description = "Gravação inexistente, sem acesso (inclui membro arquivado), ou ficheiro em falta no disco.", body = crate::openapi::ErrorBody),
        (status = 409, description = "`recording.processing`: o servidor ainda está a compor o ficheiro (`status = processing`). Não é uma falha: volta a pedir quando a gravação estiver `ready`. Só para quem chega à gravação.", body = crate::openapi::ErrorBody),
        (status = 416, description = "`Range` bem escrito mas fora do ficheiro. Traz `Content-Range: bytes */<total>`."),
    ),
    params(("Range" = Option<String>, Header, description = "`bytes=<início>-<fim>`, `bytes=<início>-` ou `bytes=-<sufixo>`. Uma só faixa: com várias, responde-se o ficheiro inteiro.")),
)]
pub async fn download(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
    Query(q): Query<DownloadQuery>,
    headers: axum::http::HeaderMap,
) -> Result<Response, ApiError> {
    let range = headers
        .get(header::RANGE)
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned);
    // Quem não chega à gravação recebe o 404 de «não existe» ANTES de qualquer
    // outra resposta: o `400` de gravação falhada levava o motivo da falha a
    // utilizadores de outra organização.
    let rec = seen_item(&state, id, auth.user_id).await?;
    // A compor: ainda não há ficheiro, e não é uma falha. Sem este ramo a
    // resposta era «esta gravação falhou», dita a quem acabou de a parar.
    if rules::file_status(rec.processing_facts()) == rules::FileStatus::Processing {
        return Err(rules::processing_conflict().into());
    }
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
    let disposition = if as_download {
        format!("attachment; filename=\"{}\"", rec.filename.replace('"', ""))
    } else {
        "inline".to_string()
    };
    serve_file_range(&path, &disposition, range.as_deref()).await
}

/// Serve o ficheiro honrando o `Range` (RFC 9110 §14).
///
/// Sem isto, procurar um instante no leitor puxava o ficheiro INTEIRO: o
/// `<video>` pede `Range` e, ao receber `200` sem `Accept-Ranges`, desiste de
/// procurar e volta a descarregar tudo de cada vez (R237). Uma gravação de uma
/// hora são centenas de MB por cada salto na barra.
///
/// Só se aceita UM intervalo: um `Range` com várias faixas responde-se com o
/// ficheiro inteiro (`200`), que a norma permite — o `multipart/byteranges`
/// não traz nada a um leitor de vídeo.
async fn serve_file_range(
    path: &std::path::Path,
    disposition: &str,
    range: Option<&str>,
) -> Result<Response, ApiError> {
    use tokio::io::{AsyncReadExt, AsyncSeekExt};

    let mut file = tokio::fs::File::open(path)
        .await
        .map_err(|_| ApiError::NotFound)?;
    let total = file.metadata().await.map_err(ApiError::internal)?.len();

    let base = [
        (header::CONTENT_TYPE, "video/webm".to_string()),
        (header::CONTENT_DISPOSITION, disposition.to_string()),
        // Anunciado SEMPRE, também na resposta inteira: é assim que o leitor
        // sabe que pode pedir um intervalo da próxima vez.
        (header::ACCEPT_RANGES, "bytes".to_string()),
    ];

    let Some(spec) = range.and_then(|r| parse_byte_range(r, total)) else {
        // Sem `Range`, ou um que não se sabe servir: o ficheiro inteiro.
        // A excepção é o `Range` bem escrito mas fora do ficheiro, que é `416`
        // e não «serve tudo» — `parse_byte_range` devolve `None` aos dois
        // casos, por isso `is_unsatisfiable` distingue-os aqui.
        if let Some(r) = range {
            if is_unsatisfiable(r, total) {
                return Ok((
                    StatusCode::RANGE_NOT_SATISFIABLE,
                    [(header::CONTENT_RANGE, format!("bytes */{total}"))],
                    (),
                )
                    .into_response());
            }
        }
        let mut data = Vec::with_capacity(total as usize);
        file.read_to_end(&mut data)
            .await
            .map_err(ApiError::internal)?;
        return Ok((base, data).into_response());
    };

    let (start, end) = spec;
    let len = end - start + 1;
    file.seek(std::io::SeekFrom::Start(start))
        .await
        .map_err(ApiError::internal)?;
    let mut data = vec![0u8; len as usize];
    file.read_exact(&mut data)
        .await
        .map_err(ApiError::internal)?;
    Ok((
        StatusCode::PARTIAL_CONTENT,
        base,
        [(
            header::CONTENT_RANGE,
            format!("bytes {start}-{end}/{total}"),
        )],
        data,
    )
        .into_response())
}

/// `bytes=<início>-<fim>` → `(início, fim)` inclusivos, já cortados ao
/// tamanho. `None` quando não é um intervalo único que se saiba servir.
///
/// Formas aceites: `bytes=0-1023`, `bytes=1024-` (até ao fim) e `bytes=-500`
/// (os últimos 500). Um ficheiro VAZIO não tem nenhum intervalo válido.
fn parse_byte_range(raw: &str, total: u64) -> Option<(u64, u64)> {
    let spec = raw.trim().strip_prefix("bytes=")?.trim();
    // Várias faixas: não se serve `multipart/byteranges`.
    if spec.contains(',') || total == 0 {
        return None;
    }
    let (from, to) = spec.split_once('-')?;
    let (start, end) = match (from.trim(), to.trim()) {
        // `-500`: o sufixo, os últimos N bytes.
        ("", suffix) => {
            let n: u64 = suffix.parse().ok()?;
            if n == 0 {
                return None;
            }
            (total.saturating_sub(n), total - 1)
        }
        (s, "") => (s.parse().ok()?, total - 1),
        (s, e) => {
            let start: u64 = s.parse().ok()?;
            let end: u64 = e.parse().ok()?;
            // Um fim para lá do ficheiro CORTA-SE, não invalida o pedido.
            (start, end.min(total - 1))
        }
    };
    if start > end || start >= total {
        return None;
    }
    Some((start, end))
}

/// O `Range` está bem escrito mas pede fora do ficheiro (`416`). Distingue-se
/// de «não percebi o cabeçalho», que é servir tudo.
fn is_unsatisfiable(raw: &str, total: u64) -> bool {
    let Some(spec) = raw.trim().strip_prefix("bytes=").map(str::trim) else {
        return false;
    };
    if spec.contains(',') {
        return false;
    }
    let Some((from, _to)) = spec.split_once('-') else {
        return false;
    };
    match from.trim().parse::<u64>() {
        Ok(start) => start >= total,
        // Sufixo (`-500`) num ficheiro vazio.
        Err(_) => total == 0,
    }
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
///
/// Só se partilha uma gravação com ficheiro: a compor ou falhada responde
/// `409`. Retirar uma partilha (`DELETE`) não depende do estado.
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
        (status = 409, description = "A gravação não tem ficheiro. `recording.processing`: o servidor ainda a está a compor — volta a pedir quando estiver `ready`. `recording.no_file`: falhou, não há o que partilhar. Só para quem a pode partilhar.", body = crate::openapi::ErrorBody),
    )
)]
pub async fn share(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
    Json(req): Json<ShareReq>,
) -> Result<Response, ApiError> {
    let rec = owned_item(&state, id, auth.user_id).await?;
    if req.user_id == auth.user_id {
        return Err(ApiError::BadRequest("cannot share with yourself".into()));
    }
    // Antes era um 500 (chave estrangeira) para um id que não existe.
    let target = crate::users::fetch_public(&state.db, req.user_id)
        .await
        .map_err(|_| ApiError::NotFound)?;
    // O estado por último, como em `publish`: primeiro o pedido tem de estar
    // bem formado e o destino existir.
    rules::require_file(rec.processing_facts())?;
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
///
/// Só para uma gravação com ficheiro: a compor ou falhada responde `409`, e
/// o link que já existia fica como estava. Revogar (`DELETE`) e ler (`GET`)
/// não dependem do estado.
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
        (status = 409, description = "A gravação não tem ficheiro. `recording.processing`: o servidor ainda a está a compor — volta a pedir quando estiver `ready`. `recording.no_file`: falhou, não há o que partilhar. Só para quem a pode partilhar.", body = crate::openapi::ErrorBody),
    )
)]
pub async fn create_link(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
    Json(req): Json<CreateLinkReq>,
) -> Result<Json<ShareLink>, ApiError> {
    let rec = owned_item(&state, id, auth.user_id).await?;
    rules::require_file(rec.processing_facts())?;

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

/// O que o caminho do conteúdo aceita na query.
#[derive(Deserialize, utoipa::IntoParams)]
#[into_params(parameter_in = Query)]
pub struct PublicShareContentQuery {
    /// Passe de leitura de um link com password — o `download_url` que
    /// `POST …/access` devolve já o traz. Curto e só deste link; **a password
    /// nunca vai num URL**.
    #[serde(default)]
    pub grant: Option<String>,
}

/// A password de um link, no CORPO.
#[derive(Deserialize, utoipa::ToSchema)]
pub struct PublicShareAccessReq {
    pub password: String,
}

/// Metadados de uma gravação partilhada por link público.
#[derive(Serialize, utoipa::ToSchema)]
pub struct PublicShareResp {
    pub recording_id: Uuid,
    pub filename: String,
    pub size_bytes: i64,
    pub created_at: DateTime<Utc>,
    /// O caminho do conteúdo. Num link com password já leva o passe de
    /// leitura (`?grant=…`), para o `<video>` e o download o usarem tal e qual.
    pub download_url: String,
    pub has_password: bool,
}

/// Quanto vale o passe de leitura de um link com password.
const SHARE_GRANT_SECS: i64 = 3600;

/// O passe de leitura: `<expira>.<hmac>`. Existe porque um `<video src>` não
/// manda cabeçalhos nem corpo — alguma coisa tem de ir no URL — e essa coisa
/// não pode ser a password, que é escolhida por uma pessoa, costuma repetir-se
/// noutros sítios e ficava escrita nos logs de acesso de todos os proxies. O
/// passe expira numa hora, só abre ESTE link, e deixa de valer quando a
/// password do link muda (o hash dela entra no que se assina).
fn share_grant(state: &AppState, token: &str, password_hash: &str, expires_at: i64) -> String {
    let key =
        delonix_meet_core::crypto::derive_key(&state.config.jwt_secret, "recording-share-grant");
    let mac = delonix_meet_core::crypto::hmac_sha256(
        key,
        format!("{token}\n{password_hash}\n{expires_at}"),
    );
    let hex: String = mac.iter().map(|b| format!("{b:02x}")).collect();
    format!("{expires_at}.{hex}")
}

fn share_grant_is_valid(state: &AppState, token: &str, password_hash: &str, grant: &str) -> bool {
    let Some((exp, _)) = grant.split_once('.') else {
        return false;
    };
    let Ok(exp) = exp.parse::<i64>() else {
        return false;
    };
    exp > Utc::now().timestamp()
        && delonix_meet_core::crypto::ct_eq(
            share_grant(state, token, password_hash, exp).as_bytes(),
            grant.as_bytes(),
        )
}

/// O link, se existir, não tiver expirado e a gravação tiver ficheiro:
/// `(gravação, hash da password, nome, tamanho, criada em)`. Expirado ou sem
/// ficheiro responde como inexistente.
type ShareLinkRow = (Uuid, Option<String>, String, i64, DateTime<Utc>);

/// A linha tal como vem da base, ainda com a validade por verificar.
type ShareLinkDbRow = (
    Uuid,
    Option<String>,
    Option<DateTime<Utc>>,
    String,
    i64,
    DateTime<Utc>,
    String,
);

async fn live_share_link(state: &AppState, token: &str) -> Result<ShareLinkRow, ApiError> {
    let row: Option<ShareLinkDbRow> = sqlx::query_as(
        r#"SELECT l.recording_id, l.password_hash, l.expires_at,
                      r.filename, r.size_bytes, r.created_at, r.status
               FROM recording_share_links l
               JOIN recordings r ON r.id = l.recording_id
               WHERE l.token = $1"#,
    )
    .bind(token)
    .fetch_optional(&state.db)
    .await?;
    let (rec_id, password_hash, expires_at, filename, size_bytes, created_at, status) =
        row.ok_or(ApiError::NotFound)?;
    if expires_at.is_some_and(|exp| Utc::now() > exp) {
        return Err(ApiError::NotFound);
    }
    // Um link já não se cria sem ficheiro, mas os que existiam antes dessa
    // regra continuam na base. Sem isto respondiam `200` com `size_bytes: 0` e
    // um `download_url` que dava `404` — e, sobre uma gravação ainda a compor,
    // o conteúdo lia um ficheiro que o gravador ainda não deu por pronto. A
    // quem abre um link não se diz o estado: não existe.
    let facts = rules::ProcessingFacts {
        status: &status,
        ..Default::default()
    };
    if !facts.has_file() {
        return Err(ApiError::NotFound);
    }
    Ok((rec_id, password_hash, filename, size_bytes, created_at))
}

/// Acesso público a uma gravação via token (sem autenticação).
///
/// Link expirado responde como inexistente (404). Um link com password
/// responde `401`: abre-se com `POST …/access`, que leva a password no corpo.
#[utoipa::path(
    get, path = "/api/public/recordings/{token}", tag = "recordings",
    params(("token" = String, Path, description = "Token do link público.")),
    responses(
        (status = 200, body = PublicShareResp),
        (status = 401, description = "O link tem password: use `POST /api/public/recordings/{token}/access`.", body = crate::openapi::ErrorBody),
        (status = 404, description = "Token inexistente ou expirado; ou a gravação não tem ficheiro (falhou, ou o servidor ainda a está a compor).", body = crate::openapi::ErrorBody),
    )
)]
pub async fn public_share(
    State(state): State<Arc<AppState>>,
    Path(token): Path<String>,
) -> Result<Json<PublicShareResp>, ApiError> {
    let (rec_id, password_hash, filename, size_bytes, created_at) =
        live_share_link(&state, &token).await?;
    if password_hash.is_some() {
        return Err(ApiError::Unauthorized);
    }
    Ok(Json(PublicShareResp {
        recording_id: rec_id,
        filename,
        size_bytes,
        created_at,
        download_url: format!("/api/public/recordings/{token}/content"),
        has_password: false,
    }))
}

/// Abre um link com password. A password vai no CORPO — nunca num URL, que
/// fica escrito nos logs de acesso — e a resposta traz o `download_url` já com
/// um passe de leitura de uma hora, para o leitor e o download.
///
/// Cinco passwords erradas em 5 minutos travam o link (`429`).
#[utoipa::path(
    post, path = "/api/public/recordings/{token}/access", tag = "recordings",
    params(("token" = String, Path, description = "Token do link público.")),
    request_body = PublicShareAccessReq,
    responses(
        (status = 200, body = PublicShareResp),
        (status = 401, description = "A password não confere.", body = crate::openapi::ErrorBody),
        (status = 404, description = "Token inexistente ou expirado; ou a gravação não tem ficheiro (falhou, ou o servidor ainda a está a compor).", body = crate::openapi::ErrorBody),
        (status = 429, description = "Cinco passwords erradas em 5 minutos neste link.", body = crate::openapi::ErrorBody),
    )
)]
pub async fn public_share_access(
    State(state): State<Arc<AppState>>,
    Path(token): Path<String>,
    Json(req): Json<PublicShareAccessReq>,
) -> Result<Json<PublicShareResp>, ApiError> {
    let (rec_id, password_hash, filename, size_bytes, created_at) =
        live_share_link(&state, &token).await?;
    let content = format!("/api/public/recordings/{token}/content");
    let download_url = match password_hash.as_deref() {
        None => content,
        Some(hash) => {
            // Antes não havia travão nenhum: a password de um link adivinhava-se
            // ao ritmo que o Argon2 deixasse.
            let key = format!("share:{token}");
            if state.mfa_limiter.is_blocked(&key) {
                return Err(ApiError::TooManyRequests);
            }
            // Um hash ilegível na base conta como password errada (falha fechado).
            if !crate::auth::verify_password(&req.password, hash) {
                if !state.mfa_limiter.check(&key) {
                    return Err(ApiError::TooManyRequests);
                }
                return Err(ApiError::Unauthorized);
            }
            let exp = Utc::now().timestamp() + SHARE_GRANT_SECS;
            format!("{content}?grant={}", share_grant(&state, &token, hash, exp))
        }
    };
    Ok(Json(PublicShareResp {
        recording_id: rec_id,
        filename,
        size_bytes,
        created_at,
        download_url,
        has_password: password_hash.is_some(),
    }))
}

/// Download via link público (sem autenticação — token é a credencial; num
/// link com password, o passe de leitura que `POST …/access` devolveu).
#[utoipa::path(
    get, path = "/api/public/recordings/{token}/content", tag = "recordings",
    params(("token" = String, Path, description = "Token do link público."), PublicShareContentQuery),
    responses(
        (status = 200, body = inline(WebmBytes), content_type = "video/webm", description = "Sempre `Content-Disposition: attachment`."),
        (status = 401, description = "O link tem password e o passe de leitura falta, expirou ou não é deste link.", body = crate::openapi::ErrorBody),
        (status = 404, description = "Token inexistente, expirado, ou sem ficheiro (a gravação falhou, ou o servidor ainda a está a compor).", body = crate::openapi::ErrorBody),
    )
)]
pub async fn public_share_download(
    State(state): State<Arc<AppState>>,
    Path(token): Path<String>,
    Query(q): Query<PublicShareContentQuery>,
) -> Result<impl IntoResponse, ApiError> {
    let (rec_id, password_hash, filename, _, _) = live_share_link(&state, &token).await?;
    if let Some(hash) = password_hash.as_deref() {
        let ok = q
            .grant
            .as_deref()
            .is_some_and(|g| share_grant_is_valid(&state, &token, hash, g));
        if !ok {
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

/// `deny_unknown_fields`: um cliente que ainda mande `title` ou `category` (o
/// contrato antigo) recebe `400` em vez de um `200` que não alterou nada. Um
/// campo que o cliente escreve e o sistema ignora é pior do que um campo que
/// não existe (R184).
#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
#[schema(as = RecordingUpdateReq)]
pub struct UpdateRecordingReq {
    /// Nome de apresentação, 1-200 caracteres, uma linha. É também o nome com
    /// que o ficheiro se descarrega.
    pub filename: Option<String>,
    /// Até 8000 caracteres. `""` apaga a descrição.
    pub description: Option<String>,
    /// Etiquetas: minúsculas, sem `#` nem vírgulas, até 20. Substitui a lista.
    pub tags: Option<Vec<String>>,
    /// `meeting` | `training` | `broadcast` | `hybrid`.
    pub kind: Option<String>,
}

/// Altera nome, descrição, etiquetas e tipo. Só o dono ou um admin activo da
/// org do dono. A publicação NÃO se muda por aqui: é `POST …/publish`.
#[utoipa::path(
    patch, path = "/api/recordings/{recording_id}", tag = "recordings",
    security(("session" = [])),
    params(("recording_id" = Uuid, Path)),
    request_body = UpdateRecordingReq,
    responses(
        (status = 200, body = RecordingItem),
        (status = 400, body = crate::openapi::ErrorBody, description = "`recording.invalid_filename` / `recording.invalid_description` / `recording.invalid_tags` / `recording.invalid_kind`"),
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
    let filename = req
        .filename
        .as_deref()
        .map(rules::validate_filename)
        .transpose()?;
    let description = req
        .description
        .as_deref()
        .map(rules::validate_description)
        .transpose()?;
    let tags = req.tags.as_deref().map(rules::normalize_tags).transpose()?;
    let kind = req.kind.as_deref().map(rules::Kind::parse).transpose()?;
    sqlx::query(
        "UPDATE recordings
            SET filename = COALESCE($2, filename),
                description = COALESCE($3, description),
                tags = COALESCE($4, tags),
                kind = COALESCE($5, kind)
          WHERE id = $1",
    )
    .bind(id)
    .bind(filename)
    .bind(description)
    .bind(tags)
    .bind(kind.map(|k| k.as_str()))
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
    /// Milissegundos desde o início.
    pub t_ms: i64,
    pub title: String,
    /// `auto` (gerado do texto pelo LLM local) | `manual` (escrito à mão).
    /// Voltar a gerar apaga os `auto` e nunca toca nos `manual`.
    pub source: String,
    /// `null` num capítulo automático, ou de uma conta já apagada.
    pub created_by: Option<Uuid>,
    pub created_at: DateTime<Utc>,
}

const CHAPTER_COLUMNS: &str = "id, recording_id, t_ms, title, source, created_by, created_at";

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
    /// Milissegundos desde o início; `0..=duration_ms` (ou `0..=172800000`
    /// quando a duração não é conhecida).
    pub t_ms: i64,
    /// 1-200 caracteres, uma linha.
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
    at: i64,
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
            AND ($2::bigint IS NULL OR (t_ms, id) > ($2, $3))
          ORDER BY t_ms, id
          LIMIT $4"
    ))
    .bind(id)
    .bind(cursor.as_ref().map(|c| c.at))
    .bind(cursor.as_ref().map(|c| c.id).unwrap_or_default())
    .bind(size as i64 + 1)
    .fetch_all(&state.db)
    .await?;
    let p = Page::from_overfetch(rows, size, |c| ChapterCursor {
        at: c.t_ms,
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
    let t_ms = rules::validate_t_ms(req.t_ms, rec.duration_ms)?;
    // O tecto e a inserção numa só instrução.
    let chapter: Option<Chapter> = sqlx::query_as(&format!(
        "INSERT INTO recording_chapters (recording_id, t_ms, title, source, created_by)
         SELECT $1, $2, $3, 'manual', $4
          WHERE (SELECT COUNT(*) FROM recording_chapters WHERE recording_id = $1) < $5
         RETURNING {CHAPTER_COLUMNS}"
    ))
    .bind(id)
    .bind(t_ms)
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
    /// Milissegundos desde o início; `null` = sobre a gravação inteira.
    pub t_ms: Option<i64>,
    /// Já censurado pelo DLP.
    pub body: String,
    pub user_id: Uuid,
    pub username: String,
    pub created_at: DateTime<Utc>,
    pub edited_at: Option<DateTime<Utc>>,
}

/// Comentários vivos (os apagados nunca saem), com o nome do autor.
const COMMENT_SELECT: &str = "SELECT c.id, c.recording_id, c.t_ms, c.body, c.user_id,
        u.username, c.created_at, c.edited_at
   FROM recording_comments c JOIN users u ON u.id = c.user_id
  WHERE c.deleted_at IS NULL";

/// A chave de ordem das sem marca temporal: no fim.
const NO_TIMESTAMP_KEY: i64 = i64::MAX;

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
    /// Milissegundos desde o início; omisso ou `null` = à gravação inteira.
    #[serde(default)]
    pub t_ms: Option<i64>,
}

#[derive(Deserialize, Default, utoipa::ToSchema)]
#[schema(as = RecordingCommentUpdateReq)]
pub struct UpdateCommentReq {
    pub body: Option<String>,
    /// Muda a marca temporal. Não se retira a marca por aqui.
    pub t_ms: Option<i64>,
}

#[derive(Serialize, Deserialize)]
struct CommentCursor {
    k: i64,
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
            AND ($2::bigint IS NULL
                 OR (COALESCE(c.t_ms, {NO_TIMESTAMP_KEY}), c.created_at, c.id) > ($2, $3, $4))
          ORDER BY COALESCE(c.t_ms, {NO_TIMESTAMP_KEY}), c.created_at, c.id
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
        k: c.t_ms.unwrap_or(NO_TIMESTAMP_KEY),
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
    if comment.user_id != user_id {
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
    let t_ms = req
        .t_ms
        .map(|a| rules::validate_t_ms(a, rec.duration_ms))
        .transpose()?;
    // DLP antes de qualquer byte chegar à base: um comentário é lido por toda
    // a gente que vê a gravação, e pode sair num export.
    let body = crate::dlp::censor(&body);
    let (comment_id,): (Uuid,) = sqlx::query_as(
        "INSERT INTO recording_comments (recording_id, t_ms, body, user_id)
         VALUES ($1, $2, $3, $4) RETURNING id",
    )
    .bind(id)
    .bind(t_ms)
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
    let t_ms = req
        .t_ms
        .map(|a| rules::validate_t_ms(a, rec.duration_ms))
        .transpose()?;
    sqlx::query(
        "UPDATE recording_comments
            SET body = COALESCE($3, body), t_ms = COALESCE($4, t_ms), edited_at = now()
          WHERE id = $1 AND recording_id = $2 AND deleted_at IS NULL",
    )
    .bind(comment_id)
    .bind(id)
    .bind(body)
    .bind(t_ms)
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

/// As gravações de uma página de pesquisa, pelos ids e na ordem pedida, com
/// os factos de `user_id`. Só as que ele VÊ na biblioteca `scope`: a pesquisa
/// já filtrou, e isto volta a aplicar `AccessFacts::listed_in` do domínio
/// (defesa em profundidade).
pub(crate) async fn library_items_by_ids(
    state: &AppState,
    user_id: Uuid,
    scope: rules::LibraryScope,
    ids: &[Uuid],
) -> Result<Vec<RecordingItem>, ApiError> {
    let rows: Vec<ItemRow> = sqlx::query_as(&format!("{} WHERE r.id = ANY($2)", *ITEM_SELECT))
        .bind(user_id)
        .bind(ids)
        .fetch_all(&state.db)
        .await?;
    let mut by_id: std::collections::HashMap<Uuid, ItemRow> = rows
        .into_iter()
        .filter(|r| r.facts().listed_in(scope, r.published))
        .map(|r| (r.id, r))
        .collect();
    Ok(ids
        .iter()
        .filter_map(|id| by_id.remove(id))
        .map(|r| r.into_item(None))
        .collect())
}
