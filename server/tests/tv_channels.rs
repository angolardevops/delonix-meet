//! Canais de TV (RFC-0001, Fase 1) contra Postgres real: contrato (201 +
//! Location, 204, paginação), concorrência optimista, validação, isolamento
//! entre organizações, a capacidade `broadcast.manage_channels` e a auditoria.
mod common;

use common::TestApp;
use serde_json::json;

fn path(org: &str) -> String {
    format!("/api/orgs/{org}/tv/channels")
}

#[sqlx::test(migrations = "./migrations")]
async fn crud_contract_with_defaults_and_audit(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let a = app.new_org("alfa.ao").await;

    let res = app
        .http
        .post(app.url(&path(a.org())))
        .bearer_auth(&a.token)
        .json(&json!({"slug": "tv-alfa", "name": "  TV Alfa "}))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 201);
    let location = res.headers()["location"].to_str().unwrap().to_string();
    let c: serde_json::Value = res.json().await.unwrap();
    let id = c["id"].as_str().unwrap();
    assert_eq!(location, format!("{}/{id}", path(a.org())));
    assert_eq!(c["name"], "TV Alfa", "o nome vem aparado");
    // Omissões: privado, fuso de Luanda, em rascunho, versão 1.
    assert_eq!(c["visibility"], "private");
    assert_eq!(c["timezone"], "Africa/Luanda");
    assert_eq!(
        c["status"], "draft",
        "sem motor, um canal não está operacional"
    );
    assert_eq!(c["version"], 1);
    assert!(c["recording_retention_days"].is_null());

    let (st, got) = app.get(&location, Some(&a.token)).await;
    assert_eq!(st, 200, "{got}");
    assert_eq!(got["slug"], "tv-alfa");

    // A trilha imutável regista a criação, sem segredos.
    let n: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM audit_logs WHERE action = 'tv.channel.created'")
            .fetch_one(&app.db)
            .await
            .unwrap();
    assert_eq!(n, 1);

    let (st, _) = app.delete(&location, Some(&a.token)).await;
    assert_eq!(st, 204);
    let (st, _) = app.get(&location, Some(&a.token)).await;
    assert_eq!(st, 404);
    let (st, _) = app.delete(&location, Some(&a.token)).await;
    assert_eq!(st, 404, "apagar duas vezes não finge sucesso");
}

#[sqlx::test(migrations = "./migrations")]
async fn stale_version_is_a_conflict_and_changes_nothing(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let a = app.new_org("alfa.ao").await;
    let (_, c) = app
        .post(
            &path(a.org()),
            Some(&a.token),
            json!({"slug": "tv-alfa", "name": "Um"}),
        )
        .await;
    let url = format!("{}/{}", path(a.org()), c["id"].as_str().unwrap());

    // Dois operadores leram a versão 1; o primeiro grava.
    let (st, v2) = app
        .patch(&url, Some(&a.token), json!({"version": 1, "name": "Dois"}))
        .await;
    assert_eq!(st, 200, "{v2}");
    assert_eq!(v2["version"], 2);

    // O segundo, ainda com a versão 1, é recusado — e não pisa o primeiro.
    let (st, v) = app
        .patch(&url, Some(&a.token), json!({"version": 1, "name": "Três"}))
        .await;
    assert_eq!(st, 409, "{v}");
    assert_eq!(v["code"], "tv.channel.version_conflict");
    let (_, now) = app.get(&url, Some(&a.token)).await;
    assert_eq!(now["name"], "Dois");
    assert_eq!(now["version"], 2);

    // Sem versão não há PATCH.
    let (st, _) = app.patch(&url, Some(&a.token), json!({"name": "x"})).await;
    assert!(st == 400 || st == 422, "{st}");

    // Retenção: um número define, `null` limpa, ausente deixa.
    let (_, r) = app
        .patch(
            &url,
            Some(&a.token),
            json!({"version": 2, "recording_retention_days": 30}),
        )
        .await;
    assert_eq!(r["recording_retention_days"], 30);
    let (_, r) = app
        .patch(&url, Some(&a.token), json!({"version": 3, "name": "Q"}))
        .await;
    assert_eq!(r["recording_retention_days"], 30, "ausente deixa como está");
    let (_, r) = app
        .patch(
            &url,
            Some(&a.token),
            json!({"version": 4, "recording_retention_days": null}),
        )
        .await;
    assert!(r["recording_retention_days"].is_null(), "null limpa");
}

