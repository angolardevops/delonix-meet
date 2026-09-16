//! Armazenamento usado e quota (G3) contra Postgres real: somas certas de
//! gravações e quadros, papéis (membro 403, outra org 404), e a quota imposta
//! no carregamento de uma gravação nova — 422 e nada escrito.
mod common;

use common::{Account, TestApp};
use serde_json::Value;

fn usage_path(a: &Account) -> String {
    format!("/api/orgs/{}/storage-usage", a.org())
}

async fn recording(app: &TestApp, room_id: &str, uploader: &str, size: i64) {
    sqlx::query(
        "INSERT INTO recordings (room_id, uploader_id, filename, size_bytes)
         VALUES ($1::uuid, $2::uuid, 'x.webm', $3)",
    )
    .bind(room_id)
    .bind(uploader)
    .bind(size)
    .execute(&app.db)
    .await
    .unwrap();
}

async fn whiteboard(app: &TestApp, org: &str, owner: &str, bytes: usize) {
    sqlx::query("INSERT INTO whiteboards (org_id, owner_id, png) VALUES ($1::uuid, $2::uuid, $3)")
        .bind(org)
        .bind(owner)
        .bind(vec![7u8; bytes])
        .execute(&app.db)
        .await
        .unwrap();
}

async fn set_quota(app: &TestApp, org: &str, max: Option<i64>) {
    sqlx::query("UPDATE organizations SET max_storage_bytes = $2 WHERE id = $1::uuid")
        .bind(org)
        .bind(max)
        .execute(&app.db)
        .await
        .unwrap();
}

#[sqlx::test(migrations = "./migrations")]
async fn sums_recordings_and_whiteboards_of_the_org_only(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let a = app.new_org("alfa.test").await;
    let b = app.new_org("beta.test").await;
    let carla = app.add_member(&a, "carla", "member").await;
    let saiu = app.add_member(&a, "saiu", "member").await;
    let room = app.new_room(&a, "sala").await["id"]
        .as_str()
        .unwrap()
        .to_string();
    let room_b = app.new_room(&b, "sala-b").await["id"]
        .as_str()
        .unwrap()
        .to_string();

    recording(&app, &room, &a.user_id, 1_000).await;
    recording(&app, &room, &carla.user_id, 500).await;
    // De quem saiu: continua da empresa (S3), continua a ocupar.
    recording(&app, &room, &saiu.user_id, 250).await;
    app.archive_member(a.org(), &saiu.user_id).await;
    recording(&app, &room_b, &b.user_id, 77_777).await;
    whiteboard(&app, a.org(), &a.user_id, 40).await;
    whiteboard(&app, a.org(), &carla.user_id, 60).await;
    whiteboard(&app, b.org(), &b.user_id, 9_999).await;

    let (st, u) = app.get(&usage_path(&a), Some(&a.token)).await;
    assert_eq!(st, 200, "{u}");
    assert_eq!(u["org_id"], a.org());
    assert_eq!(u["recordings"]["count"], 3, "{u}");
    assert_eq!(u["recordings"]["bytes"], 1_750);
    assert_eq!(u["whiteboards"]["count"], 2);
    assert_eq!(u["whiteboards"]["bytes"], 100);
    assert_eq!(u["used_bytes"], 1_850);
    assert!(u["max_storage_bytes"].is_null(), "NULL = ilimitado: {u}");
    assert!(u["remaining_bytes"].is_null());

    // O painel da org e a quota contam da mesma forma.
    let (st, stats) = app
        .get(&format!("/api/orgs/{}/stats", a.org()), Some(&a.token))
        .await;
    assert_eq!(st, 200, "{stats}");
    assert_eq!(stats["recordings_bytes"], 1_750, "{stats}");

    set_quota(&app, a.org(), Some(2_000)).await;
    let (_, u) = app.get(&usage_path(&a), Some(&a.token)).await;
    assert_eq!(u["max_storage_bytes"], 2_000);
    assert_eq!(u["remaining_bytes"], 150);
    set_quota(&app, a.org(), Some(1_000)).await;
    let (_, u) = app.get(&usage_path(&a), Some(&a.token)).await;
    assert_eq!(
        u["remaining_bytes"], 0,
        "acima do tecto resta zero, nunca negativo"
    );

    // Pessoal: só o que ESTA pessoa carregou e é dona.
    let (st, mine) = app
        .get("/api/users/me/storage-usage", Some(&carla.token))
        .await;
    assert_eq!(st, 200, "{mine}");
    assert_eq!(mine["user_id"], carla.user_id.as_str());
    assert_eq!(mine["recordings"]["count"], 1);
    assert_eq!(mine["recordings"]["bytes"], 500);
    assert_eq!(mine["whiteboards"]["bytes"], 60);
    assert_eq!(mine["used_bytes"], 560);
    let (st, _) = app.get("/api/users/me/storage-usage", None).await;
    assert_eq!(st, 401);
}

