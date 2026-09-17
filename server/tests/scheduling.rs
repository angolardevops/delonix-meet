//! Caracterização do contexto de AGENDAMENTO: reuniões pelo BFF (criar,
//! listar, conflitos, apagar, arrancar, ICS, convidados, resposta, acta),
//! agenda e plano de acção 5W2H — e a recusa cross-org em cada rota por id.
//!
//! Portado de `web/e2e/isolamento.mjs` (secção «reuniões, gravações e quadros»).
mod common;

use chrono::{Duration, Utc};
use common::{assert_denied, TestApp, INVENTED_ID};
use serde_json::{json, Value};

fn in_hours(h: i64) -> String {
    (Utc::now() + Duration::hours(h)).to_rfc3339()
}

fn ids(list: &Value) -> Vec<String> {
    list.as_array()
        .unwrap()
        .iter()
        .map(|m| m["id"].as_str().unwrap().to_string())
        .collect()
}

// ---------------------------------------------------------------------------
//  Criar / validar / listar
// ---------------------------------------------------------------------------

#[sqlx::test(migrations = "./migrations")]
async fn create_meeting_success_and_shape(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let a = app.new_org("alfa.test").await;
    let c = app.add_member(&a, "carla", "member").await;
    let starts = in_hours(2);
    let (st, m) = app
        .post(
            "/api/meetings",
            Some(&a.token),
            json!({"title": "  Revisão  ", "description": " notas ", "starts_at": starts,
                   "invitee_ids": [c.user_id, a.user_id]}),
        )
        .await;
    assert_eq!(st, 200, "{m}");
    assert_eq!(m["title"], "Revisão");
    assert_eq!(m["description"], "notas");
    assert_eq!(m["kind"], "video", "kind por omissão");
    assert_eq!(m["duration_min"], 30, "duração por omissão");
    assert_eq!(m["owner_id"], a.user_id.as_str());
    assert!(m["room_code"].is_null(), "a sala só nasce no start");
    assert_eq!(m["minutes"], "");
    assert!(m["recurrence_freq"].is_null());
    assert_eq!(m["recurrence_interval"], 1);
    assert_eq!(m["conflicts"], json!({"participants": [], "room": []}));

    let (st, _) = app
        .post(
            "/api/meetings",
            None,
            json!({"title": "x", "starts_at": starts}),
        )
        .await;
    assert_eq!(st, 401);
}

#[sqlx::test(migrations = "./migrations")]
async fn create_meeting_validation_limits(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let a = app.new_org("alfa.test").await;
    let t = Some(a.token.as_str());
    let s = in_hours(1);
    for (bad, why) in [
        (json!({"title": "   ", "starts_at": s}), "título vazio"),
        (
            json!({"title": "t".repeat(141), "starts_at": s}),
            "título > 140",
        ),
        (
            json!({"title": "x", "kind": "chat", "starts_at": s}),
            "kind",
        ),
        (
            json!({"title": "x", "duration_min": 4, "starts_at": s}),
            "duração < 5",
        ),
        (
            json!({"title": "x", "duration_min": 1441, "starts_at": s}),
            "duração > 1440",
        ),
    ] {
        let (st, body) = app.post("/api/meetings", t, bad).await;
        assert_eq!(st, 400, "{why}: {body}");
    }
    // Corpo sem `starts_at`: rejeição do extractor JSON (422).
    let (st, _) = app.post("/api/meetings", t, json!({"title": "x"})).await;
    assert_eq!(st, 422);
    // Fronteiras aceites.
    for ok in [
        json!({"title": "t".repeat(140), "duration_min": 5, "starts_at": in_hours(10), "kind": "voice"}),
        json!({"title": "y", "duration_min": 1440, "starts_at": in_hours(40)}),
    ] {
        let (st, body) = app.post("/api/meetings", t, ok).await;
        assert_eq!(st, 200, "{body}");
    }
}

#[sqlx::test(migrations = "./migrations")]
async fn list_meetings_for_owner_and_invitee(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let a = app.new_org("alfa.test").await;
    let c = app.add_member(&a, "carla", "member").await;
    let d = app.add_member(&a, "dario", "member").await;
    let m = app.new_meeting(&a, "Planeamento", &[&c.user_id]).await;

    let (st, list) = app.get("/api/meetings", Some(&a.token)).await;
    assert_eq!(st, 200);
    assert_eq!(ids(&list), vec![m["id"].as_str().unwrap()]);
    assert_eq!(list[0]["is_owner"], true);
    assert_eq!(list[0]["my_status"], "owner");
    assert_eq!(list[0]["owner_name"], "admin-alfa.test");

    let (_, list) = app.get("/api/meetings", Some(&c.token)).await;
    assert_eq!(list[0]["is_owner"], false);
    assert_eq!(list[0]["my_status"], "pending");

    // Um colega não convidado não vê a reunião.
    let (_, list) = app.get("/api/meetings", Some(&d.token)).await;
    assert_eq!(list, json!([]));
}

