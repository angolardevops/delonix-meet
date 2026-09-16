//! «A minha sala» (G2) contra Postgres real: criação preguiçosa idempotente
//! sob concorrência, alteração, rotação do código (o link antigo morre),
//! isolamento entre pessoas e dial-in só de leitura.
mod common;

use common::{jwt_claims, TestApp};
use serde_json::json;

const ROOM: &str = "/api/users/me/room";

#[sqlx::test(migrations = "./migrations")]
async fn lazy_creation_is_idempotent_under_concurrency(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let a = app.new_org("alfa.test").await;

    let (st, _) = app.get(ROOM, None).await;
    assert_eq!(st, 401, "sem sessão não há sala");

    // Quatro pedidos AO MESMO TEMPO, antes de a sala existir.
    let t = Some(a.token.as_str());
    let ((s1, r1), (s2, r2), (s3, r3), (s4, r4)) = tokio::join!(
        app.get(ROOM, t),
        app.get(ROOM, t),
        app.get(ROOM, t),
        app.get(ROOM, t)
    );
    assert_eq!(
        (s1, s2, s3, s4),
        (200, 200, 200, 200),
        "{r1} {r2} {r3} {r4}"
    );
    for r in [&r2, &r3, &r4] {
        assert_eq!(r1["code"], r["code"], "todos vêem a MESMA sala");
        assert_eq!(r1["id"], r["id"]);
    }
    let n: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM rooms WHERE owner_id = $1::uuid AND is_personal")
            .bind(&a.user_id)
            .fetch_one(&app.db)
            .await
            .unwrap();
    assert_eq!(n, 1, "uma sala pessoal por dono, na base");

    let (st, again) = app.get(ROOM, Some(&a.token)).await;
    assert_eq!(st, 200);
    assert_eq!(again["code"], r1["code"], "estável entre pedidos");
    assert_eq!(again["owner_id"], a.user_id.as_str());
    assert_eq!(again["waiting_room"], true, "nasce com sala de espera");
    assert!(
        again["name"].as_str().unwrap().starts_with("Sala de "),
        "{again}"
    );
    let code = again["code"].as_str().unwrap();
    assert!(
        again["join_url"]
            .as_str()
            .unwrap()
            .ends_with(&format!("/#/r/{code}")),
        "{again}"
    );
    assert!(again["dial_in"].is_null(), "sem sala de voz, sem dial-in");

    // É uma sala como as outras: o código abre os metadados.
    let (st, meta) = app.get(&format!("/api/rooms/{code}"), Some(&a.token)).await;
    assert_eq!(st, 200);
    assert_eq!(meta["id"], r1["id"]);
}

#[sqlx::test(migrations = "./migrations")]
async fn patch_validates_before_writing(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let a = app.new_org("alfa.test").await;

    // PATCH antes do primeiro GET também funciona (cria e altera).
    let (st, r) = app
        .patch(
            ROOM,
            Some(&a.token),
            json!({"name": "  Sala da Ana  ", "waiting_room": false}),
        )
        .await;
    assert_eq!(st, 200, "{r}");
    assert_eq!(r["name"], "Sala da Ana");
    assert_eq!(r["waiting_room"], false);

    let (st, bad) = app
        .patch(
            ROOM,
            Some(&a.token),
            json!({"name": "   ", "waiting_room": true}),
        )
        .await;
    assert_eq!(st, 400, "{bad}");
    assert_eq!(bad["code"], "personal_room.invalid_name");
    let (st, bad) = app
        .patch(ROOM, Some(&a.token), json!({"allow_guests": true}))
        .await;
    // A recusa do corpo é a do axum (422), no envelope de sempre.
    assert_eq!(
        st, 422,
        "um campo que não existe não é ignorado em silêncio: {bad}"
    );
    assert_eq!(bad["code"], "invalid_argument");
    assert!(
        bad["error"].as_str().unwrap().contains("allow_guests"),
        "{bad}"
    );

    let (_, now) = app.get(ROOM, Some(&a.token)).await;
    assert_eq!(now["name"], "Sala da Ana", "sem escrita parcial");
    assert_eq!(now["waiting_room"], false, "sem escrita parcial");

    let (st, r) = app
        .patch(ROOM, Some(&a.token), json!({"waiting_room": true}))
        .await;
    assert_eq!(st, 200);
    assert_eq!(r["waiting_room"], true);
    assert_eq!(r["name"], "Sala da Ana", "o campo omitido fica");
}

