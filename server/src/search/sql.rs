//! Tradução de uma [`ListQuery`] para Postgres (ADR-0007 §3–§5).
//!
//! Regras que este ficheiro não quebra:
//!
//! - **Nenhum texto do cliente entra no SQL.** Os nomes de campo chegam como
//!   `&'static str` da lista branca e são trocados pela expressão escrita no
//!   [`ResourceSql`]; os valores entram TODOS por `push_bind`. As únicas
//!   palavras «variáveis» no texto são as da granularidade e do período, que
//!   vêm de enums e escolhem entre literais escritos aqui.
//! - **A visibilidade é a do recurso e aplica-se a tudo:** itens, total e
//!   grupos (um `count` de outra org também é fuga).
//! - **Keyset, não OFFSET:** a página seguinte continua depois dos valores da
//!   última linha, com o `id` como desempate final.
//!
//! Cada consulta começa por `FROM (SELECT $1::uuid AS id, $2::uuid AS org_id,
//! $3::text AS tz) viewer, …`: as regras de visibilidade e os campos que
//! dependem de quem pede (`my_status`, «partilhada comigo») referem
//! `viewer.id`. O Postgres achata esta subconsulta sem FROM, por isso os
//! índices continuam a servir (confirmado com EXPLAIN no relatório).

use chrono::{DateTime, SecondsFormat, Utc};
use delonix_meet_core::query::{
    capped_total, encode_groups_cursor, encode_keyset, highlight_segments, Condition, FieldType,
    FilterValue, Granularity, HighlightSegment, IdKind, KeyValue, ListQuery, Node, Op, OrderTarget,
    Period, RowId, SearchSchema, TextQuery, TotalKind, HL_START, HL_STOP, MAX_GROUPS_PAGE,
};
use serde::Serialize;
use serde_json::{json, Map, Value as Json};
use sqlx::{postgres::PgRow, Postgres, QueryBuilder, Row};
use uuid::Uuid;

use crate::error::ApiError;

/// Limiar de `word_similarity` para erros de escrita (contrato §1).
pub const WORD_SIMILARITY_THRESHOLD: &str = "0.4";

/// Quem pede e onde: os três valores ligados à cabeça de cada consulta.
#[derive(Debug, Clone)]
pub struct Scope {
    pub me: Uuid,
    pub org_id: Option<Uuid>,
    pub tz: String,
}

/// Como um recurso se traduz para SQL. Tudo `&'static`: escrito no código.
pub struct ResourceSql {
    pub schema: &'static SearchSchema,
    /// ` CROSS JOIN tabela alias JOIN … WHERE <visibilidade>` — continua o
    /// `FROM (…) viewer`. CROSS JOIN e não vírgula: com vírgula, um `ON` não
    /// pode referir `viewer`.
    pub from: fn() -> String,
    /// Expressão do identificador (`r.id`).
    pub id: &'static str,
    /// Nome do campo → expressão SQL. Um teste exige uma por campo do schema.
    pub fields: &'static [(&'static str, &'static str)],
    /// Rótulo de um grupo (campo user/ref), em função da chave `g.k` (texto).
    pub group_labels: &'static [(&'static str, &'static str)],
    /// `tsvector` do recurso (com índice GIN), se houver.
    pub fts: Option<&'static str>,
    /// Expressões já dobradas (`dlx_fold(…)`) para trigramas; a primeira tem
    /// índice, as seguintes só servem conjuntos já pequenos (dentro da org).
    pub trigram: &'static [&'static str],
}

impl ResourceSql {
    pub fn expr(&self, field: &str) -> &'static str {
        self.fields
            .iter()
            .find(|(n, _)| *n == field)
            .map(|(_, e)| *e)
            // Inalcançável: o nome veio da lista branca e um teste garante
            // que cada campo do schema tem expressão.
            .unwrap_or("NULL")
    }

    fn group_label(&self, field: &str) -> Option<&'static str> {
        self.group_labels
            .iter()
            .find(|(n, _)| *n == field)
            .map(|(_, e)| *e)
    }
}

// ---------------------------------------------------------------------------
//  Blocos
// ---------------------------------------------------------------------------

fn push_head<'a>(qb: &mut QueryBuilder<'a, Postgres>, r: &ResourceSql, scope: &'a Scope) {
    qb.push(" FROM (SELECT ");
    qb.push_bind(scope.me);
    qb.push("::uuid AS id, ");
    qb.push_bind(scope.org_id);
    qb.push("::uuid AS org_id, ");
    qb.push_bind(scope.tz.as_str());
    qb.push("::text AS tz) viewer");
    qb.push((r.from)());
}

