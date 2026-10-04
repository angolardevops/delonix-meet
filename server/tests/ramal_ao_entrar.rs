//! R278 — ramal automático quando entra um membro. Contra Postgres real.
//!
//! A definição é por organização e nasce desligada. Com ela ligada, cada
//! caminho de entrada coberto aqui deixa o membro novo com um ramal do
//! intervalo; desligada, não; e o intervalo esgotado nunca parte a entrada.
//!
//! Recebe ramal quem entra por um ACTO de um administrador ou de um IdP.
//! Caminhos medidos: o administrador junta um colaborador (`POST …/members`) e
//! o convite aceite (`POST /api/invitations/accept`). O SSO (OIDC JIT e Odoo) e
//! a reactivação chamam a MESMA função mas não têm teste aqui.
//!
//! NÃO recebe, e está medido: quem se regista sozinho (registo aberto numa
//! instalação de organização única — email não verificado) e o convidado de
//! uma reunião criada pela API v1 (um endereço que pode nem existir).
mod common;

use common::{Account, TestApp};
use serde_json::{json, Value};

async fn definir(app: &TestApp, admin: &Account, start: u32, end: u32, auto: bool) {
    let (st, body) = app
        .put(
            &format!("/api/orgs/{}/extension-range", admin.org()),
            Some(&admin.token),
            json!({"range_start": start, "range_end": end, "auto_assign_on_join": auto}),
        )
        .await;
    assert_eq!(st, 200, "{body}");
    assert_eq!(body["auto_assign_on_join"], auto);
}

/// O número do ramal de uma pessoa nesta organização, se tiver.
async fn ramal_de(app: &TestApp, org: &str, user_id: &str) -> Option<String> {
    sqlx::query_scalar(
        "SELECT extension FROM voice_extensions WHERE org_id = $1::uuid AND member_id = $2::uuid",
    )
    .bind(org)
    .bind(user_id)
    .fetch_optional(&app.db)
    .await
    .unwrap()
}

async fn auditoria(app: &TestApp, org: &str, action: &str) -> Vec<(String, String)> {
    sqlx::query_as(
        "SELECT actor_id::text, target FROM audit_logs WHERE org_id = $1::uuid AND action = $2 ORDER BY seq",
    )
    .bind(org)
    .bind(action)
    .fetch_all(&app.db)
    .await
    .unwrap()
}

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

#[sqlx::test(migrations = "./migrations")]
async fn desligada_por_omissao_ligada_da_ramal_a_quem_o_admin_junta(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let a = app.new_org("alfa-auto.ao").await;

    // Nasce desligada: a leitura di-lo, e quem entra não recebe ramal.
    let (st, range) = app
        .get(
            &format!("/api/orgs/{}/extension-range", a.org()),
            Some(&a.token),
        )
        .await;
    assert_eq!(st, 200, "{range}");
    assert_eq!(range["auto_assign_on_join"], false);
    let antes = app.add_member(&a, "antes", "member").await;
    assert_eq!(ramal_de(&app, a.org(), &antes.user_id).await, None);

    // Um PUT só com o intervalo (cliente antigo) não a liga.
    let (st, body) = app
        .put(
            &format!("/api/orgs/{}/extension-range", a.org()),
            Some(&a.token),
            json!({"range_start": 1000, "range_end": 1999}),
        )
        .await;
    assert_eq!(st, 200, "{body}");
    assert_eq!(body["auto_assign_on_join"], false);

    // Ligada: quem entra recebe o primeiro número livre, sem PIN.
    definir(&app, &a, 2000, 2010, true).await;
    let ana = app.add_member(&a, "ana", "member").await;
    assert_eq!(
        ramal_de(&app, a.org(), &ana.user_id).await.as_deref(),
        Some("2000")
    );
    let rui = app.add_member(&a, "rui", "admin").await;
    assert_eq!(
        ramal_de(&app, a.org(), &rui.user_id).await.as_deref(),
        Some("2001")
    );
    let (st, meu) = app
        .get(
            &format!("/api/orgs/{}/my-extension", a.org()),
            Some(&ana.token),
        )
        .await;
    assert_eq!(st, 200, "{meu}");
    assert_eq!(meu["extension"], "2000");
    assert_eq!(meu["pin_state"], "unset");

    // Um PUT só com o intervalo (cliente que não conhece o campo) MANTÉM-na
    // ligada (N4): ausente não é «desligar».
    let (st, body) = app
        .put(
            &format!("/api/orgs/{}/extension-range", a.org()),
            Some(&a.token),
            json!({"range_start": 2000, "range_end": 2010}),
        )
        .await;
    assert_eq!(st, 200, "{body}");
    assert_eq!(body["auto_assign_on_join"], true, "{body}");
    let (_, lido) = app
        .get(
            &format!("/api/orgs/{}/extension-range", a.org()),
            Some(&a.token),
        )
        .await;
    assert_eq!(lido["auto_assign_on_join"], true, "{lido}");

    // Ligar não dá ramal a quem já cá estava: isso é o «atribuir a todos».
    assert_eq!(ramal_de(&app, a.org(), &antes.user_id).await, None);
    assert_eq!(ramal_de(&app, a.org(), &a.user_id).await, None);

    // Voltar a juntar a mesma pessoa não cria segundo ramal.
    let (st, _) = app
        .post(
            &format!("/api/orgs/{}/members", a.org()),
            Some(&a.token),
            json!({"email": ana.email, "title": "outra vez"}),
        )
        .await;
    assert_eq!(st, 200);
    let n: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM voice_extensions WHERE org_id = $1::uuid")
            .bind(a.org())
            .fetch_one(&app.db)
            .await
            .unwrap();
    assert_eq!(n, 2);

    // Fica na auditoria, com o actor de sistema.
    let log = auditoria(&app, a.org(), "ramal.atribuido_ao_entrar").await;
    assert_eq!(log.len(), 2, "{log:?}");
    assert_eq!(log[0].0, uuid::Uuid::nil().to_string());
    assert_eq!(log[0].1, format!("2000 → {}", ana.user_id));

    // Desligada de novo: o seguinte entra sem ramal.
    definir(&app, &a, 2000, 2010, false).await;
    let eva = app.add_member(&a, "eva", "member").await;
    assert_eq!(ramal_de(&app, a.org(), &eva.user_id).await, None);

    // A definição é da organização: ligada na A, a B não é tocada.
    definir(&app, &a, 2000, 2010, true).await;
    let b = app.new_org("beta-auto.ao").await;
    let bia = app.add_member(&b, "bia", "member").await;
    assert_eq!(ramal_de(&app, b.org(), &bia.user_id).await, None);
}

