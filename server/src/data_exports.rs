//! «Os meus dados» — exportação pessoal assíncrona. As regras (limite,
//! validade, link assinado) estão em `delonix_meet_domain::identity::data_export`.
//!
//! Contrato (BFF, sessão, sempre a própria conta):
//! - `POST /api/users/me/data-exports`                              pede → `202` + `Location` + o pedido (`queued`)
//! - `GET  /api/users/me/data-exports`                              os meus pedidos, mais recentes primeiro
//! - `GET  /api/users/me/data-exports/{export_id}`                  um pedido (estado, tamanho, validade)
//! - `POST /api/users/me/data-exports/{export_id}/download-link`    método personalizado: link temporário (15 min)
//! - `GET  /api/users/me/data-exports/{export_id}/content?exp&sig`  o ZIP, SEM sessão: a credencial é a assinatura
//!
//! **O que o ZIP leva** (tudo da própria pessoa, e só dela):
//! - `profile.json` — o perfil (como `GET /api/users/me/profile`);
//! - `preferences.json` — entrada nas sessões, notificações, guia;
//! - `recordings.json` — as gravações que CARREGOU, como links para
//!   `/api/recordings/{id}/content` (os bytes não vão: são o grosso da conta e a
//!   descarga continua sujeita às regras de acesso de sempre);
//! - `transcripts/<recording_id>.txt` — as transcrições dessas gravações;
//! - `activity.json` — o registo de auditoria em que a pessoa é o ACTOR. O
//!   `target` só vai nas acções sobre a própria conta (`auth.*`, `session.*`,
//!   `profile.*`, `security.*`, `personal_room.*`): nas outras (p.ex.
//!   `member.added`) o alvo é outra pessoa, e sai `target_redacted: true`;
//! - `storage_usage.json` — o uso de armazenamento (G3, `usage.rs`).
//!
//! **Limite honesto:** uma transcrição de uma reunião contém a fala de outras
//! pessoas. Vai, porque a gravação é da pessoa que a carregou e o ecrã promete
//! «gravações e transcrições»; fica escrito aqui e no relatório.
//!
//! **Trabalho:** uma tarefa reivindica um pedido `queued` com `FOR UPDATE SKIP
//! LOCKED` (várias réplicas não fazem o mesmo), escreve `<id>.zip.part` e muda o
//! nome no fim. Um `running` abandonado (processo morto) volta à fila ao fim de
//! `LEASE_SECS`. O ficheiro apaga-se ao fim de `FILE_TTL_HOURS` e o pedido fica
//! `expired`. O ficheiro está no disco do nó (`DATA_EXPORTS_DIR`), como as
//! gravações: com várias réplicas, o directório tem de ser partilhado.

use axum::{
    body::Body,
    extract::{Path, Query, State},
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use chrono::{DateTime, Utc};
use delonix_meet_core::page::{Page, PageRequest};
use delonix_meet_core::{crypto, DomainError};
use delonix_meet_domain::identity::data_export as rules;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use uuid::Uuid;

use crate::{auth::AuthUser, error::ApiError, AppState};

#[derive(Debug, Serialize, sqlx::FromRow, utoipa::ToSchema)]
pub struct DataExport {
    pub id: Uuid,
    /// `queued` | `running` | `ready` | `failed` | `expired`.
    pub status: String,
    /// Motivo para pessoas, quando `failed`.
    pub error: Option<String>,
    pub size_bytes: Option<i64>,
    /// O que entrou: `recordings`, `transcripts`, `activity_events`.
    #[schema(value_type = Object)]
    pub summary: serde_json::Value,
    pub created_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
    /// Quando o ficheiro é apagado (só com `ready`).
    pub expires_at: Option<DateTime<Utc>>,
}

const COLUMNS: &str =
    "id, status, error, size_bytes, summary, created_at, completed_at, expires_at";

#[derive(Serialize, utoipa::ToSchema)]
pub struct DataExportPage {
    pub items: Vec<DataExport>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_page_token: Option<String>,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct DownloadLink {
    /// Caminho relativo, carregável sem sessão até `expires_at`.
    pub url: String,
    pub expires_at: DateTime<Utc>,
}

#[derive(Deserialize, utoipa::IntoParams)]
#[into_params(parameter_in = Query)]
pub struct ListQuery {
    /// 1-100, omissão 50.
    pub page_size: Option<u32>,
    pub page_token: Option<String>,
}

#[derive(Deserialize, utoipa::IntoParams)]
#[into_params(parameter_in = Query)]
pub struct ContentQuery {
    pub exp: Option<String>,
    pub sig: Option<String>,
}

#[derive(Serialize, Deserialize)]
struct Cursor {
    at: DateTime<Utc>,
    id: Uuid,
}

#[derive(utoipa::OpenApi)]
#[openapi(
    paths(create, list, get_one, download_link, content),
    components(schemas(DataExport, DataExportPage, DownloadLink))
)]
pub struct ApiDoc;