#[sqlx::test(migrations = "./migrations")]
async fn rotate_code_kills_the_old_link(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let a = app.new_org("alfa.test").await;
    let (_, before) = app.get(ROOM, Some(&a.token)).await;
    let old = before["code"].as_str().unwrap().to_string();

    let (st, after) = app
        .post(&format!("{ROOM}/rotate-code"), Some(&a.token), json!({}))
        .await;
    assert_eq!(st, 200, "{after}");
    let new = after["code"].as_str().unwrap().to_string();
    assert_ne!(new, old);
    assert_eq!(after["id"], before["id"], "a mesma sala, código novo");

    let (st, v) = app.get(&format!("/api/rooms/{old}"), Some(&a.token)).await;
    assert_eq!(st, 404, "o link antigo deixa de abrir: {v}");
    let (st, _) = app
        .post(&format!("/api/rooms/{old}/join"), Some(&a.token), json!({}))
        .await;
    assert_eq!(st, 404, "nem para entrar");
    let (st, _) = app.get(&format!("/api/rooms/{new}"), Some(&a.token)).await;
    assert_eq!(st, 200);
    let (_, mine) = app.get(ROOM, Some(&a.token)).await;
    assert_eq!(mine["code"], new.as_str());
}

#[sqlx::test(migrations = "./migrations")]
async fn each_person_has_their_own_and_access_rules_are_unchanged(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let a = app.new_org("alfa.test").await;
    let b = app.new_org("beta.test").await;

    let (_, ra) = app
        .patch(ROOM, Some(&a.token), json!({"name": "Da Ana"}))
        .await;
    let (_, rb) = app.get(ROOM, Some(&b.token)).await;
    assert_ne!(ra["id"], rb["id"]);
    assert_eq!(rb["owner_id"], b.user_id.as_str());

    // B só alcança a SUA: alterar e rodar não tocam na de A.
    let (st, _) = app
        .patch(ROOM, Some(&b.token), json!({"name": "Da B"}))
        .await;
    assert_eq!(st, 200);
    let (st, _) = app
        .post(&format!("{ROOM}/rotate-code"), Some(&b.token), json!({}))
        .await;
    assert_eq!(st, 200);
    let (_, ra2) = app.get(ROOM, Some(&a.token)).await;
    assert_eq!(ra2["name"], "Da Ana");
    assert_eq!(ra2["code"], ra["code"], "o código de A não rodou");

    // Com o código, B vê os metadados (o código é a credencial, como em
    // qualquer sala) mas entra pela sala de espera e não pode admitir.
    let code = ra["code"].as_str().unwrap();
    let (st, join) = app
        .post(
            &format!("/api/rooms/{code}/join"),
            Some(&b.token),
            json!({}),
        )
        .await;
    assert_eq!(st, 200, "{join}");
    let claims = jwt_claims(join["room_token"].as_str().unwrap());
    assert_eq!(claims["wait"], true, "{claims}");
    // `owner`/`adm` só vão no token quando são verdadeiros.
    assert_ne!(claims["owner"], true, "{claims}");
    assert_ne!(claims["adm"], true, "{claims}");
    let (_, join_a) = app
        .post(
            &format!("/api/rooms/{code}/join"),
            Some(&a.token),
            json!({}),
        )
        .await;
    assert_eq!(
        jwt_claims(join_a["room_token"].as_str().unwrap())["owner"],
        true
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn dial_in_is_read_only_and_scoped_to_the_owners_org(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let a = app.new_org("alfa.test").await;
    let b = app.new_org("beta.test").await;
    let (_, r) = app.get(ROOM, Some(&a.token)).await;
    let code = r["code"].as_str().unwrap().to_string();

    let bind = |org: String, e164: &'static str, pin: &'static str, code: String| {
        let db = app.db.clone();
        let user = a.user_id.clone();
        async move {
            sqlx::query(
                "WITH d AS (INSERT INTO voice_did (org_id, e164) VALUES ($1::uuid, $2) RETURNING id)
                 INSERT INTO voice_room (org_id, room_code, pin, did_id, created_by)
                 SELECT $1::uuid, $3, $4, d.id, $5::uuid FROM d",
            )
            .bind(org)
            .bind(e164)
            .bind(code)
            .bind(pin)
            .bind(user)
            .execute(&db)
            .await
            .unwrap();
        }
    };
    // Uma sala de voz de OUTRA org com o mesmo código não aparece.
    bind(b.org().to_string(), "+244900000002", "222222", code.clone()).await;
    let (_, r) = app.get(ROOM, Some(&a.token)).await;
    assert!(r["dial_in"].is_null(), "dial-in de outra org: {r}");

    bind(a.org().to_string(), "+244900000001", "111111", code.clone()).await;
    let dids_before: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM voice_did")
        .fetch_one(&app.db)
        .await
        .unwrap();
    let (_, r) = app.get(ROOM, Some(&a.token)).await;
    assert_eq!(r["dial_in"]["number"], "+244900000001", "{r}");
    assert_eq!(r["dial_in"]["pin"], "111111");
    let dids_after: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM voice_did")
        .fetch_one(&app.db)
        .await
        .unwrap();
    assert_eq!(dids_before, dids_after, "a leitura não cria DIDs");

    // O dial-in fica no código antigo: rodado o código, deixa de aparecer.
    let (_, r) = app
        .post(&format!("{ROOM}/rotate-code"), Some(&a.token), json!({}))
        .await;
    assert!(r["dial_in"].is_null(), "{r}");
}
