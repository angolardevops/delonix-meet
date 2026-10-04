use super::*;
use crate::query::schema::*;

const OPTS: &[EnumOption] = &[
    EnumOption {
        value: "meeting",
        label: "Reunião",
    },
    EnumOption {
        value: "lecture",
        label: "Aula",
    },
];

static FIELDS: &[FieldSpec] = &[
    FieldSpec::new("title", "Título", FieldType::Text).sortable(),
    FieldSpec::new("category", "Categoria", FieldType::Enum)
        .options(OPTS)
        .groupable(),
    FieldSpec::new("duration_secs", "Duração", FieldType::Number)
        .sortable()
        .aggregates(&[Aggregate::Sum]),
    FieldSpec::new("created_at", "Criada em", FieldType::Datetime)
        .sortable()
        .groupable(),
    FieldSpec::new("transcribed", "Transcrita", FieldType::Bool).groupable(),
    FieldSpec::new("uploader", "Autor", FieldType::User).groupable(),
    FieldSpec {
        filterable: false,
        ..FieldSpec::new("secret_sort", "Só ordena", FieldType::Text).sortable()
    },
];

static FILTERS: &[NamedFilter] = &[
    NamedFilter {
        name: "mine",
        label: "As minhas",
        group: "owner",
        filter: r#"[["uploader","eq","me"]]"#,
    },
    NamedFilter {
        name: "lectures",
        label: "Aulas",
        group: "owner",
        filter: r#"[["category","eq","lecture"]]"#,
    },
    NamedFilter {
        name: "this_week",
        label: "Esta semana",
        group: "period",
        filter: r#"[["created_at","in_period","this_week"]]"#,
    },
];

static SCHEMA: SearchSchema = SearchSchema {
    resource: "things",
    label: "Coisas",
    collection: "/api/things",
    org_scoped: false,
    text_fields: &["title"],
    fields: FIELDS,
    filters: FILTERS,
    default_order: &["-created_at"],
    id_kind: IdKind::Uuid,
    relevance_default: true,
    invalid_query_code: "search.invalid_query",
};

fn ctx() -> Ctx {
    Ctx {
        me: Uuid::from_u128(7),
    }
}

fn code(r: Result<Node, DomainError>) -> &'static str {
    r.unwrap_err().code
}

fn params(pairs: &[(&str, &str)]) -> ListParams {
    let mut p = ListParams::default();
    for (k, v) in pairs {
        let v = Some(v.to_string());
        match *k {
            "q" => p.q = v,
            "filter" => p.filter = v,
            "filters" => p.filters = v,
            "group_by" => p.group_by = v,
            "order_by" => p.order_by = v,
            "page_token" => p.page_token = v,
            "groups_page_token" => p.groups_page_token = v,
            "page_size" => p.page_size = v.and_then(|s| s.parse().ok()),
            _ => unreachable!(),
        }
    }
    p
}

#[test]
fn every_named_filter_in_the_test_schema_validates() {
    for nf in SCHEMA.filters {
        parse_filter(&SCHEMA, nf.filter, ctx()).unwrap_or_else(|e| panic!("{}: {e}", nf.name));
    }
}

