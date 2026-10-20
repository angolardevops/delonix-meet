//! Recortes de uma gravação, e o *short* vertical — adaptador HTTP + Postgres
//! + a fila de render.
//!
//! As regras (limites, enquadramento, janela das legendas, política de
//! tentativas) estão em `delonix_meet_domain::content::clip`; o SQL fica aqui,
//! como nos dois módulos irmãos (`recording_captions.rs`, `recording_chapters.rs`)
//! — a passagem para `delonix-meet-store` tem ordem própria (ADR-0006 §6 linha D)
//! e não se antecipa num módulo novo.
//!
//! # Quem pode
//!
//! - **criar e apagar** — quem GERE a gravação (`Access::require_manage`), e
//!   só com ficheiro (`require_file`): recortar uma gravação a compor não dá
//!   nada;
//! - **ver e descarregar** — quem alcança a gravação. Um recorte é um artefacto
//!   dela, e herda-lhe a visibilidade: não há uma segunda porta de acesso (foi
//!   a lição da R304).
//!
//! # A fila
//!
//! Molde único da casa (`delonix_meet_core::jobs`): `status` + `attempts` +
//! `next_attempt_at` + `running_at` como marca de posse, reivindicados pelo
//! `claim` genérico. A marca lê-se DEPOIS da reivindicação e é exigida de volta
//! na escrita final — uma reivindicação velha nunca fecha um render que já é de
//! outra.
//!
//! O ffmpeg corre com o tecto de tempo do `recorder` e por uma vaga de
//! `fair_slots`, as MESMAS do gravador: um agente a pedir vinte recortes não
//! pode afogar as composições de outra organização, nem furar o tecto de
//! processos pesados.

use std::sync::Arc;

