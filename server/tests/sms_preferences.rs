//! Política de envio da org, telefone por pertença, preferências da conta.
mod common;

use common::TestApp;
use serde_json::json;

#[sqlx::test(migrations = "./migrations")]
async fn sms_policy_only_admin_reads_and_writes(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let a = app.new_org("alfa.test").await;
    let carla = app.add_member(&a, "carla", "member").await;

    // Omisso ⇒ 'admins' por omissão.
    let (st, v) = app
        .get(&format!("/api/orgs/{}/sms/policy", a.org()), Some(&a.token))
        .await;
    assert_eq!(st, 200, "{v}");
    assert_eq!(v["send_policy"], "admins");

    // Membro sem papel de admin: 403.
    let (st, _) = app
        .get(
            &format!("/api/orgs/{}/sms/policy", a.org()),
            Some(&carla.token),
        )
        .await;
    assert_eq!(st, 403);
    let (st, _) = app
        .put(
            &format!("/api/orgs/{}/sms/policy", a.org()),
            Some(&carla.token),
            json!({"send_policy": "members"}),
        )
        .await;
    assert_eq!(st, 403);

    // Admin muda para 'members', e a leitura reflecte.
    let (st, v) = app
        .put(
            &format!("/api/orgs/{}/sms/policy", a.org()),
            Some(&a.token),
            json!({"send_policy": "members"}),
        )
        .await;
    assert_eq!(st, 200, "{v}");
    assert_eq!(v["send_policy"], "members");
    let (_, v) = app
        .get(&format!("/api/orgs/{}/sms/policy", a.org()), Some(&a.token))
        .await;
    assert_eq!(v["send_policy"], "members");
}

#[sqlx::test(migrations = "./migrations")]
async fn member_phone_self_or_admin_normalizes_and_clears(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let a = app.new_org("alfa.test").await;
    let carla = app.add_member(&a, "carla", "member").await;
    let path = format!("/api/orgs/{}/members/{}/phone", a.org(), carla.user_id);

    // Carla muda o SEU próprio telefone — várias formas, todas normalizam.
    let (st, v) = app
        .put(&path, Some(&carla.token), json!({"phone": "923 000 111"}))
        .await;
    assert_eq!(st, 200, "{v}");
    assert_eq!(v["phone"], "+244923000111");
    assert_eq!(v["phone_source"], "manual");

    // Um COLEGA sem ser admin não pode mudar o telefone de Carla.
    let duarte = app.add_member(&a, "duarte", "member").await;
    let (st, _) = app
        .put(&path, Some(&duarte.token), json!({"phone": "923000222"}))
        .await;
    assert_eq!(st, 403);

    // O admin (dono) muda o telefone de outro membro.
    let (st, v) = app
        .put(&path, Some(&a.token), json!({"phone": "+244 923-000-222"}))
        .await;
    assert_eq!(st, 200, "{v}");
    assert_eq!(v["phone"], "+244923000222");

    // Número inválido: 422, nada muda.
    let (st, v) = app
        .put(&path, Some(&carla.token), json!({"phone": "123"}))
        .await;
    assert_eq!(st, 422, "{v}");

    // `follow_directory: false` não pede nada — 400.
    let (st, _) = app
        .put(
            &path,
            Some(&carla.token),
            json!({"follow_directory": false}),
        )
        .await;
    assert_eq!(st, 400);

    // `phone: null` limpa a substituição manual.
    let (st, v) = app
        .put(&path, Some(&carla.token), json!({"phone": null}))
        .await;
    assert_eq!(st, 200, "{v}");
    assert_eq!(v["phone"], serde_json::Value::Null);
    assert_eq!(v["phone_source"], serde_json::Value::Null);

    // Membro que não existe na org: 404.
    let (st, _) = app
        .put(
            &format!(
                "/api/orgs/{}/members/{}/phone",
                a.org(),
                uuid::Uuid::new_v4()
            ),
            Some(&a.token),
            json!({"phone": "923000333"}),
        )
        .await;
    assert_eq!(st, 404);
}

#[sqlx::test(migrations = "./migrations")]
async fn sms_preferences_are_per_account_with_phones_per_active_org(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let a = app.new_org("alfa.test").await;
    let b = app.new_org("beta.test").await;

    // Por omissão: os dois opt-out a false, sem organizações ainda (a dona
    // só se junta à org que criou — new_org já a insere como admin).
    let (st, v) = app
        .get("/api/users/me/sms-preferences", Some(&a.token))
        .await;
    assert_eq!(st, 200, "{v}");
    assert_eq!(v["contact_opt_out"], false);
    assert_eq!(v["meeting_opt_out"], false);
    let phones = v["phones"].as_array().unwrap();
    assert_eq!(phones.len(), 1, "{v}");
    assert_eq!(phones[0]["org_id"], a.org());
    assert_eq!(phones[0]["phone"], serde_json::Value::Null);

    // PUT parcial: só contact_opt_out muda, meeting_opt_out mantém-se.
    let (st, v) = app
        .put(
            "/api/users/me/sms-preferences",
            Some(&a.token),
            json!({"contact_opt_out": true}),
        )
        .await;
    assert_eq!(st, 200, "{v}");
    assert_eq!(v["contact_opt_out"], true);
    assert_eq!(v["meeting_opt_out"], false);
    let (_, v) = app
        .get("/api/users/me/sms-preferences", Some(&a.token))
        .await;
    assert_eq!(v["contact_opt_out"], true, "persistiu");

    // B não vê as preferências de A nem os telefones de A.
    let (_, v) = app
        .get("/api/users/me/sms-preferences", Some(&b.token))
        .await;
    assert_eq!(
        v["contact_opt_out"], false,
        "conta diferente, preferência própria"
    );
    assert!(
        v["phones"]
            .as_array()
            .unwrap()
            .iter()
            .all(|p| p["org_id"] != a.org()),
        "{v}"
    );

    // Um membro ARQUIVADO deixa de aparecer nos telefones (S3).
    let carla = app.add_member(&a, "carla", "member").await;
    let (_, v) = app
        .get("/api/users/me/sms-preferences", Some(&carla.token))
        .await;
    assert_eq!(v["phones"].as_array().unwrap().len(), 1);
    app.archive_member(a.org(), &carla.user_id).await;
    let (_, v) = app
        .get("/api/users/me/sms-preferences", Some(&carla.token))
        .await;
    assert_eq!(v["phones"].as_array().unwrap().len(), 0, "{v}");
}