/// Escapa `\`, `%` e `_` para um `LIKE … ESCAPE '\'`. O resultado continua a
/// entrar por bind — isto só impede que `%` escrito por alguém seja curinga.
pub fn like_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if matches!(c, '\\' | '%' | '_') {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// Limites de um período, em hora LOCAL (`timestamp`), a converter com
/// `AT TIME ZONE viewer.tz`. `None` = compara com `now()` directamente.
fn period_bounds(p: Period) -> Option<(&'static str, &'static str)> {
    Some(match p {
        Period::Today => (
            "date_trunc('day', now() AT TIME ZONE viewer.tz)",
            "date_trunc('day', now() AT TIME ZONE viewer.tz) + interval '1 day'",
        ),
        Period::Yesterday => (
            "date_trunc('day', now() AT TIME ZONE viewer.tz) - interval '1 day'",
            "date_trunc('day', now() AT TIME ZONE viewer.tz)",
        ),
        Period::ThisWeek => (
            "date_trunc('week', now() AT TIME ZONE viewer.tz)",
            "date_trunc('week', now() AT TIME ZONE viewer.tz) + interval '1 week'",
        ),
        Period::LastWeek => (
            "date_trunc('week', now() AT TIME ZONE viewer.tz) - interval '1 week'",
            "date_trunc('week', now() AT TIME ZONE viewer.tz)",
        ),
        Period::ThisMonth => (
            "date_trunc('month', now() AT TIME ZONE viewer.tz)",
            "date_trunc('month', now() AT TIME ZONE viewer.tz) + interval '1 month'",
        ),
        Period::LastMonth => (
            "date_trunc('month', now() AT TIME ZONE viewer.tz) - interval '1 month'",
            "date_trunc('month', now() AT TIME ZONE viewer.tz)",
        ),
        Period::ThisQuarter => (
            "date_trunc('quarter', now() AT TIME ZONE viewer.tz)",
            "date_trunc('quarter', now() AT TIME ZONE viewer.tz) + interval '3 months'",
        ),
        Period::LastQuarter => (
            "date_trunc('quarter', now() AT TIME ZONE viewer.tz) - interval '3 months'",
            "date_trunc('quarter', now() AT TIME ZONE viewer.tz)",
        ),
        Period::ThisYear => (
            "date_trunc('year', now() AT TIME ZONE viewer.tz)",
            "date_trunc('year', now() AT TIME ZONE viewer.tz) + interval '1 year'",
        ),
        Period::LastYear => (
            "date_trunc('year', now() AT TIME ZONE viewer.tz) - interval '1 year'",
            "date_trunc('year', now() AT TIME ZONE viewer.tz)",
        ),
        Period::Last7Days => (
            "date_trunc('day', now() AT TIME ZONE viewer.tz) - interval '6 days'",
            "date_trunc('day', now() AT TIME ZONE viewer.tz) + interval '1 day'",
        ),
        Period::Last30Days => (
            "date_trunc('day', now() AT TIME ZONE viewer.tz) - interval '29 days'",
            "date_trunc('day', now() AT TIME ZONE viewer.tz) + interval '1 day'",
        ),
        Period::Next7Days => (
            "date_trunc('day', now() AT TIME ZONE viewer.tz)",
            "date_trunc('day', now() AT TIME ZONE viewer.tz) + interval '7 days'",
        ),
        Period::Past | Period::Future => return None,
    })
}

fn push_node<'a>(qb: &mut QueryBuilder<'a, Postgres>, r: &ResourceSql, node: &'a Node) {
    match node {
        Node::And(items) | Node::Or(items) => {
            let sep = if matches!(node, Node::And(_)) {
                " AND "
            } else {
                " OR "
            };
            qb.push("(");
            for (i, n) in items.iter().enumerate() {
                if i > 0 {
                    qb.push(sep);
                }
                push_node(qb, r, n);
            }
            qb.push(")");
        }
        Node::Not(inner) => {
            qb.push("(NOT ");
            push_node(qb, r, inner);
            qb.push(")");
        }
        Node::Cond(c) => push_condition(qb, r, c),
    }
}