/// DÍVIDA: `meetings::generate_instances` tem um erro de um em
/// `recurrence_count`. Com `count = 3` gera o pai + TRÊS filhas (4 ocorrências):
/// o ramo `occurs.len() >= max - 1` só dispara depois de já ter empurrado
/// `max - 1` filhas, e empurra mais uma antes de parar.
#[sqlx::test(migrations = "./migrations")]
async fn recurring_meeting_current_behavior_count_off_by_one(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let a = app.new_org("alfa.test").await;
    let (st, m) = app
        .post(
            "/api/meetings",
            Some(&a.token),
            json!({"title": "Diária", "starts_at": in_hours(1), "recurrence_freq": "daily",
                   "recurrence_count": 3}),
        )
        .await;
    assert_eq!(st, 200, "{m}");
    assert_eq!(m["recurrence_freq"], "daily");
    assert_eq!(m["recurrence_count"], 3);
    let (_, list) = app.get("/api/meetings", Some(&a.token)).await;
    let list = list.as_array().unwrap();
    // DÍVIDA: devia ser 3 no total (pai + 2).
    assert_eq!(list.len(), 4);
    assert_eq!(
        list.iter()
            .filter(|x| x["recurrence_parent_id"] == m["id"])
            .count(),
        3
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn meeting_quota_is_enforced(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let a = app.new_org("alfa.test").await;
    let (st, body) = app
        .patch(
            &format!("/api/orgs/{}", a.org()),
            Some(&a.token),
            json!({"max_meetings": 1}),
        )
        .await;
    assert_eq!(st, 200, "{body}");
    app.new_meeting(&a, "primeira", &[]).await;
    let (st, body) = app
        .post(
            "/api/meetings",
            Some(&a.token),
            json!({"title": "segunda", "starts_at": in_hours(5)}),
        )
        .await;
    assert_eq!(st, 409, "{body}");
}

// ---------------------------------------------------------------------------
//  Conflitos
// ---------------------------------------------------------------------------

#[sqlx::test(migrations = "./migrations")]
async fn conflicts_for_participants_and_physical_room(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let a = app.new_org("alfa.test").await;
    let c = app.add_member(&a, "carla", "member").await;
    let t = Some(a.token.as_str());
    let (_, room) = app
        .post(
            &format!("/api/orgs/{}/meeting-rooms", a.org()),
            t,
            json!({"name": "Sala Grande"}),
        )
        .await;
    let start = Utc::now() + Duration::hours(3);
    let (st, m1) = app
        .post(
            "/api/meetings",
            t,
            json!({"title": "Primeira", "starts_at": start.to_rfc3339(), "duration_min": 60,
                   "invitee_ids": [c.user_id], "room_ref": room["id"]}),
        )
        .await;
    assert_eq!(st, 200, "{m1}");

    // Verificação sem criar: C está ocupada a meio da primeira.
    let probe = (start + Duration::minutes(30)).to_rfc3339();
    let (st, cf) = app
        .post(
            "/api/meetings/check-conflicts",
            Some(&c.token),
            json!({"starts_at": probe, "duration_min": 15, "room_ref": room["id"]}),
        )
        .await;
    assert_eq!(st, 200, "{cf}");
    assert_eq!(cf["participants"][0]["meeting_id"], m1["id"]);
    assert_eq!(cf["participants"][0]["user_id"], c.user_id.as_str());
    assert_eq!(cf["room"][0]["meeting_title"], "Primeira");

    // Logo a seguir (sem sobreposição): sem conflitos.
    let after = (start + Duration::minutes(60)).to_rfc3339();
    let (_, cf) = app
        .post(
            "/api/meetings/check-conflicts",
            t,
            json!({"starts_at": after, "room_ref": room["id"]}),
        )
        .await;
    assert_eq!(cf, json!({"participants": [], "room": []}));

    // Criar com a SALA ocupada: 409. Com PESSOAS ocupadas: 200 com aviso.
    let (st, body) = app
        .post(
            "/api/meetings",
            t,
            json!({"title": "Choque", "starts_at": probe, "room_ref": room["id"]}),
        )
        .await;
    assert_eq!(st, 409, "{body}");
    assert!(body["error"].as_str().unwrap().contains("Primeira"));
    let (st, body) = app
        .post(
            "/api/meetings",
            t,
            json!({"title": "Sobreposta", "starts_at": probe}),
        )
        .await;
    assert_eq!(st, 200, "{body}");
    assert_eq!(body["conflicts"]["participants"][0]["meeting_id"], m1["id"]);
}

// ---------------------------------------------------------------------------
//  Apagar / arrancar / ICS
// ---------------------------------------------------------------------------

#[sqlx::test(migrations = "./migrations")]
async fn delete_meeting_only_by_owner(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let a = app.new_org("alfa.test").await;
    let c = app.add_member(&a, "carla", "member").await;
    let d = app.add_member(&a, "dario", "member").await;
    let b = app.new_org("beta.test").await;
    let m = app.new_meeting(&a, "Apagar", &[&c.user_id]).await;
    let path = format!("/api/meetings/{}", m["id"].as_str().unwrap());

    // Leitura por id (rota nova): dono e convidado 200; colega não convidado,
    // outra org e id inventado 404 (sem dizer que existe); anónimo 401.
    for who in [&a, &c] {
        let (st, got) = app.get(&path, Some(&who.token)).await;
        assert_eq!(st, 200, "{}: {got}", who.email);
        assert_eq!(got["id"], m["id"]);
        assert_eq!(got["title"], "Apagar");
    }
    for who in [&d, &b] {
        let (st, body) = app.get(&path, Some(&who.token)).await;
        assert_eq!(st, 404, "{}: {body}", who.email);
        assert_denied("GET reunião alheia", st, &body, "Apagar");
    }
    let (st, _) = app
        .get(&format!("/api/meetings/{INVENTED_ID}"), Some(&a.token))
        .await;
    assert_eq!(st, 404);
    let (st, _) = app.get(&path, None).await;
    assert_eq!(st, 401);

    let (st, _) = app.delete(&path, Some(&c.token)).await;
    assert_eq!(st, 404, "convidado não apaga");
    let (st, body) = app.delete(&path, Some(&a.token)).await;
    assert_eq!(st, 200);
    assert_eq!(body, json!({"ok": true}));
    let (st, _) = app.delete(&path, Some(&a.token)).await;
    assert_eq!(st, 404);
    let (st, _) = app.get(&path, Some(&a.token)).await;
    assert_eq!(st, 404, "apagada deixa de se ler");
    let (_, list) = app.get("/api/meetings", Some(&c.token)).await;
    assert_eq!(list, json!([]));
}

#[sqlx::test(migrations = "./migrations")]
async fn start_meeting_creates_room_once(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let a = app.new_org("alfa.test").await;
    let c = app.add_member(&a, "carla", "member").await;
    let d = app.add_member(&a, "dario", "member").await;
    let m = app.new_meeting(&a, "Arranque", &[&c.user_id]).await;
    let path = format!("/api/meetings/{}/start", m["id"].as_str().unwrap());

    // O convidado não arranca antes do anfitrião.
    let (st, body) = app.post(&path, Some(&c.token), json!({})).await;
    assert_eq!(st, 400, "{body}");
    // Um colega não convidado: 401.
    let (st, _) = app.post(&path, Some(&d.token), json!({})).await;
    assert_eq!(st, 401);
    // Id inventado: 404.
    let (st, _) = app
        .post(
            &format!("/api/meetings/{INVENTED_ID}/start"),
            Some(&a.token),
            json!({}),
        )
        .await;
    assert_eq!(st, 404);

    let (st, s1) = app.post(&path, Some(&a.token), json!({})).await;
    assert_eq!(st, 200, "{s1}");
    let code = s1["code"].as_str().unwrap().to_string();
    assert_eq!(s1["kind"], "video");
    assert_eq!(code.len(), 12);

    // Idempotente: o segundo start devolve a mesma sala; o convidado também.
    let (_, s2) = app.post(&path, Some(&a.token), json!({})).await;
    assert_eq!(s2["code"], code.as_str());
    let (st, s3) = app.post(&path, Some(&c.token), json!({})).await;
    assert_eq!(st, 200);
    assert_eq!(s3["code"], code.as_str());

    // A sala existe e é do anfitrião; o convidado offline ficou com chamada
    // perdida.
    let (st, room) = app.get(&format!("/api/rooms/{code}"), Some(&a.token)).await;
    assert_eq!(st, 200);
    assert_eq!(room["owner_id"], a.user_id.as_str());
    let missed: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM missed_calls WHERE user_id = $1::uuid AND NOT seen",
    )
    .bind(&c.user_id)
    .fetch_one(&app.db)
    .await
    .unwrap();
    assert_eq!(missed, 1);
    let (st, body) = app
        .post(
            "/api/users/me/missed-calls/acknowledge",
            Some(&c.token),
            json!({}),
        )
        .await;
    assert_eq!(st, 200);
    assert_eq!(body, json!({"ok": true}));
    let missed: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM missed_calls WHERE user_id = $1::uuid AND NOT seen",
    )
    .bind(&c.user_id)
    .fetch_one(&app.db)
    .await
    .unwrap();
    assert_eq!(missed, 0);
}

