//! Caracterização do contexto de ORGANIZAÇÃO: orgs, filiais, grupos, salas
//! presenciais, colaboradores, estatísticas, auditoria, definições, SSO,
//! webhooks, chaves de API, voz e integração Odoo — do lado do administrador,
//! do membro sem papel, de outra organização e de um membro arquivado.
//!
//! Portado de `web/e2e/isolamento.mjs` (secções org, S3) e
//! `web/e2e/captura-empregado.mjs` (S7/R122).
mod common;

use common::{assert_denied, TestApp, INVENTED_ID, PASSWORD};
use serde_json::json;

fn org_path(org: &str, rest: &str) -> String {
    format!("/api/orgs/{org}/{rest}")
}

// ---------------------------------------------------------------------------
//  Leitura básica
// ---------------------------------------------------------------------------

#[sqlx::test(migrations = "./migrations")]
async fn my_orgs_shape(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let a = app.new_org("alfa.test").await;
    let c = app.add_member(&a, "carla", "member").await;
    let (st, orgs) = app.get("/api/orgs", Some(&a.token)).await;
    assert_eq!(st, 200);
    let o = &orgs[0];
    // `GET /api/orgs/{org_id}` devolve o MESMO item da lista, e um membro sem
    // papel também o lê (com o papel dele).
    let (st, one) = app.get(&format!("/api/orgs/{}", a.org()), Some(&a.token)).await;
    assert_eq!(st, 200, "{one}");
    assert_eq!(&one, o);
    let (st, one) = app.get(&format!("/api/orgs/{}", a.org()), Some(&c.token)).await;
    assert_eq!(st, 200, "{one}");
    assert_eq!(one["id"], a.org());
    assert_eq!(one["role"], "member");
    let (st, _) = app.get(&format!("/api/orgs/{INVENTED_ID}"), Some(&a.token)).await;
    assert_eq!(st, 404);
    let (st, _) = app.get(&format!("/api/orgs/{}", a.org()), None).await;
    assert_eq!(st, 401);
    assert_eq!(o["id"], a.org());
    assert_eq!(o["role"], "admin");
    assert_eq!(o["member_count"], 2);
    assert_eq!(o["domain"], "");
    assert_eq!(o["retention_days"], 0);
    assert!(o["max_groups"].is_null());
    assert!(o["max_rooms"].is_null());
    assert!(o["max_meetings"].is_null());
    let (st, _) = app.get("/api/orgs", None).await;
    assert_eq!(st, 401);
}

// ---------------------------------------------------------------------------
//  CRUD como administrador
// ---------------------------------------------------------------------------

#[sqlx::test(migrations = "./migrations")]
async fn admin_branches_groups_meeting_rooms(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let a = app.new_org("alfa.test").await;
    let org = a.org().to_string();
    let t = Some(a.token.as_str());

    // Filiais
    let (st, br) = app
        .post(
            &org_path(&org, "branches"),
            t,
            json!({"name": "  Luanda  ", "location": " Talatona "}),
        )
        .await;
    assert_eq!(st, 200, "{br}");
    assert_eq!(br["name"], "Luanda");
    assert_eq!(br["location"], "Talatona");
    assert_eq!(br["org_id"], org.as_str());
    for bad in [json!({"name": "  "}), json!({"name": "x".repeat(121)})] {
        let (st, _) = app.post(&org_path(&org, "branches"), t, bad).await;
        assert_eq!(st, 400);
    }
    let (st, list) = app.get(&org_path(&org, "branches"), t).await;
    assert_eq!(st, 200);
    assert_eq!(list.as_array().unwrap().len(), 1);

    // Grupos (o criador entra sempre; ids de fora da org são ignorados)
    let carla = app.add_member(&a, "carla", "member").await;
    let (st, g) = app
        .post(
            &org_path(&org, "groups"),
            t,
            json!({"name": "Direcção", "member_ids": [carla.user_id, INVENTED_ID]}),
        )
        .await;
    assert_eq!(st, 200, "{g}");
    assert_eq!(g["name"], "Direcção");
    assert_eq!(g["member_count"], 2);
    let (st, _) = app
        .post(&org_path(&org, "groups"), t, json!({"name": ""}))
        .await;
    assert_eq!(st, 400);
    let (st, list) = app.get(&org_path(&org, "groups"), t).await;
    assert_eq!(st, 200);
    assert_eq!(list[0]["id"], g["id"]);

    // Salas presenciais (capacidade negativa vira 0)
    let (st, mr) = app
        .post(
            &org_path(&org, "meeting-rooms"),
            t,
            json!({"name": "Sala A", "location": "Piso 2", "capacity": -3}),
        )
        .await;
    assert_eq!(st, 200, "{mr}");
    assert_eq!(mr["capacity"], 0);
    let (st, list) = app.get(&org_path(&org, "meeting-rooms"), t).await;
    assert_eq!(st, 200);
    assert_eq!(list.as_array().unwrap().len(), 1);
}

