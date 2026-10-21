//! Reposição de password emitida por um administrador, contra Postgres e
//! servidor reais.
//!
//! PORQUE ESTE FICHEIRO EXISTE. Até esta funcionalidade, o único caminho para
//! mudar uma password era a própria pessoa com a password actual ou uma sessão
//! recente — quem a perdia ficava fora da conta para sempre. Repor a password
//! de alguém é TOMAR-LHE a conta, e por isso os testes que mais importam aqui
//! não são os do caminho felizes: são os três controlos negativos (escalada,
//! isolamento entre organizações, conta do Odoo) e a prova de que o token é
//! de uso único.
mod common;

use common::{Account, TestApp};
use serde_json::{json, Value};

const NOVA: &str = "Pa55-reposta-!x";

/// Emite uma reposição para `alvo` em nome de `admin`.
async fn emitir(app: &TestApp, admin: &Account, alvo: &Account) -> (u16, Value) {
    app.post(
        &format!(
            "/api/orgs/{}/users/{}/password-reset",
            admin.org(),
            alvo.user_id
        ),
        Some(&admin.token),
        json!({}),
    )
    .await
}

/// Usa uma reposição. SEM token de sessão — a rota é pública de propósito,
/// porque quem repõe a password está fora da conta.
async fn usar(app: &TestApp, token: &str, password: &str) -> (u16, Value) {
    app.post(
        "/api/password-resets/accept",
        None,
        json!({"token": token, "password": password}),
    )
    .await
}

async fn entra_com(app: &TestApp, email: &str, password: &str) -> u16 {
    app.post(
        "/api/auth/login",
        None,
        json!({"email": email, "password": password}),
    )
    .await
    .0
}

/// O caminho completo: emitir, usar sem sessão, e a password velha deixa de
/// servir. Prova também que o token aparece UMA vez e que as sessões caem.
#[sqlx::test(migrations = "./migrations")]
async fn reposicao_troca_a_password_e_derruba_as_sessoes(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let dono = app.new_org("repos.ao").await;
    let alvo = app.add_member(&dono, "bruno", "member").await;

    // O alvo tem uma sessão de pé: a reposição tem de a derrubar, porque quem
    // repõe pode estar a recuperar de um acesso indevido.
    assert_eq!(
        app.get("/api/users/me", Some(&alvo.token)).await.0,
        200,
        "a sessão do alvo devia estar válida antes da reposição"
    );

    let (st, emitida) = emitir(&app, &dono, &alvo).await;
    assert_eq!(st, 201, "emitir: {emitida}");
    let token = emitida["token"]
        .as_str()
        .expect("token na resposta")
        .to_string();
    assert!(token.starts_with("dlxr_"), "prefixo do token: {token}");
    assert_eq!(emitida["delivery_channel"], "manual");
    assert_eq!(emitida["user_id"], alvo.user_id);

    let (st, usada) = usar(&app, &token, NOVA).await;
    assert_eq!(st, 200, "usar: {usada}");
    assert!(
        usada["sessions_revoked"].as_u64().unwrap() >= 1,
        "a reposição tinha de derrubar a sessão do alvo: {usada}"
    );

    assert_eq!(
        entra_com(&app, &alvo.email, NOVA).await,
        200,
        "a password nova tinha de servir"
    );
    assert_eq!(
        app.get("/api/users/me", Some(&alvo.token)).await.0,
        401,
        "a sessão antiga do alvo tinha de cair"
    );
}

/// O token é de uso único, e emitir outro invalida o anterior. Sem isto, um
/// token entregue à mão ficava a servir indefinidamente.
#[sqlx::test(migrations = "./migrations")]
async fn token_e_de_uso_unico_e_emitir_outro_revoga_o_anterior(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let dono = app.new_org("unico.ao").await;
    let alvo = app.add_member(&dono, "carla", "member").await;

    let (_, primeira) = emitir(&app, &dono, &alvo).await;
    let t1 = primeira["token"].as_str().unwrap().to_string();
    let (_, segunda) = emitir(&app, &dono, &alvo).await;
    let t2 = segunda["token"].as_str().unwrap().to_string();
    assert_ne!(t1, t2, "duas emissões não podem dar o mesmo token");

    let (st, body) = usar(&app, &t1, NOVA).await;
    assert_eq!(st, 404, "o token anterior tinha de ficar revogado: {body}");
    assert_eq!(body["code"], "password_reset.not_found");

    let (st, body) = usar(&app, &t2, NOVA).await;
    assert_eq!(st, 200, "o token actual tinha de servir: {body}");

    // Segunda vez com o MESMO token: uso único.
    let (st, body) = usar(&app, &t2, "Outra-Pa55-!y").await;
    assert_eq!(st, 404, "o token não podia servir duas vezes: {body}");
}

/// CONTROLO NEGATIVO 1 — escalada. Um administrador NÃO pode repor a password
/// do dono da organização: repor é tomar a conta, e quem não pode nomear
/// aquele papel não pode tomar aquela conta. Sem esta guarda, qualquer
/// administrador tomava a organização.
#[sqlx::test(migrations = "./migrations")]
async fn administrador_nao_repoe_a_password_do_dono(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let dono = app.new_org("escalada.ao").await;
    let admin = app.add_member(&dono, "ana", "admin").await;

    let (st, body) = app
        .post(
            &format!(
                "/api/orgs/{}/users/{}/password-reset",
                dono.org(),
                dono.user_id
            ),
            Some(&admin.token),
            json!({}),
        )
        .await;
    assert_eq!(st, 403, "um admin não pode tomar a conta do dono: {body}");
    assert_eq!(body["code"], "role.owner_assignment");

    // E o dono continua a entrar com a password dele.
    assert_eq!(entra_com(&app, &dono.email, common::PASSWORD).await, 200);
}

