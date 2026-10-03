//! «A minha conta» contra Postgres e servidor reais (R204–R207): perfil com
//! campos geridos pelo Odoo, fotografia, preferências de entrada (e a gravação
//! que pede confirmação), preferências de notificação, guia e «Novo PIN».
mod common;

use common::{TestApp, PASSWORD};
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use std::time::Duration;

const PROFILE: &str = "/api/users/me/profile";
const PNG: &[u8] = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR";

/// R204 — o perfil valida tudo antes de escrever, normaliza o telefone como o
/// gateway de SMS, e recusa com código estável os campos que vêm do Odoo.
#[sqlx::test(migrations = "./migrations")]
async fn profile_validates_normalizes_and_protects_odoo_fields(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let a = app.new_org("alfa.test").await;
    let t = Some(a.token.as_str());

    let (st, p) = app.get(PROFILE, t).await;
    assert_eq!(st, 200, "{p}");
    assert_eq!(p["timezone"], "Africa/Luanda");
    assert_eq!(p["timezone_utc_offset_minutes"], 60);
    assert_eq!(p["display_name"], p["username"]);
    assert_eq!(p["display_name_set"], false);
    assert!(p["managed_by"].is_null(), "conta local");
    assert_eq!(p["organization"]["role"], "admin");
    assert!(p["avatar_url"].is_null());
    assert!(p["department"].is_null());

    let (st, p) = app
        .patch(
            PROFILE,
            t,
            json!({"display_name": " Ana Mbala ", "job_title": "Directora de Formação",
                   "phone": "923 447 108", "timezone": "Europe/Lisbon", "locale": "pt-ao"}),
        )
        .await;
    assert_eq!(st, 200, "{p}");
    assert_eq!(p["display_name"], "Ana Mbala");
    assert_eq!(p["job_title"], "Directora de Formação");
    assert_eq!(
        p["phone"], "+244923447108",
        "mesma normalização do gateway de SMS"
    );
    assert_eq!(p["phone_source"], "manual");
    assert_eq!(p["timezone"], "Europe/Lisbon");
    assert_eq!(p["locale"], "pt-AO", "forma canónica");
    assert!(p["updated_at"].is_string());
    // o locale novo aparece também no /users/me de sempre
    let (_, me) = app.get("/api/users/me", t).await;
    assert_eq!(me["locale"], "pt-AO");

    // Um campo inválido não deixa os outros meio gravados.
    let (st, e) = app
        .patch(
            PROFILE,
            t,
            json!({"display_name": "Outro", "timezone": "Marte/Olympus"}),
        )
        .await;
    assert_eq!(
        (st, e["code"].as_str()),
        (400, Some("profile.invalid_timezone"))
    );
    let (_, p) = app.get(PROFILE, t).await;
    assert_eq!(p["display_name"], "Ana Mbala", "nada gravado");
    for (body, st_want, code) in [
        (json!({"locale": "de"}), 400, "profile.invalid_locale"),
        (
            json!({"phone": "+351 912 345 678"}),
            422,
            "profile.invalid_phone",
        ),
        (
            json!({"display_name": "a\u{202e}b"}),
            400,
            "profile.invalid_display_name",
        ),
        (
            json!({"email": "novo@alfa.test"}),
            409,
            "profile.field_read_only",
        ),
    ] {
        let (st, e) = app.patch(PROFILE, t, body.clone()).await;
        assert_eq!(
            (st, e["code"].as_str()),
            (st_want, Some(code)),
            "{body}: {e}"
        );
    }
    let (st, _) = app.patch(PROFILE, t, json!({"nickname": "x"})).await;
    assert_eq!(st, 422, "campo desconhecido não é ignorado");
    // "" apaga o telefone e volta ao username
    let (_, p) = app
        .patch(PROFILE, t, json!({"phone": "", "display_name": ""}))
        .await;
    // Apagar à mão continua `manual` (regra de `org::set_member_phone`): a
    // sincronização do directório não volta a preencher o que a pessoa apagou.
    assert!(p["phone"].is_null(), "{p}");
    assert_eq!(p["phone_source"], "manual", "{p}");
    assert_eq!(p["display_name_set"], false);
    // o PATCH antigo também passa a recusar um idioma desconhecido
    let (st, e) = app.patch("/api/users/me", t, json!({"locale": "xx"})).await;
    assert_eq!(
        (st, e["code"].as_str()),
        (400, Some("profile.invalid_locale"))
    );

    // Conta gerida por um Odoo activo: nome legal, correio e departamento são do ERP.
    sqlx::query(
        "UPDATE organizations SET odoo_enabled = TRUE, odoo_url = 'https://erp.alfa.test', odoo_db = 'alfa'
          WHERE id = $1::uuid",
    )
    .bind(a.org())
    .execute(&app.db)
    .await
    .unwrap();
    sqlx::query(
        "UPDATE users SET odoo_managed = TRUE, odoo_org_id = $1::uuid, legal_name = 'Ana Paula Mbala'
          WHERE id = $2::uuid",
    )
    .bind(a.org())
    .bind(&a.user_id)
    .execute(&app.db)
    .await
    .unwrap();
    let (_, p) = app.get(PROFILE, t).await;
    assert_eq!(p["managed_by"]["source"], "odoo", "{p}");
    assert_eq!(p["managed_by"]["sync_interval_minutes"], 60);
    assert_eq!(p["managed_by"]["odoo_url"], "https://erp.alfa.test");
    assert_eq!(p["legal_name"], "Ana Paula Mbala");
    assert_eq!(p["password"]["managed_by_odoo"], true);
    for field in ["legal_name", "email", "department"] {
        let (st, e) = app.patch(PROFILE, t, json!({ field: "x" })).await;
        assert_eq!(
            (st, e["code"].as_str()),
            (409, Some("profile.field_managed_by_odoo")),
            "{field}: {e}"
        );
    }
    // A password de uma conta gerida não se muda localmente.
    let (st, e) = app
        .patch("/api/users/me", t, json!({"password": "OutraPassword123!"}))
        .await;
    assert_eq!(
        (st, e["code"].as_str()),
        (409, Some("profile.field_managed_by_odoo"))
    );
    // Os campos editáveis continuam editáveis.
    let (st, _) = app.patch(PROFILE, t, json!({"job_title": "CTO"})).await;
    assert_eq!(st, 200);

    let n: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM audit_logs WHERE action = 'profile.updated' AND actor_id = $1::uuid
            AND target NOT LIKE '%923%'",
    )
    .bind(&a.user_id)
    .fetch_one(&app.db)
    .await
    .unwrap();
    assert!(n >= 2, "auditado sem o valor do telefone");
}