#[sqlx::test(migrations = "./migrations")]
async fn intervalo_esgotado_nao_parte_a_entrada(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let a = app.new_org("alfa-cheio.ao").await;
    // Um intervalo de dois números, um deles já ocupado por um ramal da empresa.
    definir(&app, &a, 3000, 3001, true).await;
    let (st, body) = app
        .post(
            &format!("/api/orgs/{}/extensions", a.org()),
            Some(&a.token),
            json!({"extension": "3000", "label": "Recepção"}),
        )
        .await;
    assert_eq!(st, 200, "{body}");

    let um = app.add_member(&a, "um", "member").await;
    assert_eq!(
        ramal_de(&app, a.org(), &um.user_id).await.as_deref(),
        Some("3001")
    );

    // Esgotado: a pessoa ENTRA (o `add_member` exige 200 e faz login), sem ramal.
    let dois = app.add_member(&a, "dois", "member").await;
    assert_eq!(ramal_de(&app, a.org(), &dois.user_id).await, None);
    let (st, orgs) = app.get("/api/orgs", Some(&dois.token)).await;
    assert_eq!(st, 200);
    assert_eq!(orgs[0]["id"], json!(a.org()), "{orgs}");

    // E fica registado.
    let log = auditoria(&app, a.org(), "ramal.atribuicao_automatica_falhou").await;
    assert_eq!(
        log,
        vec![(
            uuid::Uuid::nil().to_string(),
            format!("intervalo esgotado → {}", dois.user_id)
        )]
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn convite_aceite_da_ramal_e_convidado_externo_nao(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let a = app.new_org("alfa-convite.ao").await;
    let member = role_id(&app, &a, "member").await;
    let guest = role_id(&app, &a, "external_guest").await;

    // Alguém que saiu (com a definição desligada: nunca teve ramal)…
    let ex = app.add_member(&a, "ex", "member").await;
    let (st, _) = app
        .delete(
            &format!("/api/orgs/{}/members/{}", a.org(), ex.user_id),
            Some(&a.token),
        )
        .await;
    assert_eq!(st, 204);
    definir(&app, &a, 4000, 4010, true).await;

    // … volta por convite e recebe ramal.
    let (st, inv) = app
        .post(
            &format!("/api/orgs/{}/invitations", a.org()),
            Some(&a.token),
            json!({"email": ex.email, "role_id": member}),
        )
        .await;
    assert_eq!(st, 201, "{inv}");
    let (st, ok) = app
        .post(
            "/api/invitations/accept",
            Some(&ex.token),
            json!({"token": inv["token"]}),
        )
        .await;
    assert_eq!(st, 200, "{ok}");
    assert_eq!(
        ramal_de(&app, a.org(), &ex.user_id).await.as_deref(),
        Some("4000")
    );

    // Um convidado externo (conta de outra organização) entra e NÃO recebe ramal.
    let fora = app.new_org("gama-convite.ao").await;
    let (st, inv) = app
        .post(
            &format!("/api/orgs/{}/invitations", a.org()),
            Some(&a.token),
            json!({"email": fora.email, "role_id": guest, "delivery": "code"}),
        )
        .await;
    assert_eq!(st, 201, "{inv}");
    let (st, ok) = app
        .post(
            "/api/invitations/accept",
            Some(&fora.token),
            json!({"token": inv["token"]}),
        )
        .await;
    assert_eq!(st, 200, "{ok}");
    assert_eq!(ramal_de(&app, a.org(), &fora.user_id).await, None);
    assert!(
        auditoria(&app, a.org(), "ramal.atribuicao_automatica_falhou")
            .await
            .is_empty()
    );
}

/// N8: a API v1 cria uma conta e uma pertença para cada convidado do domínio —
/// um endereço que pode nem existir. Não é uma entrada decidida por um
/// administrador: não dá ramal (nem custa um Argon2 por convidado).
#[sqlx::test(migrations = "./migrations")]
async fn convidado_de_reuniao_pela_api_v1_nao_recebe_ramal(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let a = app.new_org("alfa-v1.ao").await;
    definir(&app, &a, 5000, 5010, true).await;
    let (_, key) = app.api_key(&a).await;
    let starts = (chrono::Utc::now() + chrono::Duration::hours(2)).to_rfc3339();
    let r = app
        .raw(
            reqwest::Method::POST,
            "/api/v1/meetings",
            &[("Authorization", &format!("Bearer {key}"))],
            Some(json!({
                "external_ref": "teste:ramal-ao-entrar",
                "title": "Comité",
                "starts_at": starts,
                "duration_min": 30,
                "host_email": a.email,
                "invitees": [{"email": "novo@alfa-v1.ao", "name": "Novo Colega"}]
            })),
        )
        .await;
    assert_eq!(r.status, 200, "{}", r.text);
    let novo: Value = r.json();
    assert_eq!(novo["invitees"][0]["email"], "novo@alfa-v1.ao", "{novo}");
    // A pertença nasceu…
    let membro: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM org_members m JOIN users u ON u.id = m.user_id
                         WHERE m.org_id = $1::uuid AND u.email = 'novo@alfa-v1.ao')",
    )
    .bind(a.org())
    .fetch_one(&app.db)
    .await
    .unwrap();
    assert!(membro);
    // … e nenhum ramal com ela.
    let n: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM voice_extensions WHERE org_id = $1::uuid")
            .bind(a.org())
            .fetch_one(&app.db)
            .await
            .unwrap();
    assert_eq!(n, 0);
}