#[sqlx::test(migrations = "./migrations")]
async fn admin_employees_add_list_patch_archive(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let a = app.new_org("alfa.test").await;
    let org = a.org().to_string();
    let t = Some(a.token.as_str());

    let (_, br) = app
        .post(&org_path(&org, "branches"), t, json!({"name": "Sede"}))
        .await;

    // Email de outro domínio: 400.
    let (st, body) = app
        .post(
            &org_path(&org, "members"),
            t,
            json!({"email": "x@outro.test", "password": PASSWORD}),
        )
        .await;
    assert_eq!(st, 400, "{body}");
    assert!(body["error"].as_str().unwrap().contains("@alfa.test"));
    // Papel inválido: 400.
    let (st, _) = app
        .post(
            &org_path(&org, "members"),
            t,
            json!({"email": "y@alfa.test", "role": "root", "password": PASSWORD}),
        )
        .await;
    assert_eq!(st, 400);
    // Password curta numa conta nova: 400.
    let (st, _) = app
        .post(
            &org_path(&org, "members"),
            t,
            json!({"email": "z@alfa.test", "password": "curta"}),
        )
        .await;
    assert_eq!(st, 400);

    let (st, emp) = app
        .post(
            &org_path(&org, "members"),
            t,
            json!({"email": " Dario@ALFA.test ", "username": "dario", "password": PASSWORD,
                   "title": " Engenheiro ", "branch_id": br["id"]}),
        )
        .await;
    assert_eq!(st, 200, "{emp}");
    assert_eq!(emp["email"], "dario@alfa.test");
    assert_eq!(emp["username"], "dario");
    assert_eq!(emp["role"], "member");
    assert_eq!(emp["title"], "Engenheiro");
    assert_eq!(emp["branch_name"], "Sede");
    let dario = emp["user_id"].as_str().unwrap().to_string();

    // Re-adicionar alguém que já é desta org actualiza (não duplica).
    let (st, emp) = app
        .post(
            &org_path(&org, "members"),
            t,
            json!({"email": "dario@alfa.test", "role": "admin", "title": "Chefe"}),
        )
        .await;
    assert_eq!(st, 200, "{emp}");
    assert_eq!(emp["role"], "admin");
    assert!(
        emp["branch_id"].is_null(),
        "re-adicionar sem branch limpa-a"
    );

    let (st, list) = app.get(&org_path(&org, "members"), t).await;
    assert_eq!(st, 200);
    let list = list.as_array().unwrap();
    assert_eq!(list.len(), 2);
    // A listagem traz `last_active` (o admin tem eventos de auditoria).
    let me = list
        .iter()
        .find(|e| e["user_id"] == a.user_id.as_str())
        .unwrap();
    assert!(me["last_active"].is_string(), "{me}");

    // PATCH
    let (st, emp) = app
        .patch(
            &org_path(&org, &format!("members/{dario}")),
            t,
            json!({"role": "member", "title": "  Analista "}),
        )
        .await;
    assert_eq!(st, 200, "{emp}");
    assert_eq!(emp["role"], "member");
    assert_eq!(emp["title"], "Analista");
    let (st, _) = app
        .patch(
            &org_path(&org, &format!("members/{dario}")),
            t,
            json!({"role": "dono"}),
        )
        .await;
    assert_eq!(st, 400);
    // PATCH de alguém que não é membro: 404.
    let (st, _) = app
        .patch(
            &org_path(&org, &format!("members/{INVENTED_ID}")),
            t,
            json!({"title": "x"}),
        )
        .await;
    assert_eq!(st, 404);

    // Arquivar-se a si próprio: 400.
    let (st, _) = app
        .delete(&org_path(&org, &format!("members/{}", a.user_id)), t)
        .await;
    assert_eq!(st, 400);
    // Arquivar: soft delete, sai da listagem.
    let (st, body) = app
        .delete(&org_path(&org, &format!("members/{dario}")), t)
        .await;
    assert_eq!(st, 200);
    assert_eq!(body, json!({"ok": true}));
    let (_, list) = app.get(&org_path(&org, "members"), t).await;
    assert_eq!(list.as_array().unwrap().len(), 1);
    let archived: bool = sqlx::query_scalar(
        "SELECT archived_at IS NOT NULL FROM org_members WHERE user_id = $1::uuid",
    )
    .bind(&dario)
    .fetch_one(&app.db)
    .await
    .unwrap();
    assert!(archived);
    // Arquivar alguém que não existe: 200 na mesma (UPDATE sem linhas).
    let (st, _) = app
        .delete(&org_path(&org, &format!("members/{INVENTED_ID}")), t)
        .await;
    assert_eq!(st, 200);
}