use axum::{
    extract::{Path, Query, State},
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use chrono::{DateTime, Utc};
use delonix_meet_core::{
    jobs::{Lease, Queue},
    page::{Page, PageRequest},
};
use delonix_meet_domain::content::clip as rules;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{auth::AuthUser, error::ApiError, AppState};

// ---------- A fila ----------

/// Declarada uma vez, com cada nome de coluna como literal (é o contrato do
/// `core::jobs`).
///
/// A posse tem duas colunas, e a razão é um defeito medido: `running_at` é a
/// MARCA (escrita uma vez pela reivindicação, exigida de volta na escrita
/// final) e `lease_expires_at` é o PRAZO (renovado pelo sinal de vida). Com
/// uma coluna só, o escritor de progresso mudava a marca a cada dois segundos
/// e o `UPDATE … WHERE running_at = $2` final não casava — o render acabava
/// bem e a linha ficava `processing` para sempre.
///
/// `{lease_secs}` vem do [`Lease`] que o `claim` recebe ([`rules::LEASE`]).
const FILA_CLIPS: Queue = Queue {
    name: "recording_clip",
    table: "recording_clips",
    id_column: "id",
    ready_when: "status IN ('queued', 'processing') \
                 AND attempts < {max_attempts} \
                 AND (next_attempt_at IS NULL OR next_attempt_at <= now()) \
                 AND (running_at IS NULL \
                      OR lease_expires_at IS NULL \
                      OR lease_expires_at <= now())",
    claim_set: "status = 'processing', attempts = attempts + 1, \
                next_attempt_at = NULL, running_at = now(), \
                lease_expires_at = now() + make_interval(secs => {lease_secs})",
    returning: "id, recording_id, created_by, kind, start_ms, end_ms, focus, captions_lang",
    order_by: "created_at",
    // A justiça é por quem pediu: um agente de uma organização a encher a fila
    // não empurra os recortes das outras para trás. A coluna é a da pessoa
    // porque é a que existe nesta tabela — a organização de uma gravação
    // resolve-se pela sala, e isso é uma consulta por linha.
    tenant_column: Some("created_by"),
    batch: 2,
};

/// Um recorte reivindicado para compor.
#[derive(sqlx::FromRow)]
struct PorCompor {
    id: Uuid,
    recording_id: Uuid,
    created_by: Uuid,
    kind: String,
    start_ms: i64,
    end_ms: i64,
    focus: String,
    captions_lang: Option<String>,
}

// ---------- O que sai na API ----------

#[derive(Serialize, utoipa::ToSchema, sqlx::FromRow)]
pub struct Clip {
    pub id: Uuid,
    pub recording_id: Uuid,
    pub created_by: Uuid,
    /// `clip` ou `short`.
    pub kind: String,
    pub label: String,
    pub start_ms: i64,
    pub end_ms: i64,
    /// Enquadramento do *short*: `center`, `left` ou `right`.
    pub focus: String,
    /// Língua do WebVTT que sai ao lado, quando se pediu.
    pub captions_lang: Option<String>,
    /// `queued`, `processing`, `ready` ou `failed`.
    pub status: String,
    /// 0 a 1. Lido do `-progress` do ffmpeg.
    pub progress: f64,
    pub attempts: i32,
    /// Tamanho do ficheiro, quando está pronto.
    pub size_bytes: Option<i64>,
    pub duration_ms: Option<i64>,
    /// Código ESTÁVEL da falha (contrato), p. ex. `clip.ffmpeg_failed`.
    pub error_code: Option<String>,
    pub created_at: DateTime<Utc>,
    pub ready_at: Option<DateTime<Utc>>,
}

const COLUNAS: &str = "id, recording_id, created_by, kind, label, start_ms, end_ms, focus, \
                       captions_lang, status, progress, attempts, size_bytes, duration_ms, \
                       error_code, created_at, ready_at";

#[derive(Deserialize, utoipa::ToSchema)]
pub struct CreateClipReq {
    /// `clip` (até 15 min) ou `short` (vertical 9:16, até 3 min).
    pub kind: String,
    pub label: String,
    pub start_ms: i64,
    pub end_ms: i64,
    /// Só do *short*: que parte da largura fica. Omissão: `center`.
    #[serde(default)]
    pub focus: Option<String>,
    /// Produz um WebVTT ao lado, nesta língua. **Não queima** nada na imagem.
    #[serde(default)]
    pub captions_lang: Option<String>,
}

/// `Default` (sem parâmetros) é a primeira página — é o que a tool
/// `recording_clips_list` pede.
#[derive(Deserialize, Default, utoipa::IntoParams)]
#[into_params(parameter_in = Query)]
pub struct PageQuery {
    /// 1–100, omissão 50.
    pub page_size: Option<u32>,
    pub page_token: Option<String>,
}

// ---------- Rotas ----------

/// Onde o recorte está no disco, **para quem o LÊ** (a rota que o serve, a que
/// lê as legendas, e quem o desfaz). Ao lado das gravações, com o seu próprio
/// id — o nome sai do registo de artefactos, que é a única fonte dele.
///
/// Quem PRODUZ não chama isto: chama o `recording_artifacts::reserve`, que
/// escreve a linha e devolve o caminho. Ter aqui um caminho de escrita era ter
/// como nomear um `clip-<id>.webm` sem linha que o apague — a fuga que a 0118
/// deixou aberta quatro meses.
pub(crate) fn clip_path(state: &AppState, id: Uuid) -> std::path::PathBuf {
    crate::recording_artifacts::path_for(
        &state.config.recordings_dir,
        crate::recording_artifacts::Kind::Clip(id),
    )
}

pub(crate) fn clip_vtt_path(state: &AppState, id: Uuid) -> std::path::PathBuf {
    crate::recording_artifacts::path_for(
        &state.config.recordings_dir,
        crate::recording_artifacts::Kind::ClipCaptions(id),
    )
}

/// `POST /api/recordings/{id}/clips` — pede um recorte. `202`: a composição é
/// em segundo plano, e o estado lê-se no próprio recorte.
#[utoipa::path(
    post, path = "/api/recordings/{recording_id}/clips", tag = "recordings",
    security(("session" = [])),
    params(("recording_id" = Uuid, Path, description = "Gravação de origem.")),
    request_body = CreateClipReq,
    responses(
        (status = 202, body = Clip, description = "Na fila. O estado e o progresso lêem-se no recorte."),
        (status = 400, body = crate::openapi::ErrorBody, description = "`clip.invalid_range`, `clip.too_long`, `clip.unknown_kind`, `clip.invalid_label`, `clip.unknown_focus`, `clip.invalid_language`."),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 403, body = crate::openapi::ErrorBody, description = "Vê a gravação mas não a gere."),
        (status = 404, body = crate::openapi::ErrorBody, description = "Não existe — ou é de outra organização."),
        (status = 409, body = crate::openapi::ErrorBody, description = "`recording.processing` ou `recording.no_file`: ainda não há ficheiro para recortar."),
    )
)]
pub async fn create(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(rec_id): Path<Uuid>,
    Json(req): Json<CreateClipReq>,
) -> Result<(StatusCode, Json<Clip>), ApiError> {
    let a = crate::recordings::access(&state, rec_id, auth.user_id).await?;
    a.require_manage()?;
    a.require_file()?;
    let pedido = rules::validate(
        &req.kind,
        &req.label,
        req.start_ms,
        req.end_ms,
        req.focus.as_deref(),
        req.captions_lang.as_deref(),
        a.duration_ms,
    )?;
    let clip: Clip = sqlx::query_as(&format!(
        "INSERT INTO recording_clips
             (recording_id, created_by, kind, label, start_ms, end_ms, focus, captions_lang)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
         RETURNING {COLUNAS}"
    ))
    .bind(rec_id)
    .bind(auth.user_id)
    .bind(pedido.kind.as_str())
    .bind(&pedido.label)
    .bind(pedido.start_ms)
    .bind(pedido.end_ms)
    .bind(pedido.focus.as_str())
    .bind(pedido.captions_lang.as_deref())
    .fetch_one(&state.db)
    .await?;
    crate::audit::log(
        &state.db,
        None,
        auth.user_id,
        "recording.clip_requested",
        &format!("{} ({})", clip.id, pedido.kind.as_str()),
    )
    .await;
    Ok((StatusCode::ACCEPTED, Json(clip)))
}