fn push_condition<'a>(qb: &mut QueryBuilder<'a, Postgres>, r: &ResourceSql, c: &'a Condition) {
    let e = r.expr(c.field);
    let is_text = matches!(c.ty, FieldType::Text | FieldType::Enum);
    let num = c.ty == FieldType::Number;
    // Expressão comparada: números sempre como float8 (o valor vem como f64).
    let lhs = |qb: &mut QueryBuilder<'a, Postgres>| {
        qb.push("(");
        qb.push(e);
        qb.push(if num { ")::float8" } else { ")" });
    };
    qb.push("(");
    match (&c.op, &c.value) {
        (Op::IsSet, _) => {
            lhs(qb);
            qb.push(" IS NOT NULL");
            if is_text {
                qb.push(" AND (");
                qb.push(e);
                qb.push(") <> ''");
            }
        }
        (Op::IsNotSet, _) => {
            lhs(qb);
            qb.push(" IS NULL");
            if is_text {
                qb.push(" OR (");
                qb.push(e);
                qb.push(") = ''");
            }
        }
        (Op::Contains | Op::NotContains | Op::StartsWith, FilterValue::Text(v)) => {
            if c.op == Op::NotContains {
                qb.push("NOT ");
            }
            qb.push("dlx_fold(COALESCE(");
            qb.push(e);
            qb.push(", '')) LIKE ");
            if c.op != Op::StartsWith {
                qb.push("'%' || ");
            }
            qb.push("dlx_fold(");
            qb.push_bind(like_escape(v));
            qb.push(") || '%' ESCAPE '\\'");
        }
        (Op::In | Op::NotIn, FilterValue::TextList(vs)) => {
            if c.op == Op::NotIn {
                lhs(qb);
                qb.push(" IS NULL OR NOT ");
            }
            lhs(qb);
            qb.push(" = ANY(");
            qb.push_bind(vs.as_slice());
            qb.push(")");
        }
        (Op::In | Op::NotIn, FilterValue::UuidList(vs)) => {
            if c.op == Op::NotIn {
                lhs(qb);
                qb.push(" IS NULL OR NOT ");
            }
            lhs(qb);
            qb.push(" = ANY(");
            qb.push_bind(vs.as_slice());
            qb.push(")");
        }
        (Op::InPeriod, FilterValue::Period(p)) => match period_bounds(*p) {
            Some((lo, hi)) => {
                lhs(qb);
                qb.push(" >= (");
                qb.push(lo);
                qb.push(") AT TIME ZONE viewer.tz AND ");
                lhs(qb);
                qb.push(" < (");
                qb.push(hi);
                qb.push(") AT TIME ZONE viewer.tz");
            }
            None => {
                lhs(qb);
                qb.push(if *p == Period::Past {
                    " < now()"
                } else {
                    " >= now()"
                });
            }
        },
        (Op::Between, FilterValue::NumberRange(a, b)) => {
            lhs(qb);
            qb.push(" BETWEEN ");
            qb.push_bind(*a);
            qb.push(" AND ");
            qb.push_bind(*b);
        }
        (Op::Between, FilterValue::TimeRange(a, b)) => {
            lhs(qb);
            qb.push(" BETWEEN ");
            qb.push_bind(*a);
            qb.push(" AND ");
            qb.push_bind(*b);
        }
        (op, value) => {
            lhs(qb);
            qb.push(match op {
                Op::Eq => " = ",
                Op::Ne => " IS DISTINCT FROM ",
                Op::Lt => " < ",
                Op::Lte => " <= ",
                Op::Gt => " > ",
                Op::Gte => " >= ",
                // A validação do core não deixa chegar outra combinação.
                _ => " = ",
            });
            match value {
                FilterValue::Text(v) => qb.push_bind(v.as_str()),
                FilterValue::Number(n) => qb.push_bind(*n),
                FilterValue::Time(t) => qb.push_bind(*t),
                FilterValue::Bool(b) => qb.push_bind(*b),
                FilterValue::Uuid(u) => qb.push_bind(*u),
                _ => qb.push("NULL"),
            };
        }
    }
    qb.push(")");
}

/// A condição do `q`: `tsvector` com prefixos OU trigramas (subcadeia por
/// termo, e semelhança para erros de escrita).
fn push_text<'a>(
    qb: &mut QueryBuilder<'a, Postgres>,
    r: &ResourceSql,
    t: &'a TextQuery,
    fuzzy: bool,
) {
    push_text_match(qb, r.fts, r.trigram, t, fuzzy);
}

/// O mesmo, para expressões soltas (pesquisa global).
pub fn push_text_match<'a>(
    qb: &mut QueryBuilder<'a, Postgres>,
    fts: Option<&'static str>,
    trigram: &'static [&'static str],
    t: &'a TextQuery,
    fuzzy: bool,
) {
    let mut first = true;
    qb.push("(");
    if let Some(fts) = fts {
        qb.push(fts);
        qb.push(" @@ to_tsquery('dlx_search', ");
        qb.push_bind(t.tsquery.as_str());
        qb.push(")");
        first = false;
    }
    // Subcadeia por termo: só quando o índice de trigramas serve (≥ 3
    // caracteres), quando não há tsvector, ou para escritas sem espaços
    // (chinês), onde o tsvector só acerta pelo início do bloco.
    let cjk = t.terms.iter().any(|w| w.chars().any(is_cjk));
    let long = t.terms.iter().all(|w| w.chars().count() >= 3);
    if !trigram.is_empty() && (fts.is_none() || long || cjk) {
        for tri in trigram {
            if !first {
                qb.push(" OR ");
            }
            first = false;
            qb.push("(");
            for (i, term) in t.terms.iter().enumerate() {
                if i > 0 {
                    qb.push(" AND ");
                }
                qb.push(*tri);
                qb.push(" LIKE '%' || dlx_fold(");
                qb.push_bind(term.as_str());
                qb.push(") || '%'");
            }
            qb.push(")");
        }
        if fuzzy && t.raw.chars().count() >= 3 {
            qb.push(" OR dlx_fold(");
            qb.push_bind(t.raw.as_str());
            qb.push(") <% ");
            qb.push(trigram[0]);
        }
    }
    if first {
        qb.push("false");
    }
    qb.push(")");
}

