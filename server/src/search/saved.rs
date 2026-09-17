//! Favoritos — pesquisas guardadas (contrato §5, migração 0117).
//!
//! A `query` valida-se contra o schema do recurso ao gravar (os mesmos códigos
//! da lista) e volta a validar-se ao ler: um schema que mudou devolve
//! `valid: false`, não esconde o favorito. A organização de um favorito
//! partilhado é a pertença activa principal de quem o grava — nunca o corpo.

use std::sync::Arc;

use axum::{
    extract::{Path, Query, State},
    http::{header, StatusCode},
    response::IntoResponse,
    Json,
};
use chrono::{DateTime, Utc};
use delonix_meet_core::{
    page::{Page, PageRequest},
    query::{validate_saved, Ctx},
    DomainError,
};
use serde::{Deserialize, Serialize};
use serde_json::Value as Json_;
use uuid::Uuid;

use crate::{auth::AuthUser, error::ApiError, org, AppState};

pub const MAX_PER_USER: i64 = 100;
const MAX_NAME: usize = 80;

/// A consulta guardada, na forma dos parâmetros da lista.
#[derive(Debug, Clone, Default, Serialize, Deserialize, utoipa::ToSchema)]
pub struct SavedQuery {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub q: Option<String>,
    /// O domínio (JSON), como no parâmetro `filter`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = Option<Object>)]
    pub filter: Option<Json_>,
    #[serde(default)]
    pub filters: Vec<String>,
    #[serde(default)]
    pub group_by: Vec<String>,
    #[serde(default)]
    pub order_by: Vec<String>,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct CreateSavedSearch {
    pub resource: String,
    pub name: String,
    #[serde(default)]
    pub query: SavedQuery,
    #[serde(default)]
    pub shared: bool,
    #[serde(default)]
    pub is_default: bool,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct UpdateSavedSearch {
    pub name: Option<String>,
    pub query: Option<SavedQuery>,
    pub shared: Option<bool>,
    pub is_default: Option<bool>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct Owner {
    pub id: Uuid,
    pub username: String,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct SavedSearch {
    pub id: Uuid,
    pub resource: String,
    pub name: String,
    pub query: SavedQuery,
    pub shared: bool,
    pub is_default: bool,
    pub owner: Owner,
    pub editable: bool,
    pub valid: bool,
    pub invalid_code: Option<&'static str>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct SavedSearchPage {
    pub items: Vec<SavedSearch>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_page_token: Option<String>,
}

#[derive(Debug, Deserialize, utoipa::IntoParams)]
#[into_params(parameter_in = Query)]
pub struct ListSavedQuery {
    /// Só os deste recurso.
    pub resource: Option<String>,
    pub page_size: Option<u32>,
    pub page_token: Option<String>,
}

#[derive(sqlx::FromRow)]
struct Row {
    id: Uuid,
    user_id: Uuid,
    username: String,
    resource: String,
    name: String,
    query: sqlx::types::Json<SavedQuery>,
    shared: bool,
    is_default: bool,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

const COLUMNS: &str = "s.id, s.user_id, u.username, s.resource, s.name, s.query, s.shared, \
                       s.is_default, s.created_at, s.updated_at";

fn validate_query(resource: &str, q: &SavedQuery, me: Uuid) -> Result<(), DomainError> {
    let schema = delonix_meet_domain::search::schema(resource).ok_or_else(|| {
        DomainError::invalid("search.unknown_resource", "recurso sem pesquisa")
            .with_field("resource", "recurso desconhecido")
    })?;
    validate_saved(
        schema,
        q.q.as_deref(),
        q.filter.as_ref(),
        &q.filters,
        &q.group_by,
        &q.order_by,
        Ctx { me },
    )
}

impl Row {
    fn into_item(self, me: Uuid) -> SavedSearch {
        let check = validate_query(&self.resource, &self.query.0, me);
        SavedSearch {
            id: self.id,
            resource: self.resource,
            name: self.name,
            query: self.query.0,
            shared: self.shared,
            is_default: self.is_default,
            editable: self.user_id == me,
            owner: Owner {
                id: self.user_id,
                username: self.username,
            },
            valid: check.is_ok(),
            invalid_code: check.err().map(|e| e.code),
            created_at: self.created_at,
            updated_at: self.updated_at,
        }
    }
}

fn check_name(name: &str) -> Result<String, DomainError> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > MAX_NAME {
        return Err(DomainError::invalid(
            "saved_search.invalid_name",
            "nome com 1 a 80 caracteres",
        )
        .with_field("name", "1 a 80 caracteres"));
    }
    Ok(name.to_string())
}

/// Visível a `me`: é dele, ou é partilhado numa org onde é membro activo.
async fn load_visible(state: &AppState, me: Uuid, id: Uuid) -> Result<Row, ApiError> {
    let sql = format!(
        "SELECT {COLUMNS} FROM saved_searches s JOIN users u ON u.id = s.user_id
          WHERE s.id = $2 AND (s.user_id = $1 OR (s.shared AND s.org_id IN (
                SELECT va.org_id FROM (SELECT $1::uuid AS id) viewer,
                LATERAL {} AS va(org_id))))",
        org::SQL_VIEWER_ACTIVE_ORGS
    );
    sqlx::query_as::<_, Row>(&sql)
        .bind(me)
        .bind(id)
        .fetch_optional(&state.db)
        .await?
        .ok_or_else(|| ApiError::Domain(DomainError::not_found("saved_search.not_found")))
}

fn unique_to_conflict(e: sqlx::Error) -> ApiError {
    match &e {
        sqlx::Error::Database(db) if db.is_unique_violation() => {
            if db.constraint() == Some("saved_searches_name_uidx") {
                ApiError::Domain(DomainError::conflict(
                    "saved_search.duplicate_name",
                    "já tens um favorito com este nome neste recurso",
                ))
            } else {
                ApiError::Domain(DomainError::conflict(
                    "saved_search.conflict",
                    "conflito ao gravar o favorito",
                ))
            }
        }
        _ => ApiError::from(e),
    }
}

/// Os meus favoritos e os partilhados na minha organização.
#[utoipa::path(
    get, path = "/api/users/me/saved-searches", tag = "search",
    security(("session" = [])),
    params(ListSavedQuery),
    responses(
        (status = 200, body = SavedSearchPage),
        (status = 400, body = crate::openapi::ErrorBody),
        (status = 401, body = crate::openapi::ErrorBody),
    )
)]
pub async fn list(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Query(q): Query<ListSavedQuery>,
) -> Result<Json<SavedSearchPage>, ApiError> {
    let me = auth.user_id;
    let page = PageRequest {
        page_size: q.page_size,
        page_token: q.page_token,
    };
    let size = page.size();
    #[derive(Serialize, Deserialize)]
    struct Cursor {
        at: DateTime<Utc>,
        id: Uuid,
    }
    let cursor: Option<Cursor> = page.cursor()?;
    let sql = format!(
        "SELECT {COLUMNS} FROM saved_searches s JOIN users u ON u.id = s.user_id
          WHERE (s.user_id = $1 OR (s.shared AND s.org_id IN (
                SELECT va.org_id FROM (SELECT $1::uuid AS id) viewer,
                LATERAL {} AS va(org_id))))
            AND ($2::text IS NULL OR s.resource = $2)
            AND ($3::timestamptz IS NULL OR (s.created_at, s.id) > ($3, $4))
          ORDER BY s.created_at, s.id LIMIT $5",
        org::SQL_VIEWER_ACTIVE_ORGS
    );
    let rows: Vec<Row> = sqlx::query_as(&sql)
        .bind(me)
        .bind(q.resource.as_deref())
        .bind(cursor.as_ref().map(|c| c.at))
        .bind(cursor.as_ref().map(|c| c.id).unwrap_or_default())
        .bind(size as i64 + 1)
        .fetch_all(&state.db)
        .await?;
    let p = Page::from_overfetch(rows, size, |r| Cursor {
        at: r.created_at,
        id: r.id,
    });
    Ok(Json(SavedSearchPage {
        items: p.items.into_iter().map(|r| r.into_item(me)).collect(),
        next_page_token: p.next_page_token,
    }))
}

/// Guarda uma pesquisa.
#[utoipa::path(
    post, path = "/api/users/me/saved-searches", tag = "search",
    security(("session" = [])),
    request_body = CreateSavedSearch,
    responses(
        (status = 201, body = SavedSearch, headers(("Location" = String))),
        (status = 400, description = "Consulta que não valida contra o schema (códigos `search.*`), nome inválido.", body = crate::openapi::ErrorBody),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 409, description = "`saved_search.duplicate_name`.", body = crate::openapi::ErrorBody),
        (status = 422, description = "`saved_search.no_organization`, `saved_search.limit_reached`.", body = crate::openapi::ErrorBody),
    )
)]
pub async fn create(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Json(req): Json<CreateSavedSearch>,
) -> Result<impl IntoResponse, ApiError> {
    let me = auth.user_id;
    let name = check_name(&req.name)?;
    validate_query(&req.resource, &req.query, me)?;
    let org_id = org::primary_org_of_user(&state, me)
        .await?
        .map(|(id, _)| id);
    if req.shared && org_id.is_none() {
        return Err(DomainError::precondition(
            "saved_search.no_organization",
            "só se partilha dentro de uma organização",
        )
        .into());
    }
    let mut tx = state.db.begin().await?;
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM saved_searches WHERE user_id = $1")
        .bind(me)
        .fetch_one(&mut *tx)
        .await?;
    if n >= MAX_PER_USER {
        return Err(DomainError::precondition(
            "saved_search.limit_reached",
            "no máximo 100 favoritos por pessoa",
        )
        .into());
    }
    if req.is_default {
        sqlx::query(
            "UPDATE saved_searches SET is_default = false, updated_at = now()
              WHERE user_id = $1 AND resource = $2 AND is_default",
        )
        .bind(me)
        .bind(&req.resource)
        .execute(&mut *tx)
        .await?;
    }
    let id: Uuid = sqlx::query_scalar(
        "INSERT INTO saved_searches (user_id, org_id, resource, name, query, shared, is_default)
         VALUES ($1, $2, $3, $4, $5, $6, $7) RETURNING id",
    )
    .bind(me)
    .bind(org_id)
    .bind(&req.resource)
    .bind(&name)
    .bind(sqlx::types::Json(&req.query))
    .bind(req.shared)
    .bind(req.is_default)
    .fetch_one(&mut *tx)
    .await
    .map_err(unique_to_conflict)?;
    tx.commit().await?;
    let item = load_visible(&state, me, id).await?.into_item(me);
    let location = format!("/api/users/me/saved-searches/{id}");
    Ok((
        StatusCode::CREATED,
        [(header::LOCATION, location)],
        Json(item),
    ))
}