#[sqlx::test(migrations = "./migrations")]
async fn admin_stats_audit_settings_and_quota(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let a = app.new_org("alfa.test").await;
    let org = a.org().to_string();
    let t = Some(a.token.as_str());

    let (st, s) = app.get(&org_path(&org, "stats"), t).await;
    assert_eq!(st, 200, "{s}");
    assert_eq!(s["members_total"], 1);
    assert_eq!(s["meetings_30d"], 0);
    assert_eq!(s["recordings_total"], 0);
    assert_eq!(s["avg_loss_pct"], 0.0);
    assert!(s["avg_rtt_ms"].is_null());
    assert!(s["avg_score"].is_null());
    assert!(s["top_organizers"].as_array().unwrap().is_empty());
    for k in [
        "meeting_minutes_30d",
        "active_users_30d",
        "recordings_bytes",
        "video_30d",
        "voice_30d",
        "avg_duration_min",
        "meetings_per_week",
        "quality_samples_30d",
        "pct_good",
        "pct_poor",
        "pct_low_score",
        "pct_turn_relay",
        "pct_cpu_limited",
        "meetings_prev_30d",
        "meeting_minutes_prev_30d",
        "active_users_prev_30d",
    ] {
        assert!(s.get(k).is_some(), "falta {k} em {s}");
    }

    // Auditoria: o registo e o login ficaram na trilha, e a cadeia fecha.
    let (st, audit) = app.get(&org_path(&org, "audit-events?limit=50"), t).await;
    assert_eq!(st, 200);
    let actions: Vec<&str> = audit
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["action"].as_str().unwrap())
        .collect();
    assert!(actions.contains(&"org.created"), "{actions:?}");
    assert!(actions.contains(&"auth.login"), "{actions:?}");
    assert_eq!(audit[0]["actor"], "admin-alfa.test");
    let (st, v) = app.get(&org_path(&org, "audit-events/verification"), t).await;
    assert_eq!(st, 200);
    assert_eq!(v["intact"], true, "{v}");
    assert!(v["broken_at_seq"].is_null());
    assert!(v["entries"].as_i64().unwrap() >= 2);

    // Definições: normaliza o domínio.
    let (st, body) = app
        .patch(
            &format!("/api/orgs/{org}"),
            t,
            json!({"domain": "HTTPS://Meet.Alfa.test/", "retention_days": 99999, "max_rooms": 1}),
        )
        .await;
    assert_eq!(st, 200, "{body}");
    // DÍVIDA (current behavior): `org::update_settings` corta `https://` ANTES
    // de passar a minúsculas, por isso um esquema em maiúsculas sobrevive e o
    // domínio guardado fica `https://meet.alfa.test` — que depois dá links
    // `https://https://...` em `apikeys::room_link`.
    assert_eq!(
        body,
        json!({"ok": true, "domain": "https://meet.alfa.test", "retention_days": 3650})
    );
    let (st, body) = app
        .patch(
            &format!("/api/orgs/{org}"),
            t,
            json!({"domain": "https://Meet.Alfa.test/", "retention_days": 99999, "max_rooms": 1}),
        )
        .await;
    assert_eq!(st, 200, "{body}");
    assert_eq!(
        body,
        json!({"ok": true, "domain": "meet.alfa.test", "retention_days": 3650})
    );
    let (st, _) = app
        .patch(
            &format!("/api/orgs/{org}"),
            t,
            json!({"domain": "com espaço.test"}),
        )
        .await;
    assert_eq!(st, 400);
    // A de cima (400) não alterou nada; a quota max_rooms=1 está em vigor.
    let (_, orgs) = app.get("/api/orgs", t).await;
    assert_eq!(orgs[0]["domain"], "meet.alfa.test");
    assert_eq!(orgs[0]["max_rooms"], 1);
    let (st, _) = app
        .post(&org_path(&org, "meeting-rooms"), t, json!({"name": "S1"}))
        .await;
    assert_eq!(st, 200);
    let (st, body) = app
        .post(&org_path(&org, "meeting-rooms"), t, json!({"name": "S2"}))
        .await;
    assert_eq!(st, 409, "{body}");
    assert!(body["error"].as_str().unwrap().contains("limite"));
}

#[sqlx::test(migrations = "./migrations")]
async fn admin_sso_config_crud(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let a = app.new_org("alfa.test").await;
    let org = a.org().to_string();
    let t = Some(a.token.as_str());

    let (st, body) = app.get(&org_path(&org, "sso"), t).await;
    assert_eq!(st, 200);
    assert!(body.is_null());

    let (st, _) = app
        .put(
            &org_path(&org, "sso"),
            t,
            json!({"issuer_url": "http://idp.test", "client_id": "c", "enforce_sso": false}),
        )
        .await;
    assert_eq!(st, 400, "issuer sem https");
    let (st, _) = app
        .put(
            &org_path(&org, "sso"),
            t,
            json!({"issuer_url": "https://idp.test", "client_id": " ", "enforce_sso": false}),
        )
        .await;
    assert_eq!(st, 400);
    let (st, body) = app
        .put(
            &org_path(&org, "sso"),
            t,
            json!({"issuer_url": " https://idp.test ", "client_id": "cid",
                   "client_secret": "segredo", "enforce_sso": false}),
        )
        .await;
    assert_eq!(st, 200, "{body}");
    let (st, body) = app.get(&org_path(&org, "sso"), t).await;
    assert_eq!(st, 200);
    assert_eq!(body["issuer_url"], "https://idp.test");
    assert_eq!(body["client_id"], "cid");
    assert_eq!(body["enforce_sso"], false);
    assert!(body.get("client_secret").is_none(), "{body}");

    // /api/auth/sso/discovery reflecte a configuração.
    let (st, body) = app
        .get("/api/auth/sso/discovery?domain=ALFA.test", None)
        .await;
    assert_eq!(st, 200);
    assert_eq!(body, json!({"sso_enabled": true, "enforce_sso": false}));
    let (_, body) = app.get("/api/auth/sso/discovery", None).await;
    assert_eq!(body, json!({"sso_enabled": false, "enforce_sso": false}));

    let (st, body) = app.delete(&org_path(&org, "sso"), t).await;
    assert_eq!(st, 200);
    assert_eq!(body, json!({"ok": true}));
    let (_, body) = app.get(&org_path(&org, "sso"), t).await;
    assert!(body.is_null());
}