#[sqlx::test(migrations = "./migrations")]
async fn ics_export(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let a = app.new_org("alfa.test").await;
    let c = app.add_member(&a, "carla", "member").await;
    let d = app.add_member(&a, "dario", "member").await;
    let (_, m) = app
        .post(
            "/api/meetings",
            Some(&a.token),
            json!({"title": "Plano; fase 1, rev", "starts_at": "2030-01-02T09:30:00Z",
                   "duration_min": 45, "invitee_ids": [c.user_id]}),
        )
        .await;
    let id = m["id"].as_str().unwrap();
    for tok in [&a.token, &c.token] {
        let r = app
            .raw(
                reqwest::Method::GET,
                &format!("/api/meetings/{id}/calendar.ics"),
                &[("Authorization", &format!("Bearer {tok}"))],
                None,
            )
            .await;
        assert_eq!(r.status, 200);
        assert_eq!(
            r.header("content-type").as_deref(),
            Some("text/calendar; charset=utf-8")
        );
        assert!(r
            .header("content-disposition")
            .unwrap()
            .contains("reuniao.ics"));
        assert!(r.text.starts_with("BEGIN:VCALENDAR\r\n"));
        assert!(r.text.contains(&format!("UID:{id}@delonix-meet\r\n")));
        assert!(r.text.contains("DTSTART:20300102T093000Z\r\n"));
        assert!(r.text.contains("DURATION:PT45M\r\n"));
        assert!(
            r.text.contains("SUMMARY:Plano\\; fase 1\\, rev\r\n"),
            "{}",
            r.text
        );
    }
    // Quem não é dono nem convidado não fica a saber que a reunião existe.
    let (st, body) = app
        .get(&format!("/api/meetings/{id}/calendar.ics"), Some(&d.token))
        .await;
    assert_eq!(st, 404);
    assert_denied("ics de reunião alheia", st, &body, "Plano");
    let (st, _) = app
        .get(
            &format!("/api/meetings/{INVENTED_ID}/calendar.ics"),
            Some(&a.token),
        )
        .await;
    assert_eq!(st, 404);
}