/// R205 — fotografia: tipo pelos bytes, limites, remover; a de outra pessoa só
/// se partilharmos organização.
#[sqlx::test(migrations = "./migrations")]
async fn avatar_is_sniffed_limited_and_scoped(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let a = app.new_org("alfa.test").await;
    let colleague = app.add_member(&a, "bento", "member").await;
    let outsider = app.new_org("beta.test").await;
    let put = |token: &str, body: Vec<u8>, ct: &str| {
        app.http
            .put(app.url("/api/users/me/avatar"))
            .bearer_auth(token)
            .header("content-type", ct)
            .body(body)
            .send()
    };

    let res = put(&a.token, b"<svg onload=alert(1)>".to_vec(), "image/png")
        .await
        .unwrap();
    assert_eq!(res.status(), 422);
    let e: Value = res.json().await.unwrap();
    assert_eq!(e["code"], "profile.avatar_unsupported_type");
    let mut big = PNG.to_vec();
    big.resize(1024 * 1024 + 1, 0);
    let res = put(&a.token, big, "image/png").await.unwrap();
    assert_eq!(res.status(), 422, "acima de 1 MiB, com código estável");
    let res = put(&a.token, vec![0u8; 3 * 1024 * 1024], "image/png")
        .await
        .unwrap();
    assert_eq!(res.status(), 413, "muito acima: corte do servidor");

    let res = put(&a.token, PNG.to_vec(), "application/octet-stream")
        .await
        .unwrap();
    assert_eq!(res.status(), 200);
    let p: Value = res.json().await.unwrap();
    let url = p["avatar_url"].as_str().unwrap().to_string();
    assert!(
        url.starts_with(&format!("/api/users/{}/avatar?v=", a.user_id)),
        "{url}"
    );

    for (who, token, want) in [
        ("o próprio", a.token.as_str(), 200),
        ("colega", colleague.token.as_str(), 200),
        ("outra org", outsider.token.as_str(), 404),
    ] {
        let res = app
            .http
            .get(app.url(&url))
            .bearer_auth(token)
            .send()
            .await
            .unwrap();
        assert_eq!(res.status(), want, "{who}");
        if want == 200 {
            assert_eq!(res.headers()["content-type"], "image/png", "{who}");
            assert_eq!(res.bytes().await.unwrap().as_ref(), PNG);
        }
    }
    let (st, _) = app.get(&url, None).await;
    assert_eq!(st, 401);

    let (st, _) = app.delete("/api/users/me/avatar", Some(&a.token)).await;
    assert_eq!(st, 204);
    let (st, e) = app.delete("/api/users/me/avatar", Some(&a.token)).await;
    assert_eq!((st, e["code"].as_str()), (404, Some("avatar.not_found")));
    let (_, p) = app.get(PROFILE, Some(&a.token)).await;
    assert!(p["avatar_url"].is_null());
}

