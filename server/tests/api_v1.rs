//! Caracterização da API PÚBLICA v1 (`/api/v1/*`): autenticação por chave
//! `dlx_`, salas, gravações, reuniões (criar com idempotência por
//! `external_ref`, alterar, apagar, tocar, notas), provisão de organizações por
//! segredo de plataforma, integração Odoo e armazenamento da plataforma.
//!
//! Portado de `web/e2e/isolamento.mjs` (S1, S2, S3).
mod common;

use chrono::{Duration, Utc};
use common::{jwt_claims, TestApp};
use serde_json::{json, Value};

fn bearer(key: &str) -> String {
    format!("Bearer {key}")
}

async fn v1(
    app: &TestApp,
    method: reqwest::Method,
    path: &str,
    key: &str,
    body: Option<Value>,
) -> (u16, Value) {
    let auth = bearer(key);
    // Um caminho relativo é da v1 do inquilino; um absoluto (`/api/…`) é de
    // outra superfície (operador, integração) com a mesma autenticação por
    // cabeçalho.
    let url = if path.starts_with("/api/") {
        path.to_string()
    } else {
        format!("/api/v1{path}")
    };
    let r = app
        .raw(method, &url, &[("Authorization", &auth)], body)
        .await;
    (r.status, r.json())
}

fn in_hours(h: i64) -> String {
    (Utc::now() + Duration::hours(h)).to_rfc3339()
}

// ---------------------------------------------------------------------------
//  Autenticação por chave
// ---------------------------------------------------------------------------

#[sqlx::test(migrations = "./migrations")]
async fn api_key_authenticates_org_endpoint(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let a = app.new_org("alfa.test").await;
    app.add_member(&a, "carla", "member").await;
    let (key_id, key) = app.api_key(&a).await;

    let (st, org) = v1(&app, reqwest::Method::GET, "/organization", &key, None).await;
    assert_eq!(st, 200, "{org}");
    assert_eq!(
        org,
        json!({"id": a.org(), "name": "Org alfa.test", "email_domain": "alfa.test",
               "domain": "", "members": 2})
    );
    // Também por `X-API-Key`.
    let r = app
        .raw(
            reqwest::Method::GET,
            "/api/v1/organization",
            &[("X-API-Key", &key)],
            None,
        )
        .await;
    assert_eq!(r.status, 200);
    // O uso fica registado.
    let (_, keys) = app
        .get(&format!("/api/orgs/{}/api-keys", a.org()), Some(&a.token))
        .await;
    assert!(keys[0]["last_used_at"].is_string(), "{keys}");

    // Recusas: sem chave, JWT de sessão, prefixo errado, chave inventada.
    let r = app
        .raw(reqwest::Method::GET, "/api/v1/organization", &[], None)
        .await;
    assert_eq!(r.status, 401);
    for bad in [a.token.as_str(), "abc_123", "dlx_0000"] {
        let (st, _) = v1(&app, reqwest::Method::GET, "/organization", bad, None).await;
        assert_eq!(st, 401, "{bad}");
    }

    // Revogar: 204, e a chave deixa de servir.
    let (st, _) = app
        .delete(
            &format!("/api/orgs/{}/api-keys/{key_id}", a.org()),
            Some(&a.token),
        )
        .await;
    assert_eq!(st, 204);
    let (st, _) = v1(&app, reqwest::Method::GET, "/organization", &key, None).await;
    assert_eq!(st, 401);
}

#[sqlx::test(migrations = "./migrations")]
async fn v1_is_rate_limited_per_ip(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    // Sem chave válida, 120 pedidos/min por IP; o limitador corre ANTES da
    // autenticação. Com chave válida o balde é da chave (`api_key_scopes`).
    let mut statuses = Vec::new();
    for _ in 0..121 {
        let (st, _) = v1(
            &app,
            reqwest::Method::GET,
            "/organization",
            "dlx_invalida",
            None,
        )
        .await;
        statuses.push(st);
    }
    assert!(statuses[..120].iter().all(|s| *s == 401), "{statuses:?}");
    assert_eq!(statuses[120], 429);
}

// ---------------------------------------------------------------------------
//  Salas e gravações
// ---------------------------------------------------------------------------

