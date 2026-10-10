//! Reposição de password pedida pela própria pessoa, por email (E3; migração
//! 0112) contra Postgres real.
//!
//! O que se prova: a resposta é a MESMA para qualquer endereço; só uma conta com
//! o email PROVADO recebe um link; o link repõe a password e termina as
//! sessões; o pedido por email revoga o token do administrador (uma porta só);
//! e o pedido de uma conta nunca toca na de outra organização.
//!
//! O trabalho da conta corre DEPOIS da resposta (ver `password_recovery`), por
//! isso o teste espera pela caixa de saída em vez de a ler logo.
mod common;

use std::time::Duration;

use common::{TestApp, PASSWORD};
use serde_json::json;

const PUBLIC_URL: &str = "https://meet.exemplo.ao";
const COM_CORREIO: &[(&str, &str)] = &[
    ("SMTP_HOST", "127.0.0.1"),
    ("SMTP_PORT", "2"),
    ("SMTP_FROM", "Delonix Meet <nao-responda@exemplo.ao>"),
    ("SMTP_STARTTLS", "0"),
    ("PUBLIC_URL", PUBLIC_URL),
];

async fn emails_para(app: &TestApp, to: &str) -> i64 {
    sqlx::query_scalar(
        "SELECT count(*) FROM mail_messages WHERE to_address = $1 AND purpose = 'password_reset'",
    )
    .bind(to)
    .fetch_one(&app.state.db)
    .await
    .unwrap()
}