// ---------------------------------------------------------------------------
//  Convidados, resposta, acta
// ---------------------------------------------------------------------------

#[sqlx::test(migrations = "./migrations")]
async fn invitees_and_respond(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let a = app.new_org("alfa.test").await;
    let c = app.add_member(&a, "carla", "member").await;
    let d = app.add_member(&a, "dario", "member").await;
    let m = app
        .new_meeting(&a, "Convites", &[&c.user_id, &d.user_id])
        .await;
    let id = m["id"].as_str().unwrap();
    let inv = format!("/api/meetings/{id}/invitees");
    let resp = format!("/api/meetings/{id}/invitees/me");

    let (st, list) = app.get(&inv, Some(&a.token)).await;
    assert_eq!(st, 200);
    assert_eq!(list.as_array().unwrap().len(), 2);
    assert!(list
        .as_array()
        .unwrap()
        .iter()
        .all(|i| i["status"] == "pending" && i["responded_at"].is_null()));
    // Só o anfitrião lista.
    let (st, _) = app.get(&inv, Some(&c.token)).await;
    assert_eq!(st, 401);
    let (st, _) = app
        .get(
            &format!("/api/meetings/{INVENTED_ID}/invitees"),
            Some(&a.token),
        )
        .await;
    assert_eq!(st, 404);

    let (st, _) = app
        .put(&resp, Some(&c.token), json!({"status": "maybe"}))
        .await;
    assert_eq!(st, 400);
    let (st, body) = app
        .put(&resp, Some(&d.token), json!({"status": "declined"}))
        .await;
    assert_eq!(st, 400, "recusa sem motivo: {body}");
    let (st, body) = app
        .put(&resp, Some(&c.token), json!({"status": "accepted"}))
        .await;
    assert_eq!(st, 200);
    assert_eq!(body, json!({"ok": true}));
    let (st, _) = app
        .put(
            &resp,
            Some(&d.token),
            json!({"status": "declined", "reason": " férias "}),
        )
        .await;
    assert_eq!(st, 200);
    // O anfitrião não é convidado: 404.
    let (st, _) = app
        .put(&resp, Some(&a.token), json!({"status": "accepted"}))
        .await;
    assert_eq!(st, 404);

    let (_, list) = app.get(&inv, Some(&a.token)).await;
    let by_user = |u: &str| {
        list.as_array()
            .unwrap()
            .iter()
            .find(|i| i["user_id"] == u)
            .unwrap()
            .clone()
    };
    assert_eq!(by_user(&c.user_id)["status"], "accepted");
    assert_eq!(by_user(&d.user_id)["status"], "declined");
    assert_eq!(by_user(&d.user_id)["decline_reason"], "férias");
    assert!(by_user(&d.user_id)["responded_at"].is_string());
    let (_, mine) = app.get("/api/meetings", Some(&d.token)).await;
    assert_eq!(mine[0]["my_status"], "declined");
}