#[sqlx::test(migrations = "./migrations")]
async fn v1_rooms_create_get_and_join_bot(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let a = app.new_org("alfa.test").await;
    let b = app.new_org("beta.test").await;
    let (_, ka) = app.api_key(&a).await;
    let (_, kb) = app.api_key(&b).await;

    let (st, room) = v1(
        &app,
        reqwest::Method::POST,
        "/rooms",
        &ka,
        Some(json!({"name": "  Sala API  ", "e2ee": true})),
    )
    .await;
    assert_eq!(st, 200, "{room}");
    let code = room["code"].as_str().unwrap().to_string();
    assert_eq!(room["name"], "Sala API");
    assert_eq!(room["e2ee"], true);
    assert_eq!(room["waiting_room"], false);
    assert_eq!(room["join_url"], format!("/#/r/{code}"));
    // Sem nome: nome por omissão.
    let (_, r2) = v1(&app, reqwest::Method::POST, "/rooms", &ka, Some(json!({}))).await;
    assert_eq!(r2["name"], "Reunião (API)");
    // O dono da sala é quem criou a chave.
    let (_, bff) = app.get(&format!("/api/rooms/{code}"), Some(&a.token)).await;
    assert_eq!(bff["owner_id"], a.user_id.as_str());

    // Com domínio de produção, o link é absoluto.
    let (st, body) = app
        .patch(
            &format!("/api/orgs/{}", a.org()),
            Some(&a.token),
            json!({"domain": "meet.alfa.test"}),
        )
        .await;
    assert_eq!(st, 200, "{body}");
    let (st, got) = v1(
        &app,
        reqwest::Method::GET,
        &format!("/rooms/{code}"),
        &ka,
        None,
    )
    .await;
    assert_eq!(st, 200);
    assert_eq!(
        got["join_url"],
        format!("https://meet.alfa.test/#/r/{code}")
    );

    let (st, _) = v1(&app, reqwest::Method::GET, "/rooms/aaa-bbbb-ccc", &ka, None).await;
    assert_eq!(st, 404);
    // Chave da org B não vê a sala da A.
    let (st, body) = v1(
        &app,
        reqwest::Method::GET,
        &format!("/rooms/{code}"),
        &kb,
        None,
    )
    .await;
    assert_eq!(st, 404, "{body}");

    // Bot
    let (st, bot) = v1(
        &app,
        reqwest::Method::POST,
        &format!("/rooms/{}/bots", code.to_uppercase()),
        &ka,
        Some(json!({"bot_name": "  "})),
    )
    .await;
    assert_eq!(st, 200, "{bot}");
    let claims = jwt_claims(bot["room_token"].as_str().unwrap());
    assert_eq!(claims["typ"], "room");
    assert_eq!(claims["is_bot"], true);
    assert_eq!(claims["name"], "AI Assistant");
    assert_eq!(claims["sub"], a.user_id.as_str());
    assert!(claims.get("owner").is_none());
    assert_eq!(bot["room"]["code"], code.as_str());
    assert!(bot["ws_path"].as_str().unwrap().starts_with("/ws?token="));
    let (st, _) = v1(
        &app,
        reqwest::Method::POST,
        &format!("/rooms/{code}/bots"),
        &kb,
        Some(json!({"bot_name": "espião"})),
    )
    .await;
    assert_eq!(st, 404);
}