#[sqlx::test(migrations = "./migrations")]
async fn sso_enforced_blocks_password_login(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let a = app.new_org("alfa.test").await;
    let (st, _) = app
        .put(
            &org_path(a.org(), "sso"),
            Some(&a.token),
            json!({"issuer_url": "https://idp.test", "client_id": "cid", "enforce_sso": true}),
        )
        .await;
    assert_eq!(st, 200);
    let (st, body) = app
        .post(
            "/api/auth/login",
            None,
            json!({"email": a.email, "password": PASSWORD}),
        )
        .await;
    assert_eq!(st, 400, "{body}");
    assert!(body["error"].as_str().unwrap().contains("SSO"));
}

#[sqlx::test(migrations = "./migrations")]
async fn admin_webhooks_crud(db: sqlx::PgPool) {
    // `hooks.test` na allowlist: a criação não depende de DNS no CI.
    let app = TestApp::spawn_with(db, &[("WEBHOOK_ALLOW_HOSTS", "hooks.test")]).await;
    let a = app.new_org("alfa.test").await;
    let org = a.org().to_string();
    let t = Some(a.token.as_str());

    let (st, hook) = app
        .post(
            &org_path(&org, "webhooks"),
            t,
            json!({"kind": "generic", "url": "https://hooks.test/x", "secret": "s3cr3t"}),
        )
        .await;
    assert_eq!(st, 200, "{hook}");
    assert_eq!(hook["kind"], "generic");
    assert_eq!(hook["active"], true);
    assert_eq!(
        hook["events"],
        "meeting.created,meeting.started,meeting.mom_ready,recording.ready"
    );
    assert!(
        hook.get("secret").is_none(),
        "o segredo não se devolve: {hook}"
    );

    for (bad, why) in [
        (
            json!({"kind": "discord", "url": "https://hooks.test/x"}),
            "tipo",
        ),
        (
            json!({"kind": "slack", "url": "ftp://hooks.test/x"}),
            "esquema",
        ),
        (
            json!({"kind": "slack", "url": "http://127.0.0.1/x"}),
            "interno",
        ),
        (
            json!({"kind": "slack", "url": "https://u:p@hooks.test/x"}),
            "credenciais",
        ),
        (
            json!({"kind": "slack", "url": "https://hooks.test/x", "events": "meeting.exploded"}),
            "evento",
        ),
    ] {
        let (st, body) = app.post(&org_path(&org, "webhooks"), t, bad).await;
        assert_eq!(st, 400, "{why}: {body}");
    }

    let (st, list) = app.get(&org_path(&org, "webhooks"), t).await;
    assert_eq!(st, 200);
    assert_eq!(list.as_array().unwrap().len(), 1);
    let id = hook["id"].as_str().unwrap();
    let (st, body) = app
        .delete(&org_path(&org, &format!("webhooks/{id}")), t)
        .await;
    assert_eq!(st, 200);
    assert_eq!(body, json!({"ok": true}));
    let (_, list) = app.get(&org_path(&org, "webhooks"), t).await;
    assert!(list.as_array().unwrap().is_empty());
}