/// R206 — preferências de entrada: guardadas, devolvidas no join, e «avisar
/// antes de gravar» imposto pelo servidor (sem confirmação não grava).
#[sqlx::test(migrations = "./migrations")]
async fn join_preferences_reach_the_join_and_recording_needs_confirmation(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let a = app.new_org("alfa.test").await;
    let t = Some(a.token.as_str());
    const JP: &str = "/api/users/me/join-preferences";

    let (st, d) = app.get(JP, t).await;
    assert_eq!(st, 200);
    assert_eq!(
        d["warn_before_recording"], false,
        "omissão = comportamento de hoje"
    );
    let (st, e) = app
        .put(JP, t, json!({"captions_language": "klingon"}))
        .await;
    assert_eq!(
        (st, e["code"].as_str()),
        (400, Some("join_preferences.invalid_captions_language"))
    );
    let (st, _) = app.put(JP, t, json!({"mute": true})).await;
    assert_eq!(st, 422);
    let (st, p) = app
        .put(
            JP,
            t,
            json!({"join_muted": true, "blur_background": true, "captions_always_on": true,
                   "captions_language": "PT", "warn_before_recording": true}),
        )
        .await;
    assert_eq!(st, 200, "{p}");
    assert_eq!(p["captions_language"], "pt");
    // PUT é a representação completa: o que não vem volta à omissão
    let (_, p2) = app.put(JP, t, json!({"warn_before_recording": true})).await;
    assert_eq!(p2["join_muted"], false);

    let (_, room) = app
        .post("/api/rooms", t, json!({"name": "Aula", "topology": "sfu"}))
        .await;
    let (st, join) = app
        .post(
            &format!("/api/rooms/{}/join", room["code"].as_str().unwrap()),
            t,
            json!({}),
        )
        .await;
    assert_eq!(st, 200, "{join}");
    assert_eq!(
        join["join_preferences"]["warn_before_recording"], true,
        "{join}"
    );

    // Anfitrião com a preferência pede para gravar SEM confirmar: o servidor
    // responde a pedir confirmação e não grava.
    let ws_url = format!(
        "{}/ws?token={}",
        app.base.replacen("http://", "ws://", 1),
        join["room_token"].as_str().unwrap()
    );
    let (mut ws, _) = tokio_tungstenite::connect_async(ws_url).await.unwrap();
    ws.send(tokio_tungstenite::tungstenite::Message::Text(
        r#"{"type":"server-record","active":true}"#.into(),
    ))
    .await
    .unwrap();
    let got = tokio::time::timeout(Duration::from_secs(5), async {
        while let Some(Ok(m)) = ws.next().await {
            if let Ok(text) = m.to_text() {
                if text.contains("recording-confirmation-required") {
                    return true;
                }
                assert!(
                    !text.contains("\"server-recording\""),
                    "não pode ter começado: {text}"
                );
            }
        }
        false
    })
    .await;
    assert_eq!(got, Ok(true), "o servidor pede confirmação");
}

