//! A lista branca de um recurso pesquisável: campos, operadores por tipo,
//! filtros pré-definidos e agrupamentos. É dado estático, declarado no contexto
//! de domínio do recurso; o que a UI recebe em `GET /api/search/schemas/{resource}`
//! é a vista serializada disto.

use serde::Serialize;

/// Tipo de um campo pesquisável. Decide os operadores e a forma do valor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FieldType {
    Text,
    Enum,
    Number,
    Datetime,
    Bool,
    /// Uma pessoa: UUID ou `"me"`.
    User,
    /// Referência a outro recurso (UUID).
    Ref,
}

/// Operadores do domínio de filtro.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Op {
    Eq,
    Ne,
    Contains,
    NotContains,
    StartsWith,
    In,
    NotIn,
    IsSet,
    IsNotSet,
    Lt,
    Lte,
    Gt,
    Gte,
    Between,
    InPeriod,
}

impl Op {
    pub fn parse(s: &str) -> Option<Op> {
        Some(match s {
            "eq" => Op::Eq,
            "ne" => Op::Ne,
            "contains" => Op::Contains,
            "not_contains" => Op::NotContains,
            "starts_with" => Op::StartsWith,
            "in" => Op::In,
            "not_in" => Op::NotIn,
            "is_set" => Op::IsSet,
            "is_not_set" => Op::IsNotSet,
            "lt" => Op::Lt,
            "lte" => Op::Lte,
            "gt" => Op::Gt,
            "gte" => Op::Gte,
            "between" => Op::Between,
            "in_period" => Op::InPeriod,
            _ => return None,
        })
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Op::Eq => "eq",
            Op::Ne => "ne",
            Op::Contains => "contains",
            Op::NotContains => "not_contains",
            Op::StartsWith => "starts_with",
            Op::In => "in",
            Op::NotIn => "not_in",
            Op::IsSet => "is_set",
            Op::IsNotSet => "is_not_set",
            Op::Lt => "lt",
            Op::Lte => "lte",
            Op::Gt => "gt",
            Op::Gte => "gte",
            Op::Between => "between",
            Op::InPeriod => "in_period",
        }
    }
}

impl FieldType {
    /// Os operadores que servem para este tipo — a mesma tabela do contrato
    /// (`docs/reference/pesquisa.md` §2.2).
    pub fn operators(self) -> &'static [Op] {
        use Op::*;
        match self {
            FieldType::Text => &[
                Eq,
                Ne,
                Contains,
                NotContains,
                StartsWith,
                In,
                NotIn,
                IsSet,
                IsNotSet,
            ],
            FieldType::Enum => &[Eq, Ne, In, NotIn],
            FieldType::Number => &[Eq, Ne, Lt, Lte, Gt, Gte, Between, IsSet, IsNotSet],
            FieldType::Datetime => &[Lt, Lte, Gt, Gte, Between, InPeriod, IsSet, IsNotSet],
            FieldType::Bool => &[Eq],
            FieldType::User | FieldType::Ref => &[Eq, Ne, In, NotIn, IsSet, IsNotSet],
        }
    }
}

/// Granularidade de agrupamento por data.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Granularity {
    Day,
    Week,
    Month,
    Quarter,
    Year,
}

impl Granularity {
    pub const ALL: [Granularity; 5] = [
        Granularity::Day,
        Granularity::Week,
        Granularity::Month,
        Granularity::Quarter,
        Granularity::Year,
    ];

    pub fn parse(s: &str) -> Option<Granularity> {
        Some(match s {
            "day" => Granularity::Day,
            "week" => Granularity::Week,
            "month" => Granularity::Month,
            "quarter" => Granularity::Quarter,
            "year" => Granularity::Year,
            _ => return None,
        })
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Granularity::Day => "day",
            Granularity::Week => "week",
            Granularity::Month => "month",
            Granularity::Quarter => "quarter",
            Granularity::Year => "year",
        }
    }
}

