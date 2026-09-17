//! Listas branca de pesquisa do contexto **content**: gravações e quadros
//! (ADR-0007, `docs/reference/pesquisa.md` §4.1 e §4.4).

use delonix_meet_core::query::{
    Aggregate, EnumOption, FieldSpec, FieldType, IdKind, NamedFilter, SearchSchema,
};

const CATEGORIES: &[EnumOption] = &[
    EnumOption {
        value: "meeting",
        label: "Reunião",
    },
    EnumOption {
        value: "lecture",
        label: "Aula",
    },
    EnumOption {
        value: "broadcast",
        label: "Emissão",
    },
    EnumOption {
        value: "other",
        label: "Outra",
    },
];

const STATUSES: &[EnumOption] = &[
    EnumOption {
        value: "ready",
        label: "Pronta",
    },
    EnumOption {
        value: "failed",
        label: "Falhada",
    },
];

pub static RECORDINGS: SearchSchema = SearchSchema {
    resource: "recordings",
    label: "Gravações",
    collection: "/api/recordings",
    org_scoped: false,
    text_fields: &["title", "filename", "transcript"],
    fields: &[
        FieldSpec::new("title", "Título", FieldType::Text).sortable(),
        FieldSpec::new("filename", "Ficheiro", FieldType::Text),
        FieldSpec::new("uploader", "Autor", FieldType::User).groupable(),
        FieldSpec::new("room_code", "Sala", FieldType::Text).groupable(),
        FieldSpec::new("category", "Categoria", FieldType::Enum)
            .options(CATEGORIES)
            .groupable(),
        FieldSpec::new("status", "Estado", FieldType::Enum)
            .options(STATUSES)
            .groupable(),
        FieldSpec::new("transcribed", "Com transcrição", FieldType::Bool).groupable(),
        FieldSpec::new("shared_with_me", "Partilhada comigo", FieldType::Bool),
        FieldSpec::new("duration_secs", "Duração (s)", FieldType::Number)
            .sortable()
            .aggregates(&[Aggregate::Sum, Aggregate::Avg]),
        FieldSpec::new("size_bytes", "Tamanho (bytes)", FieldType::Number)
            .sortable()
            .aggregates(&[Aggregate::Sum]),
        FieldSpec::new("width", "Largura (px)", FieldType::Number),
        FieldSpec::new("created_at", "Criada em", FieldType::Datetime)
            .sortable()
            .groupable(),
    ],
    filters: &[
        NamedFilter {
            name: "mine",
            label: "As minhas",
            group: "owner",
            filter: r#"[["uploader","eq","me"]]"#,
        },
        NamedFilter {
            name: "shared_with_me",
            label: "Partilhadas comigo",
            group: "owner",
            filter: r#"[["shared_with_me","eq",true]]"#,
        },
        NamedFilter {
            name: "transcribed",
            label: "Com transcrição",
            group: "content",
            filter: r#"[["transcribed","eq",true]]"#,
        },
        NamedFilter {
            name: "without_transcript",
            label: "Sem transcrição",
            group: "content",
            filter: r#"[["transcribed","eq",false]]"#,
        },
        NamedFilter {
            name: "failed",
            label: "Falhadas",
            group: "status",
            filter: r#"[["status","eq","failed"]]"#,
        },
        NamedFilter {
            name: "today",
            label: "Hoje",
            group: "period",
            filter: r#"[["created_at","in_period","today"]]"#,
        },
        NamedFilter {
            name: "this_week",
            label: "Esta semana",
            group: "period",
            filter: r#"[["created_at","in_period","this_week"]]"#,
        },
        NamedFilter {
            name: "this_month",
            label: "Este mês",
            group: "period",
            filter: r#"[["created_at","in_period","this_month"]]"#,
        },
        NamedFilter {
            name: "long",
            label: "Mais de 1 hora",
            group: "duration",
            filter: r#"[["duration_secs","gte",3600]]"#,
        },
        NamedFilter {
            name: "uhd",
            label: "4K",
            group: "quality",
            filter: r#"[["width","gte",3840]]"#,
        },
    ],
    default_order: &["-created_at"],
    id_kind: IdKind::Uuid,
    // Compatível com o contrato da 0045: com `q`, a biblioteca continua por
    // data; a relevância pede-se com `order_by=-_score`.
    relevance_default: false,
    invalid_query_code: "recording.invalid_query",
};

pub static WHITEBOARDS: SearchSchema = SearchSchema {
    resource: "whiteboards",
    label: "Quadros",
    collection: "/api/whiteboards",
    org_scoped: false,
    text_fields: &["title", "room_code"],
    fields: &[
        FieldSpec::new("title", "Título", FieldType::Text).sortable(),
        FieldSpec::new("room_code", "Sala", FieldType::Text).groupable(),
        FieldSpec::new("owner", "Autor", FieldType::User).groupable(),
        FieldSpec::new("is_public", "Com link público", FieldType::Bool).groupable(),
        FieldSpec::new("created_at", "Criado em", FieldType::Datetime)
            .sortable()
            .groupable(),
    ],
    filters: &[
        NamedFilter {
            name: "mine",
            label: "Os meus",
            group: "owner",
            filter: r#"[["owner","eq","me"]]"#,
        },
        NamedFilter {
            name: "public",
            label: "Com link público",
            group: "sharing",
            filter: r#"[["is_public","eq",true]]"#,
        },
        NamedFilter {
            name: "this_week",
            label: "Esta semana",
            group: "period",
            filter: r#"[["created_at","in_period","this_week"]]"#,
        },
        NamedFilter {
            name: "this_month",
            label: "Este mês",
            group: "period",
            filter: r#"[["created_at","in_period","this_month"]]"#,
        },
    ],
    default_order: &["-created_at"],
    id_kind: IdKind::Uuid,
    relevance_default: true,
    invalid_query_code: "search.invalid_query",
};
