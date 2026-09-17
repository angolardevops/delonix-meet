//! Papéis, capacidades e âmbito (ADR-0008) contra Postgres e servidor reais:
//! não-regressão dos pontos migrados, invariantes (dono, sem escalada, sistema
//! imutável, não imposta), aprovações, âmbito de departamento, SoD, simulação e
//! a equivalência entre a tabela materializada e a policy.
mod common;

use common::{Account, TestApp, INVENTED_ID, PASSWORD};
use delonix_meet_domain::identity::authorization::{Capability, SystemRole};
use serde_json::{json, Value};

async fn role_id(app: &TestApp, admin: &Account, key: &str) -> String {
    let (st, page) = app
        .get(
            &format!("/api/orgs/{}/roles", admin.org()),
            Some(&admin.token),
        )
        .await;
    assert_eq!(st, 200, "{page}");
    page["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["key"] == key || r["name"] == key)
        .unwrap_or_else(|| panic!("papel {key} em {page}"))["id"]
        .as_str()
        .unwrap()
        .to_string()
}

async fn create_role(app: &TestApp, admin: &Account, body: Value) -> String {
    let (st, r) = app
        .post(
            &format!("/api/orgs/{}/roles", admin.org()),
            Some(&admin.token),
            body,
        )
        .await;
    assert_eq!(st, 201, "{r}");
    r["id"].as_str().unwrap().to_string()
}

async fn assign(app: &TestApp, admin: &Account, user: &Account, role: &str) -> (u16, Value) {
    app.put(
        &format!("/api/orgs/{}/members/{}/role", admin.org(), user.user_id),
        Some(&admin.token),
        json!({"role_id": role}),
    )
    .await
}

/// A matriz semeada na migração é a do domínio (uma só fonte da semântica).
#[sqlx::test(migrations = "./migrations")]
async fn seeded_system_defaults_match_the_domain(db: sqlx::PgPool) {
    let rows: Vec<(String, String, String)> =
        sqlx::query_as("SELECT system_key, capability, value FROM system_role_capability_defaults")
            .fetch_all(&db)
            .await
            .unwrap();
    assert_eq!(rows.len(), SystemRole::ALL.len() * Capability::ALL.len());
    for (key, cap, value) in rows {
        let role = SystemRole::parse(&key).unwrap();
        let cap = Capability::parse(&cap).unwrap();
        assert_eq!(
            role.default_value(cap).as_str(),
            value,
            "{key} {}",
            cap.as_str()
        );
    }
}

/// Registo → o criador é dono; o texto herdado continua `admin`.
#[sqlx::test(migrations = "./migrations")]
async fn creator_is_owner_and_legacy_role_is_derived(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let a = app.new_org("dono.ao").await;
    let (st, org) = app
        .get(&format!("/api/orgs/{}", a.org()), Some(&a.token))
        .await;
    assert_eq!(st, 200, "{org}");
    assert_eq!(org["role"], "admin");
    assert_eq!(org["role_key"], "owner");
    assert_eq!(org["owner_missing"], false);
    let m = app.add_member(&a, "membro", "member").await;
    let (_, org) = app
        .get(&format!("/api/orgs/{}", a.org()), Some(&m.token))
        .await;
    assert_eq!(org["role_key"], "member");
}

/// Não-regressão: cada ponto migrado responde como antes para dono, admin,
/// membro, arquivado e outra org. Só muda o `code` do 403.
#[sqlx::test(migrations = "./migrations")]
async fn migrated_points_keep_their_status_table(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let owner = app.new_org("regressao.ao").await;
    let admin = app.add_member(&owner, "adm", "admin").await;
    let member = app.add_member(&owner, "mem", "member").await;
    let gone = app.add_member(&owner, "saiu", "member").await;
    app.archive_member(owner.org(), &gone.user_id).await;
    let other = app.new_org("outra-regressao.ao").await;
    let org = owner.org().to_string();

    let reads = [
        format!("/api/orgs/{org}/audit-events"),
        format!("/api/orgs/{org}/audit-events/verification"),
        format!("/api/orgs/{org}/stream-destinations"),
        format!("/api/orgs/{org}/api-keys"),
        format!("/api/orgs/{org}/webhooks"),
        format!("/api/orgs/{org}/stats"),
    ];
    for path in &reads {
        for (who, acc, want) in [
            ("dono", &owner, 200),
            ("admin", &admin, 200),
            ("membro", &member, 403),
            ("arquivado", &gone, 404),
            ("outra org", &other, 404),
        ] {
            let (st, body) = app.get(path, Some(&acc.token)).await;
            assert_eq!(st, want, "{who} GET {path}: {body}");
            if want == 403 {
                assert_eq!(body["code"], "authz.missing_capability", "{body}");
            }
        }
    }
    // Escritas migradas: definições e membros.
    let settings = json!({"domain": "", "retention_days": 0});
    for (who, acc, want) in [
        ("membro", &member, 403),
        ("arquivado", &gone, 404),
        ("outra org", &other, 404),
        ("admin", &admin, 200),
        ("dono", &owner, 200),
    ] {
        let (st, body) = app
            .patch(
                &format!("/api/orgs/{org}"),
                Some(&acc.token),
                settings.clone(),
            )
            .await;
        assert_eq!(st, want, "{who} PATCH org: {body}");
        let (st, body) = app
            .post(
                &format!("/api/orgs/{org}/members"),
                Some(&acc.token),
                json!({"email": format!("novo-{who}@regressao.ao").replace(' ', ""), "password": PASSWORD}),
            )
            .await;
        assert_eq!(st, want, "{who} POST members: {body}");
    }
    // Criar salas continua aberto a membros (e a contas sem org).
    let (st, body) = app
        .post("/api/rooms", Some(&member.token), json!({"name": "sala"}))
        .await;
    assert_eq!(st, 200, "{body}");
    let (st, body) = app
        .post("/api/rooms", Some(&other.token), json!({"name": "sala"}))
        .await;
    assert_eq!(st, 200, "{body}");
}

/// Invariantes do dono (ADR-0008 §5), no serviço e na base.
#[sqlx::test(migrations = "./migrations")]
async fn last_owner_and_owner_assignment(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let owner = app.new_org("ultimo-dono.ao").await;
    let admin = app.add_member(&owner, "adm", "admin").await;
    let member_role = role_id(&app, &owner, "member").await;
    let owner_role = role_id(&app, &owner, "owner").await;

    // O dono não se despromove sendo o único.
    let (st, body) = assign(&app, &owner, &owner, &member_role).await;
    assert_eq!(st, 409, "{body}");
    assert_eq!(body["code"], "role.last_owner");
    // O legado também não: o admin não arquiva o dono, nem lhe tira o papel.
    let (st, body) = app
        .delete(
            &format!("/api/orgs/{}/members/{}", owner.org(), owner.user_id),
            Some(&admin.token),
        )
        .await;
    assert_eq!(st, 409, "{body}");
    let (st, body) = app
        .patch(
            &format!("/api/orgs/{}/members/{}", owner.org(), owner.user_id),
            Some(&admin.token),
            json!({"role": "member"}),
        )
        .await;
    assert_eq!(st, 409, "{body}");
    // Só um dono dá o papel de dono.
    let (st, body) = assign(&app, &admin, &admin, &owner_role).await;
    assert_eq!(st, 403, "{body}");
    assert_eq!(body["code"], "role.owner_assignment");
    let (st, body) = assign(&app, &owner, &admin, &owner_role).await;
    assert_eq!(st, 200, "{body}");
    // Com dois donos, um pode passar a membro.
    let (st, body) = assign(&app, &admin, &owner, &member_role).await;
    assert_eq!(st, 200, "{body}");
    // Segunda linha: a base recusa deixar a org sem dono.
    let err = sqlx::query("UPDATE org_members SET archived_at = now() WHERE user_id = $1::uuid")
        .bind(&admin.user_id)
        .execute(&app.db)
        .await
        .unwrap_err();
    assert!(err.to_string().contains("role.last_owner"), "{err}");
}

/// Uma escrita herdada de `role` não esmaga um papel personalizado.
#[sqlx::test(migrations = "./migrations")]
async fn legacy_role_update_cannot_overwrite_role_id(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let owner = app.new_org("heranca.ao").await;
    let m = app.add_member(&owner, "form", "member").await;
    let formador = create_role(
        &app,
        &owner,
        json!({"name": "Formador",
        "capabilities": {"admin.view_audit": "allow"}}),
    )
    .await;
    let (st, body) = assign(&app, &owner, &m, &formador).await;
    assert_eq!(st, 200, "{body}");
    // `role` (derivado) de um personalizado é 'member'; escrever 'admin' sem mudar role_id é recusado.
    let err = sqlx::query("UPDATE org_members SET role = 'admin' WHERE user_id = $1::uuid")
        .bind(&m.user_id)
        .execute(&app.db)
        .await
        .unwrap_err();
    assert!(err.to_string().contains("derivado de role_id"), "{err}");
    // Reenviar a pessoa pelo legado sem `role` não lhe tira o papel.
    let (st, body) = app
        .post(
            &format!("/api/orgs/{}/members", owner.org()),
            Some(&owner.token),
            json!({"email": m.email, "title": "Formadora"}),
        )
        .await;
    assert_eq!(st, 200, "{body}");
    let (_, me) = app
        .get(
            &format!("/api/orgs/{}/members/me/capabilities", owner.org()),
            Some(&m.token),
        )
        .await;
    assert_eq!(me["role_id"], formador.as_str(), "{me}");
    // E a capacidade do papel é mesmo imposta.
    let (st, _) = app
        .get(
            &format!("/api/orgs/{}/audit-events", owner.org()),
            Some(&m.token),
        )
        .await;
    assert_eq!(st, 200);
}

/// Nenhuma escrita de `org_members.role` por UPDATE fora do gatilho: o papel
/// muda por `role_id` (`org::set_system_role` e companhia). Um escritor herdado
/// que volte a fazer `SET role = …` falha aqui antes de falhar na base.
#[test]
fn no_legacy_role_updates_in_source() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut bad = Vec::new();
    for entry in std::fs::read_dir(&dir).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().and_then(|e| e.to_str()) != Some("rs") {
            continue;
        }
        let text = std::fs::read_to_string(&path).unwrap();
        let flat: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
        if flat.contains("DO UPDATE SET role =")
            || flat.contains("DO UPDATE SET role=")
            || sets_role(&flat)
        {
            bad.push(path.display().to_string());
        }
    }
    assert!(
        bad.is_empty(),
        "escrita herdada de org_members.role em {bad:?}"
    );
}

