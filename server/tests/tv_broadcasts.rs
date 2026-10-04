//! Sessões de emissão de um canal de TV (RFC-0001, Fase 1) contra Postgres
//! real: pedir/parar, uma só emissão por terminar por canal, a separação entre
//! PREPARAR (`broadcast.manage_channels`) e PÔR NO AR (`broadcast.go_live`), o
//! isolamento entre organizações e a auditoria.
mod common;

use common::{Account, TestApp};
use serde_json::{json, Value};

fn channels(org: &str) -> String {
    format!("/api/orgs/{org}/tv/channels")
}

async fn new_channel(app: &TestApp, who: &Account, slug: &str) -> String {
    let (st, c) = app
        .post(
            &channels(who.org()),
            Some(&who.token),
            json!({"slug": slug, "name": slug}),
        )
        .await;
    assert_eq!(st, 201, "{c}");
    format!("{}/{}", channels(who.org()), c["id"].as_str().unwrap())
}

async fn audit_count(app: &TestApp, action: &str) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM audit_logs WHERE action = $1")
        .bind(action)
        .fetch_one(&app.db)
        .await
        .unwrap()
}

#[sqlx::test(migrations = "./migrations")]
async fn request_stop_contract_and_single_active_session(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let a = app.new_org("alfa.ao").await;
    let ch = new_channel(&app, &a, "tv-alfa").await;
    let bc = format!("{ch}/broadcasts");

    let res = app
        .http
        .post(app.url(&bc))
        .bearer_auth(&a.token)
        .json(&json!({}))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 201);
    let location = res.headers()["location"].to_str().unwrap().to_string();
    let s: Value = res.json().await.unwrap();
    assert_eq!(location, format!("{bc}/{}", s["id"].as_str().unwrap()));
    // Pedir não é estar no ar: sem executor, fica `requested`.
    assert_eq!(s["desired_state"], "live");
    assert_eq!(
        s["state"], "requested",
        "nada diz «no ar» sem um executor que o prove"
    );
    assert!(s["started_at"].is_null() && s["ended_at"].is_null());

    // Dois «pôr no ar» em corrida não criam duas emissões.
    let (st, v) = app.post(&bc, Some(&a.token), json!({})).await;
    assert_eq!(st, 409, "{v}");
    assert_eq!(v["code"], "tv.broadcast.already_active");

    // Um canal com emissão por terminar não se apaga.
    let (st, v) = app.delete(&ch, Some(&a.token)).await;
    assert_eq!(st, 409, "{v}");
    assert_eq!(v["code"], "tv.channel.on_air");
    assert_eq!(app.get(&ch, Some(&a.token)).await.0, 200);

    // Parar uma sessão que ninguém arrancou fecha-a logo; é idempotente.
    let (st, stopped) = app
        .post(&format!("{location}/stop"), Some(&a.token), json!({}))
        .await;
    assert_eq!(st, 200, "{stopped}");
    assert_eq!(stopped["desired_state"], "stopped");
    assert_eq!(stopped["state"], "ended");
    assert!(!stopped["ended_at"].is_null());
    let (st, again) = app
        .post(&format!("{location}/stop"), Some(&a.token), json!({}))
        .await;
    assert_eq!(st, 200);
    assert_eq!(
        again["ended_at"], stopped["ended_at"],
        "parar duas vezes não reescreve a história"
    );
    assert_eq!(audit_count(&app, "tv.broadcast.stop_requested").await, 1);
    assert_eq!(audit_count(&app, "tv.broadcast.requested").await, 1);

    // O canal ficou livre: nova emissão, e agora o canal já se apaga depois de parada.
    let (st, second) = app.post(&bc, Some(&a.token), json!({})).await;
    assert_eq!(st, 201, "{second}");
    let (_, list) = app.get(&bc, Some(&a.token)).await;
    let items = list["items"].as_array().unwrap();
    assert_eq!(items.len(), 2);
    assert_eq!(items[0]["id"], second["id"], "a mais recente primeiro");
    let (_, one) = app.get(&location, Some(&a.token)).await;
    assert_eq!(one["state"], "ended");
    app.post(
        &format!("{bc}/{}/stop", second["id"].as_str().unwrap()),
        Some(&a.token),
        json!({}),
    )
    .await;
    assert_eq!(app.delete(&ch, Some(&a.token)).await.0, 204);
}

