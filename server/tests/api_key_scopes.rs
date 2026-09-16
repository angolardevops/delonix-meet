//! Chaves `dlx_` com escopos, expiração, limite por chave e revogação com
//! estado HTTP honesto (auditoria S6, ADR-0004 §4). Contra Postgres real.
mod common;

use chrono::{Duration, Utc};
use common::{TestApp, INVENTED_ID};
use serde_json::{json, Value};

/// O catálogo inteiro, pela ordem canónica — o que uma chave antiga recebe.
const ALL: [&str; 7] = [
    "org:read",
    "rooms:read",
    "rooms:write",
    "bots:join",
    "meetings:read",
    "meetings:write",
    "recordings:read",
];

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

async fn key_with(app: &TestApp, admin: &common::Account, body: Value) -> (u16, Value) {
    app.post(
        &format!("/api/orgs/{}/api-keys", admin.org()),
        Some(&admin.token),
        body,
    )
    .await
}

async fn scoped_key(app: &TestApp, admin: &common::Account, scopes: &[&str]) -> String {
    let (st, k) = key_with(app, admin, json!({"name": "escopada", "scopes": scopes})).await;
    assert_eq!(st, 200, "{k}");
    k["key"].as_str().unwrap().to_string()
}

fn meeting_body(host: &str) -> Value {
    json!({"title": "Comité", "host_email": host,
           "starts_at": (Utc::now() + Duration::hours(2)).to_rfc3339()})
}

/// Cenário com uma reunião (com sala) e uma sala soltas, criadas por uma chave
/// com o catálogo inteiro. Devolve `(código da sala, id da reunião)`.
async fn fixtures(app: &TestApp, full: &str, host: &str) -> (String, String) {
    let (st, room) = v1(app, reqwest::Method::POST, "/rooms", full, Some(json!({}))).await;
    assert_eq!(st, 200, "{room}");
    let (st, m) = v1(
        app,
        reqwest::Method::POST,
        "/meetings",
        full,
        Some(meeting_body(host)),
    )
    .await;
    assert_eq!(st, 200, "{m}");
    (
        room["code"].as_str().unwrap().to_string(),
        m["id"].as_str().unwrap().to_string(),
    )
}

/// As rotas v1 autenticadas por chave, cada uma com o escopo que exige. O
/// `DELETE` vai no fim: apaga a reunião das outras.
fn routes(
    code: &str,
    mid: &str,
    host: &str,
) -> Vec<(reqwest::Method, String, Option<Value>, &'static str)> {
    use reqwest::Method as M;
    vec![
        (M::GET, "/organization".into(), None, "org:read"),
        (M::POST, "/rooms".into(), Some(json!({})), "rooms:write"),
        (M::GET, format!("/rooms/{code}"), None, "rooms:read"),
        (
            M::POST,
            format!("/rooms/{code}/bots"),
            Some(json!({"bot_name": "b"})),
            "bots:join",
        ),
        (M::GET, "/recordings".into(), None, "recordings:read"),
        (M::GET, "/meetings".into(), None, "meetings:read"),
        (M::GET, format!("/meetings/{mid}"), None, "meetings:read"),
        (
            M::GET,
            format!("/meetings/{mid}/minutes"),
            None,
            "meetings:read",
        ),
        (
            M::POST,
            "/meetings".into(),
            Some(meeting_body(host)),
            "meetings:write",
        ),
        (
            M::PATCH,
            format!("/meetings/{mid}"),
            Some(json!({"title": "Outro"})),
            "meetings:write",
        ),
        (
            M::POST,
            format!("/meetings/{mid}/ring"),
            None,
            "meetings:write",
        ),
        (
            M::DELETE,
            format!("/meetings/{mid}"),
            None,
            "meetings:write",
        ),
    ]
}

// ---------------------------------------------------------------------------
//  Escopos
// ---------------------------------------------------------------------------

