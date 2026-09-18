//! Destinos de directo guardados por ORGANIZAÇÃO (frontend/b1-emissao).
//!
//! Antes, o Estúdio guardava os destinos só em memória: a chave de emissão do
//! canal de YouTube era reescrita a cada directo e viajava na query do
//! WebSocket. Agora a organização guarda-os uma vez e o Estúdio refere-os por
//! `id` (`{"id": "<uuid>"}` no array `destinos` do `/api/rooms/{code}/broadcast`).
//!
//! **Regras que este módulo garante:**
//!
//! - A chave fica cifrada em repouso (`crypto::SecretsKey`, AAD
//!   `stream_destination/<org>/<id>`) e **nunca volta ao cliente**: nenhuma
//!   resposta a inclui, nem cifrada — só `key_set: true`.
//! - Ler é de qualquer membro activo da organização (quem emite precisa de
//!   escolher o destino); criar, alterar e apagar é do administrador (a chave
//!   é da organização, como as chaves de API e os webhooks).
//! - Um destino de outra organização responde `404`, não `403`: não se
//!   confirma que existe.
//! - Tecto de `MAX_PER_ORG` destinos por organização — é o que torna a
//!   listagem limitada por construção, sem paginação e sem corte silencioso.

use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::auth::AuthUser;
use crate::broadcast::{platform_from_url, rtmp_url_is_valid, stream_key_is_valid, Destino};
use crate::crypto::SecretsKey;
use crate::error::ApiError;
use crate::signaling::Secret;
use crate::AppState;

/// Destinos por organização. O template mostra 4–5; 50 é folga, não alvo.
pub const MAX_PER_ORG: i64 = 50;
const PLATFORMS: &[&str] = &["youtube", "facebook", "linkedin", "twitch", "rtmp"];
const LABEL_MAX: usize = 80;
const URL_MAX: usize = 512;
const KEY_MAX: usize = 512;

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct StreamDestination {
    pub id: Uuid,
    pub org_id: Uuid,
    pub label: String,
    pub platform: String,
    pub rtmp_url: String,
    /// Sempre `true`: a chave existe e está guardada. O valor nunca sai.
    pub key_set: bool,
    pub created_by: Option<Uuid>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
    pub last_used_at: Option<chrono::DateTime<chrono::Utc>>,
    /// `ok` | `erro` — como acabou a última emissão que o usou.
    pub last_status: Option<String>,
    pub last_error: Option<String>,
}

/// As colunas que se devolvem. `key_enc` NÃO está aqui, e é de propósito.
const COLUMNS: &str = "id, org_id, label, platform, rtmp_url, TRUE AS key_set, created_by, \
                       created_at, updated_at, last_used_at, last_status, last_error";

#[derive(Deserialize)]
pub struct CreateReq {
    pub label: String,
    #[serde(default)]
    pub platform: Option<String>,
    pub rtmp_url: String,
    pub stream_key: String,
}

#[derive(Deserialize)]
pub struct UpdateReq {
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub platform: Option<String>,
    #[serde(default)]
    pub rtmp_url: Option<String>,
    /// Presente = substitui a chave. Ausente = mantém a que está guardada.
    #[serde(default)]
    pub stream_key: Option<String>,
}

/// O que torna o blob de uma linha inútil noutra (ver `crypto.rs`).
fn aad(org_id: Uuid, id: Uuid) -> Vec<u8> {
    format!("stream_destination/{org_id}/{id}").into_bytes()
}

fn secrets_key(state: &AppState) -> Result<&SecretsKey, ApiError> {
    state.config.secrets_key.as_ref().ok_or_else(|| {
        ApiError::ServiceUnavailable(
            "este servidor não tem a cifra de segredos configurada (SECRETS_KEY), por isso não \
             guarda destinos de directo — comunica-o a quem o administra"
                .into(),
        )
    })
}

fn valid_label(label: &str) -> Result<String, ApiError> {
    let l = label.trim();
    if l.is_empty() || l.chars().count() > LABEL_MAX {
        return Err(ApiError::BadRequest(format!(
            "o nome do destino tem de ter entre 1 e {LABEL_MAX} caracteres"
        )));
    }
    Ok(l.to_string())
}

fn valid_url(url: &str) -> Result<String, ApiError> {
    let u = url.trim();
    if u.len() > URL_MAX || !rtmp_url_is_valid(u) {
        return Err(ApiError::BadRequest(
            "o URL do destino tem de começar por rtmp:// ou rtmps://, ter servidor, e não levar \
             credenciais nem espaços"
                .into(),
        ));
    }
    Ok(u.trim_end_matches('/').to_string())
}

