//! Opções de sessão das reuniões agendadas (R184): `format`, `waiting_room`,
//! `auto_record`, `record_quality` — criar pela BFF e pela v1 com as mesmas
//! regras, ler na lista e no `GET`, alterar nos dois `PATCH`, passar à sala no
//! arranque, gravação automática quando o anfitrião entra, e a resposta
//! `tentative` ao convite.
mod common;

use std::time::Duration;

use chrono::Utc;
use common::{jwt_claims, TestApp};
use futures_util::StreamExt;
use serde_json::{json, Value};

fn in_hours(h: i64) -> String {
    (Utc::now() + chrono::Duration::hours(h)).to_rfc3339()
}

async fn v1(
    app: &TestApp,
    method: reqwest::Method,
    path: &str,
    key: &str,
    body: Option<Value>,
) -> (u16, Value) {
    let auth = format!("Bearer {key}");
    let r = app
        .raw(
            method,
            &format!("/api/v1{path}"),
            &[("Authorization", &auth)],
            body,
        )
        .await;
    (r.status, r.json())
}

fn options_of(v: &Value) -> Value {
    json!({
        "format": v["format"],
        "waiting_room": v["waiting_room"],
        "auto_record": v["auto_record"],
        "record_quality": v["record_quality"],
    })
}

fn defaults() -> Value {
    json!({"format": "meeting", "waiting_room": false, "auto_record": false, "record_quality": "1080p"})
}

async fn room_row(app: &TestApp, code: &str) -> (bool, String, bool, Option<String>) {
    sqlx::query_as(
        "SELECT waiting_room, format, auto_record, record_quality FROM rooms WHERE code = $1",
    )
    .bind(code)
    .fetch_one(&app.db)
    .await
    .unwrap()
}

// ---------------------------------------------------------------------------
//  Criar: omissões, opções, validação — BFF e v1 com as mesmas regras
// ---------------------------------------------------------------------------

#[sqlx::test(migrations = "./migrations")]
async fn create_with_and_without_options_bff_and_v1(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let a = app.new_org("alfa.test").await;
    let (_, key) = app.api_key(&a).await;
    let starts = in_hours(2);

    // BFF sem opções: as omissões.
    let (st, m) = app
        .post(
            "/api/meetings",
            Some(&a.token),
            json!({"title": "Sem opções", "starts_at": starts}),
        )
        .await;
    assert_eq!(st, 200, "{m}");
    assert_eq!(options_of(&m), defaults());
    assert_eq!(m["conflicts"], json!({"participants": [], "room": []}));

    // BFF com opções: devolvidas e gravadas.
    let opts = json!({"format": "training", "waiting_room": true, "auto_record": true, "record_quality": "720p"});
    let mut body = json!({"title": "Com opções", "starts_at": starts});
    body.as_object_mut()
        .unwrap()
        .extend(opts.as_object().unwrap().clone());
    let (st, m) = app
        .post("/api/meetings", Some(&a.token), body.clone())
        .await;
    assert_eq!(st, 200, "{m}");
    assert_eq!(options_of(&m), opts);
    let stored: (String, bool, bool, String) = sqlx::query_as(
        "SELECT format, waiting_room, auto_record, record_quality FROM meetings WHERE id = $1::uuid",
    )
    .bind(m["id"].as_str().unwrap())
    .fetch_one(&app.db)
    .await
    .unwrap();
    assert_eq!(stored, ("training".into(), true, true, "720p".into()));

    // v1 sem opções: as mesmas omissões; a sala nasce já com elas.
    let (st, v) = v1(
        &app,
        reqwest::Method::POST,
        "/meetings",
        &key,
        Some(json!({"title": "v1 sem", "starts_at": starts, "host_email": a.email})),
    )
    .await;
    assert_eq!(st, 200, "{v}");
    assert_eq!(options_of(&v), defaults());
    let code = v["room_code"].as_str().unwrap();
    assert_eq!(
        room_row(&app, code).await,
        (false, "normal".into(), false, Some("1080p".into()))
    );

    // v1 com as mesmas opções da BFF: a mesma resposta, e a sala cumpre-as.
    let mut vbody = json!({"title": "v1 com", "starts_at": starts, "host_email": a.email});
    vbody
        .as_object_mut()
        .unwrap()
        .extend(opts.as_object().unwrap().clone());
    let (st, v) = v1(&app, reqwest::Method::POST, "/meetings", &key, Some(vbody)).await;
    assert_eq!(st, 200, "{v}");
    assert_eq!(options_of(&v), opts, "paridade BFF/v1");
    let code = v["room_code"].as_str().unwrap();
    assert_eq!(
        room_row(&app, code).await,
        (true, "training".into(), true, Some("720p".into()))
    );
    // E o GET da v1 devolve-as.
    let (st, g) = v1(
        &app,
        reqwest::Method::GET,
        &format!("/meetings/{}", v["id"].as_str().unwrap()),
        &key,
        None,
    )
    .await;
    assert_eq!(st, 200, "{g}");
    assert_eq!(options_of(&g), opts);
}