/// `GET /api/recordings/{id}/clips` — os recortes desta gravação.
#[utoipa::path(
    get, path = "/api/recordings/{recording_id}/clips", tag = "recordings",
    security(("session" = [])),
    params(("recording_id" = Uuid, Path, description = "Gravação."), PageQuery),
    responses(
        (status = 200, body = ClipPage),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 404, body = crate::openapi::ErrorBody),
    )
)]
pub async fn list(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(rec_id): Path<Uuid>,
    Query(q): Query<PageQuery>,
) -> Result<Json<ClipPage>, ApiError> {
    // Ver os recortes é ver a gravação: não há segunda porta (R304).
    crate::recordings::access(&state, rec_id, auth.user_id).await?;
    let pedido = PageRequest {
        page_size: q.page_size,
        page_token: q.page_token.clone(),
    };
    let limite = pedido.size();
    // O cursor é `(created_at, id)` — a mesma ordem do índice.
    let cursor: Option<(DateTime<Utc>, Uuid)> = pedido.cursor()?;
    let itens: Vec<Clip> = sqlx::query_as(&format!(
        "SELECT {COLUNAS} FROM recording_clips
          WHERE recording_id = $1
            AND ($2::timestamptz IS NULL OR (created_at, id) < ($2, $3))
          ORDER BY created_at DESC, id DESC
          LIMIT $4"
    ))
    .bind(rec_id)
    .bind(cursor.map(|(t, _)| t))
    .bind(cursor.map(|(_, i)| i))
    .bind(limite as i64 + 1)
    .fetch_all(&state.db)
    .await?;
    Ok(Json(ClipPage::from(Page::from_overfetch(
        itens,
        limite,
        |c: &Clip| (c.created_at, c.id),
    ))))
}

/// Uma página de recortes.
#[derive(Serialize, utoipa::ToSchema)]
pub struct ClipPage {
    pub items: Vec<Clip>,
    pub next_page_token: Option<String>,
}

impl From<Page<Clip>> for ClipPage {
    fn from(p: Page<Clip>) -> Self {
        ClipPage {
            items: p.items,
            next_page_token: p.next_page_token,
        }
    }
}

/// `GET /api/recordings/{id}/clips/{clip_id}` — um recorte, com o estado.
#[utoipa::path(
    get, path = "/api/recordings/{recording_id}/clips/{clip_id}", tag = "recordings",
    security(("session" = [])),
    params(
        ("recording_id" = Uuid, Path, description = "Gravação."),
        ("clip_id" = Uuid, Path, description = "Recorte."),
    ),
    responses(
        (status = 200, body = Clip),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 404, body = crate::openapi::ErrorBody),
    )
)]
pub async fn get_one(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path((rec_id, clip_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<Clip>, ApiError> {
    crate::recordings::access(&state, rec_id, auth.user_id).await?;
    por_id(&state, rec_id, clip_id).await.map(Json)
}

async fn por_id(state: &AppState, rec_id: Uuid, clip_id: Uuid) -> Result<Clip, ApiError> {
    sqlx::query_as(&format!(
        "SELECT {COLUNAS} FROM recording_clips WHERE id = $1 AND recording_id = $2"
    ))
    .bind(clip_id)
    .bind(rec_id)
    .fetch_optional(&state.db)
    .await?
    .ok_or(ApiError::NotFound)
}

/// `GET /api/recordings/{id}/clips/{clip_id}/content` — o ficheiro.
#[utoipa::path(
    get, path = "/api/recordings/{recording_id}/clips/{clip_id}/content", tag = "recordings",
    security(("session" = [])),
    params(
        ("recording_id" = Uuid, Path, description = "Gravação."),
        ("clip_id" = Uuid, Path, description = "Recorte."),
    ),
    responses(
        (status = 200, description = "O WebM do recorte.", content_type = "video/webm"),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 404, body = crate::openapi::ErrorBody),
        (status = 422, body = crate::openapi::ErrorBody, description = "`clip.not_ready` ainda a compor, `clip.failed` falhou."),
    )
)]
pub async fn content(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path((rec_id, clip_id)): Path<(Uuid, Uuid)>,
) -> Result<Response, ApiError> {
    crate::recordings::access(&state, rec_id, auth.user_id).await?;
    let clip = por_id(&state, rec_id, clip_id).await?;
    let estado = rules::Status::parse(&clip.status).ok_or_else(|| {
        ApiError::Internal(format!("estado de recorte desconhecido: {}", clip.status))
    })?;
    rules::ensure_downloadable(estado)?;
    let path = clip_path(&state, clip_id);
    let bytes = tokio::fs::read(&path)
        .await
        .map_err(|_| ApiError::NotFound)?;
    let nome = format!("{}.webm", sanitiza(&clip.label));
    Ok((
        [
            (header::CONTENT_TYPE, "video/webm".to_string()),
            (
                header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"{nome}\""),
            ),
        ],
        bytes,
    )
        .into_response())
}

