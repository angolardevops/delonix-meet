//! Utilizadores e convites (ADR-0008 §7, §9, §11) contra Postgres e servidor
//! reais: directório com filtros e grupos, convites (o token é a credencial),
//! acções em massa, lugares (com concorrência), importação CSV idempotente,
//! regras de entrada e grupos do Odoo no `provision`.
mod common;

use common::{Account, TestApp};
use serde_json::{json, Value};

async fn role_id(app: &TestApp, admin: &Account, key: &str) -> String {
    let (_, page) = app
        .get(
            &format!("/api/orgs/{}/roles", admin.org()),
            Some(&admin.token),
        )
        .await;
    page["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["key"] == key || r["name"] == key)
        .unwrap_or_else(|| panic!("{key}: {page}"))["id"]
        .as_str()
        .unwrap()
        .to_string()
}

async fn invite(app: &TestApp, admin: &Account, email: &str, role: &str) -> (u16, Value) {
    app.post(
        &format!("/api/orgs/{}/invitations", admin.org()),
        Some(&admin.token),
        json!({"email": email, "role_id": role}),
    )
    .await
}

fn find<'a>(items: &'a Value, email: &str) -> &'a Value {
    items["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["email"] == email)
        .unwrap_or_else(|| panic!("{email} em {items}"))
}

#[sqlx::test(migrations = "./migrations")]
async fn directory_lists_members_and_pending_invitations(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let owner = app.new_org("dir.ao").await;
    let org = owner.org().to_string();
    let ana = app.add_member(&owner, "ana", "member").await;
    let bento = app.add_member(&owner, "bento", "admin").await;
    let member = role_id(&app, &owner, "member").await;
    let (st, inv) = invite(&app, &owner, "elio@dir.ao", &member).await;
    assert_eq!(st, 201, "{inv}");
    assert!(inv["token"].as_str().unwrap().starts_with("dlxi_"));
    assert_eq!(inv["delivery_channel"], "manual");
    // Suspender a Ana; o Bento nunca entrou há 60 dias → forçamos a data.
    let (st, b) = app
        .post(
            &format!("/api/orgs/{org}/users/bulk-actions"),
            Some(&owner.token),
            json!({"action": "suspend", "user_ids": [ana.user_id]}),
        )
        .await;
    assert_eq!((st, b["succeeded"].as_u64()), (200, Some(1)), "{b}");
    sqlx::query("UPDATE users SET last_access_at = now() - interval '70 days' WHERE id = $1::uuid")
        .bind(&bento.user_id)
        .execute(&app.db)
        .await
        .unwrap();

    let (st, list) = app
        .get(&format!("/api/orgs/{org}/users"), Some(&owner.token))
        .await;
    assert_eq!(st, 200, "{list}");
    assert_eq!(
        list["counts"],
        json!({"active": 2, "invited": 1, "suspended": 1}),
        "{list}"
    );
    assert_eq!(find(&list, &ana.email)["status"], "suspended");
    assert_eq!(find(&list, "elio@dir.ao")["status"], "invited");
    assert_eq!(find(&list, "elio@dir.ao")["origin"], "invitation");
    assert!(
        owner.last_access_known(&list),
        "o dono entrou: last_access_at preenchido por issue_tokens"
    );
    assert!(!list.to_string().contains("provisioning@delonix.internal"));

    let (_, f) = app
        .get(
            &format!("/api/orgs/{org}/users?filters=inactive_60_days,active"),
            Some(&owner.token),
        )
        .await;
    assert_eq!(f["total"], 1, "{f}");
    assert_eq!(f["items"][0]["email"], bento.email.as_str());
    let (_, q) = app
        .get(&format!("/api/orgs/{org}/users?q=ELIO"), Some(&owner.token))
        .await;
    assert_eq!(q["total"], 1, "{q}");
    let (_, g) = app
        .get(
            &format!("/api/orgs/{org}/users?group_by=status&page_size=1"),
            Some(&owner.token),
        )
        .await;
    let groups = g["groups"].as_array().unwrap();
    assert_eq!(groups.len(), 3, "{g}");
    let next = g["next_page_token"].as_str().unwrap();
    let (st, e) = app
        .get(
            &format!("/api/orgs/{org}/users?page_size=1&q=x&page_token={next}"),
            Some(&owner.token),
        )
        .await;
    assert_eq!(
        (st, e["code"].as_str()),
        (400, Some("search.page_token_mismatch")),
        "{e}"
    );
    let mut seen = Vec::new();
    let mut token: Option<String> = None;
    loop {
        let path = match &token {
            Some(t) => {
                format!("/api/orgs/{org}/users?page_size=2&order_by=-last_access_at&page_token={t}")
            }
            None => format!("/api/orgs/{org}/users?page_size=2&order_by=-last_access_at"),
        };
        let (st, p) = app.get(&path, Some(&owner.token)).await;
        assert_eq!(st, 200, "{p}");
        for i in p["items"].as_array().unwrap() {
            seen.push(i["id"].as_str().unwrap().to_string());
        }
        match p["next_page_token"].as_str() {
            Some(t) => token = Some(t.to_string()),
            None => break,
        }
    }
    let unique: std::collections::BTreeSet<_> = seen.iter().collect();
    assert_eq!(
        (seen.len(), unique.len()),
        (4, 4),
        "paginação sem repetir nem perder: {seen:?}"
    );
    for (bad, code) in [
        ("filters=zzz", "search.unknown_filter"),
        ("group_by=email", "search.invalid_group_by"),
        ("filter=%5B%5D", "search.filter_unsupported"),
        ("order_by=email", "search.invalid_order_by"),
    ] {
        let (st, e) = app
            .get(&format!("/api/orgs/{org}/users?{bad}"), Some(&owner.token))
            .await;
        assert_eq!((st, e["code"].as_str()), (400, Some(code)), "{bad}: {e}");
    }
    let (st, schema) = app
        .get("/api/search/schemas/users", Some(&owner.token))
        .await;
    assert_eq!(st, 200);
    assert_eq!(schema["resource"], "users");
    // Um membro sem admin.manage_accounts não lê o directório.
    let plain = app.add_member(&owner, "simples", "member").await;
    let (st, e) = app
        .get(&format!("/api/orgs/{org}/users"), Some(&plain.token))
        .await;
    assert_eq!(
        (st, e["code"].as_str()),
        (403, Some("authz.missing_capability"))
    );
}

