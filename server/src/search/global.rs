//! `GET /api/search` — a pesquisa global do Ctrl+K (contrato §1).
//!
//! Cada tipo corre a sua consulta em paralelo e aplica a MESMA regra de
//! visibilidade do endpoint normal: os tipos que também são listas
//! (reuniões, gravações, quadros, auditoria) passam pelo motor das listas
//! (`sql::run_list`) com as mesmas `ResourceSql`; os restantes compõem as
//! regras de pertença de `org.rs`. Nenhum tipo lê `org_id` do pedido.

use std::{collections::HashMap, sync::Arc, time::Instant};

use axum::{
    extract::{Query, State},
    Json,
};
use chrono::{DateTime, Utc};
use delonix_meet_core::{
    query::{
        capped_total, highlight_segments, parse_text, HighlightSegment, ListQuery, TextQuery,
        TotalKind, HL_START, HL_STOP,
    },
    DomainError,
};
use delonix_meet_domain::identity::authorization::Capability;
use futures_util::future::join_all;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value as Json_};
use sqlx::{Postgres, QueryBuilder};
use uuid::Uuid;

use super::{
    headline_sources, resources,
    sql::{self, push_score_expr, push_text_match, ResourceSql, Scope},
    SearchParams,
};
use crate::{auth::AuthUser, error::ApiError, org, AppState};

pub const DEFAULT_LIMIT: u32 = 5;
/// No Ctrl+K a contagem é exacta até 1000 (depois, `at_least`): contar as
/// 38 000 mensagens com «orçamento» não muda o que a pessoa vê.
pub const COUNT_CAP: i64 = 1000;
pub const MAX_LIMIT: u32 = 20;

/// Os tipos, pela ordem em que os grupos saem.
pub const TYPES: [&str; 9] = [
    "meetings",
    "recordings",
    "people",
    "whiteboards",
    "rooms",
    "messages",
    "stream_destinations",
    "webhooks",
    "audit_events",
];

/// Os tipos reservados e a capacidade que cada um exige — a MESMA do endpoint
/// normal (ADR-0008 §4): `stream_destinations::require_rtmp_keys`,
/// `org::require_admin` nos webhooks, `admin.view_audit` na auditoria.
/// `audit_events` só entra quando pedido explicitamente.
fn required_capability(t: &str) -> Option<Capability> {
    match t {
        "stream_destinations" => Some(Capability::BroadcastManageRtmpKeys),
        "webhooks" => Some(Capability::OrgAdminister),
        "audit_events" => Some(Capability::AdminViewAudit),
        _ => None,
    }
}

#[derive(Debug, Deserialize, utoipa::IntoParams)]
#[into_params(parameter_in = Query)]
pub struct GlobalQuery {
    /// 1–200 caracteres, com pelo menos uma letra ou dígito.
    pub q: Option<String>,
    /// Tipos separados por vírgulas (omissão: todos os que a pessoa pode ver).
    pub types: Option<String>,
    /// Resultados por tipo, 1–20 (omissão 5).
    pub limit: Option<u32>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct GlobalHit {
    #[serde(rename = "type")]
    pub kind: &'static str,
    pub id: String,
    pub title: String,
    pub subtitle: String,
    #[schema(value_type = Vec<Object>)]
    pub highlight: Vec<HighlightSegment>,
    pub matched_in: &'static str,
    pub score: f64,
    #[schema(value_type = Object)]
    pub target: Json_,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub href: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub occurred_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct GlobalGroup {
    #[serde(rename = "type")]
    pub kind: &'static str,
    pub count: i64,
    #[schema(value_type = String)]
    pub count_kind: TotalKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub more_href: Option<String>,
    pub items: Vec<GlobalHit>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct Skipped {
    #[serde(rename = "type")]
    pub kind: &'static str,
    pub code: &'static str,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct GlobalResponse {
    pub query: String,
    pub took_ms: u64,
    pub groups: Vec<GlobalGroup>,
    pub skipped: Vec<Skipped>,
}

/// Pesquisa global (Ctrl+K): reuniões, gravações (título, transcrição,
/// capítulos e comentários), pessoas, quadros, salas, mensagens de chat e,
/// para admins, destinos de emissão, webhooks e auditoria.
#[utoipa::path(
    get, path = "/api/search", tag = "search",
    security(("session" = [])),
    params(GlobalQuery),
    responses(
        (status = 200, body = GlobalResponse),
        (status = 400, description = "`search.invalid_query`, `search.invalid_types`.", body = crate::openapi::ErrorBody),
        (status = 401, body = crate::openapi::ErrorBody),
    )
)]
pub async fn search(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Query(params): Query<GlobalQuery>,
) -> Result<Json<GlobalResponse>, ApiError> {
    let started = Instant::now();
    let me = auth.user_id;
    let raw = params.q.unwrap_or_default();
    let text = parse_text(&raw)?.ok_or_else(|| {
        DomainError::invalid(
            "search.invalid_query",
            "a pesquisa tem de ter pelo menos uma letra ou dígito",
        )
        .with_field("q", "obrigatório")
    })?;
    let limit = params.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT);