#[sqlx::test(migrations = "./migrations")]
async fn invalid_options_are_refused_with_the_same_codes_on_both_surfaces(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let a = app.new_org("alfa.test").await;
    let (_, key) = app.api_key(&a).await;
    let starts = in_hours(2);

    for (extra, code, field) in [
        (
            json!({"format": "webinar"}),
            "meeting.invalid_format",
            "format",
        ),
        (
            json!({"format": "Meeting"}),
            "meeting.invalid_format",
            "format",
        ),
        (
            json!({"record_quality": "4k"}),
            "meeting.invalid_record_quality",
            "record_quality",
        ),
    ] {
        let mut bff = json!({"title": "x", "starts_at": starts});
        bff.as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        let (st, body) = app.post("/api/meetings", Some(&a.token), bff).await;
        assert_eq!(st, 400, "BFF {extra}: {body}");
        assert_eq!(body["code"], code, "BFF {extra}: {body}");
        assert_eq!(body["details"][0]["field"], field);

        let mut v = json!({"title": "x", "starts_at": starts, "host_email": a.email});
        v.as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        let (st, body) = v1(&app, reqwest::Method::POST, "/meetings", &key, Some(v)).await;
        assert_eq!(st, 400, "v1 {extra}: {body}");
        assert_eq!(body["code"], code, "v1 {extra}: {body}");
    }
    // Um tipo errado não chega à regra: o JSON é recusado antes.
    let (st, _) = app
        .post(
            "/api/meetings",
            Some(&a.token),
            json!({"title": "x", "starts_at": starts, "auto_record": "sim"}),
        )
        .await;
    assert!(st == 400 || st == 422, "{st}");

    // Nada foi criado por nenhum dos pedidos recusados.
    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM meetings")
        .fetch_one(&app.db)
        .await
        .unwrap();
    assert_eq!(n, 0);

    // v1: gravação automática numa sala E2EE é recusada, não aceite e ignorada.
    let (st, body) = v1(
        &app,
        reqwest::Method::POST,
        "/meetings",
        &key,
        Some(
            json!({"title": "x", "starts_at": starts, "host_email": a.email,
                    "e2ee": true, "auto_record": true}),
        ),
    )
    .await;
    assert_eq!(st, 422, "{body}");
    assert_eq!(body["code"], "meeting.auto_record_e2ee");
    let rooms: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM rooms")
        .fetch_one(&app.db)
        .await
        .unwrap();
    assert_eq!(rooms, 0, "a recusa vem antes de criar a sala");
}

// ---------------------------------------------------------------------------
//  Ler: lista e GET com invitee_count, external_source e opções
// ---------------------------------------------------------------------------