trait LastAccess {
    fn last_access_known(&self, list: &Value) -> bool;
}
impl LastAccess for Account {
    fn last_access_known(&self, list: &Value) -> bool {
        !find(list, &self.email)["last_access_at"].is_null()
    }
}

/// O token é a credencial: uso único, expiração, correio certo, sem captura de
/// contas de outra org; `removed` volta por convite e não por reactivar.
#[sqlx::test(migrations = "./migrations")]
async fn invitation_acceptance_rules(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let owner = app.new_org("convite.ao").await;
    let org = owner.org().to_string();
    let outsider = app.new_org("outro-convite.ao").await;
    let member = role_id(&app, &owner, "member").await;
    let guest = role_id(&app, &owner, "external_guest").await;

    // Alguém que saiu (removed) não se reactiva…
    let ex = app.add_member(&owner, "ex", "member").await;
    let (st, _) = app
        .delete(
            &format!("/api/orgs/{org}/members/{}", ex.user_id),
            Some(&owner.token),
        )
        .await;
    assert_eq!(st, 200);
    let (_, b) = app
        .post(
            &format!("/api/orgs/{org}/users/bulk-actions"),
            Some(&owner.token),
            json!({"action": "reactivate", "user_ids": [ex.user_id]}),
        )
        .await;
    assert_eq!(b["results"][0]["code"], "member.not_reactivatable", "{b}");
    // … volta por convite.
    let (st, inv) = invite(&app, &owner, &ex.email, &member).await;
    assert_eq!(st, 201, "{inv}");
    let token = inv["token"].as_str().unwrap().to_string();
    let (st, e) = app
        .post(
            "/api/invitations/accept",
            Some(&outsider.token),
            json!({"token": token}),
        )
        .await;
    assert_eq!(
        (st, e["code"].as_str()),
        (403, Some("invitation.email_mismatch")),
        "{e}"
    );
    let (st, e) = app
        .post(
            "/api/invitations/accept",
            Some(&ex.token),
            json!({"token": "dlxi_inventado"}),
        )
        .await;
    assert_eq!(
        (st, e["code"].as_str()),
        (404, Some("invitation.not_found"))
    );
    let (st, ok) = app
        .post(
            "/api/invitations/accept",
            Some(&ex.token),
            json!({"token": token}),
        )
        .await;
    assert_eq!(st, 200, "{ok}");
    let (st, _) = app
        .post(
            "/api/invitations/accept",
            Some(&ex.token),
            json!({"token": token}),
        )
        .await;
    assert_eq!(st, 404, "uso único");
    let (_, list) = app
        .get(&format!("/api/orgs/{org}/users"), Some(&owner.token))
        .await;
    assert_eq!(find(&list, &ex.email)["status"], "active");
    assert_eq!(find(&list, &ex.email)["origin"], "invitation");

    // Conta activa noutra org: como membro não; como convidado externo sim, com validade.
    let (st, e) = invite(&app, &owner, &outsider.email, &member).await;
    assert_eq!(
        (st, e["code"].as_str()),
        (422, Some("invitation.domain_not_approved")),
        "{e}"
    );
    let (st, inv) = app
        .post(
            &format!("/api/orgs/{org}/invitations"),
            Some(&owner.token),
            json!({"email": outsider.email, "role_id": guest, "delivery": "code"}),
        )
        .await;
    assert_eq!(st, 201, "{inv}");
    let code = inv["token"].as_str().unwrap().to_lowercase();
    assert_eq!(code.len(), 10);
    let (st, ok) = app
        .post(
            "/api/invitations/accept",
            Some(&outsider.token),
            json!({"token": code}),
        )
        .await;
    assert_eq!(st, 200, "o código aceita-se sem maiúsculas: {ok}");
    assert!(!ok["access_expires_at"].is_null());

    // Reenviar roda o token.
    let (_, inv) = invite(&app, &owner, "nova@convite.ao", &member).await;
    let old = inv["token"].as_str().unwrap().to_string();
    let id = inv["id"].as_str().unwrap();
    let (st, re) = app
        .post(
            &format!("/api/orgs/{org}/invitations/{id}/resend"),
            Some(&owner.token),
            json!({}),
        )
        .await;
    assert_eq!(st, 200, "{re}");
    assert_ne!(re["token"], old.as_str());
    // Convite expirado: recusado e marcado.
    sqlx::query(
        "UPDATE org_invitations SET expires_at = now() - interval '1 minute' WHERE id = $1::uuid",
    )
    .bind(id)
    .execute(&app.db)
    .await
    .unwrap();
    let (st, dup) = invite(&app, &owner, "nova@convite.ao", &member).await;
    assert_eq!(st, 201, "um expirado não bloqueia um novo convite: {dup}");
    // Revogar: 204, depois 409.
    let did = dup["id"].as_str().unwrap();
    let (st, _) = app
        .delete(
            &format!("/api/orgs/{org}/invitations/{did}"),
            Some(&owner.token),
        )
        .await;
    assert_eq!(st, 204);
    let (st, _) = app
        .delete(
            &format!("/api/orgs/{org}/invitations/{did}"),
            Some(&owner.token),
        )
        .await;
    assert_eq!(st, 409);
    // Dono não se convida.
    let owner_role = role_id(&app, &owner, "owner").await;
    let (st, e) = invite(&app, &owner, "chefe@convite.ao", &owner_role).await;
    assert_eq!(
        (st, e["code"].as_str()),
        (422, Some("invitation.owner_forbidden"))
    );
}