/// Nenhuma rota v1 serve sem o seu escopo — e cada uma serve COM ele. É o
/// teste que apanha o handler novo que se esqueceu do `key.require`.
#[sqlx::test(migrations = "./migrations")]
async fn cada_rota_v1_exige_o_seu_escopo(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let a = app.new_org("alfa.test").await;
    let (_, full) = app.api_key(&a).await;
    let (code, mid) = fixtures(&app, &full, &a.email).await;

    for (method, path, body, scope) in routes(&code, &mid, &a.email) {
        // Todos os escopos MENOS o da rota.
        let others: Vec<&str> = ALL.iter().copied().filter(|s| *s != scope).collect();
        let without = scoped_key(&app, &a, &others).await;
        let (st, err) = v1(&app, method.clone(), &path, &without, body.clone()).await;
        assert_eq!(st, 403, "{method} {path} sem {scope}: {err}");
        assert_eq!(err["code"], "api_key.scope_missing", "{method} {path}");
        assert_eq!(
            err["details"],
            json!([{"field": "scope", "description": scope}]),
            "{method} {path}"
        );

        // Só o escopo da rota.
        let only = scoped_key(&app, &a, &[scope]).await;
        let (st, ok) = v1(&app, method.clone(), &path, &only, body).await;
        assert!(
            (200..300).contains(&st),
            "{method} {path} com {scope}: {st} {ok}"
        );
    }
}

#[sqlx::test(migrations = "./migrations")]
async fn sem_meetings_write_o_post_e_403_e_com_ele_200(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let a = app.new_org("alfa.test").await;
    let leitura = scoped_key(&app, &a, &["meetings:read"]).await;
    let escrita = scoped_key(&app, &a, &["meetings:read", "meetings:write"]).await;

    let (st, err) = v1(
        &app,
        reqwest::Method::POST,
        "/meetings",
        &leitura,
        Some(meeting_body(&a.email)),
    )
    .await;
    assert_eq!(st, 403, "{err}");
    assert_eq!(err["code"], "api_key.scope_missing");
    assert_eq!(err["details"][0]["description"], "meetings:write");
    assert!(err["request_id"].is_string(), "{err}");
    // Nada foi criado.
    let (_, list) = v1(&app, reqwest::Method::GET, "/meetings", &leitura, None).await;
    assert_eq!(list, json!({"meetings": []}));

    let (st, m) = v1(
        &app,
        reqwest::Method::POST,
        "/meetings",
        &escrita,
        Some(meeting_body(&a.email)),
    )
    .await;
    assert_eq!(st, 200, "{m}");
}

#[sqlx::test(migrations = "./migrations")]
async fn criacao_valida_escopos_e_expiracao_e_a_lista_mostra_os(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let a = app.new_org("alfa.test").await;

    for (body, code) in [
        (json!({"scopes": []}), "api_key.scopes_empty"),
        (
            json!({"scopes": ["org:read", "*"]}),
            "api_key.unknown_scope",
        ),
        (
            json!({"expires_at": (Utc::now() - Duration::minutes(1)).to_rfc3339()}),
            "api_key.expiry_in_past",
        ),
        (
            json!({"expires_at": (Utc::now() + Duration::days(731)).to_rfc3339()}),
            "api_key.expiry_too_far",
        ),
    ] {
        let (st, err) = key_with(&app, &a, body.clone()).await;
        assert_eq!(st, 400, "{body}: {err}");
        assert_eq!(err["code"], code, "{body}");
    }
    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM org_api_keys")
        .fetch_one(&app.db)
        .await
        .unwrap();
    assert_eq!(n, 0, "uma recusa não deixa chave nenhuma");

    // Sem `scopes`: o catálogo inteiro (a decisão de compatibilidade).
    let (st, k) = key_with(&app, &a, json!({"name": "omissa"})).await;
    assert_eq!(st, 200, "{k}");
    assert_eq!(k["scopes"], json!(ALL));
    assert!(k["expires_at"].is_null());

    let expira = Utc::now() + Duration::days(30);
    let (st, k2) = key_with(
        &app,
        &a,
        json!({"name": "curta", "scopes": ["meetings:write", "org:read", "org:read"],
               "expires_at": expira.to_rfc3339()}),
    )
    .await;
    assert_eq!(st, 200, "{k2}");
    assert_eq!(k2["scopes"], json!(["org:read", "meetings:write"]));

    let (st, list) = app
        .get(&format!("/api/orgs/{}/api-keys", a.org()), Some(&a.token))
        .await;
    assert_eq!(st, 200);
    let curta = list
        .as_array()
        .unwrap()
        .iter()
        .find(|x| x["id"] == k2["id"])
        .unwrap();
    assert_eq!(curta["scopes"], json!(["org:read", "meetings:write"]));
    assert!(curta["expires_at"].is_string(), "{curta}");
    assert!(curta.get("last_used_at").is_some());
    // Nunca a chave nem o hash.
    let texto = list.to_string();
    assert!(!texto.contains(k2["key"].as_str().unwrap()));
    assert!(curta.get("key").is_none() && curta.get("key_hash").is_none());
}