#[sqlx::test(migrations = "./migrations")]
async fn only_admins_of_the_org_see_it(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let a = app.new_org("alfa.test").await;
    let b = app.new_org("beta.test").await;
    let carla = app.add_member(&a, "carla", "member").await;

    let (st, v) = app.get(&usage_path(&a), Some(&carla.token)).await;
    assert_eq!(st, 403, "membro sem papel: {v}");
    let (st, v) = app.get(&usage_path(&a), Some(&b.token)).await;
    assert_eq!(st, 404, "admin de outra org não confirma que existe: {v}");
    assert!(!v.to_string().contains(a.org()));
    let (st, _) = app.get(&usage_path(&a), None).await;
    assert_eq!(st, 401);
}

async fn joined_room(app: &TestApp, who: &Account) -> String {
    let code = app.new_room(who, "gravar").await["code"]
        .as_str()
        .unwrap()
        .to_string();
    let (st, j) = app
        .post(
            &format!("/api/rooms/{code}/join"),
            Some(&who.token),
            serde_json::json!({}),
        )
        .await;
    assert_eq!(st, 200, "{j}");
    code
}

async fn upload(app: &TestApp, who: &Account, code: &str, bytes: usize) -> (u16, Value) {
    let res = app
        .http
        .post(app.url(&format!("/api/rooms/{code}/recordings?name=q.webm")))
        .bearer_auth(&who.token)
        .body(vec![0x1a; bytes])
        .send()
        .await
        .unwrap();
    let st = res.status().as_u16();
    (st, res.json().await.unwrap_or(Value::Null))
}

async fn recordings_of(app: &TestApp, who: &Account) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM recordings WHERE uploader_id = $1::uuid")
        .bind(&who.user_id)
        .fetch_one(&app.db)
        .await
        .unwrap()
}

fn files_in(app: &TestApp) -> usize {
    std::fs::read_dir(&app.state.config.recordings_dir)
        .map(|d| d.count())
        .unwrap_or(0)
}

#[sqlx::test(migrations = "./migrations")]
async fn quota_refuses_new_uploads_and_writes_nothing(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let a = app.new_org("alfa.test").await;
    let code = joined_room(&app, &a).await;
    whiteboard(&app, a.org(), &a.user_id, 30).await;
    set_quota(&app, a.org(), Some(100)).await;

    // 30 (quadro) + 70 = 100: cabe à justa.
    let (st, v) = upload(&app, &a, &code, 70).await;
    assert_eq!(st, 200, "{v}");
    assert_eq!(recordings_of(&app, &a).await, 1);
    assert_eq!(files_in(&app), 1);

    let (st, v) = upload(&app, &a, &code, 1).await;
    assert_eq!(st, 422, "{v}");
    assert_eq!(v["code"], "storage.quota_exceeded");
    assert!(v["error"].is_string(), "envelope: {v}");
    assert_eq!(recordings_of(&app, &a).await, 1, "nenhuma linha nova");
    assert_eq!(files_in(&app), 1, "nenhum ficheiro novo");

    // NULL = ilimitado.
    set_quota(&app, a.org(), None).await;
    let (st, v) = upload(&app, &a, &code, 5_000).await;
    assert_eq!(st, 200, "{v}");
    assert_eq!(recordings_of(&app, &a).await, 2);
    let _ = std::fs::remove_dir_all(&app.state.config.recordings_dir);
}