/// Há um `UPDATE org_members SET … role = …` (não `role_id`/`role_source`)?
fn sets_role(flat: &str) -> bool {
    flat.match_indices("UPDATE org_members SET").any(|(i, _)| {
        let stmt = &flat[i + "UPDATE org_members SET".len()..];
        let set = &stmt[..stmt.find("WHERE").unwrap_or(stmt.len())];
        set.split(',').any(|assignment| {
            assignment
                .split('=')
                .next()
                .map(|lhs| lhs.trim() == "role")
                .unwrap_or(false)
        })
    })
}

/// Papéis de sistema imutáveis; capacidades e limites não impostos não se gravam;
/// `org.administer` nunca num personalizado.
#[sqlx::test(migrations = "./migrations")]
async fn system_roles_and_unenforced_fields_are_locked(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let owner = app.new_org("bloqueios.ao").await;
    let org = owner.org().to_string();
    let admin_role = role_id(&app, &owner, "admin").await;
    let path = format!("/api/orgs/{org}/roles/{admin_role}/capabilities");
    let (st, body) = app
        .put(
            &path,
            Some(&owner.token),
            json!({"values": {"admin.change_retention": "requires_approval"}}),
        )
        .await;
    assert_eq!(
        (st, body["code"].as_str()),
        (422, Some("role.system_immutable")),
        "{body}"
    );
    let (st, body) = app
        .delete(
            &format!("/api/orgs/{org}/roles/{admin_role}"),
            Some(&owner.token),
        )
        .await;
    assert_eq!(st, 422, "{body}");
    for (caps, code) in [
        (
            json!({"studio.export_4k": "allow"}),
            "authz.capability_not_enforced",
        ),
        (
            json!({"org.administer": "allow"}),
            "authz.system_only_capability",
        ),
        (
            json!({"recordings.view_others": "requires_approval"}),
            "authz.approval_not_supported",
        ),
    ] {
        let (st, body) = app
            .post(
                &format!("/api/orgs/{org}/roles"),
                Some(&owner.token),
                json!({"name": format!("x-{code}"), "capabilities": caps}),
            )
            .await;
        assert_eq!((st, body["code"].as_str()), (422, Some(code)), "{body}");
    }
    let (st, body) = app
        .post(
            &format!("/api/orgs/{org}/roles"),
            Some(&owner.token),
            json!({"name": "Longo", "limits": {"max_session_minutes": 240}}),
        )
        .await;
    assert_eq!(
        (st, body["code"].as_str()),
        (422, Some("role.limit_not_enforced")),
        "{body}"
    );
    let (st, body) = app
        .post(
            &format!("/api/orgs/{org}/roles"),
            Some(&owner.token),
            json!({"name": "Emissor", "limits": {"max_simultaneous_destinations": 5},
                     "capabilities": {"broadcast.manage_rtmp_keys": "allow"}}),
        )
        .await;
    assert_eq!(st, 201, "{body}");
    assert_eq!(
        body["limits"]["max_simultaneous_destinations"]["effective"],
        5
    );
    assert_eq!(body["limits"]["max_session_minutes"]["enforced"], false);
    let (_, cat) = app.get("/api/capabilities", Some(&owner.token)).await;
    let export = cat["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["code"] == "studio.export_4k")
        .unwrap();
    assert_eq!(export["enforced"], false);
    let (_, col) = app
        .get(
            &format!(
                "/api/orgs/{org}/roles/{}/capabilities",
                body["id"].as_str().unwrap()
            ),
            Some(&owner.token),
        )
        .await;
    let row = col["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["capability"] == "studio.export_4k")
        .unwrap();
    assert_eq!(
        (row["locked"].as_bool(), row["locked_reason"].as_str()),
        (Some(true), Some("not_enforced"))
    );
}

/// Sem escalada: quem gere papéis sem uma capacidade não a dá nem atribui quem a tem.
#[sqlx::test(migrations = "./migrations")]
async fn no_escalation(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let owner = app.new_org("escalada.ao").await;
    let org = owner.org().to_string();
    let rh = app.add_member(&owner, "rh", "member").await;
    let other = app.add_member(&owner, "outro", "member").await;
    let gestor_rh = create_role(&app, &owner, json!({"name": "Gestor RH", "capabilities": {
        "admin.manage_roles": "allow", "admin.manage_accounts": "allow", "admin.view_audit": "allow"}})).await;
    assert_eq!(assign(&app, &owner, &rh, &gestor_rh).await.0, 200);

    let (st, body) = app
        .post(
            &format!("/api/orgs/{org}/roles"),
            Some(&rh.token),
            json!({"name": "Emissão", "capabilities": {"broadcast.manage_rtmp_keys": "allow"}}),
        )
        .await;
    assert_eq!(
        (st, body["code"].as_str()),
        (403, Some("authz.escalation")),
        "{body}"
    );
    // Dar-se a si próprio o Administrador: escalada.
    let admin_role = role_id(&app, &owner, "admin").await;
    let (st, body) = assign(&app, &rh, &rh, &admin_role).await;
    assert_eq!(
        (st, body["code"].as_str()),
        (403, Some("authz.escalation")),
        "{body}"
    );
    // … nem pela porta legada.
    let (st, body) = app
        .patch(
            &format!("/api/orgs/{org}/members/{}", other.user_id),
            Some(&rh.token),
            json!({"role": "admin"}),
        )
        .await;
    assert_eq!(
        (st, body["code"].as_str()),
        (403, Some("authz.escalation")),
        "{body}"
    );
    // Mas cria um papel dentro do que tem, e atribui-o.
    let auditor = create_role(
        &app,
        &rh,
        json!({"name": "Auditor", "capabilities": {"admin.view_audit": "allow"}}),
    )
    .await;
    assert_eq!(assign(&app, &rh, &other, &auditor).await.0, 200);
}

/// «Requer aprovação» cria um pedido e não executa; a aprovação serve uma vez,
/// só para o mesmo alvo, não pelo próprio, e cai se o papel mudar.
#[sqlx::test(migrations = "./migrations")]
async fn requires_approval_flow(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let owner = app.new_org("aprovacao.ao").await;
    let org = owner.org().to_string();
    let p = app.add_member(&owner, "pede", "member").await;
    let role = create_role(
        &app,
        &owner,
        json!({"name": "Guardião", "capabilities": {
        "admin.change_retention": "requires_approval", "admin.manage_roles": "allow"}}),
    )
    .await;
    assert_eq!(assign(&app, &owner, &p, &role).await.0, 200);

    let body = json!({"domain": "", "retention_days": 30});
    let (st, r1) = app
        .patch(&format!("/api/orgs/{org}"), Some(&p.token), body.clone())
        .await;
    assert_eq!(
        (st, r1["code"].as_str()),
        (403, Some("authz.approval_required")),
        "{r1}"
    );
    let req_id = r1["details"][0]["description"]
        .as_str()
        .unwrap()
        .to_string();
    let (retention,): (i32,) =
        sqlx::query_as("SELECT retention_days FROM organizations WHERE id = $1::uuid")
            .bind(&org)
            .fetch_one(&app.db)
            .await
            .unwrap();
    assert_eq!(retention, 0, "não executou");
    let (_, r2) = app
        .patch(&format!("/api/orgs/{org}"), Some(&p.token), body.clone())
        .await;
    assert_eq!(
        r2["details"][0]["description"],
        req_id.as_str(),
        "reutiliza o pendente"
    );

    let approve = format!("/api/orgs/{org}/approval-requests/{req_id}/approve");
    let (st, b) = app.post(&approve, Some(&p.token), json!({})).await;
    assert_eq!(
        (st, b["code"].as_str()),
        (403, Some("approval.self_approval")),
        "{b}"
    );
    let (st, b) = app.post(&approve, Some(&owner.token), json!({})).await;
    assert_eq!((st, b["status"].as_str()), (200, Some("approved")), "{b}");

    // Outro alvo não consome a aprovação.
    let (st, b) = app
        .patch(
            &format!("/api/orgs/{org}"),
            Some(&p.token),
            json!({"domain": "", "retention_days": 31}),
        )
        .await;
    assert_eq!(
        (st, b["code"].as_str()),
        (403, Some("authz.approval_required")),
        "{b}"
    );
    // O mesmo alvo consome-a e executa, uma vez.
    let (st, b) = app
        .patch(&format!("/api/orgs/{org}"), Some(&p.token), body.clone())
        .await;
    assert_eq!(st, 200, "{b}");
    let (st, b) = app
        .patch(&format!("/api/orgs/{org}"), Some(&p.token), body.clone())
        .await;
    assert_eq!(
        (st, b["code"].as_str()),
        (403, Some("authz.approval_required")),
        "uso único: {b}"
    );
    let pending = b["details"][0]["description"].as_str().unwrap().to_string();

    // Mudar o papel de quem pediu invalida o pendente.
    let member_role = role_id(&app, &owner, "member").await;
    assert_eq!(assign(&app, &owner, &p, &member_role).await.0, 200);
    let (_, one) = app
        .get(
            &format!("/api/orgs/{org}/approval-requests/{pending}"),
            Some(&owner.token),
        )
        .await;
    assert_eq!(one["status"], "invalidated", "{one}");
    let (st, _) = app
        .post(
            &format!("/api/orgs/{org}/approval-requests/{pending}/approve"),
            Some(&owner.token),
            json!({}),
        )
        .await;
    assert_eq!(st, 409);

    // Recusar exige razão; a auditoria regista pedir, aprovar e consumir.
    let actions: Vec<String> = sqlx::query_scalar(
        "SELECT action FROM audit_logs WHERE org_id = $1::uuid AND action LIKE 'approval.%'",
    )
    .bind(&org)
    .fetch_all(&app.db)
    .await
    .unwrap();
    for a in [
        "approval.requested",
        "approval.approved",
        "approval.consumed",
    ] {
        assert!(actions.iter().any(|x| x == a), "{a} em {actions:?}");
    }
}

/// Consumo atómico: duas repetições concorrentes com uma aprovação → uma só passa.
#[sqlx::test(migrations = "./migrations")]
async fn approval_is_consumed_once_under_concurrency(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let owner = app.new_org("atomico.ao").await;
    let org = owner.org().to_string();
    let p = app.add_member(&owner, "pede", "member").await;
    let role = create_role(
        &app,
        &owner,
        json!({"name": "Pede", "capabilities": {"admin.change_retention": "requires_approval"}}),
    )
    .await;
    assert_eq!(assign(&app, &owner, &p, &role).await.0, 200);
    let body = json!({"domain": "", "retention_days": 7});
    let (_, r) = app
        .patch(&format!("/api/orgs/{org}"), Some(&p.token), body.clone())
        .await;
    let id = r["details"][0]["description"].as_str().unwrap().to_string();
    let (st, _) = app
        .post(
            &format!("/api/orgs/{org}/approval-requests/{id}/approve"),
            Some(&owner.token),
            json!({}),
        )
        .await;
    assert_eq!(st, 200);
    let path = format!("/api/orgs/{org}");
    let (a, b) = tokio::join!(
        app.patch(&path, Some(&p.token), body.clone()),
        app.patch(&path, Some(&p.token), body.clone())
    );
    let ok = [a.0, b.0].iter().filter(|s| **s == 200).count();
    assert_eq!(ok, 1, "{a:?} {b:?}");
}

/// Âmbito de departamento: um papel de departamento gere só o seu.
#[sqlx::test(migrations = "./migrations")]
async fn department_scoped_role(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let owner = app.new_org("ambito.ao").await;
    let org = owner.org().to_string();
    let (_, com) = app
        .post(
            &format!("/api/orgs/{org}/departments"),
            Some(&owner.token),
            json!({"name": "Comunicação"}),
        )
        .await;
    let (_, rh) = app
        .post(
            &format!("/api/orgs/{org}/departments"),
            Some(&owner.token),
            json!({"name": "Recursos Humanos"}),
        )
        .await;
    let com = com["id"].as_str().unwrap().to_string();
    let rh = rh["id"].as_str().unwrap().to_string();
    let teresa = app.add_member(&owner, "teresa", "member").await;
    let paulo = app.add_member(&owner, "paulo", "member").await;
    let luisa = app.add_member(&owner, "luisa", "member").await;
    for (u, d) in [(&teresa, &com), (&paulo, &com), (&luisa, &rh)] {
        let (st, b) = app
            .post(
                &format!("/api/orgs/{org}/users/bulk-actions"),
                Some(&owner.token),
                json!({"action": "change_department", "user_ids": [u.user_id], "department_id": d}),
            )
            .await;
        assert_eq!((st, b["succeeded"].as_u64()), (200, Some(1)), "{b}");
    }
    let gestor = create_role(
        &app,
        &owner,
        json!({"name": "Gestor de emissão",
        "scope": {"kind": "department", "department_id": com},
        "capabilities": {"admin.manage_accounts": "allow"}}),
    )
    .await;
    assert_eq!(assign(&app, &owner, &teresa, &gestor).await.0, 200);

    let (st, list) = app
        .get(&format!("/api/orgs/{org}/users"), Some(&teresa.token))
        .await;
    assert_eq!(st, 200, "{list}");
    assert_eq!(list["restricted_to_department"], com.as_str());
    assert!(
        list["items"]
            .as_array()
            .unwrap()
            .iter()
            .all(|e| e["department_id"] == com.as_str()),
        "{list}"
    );
    // Dentro do departamento: sim. Fora: não.
    let (st, b) = app
        .post(
            &format!("/api/orgs/{org}/users/bulk-actions"),
            Some(&teresa.token),
            json!({"action": "suspend", "user_ids": [paulo.user_id, luisa.user_id]}),
        )
        .await;
    assert_eq!(st, 200, "{b}");
    let res = b["results"].as_array().unwrap();
    let by = |u: &Account| {
        res.iter()
            .find(|r| r["id"] == u.user_id.as_str())
            .unwrap()
            .clone()
    };
    assert_eq!(by(&paulo)["ok"], true, "{b}");
    assert_eq!(by(&luisa)["code"], "authz.missing_capability", "{b}");
    // Um recurso da organização não está no âmbito: a auditoria continua fechada.
    let (st, _) = app
        .get(
            &format!("/api/orgs/{org}/audit-events"),
            Some(&teresa.token),
        )
        .await;
    assert_eq!(st, 403);
    // A simulação explica porquê, só de leitura.
    let (st, ev) = app.post(&format!("/api/orgs/{org}/authorization/evaluations"), Some(&owner.token),
        json!({"user_id": teresa.user_id, "capability": "admin.manage_accounts", "department_id": rh})).await;
    assert_eq!(st, 200, "{ev}");
    assert_eq!(ev["items"][0]["decision"], "deny");
    assert_eq!(ev["items"][0]["reason"], "out_of_scope_member");
    let (st, _) = app
        .post(
            &format!("/api/orgs/{org}/authorization/evaluations"),
            Some(&teresa.token),
            json!({"user_id": owner.user_id}),
        )
        .await;
    assert_eq!(st, 403, "simular exige admin.manage_roles");
}

/// A tabela materializada (o que a imposição lê) é igual à policy chamada
/// directamente (a simulação), para todas as capacidades de papéis herdados.
#[sqlx::test(migrations = "./migrations")]
async fn materialized_decisions_equal_policy(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let owner = app.new_org("equivalencia.ao").await;
    let org = owner.org().to_string();
    let base = create_role(&app, &owner, json!({"name": "Base", "capabilities": {
        "admin.view_audit": "allow", "recordings.publish": "requires_approval", "sessions.create": "deny"}})).await;
    let child = create_role(
        &app,
        &owner,
        json!({"name": "Filho", "inherits_from": base,
        "capabilities": {"admin.view_audit": "deny"}}),
    )
    .await;
    let people = [
        app.add_member(&owner, "p1", "member").await,
        app.add_member(&owner, "p2", "member").await,
    ];
    assert_eq!(assign(&app, &owner, &people[0], &base).await.0, 200);
    assert_eq!(assign(&app, &owner, &people[1], &child).await.0, 200);
    // Mudar o pai muda o filho (recalcula a org inteira).
    let (st, _) = app
        .put(
            &format!("/api/orgs/{org}/roles/{base}/capabilities"),
            Some(&owner.token),
            json!({"values": {"admin.view_audit": "allow", "broadcast.manage_rtmp_keys": "allow"}}),
        )
        .await;
    assert_eq!(st, 200);
    for p in &people {
        let (_, mine) = app
            .get(
                &format!("/api/orgs/{org}/members/me/capabilities"),
                Some(&p.token),
            )
            .await;
        let (_, ev) = app
            .post(
                &format!("/api/orgs/{org}/authorization/evaluations"),
                Some(&owner.token),
                json!({"user_id": p.user_id}),
            )
            .await;
        for item in ev["items"].as_array().unwrap() {
            let cap = item["capability"].as_str().unwrap();
            let row = mine["items"]
                .as_array()
                .unwrap()
                .iter()
                .find(|m| m["capability"] == cap)
                .unwrap();
            assert_eq!(
                row["organization"], item["decision"],
                "{cap} para {}",
                p.email
            );
        }
    }
    let (_, ev) = app
        .post(
            &format!("/api/orgs/{org}/authorization/evaluations"),
            Some(&owner.token),
            json!({"user_id": people[1].user_id, "capability": "broadcast.manage_rtmp_keys"}),
        )
        .await;
    assert_eq!(
        (
            ev["items"][0]["decision"].as_str(),
            ev["items"][0]["reason"].as_str()
        ),
        (Some("allow"), Some("inherited")),
        "{ev}"
    );
    // Com um filho, o pai não se apaga.
    let (st, b) = app
        .delete(&format!("/api/orgs/{org}/roles/{base}"), Some(&owner.token))
        .await;
    assert_eq!(
        (st, b["code"].as_str()),
        (409, Some("role.has_children")),
        "{b}"
    );
}

/// Eliminar exige reatribuir; duplicar; exportar CSV.
#[sqlx::test(migrations = "./migrations")]
async fn delete_duplicate_and_export(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let owner = app.new_org("eliminar.ao").await;
    let org = owner.org().to_string();
    let m = app.add_member(&owner, "mod", "member").await;
    let modr = create_role(
        &app,
        &owner,
        json!({"name": "Moderador", "capabilities": {"recordings.publish": "allow"}}),
    )
    .await;
    assert_eq!(assign(&app, &owner, &m, &modr).await.0, 200);
    let (st, b) = app
        .delete(&format!("/api/orgs/{org}/roles/{modr}"), Some(&owner.token))
        .await;
    assert_eq!(
        (st, b["code"].as_str()),
        (422, Some("role.reassignment_required")),
        "{b}"
    );
    let (st, dup) = app
        .post(
            &format!("/api/orgs/{org}/roles/{modr}/duplicate"),
            Some(&owner.token),
            json!({}),
        )
        .await;
    assert_eq!(st, 201, "{dup}");
    assert_eq!(dup["name"], "Moderador (cópia)");
    let dup_id = dup["id"].as_str().unwrap();
    let (st, _) = app
        .delete(
            &format!("/api/orgs/{org}/roles/{modr}?reassign_to={dup_id}"),
            Some(&owner.token),
        )
        .await;
    assert_eq!(st, 204);
    let (_, mine) = app
        .get(
            &format!("/api/orgs/{org}/members/me/capabilities"),
            Some(&m.token),
        )
        .await;
    assert_eq!(mine["role_id"], dup_id);
    let r = app
        .raw(
            reqwest::Method::GET,
            &format!("/api/orgs/{org}/permission-matrix?format=csv"),
            &[("authorization", &format!("Bearer {}", owner.token))],
            None,
        )
        .await;
    assert_eq!(r.status, 200);
    assert!(r.header("content-type").unwrap().starts_with("text/csv"));
    assert!(
        r.text.contains("admin.manage_roles") && r.text.contains("Moderador (cópia)"),
        "{}",
        r.text
    );
    let (st, mx) = app
        .get(
            &format!("/api/orgs/{org}/permission-matrix"),
            Some(&owner.token),
        )
        .await;
    assert_eq!(st, 200);
    assert_eq!(mx["rows"].as_array().unwrap().len(), Capability::ALL.len());
    let actions: Vec<String> = sqlx::query_scalar(
        "SELECT action FROM audit_logs WHERE org_id = $1::uuid AND action LIKE 'role.%'",
    )
    .bind(&org)
    .fetch_all(&app.db)
    .await
    .unwrap();
    for a in [
        "role.created",
        "role.duplicated",
        "role.deleted",
        "role.matrix_exported",
    ] {
        assert!(actions.iter().any(|x| x == a), "{a} em {actions:?}");
    }
}

/// Segregação de funções: violações, filtro por papel, aceitar risco.
#[sqlx::test(migrations = "./migrations")]
async fn segregation_of_duties(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let owner = app.new_org("sod.ao").await;
    let org = owner.org().to_string();
    let admin = app.add_member(&owner, "adm", "admin").await;
    let _m = app.add_member(&owner, "mem", "member").await;
    let (st, rule) = app.post(&format!("/api/orgs/{org}/sod-rules"), Some(&owner.token),
        json!({"name": "Emitir e ver auditoria", "capabilities": ["broadcast.public_destinations", "admin.view_audit"]})).await;
    assert_eq!(st, 201, "{rule}");
    let rule_id = rule["id"].as_str().unwrap();
    let (_, v) = app
        .get(
            &format!("/api/orgs/{org}/sod-violations"),
            Some(&owner.token),
        )
        .await;
    assert_eq!(v["total"], 1, "o dono é isento; o admin viola: {v}");
    assert_eq!(v["items"][0]["user_id"], admin.user_id.as_str());
    let (st, b) = app
        .post(
            &format!("/api/orgs/{org}/sod-rules/{rule_id}/risk-acceptances"),
            Some(&owner.token),
            json!({"user_id": admin.user_id, "justification": "curta"}),
        )
        .await;
    assert_eq!(
        (st, b["code"].as_str()),
        (400, Some("sod.justification_required"))
    );
    let (st, _) = app.post(&format!("/api/orgs/{org}/sod-rules/{rule_id}/risk-acceptances"), Some(&owner.token),
        json!({"user_id": admin.user_id, "justification": "Administrador único de TI, revisto trimestralmente"})).await;
    assert_eq!(st, 201);
    let (_, v) = app
        .get(
            &format!("/api/orgs/{org}/sod-violations"),
            Some(&owner.token),
        )
        .await;
    assert_eq!(
        (v["total"].as_u64(), v["unaccepted"].as_u64()),
        (Some(1), Some(0)),
        "{v}"
    );
    let (st, b) = app
        .post(
            &format!("/api/orgs/{org}/sod-rules"),
            Some(&owner.token),
            json!({"name": "Uma só", "capabilities": ["admin.view_audit"]}),
        )
        .await;
    assert_eq!(
        (st, b["code"].as_str()),
        (400, Some("sod.too_few_capabilities"))
    );
    // Um papel que viola dá aviso ao gravar, não recusa.
    let (st, r) = app
        .post(
            &format!("/api/orgs/{org}/roles"),
            Some(&owner.token),
            json!({"name": "Tudo",
        "capabilities": {"broadcast.public_destinations": "allow", "admin.view_audit": "allow"}}),
        )
        .await;
    assert_eq!(st, 201);
    assert_eq!(r["warnings"][0]["rule_id"], rule_id);
}

/// Publicar gravações de colegas: o dono sempre; o admin da org do dono passa a
/// poder (alargamento intencional); um membro não (401 herdado).
#[sqlx::test(migrations = "./migrations")]
async fn recordings_publish_and_view_others(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let owner = app.new_org("gravacoes-rbac.ao").await;
    let admin = app.add_member(&owner, "adm", "admin").await;
    let uploader = app.add_member(&owner, "grava", "member").await;
    let member = app.add_member(&owner, "mem", "member").await;
    let other = app.new_org("fora-gravacoes.ao").await;
    let room = app.new_room(&uploader, "sala").await;
    let rec = app
        .insert_recording(room["id"].as_str().unwrap(), &uploader.user_id)
        .await;
    let link = format!("/api/recordings/{rec}/public-link");
    for (who, acc, want) in [
        ("dono", &uploader, 200),
        ("admin", &admin, 200),
        ("membro", &member, 401),
        ("outra org", &other, 401),
    ] {
        let (st, b) = app.put(&link, Some(&acc.token), json!({})).await;
        assert_eq!(st, want, "{who}: {b}");
    }
    // Ver/descarregar de outros: admin sim (antes e depois), membro não.
    let content = format!("/api/recordings/{rec}/content?dl=1");
    let (st, _) = app.get(&content, Some(&admin.token)).await;
    assert_eq!(
        st, 404,
        "admin passa a autorização e chega à leitura do ficheiro (inexistente)"
    );
    let (st, _) = app.get(&content, Some(&member.token)).await;
    assert_eq!(st, 401, "membro recusado antes do ficheiro");
    // Num papel personalizado com `recordings.view_others`, o facto liga-se.
    let viewer = create_role(
        &app,
        &owner,
        json!({"name": "Leitor", "capabilities": {"recordings.view_others": "allow"}}),
    )
    .await;
    assert_eq!(assign(&app, &owner, &member, &viewer).await.0, 200);
    let (st, meta) = app
        .get(&format!("/api/recordings/{rec}"), Some(&member.token))
        .await;
    assert_eq!(st, 200, "{meta}");
    assert_eq!(meta["can_manage"], true, "{meta}");
}

/// `sessions.create`: o convidado externo não cria salas.
#[sqlx::test(migrations = "./migrations")]
async fn sessions_create_is_enforced(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let owner = app.new_org("sessoes.ao").await;
    let guest = app.add_member(&owner, "convidado", "member").await;
    let guest_role = role_id(&app, &owner, "external_guest").await;
    assert_eq!(assign(&app, &owner, &guest, &guest_role).await.0, 200);
    let (st, b) = app
        .post("/api/rooms", Some(&guest.token), json!({"name": "x"}))
        .await;
    assert_eq!(
        (st, b["code"].as_str()),
        (403, Some("authz.missing_capability")),
        "{b}"
    );
    let starts = (chrono::Utc::now() + chrono::Duration::hours(1)).to_rfc3339();
    let (st, _) = app
        .post(
            "/api/meetings",
            Some(&guest.token),
            json!({"title": "t", "kind": "video", "starts_at": starts, "duration_min": 30}),
        )
        .await;
    assert_eq!(st, 403);
}

/// Isolamento: cada rota nova, com um id da org B, pedida pela org A → 404.
#[sqlx::test(migrations = "./migrations")]
async fn new_routes_are_isolated(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let a = app.new_org("iso-a.ao").await;
    let b = app.new_org("iso-b.ao").await;
    let ob = b.org().to_string();
    let rb = role_id(&app, &b, "member").await;
    let custom_b = create_role(&app, &b, json!({"name": "Só B"})).await;
    let (_, dept) = app
        .post(
            &format!("/api/orgs/{ob}/departments"),
            Some(&b.token),
            json!({"name": "D"}),
        )
        .await;
    let dept_b = dept["id"].as_str().unwrap().to_string();
    let (_, inv) = app
        .post(
            &format!("/api/orgs/{ob}/invitations"),
            Some(&b.token),
            json!({"email": "x@iso-b.ao", "role_id": rb}),
        )
        .await;
    let inv_b = inv["id"].as_str().unwrap().to_string();
    let (_, rule) = app
        .post(
            &format!("/api/orgs/{ob}/sod-rules"),
            Some(&b.token),
            json!({"name": "r", "capabilities": ["admin.view_audit", "admin.manage_roles"]}),
        )
        .await;
    let rule_b = rule["id"].as_str().unwrap().to_string();
    let gets = [
        format!("/api/orgs/{ob}/roles"),
        format!("/api/orgs/{ob}/roles/{custom_b}"),
        format!("/api/orgs/{ob}/roles/{custom_b}/capabilities"),
        format!("/api/orgs/{ob}/permission-matrix"),
        format!("/api/orgs/{ob}/members/me/capabilities"),
        format!("/api/orgs/{ob}/sod-rules"),
        format!("/api/orgs/{ob}/sod-rules/{rule_b}"),
        format!("/api/orgs/{ob}/sod-violations"),
        format!("/api/orgs/{ob}/role-conflicts"),
        format!("/api/orgs/{ob}/approval-requests"),
        format!("/api/orgs/{ob}/approval-requests/{INVENTED_ID}"),
        format!("/api/orgs/{ob}/users"),
        format!("/api/orgs/{ob}/invitations"),
        format!("/api/orgs/{ob}/invitations/{inv_b}"),
        format!("/api/orgs/{ob}/departments"),
        format!("/api/orgs/{ob}/departments/{dept_b}"),
        format!("/api/orgs/{ob}/seats"),
        format!("/api/orgs/{ob}/provisioning"),
        format!("/api/orgs/{ob}/entry-rules"),
    ];
    for g in &gets {
        let (st, body) = app.get(g, Some(&a.token)).await;
        assert_eq!(st, 404, "A lê {g}: {body}");
    }
    let writes: Vec<(reqwest::Method, String, Value)> = vec![
        (
            reqwest::Method::POST,
            format!("/api/orgs/{ob}/roles"),
            json!({"name": "x"}),
        ),
        (
            reqwest::Method::PATCH,
            format!("/api/orgs/{ob}/roles/{custom_b}"),
            json!({"name": "y"}),
        ),
        (
            reqwest::Method::DELETE,
            format!("/api/orgs/{ob}/roles/{custom_b}"),
            Value::Null,
        ),
        (
            reqwest::Method::POST,
            format!("/api/orgs/{ob}/roles/{custom_b}/duplicate"),
            json!({}),
        ),
        (
            reqwest::Method::PUT,
            format!("/api/orgs/{ob}/roles/{custom_b}/capabilities"),
            json!({"values": {}}),
        ),
        (
            reqwest::Method::POST,
            format!("/api/orgs/{ob}/authorization/evaluations"),
            json!({"user_id": b.user_id}),
        ),
        (
            reqwest::Method::PUT,
            format!("/api/orgs/{ob}/members/{}/role", b.user_id),
            json!({"role_id": rb}),
        ),
        (
            reqwest::Method::POST,
            format!("/api/orgs/{ob}/sod-rules"),
            json!({"name": "z", "capabilities": ["admin.view_audit", "admin.manage_roles"]}),
        ),
        (
            reqwest::Method::PATCH,
            format!("/api/orgs/{ob}/sod-rules/{rule_b}"),
            json!({"name": "w"}),
        ),
        (
            reqwest::Method::DELETE,
            format!("/api/orgs/{ob}/sod-rules/{rule_b}"),
            Value::Null,
        ),
        (
            reqwest::Method::POST,
            format!("/api/orgs/{ob}/sod-rules/{rule_b}/risk-acceptances"),
            json!({"user_id": b.user_id, "justification": "0123456789ab"}),
        ),
        (
            reqwest::Method::POST,
            format!("/api/orgs/{ob}/role-conflicts/{INVENTED_ID}/resolve"),
            json!({"decision": "keep_current"}),
        ),
        (
            reqwest::Method::POST,
            format!("/api/orgs/{ob}/approval-requests/{INVENTED_ID}/approve"),
            json!({}),
        ),
        (
            reqwest::Method::POST,
            format!("/api/orgs/{ob}/approval-requests/{INVENTED_ID}/reject"),
            json!({"reason": "x"}),
        ),
        (
            reqwest::Method::POST,
            format!("/api/orgs/{ob}/users/bulk-actions"),
            json!({"action": "suspend", "user_ids": [b.user_id]}),
        ),
        (
            reqwest::Method::POST,
            format!("/api/orgs/{ob}/users/imports"),
            json!({"csv": "email\nx@iso-b.ao"}),
        ),
        (
            reqwest::Method::POST,
            format!("/api/orgs/{ob}/invitations"),
            json!({"email": "y@iso-b.ao", "role_id": rb}),
        ),
        (
            reqwest::Method::DELETE,
            format!("/api/orgs/{ob}/invitations/{inv_b}"),
            Value::Null,
        ),
        (
            reqwest::Method::POST,
            format!("/api/orgs/{ob}/invitations/{inv_b}/resend"),
            json!({}),
        ),
        (
            reqwest::Method::POST,
            format!("/api/orgs/{ob}/departments"),
            json!({"name": "E"}),
        ),
        (
            reqwest::Method::PATCH,
            format!("/api/orgs/{ob}/departments/{dept_b}"),
            json!({"name": "F"}),
        ),
        (
            reqwest::Method::DELETE,
            format!("/api/orgs/{ob}/departments/{dept_b}"),
            Value::Null,
        ),
        (
            reqwest::Method::POST,
            format!("/api/orgs/{ob}/seats/release"),
            json!({"inactive_days": 60}),
        ),
        (
            reqwest::Method::PUT,
            format!("/api/orgs/{ob}/entry-rules"),
            json!({"create_account_on_first_login": false, "approved_domains": [], "suspend_on_odoo_exit": false, "external_guest_ttl_hours": 24}),
        ),
    ];
    for (m, path, body) in writes {
        let body = if body.is_null() { None } else { Some(body) };
        let (st, resp) = app.call(m.clone(), &path, Some(&a.token), body).await;
        assert_eq!(st, 404, "A escreve {m} {path}: {resp}");
    }
    // Nada mudou em B.
    let (_, r) = app
        .get(&format!("/api/orgs/{ob}/roles/{custom_b}"), Some(&b.token))
        .await;
    assert_eq!(r["name"], "Só B");
    let (_, i) = app
        .get(
            &format!("/api/orgs/{ob}/invitations/{inv_b}"),
            Some(&b.token),
        )
        .await;
    assert_eq!(i["status"], "pending");
}