/// Tecto de lugares: o operador fixa-o; activações concorrentes não o passam;
/// convidado externo não ocupa; libertar lugares suspende inactivos, nunca o dono.
#[sqlx::test(migrations = "./migrations")]
async fn seats(db: sqlx::PgPool) {
    // O administrador da plataforma tem de existir antes do servidor (config).
    let platform: (uuid::Uuid,) = sqlx::query_as(
        "INSERT INTO users (email, username, password_hash) VALUES ('op@plataforma.ao', 'operador', '') RETURNING id")
        .fetch_one(&db).await.unwrap();
    let app =
        TestApp::spawn_with(db, &[("PLATFORM_ADMIN_USER_IDS", &platform.0.to_string())]).await;
    let owner = app.new_org("lugares.ao").await;
    let org = owner.org().to_string();
    let a = app.add_member(&owner, "a", "member").await;
    let b = app.add_member(&owner, "b", "member").await;
    let c = app.add_member(&owner, "c", "member").await;
    // Um token do operador: dá-se-lhe password e entra.
    let hash =
        sqlx::query_scalar::<_, String>("SELECT password_hash FROM users WHERE id = $1::uuid")
            .bind(&owner.user_id)
            .fetch_one(&app.db)
            .await
            .unwrap();
    sqlx::query("UPDATE users SET password_hash = $1 WHERE id = $2")
        .bind(hash)
        .bind(platform.0)
        .execute(&app.db)
        .await
        .unwrap();
    let op = app.login("op@plataforma.ao").await;

    let (st, e) = app
        .put(
            &format!("/api/operator/v1/organizations/{org}/seats"),
            Some(&owner.token),
            json!({"max_seats": 3}),
        )
        .await;
    assert_eq!(st, 403, "o admin da org não se licencia: {e}");
    // Suspende b e c; 2 ocupados (dono + a). Tecto 3 → um só lugar livre.
    for u in [&b, &c] {
        let (_, r) = app
            .post(
                &format!("/api/orgs/{org}/users/bulk-actions"),
                Some(&owner.token),
                json!({"action": "suspend", "user_ids": [u.user_id]}),
            )
            .await;
        assert_eq!(r["succeeded"], 1, "{r}");
    }
    let (st, s) = app
        .put(
            &format!("/api/operator/v1/organizations/{org}/seats"),
            Some(&op.token),
            json!({"max_seats": 3}),
        )
        .await;
    assert_eq!(st, 200, "{s}");
    assert_eq!(
        (s["used"].as_i64(), s["available"].as_i64()),
        (Some(2), Some(1)),
        "{s}"
    );
    // Duas reactivações em simultâneo: uma entra, a outra dá seats.limit_reached.
    let path = format!("/api/orgs/{org}/users/bulk-actions");
    let (r1, r2) = tokio::join!(
        app.post(
            &path,
            Some(&owner.token),
            json!({"action": "reactivate", "user_ids": [b.user_id]})
        ),
        app.post(
            &path,
            Some(&owner.token),
            json!({"action": "reactivate", "user_ids": [c.user_id]})
        )
    );
    let oks = [&r1.1, &r2.1]
        .iter()
        .filter(|r| r["results"][0]["ok"] == true)
        .count();
    assert_eq!(oks, 1, "{r1:?} {r2:?}");
    let refused = if r1.1["results"][0]["ok"] == true {
        &r2.1
    } else {
        &r1.1
    };
    assert_eq!(
        refused["results"][0]["code"], "seats.limit_reached",
        "{refused}"
    );
    // Convidado externo não ocupa lugar.
    let guest = role_id(&app, &owner, "external_guest").await;
    let (st, inv) = app
        .post(
            &format!("/api/orgs/{org}/invitations"),
            Some(&owner.token),
            json!({"email": op.email, "role_id": guest}),
        )
        .await;
    assert_eq!(st, 201, "{inv}");
    let (st, acc) = app
        .post(
            "/api/invitations/accept",
            Some(&op.token),
            json!({"token": inv["token"]}),
        )
        .await;
    assert_eq!(st, 200, "{acc}");
    // Libertar: a entrou há muito; o dono nunca.
    sqlx::query("UPDATE users SET last_access_at = now() - interval '90 days' WHERE id IN ($1::uuid, $2::uuid)")
        .bind(&a.user_id).bind(&owner.user_id).execute(&app.db).await.unwrap();
    let (_, dry) = app
        .post(
            &format!("/api/orgs/{org}/seats/release"),
            Some(&owner.token),
            json!({"inactive_days": 60, "dry_run": true}),
        )
        .await;
    let cands: Vec<&str> = dry["candidates"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c.as_str().unwrap())
        .collect();
    assert!(
        cands.contains(&a.user_id.as_str()) && !cands.contains(&owner.user_id.as_str()),
        "{dry}"
    );
    let (st, rel) = app
        .post(
            &format!("/api/orgs/{org}/seats/release"),
            Some(&owner.token),
            json!({"inactive_days": 60}),
        )
        .await;
    assert_eq!(st, 200, "{rel}");
    let (_, seats) = app
        .get(
            &format!("/api/orgs/{org}/seats?inactive_days=60"),
            Some(&owner.token),
        )
        .await;
    assert_eq!(seats["limit"], 3);
    assert!(seats["used"].as_i64().unwrap() <= 2, "{seats}");
}