#[sqlx::test(migrations = "./migrations")]
async fn v1_recordings_list_scoped_to_org(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let a = app.new_org("alfa.test").await;
    let b = app.new_org("beta.test").await;
    let (_, ka) = app.api_key(&a).await;
    let (_, kb) = app.api_key(&b).await;
    let (st, body) = v1(&app, reqwest::Method::GET, "/recordings", &ka, None).await;
    assert_eq!(st, 200);
    assert_eq!(body, json!({"recordings": []}));

    let room = app.new_room(&a, "gravada").await;
    // Ainda privada: uma chave da própria organização não a vê — v1
    // representa a organização, não um colega com relação directa (R230).
    let rec_privada = app
        .insert_recording(room["id"].as_str().unwrap(), &a.user_id)
        .await;
    let (_, body) = v1(&app, reqwest::Method::GET, "/recordings", &ka, None).await;
    assert_eq!(body, json!({"recordings": []}), "privada não deve aparecer");

    let rec = app
        .insert_recording(room["id"].as_str().unwrap(), &a.user_id)
        .await;
    sqlx::query(
        "UPDATE recordings SET visibility = 'org', published_at = now() WHERE id = $1::uuid",
    )
    .bind(&rec)
    .execute(&app.db)
    .await
    .unwrap();
    let (_, body) = v1(&app, reqwest::Method::GET, "/recordings", &ka, None).await;
    assert_eq!(
        body["recordings"].as_array().unwrap().len(),
        1,
        "só a publicada aparece, a privada ({rec_privada}) continua fora: {body}"
    );
    let r = &body["recordings"][0];
    assert_eq!(r["id"], rec.as_str());
    assert_eq!(r["room_code"], room["code"]);
    assert_eq!(r["size_bytes"], 4);
    // O ficheiro vive em `/content`; `GET /api/recordings/{id}` são os metadados.
    assert_eq!(r["download_url"], format!("/api/recordings/{rec}/content"));
    let (_, body) = v1(&app, reqwest::Method::GET, "/recordings", &kb, None).await;
    assert_eq!(body, json!({"recordings": []}));
}

// ---------------------------------------------------------------------------
//  Reuniões
// ---------------------------------------------------------------------------