    let explicit: Option<Vec<&'static str>> = match params.types.as_deref().map(str::trim) {
        None | Some("") => None,
        Some(list) => {
            let mut out = Vec::new();
            for t in list.split(',').map(str::trim).filter(|t| !t.is_empty()) {
                let known = TYPES.iter().find(|k| **k == t).ok_or_else(|| {
                    DomainError::invalid("search.invalid_types", "tipo desconhecido")
                        .with_field("types", "tipo desconhecido")
                })?;
                if !out.contains(known) {
                    out.push(*known);
                }
            }
            Some(out)
        }
    };
    let mut skipped = Vec::new();
    let mut wanted: Vec<&'static str> = Vec::new();
    for t in TYPES.iter().copied().filter(|t| match &explicit {
        Some(list) => list.contains(t),
        None => *t != "audit_events",
    }) {
        if let Some(cap) = required_capability(t) {
            if !org::has_capability_in_any_org(&state, me, cap).await? {
                if explicit.is_some() {
                    skipped.push(Skipped {
                        kind: t,
                        code: "search.forbidden",
                    });
                }
                continue;
            }
        }
        wanted.push(t);
    }

    let scope = super::scope_for_user(&state, me).await?;
    let futures = wanted.iter().map(|t| {
        let state = state.clone();
        let text = text.clone();
        let scope = scope.clone();
        let raw = raw.clone();
        async move {
            let r = match *t {
                "meetings" => meetings(&state, &scope, &text, &raw, limit).await,
                "recordings" => recordings(&state, &scope, &text, &raw, limit).await,
                "whiteboards" => whiteboards(&state, &scope, &text, &raw, limit).await,
                "people" => people(&state, &scope, &text, limit).await,
                "rooms" => rooms(&state, &scope, &text, limit).await,
                "messages" => messages(&state, &scope, &text, limit).await,
                "stream_destinations" => stream_destinations(&state, &scope, &text, limit).await,
                "webhooks" => webhooks(&state, &scope, &text, limit).await,
                _ => audit_events(&state, &scope, &text, limit).await,
            };
            r.map(|g| (*t, g))
        }
    });
    let mut groups = Vec::new();
    for res in join_all(futures).await {
        let (_, group) = res?;
        if !group.items.is_empty() {
            groups.push(group);
        }
    }
    Ok(Json(GlobalResponse {
        query: raw,
        took_ms: started.elapsed().as_millis() as u64,
        groups,
        skipped,
    }))
}

fn more_href(collection: &str, raw: &str) -> Option<String> {
    Some(format!("{collection}?q={}", urlencode(raw)))
}

fn urlencode(s: &str) -> String {
    url::form_urlencoded::byte_serialize(s.as_bytes()).collect()
}

/// Corre um tipo que é também lista, com `q` e ordem por relevância.
async fn via_list(
    state: &AppState,
    r: &ResourceSql,
    scope: &Scope,
    raw: &str,
    limit: u32,
) -> Result<(ListQuery, sql::ListOutcome), ApiError> {
    let params = SearchParams {
        q: Some(raw.to_string()),
        order_by: Some("-_score".into()),
        page_size: Some(limit),
        ..Default::default()
    };
    let mut q = super::compile_for(r, &params, scope.me)?;
    q.total_cap = COUNT_CAP;
    let out = sql::run_list(&state.db, r, &q, scope).await?;
    Ok((q, out))
}

fn scores(out: &sql::ListOutcome) -> HashMap<String, f64> {
    out.hits
        .iter()
        .map(|h| (super::row_id_text(&h.id), h.score.unwrap_or(0.0)))
        .collect()
}