#[sqlx::test(migrations = "./migrations")]
async fn list_and_get_carry_options_count_and_source(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let a = app.new_org("alfa.test").await;
    let c = app.add_member(&a, "carla", "member").await;
    let d = app.add_member(&a, "dario", "member").await;
    let (_, key) = app.api_key(&a).await;
    let starts = in_hours(3);

    let (_, bff) = app
        .post(
            "/api/meetings",
            Some(&a.token),
            json!({"title": "BFF", "starts_at": starts, "invitee_ids": [c.user_id, d.user_id],
                   "format": "broadcast", "record_quality": "audio"}),
        )
        .await;
    let (st, odoo) = v1(
        &app,
        reqwest::Method::POST,
        "/meetings",
        &key,
        Some(
            json!({"title": "Odoo", "starts_at": in_hours(4), "host_email": a.email,
                    "external_ref": "odoo:kaeso_prod:calendar.event:4821",
                    "invitees": [{"email": c.email}]}),
        ),
    )
    .await;
    assert_eq!(st, 200, "{odoo}");
    let (st, opaque) = v1(
        &app,
        reqwest::Method::POST,
        "/meetings",
        &key,
        Some(
            json!({"title": "Opaca", "starts_at": in_hours(5), "host_email": a.email,
                    "external_ref": "evento 77"}),
        ),
    )
    .await;
    assert_eq!(st, 200, "{opaque}");

    let (st, list) = app.get("/api/meetings", Some(&a.token)).await;
    assert_eq!(st, 200, "{list}");
    let by_title = |t: &str| {
        list.as_array()
            .unwrap()
            .iter()
            .find(|m| m["title"] == t)
            .unwrap_or_else(|| panic!("{t} não está na lista: {list}"))
            .clone()
    };
    let m = by_title("BFF");
    assert_eq!(m["invitee_count"], 2);
    assert!(m["external_source"].is_null(), "criada no Meet: {m}");
    assert_eq!(
        options_of(&m),
        json!({"format": "broadcast", "waiting_room": false, "auto_record": false, "record_quality": "audio"})
    );
    let m = by_title("Odoo");
    assert_eq!(m["invitee_count"], 1);
    assert_eq!(m["external_source"], "odoo");
    assert!(
        m.get("external_ref").is_none(),
        "a referência inteira não sai pela BFF"
    );
    assert_eq!(by_title("Opaca")["external_source"], "api");

    // GET: os mesmos campos, mais os de `Meeting`.
    let id = bff["id"].as_str().unwrap();
    let (st, g) = app
        .get(&format!("/api/meetings/{id}"), Some(&a.token))
        .await;
    assert_eq!(st, 200, "{g}");
    assert_eq!(g["my_status"], "owner");
    assert_eq!(g["invitee_count"], 2);
    assert!(g["external_source"].is_null());
    assert_eq!(g["format"], "broadcast");
    assert_eq!(g["record_quality"], "audio");
    assert!(
        g.get("transcript").is_some(),
        "continua a ser o recurso completo"
    );
    let (st, g) = app
        .get(
            &format!("/api/meetings/{}", odoo["id"].as_str().unwrap()),
            Some(&c.token),
        )
        .await;
    assert_eq!(st, 200, "{g}");
    assert_eq!(g["my_status"], "pending");
    assert_eq!(g["external_source"], "odoo");

    // Outra organização: 404, na lista não aparece.
    let b = app.new_org("beta.test").await;
    let (st, _) = app
        .get(&format!("/api/meetings/{id}"), Some(&b.token))
        .await;
    assert_eq!(st, 404);
    let (_, lb) = app.get("/api/meetings", Some(&b.token)).await;
    assert_eq!(lb, json!([]));
}

// ---------------------------------------------------------------------------
//  RSVP «talvez»
// ---------------------------------------------------------------------------