#[sqlx::test(migrations = "./migrations")]
async fn v1_meetings_create_idempotent_patch_ring_notes_delete(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let a = app.new_org("alfa.test").await;
    let b = app.new_org("beta.test").await;
    let c = app.add_member(&a, "carla", "member").await;
    let (_, ka) = app.api_key(&a).await;
    let (_, kb) = app.api_key(&b).await;
    let starts = in_hours(2);

    let body = json!({
        "external_ref": "odoo:calendar.event:42",
        "title": "  Comité  ",
        "starts_at": starts,
        "duration_min": 45,
        "host_email": "ADMIN@alfa.test",
        "invitees": [
            {"email": c.email},
            {"email": "novo@alfa.test", "name": "Novo Colega"},
            {"email": "admin@alfa.test"},
            {"email": b.email},
            {"email": "\"Nome\" <x@alfa.test>"}
        ]
    });
    let (st, m) = v1(
        &app,
        reqwest::Method::POST,
        "/meetings",
        &ka,
        Some(body.clone()),
    )
    .await;
    assert_eq!(st, 200, "{m}");
    let id = m["id"].as_str().unwrap().to_string();
    assert_eq!(m["existing"], false);
    assert_eq!(m["external_ref"], "odoo:calendar.event:42");
    assert_eq!(m["title"], "Comité");
    assert_eq!(m["duration_min"], 45);
    assert_eq!(m["kind"], "video");
    assert_eq!(m["host_email"], "admin@alfa.test");
    let code = m["room_code"].as_str().unwrap().to_string();
    assert_eq!(m["join_url"], format!("/#/r/{code}"));
    // Convidados: a colega e a conta nova; o anfitrião não é convidado de si
    // próprio; o de outra org e o email mal-formado são saltados com razão.
    let emails: Vec<&str> = m["invitees"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["email"].as_str().unwrap())
        .collect();
    assert_eq!(emails, vec!["carla@alfa.test", "novo@alfa.test"]);
    assert_eq!(
        m["skipped"],
        json!([
            {"email": b.email, "reason": "o utilizador pertence a outra organização"},
            {"email": "\"nome\" <x@alfa.test>", "reason": "email inválido"}
        ])
    );
    // A conta nova ficou membro da org A.
    let (_, emps) = app
        .get(&format!("/api/orgs/{}/members", a.org()), Some(&a.token))
        .await;
    assert!(emps.to_string().contains("novo@alfa.test"));

    // Idempotência por external_ref: devolve a MESMA reunião, `existing: true`.
    let (st, again) = v1(&app, reqwest::Method::POST, "/meetings", &ka, Some(body)).await;
    assert_eq!(st, 200);
    assert_eq!(again["id"], id.as_str());
    assert_eq!(again["existing"], true);
    assert_eq!(again["skipped"], json!([]));
    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM meetings")
        .fetch_one(&app.db)
        .await
        .unwrap();
    assert_eq!(n, 1);

    // GET com since.
    let (st, list) = v1(&app, reqwest::Method::GET, "/meetings", &ka, None).await;
    assert_eq!(st, 200);
    assert_eq!(list["meetings"][0]["id"], id.as_str());
    assert_eq!(list["meetings"][0]["room_code"], code.as_str());
    assert!(list["meetings"][0]["minutes_ai_at"].is_null());
    let future = (Utc::now() + Duration::days(30))
        .to_rfc3339()
        .replace('+', "%2B");
    let (st, list) = v1(
        &app,
        reqwest::Method::GET,
        &format!("/meetings?since={future}"),
        &ka,
        None,
    )
    .await;
    assert_eq!(st, 200);
    assert_eq!(list, json!({"meetings": []}));
    let (st, _) = v1(
        &app,
        reqwest::Method::GET,
        "/meetings?since=ontem",
        &ka,
        None,
    )
    .await;
    assert_eq!(st, 400);
    let (_, list) = v1(&app, reqwest::Method::GET, "/meetings", &kb, None).await;
    assert_eq!(list, json!({"meetings": []}));

    // PATCH
    for bad in [json!({"title": "  "}), json!({"duration_min": 0})] {
        let (st, _) = v1(
            &app,
            reqwest::Method::PATCH,
            &format!("/meetings/{id}"),
            &ka,
            Some(bad),
        )
        .await;
        assert_eq!(st, 400);
    }
    let (st, p) = v1(
        &app,
        reqwest::Method::PATCH,
        &format!("/meetings/{id}"),
        &ka,
        Some(json!({"title": "Comité II", "invitees": [{"email": "novo@alfa.test"}]})),
    )
    .await;
    assert_eq!(st, 200, "{p}");
    assert_eq!(p["title"], "Comité II");
    assert_eq!(p["existing"], false);
    // A lista de convidados é SUBSTITUÍDA.
    assert_eq!(p["invitees"].as_array().unwrap().len(), 1);
    assert_eq!(p["invitees"][0]["email"], "novo@alfa.test");
    // O nome da sala acompanha o título.
    let (_, room) = app.get(&format!("/api/rooms/{code}"), Some(&a.token)).await;
    assert_eq!(room["name"], "Comité II");

    // Ring: convidados offline.
    let (st, ring) = v1(
        &app,
        reqwest::Method::POST,
        &format!("/meetings/{id}/ring"),
        &ka,
        None,
    )
    .await;
    assert_eq!(st, 200, "{ring}");
    assert_eq!(ring["ringing"], json!([]));
    assert_eq!(ring["offline"].as_array().unwrap().len(), 1);
    assert_eq!(ring["room_code"], code.as_str());

    // Leitura da reunião por id (rota nova): o mesmo recurso que o PATCH devolveu.
    let (st, got) = v1(
        &app,
        reqwest::Method::GET,
        &format!("/meetings/{id}"),
        &ka,
        None,
    )
    .await;
    assert_eq!(st, 200, "{got}");
    assert_eq!(got["id"], id.as_str());
    assert_eq!(got["title"], "Comité II");
    assert_eq!(got["room_code"], code.as_str());

    // Notas
    let (st, notes) = v1(
        &app,
        reqwest::Method::GET,
        &format!("/meetings/{id}/minutes"),
        &ka,
        None,
    )
    .await;
    assert_eq!(st, 200);
    assert_eq!(notes["id"], id.as_str());
    assert_eq!(notes["title"], "Comité II");
    assert_eq!(notes["minutes"], "");
    assert_eq!(notes["transcript"], "");

    // Chave da org B: nada disto existe para ela.
    for (method, path) in [
        (reqwest::Method::GET, format!("/meetings/{id}")),
        (reqwest::Method::PATCH, format!("/meetings/{id}")),
        (reqwest::Method::DELETE, format!("/meetings/{id}")),
        (reqwest::Method::POST, format!("/meetings/{id}/ring")),
        (reqwest::Method::GET, format!("/meetings/{id}/minutes")),
    ] {
        let body = (method == reqwest::Method::PATCH).then(|| json!({"title": "forjado"}));
        let (st, resp) = v1(&app, method.clone(), &path, &kb, body).await;
        assert_eq!(st, 404, "{method} {path}: {resp}");
    }

    // DELETE apaga a reunião e a sala (sem gravações).
    let (st, del) = v1(
        &app,
        reqwest::Method::DELETE,
        &format!("/meetings/{id}"),
        &ka,
        None,
    )
    .await;
    assert_eq!(st, 200);
    assert_eq!(del, json!({"ok": true, "room_deleted": true}));
    let (st, _) = app.get(&format!("/api/rooms/{code}"), Some(&a.token)).await;
    assert_eq!(st, 404);
    let (st, _) = v1(
        &app,
        reqwest::Method::PATCH,
        &format!("/meetings/{id}"),
        &ka,
        Some(json!({})),
    )
    .await;
    assert_eq!(st, 404);
}