pub fn is_cjk(c: char) -> bool {
    matches!(c as u32, 0x3040..=0x30FF | 0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xAC00..=0xD7AF | 0xF900..=0xFAFF)
}

fn push_score<'a>(qb: &mut QueryBuilder<'a, Postgres>, r: &ResourceSql, t: &'a TextQuery) {
    push_score_expr(qb, r.fts, r.trigram, t);
}

/// Relevância: `ts_rank_cd` (pesos A–C) + `word_similarity` do trigrama.
pub fn push_score_expr<'a>(
    qb: &mut QueryBuilder<'a, Postgres>,
    fts: Option<&'static str>,
    trigram: &'static [&'static str],
    t: &'a TextQuery,
) {
    qb.push("(0");
    if let Some(fts) = fts {
        qb.push(" + COALESCE(ts_rank_cd(");
        qb.push(fts);
        qb.push(", to_tsquery('dlx_search', ");
        qb.push_bind(t.tsquery.as_str());
        qb.push(")), 0)");
    }
    if let Some(tri) = trigram.first() {
        qb.push(" + COALESCE(word_similarity(dlx_fold(");
        qb.push_bind(t.raw.as_str());
        qb.push("), ");
        qb.push(*tri);
        qb.push("), 0)");
    }
    qb.push(")::float8");
}

/// Expressão de ordenação (não-nula).
fn push_order_expr<'a>(
    qb: &mut QueryBuilder<'a, Postgres>,
    r: &ResourceSql,
    target: OrderTarget,
    text: Option<&'a TextQuery>,
) {
    match target {
        OrderTarget::Score => match text {
            Some(t) => push_score(qb, r, t),
            None => {
                qb.push("0::float8");
            }
        },
        OrderTarget::Field(f) => {
            let e = r.expr(f.name);
            match f.ty {
                FieldType::Number => {
                    qb.push("COALESCE((");
                    qb.push(e);
                    qb.push(")::float8, -1)");
                }
                // Os campos de data ordenáveis são NOT NULL (teste
                // `sortable_datetimes_are_not_null`).
                FieldType::Datetime => {
                    qb.push("(");
                    qb.push(e);
                    qb.push(")");
                }
                _ => {
                    qb.push("COALESCE((");
                    qb.push(e);
                    qb.push(")::text, '')");
                }
            }
        }
    }
}

/// WHERE comum: visibilidade (no FROM) + filtro + texto.
fn push_where<'a>(qb: &mut QueryBuilder<'a, Postgres>, r: &ResourceSql, q: &'a ListQuery) {
    if let Some(f) = &q.filter {
        qb.push(" AND ");
        push_node(qb, r, f);
    }
    if let Some(t) = &q.text {
        qb.push(" AND ");
        push_text(qb, r, t, q.fuzzy);
    }
}

/// Valor de cursor decodificado para bind.
enum CursorBind {
    F(f64),
    T(DateTime<Utc>),
    S(String),
}

fn push_keyset<'a>(qb: &mut QueryBuilder<'a, Postgres>, r: &ResourceSql, q: &'a ListQuery) {
    let Some(c) = &q.cursor else { return };
    let binds: Vec<CursorBind> = q
        .order
        .iter()
        .zip(&c.k)
        .map(|(key, v)| match (key.target, v) {
            (OrderTarget::Score, KeyValue::Number(n)) => CursorBind::F(*n),
            (OrderTarget::Field(f), KeyValue::Number(n)) if f.ty == FieldType::Number => {
                CursorBind::F(*n)
            }
            (OrderTarget::Field(f), KeyValue::Text(s)) if f.ty == FieldType::Datetime => {
                CursorBind::T(
                    DateTime::parse_from_rfc3339(s)
                        .map(|t| t.with_timezone(&Utc))
                        .unwrap_or_default(),
                )
            }
            (_, KeyValue::Text(s)) => CursorBind::S(s.clone()),
            (_, KeyValue::Number(n)) => CursorBind::F(*n),
        })
        .collect();
    let push_val = |qb: &mut QueryBuilder<'a, Postgres>, b: &CursorBind| match b {
        CursorBind::F(n) => {
            qb.push_bind(*n);
        }
        CursorBind::T(t) => {
            qb.push_bind(*t);
        }
        CursorBind::S(s) => {
            qb.push_bind(s.clone());
        }
    };
    let text = q.text.as_ref();
    let n = q.order.len();
    qb.push(" AND (");
    // (k0 ▷ v0) OR (k0 = v0 AND k1 ▷ v1) OR … OR (k0 = v0 AND … AND id ▷ vid)
    for level in 0..=n {
        if level > 0 {
            qb.push(" OR ");
        }
        qb.push("(");
        for (key, bind) in q.order.iter().zip(&binds).take(level) {
            push_order_expr(qb, r, key.target, text);
            qb.push(" = ");
            push_val(qb, bind);
            qb.push(" AND ");
        }
        if level < n {
            push_order_expr(qb, r, q.order[level].target, text);
            qb.push(if q.order[level].desc { " < " } else { " > " });
            push_val(qb, &binds[level]);
        } else {
            let desc = q.order.last().map(|k| k.desc).unwrap_or(false);
            qb.push(r.id);
            qb.push(if desc { " < " } else { " > " });
            match &c.id {
                RowId::Uuid(u) => {
                    qb.push_bind(*u);
                }
                RowId::Int(i) => {
                    qb.push_bind(*i);
                }
            }
        }
        qb.push(")");
    }
    qb.push(")");
}

