//! Pesquisa, filtros e agrupamentos (ADR-0007, `docs/reference/pesquisa.md`).
//!
//! - [`sql`] — a tradução de uma `core::query::ListQuery` para Postgres
//!   (futuro `delonix-meet-store`);
//! - [`resources`] — a visibilidade e as expressões de cada recurso;
//! - [`global`] — `GET /api/search` (Ctrl+K);
//! - [`saved`] — favoritos (`/api/users/me/saved-searches`);
//! - aqui — os parâmetros HTTP, o envelope e as listas por recurso que os
//!   handlers de colecção chamam quando recebem parâmetros de pesquisa.

pub mod global;
pub mod resources;
pub mod saved;
pub mod sql;

use std::sync::Arc;

use axum::{
    extract::{Path, State},
    Json,
};
use delonix_meet_core::{
    query::{compile, Ctx, HighlightSegment, ListParams, ListQuery, RowId, SchemaView},
    DomainError,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{auth::AuthUser, error::ApiError, AppState};
use sql::{GroupOut, ListOutcome, ResourceSql, Scope};

/// Documentação OpenAPI das rotas deste módulo (`openapi.rs` junta-as).
#[derive(utoipa::OpenApi)]
#[openapi(
    paths(
        schemas,
        schema,
        global::search,
        saved::list,
        saved::create,
        saved::get_one,
        saved::update,
        saved::delete
    ),
    components(schemas(
        SchemaList,
        ItemSearch,
        global::GlobalResponse,
        global::GlobalGroup,
        global::GlobalHit,
        global::Skipped,
        saved::SavedQuery,
        saved::CreateSavedSearch,
        saved::UpdateSavedSearch,
        saved::SavedSearch,
        saved::SavedSearchPage,
        saved::Owner
    ))
)]
pub struct ApiDoc;

/// Fuso por omissão (a coluna `organizations.timezone` tem o mesmo).
pub const DEFAULT_TZ: &str = "Africa/Luanda";

/// Os parâmetros uniformes de uma colecção pesquisável (contrato §2.1).
#[derive(Debug, Clone, Default, Deserialize, utoipa::IntoParams)]
#[into_params(parameter_in = Query)]
pub struct SearchParams {
    /// Texto livre: sem acentos, prefixos, erros de escrita.
    pub q: Option<String>,
    /// Domínio em JSON: `[campo, operador, valor]`, `{"and":[…]}`, `{"or":[…]}`, `{"not":…}`.
    pub filter: Option<String>,
    /// Filtros pré-definidos do schema, separados por vírgulas.
    pub filters: Option<String>,
    /// Até 3 campos; datas com `:day|week|month|quarter|year`.
    pub group_by: Option<String>,
    /// Até 3 campos; `-` = descendente; `_score` = relevância (só com `q`).
    pub order_by: Option<String>,
    /// 1–100, omissão 50.
    pub page_size: Option<u32>,
    pub page_token: Option<String>,
    pub groups_page_token: Option<String>,
}

impl From<&SearchParams> for ListParams {
    fn from(p: &SearchParams) -> Self {
        ListParams {
            q: p.q.clone(),
            filter: p.filter.clone(),
            filters: p.filters.clone(),
            group_by: p.group_by.clone(),
            order_by: p.order_by.clone(),
            page_size: p.page_size,
            page_token: p.page_token.clone(),
            groups_page_token: p.groups_page_token.clone(),
        }
    }
}

impl SearchParams {
    pub fn is_search(&self) -> bool {
        ListParams::from(self).is_search()
    }
}

/// O que acompanha um item quando há `q`.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct ItemSearch {
    pub score: f64,
    #[schema(value_type = Vec<Object>)]
    pub highlight: Vec<HighlightSegment>,
}

/// Um item da colecção, na forma de sempre, mais `search` quando há `q`.
#[derive(Debug, Serialize)]
pub struct SearchItem<T> {
    #[serde(flatten)]
    pub item: T,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub search: Option<ItemSearch>,
}

/// Envelope de uma lista pesquisada: `Page<T>` (`items` + `next_page_token`)
/// com `total`, `total_kind` e os grupos (contrato §2.3).
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct SearchPage<T> {
    pub items: Vec<T>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_page_token: Option<String>,
    pub total: i64,
    /// `exact` ou `at_least` (acima de 10 000).
    #[schema(value_type = String)]
    pub total_kind: delonix_meet_core::query::TotalKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schema(value_type = Option<Vec<Object>>)]
    pub groups: Option<Vec<GroupOut>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_groups_page_token: Option<String>,
    /// Só com `q`: `exact` (prefixos e subcadeias) ou `fuzzy` (nada exacto;
    /// resultados por semelhança — a UI deve dizê-lo).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text_match: Option<&'static str>,
}