async fn meetings(
    state: &AppState,
    scope: &Scope,
    text: &TextQuery,
    raw: &str,
    limit: u32,
) -> Result<GlobalGroup, ApiError> {
    let (_, out) = via_list(state, &resources::MEETINGS, scope, raw, limit).await?;
    let ids = super::uuid_ids(&out);
    let mut hl = sql::headlines(
        &state.db,
        headline_sources::MEETINGS,
        sql::Ids::Uuid(&ids),
        text,
    )
    .await?;
    let matched = matched_columns(
        state,
        "SELECT m.id, CASE WHEN to_tsvector('dlx_search', m.title) @@ q.q THEN 'title' \
         WHEN to_tsvector('dlx_search', m.description) @@ q.q THEN 'description' \
         WHEN to_tsvector('dlx_search', m.minutes) @@ q.q THEN 'minutes' ELSE 'title' END \
         FROM meetings m, to_tsquery('dlx_search', $1) AS q(q) WHERE m.id = ANY($2)",
        text,
        &ids,
    )
    .await?;
    let sc = scores(&out);
    let items = crate::meetings::items_by_ids(state, scope.me, &ids)
        .await?
        .into_iter()
        .map(|m| {
            let id = m.id.to_string();
            GlobalHit {
                kind: "meetings",
                title: m.title.clone(),
                subtitle: format!("{} · {}", m.owner_name, m.starts_at.format("%Y-%m-%d %H:%M")),
                highlight: hl.remove(&id).unwrap_or_default(),
                matched_in: matched.get(&m.id).copied().unwrap_or("title"),
                score: sc.get(&id).copied().unwrap_or(0.0),
                target: json!({"meeting_id": m.id, "room_code": m.room_code, "starts_at": m.starts_at}),
                href: Some(format!("/api/meetings/{}", m.id)),
                occurred_at: Some(m.starts_at),
                id,
            }
        })
        .collect();
    Ok(GlobalGroup {
        kind: "meetings",
        count: out.total,
        count_kind: out.total_kind,
        more_href: more_href("/api/meetings", raw),
        items,
    })
}

/// Em que coluna acertou cada id (primeira que casa com o tsquery).
async fn matched_columns(
    state: &AppState,
    sql_text: &'static str,
    text: &TextQuery,
    ids: &[Uuid],
) -> Result<HashMap<Uuid, &'static str>, ApiError> {
    if ids.is_empty() {
        return Ok(HashMap::new());
    }
    let rows: Vec<(Uuid, String)> = sqlx::query_as(sql_text)
        .bind(&text.tsquery)
        .bind(ids)
        .fetch_all(&state.db)
        .await?;
    Ok(rows
        .into_iter()
        .map(|(id, col)| {
            let col: &'static str = match col.as_str() {
                "description" => "description",
                "minutes" => "minutes",
                "transcript" => "transcript",
                _ => "title",
            };
            (id, col)
        })
        .collect())
}

