//! "A minha conta": sessões activas (dispositivos ligados) e exportação dos
//! próprios dados. O perfil (username/password/locale) já vive em
//! [`crate::users::update_me`] e a MFA em [`crate::mfa`] — este módulo só
//! acrescenta o que faltava para a página "A minha conta" do mockup: ver e
//! revogar sessões, e descarregar os dados pessoais.

use axum::{
    extract::{Path, State},
    http::{header, HeaderMap},
    response::{IntoResponse, Response},
    Json,
};
use chrono::{DateTime, Utc};
use serde::Serialize;
use std::sync::Arc;
use uuid::Uuid;

use crate::{auth::AuthUser, error::ApiError, AppState};

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct SessionInfo {
    pub session_id: Uuid,
    pub user_agent: Option<String>,
    pub ip_address: Option<String>,
    pub started_at: DateTime<Utc>,
    pub last_used_at: DateTime<Utc>,
    /// A sessão do pedido que fez este pedido — a UI não deixa revogar sem
    /// aviso o dispositivo que está a olhar para o ecrã.
    pub current: bool,
}

fn current_session_hash(headers: &HeaderMap) -> Option<String> {
    let raw = headers.get(header::COOKIE)?.to_str().ok()?;
    let token = raw.split(';').find_map(|p| {
        p.trim()
            .strip_prefix("dlx_refresh=")
            .filter(|v| !v.is_empty())
    })?;
    Some(crate::auth::hash_refresh_token(token))
}

#[derive(sqlx::FromRow)]
struct SessionRow {
    session_id: Uuid,
    user_agent: Option<String>,
    ip_address: Option<String>,
    session_started_at: DateTime<Utc>,
    created_at: DateTime<Utc>,
    token_hash: String,
}

/// Lista as sessões activas (uma por `session_id` não revogado e não
/// expirado) — cada refresh roda o `token_hash`, mas mantém o `session_id`,
/// por isso há exactamente uma linha viva por dispositivo ligado.
pub async fn list_sessions(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    headers: HeaderMap,
) -> Result<Json<Vec<SessionInfo>>, ApiError> {
    let current_hash = current_session_hash(&headers);
    let rows: Vec<SessionRow> = sqlx::query_as(
        "SELECT session_id, user_agent, ip_address, session_started_at, created_at, token_hash
             FROM refresh_tokens
             WHERE user_id = $1 AND NOT revoked AND expires_at > now()
             ORDER BY created_at DESC",
    )
    .bind(auth.user_id)
    .fetch_all(&state.db)
    .await?;

    Ok(Json(
        rows.into_iter()
            .map(|r| SessionInfo {
                session_id: r.session_id,
                user_agent: r.user_agent,
                ip_address: r.ip_address,
                started_at: r.session_started_at,
                last_used_at: r.created_at,
                current: current_hash.as_deref() == Some(r.token_hash.as_str()),
            })
            .collect(),
    ))
}

/// Termina uma sessão à distância (ex.: "não reconheço este telemóvel").
/// Revoga a linha viva do `session_id`: o próximo refresh desse dispositivo
/// falha e ele tem de voltar a autenticar-se.
pub async fn revoke_session(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(session_id): Path<Uuid>,
) -> Result<axum::http::StatusCode, ApiError> {
    let result = sqlx::query(
        "UPDATE refresh_tokens SET revoked = TRUE
         WHERE session_id = $1 AND user_id = $2 AND NOT revoked",
    )
    .bind(session_id)
    .bind(auth.user_id)
    .execute(&state.db)
    .await?;
    if result.rows_affected() == 0 {
        return Err(ApiError::NotFound);
    }
    crate::audit::log(
        &state.db,
        None,
        auth.user_id,
        "account.session_revoked",
        &session_id.to_string(),
    )
    .await;
    Ok(axum::http::StatusCode::NO_CONTENT)
}

#[derive(Serialize, sqlx::FromRow)]
struct ExportOrgMembership {
    org_id: Uuid,
    org_name: String,
    role: String,
    title: String,
}

#[derive(Serialize, sqlx::FromRow)]
struct ExportRoom {
    id: Uuid,
    code: String,
    name: String,
    created_at: DateTime<Utc>,
}

#[derive(Serialize, sqlx::FromRow)]
struct ExportRecording {
    id: Uuid,
    room_id: Uuid,
    filename: String,
    size_bytes: i64,
    created_at: DateTime<Utc>,
}

#[derive(Serialize)]
struct DataExport {
    generated_at: DateTime<Utc>,
    profile: crate::users::UserPublic,
    organizations: Vec<ExportOrgMembership>,
    rooms_owned: Vec<ExportRoom>,
    recordings: Vec<ExportRecording>,
}

/// Exportação dos próprios dados: perfil, organizações, salas criadas e
/// gravações próprias — tudo lido das tabelas reais, para descarregar como
/// ficheiro. Não é um relatório de compliance formal, é o que a conta
/// realmente guarda sobre a pessoa.
pub async fn export_my_data(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
) -> Result<Response, ApiError> {
    let profile = crate::users::fetch_public(&state.db, auth.user_id).await?;

    let organizations: Vec<ExportOrgMembership> =
        crate::org::memberships_for_export(&state, auth.user_id).await?;

    let rooms_owned: Vec<ExportRoom> = sqlx::query_as(
        "SELECT id, code, name, created_at FROM rooms WHERE owner_id = $1 ORDER BY created_at DESC",
    )
    .bind(auth.user_id)
    .fetch_all(&state.db)
    .await?;

    let recordings: Vec<ExportRecording> = sqlx::query_as(
        "SELECT id, room_id, filename, size_bytes, created_at
         FROM recordings WHERE uploader_id = $1 ORDER BY created_at DESC",
    )
    .bind(auth.user_id)
    .fetch_all(&state.db)
    .await?;

    let export = DataExport {
        generated_at: Utc::now(),
        profile,
        organizations,
        rooms_owned,
        recordings,
    };

    crate::audit::log(&state.db, None, auth.user_id, "account.data_exported", "").await;

    let body = serde_json::to_vec_pretty(&export).map_err(|e| ApiError::Internal(e.to_string()))?;
    Ok((
        [
            (header::CONTENT_TYPE, "application/json".to_string()),
            (
                header::CONTENT_DISPOSITION,
                "attachment; filename=\"delonix-meet-dados.json\"".to_string(),
            ),
        ],
        body,
    )
        .into_response())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn current_session_hash_reads_the_refresh_cookie() {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::COOKIE,
            "outro=1; dlx_refresh=abc123; mais=2".parse().unwrap(),
        );
        assert_eq!(
            current_session_hash(&headers),
            Some(crate::auth::hash_refresh_token("abc123"))
        );
    }

    #[test]
    fn current_session_hash_none_without_cookie() {
        assert_eq!(current_session_hash(&HeaderMap::new()), None);
    }
}