/// Compila os parâmetros contra o schema do recurso.
pub fn compile_for(
    r: &ResourceSql,
    params: &SearchParams,
    me: Uuid,
) -> Result<ListQuery, ApiError> {
    compile(r.schema, &ListParams::from(params), Ctx { me }).map_err(ApiError::Domain)
}

/// O fuso de quem pede numa lista sem `{org_id}`: o da sua organização
/// principal; sem organização, Africa/Luanda.
pub async fn scope_for_user(state: &AppState, me: Uuid) -> Result<Scope, ApiError> {
    let tz = crate::org::primary_org_of_user(state, me)
        .await?
        .map(|(_, tz)| tz)
        .unwrap_or_else(|| DEFAULT_TZ.to_string());
    Ok(Scope {
        me,
        org_id: None,
        tz,
    })
}

pub async fn scope_for_org(state: &AppState, me: Uuid, org_id: Uuid) -> Result<Scope, ApiError> {
    Ok(Scope {
        me,
        org_id: Some(org_id),
        tz: crate::org::org_timezone(state, org_id).await?,
    })
}

/// Corre a lista e devolve os hits + o resto do envelope; o chamador carrega
/// os itens na forma da colecção.
pub async fn run(
    state: &AppState,
    r: &ResourceSql,
    q: &ListQuery,
    scope: &Scope,
) -> Result<ListOutcome, ApiError> {
    sql::run_list(&state.db, r, q, scope).await
}

/// Junta os itens carregados ao envelope, pela ordem dos hits, com o realce.
pub fn envelope<T>(
    has_text: bool,
    out: ListOutcome,
    items: Vec<(RowId, T)>,
    highlights: &mut std::collections::HashMap<String, Vec<HighlightSegment>>,
) -> SearchPage<SearchItem<T>> {
    let scores: std::collections::HashMap<String, Option<f64>> = out
        .hits
        .iter()
        .map(|h| (row_id_text(&h.id), h.score))
        .collect();
    SearchPage {
        items: items
            .into_iter()
            .map(|(id, item)| {
                let key = row_id_text(&id);
                let search = highlights.remove(&key).map(|highlight| ItemSearch {
                    score: scores.get(&key).copied().flatten().unwrap_or(0.0),
                    highlight,
                });
                SearchItem { item, search }
            })
            .collect(),
        next_page_token: out.next_page_token,
        total: out.total,
        total_kind: out.total_kind,
        groups: out.groups,
        next_groups_page_token: out.next_groups_page_token,
        text_match: has_text.then_some(if out.fuzzy { "fuzzy" } else { "exact" }),
    }
}

pub fn row_id_text(id: &RowId) -> String {
    match id {
        RowId::Uuid(u) => u.to_string(),
        RowId::Int(n) => n.to_string(),
    }
}

pub fn uuid_ids(out: &ListOutcome) -> Vec<Uuid> {
    out.hits
        .iter()
        .filter_map(|h| match h.id {
            RowId::Uuid(u) => Some(u),
            RowId::Int(_) => None,
        })
        .collect()
}

// ---------------------------------------------------------------------------
//  Descrição — GET /api/search/schemas[/{resource}]
// ---------------------------------------------------------------------------

#[derive(Serialize, utoipa::ToSchema)]
pub struct SchemaList {
    #[schema(value_type = Vec<Object>)]
    pub items: Vec<SchemaView>,
}

/// As listas brancas de pesquisa de todos os recursos (contrato §3).
///
/// Os recursos de organização (`org_scoped`) só aparecem a quem é membro
/// activo de alguma; a auditoria só a quem é admin activo de alguma.
#[utoipa::path(
    get, path = "/api/search/schemas", tag = "search",
    security(("session" = [])),
    responses(
        (status = 200, body = SchemaList),
        (status = 401, body = crate::openapi::ErrorBody),
    )
)]
pub async fn schemas(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
) -> Result<Json<SchemaList>, ApiError> {
    let primary = crate::org::primary_org_of_user(&state, auth.user_id).await?;
    let tz = primary
        .as_ref()
        .map(|(_, tz)| tz.clone())
        .unwrap_or_else(|| DEFAULT_TZ.to_string());
    let is_admin = crate::org::is_admin_somewhere(&state, auth.user_id).await?;
    let items = delonix_meet_domain::search::schemas()
        .into_iter()
        .filter(|s| match s.resource {
            "audit_events" => is_admin,
            _ if s.org_scoped => primary.is_some(),
            _ => true,
        })
        .map(|s| s.view(&tz))
        .collect();
    Ok(Json(SchemaList { items }))
}

