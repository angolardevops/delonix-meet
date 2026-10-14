//! A caixa de saída vista por quem administra a organização.
//!
//! **PORQUE EXISTE A ROTA.** Uma mensagem que falhava deixava o erro numa linha
//! que a API não expunha: quem configurou um relay descobria o problema por um
//! email que nunca chegou. A verificação do relay diz se a LIGAÇÃO está boa;
//! isto diz o que aconteceu a CADA mensagem.
//!
//! **O que estes testes guardam, por ordem de gravidade:**
//!
//! 1. **o corpo nunca sai** — o de um convite leva o token do convite;
//! 2. **o correio pessoal não aparece** — a reposição de password e a prova de
//!    endereço enfileiram com `org_id` nulo, e o token de uso único delas vai
//!    no corpo;
//! 3. **cada organização só vê a sua**;
//! 4. um filtro escrito à toa é um erro, não uma página vazia a fingir que não
//!    há mensagens.
mod common;

use common::{Account, TestApp};
use serde_json::{json, Value};

const RELAY: &[(&str, &str)] = &[
    ("SMTP_HOST", "127.0.0.1"),
    ("SMTP_PORT", "2"),
    ("SMTP_STARTTLS", "0"),
    ("SMTP_FROM", "Operador <nao-responda@operador.ao>"),
    ("PUBLIC_URL", "https://meet.exemplo.ao"),
];

async fn caixa(app: &TestApp, who: &Account, query: &str) -> (u16, Value) {
    app.get(
        &format!("/api/orgs/{}/mail-messages{query}", who.org()),
        Some(&who.token),
    )
    .await
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
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string()
}

/// **O TESTE QUE IMPORTA.** O corpo nunca sai — e prova-se com um convite, cujo
/// corpo leva o **token do convite**: se a listagem devolvesse corpos, uma rota
/// de diagnóstico passava a espalhar credenciais.
#[sqlx::test(migrations = "./migrations")]
async fn a_listagem_nunca_devolve_o_corpo(db: sqlx::PgPool) {
    let app = TestApp::spawn_with(db, RELAY).await;
    let dono = app.new_org("caixa.ao").await;
    let membro = role_id(&app, &dono, "member").await;

    let (st, inv) = app
        .post(
            &format!("/api/orgs/{}/invitations", dono.org()),
            Some(&dono.token),
            json!({"email": "novo@caixa.ao", "role_id": membro}),
        )
        .await;
    assert!(st < 300, "convidar: {inv}");
    let token = inv["token"].as_str().unwrap().to_string();
    assert_eq!(
        inv["delivery_channel"], "email",
        "o convite saiu por correio"
    );

    let (st, pagina) = caixa(&app, &dono, "").await;
    assert_eq!(st, 200, "{pagina}");
    let itens = pagina["items"].as_array().unwrap();
    assert_eq!(itens.len(), 1, "uma mensagem: {pagina}");
    let m = &itens[0];
    assert_eq!(m["purpose"], "invitation");
    assert_eq!(m["to_address"], "novo@caixa.ao");
    assert_eq!(m["status"], "pending");
    assert_eq!(m["attempt"], 1);

    // Nem campo de corpo, nem o token em sítio nenhum da resposta.
    assert!(
        m.get("body").is_none() && m.get("body_text").is_none(),
        "a listagem não pode ter corpo: {m}"
    );
    assert!(
        !pagina.to_string().contains(&token),
        "o TOKEN DO CONVITE saiu na listagem: {pagina}"
    );
}

/// O correio **pessoal** não é da organização e não aparece: a reposição de
/// password enfileira com `org_id` nulo de propósito, e o seu token de uso
/// único vai no corpo.
#[sqlx::test(migrations = "./migrations")]
async fn o_correio_pessoal_nao_aparece_na_caixa_da_org(db: sqlx::PgPool) {
    let app = TestApp::spawn_with(db, RELAY).await;
    let dono = app.new_org("pessoal.ao").await;

    // Uma mensagem pessoal, enfileirada como a recuperação de password faz.
    let id = delonix_server::mail_enqueue(
        &app.state,
        delonix_server::MailOutgoing {
            org_id: None,
            purpose: delonix_server::MailPurpose::PasswordReset,
            to: &dono.email,
            subject: "Reposição de password",
            body: "https://meet.exemplo.ao/#/repor-password?token=SEGREDO-DE-USO-UNICO",
            dedupe_key: None,
            calendar: None,
        },
    )
    .await
    .expect("enfileirar")
    .expect("criada");

    // Está na base...
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM mail_messages WHERE id = $1")
        .bind(id)
        .fetch_one(&app.state.db)
        .await
        .unwrap();
    assert_eq!(n, 1);

    // ...e NÃO está na caixa da organização.
    let (st, pagina) = caixa(&app, &dono, "").await;
    assert_eq!(st, 200, "{pagina}");
    assert!(
        pagina["items"].as_array().unwrap().is_empty(),
        "o correio pessoal não é da organização: {pagina}"
    );
    assert!(
        !pagina.to_string().contains("SEGREDO-DE-USO-UNICO"),
        "o token de uso único saiu: {pagina}"
    );
}