fn valid_key(key: &str) -> Result<String, ApiError> {
    let k = key.trim();
    if k.len() > KEY_MAX || !stream_key_is_valid(k) {
        return Err(ApiError::BadRequest(
            "a chave de emissão não pode estar vazia nem ter espaços".into(),
        ));
    }
    Ok(k.to_string())
}

fn valid_platform(platform: Option<&str>, url: &str) -> Result<String, ApiError> {
    match platform.map(str::trim).filter(|p| !p.is_empty()) {
        None => Ok(platform_from_url(url).to_string()),
        Some(p) if PLATFORMS.contains(&p) => Ok(p.to_string()),
        Some(p) => Err(ApiError::BadRequest(format!(
            "plataforma «{p}» desconhecida — válidas: {}",
            PLATFORMS.join(", ")
        ))),
    }
}

const LABEL_TAKEN: &str = "já existe um destino com esse nome nesta organização";

/// `GET /api/orgs/{org_id}/stream-destinations` — membro da organização.
pub async fn list(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(org_id): Path<Uuid>,
) -> Result<Json<Vec<StreamDestination>>, ApiError> {
    crate::org::require_member_pub(&state, org_id, auth.user_id).await?;
    let rows = sqlx::query_as::<_, StreamDestination>(&format!(
        "SELECT {COLUMNS} FROM stream_destinations WHERE org_id = $1 ORDER BY label LIMIT $2"
    ))
    .bind(org_id)
    .bind(MAX_PER_ORG)
    .fetch_all(&state.db)
    .await?;
    Ok(Json(rows))
}

/// `GET /api/orgs/{org_id}/stream-destinations/{destination_id}` — membro.
pub async fn get_one(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path((org_id, id)): Path<(Uuid, Uuid)>,
) -> Result<Json<StreamDestination>, ApiError> {
    crate::org::require_member_pub(&state, org_id, auth.user_id).await?;
    fetch(&state, org_id, id).await.map(Json)
}

async fn fetch(state: &AppState, org_id: Uuid, id: Uuid) -> Result<StreamDestination, ApiError> {
    sqlx::query_as::<_, StreamDestination>(&format!(
        "SELECT {COLUMNS} FROM stream_destinations WHERE org_id = $1 AND id = $2"
    ))
    .bind(org_id)
    .bind(id)
    .fetch_optional(&state.db)
    .await?
    .ok_or(ApiError::NotFound)
}