#[sqlx::test(migrations = "./migrations")]
async fn csv_import_is_idempotent(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let owner = app.new_org("importa.ao").await;
    let org = owner.org().to_string();
    let (_, d) = app
        .post(
            &format!("/api/orgs/{org}/departments"),
            Some(&owner.token),
            json!({"name": "Formação"}),
        )
        .await;
    assert!(d["id"].is_string(), "{d}");
    let existing = app.add_member(&owner, "ja", "member").await;
    let csv = format!(
        "email;role;department\nnova@importa.ao;Membro;Formação\n{};Administrador;\nnova@importa.ao;member;\nmau-correio;member;\nx@importa.ao;Inexistente;\n",
        existing.email
    );
    let (st, dry) = app
        .post(
            &format!("/api/orgs/{org}/users/imports"),
            Some(&owner.token),
            json!({"csv": csv, "dry_run": true}),
        )
        .await;
    assert_eq!(st, 200, "{dry}");
    assert_eq!(
        (
            dry["invited"].as_u64(),
            dry["updated"].as_u64(),
            dry["errors"].as_u64()
        ),
        (Some(1), Some(1), Some(3)),
        "{dry}"
    );
    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM org_invitations WHERE org_id = $1::uuid")
        .bind(&org)
        .fetch_one(&app.db)
        .await
        .unwrap();
    assert_eq!(n, 0, "dry_run não escreve");
    let (_, first) = app
        .post(
            &format!("/api/orgs/{org}/users/imports"),
            Some(&owner.token),
            json!({"csv": csv}),
        )
        .await;
    assert_eq!(
        (first["invited"].as_u64(), first["updated"].as_u64()),
        (Some(1), Some(1)),
        "{first}"
    );
    let codes: Vec<&str> = first["lines"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|l| l["code"].as_str())
        .collect();
    assert_eq!(
        codes,
        vec![
            "import.duplicate_in_file",
            "import.invalid_email",
            "import.unknown_role"
        ],
        "{first}"
    );
    let (_, second) = app
        .post(
            &format!("/api/orgs/{org}/users/imports"),
            Some(&owner.token),
            json!({"csv": csv}),
        )
        .await;
    assert_eq!(
        (
            second["invited"].as_u64(),
            second["updated"].as_u64(),
            second["unchanged"].as_u64()
        ),
        (Some(0), Some(0), Some(2)),
        "{second}"
    );
    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM org_invitations WHERE org_id = $1::uuid")
        .bind(&org)
        .fetch_one(&app.db)
        .await
        .unwrap();
    assert_eq!(n, 1, "sem duplicados");
}