/// A1: instalação de organização única com registo ABERTO. Quem se regista
/// entra na organização sem acto de ninguém e sem email verificado: NÃO recebe
/// ramal, mesmo com a atribuição automática ligada — senão um desconhecido
/// ficava com uma conta SIP (e, repetindo, esgotava o intervalo).
#[sqlx::test(migrations = "./migrations")]
async fn quem_se_regista_sozinho_nao_recebe_ramal(db: sqlx::PgPool) {
    let app = TestApp::spawn_with(
        db,
        &[("TENANCY_MODE", "single"), ("REGISTRATION_MODE", "open")],
    )
    .await;
    // O primeiro registo cria a organização única e é o seu administrador.
    let a = app.new_org("unica.ao").await;
    definir(&app, &a, 6000, 6010, true).await;

    // Um desconhecido, com um email qualquer, regista-se.
    let (st, reg) = app
        .post(
            "/api/auth/register",
            None,
            json!({"email": "desconhecido@outro-dominio.test", "username": "desconhecido",
                   "password": common::PASSWORD}),
        )
        .await;
    assert!(st < 300, "{st} {reg}");
    let intruso = app.login("desconhecido@outro-dominio.test").await;
    // Entrou MESMO na organização única (é esta a pré-condição do ataque)…
    let (st, orgs) = app.get("/api/orgs", Some(&intruso.token)).await;
    assert_eq!(st, 200);
    assert_eq!(orgs[0]["id"], json!(a.org()), "{orgs}");
    // … e não tem ramal, nem bilhete para pedir.
    assert_eq!(ramal_de(&app, a.org(), &intruso.user_id).await, None);
    let (st, body) = app
        .post(
            &format!("/api/orgs/{}/my-extension/provisioning-ticket", a.org()),
            Some(&intruso.token),
            json!({}),
        )
        .await;
    assert_eq!(st, 404, "{body}");
    assert_eq!(body["code"], "ramais.no_extension");
    assert!(auditoria(&app, a.org(), "ramal.atribuido_ao_entrar")
        .await
        .is_empty());
}