#[sqlx::test(migrations = "./migrations")]
async fn preparing_and_going_live_are_separate_capabilities(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let owner = app.new_org("alfa.ao").await;
    let ch = new_channel(&app, &owner, "tv-alfa").await;
    let bc = format!("{ch}/broadcasts");

    let mk_role = |name: &'static str, caps: Value| {
        let app = &app;
        let owner = &owner;
        async move {
            let (st, r) = app
                .post(
                    &format!("/api/orgs/{}/roles", owner.org()),
                    Some(&owner.token),
                    json!({"name": name, "capabilities": caps}),
                )
                .await;
            assert_eq!(st, 201, "{r}");
            r["id"].as_str().unwrap().to_string()
        }
    };
    let assign = |user: &Account, role: String| {
        let app = &app;
        let owner = &owner;
        let uid = user.user_id.clone();
        async move {
            let (st, b) = app
                .put(
                    &format!("/api/orgs/{}/members/{uid}/role", owner.org()),
                    Some(&owner.token),
                    json!({"role_id": role}),
                )
                .await;
            assert_eq!(st, 200, "{b}");
        }
    };

    // Quem PREPARA mas não põe no ar.
    let producer = app.add_member(&owner, "prod", "member").await;
    assign(
        &producer,
        mk_role("Produtor", json!({"broadcast.manage_channels": "allow"})).await,
    )
    .await;
    assert_eq!(
        app.post(
            &channels(owner.org()),
            Some(&producer.token),
            json!({"slug": "prep", "name": "p"})
        )
        .await
        .0,
        201,
        "o produtor prepara canais"
    );
    let (st, v) = app.post(&bc, Some(&producer.token), json!({})).await;
    assert!(
        !(200..300).contains(&st),
        "preparar não dá o direito de pôr no ar: {st} {v}"
    );
    // …mas vê o histórico, porque prepara.
    assert_eq!(app.get(&bc, Some(&producer.token)).await.0, 200);

    // Quem PÕE NO AR mas não prepara.
    let director = app.add_member(&owner, "real", "member").await;
    assign(
        &director,
        mk_role("Realizador", json!({"broadcast.go_live": "allow"})).await,
    )
    .await;
    let (st, s) = app.post(&bc, Some(&director.token), json!({})).await;
    assert_eq!(st, 201, "o realizador põe no ar: {s}");
    assert_eq!(
        app.get(&bc, Some(&director.token)).await.0,
        200,
        "e vê o histórico"
    );
    let (st, _) = app
        .post(
            &channels(owner.org()),
            Some(&director.token),
            json!({"slug": "outro", "name": "o"}),
        )
        .await;
    assert!(
        !(200..300).contains(&st),
        "pôr no ar não dá o direito de criar canais"
    );
    assert_eq!(
        app.patch(
            &ch,
            Some(&director.token),
            json!({"version": 1, "name": "x"})
        )
        .await
        .0 / 100,
        4
    );

    // Um membro simples não faz nem uma coisa nem outra.
    let plain = app.add_member(&owner, "ana", "member").await;
    for st in [
        app.post(&bc, Some(&plain.token), json!({})).await.0,
        app.get(&bc, Some(&plain.token)).await.0,
        app.post(
            &format!("{bc}/{}/stop", s["id"].as_str().unwrap()),
            Some(&plain.token),
            json!({}),
        )
        .await
        .0,
    ] {
        assert!(!(200..300).contains(&st), "membro simples: {st}");
    }
    // A sessão do realizador continua por terminar.
    let (_, still) = app
        .get(
            &format!("{bc}/{}", s["id"].as_str().unwrap()),
            Some(&owner.token),
        )
        .await;
    assert_eq!(still["state"], "requested");
    assert_eq!(still["desired_state"], "live");
}

#[sqlx::test(migrations = "./migrations")]
async fn other_org_reaches_nothing(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let a = app.new_org("alfa.ao").await;
    let b = app.new_org("beta.ao").await;
    let ch_b = new_channel(&app, &b, "tv-da-b").await;
    let bc_b = format!("{ch_b}/broadcasts");
    let (_, sb) = app.post(&bc_b, Some(&b.token), json!({})).await;
    let sid = sb["id"].as_str().unwrap();

    let refused = |st: u16| !(200..300).contains(&st);
    assert!(refused(app.post(&bc_b, Some(&a.token), json!({})).await.0));
    assert!(refused(app.get(&bc_b, Some(&a.token)).await.0));
    assert!(refused(
        app.get(&format!("{bc_b}/{sid}"), Some(&a.token)).await.0
    ));
    assert!(refused(
        app.post(&format!("{bc_b}/{sid}/stop"), Some(&a.token), json!({}))
            .await
            .0
    ));
    // Pelo caminho da PRÓPRIA org A, com o canal e a sessão da B: tudo 404.
    let ch_a = new_channel(&app, &a, "tv-da-a").await;
    let id_b = ch_b.rsplit('/').next().unwrap();
    for url in [
        format!("{}/{id_b}/broadcasts", channels(a.org())),
        format!("{}/{id_b}/broadcasts/{sid}", channels(a.org())),
        // A sessão da B pelo canal da A: o `channel_id` também conta.
        format!("{ch_a}/broadcasts/{sid}"),
    ] {
        assert_eq!(app.get(&url, Some(&a.token)).await.0, 404, "{url}");
    }
    assert_eq!(
        app.post(
            &format!("{ch_a}/broadcasts/{sid}/stop"),
            Some(&a.token),
            json!({})
        )
        .await
        .0,
        404
    );
    // A sessão da B está intacta.
    let (_, still) = app.get(&format!("{bc_b}/{sid}"), Some(&b.token)).await;
    assert_eq!(still["state"], "requested");
    assert_eq!(still["desired_state"], "live");
}