/// Período relativo de `in_period`, resolvido no fuso da organização.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Period {
    Today,
    Yesterday,
    ThisWeek,
    LastWeek,
    ThisMonth,
    LastMonth,
    ThisQuarter,
    LastQuarter,
    ThisYear,
    LastYear,
    Last7Days,
    Last30Days,
    Next7Days,
    Past,
    Future,
}

impl Period {
    pub const ALL: [&'static str; 15] = [
        "today",
        "yesterday",
        "this_week",
        "last_week",
        "this_month",
        "last_month",
        "this_quarter",
        "last_quarter",
        "this_year",
        "last_year",
        "last_7_days",
        "last_30_days",
        "next_7_days",
        "past",
        "future",
    ];

    pub fn parse(s: &str) -> Option<Period> {
        Some(match s {
            "today" => Period::Today,
            "yesterday" => Period::Yesterday,
            "this_week" => Period::ThisWeek,
            "last_week" => Period::LastWeek,
            "this_month" => Period::ThisMonth,
            "last_month" => Period::LastMonth,
            "this_quarter" => Period::ThisQuarter,
            "last_quarter" => Period::LastQuarter,
            "this_year" => Period::ThisYear,
            "last_year" => Period::LastYear,
            "last_7_days" => Period::Last7Days,
            "last_30_days" => Period::Last30Days,
            "next_7_days" => Period::Next7Days,
            "past" => Period::Past,
            "future" => Period::Future,
            _ => return None,
        })
    }
}

/// Agregado de um campo numérico por grupo.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Aggregate {
    Sum,
    Avg,
}

impl Aggregate {
    pub fn as_str(self) -> &'static str {
        match self {
            Aggregate::Sum => "sum",
            Aggregate::Avg => "avg",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct EnumOption {
    pub value: &'static str,
    pub label: &'static str,
}

/// Um campo da lista branca.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FieldSpec {
    pub name: &'static str,
    pub label: &'static str,
    pub ty: FieldType,
    pub filterable: bool,
    pub sortable: bool,
    pub groupable: bool,
    pub options: &'static [EnumOption],
    pub aggregates: &'static [Aggregate],
}

impl FieldSpec {
    /// Um campo só filtrável; os construtores `sortable()`/`groupable()`
    /// acrescentam capacidades. `const` para a declaração ser estática.
    pub const fn new(name: &'static str, label: &'static str, ty: FieldType) -> Self {
        Self {
            name,
            label,
            ty,
            filterable: true,
            sortable: false,
            groupable: false,
            options: &[],
            aggregates: &[],
        }
    }
    pub const fn sortable(mut self) -> Self {
        self.sortable = true;
        self
    }
    pub const fn groupable(mut self) -> Self {
        self.groupable = true;
        self
    }
    pub const fn options(mut self, options: &'static [EnumOption]) -> Self {
        self.options = options;
        self
    }
    pub const fn aggregates(mut self, aggregates: &'static [Aggregate]) -> Self {
        self.aggregates = aggregates;
        self
    }
}

/// Filtro pré-definido. `filter` é o domínio em JSON, validado pela MESMA
/// função que valida o `filter` do cliente (um teste percorre todos).
#[derive(Debug, Clone, Copy)]
pub struct NamedFilter {
    pub name: &'static str,
    pub label: &'static str,
    /// Filtros do mesmo grupo combinam com OU; grupos diferentes com E.
    pub group: &'static str,
    pub filter: &'static str,
}

/// Tipo do identificador da linha (desempate final do keyset).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum IdKind {
    Uuid,
    Int,
}

/// A lista branca completa de um recurso.
#[derive(Debug)]
pub struct SearchSchema {
    pub resource: &'static str,
    pub label: &'static str,
    /// A colecção, com `{org_id}` quando é de organização.
    pub collection: &'static str,
    pub org_scoped: bool,
    /// Campos em que o `q` procura (documentação para a UI).
    pub text_fields: &'static [&'static str],
    pub fields: &'static [FieldSpec],
    pub filters: &'static [NamedFilter],
    /// `campo` ou `-campo`.
    pub default_order: &'static [&'static str],
    pub id_kind: IdKind,
}