// ---------------------------------------------------------------------------
//  Execução
// ---------------------------------------------------------------------------

/// Uma linha da página: id, chaves e, com `q`, a relevância.
#[derive(Debug, Clone)]
pub struct Hit {
    pub id: RowId,
    pub score: Option<f64>,
}

#[derive(Debug, Serialize)]
pub struct GroupOut {
    pub key: Json,
    pub label: Json,
    pub count: i64,
    pub aggregates: Map<String, Json>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub range: Option<Json>,
    pub filter: Json,
    pub group_by: Vec<String>,
}

#[derive(Debug)]
pub struct ListOutcome {
    pub hits: Vec<Hit>,
    pub next_page_token: Option<String>,
    pub total: i64,
    pub total_kind: TotalKind,
    pub groups: Option<Vec<GroupOut>>,
    pub next_groups_page_token: Option<String>,
    /// Com `q`: a página veio da pesquisa aproximada?
    pub fuzzy: bool,
}

/// SET LOCAL do limiar de trigramas — só dentro de uma transacção.
async fn begin(db: &sqlx::PgPool) -> Result<sqlx::Transaction<'static, Postgres>, ApiError> {
    let mut tx = db.begin().await?;
    sqlx::query(&format!(
        "SET LOCAL pg_trgm.word_similarity_threshold = {WORD_SIMILARITY_THRESHOLD}"
    ))
    .execute(&mut *tx)
    .await?;
    Ok(tx)
}

/// O SQL da página (exposto para os testes de injecção e para o EXPLAIN).
pub fn page_sql<'a>(
    r: &ResourceSql,
    q: &'a ListQuery,
    scope: &'a Scope,
) -> QueryBuilder<'a, Postgres> {
    let mut qb = QueryBuilder::new("SELECT ");
    qb.push(r.id);
    qb.push(" AS __id");
    for (i, key) in q.order.iter().enumerate() {
        qb.push(", ");
        push_order_expr(&mut qb, r, key.target, q.text.as_ref());
        qb.push(format_args!(" AS __k{i}"));
    }
    push_head(&mut qb, r, scope);
    push_where(&mut qb, r, q);
    push_keyset(&mut qb, r, q);
    qb.push(" ORDER BY ");
    for (i, key) in q.order.iter().enumerate() {
        qb.push(format_args!("__k{i}"));
        qb.push(if key.desc { " DESC, " } else { " ASC, " });
    }
    let desc = q.order.last().map(|k| k.desc).unwrap_or(false);
    qb.push("__id");
    qb.push(if desc { " DESC" } else { " ASC" });
    qb.push(" LIMIT ");
    qb.push_bind(q.page_size as i64 + 1);
    qb
}

pub fn count_sql<'a>(
    r: &ResourceSql,
    q: &'a ListQuery,
    scope: &'a Scope,
) -> QueryBuilder<'a, Postgres> {
    let mut qb = QueryBuilder::new("SELECT count(*) FROM (SELECT 1");
    push_head(&mut qb, r, scope);
    push_where(&mut qb, r, q);
    qb.push(" LIMIT ");
    qb.push_bind(q.total_cap + 1);
    qb.push(") c");
    qb
}

fn granularity_sql(g: Granularity) -> (&'static str, &'static str, &'static str) {
    match g {
        Granularity::Day => ("day", "YYYY-MM-DD", "1 day"),
        Granularity::Week => ("week", "IYYY-\"W\"IW", "1 week"),
        Granularity::Month => ("month", "YYYY-MM", "1 month"),
        Granularity::Quarter => ("quarter", "YYYY-\"Q\"Q", "3 months"),
        Granularity::Year => ("year", "YYYY", "1 year"),
    }
}