#[sqlx::test(migrations = "./migrations")]
async fn v1_meetings_validation(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let a = app.new_org("alfa.test").await;
    let b = app.new_org("beta.test").await;
    let (_, ka) = app.api_key(&a).await;
    let s = in_hours(1);
    let many: Vec<Value> = (0..201)
        .map(|i| json!({"email": format!("u{i}@alfa.test")}))
        .collect();
    for (bad, why) in [
        (
            json!({"title": " ", "starts_at": s, "host_email": a.email}),
            "título",
        ),
        (
            json!({"title": "x", "kind": "chat", "starts_at": s, "host_email": a.email}),
            "kind",
        ),
        (
            json!({"title": "x", "duration_min": 0, "starts_at": s, "host_email": a.email}),
            "duração 0",
        ),
        (
            json!({"title": "x", "duration_min": 1441, "starts_at": s, "host_email": a.email}),
            "duração",
        ),
        (
            json!({"title": "x", "starts_at": s, "host_email": "sem-arroba"}),
            "host_email",
        ),
        (
            json!({"title": "x", "starts_at": s, "host_email": a.email, "invitees": many}),
            "200 convidados",
        ),
    ] {
        let (st, body) = v1(&app, reqwest::Method::POST, "/meetings", &ka, Some(bad)).await;
        assert_eq!(st, 400, "{why}: {body}");
    }
    // Anfitrião de outra org: 409.
    let (st, body) = v1(
        &app,
        reqwest::Method::POST,
        "/meetings",
        &ka,
        Some(json!({"title": "x", "starts_at": s, "host_email": b.email})),
    )
    .await;
    assert_eq!(st, 409, "{body}");
    // Duração mínima aceite na v1 é 1 (no BFF é 5).
    let (st, body) = v1(
        &app,
        reqwest::Method::POST,
        "/meetings",
        &ka,
        Some(json!({"title": "x", "duration_min": 1, "starts_at": s, "host_email": a.email, "kind": "voice"})),
    )
    .await;
    assert_eq!(st, 200, "{body}");
    assert_eq!(body["kind"], "voice");
}

#[sqlx::test(migrations = "./migrations")]
async fn v1_meeting_with_archived_host_is_refused(db: sqlx::PgPool) {
    // S3: um membro arquivado deixa de poder ser anfitrião pela API.
    let app = TestApp::spawn(db).await;
    let a = app.new_org("alfa.test").await;
    let c = app.add_member(&a, "carla", "member").await;
    let (_, ka) = app.api_key(&a).await;
    let body = |t: &str| json!({"title": t, "starts_at": in_hours(2), "host_email": c.email});
    let (st, m) = v1(
        &app,
        reqwest::Method::POST,
        "/meetings",
        &ka,
        Some(body("antes")),
    )
    .await;
    assert_eq!(st, 200, "{m}");
    assert_eq!(m["host_email"], c.email.as_str());
    app.archive_member(a.org(), &c.user_id).await;
    let (st, body) = v1(
        &app,
        reqwest::Method::POST,
        "/meetings",
        &ka,
        Some(body("depois")),
    )
    .await;
    assert_eq!(st, 409, "{body}");
}

