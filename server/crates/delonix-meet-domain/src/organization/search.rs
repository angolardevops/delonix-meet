//! Lista branca de pesquisa dos membros (`docs/reference/pesquisa.md` §4.3).

use delonix_meet_core::query::{
    EnumOption, FieldSpec, FieldType, IdKind, NamedFilter, SearchSchema,
};

const ROLES: &[EnumOption] = &[
    EnumOption {
        value: "admin",
        label: "Administrador",
    },
    EnumOption {
        value: "member",
        label: "Membro",
    },
];

pub static MEMBERS: SearchSchema = SearchSchema {
    resource: "members",
    label: "Membros",
    collection: "/api/orgs/{org_id}/members",
    org_scoped: true,
    text_fields: &["username", "email", "title"],
    fields: &[
        FieldSpec::new("username", "Nome", FieldType::Text).sortable(),
        FieldSpec::new("email", "Email", FieldType::Text).sortable(),
        FieldSpec::new("title", "Cargo", FieldType::Text)
            .sortable()
            .groupable(),
        FieldSpec::new("role", "Papel", FieldType::Enum)
            .options(ROLES)
            .groupable(),
        FieldSpec::new("branch", "Filial", FieldType::Ref).groupable(),
        FieldSpec::new("joined_at", "Entrou em", FieldType::Datetime)
            .sortable()
            .groupable(),
    ],
    filters: &[
        NamedFilter {
            name: "admins",
            label: "Administradores",
            group: "role",
            filter: r#"[["role","eq","admin"]]"#,
        },
        NamedFilter {
            name: "members",
            label: "Membros",
            group: "role",
            filter: r#"[["role","eq","member"]]"#,
        },
        NamedFilter {
            name: "without_branch",
            label: "Sem filial",
            group: "branch",
            filter: r#"[["branch","is_not_set"]]"#,
        },
        NamedFilter {
            name: "joined_this_month",
            label: "Entraram este mês",
            group: "period",
            filter: r#"[["joined_at","in_period","this_month"]]"#,
        },
    ],
    default_order: &["username"],
    id_kind: IdKind::Uuid,
    relevance_default: true,
    invalid_query_code: "search.invalid_query",
};