/// `GET /api/recordings/{id}/clips/{clip_id}/captions.vtt` — as legendas do
/// recorte, quando se pediram.
#[utoipa::path(
    get, path = "/api/recordings/{recording_id}/clips/{clip_id}/captions.vtt", tag = "recordings",
    security(("session" = [])),
    params(
        ("recording_id" = Uuid, Path, description = "Gravação."),
        ("clip_id" = Uuid, Path, description = "Recorte."),
    ),
    responses(
        (status = 200, description = "WebVTT com as deixas da janela, a começar em zero.", content_type = "text/vtt"),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 404, body = crate::openapi::ErrorBody, description = "O recorte não pediu legendas, ou ainda não está pronto."),
    )
)]
pub async fn captions(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path((rec_id, clip_id)): Path<(Uuid, Uuid)>,
) -> Result<Response, ApiError> {
    crate::recordings::access(&state, rec_id, auth.user_id).await?;
    let clip = por_id(&state, rec_id, clip_id).await?;
    if clip.captions_lang.is_none() {
        return Err(ApiError::NotFound);
    }
    let texto = tokio::fs::read_to_string(clip_vtt_path(&state, clip_id))
        .await
        .map_err(|_| ApiError::NotFound)?;
    Ok((
        [(header::CONTENT_TYPE, "text/vtt; charset=utf-8".to_string())],
        texto,
    )
        .into_response())
}

