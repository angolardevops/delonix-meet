//! Pesquisa, filtros, agrupamentos e favoritos (ADR-0007) contra Postgres real.
//!
//! O que se prova: o contrato (`docs/reference/pesquisa.md`), a tolerância a
//! acentos e erros, o keyset estável com empates, o agrupamento por data no
//! fuso da org, e — sobretudo — que nenhum resultado, total, grupo ou
//! favorito atravessa organizações ou chega a quem saiu.
mod common;

use common::{Account, TestApp};
use serde_json::{json, Value};

fn enc(s: &str) -> String {
    url::form_urlencoded::byte_serialize(s.as_bytes()).collect()
}

fn items(v: &Value) -> &Vec<Value> {
    v["items"]
        .as_array()
        .unwrap_or_else(|| panic!("sem items: {v}"))
}

fn ids(v: &Value, key: &str) -> Vec<String> {
    items(v)
        .iter()
        .map(|i| match &i[key] {
            Value::String(s) => s.clone(),
            other => other.to_string(),
        })
        .collect()
}

async fn exec(app: &TestApp, sql: &str, args: &[&str]) {
    let mut q = sqlx::query(sql);
    for a in args {
        q = q.bind(*a);
    }
    q.execute(&app.db).await.unwrap();
}

async fn participate(app: &TestApp, room_id: &str, user_id: &str) {
    exec(
        app,
        "INSERT INTO room_participants (room_id, user_id) VALUES ($1::uuid, $2::uuid) ON CONFLICT DO NOTHING",
        &[room_id, user_id],
    )
    .await;
}

/// Percorre todas as páginas de uma lista pesquisada.
async fn all_pages(app: &TestApp, base: &str, token: &str, key: &str) -> Vec<String> {
    let mut seen = Vec::new();
    let mut page_token: Option<String> = None;
    let sep = if base.contains('?') { '&' } else { '?' };
    for _ in 0..500 {
        let url = match &page_token {
            Some(t) => format!("{base}{sep}page_size=2&page_token={t}"),
            None => format!("{base}{sep}page_size=2"),
        };
        let (st, p) = app.get(&url, Some(token)).await;
        assert_eq!(st, 200, "{url}: {p}");
        seen.extend(ids(&p, key));
        page_token = p["next_page_token"].as_str().map(str::to_string);
        if page_token.is_none() {
            break;
        }
    }
    seen
}

struct World {
    app: TestApp,
    a: Account,
    carla: Account,
    duarte: Account,
    b: Account,
    room_id: String,
    room_code: String,
}

async fn world(db: sqlx::PgPool) -> World {
    let app = TestApp::spawn(db).await;
    let a = app.new_org("alfa.test").await;
    let b = app.new_org("beta.test").await;
    let carla = app.add_member(&a, "carla", "member").await;
    let duarte = app.add_member(&a, "duarte", "member").await;
    let room = app.new_room(&a, "Orçamento anual").await;
    let room_id = room["id"].as_str().unwrap().to_string();
    let room_code = room["code"].as_str().unwrap().to_string();
    participate(&app, &room_id, &a.user_id).await;
    participate(&app, &room_id, &carla.user_id).await;
    World {
        app,
        a,
        carla,
        duarte,
        b,
        room_id,
        room_code,
    }
}

// ---------------------------------------------------------------------------
//  Gravações
// ---------------------------------------------------------------------------