impl SearchSchema {
    pub fn field(&self, name: &str) -> Option<&'static FieldSpec> {
        self.fields.iter().find(|f| f.name == name)
    }
    pub fn named_filter(&self, name: &str) -> Option<&'static NamedFilter> {
        self.filters.iter().find(|f| f.name == name)
    }
}

// ---------------------------------------------------------------------------
//  Vista serializada (`GET /api/search/schemas/{resource}`)
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
pub struct TextSearchView {
    pub fields: &'static [&'static str],
    pub typo_tolerant: bool,
}

#[derive(Debug, Serialize)]
pub struct FieldView {
    pub name: &'static str,
    pub label: &'static str,
    #[serde(rename = "type")]
    pub ty: FieldType,
    pub operators: &'static [Op],
    pub filterable: bool,
    pub sortable: bool,
    pub groupable: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub granularities: Option<Vec<&'static str>>,
    pub aggregates: Vec<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub options: Option<&'static [EnumOption]>,
}

#[derive(Debug, Serialize)]
pub struct FilterView {
    pub name: &'static str,
    pub label: &'static str,
    pub group: &'static str,
    pub filter: serde_json::Value,
}

#[derive(Debug, Serialize)]
pub struct GroupByView {
    pub value: String,
    pub label: String,
}

#[derive(Debug, Serialize)]
pub struct SchemaView {
    pub resource: &'static str,
    pub label: &'static str,
    pub collection: &'static str,
    pub org_scoped: bool,
    pub timezone: String,
    pub text_search: TextSearchView,
    pub fields: Vec<FieldView>,
    pub filters: Vec<FilterView>,
    pub group_by: Vec<GroupByView>,
    pub default_order: &'static [&'static str],
    pub periods: &'static [&'static str],
}

impl SearchSchema {
    pub fn view(&self, timezone: &str) -> SchemaView {
        let fields = self
            .fields
            .iter()
            .map(|f| FieldView {
                name: f.name,
                label: f.label,
                ty: f.ty,
                operators: if f.filterable { f.ty.operators() } else { &[] },
                filterable: f.filterable,
                sortable: f.sortable,
                groupable: f.groupable,
                granularities: (f.ty == FieldType::Datetime && f.groupable)
                    .then(|| Granularity::ALL.iter().map(|g| g.as_str()).collect()),
                aggregates: f.aggregates.iter().map(|a| a.as_str()).collect(),
                options: (f.ty == FieldType::Enum).then_some(f.options),
            })
            .collect();
        let filters = self
            .filters
            .iter()
            .map(|nf| FilterView {
                name: nf.name,
                label: nf.label,
                group: nf.group,
                filter: serde_json::from_str(nf.filter).unwrap_or(serde_json::Value::Null),
            })
            .collect();
        let mut group_by = Vec::new();
        for f in self.fields.iter().filter(|f| f.groupable) {
            if f.ty == FieldType::Datetime {
                for g in Granularity::ALL {
                    group_by.push(GroupByView {
                        value: format!("{}:{}", f.name, g.as_str()),
                        label: format!("{}: {}", f.label, granularity_label(g)),
                    });
                }
            } else {
                group_by.push(GroupByView {
                    value: f.name.to_string(),
                    label: f.label.to_string(),
                });
            }
        }
        SchemaView {
            resource: self.resource,
            label: self.label,
            collection: self.collection,
            org_scoped: self.org_scoped,
            timezone: timezone.to_string(),
            text_search: TextSearchView {
                fields: self.text_fields,
                typo_tolerant: true,
            },
            fields,
            filters,
            group_by,
            default_order: self.default_order,
            periods: &Period::ALL,
        }
    }
}

fn granularity_label(g: Granularity) -> &'static str {
    match g {
        Granularity::Day => "dia",
        Granularity::Week => "semana",
        Granularity::Month => "mês",
        Granularity::Quarter => "trimestre",
        Granularity::Year => "ano",
    }
}