/// Espera até haver `n` emails de reposição para `to` (ou desiste aos 5 s).
async fn espera_emails(app: &TestApp, to: &str, n: i64) -> i64 {
    for _ in 0..50 {
        let c = emails_para(app, to).await;
        if c >= n {
            return c;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    emails_para(app, to).await
}

/// Para os casos em que NADA pode sair: dá à tarefa tempo de acabar.
async fn nada_saiu(app: &TestApp, to: &str) -> bool {
    tokio::time::sleep(Duration::from_millis(1500)).await;
    emails_para(app, to).await == 0
}

async fn token_do_email(app: &TestApp, to: &str) -> String {
    let body: String = sqlx::query_scalar(
        "SELECT body_text FROM mail_messages WHERE to_address = $1 AND purpose = 'password_reset'
          ORDER BY created_at DESC LIMIT 1",
    )
    .bind(to)
    .fetch_one(&app.state.db)
    .await
    .unwrap();
    let marca = format!("{PUBLIC_URL}/#/repor-password?token=");
    let i = body.find(&marca).expect("o corpo tinha de levar o link") + marca.len();
    body[i..].split_whitespace().next().unwrap().to_string()
}

async fn provar_email(app: &TestApp, user_id: &str) {
    sqlx::query("UPDATE users SET email_verified_at = now() WHERE id = $1::uuid")
        .bind(user_id)
        .execute(&app.state.db)
        .await
        .unwrap();
}

async fn pedir(app: &TestApp, email: &str) -> (u16, serde_json::Value) {
    app.post("/api/password-resets/request", None, json!({ "email": email }))
        .await
}

#[sqlx::test(migrations = "./migrations")]
async fn sem_correio_recusa_igual_para_qualquer_endereco(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let a = app.new_org("alfa.ao").await;
    let (st1, b1) = pedir(&app, &a.email).await;
    let (st2, b2) = pedir(&app, "ninguem@nenhures.ao").await;
    assert_eq!((st1, st2), (422, 422), "{b1} {b2}");
    assert_eq!(b1["code"], "mail.disabled");
    assert_eq!(b1["code"], b2["code"], "o estado do servidor não distingue contas");
}

#[sqlx::test(migrations = "./migrations")]
async fn a_resposta_e_a_mesma_e_so_o_email_provado_recebe(db: sqlx::PgPool) {
    let app = TestApp::spawn_with(db, COM_CORREIO).await;
    let provada = app.new_org("alfa.ao").await;
    let por_provar = app.new_org("beta.ao").await;
    provar_email(&app, &provada.user_id).await;

    let (st_a, a) = pedir(&app, &provada.email).await;
    let (st_b, b) = pedir(&app, &por_provar.email).await;
    let (st_c, c) = pedir(&app, "ninguem@nenhures.ao").await;
    assert_eq!((st_a, st_b, st_c), (202, 202, 202));
    assert_eq!(a, b, "conta provada e por provar respondem igual");
    assert_eq!(a, c, "conta existente e inexistente respondem igual");
    assert!(!a.to_string().contains("dlxr_"), "o token nunca vai na resposta: {a}");

    assert_eq!(espera_emails(&app, &provada.email, 1).await, 1);
    assert!(
        nada_saiu(&app, &por_provar.email).await,
        "um endereço por provar não recebe link (D7)"
    );
    assert!(nada_saiu(&app, "ninguem@nenhures.ao").await);
}

#[sqlx::test(migrations = "./migrations")]
async fn o_link_repoe_a_password_e_termina_as_sessoes(db: sqlx::PgPool) {
    let app = TestApp::spawn_with(db, COM_CORREIO).await;
    let a = app.new_org("alfa.ao").await;
    provar_email(&app, &a.user_id).await;
    // Normaliza como o registo: maiúsculas e espaços não mudam a conta.
    let (st, _) = pedir(&app, &format!("  {}  ", a.email.to_uppercase())).await;
    assert_eq!(st, 202);
    assert_eq!(espera_emails(&app, &a.email, 1).await, 1);
    let token = token_do_email(&app, &a.email).await;

    let nova = "Nova-Palavra-Passe-2026!";
    let (st, ok) = app
        .post(
            "/api/password-resets/accept",
            None,
            json!({ "token": token, "password": nova }),
        )
        .await;
    assert_eq!(st, 200, "{ok}");
    assert!(ok["sessions_revoked"].as_u64().unwrap() >= 1, "{ok}");

    // A password antiga deixou de servir; a nova serve.
    let (st, _) = app
        .post("/api/auth/login", None, json!({ "email": a.email, "password": PASSWORD }))
        .await;
    assert_eq!(st, 401, "a password antiga ainda abria a conta");
    let (st, _) = app
        .post("/api/auth/login", None, json!({ "email": a.email, "password": nova }))
        .await;
    assert_eq!(st, 200);

    let (canal, org): (String, Option<uuid::Uuid>) =
        sqlx::query_as("SELECT channel, org_id FROM password_resets WHERE user_id = $1::uuid")
            .bind(&a.user_id)
            .fetch_one(&app.state.db)
            .await
            .unwrap();
    assert_eq!((canal.as_str(), org), ("email", None));

    // Uso único.
    let (st, _) = app
        .post(
            "/api/password-resets/accept",
            None,
            json!({ "token": token, "password": "Outra-Palavra-2026!" }),
        )
        .await;
    assert_eq!(st, 404);
}

#[sqlx::test(migrations = "./migrations")]
async fn o_pedido_por_email_revoga_o_token_do_administrador(db: sqlx::PgPool) {
    let app = TestApp::spawn_with(db, COM_CORREIO).await;
    let admin = app.new_org("alfa.ao").await;
    let membro = app.add_member(&admin, "ana", "member").await;
    provar_email(&app, &membro.user_id).await;

    let (st, emitido) = app
        .post(
            &format!(
                "/api/orgs/{}/users/{}/password-reset",
                admin.org(),
                membro.user_id
            ),
            Some(&admin.token),
            json!({}),
        )
        .await;
    assert_eq!(st, 201, "{emitido}");
    let do_admin = emitido["token"].as_str().unwrap().to_string();

    pedir(&app, &membro.email).await;
    assert_eq!(espera_emails(&app, &membro.email, 1).await, 1);

    let (st, _) = app
        .post(
            "/api/password-resets/accept",
            None,
            json!({ "token": do_admin, "password": "Nova-Palavra-Passe-2026!" }),
        )
        .await;
    assert_eq!(st, 404, "dois tokens válidos para a mesma conta eram duas portas");
}

#[sqlx::test(migrations = "./migrations")]
async fn um_segundo_pedido_no_mesmo_minuto_nao_envia(db: sqlx::PgPool) {
    let app = TestApp::spawn_with(db, COM_CORREIO).await;
    let a = app.new_org("alfa.ao").await;
    provar_email(&app, &a.user_id).await;
    pedir(&app, &a.email).await;
    assert_eq!(espera_emails(&app, &a.email, 1).await, 1);
    let (st, _) = pedir(&app, &a.email).await;
    assert_eq!(st, 202, "a resposta não muda — não se diz que houve um pedido");
    tokio::time::sleep(Duration::from_millis(1500)).await;
    assert_eq!(emails_para(&app, &a.email).await, 1, "o segundo não pode ter saído");
}

#[sqlx::test(migrations = "./migrations")]
async fn uma_conta_nao_toca_na_de_outra_org(db: sqlx::PgPool) {
    let app = TestApp::spawn_with(db, COM_CORREIO).await;
    let a = app.new_org("alfa.ao").await;
    let b = app.new_org("beta.ao").await;
    provar_email(&app, &a.user_id).await;
    provar_email(&app, &b.user_id).await;
    pedir(&app, &a.email).await;
    assert_eq!(espera_emails(&app, &a.email, 1).await, 1);
    assert!(nada_saiu(&app, &b.email).await, "pedir para A não envia nada a B");
    let n_b: i64 =
        sqlx::query_scalar("SELECT count(*) FROM password_resets WHERE user_id = $1::uuid")
            .bind(&b.user_id)
            .fetch_one(&app.state.db)
            .await
            .unwrap();
    assert_eq!(n_b, 0);
    // E o token de A não abre B: repor por ele só muda a password de A.
    let token = token_do_email(&app, &a.email).await;
    app.post(
        "/api/password-resets/accept",
        None,
        json!({ "token": token, "password": "Nova-Palavra-Passe-2026!" }),
    )
    .await;
    let (st, _) = app
        .post("/api/auth/login", None, json!({ "email": b.email, "password": PASSWORD }))
        .await;
    assert_eq!(st, 200, "a password de B não podia ter mudado");
}