#[sqlx::test(migrations = "./migrations")]
async fn entry_rules_departments_and_provisioning(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let owner = app.new_org("regras.ao").await;
    let org = owner.org().to_string();
    let (_, rules) = app
        .get(&format!("/api/orgs/{org}/entry-rules"), Some(&owner.token))
        .await;
    assert_eq!(rules["create_account_on_first_login"], true);
    assert_eq!(rules["attendance_log"]["available"], false);
    let put = |att: bool| {
        json!({"create_account_on_first_login": true, "approved_domains": ["regras.ao", "@parceiro.na"],
        "suspend_on_odoo_exit": true, "external_guest_ttl_hours": 24, "attendance_log_enabled": att})
    };
    let (st, e) = app
        .put(
            &format!("/api/orgs/{org}/entry-rules"),
            Some(&owner.token),
            put(true),
        )
        .await;
    assert_eq!(
        (st, e["code"].as_str()),
        (422, Some("entry_rules.attendance_unavailable"))
    );
    let (st, r) = app
        .put(
            &format!("/api/orgs/{org}/entry-rules"),
            Some(&owner.token),
            put(false),
        )
        .await;
    assert_eq!(st, 200, "{r}");
    assert_eq!(r["approved_domains"], json!(["parceiro.na", "regras.ao"]));
    // Departamento vindo do Odoo não se renomeia à mão.
    let (_, dep) = app
        .post(
            &format!("/api/orgs/{org}/departments"),
            Some(&owner.token),
            json!({"name": "TI"}),
        )
        .await;
    let dep_id = dep["id"].as_str().unwrap();
    sqlx::query("UPDATE departments SET source = 'odoo' WHERE id = $1::uuid")
        .bind(dep_id)
        .execute(&app.db)
        .await
        .unwrap();
    let (st, e) = app
        .patch(
            &format!("/api/orgs/{org}/departments/{dep_id}"),
            Some(&owner.token),
            json!({"name": "X"}),
        )
        .await;
    assert_eq!(
        (st, e["code"].as_str()),
        (409, Some("department.managed_by_odoo"))
    );
    let (st, p) = app
        .get(&format!("/api/orgs/{org}/provisioning"), Some(&owner.token))
        .await;
    assert_eq!(st, 200, "{p}");
    assert_eq!(p["odoo"]["enabled"], false);
    assert!(p["odoo"]["last_result"].is_null());
}