#[test]
fn top_level_list_is_and_and_single_condition_is_accepted() {
    let n = parse_filter(
        &SCHEMA,
        r#"[["title","contains","orç"],["duration_secs","gte",3600]]"#,
        ctx(),
    )
    .unwrap();
    let Node::And(items) = n else { panic!() };
    assert_eq!(items.len(), 2);
    let n = parse_filter(&SCHEMA, r#"["transcribed","eq",true]"#, ctx()).unwrap();
    assert!(matches!(
        n,
        Node::Cond(Condition {
            field: "transcribed",
            op: Op::Eq,
            value: FilterValue::Bool(true),
            ..
        })
    ));
}

#[test]
fn nested_and_or_not() {
    let n = parse_filter(
        &SCHEMA,
        r#"{"or":[["category","in",["meeting","lecture"]],{"not":["uploader","eq","me"]}]}"#,
        ctx(),
    )
    .unwrap();
    let Node::Or(items) = n else { panic!() };
    assert!(
        matches!(&items[1], Node::Not(inner) if matches!(**inner, Node::Cond(Condition { value: FilterValue::Uuid(u), .. }) if u == Uuid::from_u128(7)))
    );
}

#[test]
fn hostile_field_names_are_unknown_never_passed_through() {
    for name in [
        "title; DROP TABLE users",
        "title\" OR 1=1 --",
        "r.password_hash",
        "TITLE",
        "",
        "(SELECT 1)",
    ] {
        let raw = serde_json::json!([[name, "eq", "x"]]).to_string();
        assert_eq!(
            code(parse_filter(&SCHEMA, &raw, ctx())),
            "search.unknown_field",
            "{name}"
        );
    }
}

#[test]
fn hostile_values_stay_values() {
    // O valor é aceite como TEXTO — a prova de que não chega ao SQL está no
    // teste do adaptador (bind), aqui prova-se que não muda a forma da árvore.
    let raw = r#"[["title","eq","x' OR '1'='1"]]"#;
    let Node::And(items) = parse_filter(&SCHEMA, raw, ctx()).unwrap() else {
        panic!()
    };
    assert_eq!(
        items[0],
        Node::Cond(Condition {
            field: "title",
            ty: FieldType::Text,
            op: Op::Eq,
            value: FilterValue::Text("x' OR '1'='1".into())
        })
    );
}

#[test]
fn operators_are_checked_per_type() {
    assert_eq!(
        code(parse_filter(&SCHEMA, r#"[["title","gte","a"]]"#, ctx())),
        "search.invalid_operator"
    );
    assert_eq!(
        code(parse_filter(
            &SCHEMA,
            r#"[["transcribed","contains","a"]]"#,
            ctx()
        )),
        "search.invalid_operator"
    );
    assert_eq!(
        code(parse_filter(&SCHEMA, r#"[["title","like","a"]]"#, ctx())),
        "search.invalid_operator"
    );
    assert_eq!(
        code(parse_filter(
            &SCHEMA,
            r#"[["created_at","eq","2026-01-01T00:00:00Z"]]"#,
            ctx()
        )),
        "search.invalid_operator"
    );
}

#[test]
fn values_are_typed() {
    for (raw, want) in [
        (r#"[["category","eq","other"]]"#, "search.invalid_value"),
        (r#"[["category","in",[]]]"#, "search.invalid_value"),
        (
            r#"[["duration_secs","gte","3600"]]"#,
            "search.invalid_value",
        ),
        (
            r#"[["duration_secs","between",[1]]]"#,
            "search.invalid_value",
        ),
        (r#"[["created_at","gte","ontem"]]"#, "search.invalid_value"),
        (
            r#"[["created_at","in_period","next_century"]]"#,
            "search.invalid_value",
        ),
        (
            r#"[["uploader","eq","not-a-uuid"]]"#,
            "search.invalid_value",
        ),
        (r#"[["title","is_set","x"]]"#, "search.invalid_value"),
        (r#"[["title","eq"]]"#, "search.invalid_value"),
        (r#"[["transcribed","eq","true"]]"#, "search.invalid_value"),
    ] {
        assert_eq!(code(parse_filter(&SCHEMA, raw, ctx())), want, "{raw}");
    }
    let long = serde_json::json!([["title", "eq", "x".repeat(201)]]).to_string();
    assert_eq!(
        code(parse_filter(&SCHEMA, &long, ctx())),
        "search.invalid_value"
    );
}

#[test]
fn not_filterable_field_is_refused() {
    assert_eq!(
        code(parse_filter(
            &SCHEMA,
            r#"[["secret_sort","eq","x"]]"#,
            ctx()
        )),
        "search.field_not_filterable"
    );
}

#[test]
fn shape_and_limits() {
    for raw in [
        "{",
        "42",
        r#"{"xor":[]}"#,
        r#"{"and":[]}"#,
        r#"{"and":"x"}"#,
        r#"[[1,2,3]]"#,
    ] {
        assert_eq!(
            code(parse_filter(&SCHEMA, raw, ctx())),
            "search.invalid_filter",
            "{raw}"
        );
    }
    // Profundidade 5.
    let deep = r#"{"not":{"not":{"not":{"not":{"not":["title","eq","x"]}}}}}"#;
    assert_eq!(
        code(parse_filter(&SCHEMA, deep, ctx())),
        "search.filter_too_complex"
    );
    // 21 condições.
    let many: Vec<_> = (0..21)
        .map(|_| serde_json::json!(["title", "eq", "x"]))
        .collect();
    assert_eq!(
        code(parse_filter(
            &SCHEMA,
            &serde_json::Value::Array(many).to_string(),
            ctx()
        )),
        "search.filter_too_complex"
    );
    let big_in: Vec<_> = (0..101).map(|i| i.to_string()).collect();
    let raw = serde_json::json!([["title", "in", big_in]]).to_string();
    assert_eq!(
        code(parse_filter(&SCHEMA, &raw, ctx())),
        "search.filter_too_complex"
    );
    let huge = format!(r#"[["title","eq","{}"]]"#, "x".repeat(5000));
    assert_eq!(
        code(parse_filter(&SCHEMA, &huge, ctx())),
        "search.filter_too_complex"
    );
}

#[test]
fn error_details_name_the_path() {
    let e = parse_filter(
        &SCHEMA,
        r#"[["title","eq","x"],{"or":[["nope","eq",1]]}]"#,
        ctx(),
    )
    .unwrap_err();
    assert_eq!(e.details[0].field, "filter[1].or[0]");
}

#[test]
fn text_query_keeps_only_letters_and_digits() {
    let t = parse_text("  Orçamento & 2027 | !x ").unwrap().unwrap();
    assert_eq!(t.tsquery, "orçamento:* & 2027:* & x:*");
    assert_eq!(parse_text("").unwrap(), None);
    assert_eq!(
        parse_text("'):* | !&").unwrap_err().code,
        "search.invalid_query"
    );
    assert_eq!(
        parse_text(&"a".repeat(201)).unwrap_err().code,
        "search.invalid_query"
    );
    let zh = parse_text("会议记录").unwrap().unwrap();
    assert_eq!(zh.terms, vec!["会议记录".to_string()]);
}

#[test]
fn named_filters_same_group_or_different_group_and() {
    let q = compile(
        &SCHEMA,
        &params(&[("filters", "mine,lectures,this_week")]),
        ctx(),
    )
    .unwrap();
    let Some(Node::And(groups)) = q.filter else {
        panic!("{:?}", q.filter)
    };
    assert_eq!(groups.len(), 2);
    assert!(matches!(&groups[0], Node::Or(v) if v.len() == 2));
    assert_eq!(
        compile(&SCHEMA, &params(&[("filters", "mine,ghost")]), ctx())
            .unwrap_err()
            .code,
        "search.unknown_filter"
    );
}

#[test]
fn order_defaults_and_validation() {
    let q = compile(&SCHEMA, &params(&[]), ctx()).unwrap();
    assert_eq!(q.order.len(), 1);
    assert!(q.order[0].desc);
    assert_eq!(q.order[0].name(), "created_at");
    let q = compile(&SCHEMA, &params(&[("q", "abc")]), ctx()).unwrap();
    assert_eq!(q.order[0].target, OrderTarget::Score);
    let q = compile(
        &SCHEMA,
        &params(&[("q", "abc"), ("order_by", "title")]),
        ctx(),
    )
    .unwrap();
    let q2 = compile(
        &SCHEMA,
        &params(&[("q", "abc"), ("order_by", "title,-_score")]),
        ctx(),
    )
    .unwrap();
    assert_eq!(q2.order[1].target, OrderTarget::Score);
    assert_eq!(
        compile(&SCHEMA, &params(&[("order_by", "-_score")]), ctx())
            .unwrap_err()
            .code,
        "search.invalid_order_by"
    );
    assert_eq!(q.order[0].name(), "title");
    for (o, want) in [
        ("category", "search.field_not_sortable"),
        ("nope", "search.unknown_field"),
        ("title,-title", "search.invalid_order_by"),
        (
            "title,duration_secs,created_at,secret_sort",
            "search.invalid_order_by",
        ),
    ] {
        assert_eq!(
            compile(&SCHEMA, &params(&[("order_by", o)]), ctx())
                .unwrap_err()
                .code,
            want,
            "{o}"
        );
    }
}

#[test]
fn group_by_granularity_rules() {
    let q = compile(
        &SCHEMA,
        &params(&[("group_by", "created_at:week,uploader")]),
        ctx(),
    )
    .unwrap();
    assert_eq!(q.group_by[0].granularity, Some(Granularity::Week));
    assert_eq!(q.remaining_group_by(), vec!["uploader".to_string()]);
    let q = compile(&SCHEMA, &params(&[("group_by", "created_at")]), ctx()).unwrap();
    assert_eq!(q.group_by[0].granularity, Some(Granularity::Month));
    for (g, want) in [
        ("category:month", "search.invalid_group_by"),
        ("created_at:fortnight", "search.invalid_group_by"),
        ("title", "search.field_not_groupable"),
        ("ghost", "search.unknown_field"),
        ("category,category", "search.invalid_group_by"),
        (
            "category,uploader,transcribed,created_at",
            "search.invalid_group_by",
        ),
    ] {
        assert_eq!(
            compile(&SCHEMA, &params(&[("group_by", g)]), ctx())
                .unwrap_err()
                .code,
            want,
            "{g}"
        );
    }
}

#[test]
fn page_token_bound_to_the_query_and_the_person() {
    let q = compile(&SCHEMA, &params(&[("filters", "mine")]), ctx()).unwrap();
    let token = encode_keyset(
        &q.fingerprint,
        vec![KeyValue::Text("2026-09-17T10:00:00Z".into())],
        RowId::Uuid(Uuid::from_u128(1)),
        false,
    );
    // Mesma pesquisa: aceite.
    let again = compile(
        &SCHEMA,
        &params(&[("filters", "mine"), ("page_token", &token)]),
        ctx(),
    )
    .unwrap();
    assert!(again.cursor.is_some());
    // Outra pesquisa, outra ordem, outra pessoa: recusado.
    for p in [
        params(&[("filters", "lectures"), ("page_token", &token)]),
        params(&[
            ("filters", "mine"),
            ("order_by", "title"),
            ("page_token", &token),
        ]),
    ] {
        assert_eq!(
            compile(&SCHEMA, &p, ctx()).unwrap_err().code,
            "search.page_token_mismatch"
        );
    }
    let other = Ctx {
        me: Uuid::from_u128(8),
    };
    assert_eq!(
        compile(
            &SCHEMA,
            &params(&[("filters", "mine"), ("page_token", &token)]),
            other
        )
        .unwrap_err()
        .code,
        "search.page_token_mismatch"
    );
    // Token com o tipo de chave errado (texto que não é data).
    let forged = encode_keyset(
        &q.fingerprint,
        vec![KeyValue::Text("ontem".into())],
        RowId::Uuid(Uuid::from_u128(1)),
        false,
    );
    assert_eq!(
        compile(
            &SCHEMA,
            &params(&[("filters", "mine"), ("page_token", &forged)]),
            ctx()
        )
        .unwrap_err()
        .code,
        "search.page_token_mismatch"
    );
    let forged = encode_keyset(
        &q.fingerprint,
        vec![KeyValue::Text("2026-09-17T10:00:00Z".into())],
        RowId::Int(3),
        false,
    );
    assert_eq!(
        compile(
            &SCHEMA,
            &params(&[("filters", "mine"), ("page_token", &forged)]),
            ctx()
        )
        .unwrap_err()
        .code,
        "search.page_token_mismatch"
    );
    assert_eq!(
        compile(&SCHEMA, &params(&[("page_token", "%%%")]), ctx())
            .unwrap_err()
            .code,
        "page.invalid_token"
    );
}

#[test]
fn page_size_reuses_core_page_bounds() {
    assert_eq!(
        compile(&SCHEMA, &params(&[("page_size", "500")]), ctx())
            .unwrap()
            .page_size,
        100
    );
    assert_eq!(compile(&SCHEMA, &params(&[]), ctx()).unwrap().page_size, 50);
}

#[test]
fn is_search_detects_any_parameter() {
    assert!(!ListParams::default().is_search());
    assert!(params(&[("page_size", "10")]).is_search());
    assert!(params(&[("q", "")]).is_search());
}

#[test]
fn saved_query_validation_uses_the_same_codes() {
    let e = validate_saved(
        &SCHEMA,
        None,
        Some(&serde_json::json!([["ghost", "eq", 1]])),
        &[],
        &[],
        &[],
        ctx(),
    )
    .unwrap_err();
    assert_eq!(e.code, "search.unknown_field");
    validate_saved(
        &SCHEMA,
        Some("aula"),
        Some(&serde_json::json!([["category", "eq", "lecture"]])),
        &["this_week".into()],
        &["created_at:month".into()],
        &["-duration_secs".into()],
        ctx(),
    )
    .unwrap();
}

#[test]
fn highlight_is_split_into_segments() {
    let s = format!("…e o {HL_START}orçamento{HL_STOP} do {HL_START}ano{HL_STOP}");
    let segs = highlight_segments(&s);
    assert_eq!(
        segs,
        vec![
            HighlightSegment {
                text: "…e o ".into(),
                is_match: false
            },
            HighlightSegment {
                text: "orçamento".into(),
                is_match: true
            },
            HighlightSegment {
                text: " do ".into(),
                is_match: false
            },
            HighlightSegment {
                text: "ano".into(),
                is_match: true
            },
        ]
    );
    // Marcas soltas não partem nada.
    assert_eq!(
        highlight_segments(&format!("a{HL_STOP}b")),
        vec![HighlightSegment {
            text: "ab".into(),
            is_match: false
        }]
    );
    // Sem HTML: o texto sai tal e qual para a UI escapar.
    assert_eq!(highlight_segments("<b>x</b>")[0].text, "<b>x</b>");
}

#[test]
fn schema_view_lists_operators_granularities_and_group_options() {
    let v = serde_json::to_value(SCHEMA.view("Africa/Luanda")).unwrap();
    assert_eq!(v["timezone"], "Africa/Luanda");
    let created = v["fields"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["name"] == "created_at")
        .unwrap();
    assert_eq!(created["type"], "datetime");
    assert!(created["operators"]
        .as_array()
        .unwrap()
        .contains(&"in_period".into()));
    assert_eq!(created["granularities"].as_array().unwrap().len(), 5);
    let cat = v["fields"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["name"] == "category")
        .unwrap();
    assert_eq!(cat["options"][1]["value"], "lecture");
    assert!(v["group_by"]
        .as_array()
        .unwrap()
        .iter()
        .any(|g| g["value"] == "created_at:quarter"));
    assert_eq!(v["filters"][0]["filter"][0][2], "me");
}

#[test]
fn capped_total_says_which() {
    assert_eq!(capped_total(10, TOTAL_CAP), (10, TotalKind::Exact));
    assert_eq!(
        capped_total(10_001, TOTAL_CAP),
        (10_000, TotalKind::AtLeast)
    );
}