#[sqlx::test(migrations = "./migrations")]
async fn admin_api_keys_voice_and_odoo(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let a = app.new_org("alfa.test").await;
    let org = a.org().to_string();
    let t = Some(a.token.as_str());

    // Chaves de API: a chave inteira só aparece na criação.
    let (st, k) = app
        .post(
            &org_path(&org, "api-keys"),
            t,
            json!({"name": "  integração  "}),
        )
        .await;
    assert_eq!(st, 200, "{k}");
    let key = k["key"].as_str().unwrap();
    assert!(key.starts_with("dlx_") && key.len() == 4 + 64);
    assert_eq!(k["prefix"], &key[..12]);
    assert_eq!(k["name"], "integração");
    let (st, list) = app.get(&org_path(&org, "api-keys"), t).await;
    assert_eq!(st, 200);
    assert_eq!(list[0]["id"], k["id"]);
    assert!(list[0].get("key").is_none());
    assert!(list[0]["last_used_at"].is_null());
    let (st, body) = app
        .delete(
            &org_path(&org, &format!("api-keys/{}", k["id"].as_str().unwrap())),
            t,
        )
        .await;
    assert_eq!(st, 204, "{body}");
    let (_, list) = app.get(&org_path(&org, "api-keys"), t).await;
    assert!(list.as_array().unwrap().is_empty());

    // Voz: listas vazias e resumo zerado.
    let (st, dids) = app.get(&org_path(&org, "voice/dids"), t).await;
    assert_eq!(st, 200);
    assert!(dids.is_array());
    let (st, cdr) = app.get(&org_path(&org, "voice/call-records"), t).await;
    assert_eq!(st, 200);
    assert_eq!(cdr, json!([]));
    let (st, bill) = app
        .get(&org_path(&org, "voice/billing?period=week"), t)
        .await;
    assert_eq!(st, 200, "{bill}");
    assert_eq!(bill["period"], "week");
    assert_eq!(bill["calls"], 0);
    assert_eq!(bill["total_minutes"], 0);
    assert_eq!(bill["total_cost"], 0.0);

    // Odoo
    let (st, cfg) = app.get(&org_path(&org, "integrations/odoo"), t).await;
    assert_eq!(st, 200);
    assert_eq!(cfg["org_id"], org.as_str());
    assert_eq!(cfg["odoo_enabled"], false);
    assert!(cfg["odoo_token_prefix"].is_null());
    let (st, body) = app
        .put(
            &org_path(&org, "integrations/odoo"),
            t,
            json!({"odoo_enabled": false, "odoo_url": "https://erp.alfa.test/", "odoo_db": "prod",
                   "hide_org_creation": true, "hide_sso_button": false}),
        )
        .await;
    assert_eq!(st, 200, "{body}");
    let (_, cfg) = app.get(&org_path(&org, "integrations/odoo"), t).await;
    assert_eq!(cfg["odoo_url"], "https://erp.alfa.test");
    assert_eq!(cfg["hide_org_creation"], true);
    assert_eq!(cfg["odoo_admin_id"], a.user_id.as_str());
    let (st, tok) = app
        .post(&org_path(&org, "integrations/odoo/rotate-token"), t, json!({}))
        .await;
    assert_eq!(st, 200, "{tok}");
    let token = tok["token"].as_str().unwrap();
    assert!(token.starts_with("dlxo_"));
    assert_eq!(tok["prefix"], &token[..12]);
    let (_, cfg) = app.get(&org_path(&org, "integrations/odoo"), t).await;
    assert_eq!(
        cfg["odoo_enabled"], true,
        "rodar o token activa a integração"
    );
    // O token dlxo_ autentica a listagem de utilizadores da v1.
    let r = app
        .raw(
            reqwest::Method::GET,
            "/api/integrations/odoo/v1/users",
            &[("X-Integration-Token", token)],
            None,
        )
        .await;
    assert_eq!(r.status, 200, "{}", r.text);
    assert_eq!(r.json()[0]["email"], a.email.as_str());
    assert_eq!(r.json()[0]["role"], "admin");
}

// ---------------------------------------------------------------------------
//  Membro sem papel de administrador
// ---------------------------------------------------------------------------

#[sqlx::test(migrations = "./migrations")]
async fn member_reads_directory_and_creates_groups(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let a = app.new_org("alfa.test").await;
    let c = app.add_member(&a, "carla", "member").await;
    let org = a.org().to_string();
    let t = Some(c.token.as_str());
    for p in ["branches", "members", "groups", "meeting-rooms"] {
        let (st, body) = app.get(&org_path(&org, p), t).await;
        assert_eq!(st, 200, "{p}: {body}");
    }
    // Criar grupos só exige ser membro.
    let (st, g) = app
        .post(&org_path(&org, "groups"), t, json!({"name": "Equipa"}))
        .await;
    assert_eq!(st, 200, "{g}");
    assert_eq!(g["member_count"], 1);
    let (_, orgs) = app.get("/api/orgs", t).await;
    assert_eq!(orgs[0]["role"], "member");
}

