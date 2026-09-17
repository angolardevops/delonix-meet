//! Lista branca de pesquisa da auditoria (`docs/reference/pesquisa.md` §4.5).

use delonix_meet_core::query::{FieldSpec, FieldType, IdKind, NamedFilter, SearchSchema};

pub static AUDIT_EVENTS: SearchSchema = SearchSchema {
    resource: "audit_events",
    label: "Auditoria",
    collection: "/api/orgs/{org_id}/audit-events",
    org_scoped: true,
    text_fields: &["action", "target", "actor"],
    fields: &[
        FieldSpec::new("action", "Acção", FieldType::Text)
            .sortable()
            .groupable(),
        FieldSpec::new("category", "Categoria", FieldType::Text).groupable(),
        FieldSpec::new("target", "Alvo", FieldType::Text),
        FieldSpec::new("actor", "Actor", FieldType::User).groupable(),
        FieldSpec::new("created_at", "Quando", FieldType::Datetime)
            .sortable()
            .groupable(),
    ],
    filters: &[
        NamedFilter {
            name: "logins",
            label: "Inícios de sessão",
            group: "category",
            filter: r#"[["action","starts_with","auth.login"]]"#,
        },
        NamedFilter {
            name: "security",
            label: "Segurança",
            group: "category",
            filter: r#"[["category","eq","auth"]]"#,
        },
        NamedFilter {
            name: "members",
            label: "Membros",
            group: "category",
            filter: r#"[["category","eq","member"]]"#,
        },
        NamedFilter {
            name: "integrations",
            label: "Integrações",
            group: "category",
            filter: r#"[["category","in",["apikey","webhook","stream_destination","sms"]]]"#,
        },
        NamedFilter {
            name: "mine",
            label: "As minhas acções",
            group: "actor",
            filter: r#"[["actor","eq","me"]]"#,
        },
        NamedFilter {
            name: "today",
            label: "Hoje",
            group: "period",
            filter: r#"[["created_at","in_period","today"]]"#,
        },
        NamedFilter {
            name: "last_7_days",
            label: "Últimos 7 dias",
            group: "period",
            filter: r#"[["created_at","in_period","last_7_days"]]"#,
        },
        NamedFilter {
            name: "last_30_days",
            label: "Últimos 30 dias",
            group: "period",
            filter: r#"[["created_at","in_period","last_30_days"]]"#,
        },
    ],
    default_order: &["-created_at"],
    id_kind: IdKind::Int,
    relevance_default: true,
    invalid_query_code: "search.invalid_query",
};
