//! Pesquisa de lista estilo Odoo (ADR-0007): o domínio de filtro, a ordenação,
//! o agrupamento e o cursor keyset — a parte PURA, sem SQL.
//!
//! O que entra do cliente (`q`, `filter`, `filters`, `group_by`, `order_by`,
//! `page_token`) sai daqui como uma [`ListQuery`] tipada em que **cada nome de
//! campo é um `&'static str` da lista branca** ([`SearchSchema`]). A tradução
//! para SQL (no adaptador de Postgres) só conhece esses nomes e liga todos os
//! valores por *bind*: não há caminho para um nome ou um valor do cliente
//! chegar ao texto da consulta.
//!
//! O cursor é o de [`crate::page`] (token opaco, `page_size` 1..100), com os
//! valores das chaves de ordenação, o id de desempate e uma impressão digital
//! da pesquisa — um token de outra pesquisa é `400`, nunca uma página errada.

pub mod schema;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value as Json;
use uuid::Uuid;

pub use schema::{
    Aggregate, EnumOption, FieldSpec, FieldType, Granularity, IdKind, NamedFilter, Op, Period,
    SchemaView, SearchSchema,
};

use crate::error::DomainError;
use crate::page::{decode_cursor, encode_cursor, PageRequest};

/// Profundidade máxima da árvore de filtro.
pub const MAX_DEPTH: usize = 4;
/// Condições (folhas) numa árvore.
pub const MAX_CONDITIONS: usize = 20;
/// Valores num `in`/`not_in`.
pub const MAX_IN_VALUES: usize = 100;
/// Tamanho do `filter` em bytes.
pub const MAX_FILTER_BYTES: usize = 4096;
/// Campos em `group_by` e em `order_by`.
pub const MAX_GROUP_BY: usize = 3;
pub const MAX_ORDER_BY: usize = 3;
/// `q` em caracteres.
pub const MAX_Q_CHARS: usize = 200;
/// Termos considerados de um `q`.
pub const MAX_TERMS: usize = 8;
const MAX_TERM_CHARS: usize = 64;
/// Tamanho de um valor de texto num filtro.
const MAX_TEXT_VALUE: usize = 200;
/// Grupos por página.
pub const MAX_GROUPS_PAGE: u32 = 100;

// ---------------------------------------------------------------------------
//  Erros (códigos estáveis — docs/reference/pesquisa.md §2.4)
// ---------------------------------------------------------------------------

fn err(code: &'static str, field: impl Into<String>, msg: impl Into<String>) -> DomainError {
    let msg = msg.into();
    DomainError::invalid(code, msg.clone()).with_field(field, msg)
}

// ---------------------------------------------------------------------------
//  Parâmetros de pedido
// ---------------------------------------------------------------------------

/// Os parâmetros uniformes de uma colecção pesquisável, tal como chegam.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct ListParams {
    pub q: Option<String>,
    pub filter: Option<String>,
    pub filters: Option<String>,
    pub group_by: Option<String>,
    pub order_by: Option<String>,
    pub page_size: Option<u32>,
    pub page_token: Option<String>,
    pub groups_page_token: Option<String>,
}

impl ListParams {
    /// Algum parâmetro de pesquisa presente? Sem nenhum, a colecção herdada
    /// responde com a forma antiga.
    pub fn is_search(&self) -> bool {
        self.q.is_some()
            || self.filter.is_some()
            || self.filters.is_some()
            || self.group_by.is_some()
            || self.order_by.is_some()
            || self.page_size.is_some()
            || self.page_token.is_some()
            || self.groups_page_token.is_some()
    }
}

/// Contexto de validação: quem pede (para `"me"`).
#[derive(Debug, Clone, Copy)]
pub struct Ctx {
    pub me: Uuid,
}

// ---------------------------------------------------------------------------
//  Domínio de filtro
// ---------------------------------------------------------------------------