/// Cada organização só vê a sua caixa.
#[sqlx::test(migrations = "./migrations")]
async fn uma_org_nao_ve_a_caixa_da_outra(db: sqlx::PgPool) {
    let app = TestApp::spawn_with(db, RELAY).await;
    let a = app.new_org("alfa-caixa.ao").await;
    let b = app.new_org("beta-caixa.ao").await;
    let membro_b = role_id(&app, &b, "member").await;
    let (st, _) = app
        .post(
            &format!("/api/orgs/{}/invitations", b.org()),
            Some(&b.token),
            json!({"email": "so-da-beta@beta-caixa.ao", "role_id": membro_b}),
        )
        .await;
    assert!(st < 300);

    // A, na sua caixa, não vê nada.
    let (st, pagina) = caixa(&app, &a, "").await;
    assert_eq!(st, 200);
    assert!(
        pagina["items"].as_array().unwrap().is_empty(),
        "a caixa de A está vazia: {pagina}"
    );

    // E a apontar para a org de B é recusado, sem confirmar o que lá está.
    let (st, body) = app
        .get(
            &format!("/api/orgs/{}/mail-messages", b.org()),
            Some(&a.token),
        )
        .await;
    common::assert_denied("A lê a caixa de B", st, &body, "so-da-beta@beta-caixa.ao");
}

/// Um filtro escrito à toa é um ERRO com o catálogo, não uma página vazia: numa
/// rota de diagnóstico, «não há mensagens» é a resposta mais enganadora de
/// todas.
#[sqlx::test(migrations = "./migrations")]
async fn filtro_desconhecido_e_erro_e_nao_pagina_vazia(db: sqlx::PgPool) {
    let app = TestApp::spawn_with(db, RELAY).await;
    let dono = app.new_org("filtros.ao").await;

    let (st, body) = caixa(&app, &dono, "?status=entregue").await;
    assert_eq!(st, 400, "{body}");
    assert_eq!(body["code"], "mail_messages.unknown_status");

    let (st, body) = caixa(&app, &dono, "?purpose=factura").await;
    assert_eq!(st, 400, "{body}");
    assert_eq!(body["code"], "mail_messages.unknown_purpose");

    // Os válidos passam.
    for q in [
        "?status=pending",
        "?status=failed",
        "?purpose=invitation",
        "?purpose=notification",
        "?status=sent&purpose=notification",
    ] {
        assert_eq!(caixa(&app, &dono, q).await.0, 200, "{q} tinha de servir");
    }
}

/// O erro da última tentativa é o que esta rota existe para mostrar.
#[sqlx::test(migrations = "./migrations")]
async fn a_razao_da_falha_aparece(db: sqlx::PgPool) {
    let app = TestApp::spawn_with(db, RELAY).await;
    let dono = app.new_org("falhou.ao").await;
    let membro = role_id(&app, &dono, "member").await;
    let (st, _) = app
        .post(
            &format!("/api/orgs/{}/invitations", dono.org()),
            Some(&dono.token),
            json!({"email": "destino@falhou.ao", "role_id": membro}),
        )
        .await;
    assert!(st < 300);

    // Uma volta do worker: o relay (127.0.0.1:2) não existe.
    assert_eq!(delonix_server::mail_send_due(&app.state).await.unwrap(), 0);

    let (st, pagina) = caixa(&app, &dono, "?status=failed").await;
    assert_eq!(st, 200, "{pagina}");
    let m = &pagina["items"].as_array().unwrap()[0];
    assert_eq!(m["status"], "failed");
    assert_eq!(m["attempt"], 2, "a tentativa subiu");
    assert!(
        m["error"].as_str().unwrap_or_default().len() > 3,
        "a razão da falha tinha de aparecer: {m}"
    );
    assert!(
        m["retry_at"].as_str().is_some(),
        "um relay em baixo é falha transitória: tinha de reagendar: {m}"
    );
}