/// R151 (fechada): `meetings_v1::resolve_org_user` criava a conta de um
/// `host_email` (ou convidado) desconhecido SEM verificar o domínio da
/// organização. Uma chave da org A agendava com `ninguem@beta.test` e a conta
/// nascia membro da A — e a org B, dona do domínio, já não a conseguia
/// adicionar (409). Agora: anfitrião fora do domínio → 422; convidado → skipped.
#[sqlx::test(migrations = "./migrations")]
async fn v1_meeting_refuses_to_create_accounts_outside_org_domain(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let a = app.new_org("alfa.test").await;
    let b = app.new_org("beta.test").await;
    let (_, ka) = app.api_key(&a).await;

    // Controlo positivo: anfitrião novo do PRÓPRIO domínio continua a nascer.
    let (st, m) = v1(
        &app,
        reqwest::Method::POST,
        "/meetings",
        &ka,
        Some(
            json!({"title": "ok", "starts_at": in_hours(2), "host_email": "novo@alfa.test",
                    "invitees": [{"email": "ninguem@beta.test"}]}),
        ),
    )
    .await;
    assert_eq!(st, 200, "{m}");
    assert!(
        m.to_string().contains("ninguem@beta.test"),
        "convidado fora do domínio sai em skipped: {m}"
    );

    // O ataque: anfitrião de um domínio alheio.
    let (st, body) = v1(
        &app,
        reqwest::Method::POST,
        "/meetings",
        &ka,
        Some(json!({"title": "x", "starts_at": in_hours(3), "host_email": "ninguem@beta.test"})),
    )
    .await;
    assert_eq!(st, 422, "{body}");
    assert_eq!(body["code"], "meeting.host_outside_org_domain");
    let (_, emps) = app
        .get(&format!("/api/orgs/{}/members", a.org()), Some(&a.token))
        .await;
    assert!(!emps.to_string().contains("ninguem@beta.test"), "{emps}");

    // E a org dona do domínio adiciona a pessoa sem conflito.
    let (st, body) = app
        .post(
            &format!("/api/orgs/{}/members", b.org()),
            Some(&b.token),
            json!({"email": "ninguem@beta.test"}),
        )
        .await;
    assert_eq!(st, 200, "{body}");
}

// ---------------------------------------------------------------------------
//  Integração Odoo (S2)
// ---------------------------------------------------------------------------

#[sqlx::test(migrations = "./migrations")]
async fn odoo_provision_does_not_capture_accounts(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let a = app.new_org("alfa.test").await;
    let b = app.new_org("beta.test").await;
    // Estas rotas são do token de integração `dlxo_`; a chave `dlx_` recebe
    // 401 desde o R142 (`tests/security_voice_odoo.rs`).
    let (st, tok) = app
        .post(
            &format!("/api/orgs/{}/integrations/odoo/rotate-token", a.org()),
            Some(&a.token),
            json!({}),
        )
        .await;
    assert_eq!(st, 200, "{tok}");
    let ka = tok["token"].as_str().unwrap().to_string();

    let (st, prov) = v1(
        &app,
        reqwest::Method::POST,
        "/api/integrations/odoo/v1/provision",
        &ka,
        Some(json!({
            "company": "Alfa Lda",
            "admin_email": a.email,
            "users": [
                {"odoo_uid": 91, "name": "CAPTURADO", "email": b.email, "is_admin": true},
                {"odoo_uid": 92, "name": "Novo da A", "email": "novo@alfa.test"}
            ]
        })),
    )
    .await;
    assert_eq!(st, 200, "{prov}");
    assert_eq!(prov["org_id"], a.org());
    assert_eq!(prov["created"], 1);
    assert_eq!(prov["updated"], 0);
    assert_eq!(prov["skipped"][0]["email"], b.email.as_str());
    assert!(prov["skipped"][0]["reason"]
        .as_str()
        .unwrap()
        .contains("conta local"));

    let (_, emps) = app
        .get(&format!("/api/orgs/{}/members", a.org()), Some(&a.token))
        .await;
    let s = emps.to_string();
    assert!(!s.contains(&b.email), "o admin da B entrou na A: {s}");
    assert!(s.contains("novo@alfa.test"), "controlo positivo: {s}");
    let (_, me) = app.get("/api/users/me", Some(&b.token)).await;
    assert_eq!(me["username"], "admin-beta.test");
    // O nome da org acompanha a empresa Odoo.
    let (_, orgs) = app.get("/api/orgs", Some(&a.token)).await;
    assert_eq!(orgs[0]["name"], "Alfa Lda");

    // A listagem por chave vê só a org dela.
    let (st, users) = v1(
        &app,
        reqwest::Method::GET,
        "/api/integrations/odoo/v1/users",
        &ka,
        None,
    )
    .await;
    assert_eq!(st, 200);
    assert_eq!(users.as_array().unwrap().len(), 2);
    let (st, _) = v1(
        &app,
        reqwest::Method::GET,
        "/api/integrations/odoo/v1/users",
        "dlxo_inventado",
        None,
    )
    .await;
    assert_eq!(st, 401);
}