async fn recordings(
    state: &AppState,
    scope: &Scope,
    text: &TextQuery,
    raw: &str,
    limit: u32,
) -> Result<GlobalGroup, ApiError> {
    let (_, out) = via_list(state, &resources::RECORDINGS, scope, raw, limit).await?;
    let ids = super::uuid_ids(&out);
    let mut hl = sql::headlines(
        &state.db,
        headline_sources::RECORDINGS,
        sql::Ids::Uuid(&ids),
        text,
    )
    .await?;
    let matched = matched_columns(
        state,
        "SELECT r.id, CASE WHEN to_tsvector('dlx_search', coalesce(r.title, '') || ' ' || r.filename) @@ q.q \
         THEN 'title' WHEN to_tsvector('dlx_search', r.transcript) @@ q.q THEN 'transcript' ELSE 'title' END \
         FROM recordings r, to_tsquery('dlx_search', $1) AS q(q) WHERE r.id = ANY($2)",
        text,
        &ids,
    )
    .await?;
    let sc = scores(&out);
    let mut items: Vec<GlobalHit> = crate::recordings::library_items_by_ids(state, scope.me, &ids)
        .await?
        .into_iter()
        .map(|r| {
            let id = r.id.to_string();
            GlobalHit {
                kind: "recordings",
                title: r.filename.clone(),
                subtitle: format!("{} · {}", r.uploader_name, r.created_at.format("%Y-%m-%d")),
                highlight: hl.remove(&id).unwrap_or_default(),
                matched_in: matched.get(&r.id).copied().unwrap_or("title"),
                score: sc.get(&id).copied().unwrap_or(0.0),
                target: json!({"recording_id": r.id, "at_secs": null}),
                href: Some(format!("/api/recordings/{}", r.id)),
                occurred_at: Some(r.created_at),
                id,
            }
        })
        .collect();

    // Capítulos e comentários com marca temporal, das gravações VISÍVEIS.
    for (table, col, where_extra, matched_in) in [
        ("recording_chapters", "title", "", "chapter"),
        (
            "recording_comments",
            "body",
            " AND c.deleted_at IS NULL",
            "comment",
        ),
    ] {
        // `t_ms` desde a 0068; o alvo continua em segundos.
        let mut qb =
            QueryBuilder::<Postgres>::new("SELECT c.recording_id, (c.t_ms / 1000)::int4, ");
        qb.push(format_args!(
            "ts_headline('dlx_search', c.{col}, to_tsquery('dlx_search', "
        ));
        qb.push_bind(text.tsquery.as_str());
        qb.push(format_args!(
            "), 'MaxFragments=1, MaxWords=18, MinWords=6, StartSel={HL_START}, StopSel={HL_STOP}'), \
             ts_rank_cd(to_tsvector('dlx_search', c.{col}), to_tsquery('dlx_search', "
        ));
        qb.push_bind(text.tsquery.as_str());
        qb.push("))::float8 AS score FROM (SELECT ");
        qb.push_bind(scope.me);
        qb.push("::uuid AS id, NULL::uuid AS org_id, ");
        qb.push_bind(scope.tz.as_str());
        qb.push(format_args!("::text AS tz) viewer CROSS JOIN {table} c WHERE to_tsvector('dlx_search', c.{col}) @@ to_tsquery('dlx_search', "));
        qb.push_bind(text.tsquery.as_str());
        qb.push(")");
        qb.push(where_extra);
        qb.push(" AND c.recording_id IN (SELECT r.id FROM (SELECT 1) AS _one");
        qb.push((resources::RECORDINGS.from)());
        qb.push(") ORDER BY score DESC, c.id LIMIT ");
        qb.push_bind(limit as i64);
        let rows: Vec<(Uuid, Option<i32>, String, f64)> =
            qb.build_query_as().fetch_all(&state.db).await?;
        if rows.is_empty() {
            continue;
        }
        let rec_ids: Vec<Uuid> = rows.iter().map(|r| r.0).collect();
        let recs: HashMap<Uuid, crate::recordings::RecordingItem> =
            crate::recordings::library_items_by_ids(state, scope.me, &rec_ids)
                .await?
                .into_iter()
                .map(|r| (r.id, r))
                .collect();
        for (rec_id, at_secs, marked, score) in rows {
            let Some(r) = recs.get(&rec_id) else { continue };
            items.push(GlobalHit {
                kind: "recordings",
                id: r.id.to_string(),
                title: r.filename.clone(),
                subtitle: match at_secs {
                    Some(s) => format!("{} · {:02}:{:02}", r.uploader_name, s / 60, s % 60),
                    None => r.uploader_name.clone(),
                },
                highlight: highlight_segments(&marked),
                matched_in,
                score,
                target: json!({"recording_id": r.id, "at_secs": at_secs}),
                href: Some(format!("/api/recordings/{}", r.id)),
                occurred_at: Some(r.created_at),
            });
        }
    }
    items.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| b.occurred_at.cmp(&a.occurred_at))
            .then_with(|| a.id.cmp(&b.id))
    });
    items.truncate(limit as usize);
    Ok(GlobalGroup {
        kind: "recordings",
        count: out.total,
        count_kind: out.total_kind,
        more_href: more_href("/api/recordings", raw),
        items,
    })
}

async fn whiteboards(
    state: &AppState,
    scope: &Scope,
    text: &TextQuery,
    raw: &str,
    limit: u32,
) -> Result<GlobalGroup, ApiError> {
    let (_, out) = via_list(state, &resources::WHITEBOARDS, scope, raw, limit).await?;
    let ids = super::uuid_ids(&out);
    let sc = scores(&out);
    let items = crate::whiteboards::metas_by_ids(state, scope.me, &ids)
        .await?
        .into_iter()
        .map(|w| {
            let id = w.id.to_string();
            let shown = format!("{} · {}", w.title, w.room_code);
            GlobalHit {
                kind: "whiteboards",
                highlight: substring_segments(&shown, text),
                title: w.title,
                subtitle: w.room_code.clone(),
                matched_in: "title",
                score: sc.get(&id).copied().unwrap_or(0.0),
                target: json!({"whiteboard_id": w.id}),
                href: None,
                occurred_at: Some(w.created_at),
                id,
            }
        })
        .collect();
    Ok(GlobalGroup {
        kind: "whiteboards",
        count: out.total,
        count_kind: out.total_kind,
        more_href: more_href("/api/whiteboards", raw),
        items,
    })
}

/// Realce por subcadeia (tipos sem `tsvector`): termos em minúsculas, sem
/// dobrar acentos — se nada casar, o texto sai inteiro sem realce.
pub fn substring_segments(shown: &str, text: &TextQuery) -> Vec<HighlightSegment> {
    let lower = shown.to_lowercase();
    // Só funciona quando minúsculas não mudam o tamanho em bytes.
    if lower.len() != shown.len() {
        return vec![HighlightSegment {
            text: shown.to_string(),
            is_match: false,
        }];
    }
    let mut marks = vec![false; shown.len()];
    for term in &text.terms {
        let mut from = 0;
        while let Some(pos) = lower[from..].find(term.as_str()) {
            let start = from + pos;
            for m in marks.iter_mut().skip(start).take(term.len()) {
                *m = true;
            }
            from = start + term.len().max(1);
        }
    }
    let mut marked = String::with_capacity(shown.len() + 8);
    let mut on = false;
    for (i, c) in shown.char_indices() {
        if marks[i] != on {
            marked.push(if marks[i] { HL_START } else { HL_STOP });
            on = marks[i];
        }
        marked.push(c);
    }
    if on {
        marked.push(HL_STOP);
    }
    highlight_segments(&marked)
}