#[sqlx::test(migrations = "./migrations")]
async fn tentative_rsvp_needs_no_reason_and_is_listed(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let a = app.new_org("alfa.test").await;
    let c = app.add_member(&a, "carla", "member").await;
    let m = app.new_meeting(&a, "Talvez", &[&c.user_id]).await;
    let id = m["id"].as_str().unwrap();

    let (st, body) = app
        .put(
            &format!("/api/meetings/{id}/invitees/me"),
            Some(&c.token),
            json!({"status": "tentative"}),
        )
        .await;
    assert_eq!(st, 200, "sem motivo: {body}");
    assert_eq!(body["status"], "tentative");
    assert!(body["responded_at"].is_string());

    let (_, mine) = app.get("/api/meetings", Some(&c.token)).await;
    assert_eq!(mine[0]["my_status"], "tentative");
    let (_, g) = app
        .get(&format!("/api/meetings/{id}"), Some(&c.token))
        .await;
    assert_eq!(g["my_status"], "tentative");
    let (_, inv) = app
        .get(&format!("/api/meetings/{id}/invitees"), Some(&a.token))
        .await;
    assert_eq!(inv[0]["status"], "tentative");
    // Respondeu: sai da quarentena como quem aceita.
    let n: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM meet_quarantine WHERE user_id = $1::uuid")
            .bind(&c.user_id)
            .fetch_one(&app.db)
            .await
            .unwrap();
    assert_eq!(n, 0);
    // A base só aceita os quatro estados.
    let r = sqlx::query("UPDATE meeting_invitees SET status = 'maybe' WHERE user_id = $1::uuid")
        .bind(&c.user_id)
        .execute(&app.db)
        .await;
    assert!(r.is_err(), "CHECK da 0055");
}

// ---------------------------------------------------------------------------
//  Arranque: a sala nasce com as opções
// ---------------------------------------------------------------------------

#[sqlx::test(migrations = "./migrations")]
async fn start_creates_room_with_the_scheduled_options(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let a = app.new_org("alfa.test").await;
    let c = app.add_member(&a, "carla", "member").await;
    let starts = in_hours(1);

    // Controlo: sem sala de espera, o convidado entra directo.
    let (_, plain) = app
        .post(
            "/api/meetings",
            Some(&a.token),
            json!({"title": "Aberta", "starts_at": starts, "invitee_ids": [c.user_id]}),
        )
        .await;
    let (st, s) = app
        .post(
            &format!("/api/meetings/{}/start", plain["id"].as_str().unwrap()),
            Some(&a.token),
            json!({}),
        )
        .await;
    assert_eq!(st, 200, "{s}");
    assert_eq!(options_of(&s), defaults());
    let code = s["code"].as_str().unwrap();
    let (_, j) = app
        .post(
            &format!("/api/rooms/{code}/join"),
            Some(&c.token),
            json!({}),
        )
        .await;
    let cl = jwt_claims(j["room_token"].as_str().unwrap());
    assert!(cl.get("wait").is_none(), "convidado entra directo: {cl}");

    // Com as opções: a sala nasce com elas e o convidado vai para a espera.
    let (_, m) = app
        .post(
            "/api/meetings",
            Some(&a.token),
            json!({"title": "Formação", "starts_at": starts, "invitee_ids": [c.user_id],
                   "format": "training", "waiting_room": true, "auto_record": true,
                   "record_quality": "2160p"}),
        )
        .await;
    let (st, s) = app
        .post(
            &format!("/api/meetings/{}/start", m["id"].as_str().unwrap()),
            Some(&a.token),
            json!({}),
        )
        .await;
    assert_eq!(st, 200, "{s}");
    assert_eq!(s["format"], "training");
    assert_eq!(s["waiting_room"], true);
    let code = s["code"].as_str().unwrap().to_string();
    assert_eq!(
        room_row(&app, &code).await,
        (true, "training".into(), true, Some("2160p".into()))
    );
    let (st, j) = app
        .post(
            &format!("/api/rooms/{code}/join"),
            Some(&c.token),
            json!({}),
        )
        .await;
    assert_eq!(st, 200, "{j}");
    assert_eq!(j["room"]["waiting_room"], true);
    assert_eq!(j["room"]["format"], "training");
    let cl = jwt_claims(j["room_token"].as_str().unwrap());
    assert_eq!(cl["wait"], true, "até o convidado espera: {cl}");
    assert_eq!(cl["wr"], true);

    // O segundo start (sala reutilizada) devolve as mesmas opções.
    let (_, s2) = app
        .post(
            &format!("/api/meetings/{}/start", m["id"].as_str().unwrap()),
            Some(&c.token),
            json!({}),
        )
        .await;
    assert_eq!(s2["code"], code.as_str());
    assert_eq!(options_of(&s2), options_of(&s));
}