// ---------------------------------------------------------------------------
//  Provisão de organizações (segredo de plataforma)
// ---------------------------------------------------------------------------

#[sqlx::test(migrations = "./migrations")]
async fn admin_orgs_without_configured_secret_is_401(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let r = app
        .raw(
            reqwest::Method::POST,
            "/api/operator/v1/organizations",
            &[("X-Provisioning-Secret", "")],
            Some(json!({"name": "Org"})),
        )
        .await;
    assert_eq!(r.status, 401);
}

#[sqlx::test(migrations = "./migrations")]
async fn admin_orgs_provisioning_with_secret(db: sqlx::PgPool) {
    let secret = "segredo-de-plataforma-de-teste";
    let app = TestApp::spawn_with(db, &[("PROVISIONING_SECRET", secret)]).await;
    let post = |hdr: &'static str, body: Value| {
        let app = &app;
        async move {
            app.raw(
                reqwest::Method::POST,
                "/api/operator/v1/organizations",
                &[("X-Provisioning-Secret", hdr)],
                Some(body),
            )
            .await
        }
    };
    let r = post("errado", json!({"name": "Org"})).await;
    assert_eq!(r.status, 401);
    let r = app
        .raw(
            reqwest::Method::POST,
            "/api/operator/v1/organizations",
            &[],
            Some(json!({"name": "Org"})),
        )
        .await;
    assert_eq!(r.status, 401);

    let r = post(secret, json!({"name": " "})).await;
    assert_eq!(r.status, 400);
    let r = post(secret, json!({"name": "Org", "odoo_db": "prod"})).await;
    assert_eq!(r.status, 400, "odoo_db sem odoo_company_id: {}", r.text);

    let body = json!({"name": "Gama SA", "key_name": "Odoo", "email_domain": " GAMA.test ",
                      "odoo_db": "prod", "odoo_company_id": 7, "setup_odoo": true,
                      "sso": {"issuer_url": "https://idp.gama.test", "client_id": "cid",
                              "client_secret": "s", "enforce_sso": false}});
    let r = post(secret, body.clone()).await;
    assert_eq!(r.status, 200, "{}", r.text);
    let p = r.json();
    assert_eq!(p["name"], "Gama SA");
    assert_eq!(p["slug"], "gama-sa");
    assert_eq!(p["sso_configured"], true);
    assert!(p["odoo_token"].as_str().unwrap().starts_with("dlxo_"));
    let key = p["api_key"].as_str().unwrap().to_string();
    assert!(key.starts_with("dlx_"));

    let (st, org) = v1(&app, reqwest::Method::GET, "/organization", &key, None).await;
    assert_eq!(st, 200);
    assert_eq!(org["id"], p["org_id"]);
    assert_eq!(org["email_domain"], "gama.test");
    assert_eq!(org["members"], 1, "o utilizador de serviço");

    // Mesma empresa Odoo (db + company_id): mesma org, chave NOVA.
    let r = post(secret, body).await;
    assert_eq!(r.status, 200);
    let p2 = r.json();
    assert_eq!(p2["org_id"], p["org_id"]);
    assert_ne!(p2["api_key"], p["api_key"]);
    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM organizations WHERE odoo_db = 'prod'")
        .fetch_one(&app.db)
        .await
        .unwrap();
    assert_eq!(n, 1);

    // Sem odoo: slug em colisão ganha sufixo; sem odoo_token.
    let r = post(secret, json!({"name": "Gama SA"})).await;
    assert_eq!(r.status, 200);
    assert_eq!(r.json()["slug"], "gama-sa-1");
    assert!(r.json().get("odoo_token").is_none());
    assert_eq!(r.json()["sso_configured"], false);
}