fn signing_key(state: &AppState) -> [u8; 32] {
    crypto::derive_key(&state.config.jwt_secret, rules::KEY_PURPOSE)
}

fn file_path(state: &AppState, id: Uuid) -> std::path::PathBuf {
    state.config.data_exports_dir.join(format!("{id}.zip"))
}

fn not_found() -> ApiError {
    DomainError::not_found("data_export.not_found").into()
}

async fn fetch_own(state: &AppState, user_id: Uuid, id: Uuid) -> Result<DataExport, ApiError> {
    sqlx::query_as(&format!(
        "SELECT {COLUMNS} FROM data_exports WHERE id = $1 AND user_id = $2"
    ))
    .bind(id)
    .bind(user_id)
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(not_found)
}

/// Pede uma exportação dos meus dados. Uma de cada vez e no máximo 3 por 24 h.
#[utoipa::path(
    post, path = "/api/users/me/data-exports", tag = "account",
    security(("session" = [])),
    responses(
        (status = 202, body = DataExport, headers(("Location" = String))),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 409, description = "Já há uma em curso (`data_export.already_running`).", body = crate::openapi::ErrorBody),
        (status = 429, description = "Três nas últimas 24 h (`data_export.rate_limited`).", body = crate::openapi::ErrorBody),
    )
)]
pub async fn create(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
) -> Result<Response, ApiError> {
    let (active, last_24h, oldest): (i64, i64, Option<DateTime<Utc>>) = sqlx::query_as(
        "SELECT COUNT(*) FILTER (WHERE status IN ('queued', 'running')),
                COUNT(*) FILTER (WHERE created_at > now() - interval '24 hours'),
                MIN(created_at) FILTER (WHERE created_at > now() - interval '24 hours')
           FROM data_exports WHERE user_id = $1",
    )
    .bind(auth.user_id)
    .fetch_one(&state.db)
    .await?;
    rules::check_rate(active, last_24h, oldest, Utc::now())?;
    let export: DataExport = sqlx::query_as(&format!(
        "INSERT INTO data_exports (user_id) VALUES ($1) RETURNING {COLUMNS}"
    ))
    .bind(auth.user_id)
    .fetch_one(&state.db)
    .await
    .map_err(|e| match &e {
        // Dois pedidos ao mesmo tempo: o índice único é o árbitro.
        sqlx::Error::Database(db) if db.is_unique_violation() => DomainError::conflict(
            "data_export.already_running",
            "já há uma exportação em curso: espere que termine",
        )
        .into(),
        _ => ApiError::from(e),
    })?;
    crate::audit::log(
        &state.db,
        None,
        auth.user_id,
        "data_export.requested",
        &export.id.to_string(),
    )
    .await;
    // Começa já, sem esperar pelo varrimento periódico.
    let worker = state.clone();
    tokio::spawn(async move { run_queue(&worker).await });
    Ok((
        StatusCode::ACCEPTED,
        [(
            header::LOCATION,
            format!("/api/users/me/data-exports/{}", export.id),
        )],
        Json(export),
    )
        .into_response())
}