/// R153 (fechada): um MEMBRO autenticado que pede uma operação de admin recebia
/// 401, e o web lia-o como «sessão inválida» e gastava um refresh. Agora 403.
#[sqlx::test(migrations = "./migrations")]
async fn member_admin_ops_are_forbidden_403(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let a = app.new_org("alfa.test").await;
    let c = app.add_member(&a, "carla", "member").await;
    let org = a.org().to_string();
    let t = Some(c.token.as_str());

    let gets = [
        "stats",
        "audit-events",
        "audit-events/verification",
        "sso",
        "webhooks",
        "api-keys",
        "voice/dids",
        "voice/call-records",
        "voice/billing",
        "integrations/odoo",
    ];
    for p in gets {
        let (st, body) = app.get(&org_path(&org, p), t).await;
        // R153: membro sem papel de admin → 403 (era 401).
        assert_eq!(st, 403, "GET {p}: {body}");
    }
    let posts = [
        ("branches", json!({"name": "x"})),
        (
            "members",
            json!({"email": "novo@alfa.test", "password": PASSWORD}),
        ),
        ("meeting-rooms", json!({"name": "x"})),
        ("api-keys", json!({"name": "x"})),
        (
            "webhooks",
            json!({"kind": "generic", "url": "https://x.test"}),
        ),
        ("integrations/odoo/rotate-token", json!({})),
    ];
    for (p, b) in posts {
        let (st, body) = app.post(&org_path(&org, p), t, b).await;
        assert_eq!(st, 403, "POST {p}: {body}");
    }
    // As definições são o próprio recurso: `PATCH /api/orgs/{org_id}`.
    let (st, body) = app
        .patch(&format!("/api/orgs/{org}"), t, json!({"domain": "x.test"}))
        .await;
    assert_eq!(st, 403, "PATCH org: {body}");
    let (st, _) = app
        .delete(&org_path(&org, &format!("members/{}", a.user_id)), t)
        .await;
    assert_eq!(st, 403);
    let (st, _) = app
        .patch(
            &org_path(&org, &format!("members/{}", c.user_id)),
            t,
            json!({"role": "admin"}),
        )
        .await;
    assert_eq!(st, 403, "um membro não se promove a si próprio");
    let (_, orgs) = app.get("/api/orgs", t).await;
    assert_eq!(orgs[0]["role"], "member");
}

// ---------------------------------------------------------------------------
//  Isolamento entre organizações
// ---------------------------------------------------------------------------

#[sqlx::test(migrations = "./migrations")]
async fn cross_org_admin_is_denied_on_every_org_route(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let a = app.new_org("alfa.test").await;
    let b = app.new_org("beta.test").await;
    let borg = b.org().to_string();
    let t = Some(a.token.as_str());

    // Controlo positivo: B alcança o que é seu.
    let (st, _) = app.get(&org_path(&borg, "stats"), Some(&b.token)).await;
    assert_eq!(st, 200);
    let (st, own) = app.get(&format!("/api/orgs/{borg}"), Some(&b.token)).await;
    assert_eq!(st, 200, "{own}");
    assert_eq!(own["id"], borg.as_str());
    assert_eq!(own["name"], "Org beta.test");
    assert_eq!(own["role"], "admin");

    let gets = [
        "branches",
        "members",
        "groups",
        "meeting-rooms",
        "stats",
        "audit-events",
        "audit-events/verification",
        "sso",
        "webhooks",
        "api-keys",
        "voice/dids",
        "voice/call-records",
        "voice/billing",
        "integrations/odoo",
    ];
    for p in gets {
        let (st, body) = app.get(&org_path(&borg, p), t).await;
        // Não-membro: 404 (esconde a existência).
        assert_eq!(st, 404, "GET {p}: {body}");
        assert_denied(p, st, &body, &b.email);
    }
    let writes = [
        ("branches", json!({"name": "forjada"})),
        ("groups", json!({"name": "forjado"})),
        (
            "members",
            json!({"email": "x@beta.test", "password": PASSWORD}),
        ),
        ("meeting-rooms", json!({"name": "forjada"})),
        ("api-keys", json!({"name": "forjada"})),
        ("integrations/odoo/rotate-token", json!({})),
    ];
    for (p, body) in writes {
        let (st, resp) = app.post(&org_path(&borg, p), t, body).await;
        assert_eq!(st, 404, "POST {p}: {resp}");
    }
    let (st, resp) = app
        .patch(&format!("/api/orgs/{borg}"), t, json!({"domain": "evil.test"}))
        .await;
    assert_eq!(st, 404, "PATCH org da B: {resp}");
    // Leitura do recurso (rota nova): não-membro 404, sem fuga de dados.
    let (st, resp) = app.get(&format!("/api/orgs/{borg}"), t).await;
    assert_eq!(st, 404, "GET org da B: {resp}");
    assert_denied("GET /api/orgs/{org_id} da B", st, &resp, "Org beta.test");
    let (st, _) = app
        .put(
            &org_path(&borg, "sso"),
            t,
            json!({"issuer_url": "https://evil.test", "client_id": "x", "enforce_sso": true}),
        )
        .await;
    assert_eq!(st, 404);
    let (st, _) = app
        .put(
            &org_path(&borg, "integrations/odoo"),
            t,
            json!({"odoo_enabled": true, "odoo_url": "https://evil.test", "odoo_db": "x",
                   "hide_org_creation": true, "hide_sso_button": true}),
        )
        .await;
    assert_eq!(st, 404);
    let (st, _) = app
        .delete(&org_path(&borg, &format!("members/{}", b.user_id)), t)
        .await;
    assert_eq!(st, 404);
    let (st, _) = app
        .patch(
            &org_path(&borg, &format!("members/{}", b.user_id)),
            t,
            json!({"role": "member"}),
        )
        .await;
    assert_eq!(st, 404);
    let (st, _) = app.delete(&org_path(&borg, "sso"), t).await;
    assert_eq!(st, 404);

    // Nada disto mudou o estado da B.
    let (_, orgs) = app.get("/api/orgs", Some(&b.token)).await;
    assert_eq!(orgs[0]["domain"], "");
    assert_eq!(orgs[0]["role"], "admin");
    let (_, br) = app.get(&org_path(&borg, "branches"), Some(&b.token)).await;
    assert_eq!(br, json!([]));
    let (_, g) = app.get(&org_path(&borg, "groups"), Some(&b.token)).await;
    assert_eq!(g, json!([]));

    // Anónimo: 401.
    let (st, _) = app.get(&org_path(&borg, "stats"), None).await;
    assert_eq!(st, 401);
}