#[sqlx::test(migrations = "./migrations")]
async fn minutes_by_id_and_by_room_and_notes(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let a = app.new_org("alfa.test").await;
    let c = app.add_member(&a, "carla", "member").await;
    let d = app.add_member(&a, "dario", "member").await;
    let m = app.new_meeting(&a, "Acta", &[&c.user_id]).await;
    let id = m["id"].as_str().unwrap();

    let (st, body) = app
        .put(
            &format!("/api/meetings/{id}/minutes"),
            Some(&c.token),
            json!({"minutes": "  decisões  ", "transcript": "t"}),
        )
        .await;
    assert_eq!(st, 200, "o convidado escreve a acta: {body}");
    let (st, _) = app
        .put(
            &format!("/api/meetings/{id}/minutes"),
            Some(&d.token),
            json!({"minutes": "forjada"}),
        )
        .await;
    assert_eq!(st, 401);
    let (_, list) = app.get("/api/meetings", Some(&a.token)).await;
    assert_eq!(list[0]["minutes"], "decisões");

    // Por sala: precisa de a reunião ter sala (start) e de ter participado.
    let (_, s) = app
        .post(
            &format!("/api/meetings/{id}/start"),
            Some(&a.token),
            json!({}),
        )
        .await;
    let code = s["code"].as_str().unwrap();
    let (st, _) = app
        .put(
            &format!("/api/rooms/{code}/minutes"),
            Some(&a.token),
            json!({"minutes": "pela sala", "transcript": "tx"}),
        )
        .await;
    assert_eq!(st, 200);
    let (st, _) = app
        .put(
            &format!("/api/rooms/{code}/minutes"),
            Some(&d.token),
            json!({"minutes": "forjada"}),
        )
        .await;
    assert_eq!(st, 404);
    // A escrita é `PUT` (singleton): o `POST` antigo já não existe.
    let (st, _) = app
        .post(
            &format!("/api/rooms/{code}/minutes"),
            Some(&a.token),
            json!({"minutes": "pelo método antigo"}),
        )
        .await;
    assert_eq!(st, 405);

    // Leitura (`GET …/minutes`, antes `/notes`): só quem PARTICIPOU (join) na sala.
    let (st, _) = app
        .get(&format!("/api/rooms/{code}/minutes"), Some(&a.token))
        .await;
    assert_eq!(st, 401, "arrancar não é participar");
    let (st, _) = app
        .post(
            &format!("/api/rooms/{code}/join"),
            Some(&a.token),
            json!({}),
        )
        .await;
    assert_eq!(st, 200);
    let (st, notes) = app
        .get(&format!("/api/rooms/{code}/minutes"), Some(&a.token))
        .await;
    assert_eq!(st, 200);
    assert_eq!(
        notes,
        json!({"title": "Acta", "minutes": "pela sala", "transcript": "tx"})
    );
}

// ---------------------------------------------------------------------------
//  Agenda e plano de acção
// ---------------------------------------------------------------------------

#[sqlx::test(migrations = "./migrations")]
async fn agenda_crud_and_permissions(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let a = app.new_org("alfa.test").await;
    let c = app.add_member(&a, "carla", "member").await;
    let d = app.add_member(&a, "dario", "member").await;
    let m = app.new_meeting(&a, "Agenda", &[&c.user_id]).await;
    let base = format!("/api/meetings/{}/agenda-items", m["id"].as_str().unwrap());

    let (st, list) = app.get(&base, Some(&c.token)).await;
    assert_eq!(st, 200);
    assert_eq!(list, json!([]));
    let (st, _) = app.get(&base, Some(&d.token)).await;
    assert_eq!(st, 404);

    let (st, _) = app
        .post(&base, Some(&c.token), json!({"topic": "do convidado"}))
        .await;
    assert_eq!(st, 403, "só o anfitrião acrescenta");
    for bad in [json!({"topic": " "}), json!({"topic": "t".repeat(201)})] {
        let (st, _) = app.post(&base, Some(&a.token), bad).await;
        assert_eq!(st, 400);
    }
    let (st, i1) = app
        .post(
            &base,
            Some(&a.token),
            json!({"topic": " Abertura ", "duration_min": 999}),
        )
        .await;
    assert_eq!(st, 200, "{i1}");
    assert_eq!(i1["topic"], "Abertura");
    assert_eq!(i1["position"], 1);
    assert_eq!(i1["duration_min"], 480, "clamp a 480");
    assert_eq!(i1["done"], false);
    let (_, i2) = app
        .post(&base, Some(&a.token), json!({"topic": "Fecho"}))
        .await;
    assert_eq!(i2["position"], 2);
    assert_eq!(i2["duration_min"], 5, "duração por omissão");

    let item = format!("{base}/{}", i1["id"].as_str().unwrap());
    // O convidado marca como feito, mas não edita o tópico.
    let (st, it) = app
        .patch(&item, Some(&c.token), json!({"done": true}))
        .await;
    assert_eq!(st, 200, "{it}");
    assert_eq!(it["done"], true);
    assert_eq!(it["done_by_id"], c.user_id.as_str());
    let (st, _) = app
        .patch(&item, Some(&c.token), json!({"topic": "x"}))
        .await;
    assert_eq!(st, 403);
    let (st, _) = app
        .patch(&item, Some(&d.token), json!({"done": false}))
        .await;
    assert_eq!(st, 404);
    let (st, it) = app
        .patch(
            &item,
            Some(&a.token),
            json!({"topic": "Abertura formal", "position": 7, "done": false}),
        )
        .await;
    assert_eq!(st, 200);
    assert_eq!(it["topic"], "Abertura formal");
    assert_eq!(it["position"], 7);
    assert!(it["done_at"].is_null());
    let (st, _) = app
        .patch(
            &format!("{base}/{INVENTED_ID}"),
            Some(&a.token),
            json!({"done": true}),
        )
        .await;
    assert_eq!(st, 404);

    let (st, _) = app.delete(&item, Some(&c.token)).await;
    assert_eq!(st, 403);
    let (st, body) = app.delete(&item, Some(&a.token)).await;
    assert_eq!(st, 200);
    assert_eq!(body, json!({"ok": true}));
    let (_, list) = app.get(&base, Some(&a.token)).await;
    assert_eq!(ids(&list), vec![i2["id"].as_str().unwrap()]);
}