// ---------------------------------------------------------------------------
//  Tipos que não são listas
// ---------------------------------------------------------------------------

fn push_viewer<'a>(qb: &mut QueryBuilder<'a, Postgres>, scope: &'a Scope) {
    qb.push(" FROM (SELECT ");
    qb.push_bind(scope.me);
    qb.push("::uuid AS id, NULL::uuid AS org_id, ");
    qb.push_bind(scope.tz.as_str());
    qb.push("::text AS tz) viewer");
}

/// Constrói a consulta de um tipo (itens ou contagem) a partir do corpo
/// comum `FROM … WHERE visibilidade AND texto`.
/// A consulta de um tipo que não é lista: tudo escrito no código.
struct Custom {
    select: &'static str,
    body: for<'q> fn(&mut QueryBuilder<'q, Postgres>, &'q TextQuery, bool),
    order: &'static str,
    score: Option<(Option<&'static str>, &'static [&'static str])>,
}

async fn run_custom<T>(
    state: &AppState,
    scope: &Scope,
    text: &TextQuery,
    limit: u32,
    c: Custom,
) -> Result<(Vec<T>, i64, TotalKind), ApiError>
where
    T: for<'r> sqlx::FromRow<'r, sqlx::postgres::PgRow> + Send + Unpin,
{
    let Custom {
        select,
        body,
        order,
        score,
    } = c;
    // Exacta primeiro; aproximada só se a exacta não der nada (como nas listas).
    for fuzzy in [false, true] {
        if fuzzy && text.raw.chars().count() < 3 {
            break;
        }
        let mut tx = state.db.begin().await?;
        sqlx::query(&format!(
            "SET LOCAL pg_trgm.word_similarity_threshold = {}",
            sql::WORD_SIMILARITY_THRESHOLD
        ))
        .execute(&mut *tx)
        .await?;
        let mut qb = QueryBuilder::<Postgres>::new("SELECT ");
        qb.push(select);
        qb.push(", ");
        match score {
            Some((fts, tri)) => push_score_expr(&mut qb, fts, tri, text),
            None => {
                qb.push("0::float8");
            }
        }
        qb.push(" AS score");
        push_viewer(&mut qb, scope);
        body(&mut qb, text, fuzzy);
        qb.push(" ORDER BY score DESC, ");
        qb.push(order);
        qb.push(" LIMIT ");
        qb.push_bind(limit as i64);
        let rows: Vec<T> = qb.build_query_as().fetch_all(&mut *tx).await?;
        if rows.is_empty() && !fuzzy {
            tx.commit().await?;
            continue;
        }
        let mut cq = QueryBuilder::<Postgres>::new("SELECT count(*) FROM (SELECT 1");
        push_viewer(&mut cq, scope);
        body(&mut cq, text, fuzzy);
        cq.push(" LIMIT ");
        cq.push_bind(COUNT_CAP + 1);
        cq.push(") c");
        let n: i64 = cq.build_query_scalar().fetch_one(&mut *tx).await?;
        tx.commit().await?;
        let (count, kind) = capped_total(n, COUNT_CAP);
        return Ok((rows, count, kind));
    }
    Ok((Vec::new(), 0, TotalKind::Exact))
}

const PEOPLE_TRGM: &[&str] = &["dlx_fold(u.username || ' ' || u.email)"];

#[derive(sqlx::FromRow)]
struct PersonRow {
    id: Uuid,
    username: String,
    email: String,
    score: f64,
}

async fn people(
    state: &AppState,
    scope: &Scope,
    text: &TextQuery,
    limit: u32,
) -> Result<GlobalGroup, ApiError> {
    fn body<'q>(qb: &mut QueryBuilder<'q, Postgres>, t: &'q TextQuery, fuzzy: bool) {
        // A regra do `users::search`: colegas ACTIVOS de uma org comum.
        // A conversa directa (migração 0051) só a vê quem a enviou e quem a
        // recebeu — a regra de `rooms::room_chat`.
        static VIS: std::sync::LazyLock<String> = std::sync::LazyLock::new(|| {
            [
                " CROSS JOIN users u WHERE u.id <> viewer.id AND ",
                &org::sql_active_colleague_of_viewer("u.id"),
                " AND ",
            ]
            .concat()
        });
        qb.push(VIS.as_str());
        push_text_match(qb, None, PEOPLE_TRGM, t, fuzzy);
    }
    let (rows, count, kind) = run_custom::<PersonRow>(
        state,
        scope,
        text,
        limit,
        Custom {
            select: "u.id, u.username, u.email",
            body,
            order: "u.username",
            score: Some((None, PEOPLE_TRGM)),
        },
    )
    .await?;
    let items = rows
        .into_iter()
        .map(|p| {
            let shown = format!("{} <{}>", p.username, p.email);
            GlobalHit {
                kind: "people",
                id: p.id.to_string(),
                highlight: substring_segments(&shown, text),
                matched_in: if text
                    .terms
                    .iter()
                    .any(|t| p.username.to_lowercase().contains(t))
                {
                    "name"
                } else {
                    "email"
                },
                title: p.username,
                subtitle: p.email,
                score: p.score,
                target: json!({"user_id": p.id}),
                href: None,
                occurred_at: None,
            }
        })
        .collect();
    Ok(GlobalGroup {
        kind: "people",
        count,
        count_kind: kind,
        more_href: None,
        items,
    })
}

const ROOMS_TRGM: &[&str] = &["dlx_fold(rm.code || ' ' || rm.name)"];

#[derive(sqlx::FromRow)]
struct RoomRow {
    code: String,
    name: String,
    created_at: DateTime<Utc>,
    score: f64,
}

async fn rooms(
    state: &AppState,
    scope: &Scope,
    text: &TextQuery,
    limit: u32,
) -> Result<GlobalGroup, ApiError> {
    fn body<'q>(qb: &mut QueryBuilder<'q, Postgres>, t: &'q TextQuery, fuzzy: bool) {
        // Salas que a pessoa JÁ conhece: dela, onde esteve, convidada
        // para uma reunião nessa sala, ou co-anfitriã. Mais restrito do
        // que `room_access` (que autoriza qualquer colega a pedir para
        // entrar): a pesquisa não revela códigos de salas de colegas.
        qb.push(
            " CROSS JOIN rooms rm WHERE rm.id IN (\
               SELECT r1.id FROM rooms r1 WHERE r1.owner_id = viewer.id \
               UNION SELECT p.room_id FROM room_participants p WHERE p.user_id = viewer.id \
               UNION SELECT r3.id FROM rooms r3 JOIN meetings m3 ON m3.room_code = r3.code \
                      JOIN meeting_invitees mi3 ON mi3.meeting_id = m3.id WHERE mi3.user_id = viewer.id \
               UNION SELECT ra.room_id FROM room_admitters ra WHERE ra.user_id = viewer.id) AND ",
        );
        push_text_match(qb, None, ROOMS_TRGM, t, fuzzy);
    }
    let (rows, count, kind) = run_custom::<RoomRow>(
        state,
        scope,
        text,
        limit,
        Custom {
            select: "rm.code, rm.name, rm.created_at",
            body,
            order: "rm.created_at DESC",
            score: Some((None, ROOMS_TRGM)),
        },
    )
    .await?;
    let items = rows
        .into_iter()
        .map(|r| {
            let shown = format!("{} · {}", r.name, r.code);
            GlobalHit {
                kind: "rooms",
                id: r.code.clone(),
                highlight: substring_segments(&shown, text),
                matched_in: if text.terms.iter().any(|t| r.code.contains(t.as_str())) {
                    "code"
                } else {
                    "name"
                },
                title: r.name,
                subtitle: r.code.clone(),
                score: r.score,
                target: json!({"room_code": r.code}),
                href: Some(format!("/api/rooms/{}", r.code)),
                occurred_at: Some(r.created_at),
            }
        })
        .collect();
    Ok(GlobalGroup {
        kind: "rooms",
        count,
        count_kind: kind,
        more_href: None,
        items,
    })
}