#[sqlx::test(migrations = "./migrations")]
async fn cross_org_delete_leaves_key_and_webhook_alive(db: sqlx::PgPool) {
    let app = TestApp::spawn_with(db, &[("WEBHOOK_ALLOW_HOSTS", "hooks.test")]).await;
    let a = app.new_org("alfa.test").await;
    let b = app.new_org("beta.test").await;
    let borg = b.org().to_string();
    let (key_id, _) = app.api_key(&b).await;
    let (st, hook) = app
        .post(
            &org_path(&borg, "webhooks"),
            Some(&b.token),
            json!({"kind": "generic", "url": "https://hooks.test/h", "secret": "s"}),
        )
        .await;
    assert_eq!(st, 200, "{hook}");
    let hook_id = hook["id"].as_str().unwrap();

    // A ataca pelo caminho da B...
    let (st, _) = app
        .delete(
            &org_path(&borg, &format!("api-keys/{key_id}")),
            Some(&a.token),
        )
        .await;
    assert_eq!(st, 404);
    let (st, _) = app
        .delete(
            &org_path(&borg, &format!("webhooks/{hook_id}")),
            Some(&a.token),
        )
        .await;
    assert_eq!(st, 404);
    // ... e pelo SEU caminho com os ids da B (o DELETE filtra por org_id).
    let (st, _) = app
        .delete(
            &org_path(a.org(), &format!("api-keys/{key_id}")),
            Some(&a.token),
        )
        .await;
    assert_eq!(st, 404, "a chave é da B: não existe nesta organização");
    let (st, _) = app
        .delete(
            &org_path(a.org(), &format!("webhooks/{hook_id}")),
            Some(&a.token),
        )
        .await;
    assert_eq!(st, 200);

    let (_, keys) = app.get(&org_path(&borg, "api-keys"), Some(&b.token)).await;
    assert!(
        keys.to_string().contains(&key_id),
        "a chave da B desapareceu"
    );
    let (_, hooks) = app.get(&org_path(&borg, "webhooks"), Some(&b.token)).await;
    assert!(
        hooks.to_string().contains(hook_id),
        "o webhook da B desapareceu"
    );
}

// ---------------------------------------------------------------------------
//  S3: membro arquivado perde o acesso
// ---------------------------------------------------------------------------

#[sqlx::test(migrations = "./migrations")]
async fn archived_members_lose_org_access(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let a = app.new_org("alfa.test").await;
    let org = a.org().to_string();
    let c = app.add_member(&a, "carla", "member").await;
    let d = app.add_member(&a, "dario", "admin").await;

    let room = app.new_room(&a, "sala da A").await;
    let code = room["code"].as_str().unwrap();
    let rec = app
        .insert_recording(room["id"].as_str().unwrap(), &a.user_id)
        .await;

    // ANTES (controlo positivo)
    let (st, _) = app
        .get(&format!("/api/rooms/{code}/messages"), Some(&c.token))
        .await;
    assert_eq!(st, 200);
    let (_, found) = app.get("/api/users?q=admin-alfa", Some(&c.token)).await;
    assert!(found.to_string().contains(&a.user_id));
    let (st, _) = app.get(&org_path(&org, "stats"), Some(&d.token)).await;
    assert_eq!(st, 200);
    // D (admin) pode descarregar: passa a autorização e só falha no ficheiro.
    let (st, _) = app
        .get(&format!("/api/recordings/{rec}/content?dl=1"), Some(&d.token))
        .await;
    assert_eq!(st, 404, "autorizado; o ficheiro não existe");

    // ARQUIVAR pelo endpoint.
    for u in [&c.user_id, &d.user_id] {
        let (st, _) = app
            .delete(&org_path(&org, &format!("members/{u}")), Some(&a.token))
            .await;
        assert_eq!(st, 200);
    }

    // DEPOIS
    let (st, _) = app
        .get(&format!("/api/rooms/{code}/messages"), Some(&c.token))
        .await;
    assert_eq!(st, 403);
    let (_, found) = app.get("/api/users?q=admin-alfa", Some(&c.token)).await;
    assert_eq!(found, json!([]));
    for p in ["members", "branches", "groups"] {
        let (st, _) = app.get(&org_path(&org, p), Some(&c.token)).await;
        assert_eq!(st, 404, "{p}");
    }
    let (st, _) = app.get(&org_path(&org, "stats"), Some(&d.token)).await;
    assert_eq!(st, 404);
    let (st, _) = app
        .get(&format!("/api/recordings/{rec}/content?dl=1"), Some(&d.token))
        .await;
    assert_eq!(st, 401);
    // A sessão em si continua válida (o JWT não é revogado).
    let (st, _) = app.get("/api/users/me", Some(&c.token)).await;
    assert_eq!(st, 200);
}