#[sqlx::test(migrations = "./migrations")]
async fn action_plan_crud_and_permissions(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let a = app.new_org("alfa.test").await;
    let c = app.add_member(&a, "carla", "member").await;
    let d = app.add_member(&a, "dario", "member").await;
    let m = app.new_meeting(&a, "5W2H", &[&c.user_id]).await;
    let id = m["id"].as_str().unwrap();
    let plan = format!("/api/meetings/{id}/action-plan");
    let items = format!("{plan}/items");

    let (st, body) = app.get(&plan, Some(&c.token)).await;
    assert_eq!(st, 200);
    assert!(body.is_null(), "sem plano ainda: {body}");
    let (st, _) = app.get(&plan, Some(&d.token)).await;
    assert_eq!(st, 404);

    let (st, _) = app.put(&plan, Some(&c.token), json!({"goal": "x"})).await;
    assert_eq!(st, 403);
    let (st, p) = app
        .put(&plan, Some(&a.token), json!({"goal": "  Entregar v1  "}))
        .await;
    assert_eq!(st, 200, "{p}");
    assert_eq!(p["goal"], "Entregar v1");
    assert_eq!(p["meeting_id"], id);
    assert_eq!(p["items"], json!([]));

    let (st, _) = app
        .post(
            &items,
            Some(&a.token),
            json!({"what": "x", "status": "blocked"}),
        )
        .await;
    assert_eq!(st, 400);
    let (st, _) = app.post(&items, Some(&c.token), json!({"what": "x"})).await;
    assert_eq!(st, 403);
    let (st, it) = app
        .post(
            &items,
            Some(&a.token),
            json!({"what": " Rever contrato ", "who_id": c.user_id, "when_date": "2030-05-01"}),
        )
        .await;
    assert_eq!(st, 200, "{it}");
    assert_eq!(it["what"], "Rever contrato");
    assert_eq!(
        it["who_name"], "carla-alfa.test",
        "resolve o nome pelo who_id"
    );
    assert_eq!(it["status"], "todo");
    assert_eq!(it["position"], 1);
    assert_eq!(it["when_date"], "2030-05-01");
    let item = format!("{items}/{}", it["id"].as_str().unwrap());

    // Convidado muda o estado, não o conteúdo.
    let (st, x) = app
        .patch(&item, Some(&c.token), json!({"status": "doing"}))
        .await;
    assert_eq!(st, 200);
    assert_eq!(x["status"], "doing");
    let (st, _) = app
        .patch(&item, Some(&c.token), json!({"what": "outra coisa"}))
        .await;
    assert_eq!(st, 403);
    let (st, _) = app
        .patch(&item, Some(&a.token), json!({"status": "parado"}))
        .await;
    assert_eq!(st, 400);
    let (st, x) = app
        .patch(
            &item,
            Some(&a.token),
            json!({"what": "Assinar", "who_name": "Externo", "status": "done"}),
        )
        .await;
    assert_eq!(st, 200);
    assert_eq!(x["what"], "Assinar");
    assert_eq!(x["who_name"], "Externo");
    assert_eq!(x["status"], "done");
    let (st, _) = app
        .patch(
            &format!("{items}/{INVENTED_ID}"),
            Some(&a.token),
            json!({"status": "done"}),
        )
        .await;
    assert_eq!(st, 404);

    let (_, p) = app.get(&plan, Some(&a.token)).await;
    assert_eq!(p["items"].as_array().unwrap().len(), 1);

    // O item tem de ser DESTA reunião: com o id de outra reunião do mesmo
    // dono, 404 — e o item não muda.
    let other = app.new_meeting(&a, "outra", &[]).await;
    let foreign_path = format!(
        "/api/meetings/{}/action-plan/items/{}",
        other["id"].as_str().unwrap(),
        it["id"].as_str().unwrap()
    );
    let (st, body) = app
        .patch(&foreign_path, Some(&a.token), json!({"what": "desviado"}))
        .await;
    assert_eq!(st, 404, "item de outra reunião: {body}");
    let (st, _) = app.delete(&foreign_path, Some(&a.token)).await;
    assert_eq!(st, 404);

    let (_, p) = app.get(&plan, Some(&a.token)).await;
    assert_eq!(p["items"][0]["what"], "Assinar");

    let (st, _) = app.delete(&item, Some(&c.token)).await;
    assert_eq!(st, 403);
    let r = app
        .raw(
            reqwest::Method::DELETE,
            &item,
            &[("Authorization", &format!("Bearer {}", a.token))],
            None,
        )
        .await;
    assert_eq!(r.status, 204);
    assert_eq!(r.text, "", "204 sem corpo");
    let (st, _) = app.delete(&item, Some(&a.token)).await;
    assert_eq!(st, 404);
}