/// A lista branca de pesquisa de um recurso: campos, operadores, filtros
/// pré-definidos, agrupamentos (contrato §3).
#[utoipa::path(
    get, path = "/api/search/schemas/{resource}", tag = "search",
    security(("session" = [])),
    params(("resource" = String, Path, description = "`meetings`, `recordings`, `members`, `whiteboards`, `audit_events`.")),
    responses(
        (status = 200, body = Object),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 404, description = "`search.unknown_resource`.", body = crate::openapi::ErrorBody),
    )
)]
pub async fn schema(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(resource): Path<String>,
) -> Result<Json<SchemaView>, ApiError> {
    let s = delonix_meet_domain::search::schema(&resource)
        .ok_or_else(|| ApiError::Domain(DomainError::not_found("search.unknown_resource")))?;
    let tz = crate::org::primary_org_of_user(&state, auth.user_id)
        .await?
        .map(|(_, tz)| tz)
        .unwrap_or_else(|| DEFAULT_TZ.to_string());
    Ok(Json(s.view(&tz)))
}

// ---------------------------------------------------------------------------
//  Listas por recurso (chamadas pelos handlers de colecção)
// ---------------------------------------------------------------------------

/// Texto dos campos para o `ts_headline`, por recurso (id + txt).
pub mod headline_sources {
    pub const RECORDINGS: &str =
        "SELECT r.id, CASE WHEN r.transcript <> '' THEN coalesce(r.title, '') || ' ' || r.filename || ' — ' || r.transcript ELSE coalesce(r.title, '') || ' ' || r.filename END AS txt FROM recordings r";
    pub const MEETINGS: &str =
        "SELECT m.id, m.title || ' — ' || m.description || ' — ' || m.minutes AS txt FROM meetings m";
    pub const MEMBERS: &str =
        "SELECT u.id, u.username || ' <' || u.email || '>' AS txt FROM users u";
    pub const WHITEBOARDS: &str =
        "SELECT w.id, w.title || ' · ' || w.room_code AS txt FROM whiteboards w";
    pub const AUDIT_EVENTS: &str =
        "SELECT a.id, a.action || ' ' || a.target || ' · ' || a.actor_name AS txt FROM audit_logs a";
}

async fn highlights_for(
    state: &AppState,
    q: &ListQuery,
    out: &ListOutcome,
    source: &'static str,
) -> Result<std::collections::HashMap<String, Vec<HighlightSegment>>, ApiError> {
    let Some(t) = &q.text else {
        return Ok(Default::default());
    };
    let uuids = uuid_ids(out);
    let ints: Vec<i64> = out
        .hits
        .iter()
        .filter_map(|h| match h.id {
            RowId::Int(n) => Some(n),
            RowId::Uuid(_) => None,
        })
        .collect();
    let ids = if ints.is_empty() {
        sql::Ids::Uuid(&uuids)
    } else {
        sql::Ids::Int(&ints)
    };
    sql::headlines(&state.db, source, ids, t).await
}

pub type RecordingSearchPage = SearchPage<SearchItem<crate::recordings::RecordingItem>>;

pub async fn list_recordings(
    state: &AppState,
    me: Uuid,
    params: &SearchParams,
) -> Result<RecordingSearchPage, ApiError> {
    let r = &resources::RECORDINGS;
    let q = compile_for(r, params, me)?;
    let scope = scope_for_user(state, me).await?;
    let out = run(state, r, &q, &scope).await?;
    let ids = uuid_ids(&out);
    let mut hl = highlights_for(state, &q, &out, headline_sources::RECORDINGS).await?;
    let items = crate::recordings::library_items_by_ids(state, me, &ids)
        .await?
        .into_iter()
        .map(|mut item| {
            // Compatibilidade com o `snippet` da 0045 (marcas «»).
            if let Some(segs) = hl.get(&item.id.to_string()) {
                item.snippet = Some(
                    segs.iter()
                        .map(|s| {
                            if s.is_match {
                                format!("«{}»", s.text)
                            } else {
                                s.text.clone()
                            }
                        })
                        .collect(),
                );
            }
            (RowId::Uuid(item.id), item)
        })
        .collect();
    Ok(envelope(q.text.is_some(), out, items, &mut hl))
}