/// Os meus pedidos de exportação, mais recentes primeiro.
#[utoipa::path(
    get, path = "/api/users/me/data-exports", tag = "account",
    security(("session" = [])),
    params(ListQuery),
    responses(
        (status = 200, body = DataExportPage),
        (status = 400, description = "page_token inválido", body = crate::openapi::ErrorBody),
        (status = 401, body = crate::openapi::ErrorBody),
    )
)]
pub async fn list(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Query(q): Query<ListQuery>,
) -> Result<Json<DataExportPage>, ApiError> {
    let page = PageRequest {
        page_size: q.page_size,
        page_token: q.page_token,
    };
    let size = page.size();
    let cursor: Option<Cursor> = page.cursor()?;
    let rows: Vec<DataExport> = sqlx::query_as(&format!(
        "SELECT {COLUMNS} FROM data_exports
          WHERE user_id = $1 AND ($2::timestamptz IS NULL OR (created_at, id) < ($2, $3))
          ORDER BY created_at DESC, id DESC LIMIT $4"
    ))
    .bind(auth.user_id)
    .bind(cursor.as_ref().map(|c| c.at))
    .bind(cursor.as_ref().map(|c| c.id).unwrap_or_default())
    .bind(size as i64 + 1)
    .fetch_all(&state.db)
    .await?;
    let p = Page::from_overfetch(rows, size, |e| Cursor {
        at: e.created_at,
        id: e.id,
    });
    Ok(Json(DataExportPage {
        items: p.items,
        next_page_token: p.next_page_token,
    }))
}

/// Um pedido meu.
#[utoipa::path(
    get, path = "/api/users/me/data-exports/{export_id}", tag = "account",
    security(("session" = [])),
    params(("export_id" = Uuid, Path)),
    responses(
        (status = 200, body = DataExport),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 404, description = "Não existe ou é de outra pessoa (`data_export.not_found`).", body = crate::openapi::ErrorBody),
    )
)]
pub async fn get_one(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(export_id): Path<Uuid>,
) -> Result<Json<DataExport>, ApiError> {
    Ok(Json(fetch_own(&state, auth.user_id, export_id).await?))
}

/// Método personalizado: link temporário (15 min) para descarregar o ZIP.
#[utoipa::path(
    post, path = "/api/users/me/data-exports/{export_id}/download-link", tag = "account",
    security(("session" = [])),
    params(("export_id" = Uuid, Path)),
    responses(
        (status = 200, body = DownloadLink),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 404, description = "`data_export.not_found`.", body = crate::openapi::ErrorBody),
        (status = 409, description = "Ainda não está pronta, falhou ou expirou (`data_export.not_ready`).", body = crate::openapi::ErrorBody),
    )
)]
pub async fn download_link(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(export_id): Path<Uuid>,
) -> Result<Json<DownloadLink>, ApiError> {
    let export = fetch_own(&state, auth.user_id, export_id).await?;
    let ready = export.status == "ready" && export.expires_at.is_some_and(|t| t > Utc::now());
    if !ready {
        return Err(DomainError::conflict(
            "data_export.not_ready",
            format!("a exportação está «{}»", export.status),
        )
        .into());
    }
    let exp = (Utc::now().timestamp() + rules::LINK_TTL_SECS)
        .min(export.expires_at.map(|t| t.timestamp()).unwrap_or(i64::MAX));
    crate::audit::log(
        &state.db,
        None,
        auth.user_id,
        "data_export.link_issued",
        &export_id.to_string(),
    )
    .await;
    Ok(Json(DownloadLink {
        url: rules::signed_path(&signing_key(&state), export_id, exp),
        expires_at: DateTime::from_timestamp(exp, 0).unwrap_or_else(Utc::now),
    }))
}

/// O ZIP. Sem sessão: vale a assinatura. Assinatura errada, prazo expirado,
/// ficheiro apagado ou inexistente dão todos `404`.
#[utoipa::path(
    get, path = "/api/users/me/data-exports/{export_id}/content", tag = "account",
    security(()),
    params(("export_id" = Uuid, Path), ContentQuery),
    responses(
        (status = 200, description = "`application/zip`."),
        (status = 404, description = "`data_export.not_found`.", body = crate::openapi::ErrorBody),
    )
)]
pub async fn content(
    State(state): State<Arc<AppState>>,
    Path(export_id): Path<String>,
    Query(q): Query<ContentQuery>,
) -> Result<Response, ApiError> {
    let id = Uuid::parse_str(&export_id).map_err(|_| not_found())?;
    let valid = match (q.exp.as_deref(), q.sig.as_deref()) {
        (Some(exp), Some(sig)) => {
            rules::verify(&signing_key(&state), id, exp, sig, Utc::now().timestamp())
        }
        _ => false,
    };
    if !valid {
        return Err(not_found());
    }
    let ready: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM data_exports WHERE id = $1 AND status = 'ready' AND expires_at > now())",
    )
    .bind(id)
    .fetch_one(&state.db)
    .await?;
    if !ready {
        return Err(not_found());
    }
    // Em memória: o ZIP não leva os bytes das gravações (vão como links), por
    // isso fica em kilobytes ou poucos megabytes.
    let bytes = tokio::fs::read(file_path(&state, id))
        .await
        .map_err(|_| not_found())?;
    Ok((
        [
            (header::CONTENT_TYPE, "application/zip".to_string()),
            (
                header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"delonix-meet-dados-{id}.zip\""),
            ),
            (header::CACHE_CONTROL, "private, no-store".to_string()),
            (header::REFERRER_POLICY, "no-referrer".to_string()),
        ],
        Body::from(bytes),
    )
        .into_response())
}