// ---------------------------------------------------------------------------
//  Isolamento cross-org nas rotas por id
// ---------------------------------------------------------------------------

#[sqlx::test(migrations = "./migrations")]
async fn cross_org_meeting_routes_are_denied(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let a = app.new_org("alfa.test").await;
    let b = app.new_org("beta.test").await;
    let m = app.new_meeting(&b, "reunião privada da B", &[]).await;
    let id = m["id"].as_str().unwrap();
    let t = Some(a.token.as_str());
    let leak = "reunião privada da B";

    let (_, ag) = app
        .post(
            &format!("/api/meetings/{id}/agenda-items"),
            Some(&b.token),
            json!({"topic": "tópico da B"}),
        )
        .await;
    let ag_item = format!(
        "/api/meetings/{id}/agenda-items/{}",
        ag["id"].as_str().unwrap()
    );

    let checks: Vec<(u16, Value, &str)> = vec![
        {
            let (s, v) = app.delete(&format!("/api/meetings/{id}"), t).await;
            (s, v, "DELETE")
        },
        {
            let (s, v) = app.get(&format!("/api/meetings/{id}"), t).await;
            (s, v, "GET")
        },
        {
            let (s, v) = app
                .put(
                    &format!("/api/meetings/{id}/minutes"),
                    t,
                    json!({"minutes": "acta forjada"}),
                )
                .await;
            (s, v, "minutes")
        },
        {
            let (s, v) = app
                .get(&format!("/api/meetings/{id}/agenda-items"), t)
                .await;
            (s, v, "agenda")
        },
        {
            let (s, v) = app
                .post(
                    &format!("/api/meetings/{id}/agenda-items"),
                    t,
                    json!({"topic": "forjado"}),
                )
                .await;
            (s, v, "agenda POST")
        },
        {
            let (s, v) = app.patch(&ag_item, t, json!({"done": true})).await;
            (s, v, "agenda PATCH")
        },
        {
            let (s, v) = app.delete(&ag_item, t).await;
            (s, v, "agenda DELETE")
        },
        {
            let (s, v) = app.get(&format!("/api/meetings/{id}/invitees"), t).await;
            (s, v, "invitees")
        },
        {
            let (s, v) = app.get(&format!("/api/meetings/{id}/action-plan"), t).await;
            (s, v, "action-plan")
        },
        {
            let (s, v) = app
                .put(
                    &format!("/api/meetings/{id}/action-plan"),
                    t,
                    json!({"goal": "forjado"}),
                )
                .await;
            (s, v, "action-plan PUT")
        },
        {
            let (s, v) = app
                .post(
                    &format!("/api/meetings/{id}/action-plan/items"),
                    t,
                    json!({"what": "tarefa forjada"}),
                )
                .await;
            (s, v, "action-plan items")
        },
        {
            let (s, v) = app
                .get(&format!("/api/meetings/{id}/calendar.ics"), t)
                .await;
            (s, v, "ics")
        },
        {
            let (s, v) = app
                .put(
                    &format!("/api/meetings/{id}/invitees/me"),
                    t,
                    json!({"status": "accepted"}),
                )
                .await;
            (s, v, "respond")
        },
        {
            let (s, v) = app
                .post(&format!("/api/meetings/{id}/start"), t, json!({}))
                .await;
            (s, v, "start")
        },
    ];
    for (st, body, what) in checks {
        assert_denied(what, st, &body, leak);
        // Não é só «não-2xx»: um 405 queria dizer que o teste bateu no método
        // errado e não provou nada.
        assert_ne!(st, 405, "{what}: método errado no teste");
    }

    // O estado da B sobreviveu a tudo.
    let (_, list) = app.get("/api/meetings", Some(&b.token)).await;
    assert_eq!(ids(&list), vec![id]);
    assert_eq!(list[0]["minutes"], "");
    assert!(list[0]["room_code"].is_null(), "A não arrancou a reunião");
    let (_, agenda) = app
        .get(&format!("/api/meetings/{id}/agenda-items"), Some(&b.token))
        .await;
    assert_eq!(agenda[0]["done"], false);
    let (_, plan) = app
        .get(&format!("/api/meetings/{id}/action-plan"), Some(&b.token))
        .await;
    assert!(plan.is_null());
    // E A não vê a reunião da B.
    let (_, list) = app.get("/api/meetings", t).await;
    assert_eq!(list, json!([]));
}