/// `DELETE /api/recordings/{id}/clips/{clip_id}` — apaga o recorte e o ficheiro.
#[utoipa::path(
    delete, path = "/api/recordings/{recording_id}/clips/{clip_id}", tag = "recordings",
    security(("session" = [])),
    params(
        ("recording_id" = Uuid, Path, description = "Gravação."),
        ("clip_id" = Uuid, Path, description = "Recorte."),
    ),
    responses(
        (status = 204, description = "Apagado."),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 403, body = crate::openapi::ErrorBody),
        (status = 404, body = crate::openapi::ErrorBody),
    )
)]
pub async fn delete(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path((rec_id, clip_id)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, ApiError> {
    let a = crate::recordings::access(&state, rec_id, auth.user_id).await?;
    a.require_manage()?;
    let apagou = sqlx::query("DELETE FROM recording_clips WHERE id = $1 AND recording_id = $2")
        .bind(clip_id)
        .bind(rec_id)
        .execute(&state.db)
        .await?
        .rows_affected();
    if apagou == 0 {
        return Err(ApiError::NotFound);
    }
    // O ficheiro depois da linha: a ordem inversa deixava uma linha a apontar
    // para nada. Se o processo morrer entre os dois, os ficheiros ficam — e
    // AGORA têm quem os recolha: as linhas de `recording_artifacts` sobrevivem
    // ao recorte (não têm FK), logo a purga da gravação leva-os. Era aqui que a
    // fuga 2 vivia, e o comentário que dizia que «a varredura de órfãos do
    // recorder já sabe limpar» estava simplesmente errado: ela só olhava para
    // `tmp-*`.
    // Um só caminho para os dois ficheiros e as duas linhas, e a ordem certa
    // lá dentro: o ficheiro primeiro, a linha só se ele saiu. Escrito à mão
    // (`let _ = remove_file(...)` e depois um `forget` incondicional), um
    // `unlink` recusado apagava a única linha que nomeava bytes que ficavam no
    // disco — e, como a linha do recorte já cá não está, nada voltaria a
    // encontrá-los.
    let ficheiros = [clip_path(&state, clip_id), clip_vtt_path(&state, clip_id)];
    crate::recording_artifacts::discard(
        &state.db,
        &state.config.recordings_dir,
        &state.metrics,
        &ficheiros,
    )
    .await;
    crate::audit::log(
        &state.db,
        None,
        auth.user_id,
        "recording.clip_deleted",
        &clip_id.to_string(),
    )
    .await;
    Ok(StatusCode::NO_CONTENT)
}

/// Nome de ficheiro a partir do rótulo: sem caminho, sem controlo, sem aspas.
fn sanitiza(label: &str) -> String {
    let s: String = label
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect();
    let s = s.trim_matches('-').to_string();
    if s.is_empty() {
        "recorte".into()
    } else {
        s.chars().take(60).collect()
    }
}

// ---------- A fila: compor ----------

/// Uma volta da fila. Reivindica até dois recortes e compõe-nos.
pub async fn render_due(state: &Arc<AppState>) -> Result<usize, sqlx::Error> {
    let levados: Vec<PorCompor> = crate::jobs::claim(
        state,
        &crate::jobs::Worker {
            queue: FILA_CLIPS,
            retry: rules::RETRY,
            lease: Some(Lease {
                duration: rules::LEASE,
            }),
        },
    )
    .await?;
    let n = levados.len();
    for job in levados {
        // A marca desta reivindicação, lida DEPOIS do `claim_set` (a escolha e
        // a marca são duas instruções da mesma transacção, por isso não vem no
        // `returning`). É ela que a escrita final exige de volta.
        let marca: Option<(DateTime<Utc>,)> =
            sqlx::query_as("SELECT running_at FROM recording_clips WHERE id = $1")
                .bind(job.id)
                .fetch_optional(&state.db)
                .await?;
        let Some((running_at,)) = marca else {
            tracing::warn!(clip = %job.id, "recorte reivindicado sem marca de posse");
            continue;
        };
        // Um render por vez nesta volta, de propósito: o tecto de processos
        // pesados é o do `fair_slots`, não o do tamanho do lote.
        compoe_um(state, job, running_at).await;
    }
    Ok(n)
}

async fn compoe_um(state: &Arc<AppState>, job: PorCompor, running_at: DateTime<Utc>) {
    match compoe(state, &job).await {
        Ok((size, duration_ms)) => {
            let fechou = sqlx::query(
                "UPDATE recording_clips
                    SET status = 'ready', progress = 1, size_bytes = $3, duration_ms = $4,
                        filename = $5, ready_at = now(), error_code = NULL,
                        lease_expires_at = NULL
                  WHERE id = $1 AND running_at = $2",
            )
            .bind(job.id)
            .bind(running_at)
            .bind(size)
            .bind(duration_ms)
            .bind(format!("clip-{}.webm", job.id))
            .execute(&state.db)
            .await
            .map(|r| r.rows_affected())
            .unwrap_or(0);
            if fechou > 0 {
                tracing::info!(clip = %job.id, size, "recorte composto");
            } else {
                // Zero linhas tem DUAS causas, e só uma delas autoriza apagar.
                // Se a linha do recorte ainda existe, a reivindicação é de
                // outra volta (`running_at` mudou) e o ficheiro é dela — não se
                // toca. Se já não existe, a gravação foi apagada enquanto o
                // ffmpeg corria (a retenção ou uma cascata levaram o recorte) e
                // o ficheiro que acabámos de escrever não tem dono nenhum: o
                // `ffmpeg` escreve-o num descritor que a purga já não vê, logo
                // sem isto ficava no disco para sempre.
                let existe: Option<(i32,)> =
                    sqlx::query_as("SELECT 1 FROM recording_clips WHERE id = $1")
                        .bind(job.id)
                        .fetch_optional(&state.db)
                        .await
                        .unwrap_or(None);
                if existe.is_none() {
                    tracing::warn!(clip = %job.id,
                        "o recorte deixou de existir enquanto compunha — o ficheiro é descartado");
                    crate::recording_artifacts::discard(
                        &state.db,
                        &state.config.recordings_dir,
                        &state.metrics,
                        &[clip_path(state, job.id), clip_vtt_path(state, job.id)],
                    )
                    .await;
                } else {
                    tracing::warn!(clip = %job.id,
                        "a reivindicação já não é desta volta — o ficheiro fica para quem a tem");
                }
            }
        }
        Err(e) => {
            let codigo = codigo_de_erro(&e);
            tracing::warn!(clip = %job.id, error = %e, codigo, "recorte falhou");
            // `attempts` já foi contado pela reivindicação. Com tentativas a
            // sobrar volta a `queued` com espera; na última fica `failed` — e
            // em nenhum caso se apaga o que havia.
            let _ = sqlx::query(
                "UPDATE recording_clips
                    SET status = CASE WHEN attempts >= $3 THEN 'failed' ELSE 'queued' END,
                        error_code = $4,
                        next_attempt_at = CASE WHEN attempts >= $3 THEN NULL
                                               ELSE now() + make_interval(secs => $5) END,
                        running_at = NULL, lease_expires_at = NULL
                  WHERE id = $1 AND running_at = $2",
            )
            .bind(job.id)
            .bind(running_at)
            .bind(rules::RETRY.max_attempts)
            .bind(codigo)
            .bind(rules::RETRY.delays.first().copied().unwrap_or(300) as f64)
            .execute(&state.db)
            .await;
            let _ = tokio::fs::remove_file(clip_path(state, job.id)).await;
        }
    }
}

/// O código ESTÁVEL que o cliente vê. O texto do erro NÃO sai: o stderr do
/// ffmpeg traz caminhos do servidor.
fn codigo_de_erro(e: &anyhow::Error) -> &'static str {
    let t = e.to_string();
    if t.contains("ffmpeg-ausente") {
        "clip.ffmpeg_missing"
    } else if t.contains("source-missing") {
        "clip.source_missing"
    } else if t.contains("timed out") || t.contains("prazo") {
        "clip.timeout"
    } else if t.contains("artefacto-nao-registado") {
        // O registo do artefacto não respondeu, e o ffmpeg nem correu. Sem um
        // código próprio isto caía no `clip.ffmpeg_failed` abaixo e mandava
        // quem opera investigar o codificador por uma falha da base — e o
        // `error_code` é contrato: a consola e a tool mostram-no.
        "clip.registry_unavailable"
    } else {
        "clip.ffmpeg_failed"
    }
}

/// Compõe um recorte. Devolve `(tamanho, duração em ms)`.
async fn compoe(state: &Arc<AppState>, job: &PorCompor) -> anyhow::Result<(i64, i64)> {
    let src =
        crate::recording_artifacts::composed_path(&state.config.recordings_dir, job.recording_id);
    if !tokio::fs::try_exists(&src).await.unwrap_or(false) {
        anyhow::bail!("source-missing: {}", src.display());
    }
    // A LINHA ANTES DO FICHEIRO (`recording_artifacts`, migração 0126): o
    // ffmpeg abaixo escreve o `clip-<id>.webm`, e sem registo esse ficheiro
    // sobrevivia à gravação inteira — o `ON DELETE CASCADE` da 0118 levava a
    // linha do recorte e os bytes ficavam, sem nada na base que os nomeasse.
    // O `out` SAI da reserva: sem linha não há caminho para dar ao ffmpeg, e o
    // render aborta — o recorte volta à fila (é retentável) e não se deixa um
    // ficheiro sem dono no disco.
    let Some(out) = crate::recording_artifacts::reserve(
        &state.db,
        job.recording_id,
        crate::recording_artifacts::Kind::Clip(job.id),
        &state.config.recordings_dir,
    )
    .await
    else {
        anyhow::bail!("artefacto-nao-registado: clip {}", job.id);
    };

    let kind = rules::Kind::parse(&job.kind).unwrap_or(rules::Kind::Clip);
    let focus = rules::Focus::parse(&job.focus).unwrap_or_default();
    let crop = if kind == rules::Kind::Short {
        // A dimensão da origem já foi medida no upload/composição; se faltar,
        // não se inventa: sem ela o short sai sem corte, e isso diz-se no erro
        // em vez de produzir uma imagem distorcida.
        let dim: Option<(Option<i32>, Option<i32>)> =
            sqlx::query_as("SELECT width, height FROM recordings WHERE id = $1")
                .bind(job.recording_id)
                .fetch_optional(&state.db)
                .await?;
        match dim {
            Some((Some(w), Some(h))) if w > 0 && h > 0 => {
                Some(rules::vertical_crop(w as u32, h as u32, focus))
            }
            _ => anyhow::bail!("source-missing: dimensão da gravação desconhecida"),
        }
    } else {
        None
    };

    let mut cmd = tokio::process::Command::new(&state.config.ffmpeg_bin);
    cmd.args(clip_args(
        &src,
        &out,
        job.start_ms,
        job.end_ms - job.start_ms,
        crop,
    ));

    let limite = std::time::Duration::from_secs(state.config.ffmpeg_timeout_secs);
    // Vaga ANTES de arrancar o ffmpeg: esperar a vez não conta como tempo de
    // composição. E é a MESMA pilha de vagas do gravador — um recorte não é um
    // caminho para furar o tecto de processos pesados.
    let _slot = crate::recorder::wait_for_slot(
        state.compose_slots.clone(),
        job.created_by,
        std::time::Duration::from_secs(20),
        || async {
            // Sinal de vida enquanto espera vaga: renova o PRAZO e não a
            // marca. Esperar a vez não pode fazer a linha parecer de outra
            // reivindicação.
            let _ = sqlx::query(
                "UPDATE recording_clips
                    SET lease_expires_at = now() + make_interval(secs => $2)
                  WHERE id = $1 AND status = 'processing'",
            )
            .bind(job.id)
            .bind(rules::LEASE.as_secs() as f64)
            .execute(&state.db)
            .await;
        },
    )
    .await?;

    let (tx, rx) = tokio::sync::watch::channel(0i64);
    let esperado = job.end_ms - job.start_ms;
    let escritor = tokio::spawn(escreve_progresso(state.clone(), job.id, esperado, rx));
    let status = crate::recorder::run_bounded_progress(&mut cmd, limite, Some(tx)).await;
    escritor.abort();
    let status = status?;
    if !status.success() {
        anyhow::bail!("ffmpeg exited with {status}");
    }

    if let Some(lang) = &job.captions_lang {
        escreve_vtt(state, job, lang).await;
    }

    let size = tokio::fs::metadata(&out).await?.len() as i64;
    Ok((size, esperado))
}

/// Escreve o progresso lido do ffmpeg, no máximo a cada 2 s.
async fn escreve_progresso(
    state: Arc<AppState>,
    id: Uuid,
    esperado_ms: i64,
    mut rx: tokio::sync::watch::Receiver<i64>,
) {
    let mut ultimo = std::time::Instant::now() - std::time::Duration::from_secs(5);
    while rx.changed().await.is_ok() {
        let feito = *rx.borrow();
        if ultimo.elapsed() < std::time::Duration::from_secs(2) {
            continue;
        }
        ultimo = std::time::Instant::now();
        let p = if esperado_ms > 0 {
            (feito as f64 / esperado_ms as f64).clamp(0.0, 0.99)
        } else {
            0.0
        };
        // O PRAZO renova-se com o progresso; a MARCA fica. Um render que anda
        // não é reivindicado por outro nó, e a escrita final continua a
        // reconhecer a sua própria reivindicação.
        let _ = sqlx::query(
            "UPDATE recording_clips
                SET progress = $2, lease_expires_at = now() + make_interval(secs => $3)
              WHERE id = $1 AND status = 'processing'",
        )
        .bind(id)
        .bind(p)
        .bind(rules::LEASE.as_secs() as f64)
        .execute(&state.db)
        .await;
    }
}

/// Escreve o WebVTT do recorte, a partir das legendas que a gravação já tem.
///
/// Falhar aqui **não** falha o recorte: o vídeo está composto, e um recorte sem
/// o ficheiro de legendas é melhor do que nenhum recorte. O que não há é
/// silêncio — fica no registo.
async fn escreve_vtt(state: &Arc<AppState>, job: &PorCompor, lang: &str) {
    // A legenda está guardada como WebVTT de TEXTO (migração 0059), não como
    // deixas em JSON: lê-se com o `parse_vtt` que já existe, em vez de um
    // segundo leitor de VTT nesta casa.
    let linha: Result<Option<(String,)>, _> = sqlx::query_as(
        "SELECT vtt FROM recording_captions
          WHERE recording_id = $1 AND lang = $2 AND status IN ('draft', 'published')",
    )
    .bind(job.recording_id)
    .bind(lang)
    .fetch_optional(&state.db)
    .await;
    let Ok(Some((vtt_origem,))) = linha else {
        tracing::info!(clip = %job.id, lang, "recorte: sem legendas prontas nesta língua");
        return;
    };
    let Ok(cues) = crate::recording_captions::parse_vtt(&vtt_origem) else {
        tracing::warn!(clip = %job.id, lang, "recorte: o VTT da gravação não se consegue ler");
        return;
    };
    let todas: Vec<rules::Cue> = cues
        .into_iter()
        .map(|c| (c.start_ms, c.end_ms, c.text))
        .collect();
    let janela = rules::cues_in_window(&todas, job.start_ms, job.end_ms);
    if janela.is_empty() {
        return;
    }
    let vtt = crate::recording_captions::cues_to_vtt(
        &janela
            .into_iter()
            .map(|(a, b, t)| crate::recording_captions::Cue {
                start_ms: a,
                end_ms: b,
                text: t,
            })
            .collect::<Vec<_>>(),
    );
    // A LINHA ANTES DO FICHEIRO, como no `.webm` do recorte. Aqui a falha do
    // registo só custa as legendas (o vídeo está composto e é melhor um recorte
    // sem legendas do que nenhum), por isso diz-se e sai — nunca se escreve o
    // ficheiro sem a linha, que era a fuga.
    let Some(vtt_path) = crate::recording_artifacts::reserve(
        &state.db,
        job.recording_id,
        crate::recording_artifacts::Kind::ClipCaptions(job.id),
        &state.config.recordings_dir,
    )
    .await
    else {
        tracing::warn!(clip = %job.id, "recorte: o .vtt não ficou reservado — não se escreve");
        return;
    };
    if let Err(e) = tokio::fs::write(&vtt_path, vtt).await {
        tracing::warn!(clip = %job.id, error = %e, "recorte: não consegui escrever o .vtt");
    }
}

/// Os argumentos do ffmpeg para um recorte.
///
/// **Recodifica sempre** (ver o cabeçalho do domínio): `-c copy` só é exacto
/// num keyframe, e um recorte que começa «por volta de» não serve. VP9 + Opus,
/// que é o que a imagem LGPL do servidor tem — e o mesmo par que a composição
/// em grelha já usa.
///
/// `-ss` ANTES do `-i` (procura rápida) com `-t` (duração), e não `-to`: com
/// procura de entrada o `-to` conta do início do OUTPUT, e a diferença entre os
/// dois é um recorte com o tamanho errado.
///
/// `-map 0:v:0?` e `-map 0:a:0?` com `?`: uma gravação sem áudio não falha.
fn clip_args(
    src: &std::path::Path,
    out: &std::path::Path,
    start_ms: i64,
    duration_ms: i64,
    crop: Option<rules::Crop>,
) -> Vec<std::ffi::OsString> {
    let mut a: Vec<std::ffi::OsString> = vec![
        "-nostdin".into(),
        "-y".into(),
        "-ss".into(),
        format!("{:.3}", start_ms as f64 / 1000.0).into(),
        "-i".into(),
        src.into(),
        "-t".into(),
        format!("{:.3}", duration_ms as f64 / 1000.0).into(),
        "-map".into(),
        "0:v:0?".into(),
        "-map".into(),
        "0:a:0?".into(),
    ];
    if let Some(c) = crop {
        let (ow, oh) = rules::vertical_output(c);
        a.push("-vf".into());
        a.push(format!("crop={}:{}:{}:{},scale={}:{}", c.w, c.h, c.x, c.y, ow, oh).into());
    }
    a.extend::<Vec<std::ffi::OsString>>(vec![
        "-c:v".into(),
        "libvpx-vp9".into(),
        "-crf".into(),
        "32".into(),
        "-b:v".into(),
        "0".into(),
        // `good` + `cpu-used=4` + `row-mt`: o compromisso que compõe um recorte
        // de minutos em tempo útil. `best` triplica o tempo para uma diferença
        // que ninguém vê num vídeo de reunião.
        "-deadline".into(),
        "good".into(),
        "-cpu-used".into(),
        "4".into(),
        "-row-mt".into(),
        "1".into(),
        "-c:a".into(),
        "libopus".into(),
        "-b:a".into(),
        "128k".into(),
        "-progress".into(),
        "pipe:1".into(),
        "-nostats".into(),
    ]);
    a.push(out.into());
    a
}

#[derive(utoipa::OpenApi)]
#[openapi(
    paths(create, list, get_one, content, captions, delete),
    components(schemas(Clip, ClipPage, CreateClipReq))
)]
pub struct ApiDoc;