// ---------------------------------------------------------------------------
//  Trabalho
// ---------------------------------------------------------------------------

/// Processa a fila até não haver mais pedidos `queued`. Público para os testes.
pub async fn run_queue(state: &AppState) {
    // Pedidos abandonados por um processo que morreu voltam à fila.
    let _ = sqlx::query(
        "UPDATE data_exports SET status = 'queued', started_at = NULL
          WHERE status = 'running' AND started_at < now() - make_interval(secs => $1)",
    )
    .bind(rules::LEASE_SECS as f64)
    .execute(&state.db)
    .await;
    loop {
        let claimed: Result<Option<(Uuid, Uuid)>, sqlx::Error> = sqlx::query_as(
            "UPDATE data_exports SET status = 'running', started_at = now()
              WHERE id = (SELECT id FROM data_exports WHERE status = 'queued'
                           ORDER BY created_at FOR UPDATE SKIP LOCKED LIMIT 1)
              RETURNING id, user_id",
        )
        .fetch_optional(&state.db)
        .await;
        let (id, user_id) = match claimed {
            Ok(Some(x)) => x,
            Ok(None) => return,
            Err(e) => {
                tracing::warn!(error = %e, "exportação: não foi possível reivindicar");
                return;
            }
        };
        match build(state, id, user_id).await {
            Ok((size, summary)) => {
                let _ = sqlx::query(
                    "UPDATE data_exports SET status = 'ready', size_bytes = $2, summary = $3,
                            completed_at = now(), expires_at = now() + make_interval(hours => $4)
                      WHERE id = $1 AND status = 'running'",
                )
                .bind(id)
                .bind(size)
                .bind(summary)
                .bind(rules::FILE_TTL_HOURS as i32)
                .execute(&state.db)
                .await;
                crate::audit::log(
                    &state.db,
                    None,
                    user_id,
                    "data_export.ready",
                    &id.to_string(),
                )
                .await;
            }
            Err(e) => {
                tracing::error!(%id, error = %e, "exportação falhou");
                let _ =
                    tokio::fs::remove_file(file_path(state, id).with_extension("zip.part")).await;
                let _ = sqlx::query(
                    "UPDATE data_exports SET status = 'failed', completed_at = now(),
                            error = 'não foi possível gerar a exportação; peça outra'
                      WHERE id = $1",
                )
                .bind(id)
                .execute(&state.db)
                .await;
            }
        }
    }
}

/// Apaga os ficheiros vencidos e marca os pedidos `expired`. Devolve quantos.
pub async fn sweep_expired(state: &AppState) -> Result<u64, sqlx::Error> {
    let ids: Vec<Uuid> = sqlx::query_scalar(
        "UPDATE data_exports SET status = 'expired'
          WHERE status = 'ready' AND expires_at <= now() RETURNING id",
    )
    .fetch_all(&state.db)
    .await?;
    for id in &ids {
        let _ = tokio::fs::remove_file(file_path(state, *id)).await;
    }
    Ok(ids.len() as u64)
}

/// Acções cujo `target` é a própria conta (ver o topo do módulo).
const SELF_TARGET_PREFIXES: &[&str] = &[
    "auth.",
    "session.",
    "profile.",
    "security.",
    "personal_room.",
    "data_export.",
];