/// R125 (fechada): um PATCH com corpo vazio, ou só com campos desconhecidos
/// (o `{"done": true}` do e2e), saltava as verificações e devolvia o item a
/// qualquer sessão. Agora a pertença à reunião verifica-se sempre primeiro.
#[sqlx::test(migrations = "./migrations")]
async fn patch_action_item_refuses_session_from_other_org(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let a = app.new_org("alfa.test").await;
    let b = app.new_org("beta.test").await;
    let m = app.new_meeting(&b, "reunião da B", &[]).await;
    let (_, it) = app
        .post(
            &format!(
                "/api/meetings/{}/action-plan/items",
                m["id"].as_str().unwrap()
            ),
            Some(&b.token),
            json!({"what": "segredo comercial da B"}),
        )
        .await;
    let item_id = it["id"].as_str().unwrap();
    // Pelo caminho da reunião da B...
    let (st, body) = app
        .patch(
            &format!(
                "/api/meetings/{}/action-plan/items/{item_id}",
                m["id"].as_str().unwrap()
            ),
            Some(&a.token),
            json!({"done": true}),
        )
        .await;
    assert!(!(200..300).contains(&st), "{st} {body}");
    assert!(!body.to_string().contains("segredo comercial"), "{body}");
    // ... e pelo caminho de uma reunião da própria A com o id do item da B.
    let own = app.new_meeting(&a, "reunião da A", &[]).await;
    let (st, body) = app
        .patch(
            &format!(
                "/api/meetings/{}/action-plan/items/{item_id}",
                own["id"].as_str().unwrap()
            ),
            Some(&a.token),
            json!({"done": true}),
        )
        .await;
    assert_eq!(st, 404, "{body}");
    assert!(!body.to_string().contains("segredo comercial"), "{body}");
}

/// DÍVIDA: `meetings::create` aceita `invitee_ids` de QUALQUER utilizador,
/// sem verificar que partilha organização com o anfitrião. A reunião aparece
/// na lista do utilizador de outra org (título incluído), e ele passa a poder
/// ler a agenda e escrever a acta.
#[sqlx::test(migrations = "./migrations")]
async fn create_meeting_current_behavior_accepts_foreign_org_invitee(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let a = app.new_org("alfa.test").await;
    let b = app.new_org("beta.test").await;
    let m = app
        .new_meeting(&a, "convite atravessado", &[&b.user_id])
        .await;
    let id = m["id"].as_str().unwrap();
    let (_, list) = app.get("/api/meetings", Some(&b.token)).await;
    assert_eq!(ids(&list), vec![id]);
    assert_eq!(list[0]["title"], "convite atravessado");
    let (st, _) = app
        .get(&format!("/api/meetings/{id}/agenda-items"), Some(&b.token))
        .await;
    assert_eq!(st, 200);
}

#[sqlx::test(migrations = "./migrations")]
async fn quarantine_analytics(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let a = app.new_org("alfa.test").await;
    let b = app.new_org("beta.test").await;
    let c = app.add_member(&a, "carla", "member").await;
    // Reunião que já começou com convidado pendente: o sweep põe-no em
    // quarentena.
    let (st, _) = app
        .post(
            "/api/meetings",
            Some(&a.token),
            json!({"title": "passada", "starts_at": (Utc::now() - Duration::hours(1)).to_rfc3339(),
                   "invitee_ids": [c.user_id]}),
        )
        .await;
    assert_eq!(st, 200);
    let path = format!("/api/orgs/{}/analytics/quarantine", a.org());
    let (st, rows) = app
        .get(&format!("{path}?period=week"), Some(&a.token))
        .await;
    assert_eq!(st, 200, "{rows}");
    assert_eq!(rows[0]["user_id"], c.user_id.as_str());
    assert_eq!(rows[0]["count"], 1);
    // Sem `period`: mês por omissão, e o mesmo resultado.
    let (st, rows) = app.get(&path, Some(&a.token)).await;
    assert_eq!(st, 200, "{rows}");
    assert_eq!(rows[0]["user_id"], c.user_id.as_str());
    // Membro sem papel de admin: 403.
    let (st, body) = app.get(&path, Some(&c.token)).await;
    assert_eq!(st, 403, "{body}");
    assert_denied("quarentena para membro", st, &body, &c.user_id);
    // Não-membro (admin de outra org) pelo caminho da A: 404, sem fuga.
    let (st, body) = app.get(&path, Some(&b.token)).await;
    assert_eq!(st, 404, "{body}");
    assert_denied("quarentena da A para a B", st, &body, &c.user_id);
    // Controlo positivo da B: a org dela responde, e vazia.
    let (st, rows) = app
        .get(
            &format!("/api/orgs/{}/analytics/quarantine", b.org()),
            Some(&b.token),
        )
        .await;
    assert_eq!(st, 200);
    assert_eq!(rows, json!([]));
}