/// R152 (fechada): `org::my_orgs` não filtrava `archived_at` — um membro
/// arquivado continuava a ver a organização (com o papel antigo) e o
/// `member_count` contava os arquivados.
#[sqlx::test(migrations = "./migrations")]
async fn my_orgs_hides_org_from_archived_member(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let a = app.new_org("alfa.test").await;
    let c = app.add_member(&a, "carla", "member").await;
    // Controlo positivo: antes de arquivar, vê a org e conta 2.
    let (_, orgs) = app.get("/api/orgs", Some(&c.token)).await;
    assert_eq!(orgs.as_array().unwrap().len(), 1, "{orgs}");
    let (_, orgs) = app.get("/api/orgs", Some(&a.token)).await;
    assert_eq!(orgs[0]["member_count"], 2);

    app.archive_member(a.org(), &c.user_id).await;
    let (st, orgs) = app.get("/api/orgs", Some(&c.token)).await;
    assert_eq!(st, 200);
    assert_eq!(orgs, serde_json::json!([]), "{orgs}");
    // Nem pelo id: arquivado deixa de ser membro activo.
    let (st, _) = app.get(&format!("/api/orgs/{}", a.org()), Some(&c.token)).await;
    assert_eq!(st, 404);
    let (_, orgs) = app.get("/api/orgs", Some(&a.token)).await;
    assert_eq!(orgs[0]["member_count"], 1);
}

// ---------------------------------------------------------------------------
//  S7 / R122: add_employee não captura contas de outra organização
// ---------------------------------------------------------------------------

#[sqlx::test(migrations = "./migrations")]
async fn add_employee_refuses_active_member_of_another_org(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let a = app.new_org("alfa.test").await;
    let v = app.new_org("beta.test").await;
    // Org do atacante LEGADA (email_domain vazio) — salta a barreira do
    // domínio, que a API não deixa editar.
    sqlx::query("UPDATE organizations SET email_domain = '' WHERE id = $1::uuid")
        .bind(a.org())
        .execute(&app.db)
        .await
        .unwrap();

    // Controlo positivo: a org legada ainda adiciona uma conta NOVA de
    // qualquer domínio.
    let (st, body) = app
        .post(
            &org_path(a.org(), "members"),
            Some(&a.token),
            json!({"email": "novo@qualquer.test", "username": "novo", "password": PASSWORD}),
        )
        .await;
    assert_eq!(st, 200, "{body}");

    let (st, body) = app
        .post(
            &org_path(a.org(), "members"),
            Some(&a.token),
            json!({"email": v.email, "role": "admin"}),
        )
        .await;
    assert_eq!(st, 409, "{body}");
    assert!(body["error"]
        .as_str()
        .unwrap()
        .contains("já pertence a outra organização"));

    let in_attacker: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM org_members WHERE user_id = $1::uuid AND org_id = $2::uuid",
    )
    .bind(&v.user_id)
    .bind(a.org())
    .fetch_one(&app.db)
    .await
    .unwrap();
    assert_eq!(in_attacker, 0);
    let (_, orgs) = app.get("/api/orgs", Some(&v.token)).await;
    assert_eq!(orgs.as_array().unwrap().len(), 1);
    assert_eq!(orgs[0]["id"], v.org());
}

#[sqlx::test(migrations = "./migrations")]
async fn add_employee_accepts_account_archived_elsewhere(db: sqlx::PgPool) {
    // A guarda só olha para membros ACTIVOS de outra org: uma conta arquivada
    // na org B pode ser adicionada à org A (legada, sem domínio).
    let app = TestApp::spawn(db).await;
    let a = app.new_org("alfa.test").await;
    let b = app.new_org("beta.test").await;
    let x = app.add_member(&b, "xavier", "member").await;
    app.archive_member(b.org(), &x.user_id).await;
    sqlx::query("UPDATE organizations SET email_domain = '' WHERE id = $1::uuid")
        .bind(a.org())
        .execute(&app.db)
        .await
        .unwrap();
    let (st, body) = app
        .post(
            &org_path(a.org(), "members"),
            Some(&a.token),
            json!({"email": x.email}),
        )
        .await;
    assert_eq!(st, 200, "{body}");
    assert_eq!(body["user_id"], x.user_id.as_str());
}