/// Grupos do Odoo no `provision`: atribuem o papel mapeado a contas desta org,
/// voltam a Membro quando saem, nunca dão Proprietário, e um papel manual gera
/// conflito em vez de ser esmagado.
#[sqlx::test(migrations = "./migrations")]
async fn odoo_groups_map_roles_on_provision(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let owner = app.new_org("odoo-grupos.ao").await;
    let org = owner.org().to_string();
    let (_, tok) = app
        .post(
            &format!("/api/orgs/{org}/integrations/odoo/rotate-token"),
            Some(&owner.token),
            json!({}),
        )
        .await;
    let dlxo = tok["token"].as_str().unwrap().to_string();
    let (st, role) = app.post(&format!("/api/orgs/{org}/roles"), Some(&owner.token), json!({"name": "Gestor de emissão",
        "odoo_group": "delonix_comunicacao.emissao", "capabilities": {"broadcast.manage_rtmp_keys": "allow"}})).await;
    assert_eq!(st, 201, "{role}");
    let role_id_ = role["id"].as_str().unwrap().to_string();
    let owner_role = role_id(&app, &owner, "owner").await;
    let (st, e) = app
        .patch(
            &format!("/api/orgs/{org}/roles/{owner_role}"),
            Some(&owner.token),
            json!({"odoo_group": "x.y"}),
        )
        .await;
    assert_eq!(st, 422, "{e}");

    let provision = |groups: Value, full: bool| {
        json!({"company": "Org odoo-grupos.ao", "admin_email": owner.email,
        "full_directory": full,
        "users": [{"odoo_uid": 7, "name": "Teresa Kiala", "email": "teresa@odoo-grupos.ao", "groups": groups,
                   "department_id": 3, "department_name": "Comunicação"}]})
    };
    let (st, r) = app
        .post(
            "/api/integrations/odoo/v1/provision",
            Some(&dlxo),
            provision(json!(["delonix_comunicacao.emissao"]), false),
        )
        .await;
    assert_eq!(st, 200, "{r}");
    assert_eq!(r["role_changes"], 1, "{r}");
    let (_, list) = app
        .get(
            &format!("/api/orgs/{org}/users?q=teresa"),
            Some(&owner.token),
        )
        .await;
    let t = &list["items"][0];
    assert_eq!(
        (
            t["role_id"].as_str(),
            t["department_name"].as_str(),
            t["origin"].as_str()
        ),
        (
            Some(role_id_.as_str()),
            Some("Comunicação"),
            Some("odoo_sso")
        ),
        "{list}"
    );
    let (_, got) = app
        .get(
            &format!("/api/orgs/{org}/roles/{role_id_}"),
            Some(&owner.token),
        )
        .await;
    assert_eq!(
        (
            got["odoo_sync"]["total"].as_i64(),
            got["odoo_sync"]["synced"].as_i64()
        ),
        (Some(1), Some(1)),
        "{got}"
    );

    // Sai do grupo → Membro.
    let (_, r) = app
        .post(
            "/api/integrations/odoo/v1/provision",
            Some(&dlxo),
            provision(json!([]), false),
        )
        .await;
    assert_eq!(r["role_changes"], 1, "{r}");
    // Papel dado à mão + grupo → conflito, sem mudar.
    let teresa_id = t["user_id"].as_str().unwrap().to_string();
    let admin_role = role_id(&app, &owner, "admin").await;
    let (st, _) = app
        .put(
            &format!("/api/orgs/{org}/members/{teresa_id}/role"),
            Some(&owner.token),
            json!({"role_id": admin_role}),
        )
        .await;
    assert_eq!(st, 200);
    let (_, r) = app
        .post(
            "/api/integrations/odoo/v1/provision",
            Some(&dlxo),
            provision(json!(["delonix_comunicacao.emissao"]), false),
        )
        .await;
    assert_eq!(
        (r["role_changes"].as_u64(), r["role_conflicts"].as_u64()),
        (Some(0), Some(1)),
        "{r}"
    );
    let (_, conflicts) = app
        .get(
            &format!("/api/orgs/{org}/role-conflicts"),
            Some(&owner.token),
        )
        .await;
    let cid = conflicts["items"][0]["id"].as_str().unwrap();
    let (st, res) = app
        .post(
            &format!("/api/orgs/{org}/role-conflicts/{cid}/resolve"),
            Some(&owner.token),
            json!({"decision": "apply_proposed"}),
        )
        .await;
    assert_eq!(
        (st, res["status"].as_str()),
        (200, Some("resolved")),
        "{res}"
    );
    // Sai do Odoo (lista completa) com a regra activa → suspenso `odoo_exit`.
    let (st, _) = app
        .put(
            &format!("/api/orgs/{org}/entry-rules"),
            Some(&owner.token),
            json!({"create_account_on_first_login": true,
        "approved_domains": [], "suspend_on_odoo_exit": true, "external_guest_ttl_hours": 24}),
        )
        .await;
    assert_eq!(st, 200);
    let empty = json!({"company": "Org odoo-grupos.ao", "admin_email": owner.email, "full_directory": true, "users": []});
    let (_, r) = app
        .post("/api/integrations/odoo/v1/provision", Some(&dlxo), empty)
        .await;
    assert_eq!(r["suspended"], 1, "{r}");
    let (_, p) = app
        .get(&format!("/api/orgs/{org}/provisioning"), Some(&owner.token))
        .await;
    assert_eq!(p["odoo"]["last_result"]["suspended"], 1, "{p}");
    // Regra «criar conta na primeira entrada» desligada: conta nova não nasce.
    let (st, _) = app
        .put(
            &format!("/api/orgs/{org}/entry-rules"),
            Some(&owner.token),
            json!({"create_account_on_first_login": false,
        "approved_domains": [], "suspend_on_odoo_exit": false, "external_guest_ttl_hours": 24}),
        )
        .await;
    assert_eq!(st, 200);
    let (_, r) = app.post("/api/integrations/odoo/v1/provision", Some(&dlxo), json!({"company": "Org odoo-grupos.ao",
        "admin_email": owner.email, "users": [{"odoo_uid": 9, "name": "Novo", "email": "novo@odoo-grupos.ao"}]})).await;
    assert_eq!(
        (
            r["created"].as_u64(),
            r["skipped"].as_array().map(|a| a.len())
        ),
        (Some(0), Some(1)),
        "{r}"
    );
}