// ---------------------------------------------------------------------------
//  PATCH: BFF (só opções, só o anfitrião) e v1 — a mesma função
// ---------------------------------------------------------------------------

#[sqlx::test(migrations = "./migrations")]
async fn patch_options_bff_and_v1(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let a = app.new_org("alfa.test").await;
    let b = app.new_org("beta.test").await;
    let c = app.add_member(&a, "carla", "member").await;
    let (_, ka) = app.api_key(&a).await;
    let (_, kb) = app.api_key(&b).await;
    let m = app.new_meeting(&a, "Mudar", &[&c.user_id]).await;
    let id = m["id"].as_str().unwrap();
    let path = format!("/api/meetings/{id}");

    // Parcial: só o que vem muda.
    let (st, p) = app
        .patch(
            &path,
            Some(&a.token),
            json!({"waiting_room": true, "record_quality": "720p"}),
        )
        .await;
    assert_eq!(st, 200, "{p}");
    assert_eq!(
        options_of(&p),
        json!({"format": "meeting", "waiting_room": true, "auto_record": false, "record_quality": "720p"})
    );
    assert_eq!(p["invitee_count"], 1);

    // Recusas: valor inválido (400 com código), campo desconhecido (não se
    // ignora), convidado (403), outra org (404), id inventado (404).
    let (st, body) = app
        .patch(&path, Some(&a.token), json!({"format": "aula"}))
        .await;
    assert_eq!(st, 400);
    assert_eq!(body["code"], "meeting.invalid_format");
    let (st, body) = app
        .patch(&path, Some(&a.token), json!({"title": "outro"}))
        .await;
    assert_eq!(st, 422, "campo desconhecido: {body}");
    let (st, body) = app
        .patch(&path, Some(&c.token), json!({"auto_record": true}))
        .await;
    assert_eq!(st, 403, "{body}");
    assert_eq!(body["code"], "meeting.not_host");
    let (st, _) = app
        .patch(&path, Some(&b.token), json!({"auto_record": true}))
        .await;
    assert_eq!(st, 404);
    let (st, _) = app
        .patch(
            &format!("/api/meetings/{}", common::INVENTED_ID),
            Some(&a.token),
            json!({"auto_record": true}),
        )
        .await;
    assert_eq!(st, 404);
    let (_, g) = app.get(&path, Some(&a.token)).await;
    assert_eq!(g["auto_record"], false, "nenhuma recusa escreveu");
    assert_eq!(g["format"], "meeting");

    // Depois do arranque, o PATCH passa à sala.
    let (_, s) = app
        .post(&format!("{path}/start"), Some(&a.token), json!({}))
        .await;
    let code = s["code"].as_str().unwrap().to_string();
    assert_eq!(
        room_row(&app, &code).await,
        (true, "normal".into(), false, Some("720p".into()))
    );
    let (st, _) = app
        .patch(
            &path,
            Some(&a.token),
            json!({"format": "hybrid", "waiting_room": false}),
        )
        .await;
    assert_eq!(st, 200);
    assert_eq!(
        room_row(&app, &code).await,
        (false, "hybrid".into(), false, Some("720p".into()))
    );

    // v1: cria, altera opções junto com o título, a sala acompanha.
    let (_, v) = v1(
        &app,
        reqwest::Method::POST,
        "/meetings",
        &ka,
        Some(json!({"title": "API", "starts_at": in_hours(2), "host_email": a.email})),
    )
    .await;
    let vid = v["id"].as_str().unwrap().to_string();
    let vcode = v["room_code"].as_str().unwrap().to_string();
    let (st, p) = v1(
        &app,
        reqwest::Method::PATCH,
        &format!("/meetings/{vid}"),
        &ka,
        Some(json!({"title": "API 2", "auto_record": true, "format": "training"})),
    )
    .await;
    assert_eq!(st, 200, "{p}");
    assert_eq!(p["title"], "API 2");
    assert_eq!(p["auto_record"], true);
    assert_eq!(p["format"], "training");
    assert_eq!(
        room_row(&app, &vcode).await,
        (false, "training".into(), true, Some("1080p".into()))
    );
    let (st, body) = v1(
        &app,
        reqwest::Method::PATCH,
        &format!("/meetings/{vid}"),
        &ka,
        Some(json!({"title": "não entra", "record_quality": "8k"})),
    )
    .await;
    assert_eq!(st, 400);
    assert_eq!(body["code"], "meeting.invalid_record_quality");
    let (_, g) = v1(
        &app,
        reqwest::Method::GET,
        &format!("/meetings/{vid}"),
        &ka,
        None,
    )
    .await;
    assert_eq!(g["title"], "API 2", "a recusa não escreveu o título");
    // Chave de outra org: 404.
    let (st, _) = v1(
        &app,
        reqwest::Method::PATCH,
        &format!("/meetings/{vid}"),
        &kb,
        Some(json!({"auto_record": false})),
    )
    .await;
    assert_eq!(st, 404);

    // v1 numa sala E2EE: ligar a gravação automática é 422.
    let (_, e) = v1(
        &app,
        reqwest::Method::POST,
        "/meetings",
        &ka,
        Some(json!({"title": "Cifrada", "starts_at": in_hours(2), "host_email": a.email, "e2ee": true})),
    )
    .await;
    let (st, body) = v1(
        &app,
        reqwest::Method::PATCH,
        &format!("/meetings/{}", e["id"].as_str().unwrap()),
        &ka,
        Some(json!({"auto_record": true})),
    )
    .await;
    assert_eq!(st, 422, "{body}");
    assert_eq!(body["code"], "meeting.auto_record_e2ee");
}

