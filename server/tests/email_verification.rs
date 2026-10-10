//! Prova do endereço de email (D7; migração 0111) contra Postgres real.
//!
//! O que se prova: o token só sai no email (nunca na resposta), o link sai de
//! `PUBLIC_URL` com o token no fragmento, a prova é de uso único e expira, a
//! conta de uma organização não prova a de outra, e mudar o email anula a
//! prova — na base, não num handler.
//!
//! O relay é `127.0.0.1:2`, onde nada ouve: o que se mede é o que fica na caixa
//! de saída, não a entrega.
mod common;

use common::TestApp;
use serde_json::json;

const PUBLIC_URL: &str = "https://meet.exemplo.ao";
const COM_CORREIO: &[(&str, &str)] = &[
    ("SMTP_HOST", "127.0.0.1"),
    ("SMTP_PORT", "2"),
    ("SMTP_FROM", "Delonix Meet <nao-responda@exemplo.ao>"),
    ("SMTP_STARTTLS", "0"),
    ("PUBLIC_URL", PUBLIC_URL),
];

/// O token que o último email da pessoa leva — tirado do CORPO, como a pessoa o
/// tiraria.
async fn token_do_email(app: &TestApp, to: &str) -> String {
    let (purpose, body): (String, String) = sqlx::query_as(
        "SELECT purpose, body_text FROM mail_messages WHERE to_address = $1
          ORDER BY created_at DESC LIMIT 1",
    )
    .bind(to)
    .fetch_one(&app.state.db)
    .await
    .unwrap_or_else(|e| panic!("tinha de haver um email para {to}: {e}"));
    assert_eq!(purpose, "email_verification");
    let marca = format!("{PUBLIC_URL}/#/verificar-email?token=");
    let i = body.find(&marca).expect("o corpo tinha de levar o link") + marca.len();
    body[i..].split_whitespace().next().unwrap().to_string()
}

async fn verificado(app: &TestApp, user_id: &str) -> bool {
    sqlx::query_scalar("SELECT email_verified_at IS NOT NULL FROM users WHERE id = $1::uuid")
        .bind(user_id)
        .fetch_one(&app.state.db)
        .await
        .unwrap()
}

#[sqlx::test(migrations = "./migrations")]
async fn sem_correio_recusa_e_nao_cria_token(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let a = app.new_org("alfa.ao").await;
    let (st, body) = app
        .post(
            "/api/users/me/email-verification",
            Some(&a.token),
            json!({}),
        )
        .await;
    assert_eq!(st, 422, "{body}");
    assert_eq!(body["code"], "mail.disabled", "{body}");
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM email_verifications")
        .fetch_one(&app.state.db)
        .await
        .unwrap();
    assert_eq!(n, 0, "não se deixa um token pendente que ninguém recebe");
}

#[sqlx::test(migrations = "./migrations")]
async fn sem_public_url_recusa_em_vez_de_adivinhar_o_host(db: sqlx::PgPool) {
    let sem_url: Vec<(&str, &str)> = COM_CORREIO
        .iter()
        .copied()
        .filter(|(k, _)| *k != "PUBLIC_URL")
        .collect();
    let app = TestApp::spawn_with(db, &sem_url).await;
    let a = app.new_org("alfa.ao").await;
    let (st, body) = app
        .post(
            "/api/users/me/email-verification",
            Some(&a.token),
            json!({}),
        )
        .await;
    assert_eq!(st, 422, "{body}");
    assert_eq!(body["code"], "mail.public_url_missing", "{body}");
}