pub fn groups_sql<'a>(
    r: &ResourceSql,
    q: &'a ListQuery,
    scope: &'a Scope,
) -> QueryBuilder<'a, Postgres> {
    let g = q.group_by[0];
    let e = r.expr(g.field.name);
    let mut qb = QueryBuilder::new("SELECT x.* FROM (SELECT g.*, ");
    match r.group_label(g.field.name) {
        Some(label) => {
            qb.push("COALESCE(");
            qb.push(label);
            qb.push(", g.k)");
        }
        None => {
            qb.push("g.k");
        }
    }
    qb.push(" AS lbl FROM (SELECT ");
    match g.granularity {
        Some(gran) => {
            let (unit, fmt, step) = granularity_sql(gran);
            let local = format!("date_trunc('{unit}', ({e}) AT TIME ZONE viewer.tz)");
            qb.push(format_args!(
                "to_char({local}, '{fmt}') AS k, \
                 min(({local}) AT TIME ZONE viewer.tz) AS lo, \
                 min(({local} + interval '{step}') AT TIME ZONE viewer.tz) AS hi"
            ));
        }
        None => {
            qb.push("(");
            qb.push(e);
            qb.push(")::text AS k, NULL::timestamptz AS lo, NULL::timestamptz AS hi");
        }
    }
    qb.push(", count(*) AS n");
    for (i, f) in q.schema.fields.iter().enumerate() {
        for a in f.aggregates {
            qb.push(format_args!(", {}((", a.as_str()));
            qb.push(r.expr(f.name));
            qb.push(format_args!(")::float8) AS a{i}_{}", a.as_str()));
        }
    }
    push_head(&mut qb, r, scope);
    push_where(&mut qb, r, q);
    qb.push(" GROUP BY 1) g) x");
    if let Some(c) = &q.groups_cursor {
        qb.push(" WHERE (x.lbl > ");
        qb.push_bind(c.s.as_str());
        qb.push(" OR (x.lbl = ");
        qb.push_bind(c.s.as_str());
        qb.push(" AND x.k > ");
        qb.push_bind(c.k.as_str());
        qb.push(") OR x.k IS NULL)");
    }
    qb.push(" ORDER BY x.lbl ASC NULLS LAST, x.k ASC NULLS LAST LIMIT ");
    qb.push_bind(MAX_GROUPS_PAGE as i64 + 1);
    qb
}

fn rfc3339(t: DateTime<Utc>) -> String {
    t.to_rfc3339_opts(SecondsFormat::AutoSi, true)
}

/// Pode a pesquisa aproximada ajudar? (Só com `q` de ≥ 3 caracteres e onde
/// há trigramas.)
pub fn fuzzy_possible(r: &ResourceSql, q: &ListQuery) -> bool {
    !r.trigram.is_empty() && q.text.as_ref().is_some_and(|t| t.raw.chars().count() >= 3)
}

/// Corre a lista. Com `q`, a pesquisa exacta (prefixos e subcadeias) vem
/// primeiro; só se não encontrar nada é que entram os erros de escrita — senão
/// «orcamento» traria «planeamento» ao lado de «Orçamento».
pub async fn run_list(
    db: &sqlx::PgPool,
    r: &ResourceSql,
    q: &ListQuery,
    scope: &Scope,
) -> Result<ListOutcome, ApiError> {
    let out = run_list_once(db, r, q, scope).await?;
    if out.hits.is_empty() && q.cursor.is_none() && !q.fuzzy && fuzzy_possible(r, q) {
        let mut fz = q.clone();
        fz.fuzzy = true;
        return run_list_once(db, r, &fz, scope).await;
    }
    Ok(out)
}