// ---------------------------------------------------------------------------
//  Gravação automática: arranca quando o anfitrião entra
// ---------------------------------------------------------------------------

/// Entra na sala pelo `/ws` com o token de `join` e espera pelo `joined`.
async fn host_enters(
    app: &TestApp,
    token: &str,
    code: &str,
) -> tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>> {
    let (st, j) = app
        .post(&format!("/api/rooms/{code}/join"), Some(token), json!({}))
        .await;
    assert_eq!(st, 200, "{j}");
    let url = format!(
        "{}{}",
        app.base.replacen("http://", "ws://", 1),
        j["ws_path"].as_str().unwrap()
    );
    let (mut ws, _) = tokio_tungstenite::connect_async(url)
        .await
        .expect("ligar ao /ws");
    tokio::time::timeout(Duration::from_secs(5), async {
        while let Some(msg) = ws.next().await {
            let Ok(text) = msg.unwrap().into_text() else {
                continue;
            };
            if serde_json::from_str::<Value>(&text)
                .map(|v| v["type"] == "joined")
                .unwrap_or(false)
            {
                return;
            }
        }
        panic!("o /ws fechou antes do joined");
    })
    .await
    .expect("sem joined em 5 s");
    ws
}

async fn room_id(app: &TestApp, code: &str) -> uuid::Uuid {
    sqlx::query_scalar("SELECT id FROM rooms WHERE code = $1")
        .bind(code)
        .fetch_one(&app.db)
        .await
        .unwrap()
}