#[derive(sqlx::FromRow)]
struct MessageRow {
    id: Uuid,
    code: String,
    username: String,
    headline: String,
    created_at: DateTime<Utc>,
    score: f64,
}

async fn messages(
    state: &AppState,
    scope: &Scope,
    text: &TextQuery,
    limit: u32,
) -> Result<GlobalGroup, ApiError> {
    fn body<'q>(qb: &mut QueryBuilder<'q, Postgres>, t: &'q TextQuery, fuzzy: bool) {
        // A conversa directa (migração 0051) só a vê quem a enviou e quem a
        // recebeu — a regra de `rooms::room_chat`.
        static VIS: std::sync::LazyLock<String> = std::sync::LazyLock::new(|| {
            [
                // Primeiro as salas de quem pede (dele ou onde esteve), em
                // semi-junção; depois, para as que não são dele, a regra do
                // `room_access` (quem saiu da org deixa de ver).
                " CROSS JOIN room_chat_messages c JOIN rooms rm ON rm.id = c.room_id \
                 WHERE c.room_id IN (SELECT r1.id FROM rooms r1 WHERE r1.owner_id = viewer.id \
                                     UNION SELECT p.room_id FROM room_participants p WHERE p.user_id = viewer.id) \
                 AND (rm.owner_id = viewer.id OR ",
                &org::sql_active_colleague_of_viewer("rm.owner_id"),
                " OR EXISTS (SELECT 1 FROM meeting_invitees mi JOIN meetings m ON m.id = mi.meeting_id \
                             WHERE m.room_code = rm.code AND mi.user_id = viewer.id) \
                   OR EXISTS (SELECT 1 FROM room_admitters ra WHERE ra.room_id = rm.id AND ra.user_id = viewer.id)) \
                 AND (c.to_user_id IS NULL OR c.user_id = viewer.id OR c.to_user_id = viewer.id) AND ",
            ]
            .concat()
        });
        qb.push(VIS.as_str());
        push_text_match(
            qb,
            Some("to_tsvector('dlx_search', c.message)"),
            &[],
            t,
            fuzzy,
        );
    }
    let (rows, count, kind) = run_custom::<MessageRow>(
        state,
        scope,
        text,
        limit,
        Custom {
            select: // O realce é feito na mesma consulta (o texto já está na linha).
        "c.id, rm.code, c.username, c.message AS headline, c.created_at",
            body,
            order: "c.created_at DESC",
            // Mensagens por recência: o `ts_rank_cd` relia cada mensagem.
                score: None,
        },
    )
    .await?;
    // `ts_headline` só para as linhas mostradas.
    let ids: Vec<Uuid> = rows.iter().map(|m| m.id).collect();
    let mut hl = sql::headlines(
        &state.db,
        "SELECT c.id, c.message AS txt FROM room_chat_messages c",
        sql::Ids::Uuid(&ids),
        text,
    )
    .await?;
    let items = rows
        .into_iter()
        .map(|m| {
            let id = m.id.to_string();
            GlobalHit {
                kind: "messages",
                highlight: hl.remove(&id).unwrap_or_else(|| {
                    vec![HighlightSegment {
                        text: m.headline.clone(),
                        is_match: false,
                    }]
                }),
                title: m.username,
                subtitle: m.code.clone(),
                matched_in: "message",
                score: m.score,
                target: json!({"room_code": m.code, "message_id": m.id, "created_at": m.created_at}),
                href: None,
                occurred_at: Some(m.created_at),
                id,
            }
        })
        .collect();
    Ok(GlobalGroup {
        kind: "messages",
        count,
        count_kind: kind,
        more_href: None,
        items,
    })
}