/// CONTROLO NEGATIVO 2 — isolamento. Um administrador de outra organização não
/// repõe a password de alguém daqui, e a recusa não confirma que a conta
/// existe (`member.not_found`, o mesmo que daria um id inventado).
#[sqlx::test(migrations = "./migrations")]
async fn administrador_de_outra_org_nao_repoe(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let a = app.new_org("alfa-rp.ao").await;
    let alvo = app.add_member(&a, "diogo", "member").await;
    let b = app.new_org("beta-rp.ao").await;

    // B a apontar para a org de A: nem a org é dele.
    let (st, body) = app
        .post(
            &format!(
                "/api/orgs/{}/users/{}/password-reset",
                a.org(),
                alvo.user_id
            ),
            Some(&b.token),
            json!({}),
        )
        .await;
    common::assert_denied("B repõe na org de A", st, &body, &alvo.email);

    // B a apontar para a SUA org com um utilizador de A: o alvo não é membro.
    let (st, body) = app
        .post(
            &format!(
                "/api/orgs/{}/users/{}/password-reset",
                b.org(),
                alvo.user_id
            ),
            Some(&b.token),
            json!({}),
        )
        .await;
    assert_eq!(st, 404, "o alvo não é membro da org de B: {body}");
    assert_eq!(body["code"], "member.not_found");

    assert_eq!(
        entra_com(&app, &alvo.email, common::PASSWORD).await,
        200,
        "a password do alvo não podia ter mudado"
    );
}

/// CONTROLO NEGATIVO 3 — Odoo. A password de uma conta gerida pelo Odoo vive
/// no Odoo; repô-la aqui escrevia um hash que não serve para nada. E a conta
/// pode passar a ser gerida DEPOIS de a reposição ser emitida, por isso o uso
/// reconfere.
#[sqlx::test(migrations = "./migrations")]
async fn conta_do_odoo_nao_se_repoe_nem_ao_emitir_nem_ao_usar(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let dono = app.new_org("odoo-rp.ao").await;
    let alvo = app.add_member(&dono, "elsa", "member").await;

    // Primeiro emite-se com a conta ainda normal.
    let (st, emitida) = emitir(&app, &dono, &alvo).await;
    assert_eq!(st, 201, "emitir: {emitida}");
    let token = emitida["token"].as_str().unwrap().to_string();

    // Só DEPOIS a conta passa a ser gerida pelo Odoo.
    sqlx::query("UPDATE organizations SET odoo_enabled = TRUE WHERE id = $1::uuid")
        .bind(dono.org())
        .execute(&app.db)
        .await
        .unwrap();
    sqlx::query("UPDATE users SET odoo_managed = TRUE, odoo_org_id = $1::uuid WHERE id = $2::uuid")
        .bind(dono.org())
        .bind(&alvo.user_id)
        .execute(&app.db)
        .await
        .unwrap();

    let (st, body) = usar(&app, &token, NOVA).await;
    assert_eq!(st, 409, "o uso tinha de reconferir o Odoo: {body}");
    assert_eq!(body["code"], "account.managed_by_odoo");

    // E emitir uma nova também é recusado.
    let (st, body) = emitir(&app, &dono, &alvo).await;
    assert_eq!(st, 409, "emitir para conta do Odoo: {body}");
    assert_eq!(body["code"], "account.managed_by_odoo");

    assert_eq!(
        entra_com(&app, &alvo.email, common::PASSWORD).await,
        200,
        "a password do alvo não podia ter mudado"
    );
}

/// Um token expirado não serve, e a recusa diz-lhe porquê (412) para a pessoa
/// saber que tem de pedir outro (422, como toda a pré-condição falhada nesta
/// base) — ao contrário do 404 de um token errado.
#[sqlx::test(migrations = "./migrations")]
async fn token_expirado_e_recusado_e_nao_troca_a_password(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let dono = app.new_org("prazo.ao").await;
    let alvo = app.add_member(&dono, "fabio", "member").await;

    let (_, emitida) = emitir(&app, &dono, &alvo).await;
    let token = emitida["token"].as_str().unwrap().to_string();
    sqlx::query(
        "UPDATE password_resets SET expires_at = now() - interval '1 minute' WHERE id = $1::uuid",
    )
    .bind(emitida["id"].as_str().unwrap())
    .execute(&app.db)
    .await
    .unwrap();

    let (st, body) = usar(&app, &token, NOVA).await;
    assert_eq!(st, 422, "token expirado: {body}");
    assert_eq!(body["code"], "password_reset.expired");
    assert_eq!(
        entra_com(&app, &alvo.email, common::PASSWORD).await,
        200,
        "a password não podia ter mudado"
    );

    // E depois de expirado fica expirado: não volta a ser pendente.
    let (st, _) = usar(&app, &token, NOVA).await;
    assert_eq!(
        st, 404,
        "um token já marcado expirado deixa de ser pendente"
    );
}

/// A password nova passa pela MESMA política do registo e da mudança: a
/// reposição não é uma porta por onde entra uma password fraca.
#[sqlx::test(migrations = "./migrations")]
async fn password_nova_obedece_a_politica(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let dono = app.new_org("politica.ao").await;
    let alvo = app.add_member(&dono, "gina", "member").await;

    let (_, emitida) = emitir(&app, &dono, &alvo).await;
    let token = emitida["token"].as_str().unwrap().to_string();

    let (st, body) = usar(&app, &token, "123").await;
    assert_eq!(st, 400, "password fraca: {body}");

    // E o token continua pendente: uma tentativa falhada não o queima.
    let (st, body) = usar(&app, &token, NOVA).await;
    assert_eq!(st, 200, "o token tinha de continuar a servir: {body}");
}