/// Espera até `secs` segundos que a sala esteja a gravar.
async fn recording_by(app: &TestApp, room: uuid::Uuid, secs: u64) -> Option<String> {
    let deadline = std::time::Instant::now() + Duration::from_secs(secs);
    loop {
        if let Some(by) = app.state.sfu.recording_by(room).await {
            return Some(by);
        }
        if std::time::Instant::now() > deadline {
            return None;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

#[sqlx::test(migrations = "./migrations")]
async fn auto_record_starts_when_the_host_enters(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let a = app.new_org("alfa.test").await;

    // Com gravação automática: o anfitrião entra e o gravador arranca.
    let (_, m) = app
        .post(
            "/api/meetings",
            Some(&a.token),
            json!({"title": "Grava", "starts_at": in_hours(1), "auto_record": true,
                   "record_quality": "audio"}),
        )
        .await;
    let (_, s) = app
        .post(
            &format!("/api/meetings/{}/start", m["id"].as_str().unwrap()),
            Some(&a.token),
            json!({}),
        )
        .await;
    let code = s["code"].as_str().unwrap().to_string();
    let rid = room_id(&app, &code).await;
    let _ws = host_enters(&app, &a.token, &code).await;
    let by = recording_by(&app, rid, 5).await;
    assert!(by.is_some(), "a gravação automática não arrancou");

    // Controlo: a mesma entrada, sem gravação automática, não grava.
    let m2 = app.new_meeting(&a, "Não grava", &[]).await;
    let (_, s2) = app
        .post(
            &format!("/api/meetings/{}/start", m2["id"].as_str().unwrap()),
            Some(&a.token),
            json!({}),
        )
        .await;
    let code2 = s2["code"].as_str().unwrap().to_string();
    let rid2 = room_id(&app, &code2).await;
    let _ws2 = host_enters(&app, &a.token, &code2).await;
    assert_eq!(recording_by(&app, rid2, 2).await, None);

    // Sala E2EE com `auto_record` escrito à mão na base (a API recusa-o): o
    // gravador não arranca — sem chave, gravaria ruído cifrado.
    let m3 = app.new_meeting(&a, "Cifrada", &[]).await;
    let (_, s3) = app
        .post(
            &format!("/api/meetings/{}/start", m3["id"].as_str().unwrap()),
            Some(&a.token),
            json!({}),
        )
        .await;
    let code3 = s3["code"].as_str().unwrap().to_string();
    sqlx::query("UPDATE rooms SET e2ee = true, auto_record = true WHERE code = $1")
        .bind(&code3)
        .execute(&app.db)
        .await
        .unwrap();
    let rid3 = room_id(&app, &code3).await;
    let _ws3 = host_enters(&app, &a.token, &code3).await;
    assert_eq!(recording_by(&app, rid3, 2).await, None, "E2EE");

    // Sala que já tem uma gravação: voltar a entrar não recomeça.
    let m4 = app.new_meeting(&a, "Já gravada", &[]).await;
    let (_, s4) = app
        .post(
            &format!("/api/meetings/{}/start", m4["id"].as_str().unwrap()),
            Some(&a.token),
            json!({}),
        )
        .await;
    let code4 = s4["code"].as_str().unwrap().to_string();
    let rid4 = room_id(&app, &code4).await;
    sqlx::query("UPDATE rooms SET auto_record = true WHERE id = $1")
        .bind(rid4)
        .execute(&app.db)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO recordings (room_id, uploader_id, filename, size_bytes)
         VALUES ($1, $2::uuid, 'anterior.webm', 1)",
    )
    .bind(rid4)
    .bind(&a.user_id)
    .execute(&app.db)
    .await
    .unwrap();
    let _ws4 = host_enters(&app, &a.token, &code4).await;
    assert_eq!(recording_by(&app, rid4, 2).await, None, "já tinha gravação");
}