const DEST_TRGM: &[&str] = &["dlx_fold(sd.label || ' ' || sd.kind)"];

#[derive(sqlx::FromRow)]
struct DestRow {
    id: Uuid,
    org_id: Uuid,
    label: String,
    kind: String,
    created_at: DateTime<Utc>,
    score: f64,
}

async fn stream_destinations(
    state: &AppState,
    scope: &Scope,
    text: &TextQuery,
    limit: u32,
) -> Result<GlobalGroup, ApiError> {
    fn body<'q>(qb: &mut QueryBuilder<'q, Postgres>, t: &'q TextQuery, fuzzy: bool) {
        qb.push(" CROSS JOIN stream_destinations sd WHERE sd.org_id IN ");
        qb.push(org::sql_viewer_orgs_with(
            Capability::BroadcastManageRtmpKeys,
        ));
        qb.push(" AND ");
        push_text_match(qb, None, DEST_TRGM, t, fuzzy);
    }
    let (rows, count, kind) = run_custom::<DestRow>(
        state,
        scope,
        text,
        limit,
        Custom {
            select: "sd.id, sd.org_id, sd.label, sd.kind, sd.created_at",
            body,
            order: "sd.created_at DESC",
            score: Some((None, DEST_TRGM)),
        },
    )
    .await?;
    let items = rows
        .into_iter()
        .map(|d| {
            let shown = format!("{} · {}", d.label, d.kind);
            GlobalHit {
                kind: "stream_destinations",
                id: d.id.to_string(),
                highlight: substring_segments(&shown, text),
                title: d.label,
                subtitle: d.kind,
                matched_in: "name",
                score: d.score,
                target: json!({"org_id": d.org_id, "stream_destination_id": d.id}),
                href: Some(format!(
                    "/api/orgs/{}/stream-destinations/{}",
                    d.org_id, d.id
                )),
                occurred_at: Some(d.created_at),
            }
        })
        .collect();
    Ok(GlobalGroup {
        kind: "stream_destinations",
        count,
        count_kind: kind,
        more_href: None,
        items,
    })
}