#[sqlx::test(migrations = "./migrations")]
async fn validation_and_unique_slug(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let a = app.new_org("alfa.ao").await;
    let b = app.new_org("beta.ao").await;
    let ok = json!({"slug": "tv-um", "name": "Um"});
    assert_eq!(
        app.post(&path(a.org()), Some(&a.token), ok.clone()).await.0,
        201
    );

    // O endereço é único NA organização; outra organização pode usar o mesmo.
    let (st, v) = app.post(&path(a.org()), Some(&a.token), ok.clone()).await;
    assert_eq!(st, 409, "{v}");
    assert_eq!(v["code"], "tv.channel.slug_taken");
    assert_eq!(app.post(&path(b.org()), Some(&b.token), ok).await.0, 201);

    for (body, code) in [
        (
            json!({"slug": "TV Um", "name": "x"}),
            "tv.channel.invalid_slug",
        ),
        (
            json!({"slug": "tv-dois", "name": " "}),
            "tv.channel.invalid_name",
        ),
        (
            json!({"slug": "tv-dois", "name": "x", "visibility": "publico"}),
            "tv.channel.invalid_visibility",
        ),
        (
            json!({"slug": "tv-dois", "name": "x", "timezone": "Foo/Bar"}),
            "tv.channel.unknown_timezone",
        ),
        (
            json!({"slug": "tv-dois", "name": "x", "timezone": "../etc"}),
            "tv.channel.invalid_timezone",
        ),
        (
            json!({"slug": "tv-dois", "name": "x", "recording_retention_days": 0}),
            "tv.channel.invalid_retention",
        ),
    ] {
        let (st, v) = app.post(&path(a.org()), Some(&a.token), body.clone()).await;
        assert_eq!(st, 400, "{body}: {v}");
        assert_eq!(v["code"], code, "{body}");
    }
    // Nada do que foi recusado ficou gravado.
    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM tv_channels WHERE org_id = $1::uuid")
        .bind(a.org())
        .fetch_one(&app.db)
        .await
        .unwrap();
    assert_eq!(n, 1);
}

#[sqlx::test(migrations = "./migrations")]
async fn pagination_is_bounded_and_complete(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let a = app.new_org("alfa.ao").await;
    for i in 0..5 {
        let (st, _) = app
            .post(
                &path(a.org()),
                Some(&a.token),
                json!({"slug": format!("canal-{i}"), "name": format!("C{i}")}),
            )
            .await;
        assert_eq!(st, 201);
    }
    let mut seen = Vec::new();
    let mut token: Option<String> = None;
    loop {
        let q = match &token {
            Some(t) => format!("{}?page_size=2&page_token={t}", path(a.org())),
            None => format!("{}?page_size=2", path(a.org())),
        };
        let (st, p) = app.get(&q, Some(&a.token)).await;
        assert_eq!(st, 200, "{p}");
        let items = p["items"].as_array().unwrap();
        assert!(items.len() <= 2);
        seen.extend(
            items
                .iter()
                .map(|i| i["slug"].as_str().unwrap().to_string()),
        );
        match p["next_page_token"].as_str() {
            Some(t) => token = Some(t.to_string()),
            None => break,
        }
    }
    assert_eq!(
        seen,
        ["canal-0", "canal-1", "canal-2", "canal-3", "canal-4"]
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn other_org_and_plain_member_are_refused(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let a = app.new_org("alfa.ao").await;
    let b = app.new_org("beta.ao").await;
    let (_, created) = app
        .post(
            &path(b.org()),
            Some(&b.token),
            json!({"slug": "tv-da-b", "name": "Da B"}),
        )
        .await;
    let loc = format!("{}/{}", path(b.org()), created["id"].as_str().unwrap());

    // A não alcança nada da B: nem lista, nem lê, nem altera, nem apaga, nem cria.
    let refused = |st: u16| !(200..300).contains(&st);
    assert!(refused(app.get(&path(b.org()), Some(&a.token)).await.0));
    assert!(refused(app.get(&loc, Some(&a.token)).await.0));
    assert!(refused(
        app.patch(&loc, Some(&a.token), json!({"version": 1, "name": "x"}))
            .await
            .0
    ));
    assert!(refused(app.delete(&loc, Some(&a.token)).await.0));
    assert!(refused(
        app.post(
            &path(b.org()),
            Some(&a.token),
            json!({"slug": "intruso", "name": "x"})
        )
        .await
        .0
    ));
    // Pelo caminho da PRÓPRIA org A, com o id da B: o `org_id` do WHERE esconde-o.
    let via_a = format!("{}/{}", path(a.org()), created["id"].as_str().unwrap());
    assert_eq!(app.get(&via_a, Some(&a.token)).await.0, 404);
    assert_eq!(app.delete(&via_a, Some(&a.token)).await.0, 404);
    // O canal da B continua intacto.
    let (st, still) = app.get(&loc, Some(&b.token)).await;
    assert_eq!(st, 200);
    assert_eq!(still["name"], "Da B");
    assert_eq!(still["version"], 1);

    // Um membro simples da PRÓPRIA org não tem `broadcast.manage_channels`.
    let member = app.add_member(&b, "ana", "member").await;
    for st in [
        app.get(&path(b.org()), Some(&member.token)).await.0,
        app.get(&loc, Some(&member.token)).await.0,
        app.post(
            &path(b.org()),
            Some(&member.token),
            json!({"slug": "ana-tv", "name": "x"}),
        )
        .await
        .0,
        app.delete(&loc, Some(&member.token)).await.0,
    ] {
        assert!(refused(st), "um membro simples não gere canais: {st}");
    }
    assert_eq!(
        app.get(&loc, Some(&b.token)).await.0,
        200,
        "e o canal sobrevive"
    );
}