/// R207 — preferências de notificação: matriz com entrega honesta, e o
/// `in_app` desligado impede o produtor de criar a notificação.
#[sqlx::test(migrations = "./migrations")]
async fn notification_preferences_are_honest_and_enforced(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let host = app.new_org("alfa.test").await;
    let guest = app.add_member(&host, "bento", "member").await;
    const NP: &str = "/api/users/me/notification-preferences";

    let (st, p) = app.get(NP, Some(&guest.token)).await;
    assert_eq!(st, 200, "{p}");
    let channels: Vec<(String, String)> = p["channels"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| {
            (
                c["channel"].as_str().unwrap().into(),
                c["delivery"].as_str().unwrap().into(),
            )
        })
        .collect();
    assert!(channels.contains(&("in_app".into(), "available".into())));
    assert!(
        channels.contains(&("email".into(), "not_configured".into())),
        "sem adaptador de correio"
    );
    assert!(channels.contains(&("sms".into(), "not_configured".into())));
    let invited = p["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["kind"] == "meeting.invited")
        .unwrap();
    assert_eq!(invited["in_app"], true);

    for (body, code) in [
        (
            json!({"preferences": [{"kind": "x.y", "channel": "email", "enabled": true}]}),
            "notification_preferences.unknown_kind",
        ),
        (
            json!({"preferences": [{"kind": "call.missed", "channel": "push", "enabled": true}]}),
            "notification_preferences.unknown_channel",
        ),
        (
            json!({"preferences": [
            {"kind": "call.missed", "channel": "sms", "enabled": true},
            {"kind": "call.missed", "channel": "sms", "enabled": false}]}),
            "notification_preferences.duplicate",
        ),
    ] {
        let (st, e) = app.put(NP, Some(&guest.token), body).await;
        assert_eq!((st, e["code"].as_str()), (400, Some(code)), "{e}");
    }
    let invites = || async {
        let (_, inbox) = app
            .get("/api/users/me/notifications", Some(&guest.token))
            .await;
        inbox["items"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|n| n["kind"] == "meeting.invited")
            .count()
    };
    // Controlo positivo: com a omissão, o convite cria notificação.
    app.new_meeting(&host, "Stand-up 1", &[&guest.user_id])
        .await;
    assert_eq!(invites().await, 1, "com in_app ligado há notificação");

    let (st, p) = app
        .put(
            NP,
            Some(&guest.token),
            json!({"preferences": [{"kind": "meeting.invited", "channel": "in_app", "enabled": false}]}),
        )
        .await;
    assert_eq!(st, 200, "{p}");
    let invited = p["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["kind"] == "meeting.invited")
        .unwrap();
    assert_eq!(invited["in_app"], false);

    app.new_meeting(&host, "Stand-up 2", &[&guest.user_id])
        .await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(
        invites().await,
        1,
        "com in_app desligado o segundo convite não cria notificação"
    );
}

/// R207 — guia: passos validados contra a lista versionada, «N de M», saltar e
/// recomeçar; «Novo PIN» só com dial-in, e o antigo deixa de valer.
#[sqlx::test(migrations = "./migrations")]
async fn tour_progress_and_new_pin(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let a = app.new_org("alfa.test").await;
    let t = Some(a.token.as_str());

    let (st, tour) = app.get("/api/users/me/tour", t).await;
    assert_eq!(st, 200, "{tour}");
    assert_eq!(
        (
            tour["enabled"].as_bool(),
            tour["total"].as_u64(),
            tour["completed_count"].as_u64()
        ),
        (Some(true), Some(6), Some(0))
    );
    let (st, e) = app
        .put(
            "/api/users/me/tour/steps/home.inventado",
            t,
            json!({"completed": true}),
        )
        .await;
    assert_eq!((st, e["code"].as_str()), (404, Some("tour.unknown_step")));
    for step in [
        "home.start-now",
        "home.schedule-from-odoo",
        "home.one-room-three-modes",
        "home.start-now",
    ] {
        let (st, _) = app
            .put(
                &format!("/api/users/me/tour/steps/{step}"),
                t,
                json!({"completed": true}),
            )
            .await;
        assert_eq!(st, 200);
    }
    let (_, tour) = app.get("/api/users/me/tour", t).await;
    assert_eq!(tour["completed_count"], 3, "idempotente: {tour}");
    assert_eq!(tour["next_step"], "home.upcoming-sessions");
    let (_, tour) = app
        .patch("/api/users/me/tour", t, json!({"enabled": false}))
        .await;
    assert_eq!(tour["enabled"], false);
    let (_, tour) = app.post("/api/users/me/tour/skip", t, json!({})).await;
    assert!(tour["skipped_at"].is_string());
    let (_, tour) = app.post("/api/users/me/tour/restart", t, json!({})).await;
    assert_eq!(
        (tour["enabled"].as_bool(), tour["completed_count"].as_u64()),
        (Some(true), Some(0))
    );
    assert!(tour["skipped_at"].is_null());

    // «Novo PIN» sem dial-in: 409 estável.
    let (st, e) = app
        .post("/api/users/me/room/rotate-pin", t, json!({}))
        .await;
    assert_eq!(
        (st, e["code"].as_str()),
        (409, Some("personal_room.no_dial_in"))
    );
    let (_, room) = app.get("/api/users/me/room", t).await;
    sqlx::query(
        "WITH d AS (INSERT INTO voice_did (org_id, e164) VALUES ($1::uuid, '+244222640100') RETURNING id)
         INSERT INTO voice_room (org_id, room_code, pin, did_id, created_by)
         SELECT $1::uuid, $2, '842090', d.id, $3::uuid FROM d",
    )
    .bind(a.org())
    .bind(room["code"].as_str().unwrap())
    .bind(&a.user_id)
    .execute(&app.db)
    .await
    .unwrap();
    let (st, r) = app
        .post("/api/users/me/room/rotate-pin", t, json!({}))
        .await;
    assert_eq!(st, 200, "{r}");
    let new_pin = r["dial_in"]["pin"].as_str().unwrap();
    assert_ne!(new_pin, "842090");
    assert_eq!(new_pin.len(), 6);
    let stored: String = sqlx::query_scalar("SELECT pin FROM voice_room WHERE room_code = $1")
        .bind(room["code"].as_str().unwrap())
        .fetch_one(&app.db)
        .await
        .unwrap();
    assert_eq!(stored, new_pin, "o PIN antigo deixou de existir");
    let n: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM audit_logs WHERE action = 'personal_room.pin_rotated' AND actor_id = $1::uuid",
    )
    .bind(&a.user_id)
    .fetch_one(&app.db)
    .await
    .unwrap();
    assert_eq!(n, 1);
    let _ = PASSWORD;
}

/// R204 — o nome legal de uma conta gerida chega pela sincronização do Odoo
/// (e um email no lugar do nome não o apaga).
#[sqlx::test(migrations = "./migrations")]
async fn legal_name_comes_from_the_odoo_sync(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let a = app.new_org("alfa.test").await;
    let (st, tok) = app
        .post(
            &format!("/api/orgs/{}/integrations/odoo/rotate-token", a.org()),
            Some(&a.token),
            json!({}),
        )
        .await;
    assert_eq!(st, 200, "{tok}");
    let key = format!("Bearer {}", tok["token"].as_str().unwrap());
    let body = |name: &str| {
        json!({"company": "Alfa Lda", "admin_email": a.email,
               "users": [{"odoo_uid": 7, "name": name, "email": "ana@alfa.test"}]})
    };
    let auth = [("Authorization", key.as_str())];
    let path = "/api/integrations/odoo/v1/provision";
    let r = app
        .raw(
            reqwest::Method::POST,
            path,
            &auth,
            Some(body("Ana Paula Mbala")),
        )
        .await;
    assert_eq!(r.status, 200, "{}", r.text);
    let legal = || async {
        sqlx::query_scalar::<_, Option<String>>(
            "SELECT legal_name FROM users WHERE email = 'ana@alfa.test'",
        )
        .fetch_one(&app.db)
        .await
        .unwrap()
    };
    assert_eq!(
        legal().await.as_deref(),
        Some("Ana Paula Mbala"),
        "conta criada pela sincronização"
    );
    let r = app
        .raw(
            reqwest::Method::POST,
            path,
            &auth,
            Some(body("Ana Paula Mbala Quissanga")),
        )
        .await;
    assert_eq!(r.status, 200, "{}", r.text);
    assert_eq!(
        legal().await.as_deref(),
        Some("Ana Paula Mbala Quissanga"),
        "a seguinte actualiza"
    );
    let r = app
        .raw(
            reqwest::Method::POST,
            path,
            &auth,
            Some(body("ana@alfa.test")),
        )
        .await;
    assert_eq!(r.status, 200, "{}", r.text);
    assert_eq!(
        legal().await.as_deref(),
        Some("Ana Paula Mbala Quissanga"),
        "um email não é nome legal"
    );
}