/// Só o anfitrião do URL: o caminho de um webhook do Slack/Teams é o segredo.
const HOOK_TRGM: &[&str] =
    &["dlx_fold(w.kind || ' ' || coalesce(substring(w.url from '^[A-Za-z]+://([^/:?#]+)'), ''))"];

#[derive(sqlx::FromRow)]
struct HookRow {
    id: Uuid,
    org_id: Uuid,
    kind: String,
    host: Option<String>,
    created_at: DateTime<Utc>,
    score: f64,
}

async fn webhooks(
    state: &AppState,
    scope: &Scope,
    text: &TextQuery,
    limit: u32,
) -> Result<GlobalGroup, ApiError> {
    fn body<'q>(qb: &mut QueryBuilder<'q, Postgres>, t: &'q TextQuery, fuzzy: bool) {
        qb.push(" CROSS JOIN org_webhooks w WHERE w.org_id IN ");
        qb.push(org::sql_viewer_orgs_with(Capability::OrgAdminister));
        qb.push(" AND ");
        push_text_match(qb, None, HOOK_TRGM, t, fuzzy);
    }
    let (rows, count, kind) = run_custom::<HookRow>(
        state,
        scope,
        text,
        limit,
        Custom {
            select: "w.id, w.org_id, w.kind, substring(w.url from '^[A-Za-z]+://([^/:?#]+)') AS host, w.created_at",
            body,
            order: "w.created_at DESC",
            score: Some((None, HOOK_TRGM)),
        },
    )
    .await?;
    let items = rows
        .into_iter()
        .map(|w| {
            let host = w.host.unwrap_or_default();
            let shown = format!("{} · {}", w.kind, host);
            GlobalHit {
                kind: "webhooks",
                id: w.id.to_string(),
                highlight: substring_segments(&shown, text),
                title: host,
                subtitle: w.kind,
                matched_in: "url_host",
                score: w.score,
                target: json!({"org_id": w.org_id, "webhook_id": w.id}),
                href: None,
                occurred_at: Some(w.created_at),
            }
        })
        .collect();
    Ok(GlobalGroup {
        kind: "webhooks",
        count,
        count_kind: kind,
        more_href: None,
        items,
    })
}

const AUDIT_TRGM: &[&str] = &["dlx_fold(a.action || ' ' || a.target || ' ' || a.actor_name)"];

#[derive(sqlx::FromRow)]
struct AuditRow {
    id: i64,
    org_id: Option<Uuid>,
    action: String,
    target: String,
    actor_name: String,
    created_at: DateTime<Utc>,
    score: f64,
}

async fn audit_events(
    state: &AppState,
    scope: &Scope,
    text: &TextQuery,
    limit: u32,
) -> Result<GlobalGroup, ApiError> {
    fn body<'q>(qb: &mut QueryBuilder<'q, Postgres>, t: &'q TextQuery, fuzzy: bool) {
        // Subconjunto da regra do `audit::list`: os eventos das orgs onde
        // é admin (os sem org ficam para a lista da org).
        qb.push(" CROSS JOIN audit_logs a WHERE a.org_id IN ");
        qb.push(org::sql_viewer_orgs_with(Capability::AdminViewAudit));
        qb.push(" AND ");
        push_text_match(qb, None, AUDIT_TRGM, t, fuzzy);
    }
    let (rows, count, kind) = run_custom::<AuditRow>(
        state,
        scope,
        text,
        limit,
        Custom {
            select: "a.id, a.org_id, a.action, a.target, a.actor_name, a.created_at",
            body,
            order: "a.created_at DESC, a.id DESC",
            score: Some((None, AUDIT_TRGM)),
        },
    )
    .await?;
    let items = rows
        .into_iter()
        .map(|a| {
            let shown = format!("{} {} · {}", a.action, a.target, a.actor_name);
            GlobalHit {
                kind: "audit_events",
                id: a.id.to_string(),
                highlight: substring_segments(&shown, text),
                title: a.action,
                subtitle: format!("{} · {}", a.actor_name, a.target),
                matched_in: "action",
                score: a.score,
                target: json!({"org_id": a.org_id, "audit_event_id": a.id}),
                href: None,
                occurred_at: Some(a.created_at),
            }
        })
        .collect();
    Ok(GlobalGroup {
        kind: "audit_events",
        count,
        count_kind: kind,
        more_href: None,
        items,
    })
}