async fn run_list_once(
    db: &sqlx::PgPool,
    r: &ResourceSql,
    q: &ListQuery,
    scope: &Scope,
) -> Result<ListOutcome, ApiError> {
    let mut tx = begin(db).await?;

    let rows: Vec<PgRow> = page_sql(r, q, scope).build().fetch_all(&mut *tx).await?;
    let size = q.page_size as usize;
    let more = rows.len() > size;
    let rows = &rows[..rows.len().min(size)];
    let mut hits = Vec::with_capacity(rows.len());
    for row in rows {
        let id = match q.schema.id_kind {
            IdKind::Uuid => RowId::Uuid(row.try_get::<Uuid, _>("__id")?),
            IdKind::Int => RowId::Int(row.try_get::<i64, _>("__id")?),
        };
        let score = match q.order.iter().position(|k| k.target == OrderTarget::Score) {
            Some(i) => Some(row.try_get::<f64, _>(format!("__k{i}").as_str())?),
            None => None,
        };
        hits.push(Hit { id, score });
    }
    let next_page_token = if more {
        let last = rows.last().expect("more ⇒ há linhas");
        let mut keys = Vec::with_capacity(q.order.len());
        for (i, key) in q.order.iter().enumerate() {
            let col = format!("__k{i}");
            keys.push(match key.target {
                OrderTarget::Score => KeyValue::Number(last.try_get::<f64, _>(col.as_str())?),
                OrderTarget::Field(f) => match f.ty {
                    FieldType::Number => KeyValue::Number(last.try_get::<f64, _>(col.as_str())?),
                    FieldType::Datetime => {
                        KeyValue::Text(rfc3339(last.try_get::<DateTime<Utc>, _>(col.as_str())?))
                    }
                    _ => KeyValue::Text(last.try_get::<String, _>(col.as_str())?),
                },
            });
        }
        let id = hits.last().expect("há linhas").id.clone();
        Some(encode_keyset(&q.fingerprint, keys, id, q.fuzzy))
    } else {
        None
    };

    // Total: numa página só e sem cursor, as linhas contam-se sozinhas.
    let (total, total_kind) = if q.cursor.is_none() && !more {
        (hits.len() as i64, TotalKind::Exact)
    } else {
        let counted: i64 = count_sql(r, q, scope)
            .build_query_scalar()
            .fetch_one(&mut *tx)
            .await?;
        capped_total(counted, q.total_cap)
    };

    let (groups, next_groups_page_token) = if q.group_by.is_empty() {
        (None, None)
    } else {
        let g = q.group_by[0];
        let rows: Vec<PgRow> = groups_sql(r, q, scope).build().fetch_all(&mut *tx).await?;
        let more = rows.len() > MAX_GROUPS_PAGE as usize;
        let rows = &rows[..rows.len().min(MAX_GROUPS_PAGE as usize)];
        let mut out = Vec::with_capacity(rows.len());
        for row in rows {
            let k: Option<String> = row.try_get("k")?;
            let lbl: Option<String> = row.try_get("lbl")?;
            let n: i64 = row.try_get("n")?;
            let mut aggregates = Map::new();
            for (i, f) in q.schema.fields.iter().enumerate() {
                if f.aggregates.is_empty() {
                    continue;
                }
                let mut m = Map::new();
                for a in f.aggregates {
                    let v: Option<f64> = row.try_get(format!("a{i}_{}", a.as_str()).as_str())?;
                    m.insert(a.as_str().into(), json!(v));
                }
                aggregates.insert(f.name.into(), Json::Object(m));
            }
            let name = g.field.name;
            let (key, label, range, filter) = match (&k, g.granularity) {
                (None, _) => (Json::Null, Json::Null, None, json!([name, "is_not_set"])),
                (Some(k), Some(_)) => {
                    let lo: DateTime<Utc> = row.try_get("lo")?;
                    let hi: DateTime<Utc> = row.try_get("hi")?;
                    (
                        json!(k),
                        json!(k),
                        Some(json!({"from": rfc3339(lo), "to": rfc3339(hi)})),
                        json!({"and": [[name, "gte", rfc3339(lo)], [name, "lt", rfc3339(hi)]]}),
                    )
                }
                (Some(k), None) => match g.field.ty {
                    FieldType::Bool => {
                        let b = k == "true";
                        (json!(b), json!(b), None, json!([name, "eq", b]))
                    }
                    FieldType::Enum => {
                        let label = g
                            .field
                            .options
                            .iter()
                            .find(|o| o.value == k)
                            .map(|o| o.label)
                            .unwrap_or(k.as_str());
                        (json!(k), json!(label), None, json!([name, "eq", k]))
                    }
                    _ => (
                        json!(k),
                        json!(lbl.as_deref().unwrap_or(k)),
                        None,
                        json!([name, "eq", k]),
                    ),
                },
            };
            out.push(GroupOut {
                key,
                label,
                count: n,
                aggregates,
                range,
                filter,
                group_by: q.remaining_group_by(),
            });
        }
        let token = if more {
            let last = rows.last().expect("more ⇒ há grupos");
            let k: Option<String> = last.try_get("k")?;
            let lbl: Option<String> = last.try_get("lbl")?;
            // O grupo sem valor é sempre o último: depois dele não há mais.
            match (lbl, k) {
                (Some(s), Some(k)) => Some(encode_groups_cursor(&q.fingerprint, s, k)),
                _ => None,
            }
        } else {
            None
        };
        (Some(out), token)
    };

    tx.commit().await?;
    Ok(ListOutcome {
        hits,
        next_page_token,
        total,
        total_kind,
        groups,
        next_groups_page_token,
        fuzzy: q.fuzzy,
    })
}