async fn build(
    state: &AppState,
    id: Uuid,
    user_id: Uuid,
) -> Result<(i64, serde_json::Value), anyhow::Error> {
    use serde_json::json;
    let profile = crate::account::build_profile(state, user_id)
        .await
        .map_err(|e| anyhow::anyhow!("perfil: {e}"))?;
    let preferences = json!({
        "join": crate::account::JoinPreferencesBody::from(
            crate::account::load_join_preferences(&state.db, user_id).await?),
        "notifications": crate::account::load_notification_preferences(state, user_id)
            .await.map_err(|e| anyhow::anyhow!("notificações: {e}"))?,
        "tour": crate::account::load_tour(&state.db, user_id)
            .await.map_err(|e| anyhow::anyhow!("guia: {e}"))?,
    });
    let recordings: Vec<(Uuid, String, i64, String, DateTime<Utc>, String)> = sqlx::query_as(
        "SELECT id, filename, size_bytes, status, created_at, transcript
           FROM recordings WHERE uploader_id = $1 ORDER BY created_at, id",
    )
    .bind(user_id)
    .fetch_all(&state.db)
    .await?;
    let activity: Vec<(String, String, DateTime<Utc>)> = sqlx::query_as(
        "SELECT action, target, created_at FROM audit_logs
          WHERE actor_id = $1 ORDER BY created_at DESC, id DESC LIMIT $2",
    )
    .bind(user_id)
    .bind(rules::MAX_ACTIVITY_ROWS)
    .fetch_all(&state.db)
    .await?;
    let usage = crate::usage::user_storage_usage(state, user_id)
        .await
        .map_err(|e| anyhow::anyhow!("uso: {e}"))?;

    let recordings_json: Vec<serde_json::Value> = recordings
        .iter()
        .map(|(rid, filename, size, status, created_at, transcript)| {
            json!({
                "id": rid, "filename": filename, "size_bytes": size, "status": status,
                "created_at": created_at,
                "link": format!("/api/recordings/{rid}/content"),
                "transcript_file": (!transcript.trim().is_empty()).then(|| format!("transcripts/{rid}.txt")),
            })
        })
        .collect();
    let activity_json: Vec<serde_json::Value> = activity
        .iter()
        .map(|(action, target, at)| {
            if SELF_TARGET_PREFIXES.iter().any(|p| action.starts_with(p)) {
                json!({"action": action, "target": target, "created_at": at})
            } else {
                json!({"action": action, "target_redacted": true, "created_at": at})
            }
        })
        .collect();
    let transcripts: Vec<(Uuid, String)> = recordings
        .iter()
        .filter(|r| !r.5.trim().is_empty())
        .map(|r| (r.0, r.5.clone()))
        .collect();
    let summary = json!({
        "recordings": recordings.len(),
        "transcripts": transcripts.len(),
        "activity_events": activity.len(),
    });
    let manifest = json!({
        "format": "delonix-meet.personal-export.v1",
        "user_id": user_id,
        "generated_at": Utc::now(),
        "contents": ["profile.json", "preferences.json", "recordings.json", "transcripts/", "activity.json", "storage_usage.json"],
        "notes": "As gravações vão como links (descarga com sessão). As transcrições podem conter a fala de outros participantes da reunião.",
        "summary": summary,
    });

    let mut files: Vec<(String, Vec<u8>)> = vec![
        (
            "manifest.json".into(),
            serde_json::to_vec_pretty(&manifest)?,
        ),
        ("profile.json".into(), serde_json::to_vec_pretty(&profile)?),
        (
            "preferences.json".into(),
            serde_json::to_vec_pretty(&preferences)?,
        ),
        (
            "recordings.json".into(),
            serde_json::to_vec_pretty(&recordings_json)?,
        ),
        (
            "activity.json".into(),
            serde_json::to_vec_pretty(&activity_json)?,
        ),
        (
            "storage_usage.json".into(),
            serde_json::to_vec_pretty(&usage)?,
        ),
    ];
    for (rid, text) in transcripts {
        files.push((format!("transcripts/{rid}.txt"), text.into_bytes()));
    }

    let dir = state.config.data_exports_dir.clone();
    let final_path = file_path(state, id);
    let size = tokio::task::spawn_blocking(move || -> Result<i64, anyhow::Error> {
        use std::io::Write;
        std::fs::create_dir_all(&dir)?;
        let part = final_path.with_extension("zip.part");
        let f = std::fs::File::create(&part)?;
        let mut zip = zip::ZipWriter::new(f);
        let opts = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored)
            .unix_permissions(0o600);
        for (name, bytes) in files {
            zip.start_file(name, opts)?;
            zip.write_all(&bytes)?;
        }
        zip.finish()?;
        std::fs::rename(&part, &final_path)?;
        Ok(std::fs::metadata(&final_path)?.len() as i64)
    })
    .await??;
    Ok((size, summary))
}