#[cfg(test)]
mod tests {
    use super::*;

    fn args(crop: Option<rules::Crop>) -> Vec<String> {
        clip_args(
            std::path::Path::new("/rec/a.webm"),
            std::path::Path::new("/rec/clip-b.webm"),
            65_500,
            12_000,
            crop,
        )
        .into_iter()
        .map(|s| s.to_string_lossy().into_owned())
        .collect()
    }

    #[test]
    fn o_corte_e_exacto_e_recodifica() {
        let a = args(None);
        // `-ss` ANTES do `-i`: procura rápida.
        let ss = a.iter().position(|x| x == "-ss").unwrap();
        let i = a.iter().position(|x| x == "-i").unwrap();
        assert!(ss < i, "o -ss tem de vir antes do -i: {a:?}");
        assert_eq!(a[ss + 1], "65.500");
        // `-t` (duração) e nunca `-to`: com procura de entrada o `-to` contava
        // do início do output e o recorte saía com o tamanho errado.
        assert!(a.contains(&"-t".to_string()));
        assert!(!a.contains(&"-to".to_string()));
        assert_eq!(a[a.iter().position(|x| x == "-t").unwrap() + 1], "12.000");
        // Recodifica — nunca `-c copy`.
        assert!(a.windows(2).any(|w| w[0] == "-c:v" && w[1] == "libvpx-vp9"));
        assert!(a.windows(2).any(|w| w[0] == "-c:a" && w[1] == "libopus"));
        assert!(
            !a.iter().any(|x| x == "copy"),
            "cortar em cópia não é exacto"
        );
        // Sem áudio não falha.
        assert!(a.contains(&"0:a:0?".to_string()));
        assert!(a.contains(&"0:v:0?".to_string()));
        // Progresso legível para a barra.
        assert!(a
            .windows(2)
            .any(|w| w[0] == "-progress" && w[1] == "pipe:1"));
        // Um recorte normal não leva filtro de imagem.
        assert!(!a.contains(&"-vf".to_string()));
        assert_eq!(a.last().unwrap(), "/rec/clip-b.webm");
    }