/// Valor tipado de uma condição, já validado contra o tipo do campo.
#[derive(Debug, Clone, PartialEq)]
pub enum FilterValue {
    None,
    Text(String),
    TextList(Vec<String>),
    Number(f64),
    NumberRange(f64, f64),
    Time(DateTime<Utc>),
    TimeRange(DateTime<Utc>, DateTime<Utc>),
    Period(Period),
    Bool(bool),
    Uuid(Uuid),
    UuidList(Vec<Uuid>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Condition {
    /// Nome da lista branca — `&'static`, nunca texto do cliente.
    pub field: &'static str,
    pub ty: FieldType,
    pub op: Op,
    pub value: FilterValue,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Node {
    And(Vec<Node>),
    Or(Vec<Node>),
    Not(Box<Node>),
    Cond(Condition),
}

struct Parser<'a> {
    schema: &'a SearchSchema,
    ctx: Ctx,
    conditions: usize,
}

/// Valida um domínio em JSON contra a lista branca.
pub fn parse_filter(schema: &SearchSchema, raw: &str, ctx: Ctx) -> Result<Node, DomainError> {
    if raw.len() > MAX_FILTER_BYTES {
        return Err(err(
            "search.filter_too_complex",
            "filter",
            format!("o filtro tem mais de {MAX_FILTER_BYTES} bytes"),
        ));
    }
    let json: Json = serde_json::from_str(raw).map_err(|_| {
        err(
            "search.invalid_filter",
            "filter",
            "o filtro não é JSON válido",
        )
    })?;
    parse_filter_value(schema, &json, ctx)
}

/// Como [`parse_filter`], a partir de JSON já lido (favoritos, grupos).
pub fn parse_filter_value(
    schema: &SearchSchema,
    json: &Json,
    ctx: Ctx,
) -> Result<Node, DomainError> {
    let mut p = Parser {
        schema,
        ctx,
        conditions: 0,
    };
    p.top(json)
}

impl Parser<'_> {
    fn top(&mut self, json: &Json) -> Result<Node, DomainError> {
        // Lista no topo = E, a não ser que seja ela própria uma condição.
        if let Json::Array(items) = json {
            if !is_condition(items) {
                let mut nodes = Vec::with_capacity(items.len());
                for (i, item) in items.iter().enumerate() {
                    nodes.push(self.node(item, 1, &format!("filter[{i}]"))?);
                }
                return Ok(Node::And(nodes));
            }
        }
        self.node(json, 0, "filter")
    }

    fn node(&mut self, json: &Json, depth: usize, path: &str) -> Result<Node, DomainError> {
        if depth > MAX_DEPTH {
            return Err(err(
                "search.filter_too_complex",
                path,
                format!("o filtro tem mais de {MAX_DEPTH} níveis"),
            ));
        }
        match json {
            Json::Array(items) if is_condition(items) => self.condition(items, path),
            Json::Object(map) if map.len() == 1 => {
                let (k, v) = map.iter().next().expect("len 1");
                match k.as_str() {
                    "and" | "or" => {
                        let Json::Array(items) = v else {
                            return Err(err(
                                "search.invalid_filter",
                                format!("{path}.{k}"),
                                "«and»/«or» levam uma lista",
                            ));
                        };
                        if items.is_empty() {
                            return Err(err(
                                "search.invalid_filter",
                                format!("{path}.{k}"),
                                "«and»/«or» vazio",
                            ));
                        }
                        let mut nodes = Vec::with_capacity(items.len());
                        for (i, item) in items.iter().enumerate() {
                            nodes.push(self.node(item, depth + 1, &format!("{path}.{k}[{i}]"))?);
                        }
                        Ok(if k == "and" {
                            Node::And(nodes)
                        } else {
                            Node::Or(nodes)
                        })
                    }
                    "not" => Ok(Node::Not(Box::new(self.node(
                        v,
                        depth + 1,
                        &format!("{path}.not"),
                    )?))),
                    _ => Err(err(
                        "search.invalid_filter",
                        path,
                        "nó desconhecido (esperado and, or, not ou [campo, operador, valor])",
                    )),
                }
            }
            _ => Err(err(
                "search.invalid_filter",
                path,
                "nó inválido (esperado and, or, not ou [campo, operador, valor])",
            )),
        }
    }

    fn condition(&mut self, items: &[Json], path: &str) -> Result<Node, DomainError> {
        self.conditions += 1;
        if self.conditions > MAX_CONDITIONS {
            return Err(err(
                "search.filter_too_complex",
                "filter",
                format!("o filtro tem mais de {MAX_CONDITIONS} condições"),
            ));
        }
        let name = items[0].as_str().unwrap_or_default();
        let op_name = items[1].as_str().unwrap_or_default();
        let field = self.schema.field(name).ok_or_else(|| {
            err(
                "search.unknown_field",
                path,
                format!("o campo não existe em {}", self.schema.resource),
            )
        })?;
        if !field.filterable {
            return Err(err(
                "search.field_not_filterable",
                path,
                "este campo não se pode filtrar",
            ));
        }
        let op = Op::parse(op_name)
            .filter(|op| field.ty.operators().contains(op))
            .ok_or_else(|| {
                err(
                    "search.invalid_operator",
                    path,
                    "operador inexistente ou que não serve para este tipo de campo",
                )
            })?;
        let value = self.value(field, op, items.get(2), path)?;
        Ok(Node::Cond(Condition {
            field: field.name,
            ty: field.ty,
            op,
            value,
        }))
    }

    fn value(
        &self,
        field: &FieldSpec,
        op: Op,
        raw: Option<&Json>,
        path: &str,
    ) -> Result<FilterValue, DomainError> {
        let bad = |msg: &str| err("search.invalid_value", path, msg.to_string());
        match op {
            Op::IsSet | Op::IsNotSet => {
                return match raw {
                    None | Some(Json::Null) => Ok(FilterValue::None),
                    Some(_) => Err(bad("is_set/is_not_set não levam valor")),
                }
            }
            _ => {}
        }
        let raw = raw.ok_or_else(|| bad("falta o valor"))?;
        match (field.ty, op) {
            (FieldType::Text, Op::In | Op::NotIn) => Ok(FilterValue::TextList(
                self.list(raw, path)?
                    .iter()
                    .map(|v| text(v).ok_or_else(|| bad("esperado texto")))
                    .collect::<Result<_, _>>()?,
            )),
            (FieldType::Text, _) => {
                Ok(FilterValue::Text(text(raw).ok_or_else(|| {
                    bad("esperado texto (até 200 caracteres)")
                })?))
            }
            (FieldType::Enum, Op::In | Op::NotIn) => Ok(FilterValue::TextList(
                self.list(raw, path)?
                    .iter()
                    .map(|v| enum_value(field, v).ok_or_else(|| bad("valor fora das opções")))
                    .collect::<Result<_, _>>()?,
            )),
            (FieldType::Enum, _) => Ok(FilterValue::Text(
                enum_value(field, raw).ok_or_else(|| bad("valor fora das opções"))?,
            )),
            (FieldType::Number, Op::Between) => {
                let (a, b) = pair(raw).ok_or_else(|| bad("esperado [mín, máx]"))?;
                let (a, b) = (
                    number(a).ok_or_else(|| bad("esperado número"))?,
                    number(b).ok_or_else(|| bad("esperado número"))?,
                );
                Ok(FilterValue::NumberRange(a, b))
            }
            (FieldType::Number, _) => Ok(FilterValue::Number(
                number(raw).ok_or_else(|| bad("esperado número"))?,
            )),
            (FieldType::Datetime, Op::Between) => {
                let (a, b) = pair(raw).ok_or_else(|| bad("esperado [de, até]"))?;
                Ok(FilterValue::TimeRange(
                    time(a).ok_or_else(|| bad("esperada data RFC 3339"))?,
                    time(b).ok_or_else(|| bad("esperada data RFC 3339"))?,
                ))
            }
            (FieldType::Datetime, Op::InPeriod) => Ok(FilterValue::Period(
                raw.as_str()
                    .and_then(Period::parse)
                    .ok_or_else(|| bad("período desconhecido"))?,
            )),
            (FieldType::Datetime, _) => Ok(FilterValue::Time(
                time(raw).ok_or_else(|| bad("esperada data RFC 3339"))?,
            )),
            (FieldType::Bool, _) => Ok(FilterValue::Bool(
                raw.as_bool().ok_or_else(|| bad("esperado true/false"))?,
            )),
            (FieldType::User | FieldType::Ref, Op::In | Op::NotIn) => Ok(FilterValue::UuidList(
                self.list(raw, path)?
                    .iter()
                    .map(|v| self.id(field.ty, v).ok_or_else(|| bad("esperado UUID")))
                    .collect::<Result<_, _>>()?,
            )),
            (FieldType::User | FieldType::Ref, _) => Ok(FilterValue::Uuid(
                self.id(field.ty, raw)
                    .ok_or_else(|| bad("esperado UUID (ou \"me\")"))?,
            )),
        }
    }

    fn list<'j>(&self, raw: &'j Json, path: &str) -> Result<&'j Vec<Json>, DomainError> {
        let Json::Array(items) = raw else {
            return Err(err("search.invalid_value", path, "esperada uma lista"));
        };
        if items.is_empty() {
            return Err(err("search.invalid_value", path, "lista vazia"));
        }
        if items.len() > MAX_IN_VALUES {
            return Err(err(
                "search.filter_too_complex",
                path,
                format!("mais de {MAX_IN_VALUES} valores"),
            ));
        }
        Ok(items)
    }

    fn id(&self, ty: FieldType, raw: &Json) -> Option<Uuid> {
        let s = raw.as_str()?;
        if ty == FieldType::User && s == "me" {
            return Some(self.ctx.me);
        }
        Uuid::parse_str(s).ok()
    }
}