pub type MeetingSearchPage = SearchPage<SearchItem<crate::meetings::MeetingItem>>;

pub async fn list_meetings(
    state: &AppState,
    me: Uuid,
    params: &SearchParams,
) -> Result<MeetingSearchPage, ApiError> {
    let r = &resources::MEETINGS;
    let q = compile_for(r, params, me)?;
    let scope = scope_for_user(state, me).await?;
    let out = run(state, r, &q, &scope).await?;
    let ids = uuid_ids(&out);
    let mut hl = highlights_for(state, &q, &out, headline_sources::MEETINGS).await?;
    let items = crate::meetings::items_by_ids(state, me, &ids)
        .await?
        .into_iter()
        .map(|m| (RowId::Uuid(m.id), m))
        .collect();
    Ok(envelope(q.text.is_some(), out, items, &mut hl))
}

pub type MemberSearchPage = SearchPage<SearchItem<crate::org::Employee>>;

/// Chamar DEPOIS de `require_member`.
pub async fn list_members(
    state: &AppState,
    me: Uuid,
    org_id: Uuid,
    params: &SearchParams,
) -> Result<MemberSearchPage, ApiError> {
    let r = &resources::MEMBERS;
    let q = compile_for(r, params, me)?;
    let scope = scope_for_org(state, me, org_id).await?;
    let out = run(state, r, &q, &scope).await?;
    let ids = uuid_ids(&out);
    let mut hl = highlights_for(state, &q, &out, headline_sources::MEMBERS).await?;
    let mut by_id: std::collections::HashMap<Uuid, crate::org::Employee> =
        crate::org::employees_by_ids(state, org_id, &ids)
            .await?
            .into_iter()
            .map(|e| (e.user_id, e))
            .collect();
    let items = ids
        .iter()
        .filter_map(|id| by_id.remove(id).map(|e| (RowId::Uuid(*id), e)))
        .collect();
    Ok(envelope(q.text.is_some(), out, items, &mut hl))
}

pub type WhiteboardSearchPage = SearchPage<SearchItem<crate::whiteboards::WhiteboardMeta>>;

pub async fn list_whiteboards(
    state: &AppState,
    me: Uuid,
    params: &SearchParams,
) -> Result<WhiteboardSearchPage, ApiError> {
    let r = &resources::WHITEBOARDS;
    let q = compile_for(r, params, me)?;
    let scope = scope_for_user(state, me).await?;
    let out = run(state, r, &q, &scope).await?;
    let ids = uuid_ids(&out);
    let mut hl = highlights_for(state, &q, &out, headline_sources::WHITEBOARDS).await?;
    let items = crate::whiteboards::metas_by_ids(state, me, &ids)
        .await?
        .into_iter()
        .map(|w| (RowId::Uuid(w.id), w))
        .collect();
    Ok(envelope(q.text.is_some(), out, items, &mut hl))
}

pub type AuditSearchPage = SearchPage<SearchItem<crate::audit::AuditEntry>>;

/// Chamar DEPOIS de `require_admin`.
pub async fn list_audit_events(
    state: &AppState,
    me: Uuid,
    org_id: Uuid,
    params: &SearchParams,
) -> Result<AuditSearchPage, ApiError> {
    let r = &resources::AUDIT_EVENTS;
    let q = compile_for(r, params, me)?;
    let scope = scope_for_org(state, me, org_id).await?;
    let out = run(state, r, &q, &scope).await?;
    let ids: Vec<i64> = out
        .hits
        .iter()
        .filter_map(|h| match h.id {
            RowId::Int(n) => Some(n),
            RowId::Uuid(_) => None,
        })
        .collect();
    let mut hl = highlights_for(state, &q, &out, headline_sources::AUDIT_EVENTS).await?;
    let items = crate::audit::entries_by_ids(state, org_id, &ids)
        .await?
        .into_iter()
        .map(|e| (RowId::Int(e.id), e))
        .collect();
    Ok(envelope(q.text.is_some(), out, items, &mut hl))
}