    #[test]
    fn o_short_corta_e_reduz_numa_so_cadeia() {
        let crop = rules::vertical_crop(1280, 720, rules::Focus::Center);
        let a = args(Some(crop));
        let vf = &a[a.iter().position(|x| x == "-vf").unwrap() + 1];
        assert_eq!(vf, "crop=404:720:438:0,scale=404:720");
        // E uma origem grande reduz-se a 1920 de altura.
        //
        // 1078 e não 1080, de propósito: 2160 × 9/16 = 1215, que é ÍMPAR, e o
        // VP9 recusa lados ímpares — o corte fica em 1214. Reduzi-lo a 1920 de
        // altura dá 1078, e a saída guarda a proporção do CORTE em vez de
        // esticar dois pixels para fingir um 9:16 exacto. É a promessa do
        // módulo: corta, nunca estica.
        let grande = rules::vertical_crop(3840, 2160, rules::Focus::Left);
        let a = args(Some(grande));
        let vf = &a[a.iter().position(|x| x == "-vf").unwrap() + 1];
        assert!(vf.starts_with("crop=1214:2160:0:0"), "{vf}");
        assert!(vf.ends_with(",scale=1078:1920"), "{vf}");
    }

    #[test]
    fn o_nome_do_ficheiro_nao_sai_do_sitio() {
        // `char::is_alphanumeric` é de UNICODE: o `ç` fica, e é melhor assim —
        // o nome do ficheiro continua legível em português. O que não fica é o
        // que mexe com caminhos e com o cabeçalho.
        assert_eq!(sanitiza("Troço do Kilamba"), "Troço-do-Kilamba");
        assert_eq!(sanitiza("../../etc/passwd"), "etc-passwd");
        assert_eq!(sanitiza(""), "recorte");
        assert_eq!(sanitiza("---"), "recorte");
        assert!(!sanitiza("a\"b").contains('"'));
        assert!(sanitiza(&"x".repeat(200)).chars().count() <= 60);
    }
}