#[sqlx::test(migrations = "./migrations")]
async fn recordings_filters_groups_and_accent_typo_tolerance(db: sqlx::PgPool) {
    let w = world(db).await;
    let app = &w.app;
    let r1 = app.insert_recording(&w.room_id, &w.a.user_id).await;
    let r2 = app.insert_recording(&w.room_id, &w.a.user_id).await;
    let r3 = app.insert_recording(&w.room_id, &w.carla.user_id).await;
    exec(app, "UPDATE recordings SET title = 'Revisão do Orçamento 2027', category = 'lecture', duration_secs = 4000, transcript = 'falámos de química e do orçamento', transcribed_at = now() WHERE id = $1::uuid", &[&r1]).await;
    exec(
        app,
        "UPDATE recordings SET title = 'Planeamento', duration_secs = 100 WHERE id = $1::uuid",
        &[&r2],
    )
    .await;
    exec(app, "UPDATE recordings SET title = 'Aula de física', category = 'lecture', duration_secs = 2000 WHERE id = $1::uuid", &[&r3]).await;

    // Sem acentos, com prefixo e com erro de escrita.
    for q in ["orcamento", "ORÇAM", "orcamneto", "quimica"] {
        let (st, p) = app
            .get(&format!("/api/recordings?q={}", enc(q)), Some(&w.a.token))
            .await;
        assert_eq!(st, 200, "{q}: {p}");
        assert_eq!(ids(&p, "id"), vec![r1.clone()], "{q}: {p}");
        assert!(items(&p)[0]["search"]["highlight"].is_array(), "{p}");
    }
    // O `snippet` da 0045 continua.
    let (_, p) = app.get("/api/recordings?q=quimica", Some(&w.a.token)).await;
    assert!(
        items(&p)[0]["snippet"]
            .as_str()
            .unwrap()
            .contains("«química»"),
        "{p}"
    );

    // Filtro pré-definido + domínio + agrupamento com agregados.
    let (st, p) = app
        .get(
            &format!(
                "/api/recordings?filters=mine&filter={}&group_by=category",
                enc(r#"[["duration_secs","gte",50]]"#)
            ),
            Some(&w.a.token),
        )
        .await;
    assert_eq!(st, 200, "{p}");
    assert_eq!(p["total"], 2, "{p}");
    assert_eq!(p["total_kind"], "exact");
    let groups = p["groups"].as_array().unwrap();
    let lecture = groups.iter().find(|g| g["key"] == "lecture").unwrap();
    assert_eq!(lecture["count"], 1);
    assert_eq!(lecture["label"], "Aula");
    assert_eq!(lecture["aggregates"]["duration_secs"]["sum"], 4000.0);
    assert_eq!(lecture["filter"], json!(["category", "eq", "lecture"]));

    // Abrir o grupo = juntar o filtro dele.
    let (_, p) = app
        .get(
            &format!(
                "/api/recordings?filters=mine&filter={}",
                enc(&json!({"and": [["duration_secs","gte",50], lecture["filter"]]}).to_string())
            ),
            Some(&w.a.token),
        )
        .await;
    assert_eq!(ids(&p, "id"), vec![r1.clone()]);

    // Mesmo grupo de filtros = OU («As minhas» OU «Partilhadas comigo»).
    let (_, p) = app
        .get(
            "/api/recordings?filters=mine,shared_with_me",
            Some(&w.carla.token),
        )
        .await;
    assert_eq!(ids(&p, "id"), vec![r3.clone()], "{p}");

    // Relevância só quando pedida.
    let (st, p) = app
        .get(
            "/api/recordings?q=orcamento&order_by=-_score",
            Some(&w.a.token),
        )
        .await;
    assert_eq!(st, 200, "{p}");
    assert!(items(&p)[0]["search"]["score"].as_f64().unwrap() > 0.0);
}

#[sqlx::test(migrations = "./migrations")]
async fn recordings_keyset_is_stable_with_ties_and_never_repeats(db: sqlx::PgPool) {
    let w = world(db).await;
    let app = &w.app;
    let mut created = Vec::new();
    for _ in 0..7 {
        created.push(app.insert_recording(&w.room_id, &w.a.user_id).await);
    }
    // Todos com o MESMO created_at e a mesma duração: só o id desempata.
    exec(
        app,
        "UPDATE recordings SET created_at = '2026-09-01T10:00:00Z', duration_secs = 60",
        &[],
    )
    .await;
    for order in [
        "",
        "&order_by=-duration_secs",
        "&order_by=title,-created_at",
    ] {
        let seen = all_pages(
            app,
            &format!("/api/recordings?filters=mine{order}"),
            &w.a.token,
            "id",
        )
        .await;
        let mut uniq = seen.clone();
        uniq.sort();
        uniq.dedup();
        assert_eq!(seen.len(), 7, "{order}: {seen:?}");
        assert_eq!(uniq.len(), 7, "{order}: repetiu");
    }
    // Token de uma pesquisa usado noutra: 400, nunca uma página errada.
    let (_, p) = app
        .get("/api/recordings?filters=mine&page_size=2", Some(&w.a.token))
        .await;
    let t = p["next_page_token"].as_str().unwrap();
    let (st, e) = app
        .get(
            &format!("/api/recordings?filters=shared_with_me&page_size=2&page_token={t}"),
            Some(&w.a.token),
        )
        .await;
    assert_eq!(st, 400);
    assert_eq!(e["code"], "search.page_token_mismatch");
    // E por outra pessoa.
    let (st, e) = app
        .get(
            &format!("/api/recordings?filters=mine&page_size=2&page_token={t}"),
            Some(&w.carla.token),
        )
        .await;
    assert_eq!(st, 400, "{e}");
}

/// A pesquisa e a biblioteca herdada concordam, pessoa a pessoa — incluindo
/// o colega que não esteve na sala, o arquivado e a outra organização.
#[sqlx::test(migrations = "./migrations")]
async fn recordings_visibility_matches_the_library(db: sqlx::PgPool) {
    let w = world(db).await;
    let app = &w.app;
    let mine = app.insert_recording(&w.room_id, &w.a.user_id).await;
    let other_room = app.new_room(&w.duarte, "sala do duarte").await;
    let d_rec = app
        .insert_recording(other_room["id"].as_str().unwrap(), &w.duarte.user_id)
        .await;
    // Partilhada com a carla.
    exec(app, "INSERT INTO recording_shares (recording_id, user_id, shared_by) VALUES ($1::uuid, $2::uuid, $3::uuid)", &[&d_rec, &w.carla.user_id, &w.duarte.user_id]).await;
    let b_room = app.new_room(&w.b, "beta").await;
    app.insert_recording(b_room["id"].as_str().unwrap(), &w.b.user_id)
        .await;

    for who in [&w.a, &w.carla, &w.duarte, &w.b] {
        let (_, lib) = app.get("/api/recordings", Some(&who.token)).await;
        let mut legacy: Vec<String> = lib
            .as_array()
            .unwrap()
            .iter()
            .map(|r| r["id"].as_str().unwrap().to_string())
            .collect();
        let mut searched = all_pages(
            app,
            "/api/recordings?order_by=-created_at",
            &who.token,
            "id",
        )
        .await;
        legacy.sort();
        searched.sort();
        assert_eq!(searched, legacy, "{}", who.email);
        let (_, p) = app
            .get("/api/recordings?page_size=1", Some(&who.token))
            .await;
        assert_eq!(p["total"], legacy.len(), "o total também: {}", who.email);
    }
    // Arquivada, a carla deixa de ver tudo — itens, total e grupos.
    app.archive_member(w.a.org(), &w.carla.user_id).await;
    let (_, p) = app
        .get("/api/recordings?group_by=uploader", Some(&w.carla.token))
        .await;
    assert_eq!(p["total"], 0, "{p}");
    assert_eq!(p["groups"], json!([]), "{p}");
    // Filtrar pelo autor de outra org não sonda nada.
    let (st, p) = app
        .get(
            &format!(
                "/api/recordings?filter={}",
                enc(&json!([["uploader", "eq", w.a.user_id]]).to_string())
            ),
            Some(&w.b.token),
        )
        .await;
    assert_eq!(st, 200);
    assert_eq!(p["total"], 0, "{p}");
    let _ = mine;
}

#[sqlx::test(migrations = "./migrations")]
async fn whitelist_errors_have_stable_codes_and_injection_is_inert(db: sqlx::PgPool) {
    let w = world(db).await;
    let app = &w.app;
    for (query, code) in [
        (
            format!("filter={}", enc(r#"[["password_hash","eq","x"]]"#)),
            "search.unknown_field",
        ),
        (
            format!("filter={}", enc(r#"[["title\" OR 1=1 --","eq","x"]]"#)),
            "search.unknown_field",
        ),
        (
            format!("filter={}", enc(r#"[["title","gte","x"]]"#)),
            "search.invalid_operator",
        ),
        (
            format!("filter={}", enc(r#"[["category","eq","secret"]]"#)),
            "search.invalid_value",
        ),
        ("filter=%7Bnope".to_string(), "search.invalid_filter"),
        ("filters=ghost".to_string(), "search.unknown_filter"),
        ("group_by=title".to_string(), "search.field_not_groupable"),
        (
            "group_by=category:month".to_string(),
            "search.invalid_group_by",
        ),
        ("order_by=category".to_string(), "search.field_not_sortable"),
        ("order_by=-_score".to_string(), "search.invalid_order_by"),
        ("q=%21%21%21".to_string(), "recording.invalid_query"),
        ("page_token=%25%25%25".to_string(), "page.invalid_token"),
    ] {
        let (st, e) = app
            .get(&format!("/api/recordings?{query}"), Some(&w.a.token))
            .await;
        assert_eq!(st, 400, "{query}: {e}");
        assert_eq!(e["code"], code, "{query}: {e}");
        assert!(e["request_id"].is_string() || e["request_id"].is_null());
    }
    // Valor hostil: é só texto, e a base continua lá.
    let evil = r#"[["title","contains","x'); DROP TABLE users; --"],["room_code","in",["'; DELETE FROM recordings; --"]]]"#;
    let (st, p) = app
        .get(
            &format!(
                "/api/recordings?filter={}&q={}",
                enc(evil),
                enc("'); DROP TABLE users; --")
            ),
            Some(&w.a.token),
        )
        .await;
    assert_eq!(st, 200, "{p}");
    assert_eq!(p["total"], 0);
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM users")
        .fetch_one(&app.db)
        .await
        .unwrap();
    assert!(n >= 4);
    // Curingas do LIKE são literais.
    let rec = app.insert_recording(&w.room_id, &w.a.user_id).await;
    exec(
        app,
        "UPDATE recordings SET title = 'cem por cento' WHERE id = $1::uuid",
        &[&rec],
    )
    .await;
    let (_, p) = app
        .get(
            &format!(
                "/api/recordings?filter={}",
                enc(r#"[["title","contains","%"]]"#)
            ),
            Some(&w.a.token),
        )
        .await;
    assert_eq!(p["total"], 0, "`%` não é curinga: {p}");
}

// ---------------------------------------------------------------------------
//  Reuniões — fuso da organização
// ---------------------------------------------------------------------------

#[sqlx::test(migrations = "./migrations")]
async fn meetings_group_by_day_uses_the_org_timezone(db: sqlx::PgPool) {
    let w = world(db).await;
    let app = &w.app;
    let m1 = app
        .new_meeting(&w.a, "Revisão semanal", &[&w.carla.user_id])
        .await;
    let m2 = app.new_meeting(&w.a, "Orçamento", &[]).await;
    let m1 = m1["id"].as_str().unwrap().to_string();
    let m2 = m2["id"].as_str().unwrap().to_string();
    // 23:30 UTC de 1 de Março = 00:30 de 2 de Março em Luanda (UTC+1).
    exec(
        app,
        "UPDATE meetings SET starts_at = '2026-03-01T23:30:00Z' WHERE id = $1::uuid",
        &[&m1],
    )
    .await;
    exec(app, "UPDATE meetings SET starts_at = '2026-03-01T10:00:00Z', duration_min = 45 WHERE id = $1::uuid", &[&m2]).await;

    let (st, p) = app
        .get("/api/meetings?group_by=starts_at:day", Some(&w.a.token))
        .await;
    assert_eq!(st, 200, "{p}");
    let keys: Vec<&str> = p["groups"]
        .as_array()
        .unwrap()
        .iter()
        .map(|g| g["key"].as_str().unwrap())
        .collect();
    assert_eq!(keys, vec!["2026-03-01", "2026-03-02"], "{p}");
    let g2 = &p["groups"][1];
    assert_eq!(g2["range"]["from"], "2026-03-01T23:00:00Z");
    assert_eq!(g2["range"]["to"], "2026-03-02T23:00:00Z");

    // Noutro fuso, o mesmo instante cai noutro dia.
    exec(
        app,
        "UPDATE organizations SET timezone = 'UTC' WHERE id = $1::uuid",
        &[w.a.org()],
    )
    .await;
    let (_, p) = app
        .get("/api/meetings?group_by=starts_at:day", Some(&w.a.token))
        .await;
    let keys: Vec<&str> = p["groups"]
        .as_array()
        .unwrap()
        .iter()
        .map(|g| g["key"].as_str().unwrap())
        .collect();
    assert_eq!(keys, vec!["2026-03-01"], "{p}");
    assert_eq!(p["groups"][0]["count"], 2);
    assert_eq!(
        p["groups"][0]["aggregates"]["duration_min"]["sum"], 75.0,
        "{p}"
    );

    // Convidada: vê a m1 com «Por responder»; a outra org não vê nada.
    let (_, p) = app
        .get(
            "/api/meetings?filters=pending_response",
            Some(&w.carla.token),
        )
        .await;
    assert_eq!(ids(&p, "id"), vec![m1.clone()], "{p}");
    let (_, p) = app.get("/api/meetings?q=orcamento", Some(&w.b.token)).await;
    assert_eq!(p["total"], 0);
    let (_, p) = app.get("/api/meetings?q=orcamento", Some(&w.a.token)).await;
    assert_eq!(ids(&p, "id"), vec![m2.clone()], "{p}");
    // Período relativo e semana ISO não rebentam.
    let (st, p) = app
        .get(
            "/api/meetings?filters=this_week,upcoming&group_by=starts_at:week",
            Some(&w.a.token),
        )
        .await;
    assert_eq!(st, 200, "{p}");
}

// ---------------------------------------------------------------------------
//  Membros, quadros, auditoria — isolamento por organização
// ---------------------------------------------------------------------------

#[sqlx::test(migrations = "./migrations")]
async fn members_whiteboards_audit_stay_inside_the_org(db: sqlx::PgPool) {
    let w = world(db).await;
    let app = &w.app;
    let org = w.a.org().to_string();

    let (st, p) = app
        .get(
            &format!("/api/orgs/{org}/members?q=carl&group_by=role"),
            Some(&w.duarte.token),
        )
        .await;
    assert_eq!(st, 200, "{p}");
    assert_eq!(ids(&p, "user_id"), vec![w.carla.user_id.clone()]);
    let (_, p) = app
        .get(
            &format!("/api/orgs/{org}/members?group_by=role"),
            Some(&w.duarte.token),
        )
        .await;
    let admins = p["groups"]
        .as_array()
        .unwrap()
        .iter()
        .find(|g| g["key"] == "admin")
        .unwrap();
    assert_eq!(admins["count"], 1);
    for path in [
        format!("/api/orgs/{org}/members?q=carla"),
        format!("/api/orgs/{org}/audit-events?filters=logins"),
    ] {
        let (st, e) = app.get(&path, Some(&w.b.token)).await;
        assert!(st == 403 || st == 404, "{path}: {st} {e}");
        assert!(!e.to_string().contains("carla"), "{e}");
    }
    // Auditoria: só admin; filtros e grupos.
    let (st, _) = app
        .get(
            &format!("/api/orgs/{org}/audit-events?filters=logins"),
            Some(&w.carla.token),
        )
        .await;
    assert_eq!(st, 403);
    let (st, p) = app
        .get(
            &format!("/api/orgs/{org}/audit-events?filters=logins&group_by=actor"),
            Some(&w.a.token),
        )
        .await;
    assert_eq!(st, 200, "{p}");
    assert!(p["total"].as_i64().unwrap() >= 1, "{p}");
    assert!(items(&p)
        .iter()
        .all(|e| e["action"].as_str().unwrap().starts_with("auth.login")));
    let seen = all_pages(
        app,
        &format!("/api/orgs/{org}/audit-events?order_by=-created_at"),
        &w.a.token,
        "id",
    )
    .await;
    let _ = seen;

    // Quadros.
    let png = common::PNG_1X1;
    let (st, wb) = app
        .post(
            "/api/whiteboards",
            Some(&w.a.token),
            json!({"title": "Arquitectura da rede", "room_code": w.room_code, "png_base64": png}),
        )
        .await;
    assert!(st < 300, "{wb}");
    let (_, p) = app
        .get("/api/whiteboards?q=arquitetura", Some(&w.carla.token))
        .await;
    assert_eq!(p["total"], 1, "typo/acento: {p}");
    let (_, p) = app
        .get("/api/whiteboards?q=arquitectura", Some(&w.b.token))
        .await;
    assert_eq!(p["total"], 0, "{p}");
}

// ---------------------------------------------------------------------------
//  Ctrl+K
// ---------------------------------------------------------------------------

#[sqlx::test(migrations = "./migrations")]
async fn global_search_finds_deep_content_and_never_crosses_orgs(db: sqlx::PgPool) {
    let w = world(db).await;
    let app = &w.app;
    let rec = app.insert_recording(&w.room_id, &w.a.user_id).await;
    exec(app, "UPDATE recordings SET title = 'Comité', transcript = 'o zebróide apareceu na reunião' WHERE id = $1::uuid", &[&rec]).await;
    exec(app, "INSERT INTO recording_chapters (recording_id, at_secs, title, created_by) VALUES ($1::uuid, 754, 'Capítulo do hipopótamo', $2::uuid)", &[&rec, &w.a.user_id]).await;
    exec(app, "INSERT INTO room_chat_messages (room_id, user_id, username, message) VALUES ($1::uuid, $2::uuid, 'carla', 'combinamos o girassol amanhã')", &[&w.room_id, &w.carla.user_id]).await;
    app.new_meeting(&w.a, "Girassol trimestral", &[]).await;
    exec(app, "INSERT INTO org_webhooks (org_id, kind, url, created_by) VALUES ($1::uuid, 'slack', 'https://hooks.slack.com/services/SEGREDOXYZ', $2::uuid)", &[w.a.org(), &w.a.user_id]).await;

    let (st, r) = app.get("/api/search?q=zebroide", Some(&w.a.token)).await;
    assert_eq!(st, 200, "{r}");
    let g = &r["groups"][0];
    assert_eq!(g["type"], "recordings", "{r}");
    assert_eq!(g["items"][0]["matched_in"], "transcript");
    assert!(
        g["items"][0]["highlight"]
            .as_array()
            .unwrap()
            .iter()
            .any(|s| s["match"] == true),
        "{r}"
    );

    let (_, r) = app.get("/api/search?q=hipopotamo", Some(&w.a.token)).await;
    let hit = &r["groups"][0]["items"][0];
    assert_eq!(hit["matched_in"], "chapter", "{r}");
    assert_eq!(hit["target"]["at_secs"], 754);

    let (_, r) = app
        .get("/api/search?q=girassol", Some(&w.carla.token))
        .await;
    let types: Vec<&str> = r["groups"]
        .as_array()
        .unwrap()
        .iter()
        .map(|g| g["type"].as_str().unwrap())
        .collect();
    assert!(types.contains(&"messages"), "{r}");
    let (_, r) = app.get("/api/search?q=girassol", Some(&w.a.token)).await;
    let types: Vec<&str> = r["groups"]
        .as_array()
        .unwrap()
        .iter()
        .map(|g| g["type"].as_str().unwrap())
        .collect();
    assert!(
        types.contains(&"meetings") && types.contains(&"messages"),
        "{r}"
    );

    // Pessoas por erro de escrita; salas pelo código.
    let (_, r) = app
        .get("/api/search?q=carrla&types=people", Some(&w.duarte.token))
        .await;
    assert_eq!(r["groups"][0]["items"][0]["id"], w.carla.user_id, "{r}");
    let (_, r) = app
        .get(
            &format!("/api/search?q={}&types=rooms", w.room_code),
            Some(&w.carla.token),
        )
        .await;
    assert_eq!(
        r["groups"][0]["items"][0]["target"]["room_code"], w.room_code,
        "{r}"
    );
    // O Duarte é colega mas nunca esteve na sala: não descobre o código.
    let (_, r) = app
        .get(
            &format!("/api/search?q={}&types=rooms", w.room_code),
            Some(&w.duarte.token),
        )
        .await;
    assert_eq!(r["groups"], json!([]), "{r}");

    // Webhooks: só admin, e só pelo anfitrião — o caminho é segredo.
    let (_, r) = app
        .get("/api/search?q=hooks.slack&types=webhooks", Some(&w.a.token))
        .await;
    assert_eq!(r["groups"][0]["type"], "webhooks", "{r}");
    assert!(!r.to_string().contains("SEGREDOXYZ"), "{r}");
    let (_, r) = app
        .get("/api/search?q=segredoxyz&types=webhooks", Some(&w.a.token))
        .await;
    assert_eq!(r["groups"], json!([]), "{r}");
    let (_, r) = app
        .get(
            "/api/search?q=slack&types=webhooks,meetings",
            Some(&w.carla.token),
        )
        .await;
    assert_eq!(
        r["skipped"],
        json!([{"type": "webhooks", "code": "search.forbidden"}]),
        "{r}"
    );

    // A outra organização não encontra NADA disto.
    for q in [
        "zebroide",
        "hipopotamo",
        "girassol",
        "carla",
        "slack",
        &w.room_code,
    ] {
        let (st, r) = app
            .get(&format!("/api/search?q={}&types=meetings,recordings,people,whiteboards,rooms,messages,stream_destinations,webhooks,audit_events", enc(q)), Some(&w.b.token))
            .await;
        assert_eq!(st, 200, "{r}");
        assert_eq!(r["groups"], json!([]), "{q}: {r}");
    }
    // Quem saiu também não.
    app.archive_member(w.a.org(), &w.carla.user_id).await;
    for q in ["zebroide", "girassol", "duarte"] {
        let (_, r) = app
            .get(&format!("/api/search?q={q}"), Some(&w.carla.token))
            .await;
        assert_eq!(r["groups"], json!([]), "{q}: {r}");
    }
    let (st, e) = app.get("/api/search?q=%21%21", Some(&w.a.token)).await;
    assert_eq!(
        (st, e["code"].as_str()),
        (400, Some("search.invalid_query"))
    );
    let (st, e) = app
        .get("/api/search?q=abc&types=passwords", Some(&w.a.token))
        .await;
    assert_eq!(
        (st, e["code"].as_str()),
        (400, Some("search.invalid_types"))
    );
}

// ---------------------------------------------------------------------------
//  Descrição e favoritos
// ---------------------------------------------------------------------------

#[sqlx::test(migrations = "./migrations")]
async fn schemas_describe_the_panel(db: sqlx::PgPool) {
    let w = world(db).await;
    let app = &w.app;
    let (st, s) = app
        .get("/api/search/schemas/recordings", Some(&w.carla.token))
        .await;
    assert_eq!(st, 200, "{s}");
    assert_eq!(s["timezone"], "Africa/Luanda");
    assert!(s["filters"]
        .as_array()
        .unwrap()
        .iter()
        .any(|f| f["name"] == "transcribed"));
    let (_, list) = app.get("/api/search/schemas", Some(&w.carla.token)).await;
    let names: Vec<&str> = list["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["resource"].as_str().unwrap())
        .collect();
    assert!(
        !names.contains(&"audit_events"),
        "membro não vê o schema da auditoria: {names:?}"
    );
    let (_, list) = app.get("/api/search/schemas", Some(&w.a.token)).await;
    assert_eq!(list["items"].as_array().unwrap().len(), 5);
    let (st, e) = app
        .get("/api/search/schemas/passwords", Some(&w.a.token))
        .await;
    assert_eq!(
        (st, e["code"].as_str()),
        (404, Some("search.unknown_resource"))
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn saved_searches_crud_sharing_and_isolation(db: sqlx::PgPool) {
    let w = world(db).await;
    let app = &w.app;
    let base = "/api/users/me/saved-searches";
    let body = json!({"resource": "recordings", "name": "Aulas longas",
        "query": {"filter": [["category","eq","lecture"]], "filters": ["long"], "group_by": ["uploader"]},
        "shared": true, "is_default": true});
    let res = app
        .http
        .post(app.url(base))
        .bearer_auth(&w.a.token)
        .json(&body)
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 201);
    let location = res.headers()["location"].to_str().unwrap().to_string();
    let created: Value = res.json().await.unwrap();
    assert_eq!(
        location,
        format!("{base}/{}", created["id"].as_str().unwrap())
    );
    assert_eq!(created["valid"], true);
    assert_eq!(created["editable"], true);

    let (st, e) = app.post(base, Some(&w.a.token), body.clone()).await;
    assert_eq!(
        (st, e["code"].as_str()),
        (409, Some("saved_search.duplicate_name"))
    );
    let (st, e) = app
        .post(
            base,
            Some(&w.a.token),
            json!({"resource": "recordings", "name": "x", "query": {"filter": [["ghost","eq",1]]}}),
        )
        .await;
    assert_eq!(
        (st, e["code"].as_str()),
        (400, Some("search.unknown_field"))
    );

    // Partilhado: a colega vê, não altera; a outra org nem vê.
    let (_, p) = app
        .get(&format!("{base}?resource=recordings"), Some(&w.carla.token))
        .await;
    assert_eq!(items(&p).len(), 1, "{p}");
    assert_eq!(items(&p)[0]["editable"], false);
    let (st, e) = app
        .patch(&location, Some(&w.carla.token), json!({"name": "meu"}))
        .await;
    assert_eq!(
        (st, e["code"].as_str()),
        (403, Some("saved_search.not_owner"))
    );
    let (st, _) = app.delete(&location, Some(&w.carla.token)).await;
    assert_eq!(st, 403);
    for (method, st_ok) in [("get", 404), ("patch", 404), ("delete", 404)] {
        let (st, _) = match method {
            "get" => app.get(&location, Some(&w.b.token)).await,
            "patch" => {
                app.patch(&location, Some(&w.b.token), json!({"name": "roubado"}))
                    .await
            }
            _ => app.delete(&location, Some(&w.b.token)).await,
        };
        assert_eq!(st, st_ok, "{method}");
    }
    let (_, p) = app.get(base, Some(&w.b.token)).await;
    assert_eq!(items(&p).len(), 0);

    // Arquivada, a colega deixa de ver o partilhado.
    app.archive_member(w.a.org(), &w.carla.user_id).await;
    let (_, p) = app.get(base, Some(&w.carla.token)).await;
    assert_eq!(items(&p).len(), 0, "{p}");

    // Por omissão: só um por pessoa e recurso.
    let (st, second) = app
        .post(base, Some(&w.a.token), json!({"resource": "recordings", "name": "Minhas", "query": {"filters": ["mine"]}, "is_default": true}))
        .await;
    assert_eq!(st, 201, "{second}");
    let (_, first) = app.get(&location, Some(&w.a.token)).await;
    assert_eq!(first["is_default"], false);
    let (st, upd) = app
        .patch(
            &location,
            Some(&w.a.token),
            json!({"name": "Aulas muito longas", "shared": false}),
        )
        .await;
    assert_eq!(st, 200, "{upd}");
    assert_eq!(upd["name"], "Aulas muito longas");
    let (st, _) = app.delete(&location, Some(&w.a.token)).await;
    assert_eq!(st, 204);
    let (st, _) = app.get(&location, Some(&w.a.token)).await;
    assert_eq!(st, 404);
}