#[sqlx::test(migrations = "./migrations")]
async fn o_link_do_email_prova_o_endereco_uma_vez(db: sqlx::PgPool) {
    let app = TestApp::spawn_with(db, COM_CORREIO).await;
    let a = app.new_org("alfa.ao").await;

    let (st, s) = app
        .get("/api/users/me/email-verification", Some(&a.token))
        .await;
    assert_eq!((st, s["status"].as_str()), (200, Some("unverified")), "{s}");

    let (st, body) = app
        .post(
            "/api/users/me/email-verification",
            Some(&a.token),
            json!({}),
        )
        .await;
    assert_eq!(st, 202, "{body}");
    assert_eq!(body["status"], "sent");
    // O token NÃO vem na resposta: devolvê-lo provaria a sessão, não o endereço.
    assert!(
        !body.to_string().contains("dlxv_"),
        "o token não pode sair na resposta: {body}"
    );
    let (_, s) = app
        .get("/api/users/me/email-verification", Some(&a.token))
        .await;
    assert_eq!(s["status"], "pending", "{s}");

    let token = token_do_email(&app, &a.email).await;
    // Aceita-se SEM sessão: o link abre-se noutro aparelho.
    let (st, ok) = app
        .post(
            "/api/email-verifications/accept",
            None,
            json!({ "token": token }),
        )
        .await;
    assert_eq!(st, 200, "{ok}");
    assert_eq!(ok["email"], a.email.as_str());
    assert!(verificado(&app, &a.user_id).await);
    let (_, s) = app
        .get("/api/users/me/email-verification", Some(&a.token))
        .await;
    assert_eq!(s["status"], "verified", "{s}");

    // Uso único: o mesmo token já não serve, com o mesmo 404 de um inventado.
    let (st, again) = app
        .post(
            "/api/email-verifications/accept",
            None,
            json!({ "token": token }),
        )
        .await;
    assert_eq!(st, 404, "{again}");
    assert_eq!(again["code"], "email_verification.not_found");
    let (st, inventado) = app
        .post(
            "/api/email-verifications/accept",
            None,
            json!({ "token": "dlxv_00" }),
        )
        .await;
    assert_eq!((st, &inventado["code"]), (404, &again["code"]));

    // Já provado: pedir outra vez não envia nada.
    let (st, body) = app
        .post(
            "/api/users/me/email-verification",
            Some(&a.token),
            json!({}),
        )
        .await;
    assert_eq!(
        (st, body["status"].as_str()),
        (200, Some("verified")),
        "{body}"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn o_token_de_uma_conta_nao_prova_a_de_outra_org(db: sqlx::PgPool) {
    let app = TestApp::spawn_with(db, COM_CORREIO).await;
    let a = app.new_org("alfa.ao").await;
    let b = app.new_org("beta.ao").await;
    let (st, _) = app
        .post(
            "/api/users/me/email-verification",
            Some(&a.token),
            json!({}),
        )
        .await;
    assert_eq!(st, 202);
    let token_a = token_do_email(&app, &a.email).await;
    let (st, _) = app
        .post(
            "/api/email-verifications/accept",
            None,
            json!({ "token": token_a }),
        )
        .await;
    assert_eq!(st, 200);
    assert!(verificado(&app, &a.user_id).await);
    assert!(
        !verificado(&app, &b.user_id).await,
        "provar a conta de alfa não pode provar a de beta"
    );
    // E a B vê o seu estado, não o de A.
    let (_, s) = app
        .get("/api/users/me/email-verification", Some(&b.token))
        .await;
    assert_eq!(s["status"], "unverified", "{s}");
    assert_eq!(s["email"], b.email.as_str(), "{s}");
}

#[sqlx::test(migrations = "./migrations")]
async fn um_segundo_pedido_no_mesmo_minuto_espera(db: sqlx::PgPool) {
    let app = TestApp::spawn_with(db, COM_CORREIO).await;
    let a = app.new_org("alfa.ao").await;
    let (st, _) = app
        .post(
            "/api/users/me/email-verification",
            Some(&a.token),
            json!({}),
        )
        .await;
    assert_eq!(st, 202);
    let bearer = format!("Bearer {}", a.token);
    let r = app
        .raw(
            reqwest::Method::POST,
            "/api/users/me/email-verification",
            &[("authorization", bearer.as_str())],
            Some(json!({})),
        )
        .await;
    assert_eq!(r.status, 429, "{}", r.text);
    let espera: u64 = r.header("retry-after").unwrap().parse().unwrap();
    assert!((1..=60).contains(&espera), "Retry-After {espera}");
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM mail_messages")
        .fetch_one(&app.state.db)
        .await
        .unwrap();
    assert_eq!(n, 1, "o segundo pedido não pode ter enviado outro email");

    // Passado o minuto, o pedido novo REVOGA o anterior: só um link vale.
    sqlx::query("UPDATE email_verifications SET created_at = now() - interval '2 minutes'")
        .execute(&app.state.db)
        .await
        .unwrap();
    let antigo = token_do_email(&app, &a.email).await;
    let (st, _) = app
        .post(
            "/api/users/me/email-verification",
            Some(&a.token),
            json!({}),
        )
        .await;
    assert_eq!(st, 202);
    let (st, _) = app
        .post(
            "/api/email-verifications/accept",
            None,
            json!({ "token": antigo }),
        )
        .await;
    assert_eq!(st, 404, "o link anterior tinha de ficar revogado");
}

#[sqlx::test(migrations = "./migrations")]
async fn um_link_expirado_nao_prova_nada(db: sqlx::PgPool) {
    let app = TestApp::spawn_with(db, COM_CORREIO).await;
    let a = app.new_org("alfa.ao").await;
    app.post(
        "/api/users/me/email-verification",
        Some(&a.token),
        json!({}),
    )
    .await;
    let token = token_do_email(&app, &a.email).await;
    sqlx::query("UPDATE email_verifications SET expires_at = now() - interval '1 second'")
        .execute(&app.state.db)
        .await
        .unwrap();
    let (st, body) = app
        .post(
            "/api/email-verifications/accept",
            None,
            json!({ "token": token }),
        )
        .await;
    assert_eq!(st, 422, "{body}");
    assert_eq!(body["code"], "email_verification.expired");
    assert!(!verificado(&app, &a.user_id).await);
}

/// O trigger da 0111: mudar o email anula a prova e revoga o link pendente.
/// Hoje nenhuma rota muda o email; a regra está na base para a primeira que o
/// venha a fazer não a poder esquecer.
#[sqlx::test(migrations = "./migrations")]
async fn mudar_o_email_anula_a_prova_e_o_link_pendente(db: sqlx::PgPool) {
    let app = TestApp::spawn_with(db, COM_CORREIO).await;
    let a = app.new_org("alfa.ao").await;
    app.post(
        "/api/users/me/email-verification",
        Some(&a.token),
        json!({}),
    )
    .await;
    let token = token_do_email(&app, &a.email).await;
    app.post(
        "/api/email-verifications/accept",
        None,
        json!({ "token": token }),
    )
    .await;
    assert!(verificado(&app, &a.user_id).await);

    // Um segundo link pendente (o primeiro já foi usado), e depois a mudança.
    sqlx::query("UPDATE users SET email_verified_at = NULL WHERE id = $1::uuid")
        .bind(&a.user_id)
        .execute(&app.state.db)
        .await
        .unwrap();
    sqlx::query("UPDATE email_verifications SET created_at = now() - interval '2 minutes'")
        .execute(&app.state.db)
        .await
        .unwrap();
    app.post(
        "/api/users/me/email-verification",
        Some(&a.token),
        json!({}),
    )
    .await;
    let pendente = token_do_email(&app, &a.email).await;
    sqlx::query("UPDATE users SET email_verified_at = now() WHERE id = $1::uuid")
        .bind(&a.user_id)
        .execute(&app.state.db)
        .await
        .unwrap();

    sqlx::query("UPDATE users SET email = 'outro@alfa.ao' WHERE id = $1::uuid")
        .bind(&a.user_id)
        .execute(&app.state.db)
        .await
        .unwrap();
    assert!(
        !verificado(&app, &a.user_id).await,
        "o email novo não foi provado por ninguém"
    );
    let (st, _) = app
        .post(
            "/api/email-verifications/accept",
            None,
            json!({ "token": pendente }),
        )
        .await;
    assert_eq!(st, 404, "o link do endereço antigo tinha de ficar revogado");
    assert!(!verificado(&app, &a.user_id).await);

    // Uma escrita que NÃO muda o email não mexe na prova.
    sqlx::query("UPDATE users SET email_verified_at = now() WHERE id = $1::uuid")
        .bind(&a.user_id)
        .execute(&app.state.db)
        .await
        .unwrap();
    sqlx::query("UPDATE users SET email = email, username = 'novo' WHERE id = $1::uuid")
        .bind(&a.user_id)
        .execute(&app.state.db)
        .await
        .unwrap();
    assert!(verificado(&app, &a.user_id).await);
}