/// Um favorito (meu, ou partilhado na minha organização).
#[utoipa::path(
    get, path = "/api/users/me/saved-searches/{saved_search_id}", tag = "search",
    security(("session" = [])),
    params(("saved_search_id" = Uuid, Path)),
    responses(
        (status = 200, body = SavedSearch),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 404, body = crate::openapi::ErrorBody),
    )
)]
pub async fn get_one(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> Result<Json<SavedSearch>, ApiError> {
    Ok(Json(
        load_visible(&state, auth.user_id, id)
            .await?
            .into_item(auth.user_id),
    ))
}

async fn owned(state: &AppState, me: Uuid, id: Uuid) -> Result<Row, ApiError> {
    let row = load_visible(state, me, id).await?;
    if row.user_id != me {
        return Err(DomainError::forbidden("saved_search.not_owner").into());
    }
    Ok(row)
}

/// Altera nome, consulta, partilha ou «por omissão». Só o dono.
#[utoipa::path(
    patch, path = "/api/users/me/saved-searches/{saved_search_id}", tag = "search",
    security(("session" = [])),
    params(("saved_search_id" = Uuid, Path)),
    request_body = UpdateSavedSearch,
    responses(
        (status = 200, body = SavedSearch),
        (status = 400, body = crate::openapi::ErrorBody),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 403, description = "`saved_search.not_owner`: partilhado de outra pessoa.", body = crate::openapi::ErrorBody),
        (status = 404, body = crate::openapi::ErrorBody),
        (status = 409, body = crate::openapi::ErrorBody),
        (status = 422, body = crate::openapi::ErrorBody),
    )
)]
pub async fn update(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
    Json(req): Json<UpdateSavedSearch>,
) -> Result<Json<SavedSearch>, ApiError> {
    let me = auth.user_id;
    let row = owned(&state, me, id).await?;
    let name = match &req.name {
        Some(n) => check_name(n)?,
        None => row.name.clone(),
    };
    let query = match req.query {
        Some(q) => {
            validate_query(&row.resource, &q, me)?;
            q
        }
        None => row.query.0.clone(),
    };
    let shared = req.shared.unwrap_or(row.shared);
    let is_default = req.is_default.unwrap_or(row.is_default);
    // A org volta a derivar-se: quem mudou de organização partilha na nova.
    let org_id = org::primary_org_of_user(&state, me)
        .await?
        .map(|(id, _)| id);
    if shared && org_id.is_none() {
        return Err(DomainError::precondition(
            "saved_search.no_organization",
            "só se partilha dentro de uma organização",
        )
        .into());
    }
    let mut tx = state.db.begin().await?;
    if is_default && !row.is_default {
        sqlx::query(
            "UPDATE saved_searches SET is_default = false, updated_at = now()
              WHERE user_id = $1 AND resource = $2 AND is_default",
        )
        .bind(me)
        .bind(&row.resource)
        .execute(&mut *tx)
        .await?;
    }
    sqlx::query(
        "UPDATE saved_searches SET name = $2, query = $3, shared = $4, is_default = $5,
                org_id = $6, updated_at = now()
          WHERE id = $1 AND user_id = $7",
    )
    .bind(id)
    .bind(&name)
    .bind(sqlx::types::Json(&query))
    .bind(shared)
    .bind(is_default)
    .bind(org_id)
    .bind(me)
    .execute(&mut *tx)
    .await
    .map_err(unique_to_conflict)?;
    tx.commit().await?;
    Ok(Json(load_visible(&state, me, id).await?.into_item(me)))
}

/// Apaga um favorito. Só o dono.
#[utoipa::path(
    delete, path = "/api/users/me/saved-searches/{saved_search_id}", tag = "search",
    security(("session" = [])),
    params(("saved_search_id" = Uuid, Path)),
    responses(
        (status = 204),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 403, body = crate::openapi::ErrorBody),
        (status = 404, body = crate::openapi::ErrorBody),
    )
)]
pub async fn delete(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
    let me = auth.user_id;
    owned(&state, me, id).await?;
    sqlx::query("DELETE FROM saved_searches WHERE id = $1 AND user_id = $2")
        .bind(id)
        .bind(me)
        .execute(&state.db)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}