/// Ids de uma página, com o tipo certo para o bind.
pub enum Ids<'a> {
    Uuid(&'a [Uuid]),
    Int(&'a [i64]),
}

/// `ts_headline` com as marcas internas, já partido em segmentos. `source` é
/// um SELECT escrito no código com as colunas `id` e `txt`.
pub async fn headlines(
    db: &sqlx::PgPool,
    source: &'static str,
    ids: Ids<'_>,
    t: &TextQuery,
) -> Result<std::collections::HashMap<String, Vec<HighlightSegment>>, ApiError> {
    let mut qb = QueryBuilder::<Postgres>::new(
        "SELECT s.id::text, ts_headline('dlx_search', s.txt, to_tsquery('dlx_search', ",
    );
    qb.push_bind(t.tsquery.as_str());
    qb.push(format_args!(
        "), 'MaxFragments=1, MaxWords=18, MinWords=6, StartSel={HL_START}, StopSel={HL_STOP}') FROM ("
    ));
    qb.push(source);
    qb.push(") s WHERE s.id = ANY(");
    match ids {
        Ids::Uuid(v) if !v.is_empty() => {
            qb.push_bind(v.to_vec());
        }
        Ids::Int(v) if !v.is_empty() => {
            qb.push_bind(v.to_vec());
        }
        _ => return Ok(Default::default()),
    }
    qb.push(")");
    let rows: Vec<(String, String)> = qb.build_query_as().fetch_all(db).await?;
    Ok(rows
        .into_iter()
        .map(|(id, marked)| (id, highlight_segments(&marked)))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::search::{resources, SearchParams};

    fn scope() -> Scope {
        Scope {
            me: Uuid::from_u128(1),
            org_id: None,
            tz: "Africa/Luanda".into(),
        }
    }

    fn sql_of(r: &ResourceSql, p: SearchParams) -> String {
        let q = crate::search::compile_for(r, &p, Uuid::from_u128(1)).unwrap();
        let s = scope();
        let mut out = page_sql(r, &q, &s).sql().to_string();
        out.push_str(count_sql(r, &q, &s).sql());
        if !q.group_by.is_empty() {
            out.push_str(groups_sql(r, &q, &s).sql());
        }
        out
    }

    /// O texto hostil entra como VALOR ligado: nunca aparece no SQL.
    #[test]
    fn hostile_values_never_reach_the_sql_text() {
        let evil = "x'); DROP TABLE users; --";
        let filter = serde_json::json!([
            ["title", "contains", evil],
            ["room_code", "in", [evil, "b"]],
            ["filename", "starts_with", "%_\\"]
        ])
        .to_string();
        let sql = sql_of(
            &resources::RECORDINGS,
            SearchParams {
                q: Some(format!("{evil} orçamento")),
                filter: Some(filter),
                group_by: Some("uploader".into()),
                ..Default::default()
            },
        );
        assert!(!sql.contains("DROP"), "{sql}");
        assert!(!sql.contains("orçamento"), "{sql}");
        assert!(sql.contains("$1"), "{sql}");
    }

    #[test]
    fn like_escape_neutralises_wildcards() {
        assert_eq!(like_escape("50%_a\\b"), "50\\%\\_a\\\\b");
    }

    /// Cada combinação de campo × operador × recurso gera SQL (o Postgres a
    /// sério valida-o nos testes de integração).
    #[test]
    fn every_field_operator_combination_builds() {
        use delonix_meet_core::query::FieldType;
        for r in resources::all() {
            for f in r.schema.fields.iter().filter(|f| f.filterable) {
                for op in f.ty.operators() {
                    let v = match (f.ty, op.as_str()) {
                        (_, "is_set" | "is_not_set") => serde_json::Value::Null,
                        (FieldType::Enum, "in" | "not_in") => {
                            serde_json::json!([f.options[0].value])
                        }
                        (FieldType::Enum, _) => serde_json::json!(f.options[0].value),
                        (FieldType::Text, "in" | "not_in") => serde_json::json!(["a"]),
                        (FieldType::Text, _) => serde_json::json!("a"),
                        (FieldType::Number, "between") => serde_json::json!([1, 2]),
                        (FieldType::Number, _) => serde_json::json!(1),
                        (FieldType::Datetime, "between") => {
                            serde_json::json!(["2026-01-01T00:00:00Z", "2026-02-01T00:00:00Z"])
                        }
                        (FieldType::Datetime, "in_period") => serde_json::json!("this_week"),
                        (FieldType::Datetime, _) => serde_json::json!("2026-01-01T00:00:00Z"),
                        (FieldType::Bool, _) => serde_json::json!(true),
                        (FieldType::User, "in" | "not_in") => serde_json::json!(["me"]),
                        (_, "in" | "not_in") => serde_json::json!([Uuid::nil()]),
                        _ => serde_json::json!(Uuid::nil()),
                    };
                    let cond = if v.is_null() {
                        serde_json::json!([[f.name, op.as_str()]])
                    } else {
                        serde_json::json!([[f.name, op.as_str(), v]])
                    };
                    let sql = sql_of(
                        r,
                        SearchParams {
                            filter: Some(cond.to_string()),
                            ..Default::default()
                        },
                    );
                    assert!(sql.contains("viewer"), "{}.{}", r.schema.resource, f.name);
                }
            }
        }
    }
}