// ---------------------------------------------------------------------------
//  Armazenamento da plataforma (S1)
// ---------------------------------------------------------------------------

fn access_token_for(app: &TestApp, user_id: uuid::Uuid) -> String {
    let now = Utc::now().timestamp();
    jsonwebtoken::encode(
        &jsonwebtoken::Header::default(),
        &json!({"sub": user_id, "typ": "access", "iat": now, "exp": now + 600}),
        &jsonwebtoken::EncodingKey::from_secret(app.state.config.jwt_secret.as_bytes()),
    )
    .unwrap()
}

#[sqlx::test(migrations = "./migrations")]
async fn platform_storage_requires_declared_platform_admin(db: sqlx::PgPool) {
    let platform_admin = uuid::Uuid::new_v4();
    let app = TestApp::spawn_with(
        db,
        &[("PLATFORM_ADMIN_USER_IDS", &platform_admin.to_string())],
    )
    .await;
    let a = app.new_org("alfa.test").await;
    let (_, ka) = app.api_key(&a).await;

    // Admin de org recém-registado: 403 em tudo, antes do handler correr.
    for (method, path, body) in [
        (reqwest::Method::GET, "/api/operator/v1/storage", None),
        (
            reqwest::Method::PUT,
            "/api/operator/v1/storage",
            Some(
                json!({"storage_type": "webdav", "webdav_url": "http://169.254.169.254/", "webdav_user": "x"}),
            ),
        ),
        (
            reqwest::Method::POST,
            "/api/operator/v1/storage/test",
            Some(json!({})),
        ),
        (
            reqwest::Method::GET,
            "/api/operator/v1/storage/pvc-manifest",
            None,
        ),
    ] {
        let (st, resp) = v1(&app, method.clone(), path, &a.token, body.clone()).await;
        assert_eq!(st, 403, "{method} {path}: {resp}");
        // Uma chave de API não é sessão: 401.
        let (st, _) = v1(&app, method, path, &ka, body).await;
        assert_eq!(st, 401);
    }
    let r = app
        .raw(reqwest::Method::GET, "/api/operator/v1/storage", &[], None)
        .await;
    assert_eq!(r.status, 401);

    // Controlo positivo: o administrador DECLARADO lê e escreve.
    let tok = access_token_for(&app, platform_admin);
    let (st, cfg) = v1(
        &app,
        reqwest::Method::GET,
        "/api/operator/v1/storage",
        &tok,
        None,
    )
    .await;
    assert_eq!(st, 200, "{cfg}");
    assert_eq!(cfg["storage_type"], "local");
    assert_eq!(cfg["webdav_password_set"], false);
    let (st, _) = v1(
        &app,
        reqwest::Method::PUT,
        "/api/operator/v1/storage",
        &tok,
        Some(json!({"storage_type": "ftp"})),
    )
    .await;
    assert_eq!(st, 400);
    let (st, _) = v1(
        &app,
        reqwest::Method::PUT,
        "/api/operator/v1/storage",
        &tok,
        Some(
            json!({"storage_type": "webdav", "webdav_url": "https://dav.test", "webdav_user": "u",
                    "webdav_password": "segredo"}),
        ),
    )
    .await;
    assert_eq!(st, 200);
    let (_, cfg) = v1(
        &app,
        reqwest::Method::GET,
        "/api/operator/v1/storage",
        &tok,
        None,
    )
    .await;
    assert_eq!(cfg["storage_type"], "webdav");
    assert_eq!(cfg["webdav_password_set"], true);
    assert!(!cfg.to_string().contains("segredo"), "{cfg}");
}
