//! Lista branca de pesquisa das reuniões (`docs/reference/pesquisa.md` §4.2).

use delonix_meet_core::query::{
    Aggregate, EnumOption, FieldSpec, FieldType, IdKind, NamedFilter, SearchSchema,
};

const KINDS: &[EnumOption] = &[
    EnumOption {
        value: "video",
        label: "Vídeo",
    },
    EnumOption {
        value: "voice",
        label: "Voz",
    },
];

const MY_STATUS: &[EnumOption] = &[
    EnumOption {
        value: "owner",
        label: "Organizador",
    },
    EnumOption {
        value: "pending",
        label: "Por responder",
    },
    EnumOption {
        value: "accepted",
        label: "Aceite",
    },
    EnumOption {
        value: "declined",
        label: "Recusada",
    },
];

const fn nf(
    name: &'static str,
    label: &'static str,
    group: &'static str,
    filter: &'static str,
) -> NamedFilter {
    NamedFilter {
        name,
        label,
        group,
        filter,
    }
}

pub static MEETINGS: SearchSchema = SearchSchema {
    resource: "meetings",
    label: "Reuniões",
    collection: "/api/meetings",
    org_scoped: false,
    text_fields: &["title", "description", "minutes"],
    fields: &[
        FieldSpec::new("title", "Título", FieldType::Text).sortable(),
        FieldSpec::new("description", "Descrição", FieldType::Text),
        FieldSpec::new("owner", "Organizador", FieldType::User).groupable(),
        FieldSpec::new("kind", "Tipo", FieldType::Enum)
            .options(KINDS)
            .groupable(),
        FieldSpec::new("starts_at", "Início", FieldType::Datetime)
            .sortable()
            .groupable(),
        FieldSpec::new("duration_min", "Duração (min)", FieldType::Number)
            .sortable()
            .aggregates(&[Aggregate::Sum]),
        FieldSpec::new("my_status", "A minha resposta", FieldType::Enum)
            .options(MY_STATUS)
            .groupable(),
        FieldSpec::new("recurring", "Recorrente", FieldType::Bool).groupable(),
        FieldSpec::new("has_minutes", "Com acta", FieldType::Bool).groupable(),
        FieldSpec::new("meeting_room", "Sala física", FieldType::Ref).groupable(),
        FieldSpec::new("created_at", "Criada em", FieldType::Datetime).sortable(),
    ],
    filters: &[
        nf(
            "mine",
            "Organizadas por mim",
            "owner",
            r#"[["owner","eq","me"]]"#,
        ),
        nf(
            "invited",
            "Convidado",
            "owner",
            r#"[["my_status","ne","owner"]]"#,
        ),
        nf(
            "pending_response",
            "Por responder",
            "response",
            r#"[["my_status","eq","pending"]]"#,
        ),
        nf(
            "accepted",
            "Aceites",
            "response",
            r#"[["my_status","eq","accepted"]]"#,
        ),
        nf(
            "declined",
            "Recusadas",
            "response",
            r#"[["my_status","eq","declined"]]"#,
        ),
        nf("video", "Vídeo", "kind", r#"[["kind","eq","video"]]"#),
        nf("voice", "Voz", "kind", r#"[["kind","eq","voice"]]"#),
        nf(
            "upcoming",
            "Próximas",
            "period",
            r#"[["starts_at","in_period","future"]]"#,
        ),
        nf(
            "past",
            "Passadas",
            "period",
            r#"[["starts_at","in_period","past"]]"#,
        ),
        nf(
            "today",
            "Hoje",
            "period",
            r#"[["starts_at","in_period","today"]]"#,
        ),
        nf(
            "this_week",
            "Esta semana",
            "period",
            r#"[["starts_at","in_period","this_week"]]"#,
        ),
        nf(
            "next_7_days",
            "Próximos 7 dias",
            "period",
            r#"[["starts_at","in_period","next_7_days"]]"#,
        ),
        nf(
            "recurring",
            "Recorrentes",
            "content",
            r#"[["recurring","eq",true]]"#,
        ),
        nf(
            "with_minutes",
            "Com acta",
            "content",
            r#"[["has_minutes","eq",true]]"#,
        ),
    ],
    default_order: &["starts_at"],
    id_kind: IdKind::Uuid,
    relevance_default: true,
    invalid_query_code: "search.invalid_query",
};