// ---------------------------------------------------------------------------
//  Expiração e registo de uso
// ---------------------------------------------------------------------------

#[sqlx::test(migrations = "./migrations")]
async fn chave_expirada_e_401_api_key_expired(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let a = app.new_org("alfa.test").await;
    let (st, k) = key_with(
        &app,
        &a,
        json!({"expires_at": (Utc::now() + Duration::hours(1)).to_rfc3339()}),
    )
    .await;
    assert_eq!(st, 200, "{k}");
    let key = k["key"].as_str().unwrap();
    let (st, _) = v1(&app, reqwest::Method::GET, "/organization", key, None).await;
    assert_eq!(st, 200, "antes de expirar serve");

    sqlx::query("UPDATE org_api_keys SET expires_at = now() - interval '1 second' WHERE id = $1")
        .bind(uuid::Uuid::parse_str(k["id"].as_str().unwrap()).unwrap())
        .execute(&app.db)
        .await
        .unwrap();
    let (st, err) = v1(&app, reqwest::Method::GET, "/organization", key, None).await;
    assert_eq!(st, 401, "{err}");
    assert_eq!(err["code"], "api_key.expired");
    // Desconhecida e revogada continuam a ser o 401 de sempre.
    let (st, err) = v1(&app, reqwest::Method::GET, "/organization", "dlx_nao_existe", None).await;
    assert_eq!(
        (st, err["code"].as_str()),
        (401, Some("auth.unauthenticated"))
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn last_used_at_no_maximo_uma_escrita_por_minuto(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let a = app.new_org("alfa.test").await;
    let (id, key) = app.api_key(&a).await;
    let id = uuid::Uuid::parse_str(&id).unwrap();
    let last = || async {
        sqlx::query_scalar::<_, Option<chrono::DateTime<Utc>>>(
            "SELECT last_used_at FROM org_api_keys WHERE id = $1",
        )
        .bind(id)
        .fetch_one(&app.db)
        .await
        .unwrap()
    };
    assert!(last().await.is_none());
    v1(&app, reqwest::Method::GET, "/organization", &key, None).await;
    let t1 = last().await.expect("o primeiro uso fica registado");
    v1(&app, reqwest::Method::GET, "/organization", &key, None).await;
    assert_eq!(last().await, Some(t1), "dentro do minuto não se escreve");

    sqlx::query(
        "UPDATE org_api_keys SET last_used_at = now() - interval '2 minutes' WHERE id = $1",
    )
    .bind(id)
    .execute(&app.db)
    .await
    .unwrap();
    let antigo = last().await.unwrap();
    v1(&app, reqwest::Method::GET, "/organization", &key, None).await;
    assert!(
        last().await.unwrap() > antigo,
        "passado o minuto volta a registar"
    );
}

// ---------------------------------------------------------------------------
//  Compatibilidade: chaves anteriores à migração e chaves provisionadas
// ---------------------------------------------------------------------------

/// Uma chave gravada como antes da 0046 (sem `scopes` nem `expires_at`) e a
/// migração aplicada por cima: serve em TODAS as rotas v1 onde servia.
#[sqlx::test(migrations = "./migrations")]
async fn chave_anterior_a_migracao_continua_a_servir(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let a = app.new_org("alfa.test").await;

    // Volta a tabela à forma anterior à 0046…
    sqlx::raw_sql(
        "ALTER TABLE org_api_keys DROP CONSTRAINT org_api_keys_scopes_not_empty;
         ALTER TABLE org_api_keys DROP COLUMN scopes, DROP COLUMN expires_at;",
    )
    .execute(&app.db)
    .await
    .unwrap();
    // …grava a chave com o INSERT de antes…
    let legacy = "dlx_legado0000000000000000000000000000000000000000000000000000000000";
    sqlx::query(
        "INSERT INTO org_api_keys (org_id, name, prefix, key_hash, created_by)
         VALUES ($1, 'legado', $2, encode(sha256($3::bytea), 'hex'), $4)",
    )
    .bind(uuid::Uuid::parse_str(a.org()).unwrap())
    .bind(&legacy[..12])
    .bind(legacy)
    .bind(uuid::Uuid::parse_str(&a.user_id).unwrap())
    .execute(&app.db)
    .await
    .unwrap();
    // …e aplica a migração tal como está no repositório.
    sqlx::raw_sql(include_str!("../migrations/0046_api_key_scopes.sql"))
        .execute(&app.db)
        .await
        .unwrap();

    let scopes: Vec<String> =
        sqlx::query_scalar("SELECT scopes FROM org_api_keys WHERE name = 'legado'")
            .fetch_one(&app.db)
            .await
            .unwrap();
    assert_eq!(scopes, ALL, "a migração dá o catálogo inteiro");

    let (code, mid) = fixtures(&app, legacy, &a.email).await;
    for (method, path, body, _) in routes(&code, &mid, &a.email) {
        let (st, res) = v1(&app, method.clone(), &path, legacy, body).await;
        assert!((200..300).contains(&st), "{method} {path}: {st} {res}");
    }

    // E o DEFAULT não ficou: uma inserção que esqueça os escopos falha.
    let r = sqlx::query(
        "INSERT INTO org_api_keys (org_id, name, prefix, key_hash, created_by)
         VALUES ($1, 'sem', 'dlx_semesc00', 'h', $2)",
    )
    .bind(uuid::Uuid::parse_str(a.org()).unwrap())
    .bind(uuid::Uuid::parse_str(&a.user_id).unwrap())
    .execute(&app.db)
    .await;
    assert!(r.is_err(), "sem DEFAULT, sem escopos implícitos");
}

/// O caminho do módulo Odoo `nk_delonix_meet`: provisiona a org, recebe a
/// chave, e usa-a no sync de calendário (`GET /meetings?since=`, notas) e na
/// escrita (criar, alterar, tocar, cancelar).
#[sqlx::test(migrations = "./migrations")]
async fn chave_provisionada_serve_os_fluxos_do_odoo(db: sqlx::PgPool) {
    let secret = "segredo-de-plataforma-de-teste";
    let app = TestApp::spawn_with(db, &[("PROVISIONING_SECRET", secret)]).await;
    let provision = |body: Value| {
        let app = &app;
        async move {
            app.raw(
                reqwest::Method::POST,
                "/api/operator/v1/organizations",
                &[("X-Provisioning-Secret", secret)],
                Some(body),
            )
            .await
        }
    };
    let r = provision(json!({"name": "Gama SA", "email_domain": "gama.test",
                             "odoo_db": "prod", "odoo_company_id": 7, "setup_odoo": true,
                             "odoo_url": "http://odoo.test"}))
    .await;
    assert_eq!(r.status, 200, "{}", r.text);
    let key = r.json()["api_key"].as_str().unwrap().to_string();
    let scopes: Vec<String> =
        sqlx::query_scalar("SELECT scopes FROM org_api_keys ORDER BY created_at DESC LIMIT 1")
            .fetch_one(&app.db)
            .await
            .unwrap();
    assert_eq!(scopes, ALL);

    use reqwest::Method as M;
    let host = "anfitriao@gama.test";
    let (st, m) = v1(&app, M::POST, "/meetings", &key, Some(meeting_body(host))).await;
    assert_eq!(st, 200, "{m}");
    let mid = m["id"].as_str().unwrap();
    let since = (Utc::now() - Duration::hours(1)).to_rfc3339();
    let (st, list) = v1(
        &app,
        M::GET,
        &format!("/meetings?since={}", urlencode(&since)),
        &key,
        None,
    )
    .await;
    assert_eq!(st, 200, "{list}");
    assert_eq!(list["meetings"][0]["id"], mid);
    for (method, path, body) in [
        (M::GET, format!("/meetings/{mid}"), None),
        (M::GET, format!("/meetings/{mid}/minutes"), None),
        (
            M::PATCH,
            format!("/meetings/{mid}"),
            Some(json!({"title": "Outro"})),
        ),
        (M::POST, format!("/meetings/{mid}/ring"), None),
        (M::GET, "/organization".to_string(), None),
        (M::DELETE, format!("/meetings/{mid}"), None),
    ] {
        let (st, res) = v1(&app, method.clone(), &path, &key, body).await;
        assert_eq!(st, 200, "{method} {path}: {res}");
    }

    // Um provisionador que peça menos recebe menos.
    let r = provision(json!({"name": "Delta", "scopes": ["meetings:read"]})).await;
    assert_eq!(r.status, 200, "{}", r.text);
    let leitura = r.json()["api_key"].as_str().unwrap().to_string();
    let (st, err) = v1(
        &app,
        M::POST,
        "/meetings",
        &leitura,
        Some(meeting_body("x@delta.test")),
    )
    .await;
    assert_eq!(
        (st, err["code"].as_str()),
        (403, Some("api_key.scope_missing"))
    );
    // Escopo inválido: recusado ANTES de criar a organização.
    let r = provision(json!({"name": "Épsilon", "scopes": ["tudo"]})).await;
    assert_eq!(r.status, 400, "{}", r.text);
    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM organizations WHERE name = 'Épsilon'")
        .fetch_one(&app.db)
        .await
        .unwrap();
    assert_eq!(n, 0);
}

fn urlencode(s: &str) -> String {
    s.replace('+', "%2B").replace(':', "%3A")
}

// ---------------------------------------------------------------------------
//  Limite por chave
// ---------------------------------------------------------------------------

/// Duas chaves atrás do mesmo IP têm orçamentos separados, e o 429 diz quanto
/// falta da janela.
#[sqlx::test(migrations = "./migrations")]
async fn limite_por_chave_isola_duas_chaves_do_mesmo_ip(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let a = app.new_org("alfa.test").await;
    let (_, k1) = app.api_key(&a).await;
    let (_, k2) = app.api_key(&a).await;

    for i in 0..120 {
        let (st, _) = v1(&app, reqwest::Method::GET, "/organization", &k1, None).await;
        assert_eq!(st, 200, "pedido {i} da chave 1");
    }
    let auth = format!("Bearer {k1}");
    let r = app
        .raw(
            reqwest::Method::GET,
            "/api/v1/organization",
            &[("Authorization", &auth)],
            None,
        )
        .await;
    assert_eq!(r.status, 429);
    assert_eq!(r.json()["code"], "rate_limited");
    let retry: u64 = r.header("Retry-After").unwrap().parse().unwrap();
    assert!((1..=60).contains(&retry), "Retry-After = {retry}");

    // A chave 2, mesmo IP: intacta.
    let (st, _) = v1(&app, reqwest::Method::GET, "/organization", &k2, None).await;
    assert_eq!(st, 200, "a chave 2 não paga pela chave 1");
    // Sem chave válida conta o IP, que também está intacto.
    let (st, _) = v1(&app, reqwest::Method::GET, "/organization", "dlx_inventada", None).await;
    assert_eq!(st, 401);
}

// ---------------------------------------------------------------------------
//  Revogação
// ---------------------------------------------------------------------------

#[sqlx::test(migrations = "./migrations")]
async fn revogar_204_e_404_para_inexistente_ou_de_outra_org(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let a = app.new_org("alfa.test").await;
    let b = app.new_org("beta.test").await;
    let (ka_id, ka) = app.api_key(&a).await;
    let (kb_id, kb) = app.api_key(&b).await;
    let path = |acc: &common::Account, id: &str| format!("/api/orgs/{}/api-keys/{id}", acc.org());
    let delete = |p: String, token: String| {
        let app = &app;
        async move {
            app.raw(
                reqwest::Method::DELETE,
                &p,
                &[("Authorization", &format!("Bearer {token}"))],
                None,
            )
            .await
        }
    };

    // A tenta a chave da B pelo seu próprio caminho: 404, e a chave da B vive.
    let r = delete(path(&a, &kb_id), a.token.clone()).await;
    assert_eq!(r.status, 404, "{}", r.text);
    assert_eq!(r.json()["code"], "api_key.not_found");
    // …e pelo caminho da B: 404 (não é membro).
    let r = delete(path(&b, &kb_id), a.token.clone()).await;
    assert_eq!(r.status, 404, "{}", r.text);
    let (st, _) = v1(&app, reqwest::Method::GET, "/organization", &kb, None).await;
    assert_eq!(st, 200, "a chave da B continua a servir");

    // Inexistente: 404.
    let r = delete(path(&a, INVENTED_ID), a.token.clone()).await;
    assert_eq!(r.status, 404);

    // A revoga a sua: 204 sem corpo, e a chave deixa de servir.
    let r = delete(path(&a, &ka_id), a.token.clone()).await;
    assert_eq!(r.status, 204);
    assert!(r.text.is_empty(), "{}", r.text);
    let (st, _) = v1(&app, reqwest::Method::GET, "/organization", &ka, None).await;
    assert_eq!(st, 401);
    // Revogar outra vez: já não existe.
    let r = delete(path(&a, &ka_id), a.token.clone()).await;
    assert_eq!(r.status, 404);
}