/// `POST /api/orgs/{org_id}/stream-destinations` — administrador.
/// `201 Created` + `Location` + o recurso (sem a chave).
pub async fn create(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(org_id): Path<Uuid>,
    Json(req): Json<CreateReq>,
) -> Result<Response, ApiError> {
    crate::rbac::require_permission(&state, org_id, auth.user_id, "streaming.manage").await?;
    let key = secrets_key(&state)?;
    let label = valid_label(&req.label)?;
    let url = valid_url(&req.rtmp_url)?;
    let platform = valid_platform(req.platform.as_deref(), &url)?;
    let stream_key = Secret::new(valid_key(&req.stream_key)?);

    let (count,): (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM stream_destinations WHERE org_id = $1")
            .bind(org_id)
            .fetch_one(&state.db)
            .await?;
    if count >= MAX_PER_ORG {
        return Err(ApiError::Conflict(format!(
            "esta organização já tem {MAX_PER_ORG} destinos guardados; apaga um antes de criar outro"
        )));
    }

    let id = Uuid::new_v4();
    let key_enc = key.seal(stream_key.expose().as_bytes(), &aad(org_id, id));
    let row = sqlx::query_as::<_, StreamDestination>(&format!(
        "INSERT INTO stream_destinations (id, org_id, label, platform, rtmp_url, key_enc, created_by)
         VALUES ($1, $2, $3, $4, $5, $6, $7)
         RETURNING {COLUMNS}"
    ))
    .bind(id)
    .bind(org_id)
    .bind(&label)
    .bind(&platform)
    .bind(&url)
    .bind(&key_enc)
    .bind(auth.user_id)
    .fetch_one(&state.db)
    .await
    .map_err(|e| ApiError::from_unique(e, LABEL_TAKEN))?;
    crate::audit::log(
        &state.db,
        Some(org_id),
        auth.user_id,
        "stream_destination.created",
        &row.label,
    )
    .await;
    let location = format!("/api/orgs/{org_id}/stream-destinations/{id}");
    Ok((
        StatusCode::CREATED,
        [(header::LOCATION, location)],
        Json(row),
    )
        .into_response())
}

/// `PATCH /api/orgs/{org_id}/stream-destinations/{destination_id}` — administrador.
pub async fn update(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path((org_id, id)): Path<(Uuid, Uuid)>,
    Json(req): Json<UpdateReq>,
) -> Result<Json<StreamDestination>, ApiError> {
    crate::rbac::require_permission(&state, org_id, auth.user_id, "streaming.manage").await?;
    let current = fetch(&state, org_id, id).await?;
    let label = match req.label.as_deref() {
        Some(l) => valid_label(l)?,
        None => current.label,
    };
    let url = match req.rtmp_url.as_deref() {
        Some(u) => valid_url(u)?,
        None => current.rtmp_url,
    };
    let platform = match req.platform.as_deref() {
        Some(p) => valid_platform(Some(p), &url)?,
        None => current.platform,
    };
    let key_enc = match req.stream_key.as_deref() {
        Some(k) => {
            let k = Secret::new(valid_key(k)?);
            Some(secrets_key(&state)?.seal(k.expose().as_bytes(), &aad(org_id, id)))
        }
        None => None,
    };
    let row = sqlx::query_as::<_, StreamDestination>(&format!(
        "UPDATE stream_destinations
            SET label = $3, platform = $4, rtmp_url = $5,
                key_enc = COALESCE($6, key_enc), updated_at = now()
          WHERE org_id = $1 AND id = $2
         RETURNING {COLUMNS}"
    ))
    .bind(org_id)
    .bind(id)
    .bind(&label)
    .bind(&platform)
    .bind(&url)
    .bind(key_enc)
    .fetch_optional(&state.db)
    .await
    .map_err(|e| ApiError::from_unique(e, LABEL_TAKEN))?
    .ok_or(ApiError::NotFound)?;
    let action = if req.stream_key.is_some() {
        "stream_destination.key_rotated"
    } else {
        "stream_destination.updated"
    };
    crate::audit::log(&state.db, Some(org_id), auth.user_id, action, &row.label).await;
    Ok(Json(row))
}

/// `DELETE /api/orgs/{org_id}/stream-destinations/{destination_id}` —
/// administrador. `204`; `404` se não existir nesta organização.
pub async fn delete(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path((org_id, id)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, ApiError> {
    crate::rbac::require_permission(&state, org_id, auth.user_id, "streaming.manage").await?;
    let removed: Option<(String,)> = sqlx::query_as(
        "DELETE FROM stream_destinations WHERE org_id = $1 AND id = $2 RETURNING label",
    )
    .bind(org_id)
    .bind(id)
    .fetch_optional(&state.db)
    .await?;
    let (label,) = removed.ok_or(ApiError::NotFound)?;
    crate::audit::log(
        &state.db,
        Some(org_id),
        auth.user_id,
        "stream_destination.deleted",
        &label,
    )
    .await;
    Ok(StatusCode::NO_CONTENT)
}

// ---------------------------------------------------------------------------
//  Uso pelo directo (broadcast.rs)
// ---------------------------------------------------------------------------

const NOT_YOURS: &str = "o destino guardado não existe ou não pertence à tua organização";

/// Resolve `{"id": …}` num `Destino` com a chave decifrada — só para um
/// membro ACTIVO da organização dona do destino. O erro é uma frase para a
/// recusa do WebSocket; «não existe» e «é de outra organização» dizem o mesmo.
pub(crate) async fn resolve_for_broadcast(
    state: &AppState,
    user_id: Uuid,
    id: Uuid,
) -> Result<Destino, String> {
    let key = secrets_key(state).map_err(|e| e.to_string())?;
    type Row = (Uuid, String, String, String, Vec<u8>);
    let row: Option<Row> = sqlx::query_as(
        "SELECT org_id, label, platform, rtmp_url, key_enc FROM stream_destinations WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(&state.db)
    .await
    .map_err(|e| {
        tracing::error!(erro = %e, "destino guardado ilegível");
        "não foi possível ler o destino guardado".to_string()
    })?;
    let (org_id, label, platform, url, key_enc) = row.ok_or(NOT_YOURS)?;
    match crate::org::role_in_org(state, org_id, user_id).await {
        Ok(Some(_)) => {}
        _ => return Err(NOT_YOURS.into()),
    }
    let plain = key.open(&key_enc, &aad(org_id, id)).map_err(|e| {
        tracing::error!(destino = %id, erro = %e, "a chave de um destino guardado não decifra");
        format!("a chave do destino «{label}» não decifra neste servidor: guarda-a outra vez")
    })?;
    let chave = String::from_utf8(plain)
        .map_err(|_| format!("a chave do destino «{label}» está corrompida: guarda-a outra vez"))?;
    Ok(Destino {
        url,
        chave: Secret::new(chave),
        rotulo: label,
        id: Some(id),
        platform,
    })
}

/// Marca os destinos guardados usados numa emissão. Falhar aqui não impede o
/// directo — é só o «usado há…» que fica por actualizar.
pub(crate) async fn mark_used(state: &AppState, destinos: &[Destino]) {
    let ids: Vec<Uuid> = destinos.iter().filter_map(|d| d.id).collect();
    if ids.is_empty() {
        return;
    }
    if let Err(e) =
        sqlx::query("UPDATE stream_destinations SET last_used_at = now() WHERE id = ANY($1)")
            .bind(&ids)
            .execute(&state.db)
            .await
    {
        tracing::warn!(erro = %e, "não actualizei o last_used_at dos destinos");
    }
}

/// Guarda como acabou cada destino guardado: `ok` se chegou a enviar e não
/// desistiu; `erro` com o motivo em qualquer outro caso.
pub(crate) async fn record_outcome(
    state: &AppState,
    reports: &[crate::broadcast::DestinationReport],
) {
    for r in reports {
        let Some(id) = r.id else { continue };
        let (status, error) = outcome(r);
        if let Err(e) = sqlx::query(
            "UPDATE stream_destinations SET last_status = $2, last_error = $3 WHERE id = $1",
        )
        .bind(id)
        .bind(status)
        .bind(error)
        .execute(&state.db)
        .await
        {
            tracing::warn!(erro = %e, "não guardei o resultado do destino");
        }
    }
}

fn outcome(r: &crate::broadcast::DestinationReport) -> (&'static str, Option<String>) {
    match (&r.motivo, r.bytes_enviados) {
        (None, n) if n > 0 => ("ok", None),
        (Some(m), _) => ("erro", Some(m.clone())),
        (None, _) => ("erro", Some("não chegou a ficar no ar".into())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::broadcast::{DestinationReport, DestinationState};

    #[test]
    fn the_key_column_is_never_selected() {
        // A regra «a chave nunca volta ao cliente» é, aqui, uma regra sobre o
        // SQL: nenhuma resposta pode ler `key_enc`.
        assert!(!COLUMNS.contains("key_enc"));
    }

    #[test]
    fn urls_must_be_rtmp_and_keys_must_not_be_blank() {
        assert!(valid_url("rtmp://a.rtmp.youtube.com/live2/").is_ok());
        assert_eq!(
            valid_url("rtmps://live-api-s.facebook.com:443/rtmp/").unwrap(),
            "rtmps://live-api-s.facebook.com:443/rtmp"
        );
        for mau in [
            "file:///etc/passwd",
            "/tmp/saida.flv",
            "http://exemplo/live",
            "rtmp://utilizador:pw@exemplo/live",
            "rtmp:// exemplo/live",
            "",
        ] {
            assert!(valid_url(mau).is_err(), "{mau}");
        }
        assert!(valid_key("  ").is_err());
        assert!(valid_key("abc def").is_err());
        assert!(valid_key("abcd-1234").is_ok());
    }

    #[test]
    fn platform_is_derived_when_absent_and_checked_when_present() {
        assert_eq!(
            valid_platform(None, "rtmp://a.rtmp.youtube.com/live2").unwrap(),
            "youtube"
        );
        assert_eq!(
            valid_platform(Some("twitch"), "rtmp://x/y").unwrap(),
            "twitch"
        );
        assert!(valid_platform(Some("tiktok"), "rtmp://x/y").is_err());
    }

    #[test]
    fn labels_are_trimmed_and_bounded() {
        assert_eq!(valid_label("  Canal  ").unwrap(), "Canal");
        assert!(valid_label("   ").is_err());
        assert!(valid_label(&"x".repeat(81)).is_err());
    }

    #[test]
    fn the_aad_binds_org_and_row() {
        let (o1, o2, id) = (Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4());
        assert_ne!(aad(o1, id), aad(o2, id));
    }

    fn report(motivo: Option<&str>, bytes: u64) -> DestinationReport {
        DestinationReport {
            dest: 0,
            id: Some(Uuid::new_v4()),
            rotulo: "yt".into(),
            platform: "youtube".into(),
            estado: DestinationState::Stopped,
            kbps: 0,
            perdas: 0.0,
            motivo: motivo.map(String::from),
            tentativas: 0,
            frames_descartados: 0,
            bytes_enviados: bytes,
        }
    }

    #[test]
    fn outcome_says_ok_only_for_a_destination_that_actually_sent() {
        assert_eq!(outcome(&report(None, 10)), ("ok", None));
        assert_eq!(outcome(&report(None, 0)).0, "erro");
        assert_eq!(outcome(&report(Some("desistiu"), 10)).0, "erro");
    }
}