/// `[campo, operador]` ou `[campo, operador, valor]` com os dois primeiros texto.
fn is_condition(items: &[Json]) -> bool {
    (items.len() == 2 || items.len() == 3) && items[0].is_string() && items[1].is_string()
}

fn text(v: &Json) -> Option<String> {
    let s = v.as_str()?;
    (s.chars().count() <= MAX_TEXT_VALUE).then(|| s.to_string())
}

fn enum_value(field: &FieldSpec, v: &Json) -> Option<String> {
    let s = v.as_str()?;
    field
        .options
        .iter()
        .any(|o| o.value == s)
        .then(|| s.to_string())
}

fn number(v: &Json) -> Option<f64> {
    v.as_f64().filter(|n| n.is_finite())
}

fn time(v: &Json) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(v.as_str()?)
        .ok()
        .map(|t| t.with_timezone(&Utc))
}

fn pair(v: &Json) -> Option<(&Json, &Json)> {
    match v {
        Json::Array(items) if items.len() == 2 => Some((&items[0], &items[1])),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
//  Texto livre
// ---------------------------------------------------------------------------

/// O `q` já partido: termos só com letras e dígitos (não há operadores de
/// `tsquery` que passem) e a `tsquery` de prefixos pronta a ligar por bind.
#[derive(Debug, Clone, PartialEq)]
pub struct TextQuery {
    pub raw: String,
    pub terms: Vec<String>,
    /// `termo:* & termo:*` — só letras, dígitos, `:*` e `&`.
    pub tsquery: String,
}

pub fn parse_text(raw: &str) -> Result<Option<TextQuery>, DomainError> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    if trimmed.chars().count() > MAX_Q_CHARS {
        return Err(err(
            "search.invalid_query",
            "q",
            format!("a pesquisa tem mais de {MAX_Q_CHARS} caracteres"),
        ));
    }
    let terms: Vec<String> = trimmed
        .split(|c: char| !c.is_alphanumeric())
        .filter(|t| !t.is_empty())
        .take(MAX_TERMS)
        .map(|t| {
            t.chars()
                .take(MAX_TERM_CHARS)
                .collect::<String>()
                .to_lowercase()
        })
        .collect();
    if terms.is_empty() {
        return Err(err(
            "search.invalid_query",
            "q",
            "a pesquisa tem de ter pelo menos uma letra ou dígito",
        ));
    }
    let tsquery = terms
        .iter()
        .map(|t| format!("{t}:*"))
        .collect::<Vec<_>>()
        .join(" & ");
    Ok(Some(TextQuery {
        raw: trimmed.to_string(),
        terms,
        tsquery,
    }))
}

// ---------------------------------------------------------------------------
//  Ordenação e agrupamento
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum OrderTarget {
    Field(&'static FieldSpec),
    /// Relevância do `q`.
    Score,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OrderKey {
    pub target: OrderTarget,
    pub desc: bool,
}

impl OrderKey {
    pub fn name(&self) -> &'static str {
        match self.target {
            OrderTarget::Field(f) => f.name,
            OrderTarget::Score => "_score",
        }
    }
}

fn parse_order(
    schema: &'static SearchSchema,
    raw: Option<&str>,
    has_text: bool,
) -> Result<Vec<OrderKey>, DomainError> {
    let raw = raw.map(str::trim).filter(|s| !s.is_empty());
    let Some(raw) = raw else {
        if has_text {
            return Ok(vec![OrderKey {
                target: OrderTarget::Score,
                desc: true,
            }]);
        }
        return schema
            .default_order
            .iter()
            .map(|o| order_key(schema, o))
            .collect();
    };
    let parts: Vec<&str> = raw.split(',').map(str::trim).collect();
    if parts.len() > MAX_ORDER_BY {
        return Err(err(
            "search.invalid_order_by",
            "order_by",
            format!("no máximo {MAX_ORDER_BY} campos"),
        ));
    }
    let mut keys: Vec<OrderKey> = Vec::new();
    for p in parts {
        let key = order_key(schema, p)?;
        if keys.iter().any(|k| k.name() == key.name()) {
            return Err(err("search.invalid_order_by", "order_by", "campo repetido"));
        }
        keys.push(key);
    }
    Ok(keys)
}

fn order_key(schema: &'static SearchSchema, p: &str) -> Result<OrderKey, DomainError> {
    let (desc, name) = match p.strip_prefix('-') {
        Some(n) => (true, n),
        None => (false, p),
    };
    let field = schema.field(name).ok_or_else(|| {
        err(
            "search.unknown_field",
            "order_by",
            format!("o campo não existe em {}", schema.resource),
        )
    })?;
    if !field.sortable {
        return Err(err(
            "search.field_not_sortable",
            "order_by",
            "este campo não se pode ordenar",
        ));
    }
    Ok(OrderKey {
        target: OrderTarget::Field(field),
        desc,
    })
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GroupBy {
    pub field: &'static FieldSpec,
    pub granularity: Option<Granularity>,
}

impl GroupBy {
    pub fn spec(&self) -> String {
        match self.granularity {
            Some(g) => format!("{}:{}", self.field.name, g.as_str()),
            None => self.field.name.to_string(),
        }
    }
}

fn parse_group_by(schema: &SearchSchema, raw: Option<&str>) -> Result<Vec<GroupBy>, DomainError> {
    let Some(raw) = raw.map(str::trim).filter(|s| !s.is_empty()) else {
        return Ok(Vec::new());
    };
    let parts: Vec<&str> = raw.split(',').map(str::trim).collect();
    let bad = |msg: &str| err("search.invalid_group_by", "group_by", msg.to_string());
    if parts.len() > MAX_GROUP_BY {
        return Err(bad("no máximo 3 campos"));
    }
    let mut out: Vec<GroupBy> = Vec::new();
    for p in parts {
        let (name, gran) = match p.split_once(':') {
            Some((n, g)) => (n, Some(g)),
            None => (p, None),
        };
        let field = schema.field(name).ok_or_else(|| {
            err(
                "search.unknown_field",
                "group_by",
                format!("o campo não existe em {}", schema.resource),
            )
        })?;
        if !field.groupable {
            return Err(err(
                "search.field_not_groupable",
                "group_by",
                "este campo não se pode agrupar",
            ));
        }
        let granularity = match (field.ty, gran) {
            (FieldType::Datetime, None) => Some(Granularity::Month),
            (FieldType::Datetime, Some(g)) => {
                Some(Granularity::parse(g).ok_or_else(|| bad("granularidade desconhecida"))?)
            }
            (_, Some(_)) => return Err(bad("granularidade só em campos de data")),
            (_, None) => None,
        };
        if out.iter().any(|g| g.field.name == field.name) {
            return Err(bad("campo repetido"));
        }
        out.push(GroupBy { field, granularity });
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
//  Cursor keyset
// ---------------------------------------------------------------------------

/// Valor de uma chave de ordenação guardado no cursor.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum KeyValue {
    Number(f64),
    Text(String),
}

/// Identificador da última linha (desempate).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum RowId {
    Int(i64),
    Uuid(Uuid),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct KeysetCursor {
    /// Impressão digital da pesquisa.
    pub f: String,
    /// Valores das chaves (datas em RFC 3339, números, texto).
    pub k: Vec<KeyValue>,
    pub id: RowId,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GroupsCursor {
    pub f: String,
    /// Rótulo de ordenação e chave do último grupo mostrado.
    pub s: String,
    pub k: String,
}

// ---------------------------------------------------------------------------
//  A consulta compilada
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct ListQuery {
    pub schema: &'static SearchSchema,
    pub text: Option<TextQuery>,
    pub filter: Option<Node>,
    pub order: Vec<OrderKey>,
    pub group_by: Vec<GroupBy>,
    pub page_size: u32,
    pub cursor: Option<KeysetCursor>,
    pub groups_cursor: Option<GroupsCursor>,
    pub fingerprint: String,
}

impl ListQuery {
    /// O `group_by` que falta depois do primeiro (para expandir um grupo).
    pub fn remaining_group_by(&self) -> Vec<String> {
        self.group_by.iter().skip(1).map(|g| g.spec()).collect()
    }
}

/// Filtros pré-definidos: mesmo grupo = OU, grupos diferentes = E.
fn named_filters(
    schema: &'static SearchSchema,
    raw: Option<&str>,
    ctx: Ctx,
) -> Result<Option<Node>, DomainError> {
    let Some(raw) = raw.map(str::trim).filter(|s| !s.is_empty()) else {
        return Ok(None);
    };
    let mut groups: Vec<(&'static str, Vec<Node>)> = Vec::new();
    for name in raw.split(',').map(str::trim).filter(|s| !s.is_empty()) {
        let nf = schema.named_filter(name).ok_or_else(|| {
            err(
                "search.unknown_filter",
                "filters",
                format!("filtro pré-definido desconhecido em {}", schema.resource),
            )
        })?;
        let node = parse_filter(schema, nf.filter, ctx)?;
        match groups.iter_mut().find(|(g, _)| *g == nf.group) {
            Some((_, nodes)) => nodes.push(node),
            None => groups.push((nf.group, vec![node])),
        }
    }
    let ands: Vec<Node> = groups
        .into_iter()
        .map(|(_, mut nodes)| {
            if nodes.len() == 1 {
                nodes.remove(0)
            } else {
                Node::Or(nodes)
            }
        })
        .collect();
    Ok(match ands.len() {
        0 => None,
        _ => Some(Node::And(ands)),
    })
}

/// Compila os parâmetros de uma colecção contra a lista branca do recurso.
pub fn compile(
    schema: &'static SearchSchema,
    params: &ListParams,
    ctx: Ctx,
) -> Result<ListQuery, DomainError> {
    let text = match params.q.as_deref() {
        Some(q) => parse_text(q)?,
        None => None,
    };
    let client = match params.filter.as_deref().map(str::trim) {
        Some(f) if !f.is_empty() => Some(parse_filter(schema, f, ctx)?),
        _ => None,
    };
    let named = named_filters(schema, params.filters.as_deref(), ctx)?;
    let filter = match (client, named) {
        (None, None) => None,
        (Some(a), None) => Some(a),
        (None, Some(b)) => Some(b),
        (Some(a), Some(b)) => Some(Node::And(vec![a, b])),
    };
    let order = parse_order(schema, params.order_by.as_deref(), text.is_some())?;
    let group_by = parse_group_by(schema, params.group_by.as_deref())?;

    // A impressão digital cobre tudo o que muda o conjunto ou a ordem. O `me`
    // entra: um token de outra pessoa com «As minhas» não é a mesma pesquisa.
    let fingerprint = crate::crypto::sha256_hex(format!(
        "{}\u{1}{}\u{1}{}\u{1}{}\u{1}{}\u{1}{}\u{1}{}",
        schema.resource,
        ctx.me,
        text.as_ref().map(|t| t.tsquery.as_str()).unwrap_or(""),
        params.filter.as_deref().unwrap_or("").trim(),
        params.filters.as_deref().unwrap_or("").trim(),
        order
            .iter()
            .map(|k| format!("{}{}", if k.desc { "-" } else { "" }, k.name()))
            .collect::<Vec<_>>()
            .join(","),
        group_by
            .iter()
            .map(|g| g.spec())
            .collect::<Vec<_>>()
            .join(","),
    ))[..16]
        .to_string();

    let page = PageRequest {
        page_size: params.page_size,
        page_token: params.page_token.clone(),
    };
    let cursor: Option<KeysetCursor> = page.cursor()?;
    if let Some(c) = &cursor {
        if c.f != fingerprint || c.k.len() != order.len() {
            return Err(err(
                "search.page_token_mismatch",
                "page_token",
                "o page_token é de outra pesquisa",
            ));
        }
        let id_ok = matches!(
            (&c.id, schema.id_kind),
            (RowId::Int(_), IdKind::Int) | (RowId::Uuid(_), IdKind::Uuid)
        );
        let keys_ok = order.iter().zip(&c.k).all(|(key, v)| match key.target {
            OrderTarget::Score => matches!(v, KeyValue::Number(_)),
            OrderTarget::Field(f) => match f.ty {
                FieldType::Number => matches!(v, KeyValue::Number(_)),
                FieldType::Datetime => {
                    matches!(v, KeyValue::Text(t) if DateTime::parse_from_rfc3339(t).is_ok())
                }
                _ => matches!(v, KeyValue::Text(_)),
            },
        });
        if !id_ok || !keys_ok {
            return Err(err(
                "search.page_token_mismatch",
                "page_token",
                "o page_token é de outra pesquisa",
            ));
        }
    }
    let groups_cursor: Option<GroupsCursor> = match params.groups_page_token.as_deref() {
        None | Some("") => None,
        Some(t) => Some(decode_cursor(t)?),
    };
    if let Some(g) = &groups_cursor {
        if g.f != fingerprint {
            return Err(err(
                "search.page_token_mismatch",
                "groups_page_token",
                "o groups_page_token é de outra pesquisa",
            ));
        }
    }
    Ok(ListQuery {
        schema,
        text,
        filter,
        order,
        group_by,
        page_size: page.size(),
        cursor,
        groups_cursor,
        fingerprint,
    })
}

/// Codifica o cursor da próxima página.
pub fn encode_keyset(fingerprint: &str, keys: Vec<KeyValue>, id: RowId) -> String {
    encode_cursor(&KeysetCursor {
        f: fingerprint.to_string(),
        k: keys,
        id,
    })
}

pub fn encode_groups_cursor(fingerprint: &str, sort: String, key: String) -> String {
    encode_cursor(&GroupsCursor {
        f: fingerprint.to_string(),
        s: sort,
        k: key,
    })
}

/// Valida uma consulta guardada (favorito) contra o schema, sem a executar.
/// Devolve o erro do primeiro problema — o mesmo código que a lista daria.
pub fn validate_saved(
    schema: &'static SearchSchema,
    q: Option<&str>,
    filter: Option<&Json>,
    filters: &[String],
    group_by: &[String],
    order_by: &[String],
    ctx: Ctx,
) -> Result<(), DomainError> {
    let params = ListParams {
        q: q.map(str::to_string),
        filter: filter.map(|f| f.to_string()),
        filters: (!filters.is_empty()).then(|| filters.join(",")),
        group_by: (!group_by.is_empty()).then(|| group_by.join(",")),
        order_by: (!order_by.is_empty()).then(|| order_by.join(",")),
        ..Default::default()
    };
    compile(schema, &params, ctx).map(|_| ())
}

/// Total com o tecto do contrato.
pub const TOTAL_CAP: i64 = 10_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TotalKind {
    Exact,
    AtLeast,
}

/// Lê um total contado com `LIMIT cap + 1`.
pub fn capped_total(counted: i64, cap: i64) -> (i64, TotalKind) {
    if counted > cap {
        (cap, TotalKind::AtLeast)
    } else {
        (counted, TotalKind::Exact)
    }
}

// ---------------------------------------------------------------------------
//  Realce em segmentos
// ---------------------------------------------------------------------------

/// Marcas internas do `ts_headline` (caracteres de controlo que não aparecem
/// em texto de pessoas). Saem daqui como segmentos; nunca chegam ao cliente.
pub const HL_START: char = '\u{2}';
pub const HL_STOP: char = '\u{3}';

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct HighlightSegment {
    pub text: String,
    #[serde(rename = "match")]
    pub is_match: bool,
}

/// Parte o texto do `ts_headline` em segmentos. Marcas que venham no texto
/// original (não deviam) são removidas em vez de partir o resultado.
pub fn highlight_segments(marked: &str) -> Vec<HighlightSegment> {
    let mut out: Vec<HighlightSegment> = Vec::new();
    let mut cur = String::new();
    let mut in_match = false;
    for c in marked.chars() {
        match c {
            HL_START if !in_match => {
                if !cur.is_empty() {
                    out.push(HighlightSegment {
                        text: std::mem::take(&mut cur),
                        is_match: false,
                    });
                }
                in_match = true;
            }
            HL_STOP if in_match => {
                if !cur.is_empty() {
                    out.push(HighlightSegment {
                        text: std::mem::take(&mut cur),
                        is_match: true,
                    });
                }
                in_match = false;
            }
            HL_START | HL_STOP => {}
            c => cur.push(c),
        }
    }
    if !cur.is_empty() {
        out.push(HighlightSegment {
            text: cur,
            is_match: in_match,
        });
    }
    // Junta segmentos vizinhos do mesmo tipo («a» «b» → «a b» realçado).
    let mut merged: Vec<HighlightSegment> = Vec::with_capacity(out.len());
    for seg in out {
        match merged.last_mut() {
            Some(last) if last.is_match == seg.is_match => last.text.push_str(&seg.text),
            _ => merged.push(seg),
        }
    }
    merged
}

#[cfg(test)]
mod tests;
