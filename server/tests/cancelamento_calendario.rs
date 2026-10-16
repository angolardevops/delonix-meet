//! Cancelar uma reunião tem de a TIRAR da agenda de quem foi convidado.
//!
//! **O que isto guarda.** Até aqui o cancelamento saía por email e o evento
//! ficava na agenda: o `notifications.rs` dizia-o em comentário e nomeava a
//! causa — o `meetings::delete` apaga a linha e notifica depois, e um
//! `METHOD:CANCEL` sem o `SEQUENCE` actual é ignorado pelo Outlook (um número
//! menor que o conhecido não conta). O `.ics` passa a construir-se ANTES do
//! `DELETE`, a partir do que a audiência captura.
//!
//! **O que isto NÃO prova:** que o Outlook ou o Google retiram de facto o
//! evento. Isso exige uma caixa de correio a sério, que não há aqui. O que se
//! afirma são os bytes que lhes seriam entregues — o `METHOD`, o `UID` e o
//! `SEQUENCE` que a norma exige para a remoção contar.

mod common;

use common::TestApp;
use serde_json::json;

/// Relay de mentira: basta estar CONFIGURADO para a mensagem entrar na fila.
const RELAY: &[(&str, &str)] = &[
    ("SMTP_HOST", "127.0.0.1"),
    ("SMTP_PORT", "2"),
    ("SMTP_STARTTLS", "0"),
    ("SMTP_FROM", "Delonix Meet <nao-responda@exemplo.ao>"),
    ("PUBLIC_URL", "https://meet.exemplo.ao"),
];

/// A caixa de saída, com o calendário que viajou em cada mensagem.
async fn caixa(app: &TestApp) -> Vec<(String, String, Option<String>, Option<String>)> {
    sqlx::query_as(
        "SELECT purpose, to_address, calendar_method, calendar_ics FROM mail_messages
          ORDER BY created_at",
    )
    .fetch_all(&app.state.db)
    .await
    .unwrap()
}

async fn nova_reuniao(
    app: &TestApp,
    dono: &common::Account,
    titulo: &str,
    convidados: &[&str],
    externos: &[&str],
) -> serde_json::Value {
    let starts = (chrono::Utc::now() + chrono::Duration::hours(3)).to_rfc3339();
    let (st, body) = app
        .post(
            "/api/meetings",
            Some(&dono.token),
            json!({"title": titulo, "kind": "video", "starts_at": starts,
                   "duration_min": 45, "invitee_ids": convidados,
                   "external_emails": externos}),
        )
        .await;
    assert_eq!(st, 200, "criar reunião: {body}");
    body
}

/// Marca o endereço como provado — o que a prova de endereço faz quando a
/// pessoa abre o link. Sem isto o centro de notificações não manda correio a
/// quem tem conta, de propósito (`notifications.rs`: «só endereços provados»),
/// e o teste media o portão em vez do cancelamento.
async fn provar_email(app: &TestApp, user_id: &str) {
    sqlx::query("UPDATE users SET email_verified_at = now() WHERE id = $1::uuid")
        .bind(user_id)
        .execute(&app.state.db)
        .await
        .unwrap();
}

/// Lê o `SEQUENCE:` de um `.ics`.
fn sequence(ics: &str) -> i32 {
    ics.lines()
        .find_map(|l| l.strip_prefix("SEQUENCE:"))
        .expect("todo o .ics leva SEQUENCE")
        .trim()
        .parse()
        .expect("o SEQUENCE é um inteiro")
}

/// Lê o `UID:` de um `.ics`.
fn uid(ics: &str) -> String {
    ics.lines()
        .find_map(|l| l.strip_prefix("UID:"))
        .expect("todo o .ics leva UID")
        .trim()
        .to_string()
}

#[sqlx::test(migrations = "./migrations")]
async fn cancelar_manda_o_cancel_ao_externo_com_o_mesmo_uid(db: sqlx::PgPool) {
    let app = TestApp::spawn_with(db, RELAY).await;
    let dono = app.new_org("interna.ao").await;

    let reuniao = nova_reuniao(
        &app,
        &dono,
        "Revisão trimestral",
        &[],
        &["maria@outra-empresa.ao"],
    )
    .await;
    let id = reuniao["id"].as_str().expect("id da reunião");

    // O convite, que é o que o cancelamento tem de anular.
    let convite = caixa(&app).await;
    assert_eq!(convite.len(), 1, "o convite saiu: {convite:?}");
    let ics_convite = convite[0].3.as_deref().expect("o convite leva .ics");
    let (uid_convite, seq_convite) = (uid(ics_convite), sequence(ics_convite));

    let (st, body) = app
        .delete(&format!("/api/meetings/{id}"), Some(&dono.token))
        .await;
    assert_eq!(st, 204, "apagar a reunião: {body}");

    let msgs = caixa(&app).await;
    assert_eq!(msgs.len(), 2, "convite + cancelamento: {msgs:?}");
    let (purpose, to, metodo, ics) = &msgs[1];
    assert_eq!(purpose, "meeting_invitation");
    assert_eq!(to, "maria@outra-empresa.ao");
    assert_eq!(metodo.as_deref(), Some("CANCEL"), "é um cancelamento");

    let ics = ics.as_deref().expect("o cancelamento tinha de levar .ics");
    assert!(ics.contains("METHOD:CANCEL\r\n"), "{ics}");
    // O MESMO UID: é por ele que o cliente sabe QUAL evento retirar. Um UID
    // diferente criava um segundo evento em vez de apagar o primeiro.
    assert_eq!(uid(ics), uid_convite, "mesmo UID do convite");
    // E um SEQUENCE não inferior: o Outlook ignora uma revisão mais antiga do
    // que a que já conhece, e o cancelamento passava sem efeito.
    assert!(
        sequence(ics) >= seq_convite,
        "SEQUENCE {} < {seq_convite} do convite — o cliente ignorava: {ics}",
        sequence(ics)
    );
    // O ATTENDEE DESTE endereço, como no convite.
    assert!(
        ics.contains("mailto:maria@outra-empresa.ao"),
        "o ATTENDEE do cancelamento é de quem o recebe: {ics}"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn cancelar_manda_o_cancel_a_quem_tem_conta(db: sqlx::PgPool) {
    let app = TestApp::spawn_with(db, RELAY).await;
    let dono = app.new_org("interna.ao").await;
    let convidado = app.add_member(&dono, "bruno", "member").await;
    provar_email(&app, &convidado.user_id).await;

    let reuniao = nova_reuniao(&app, &dono, "Ponto semanal", &[&convidado.user_id], &[]).await;
    let id = reuniao["id"].as_str().expect("id da reunião");

    let (st, _) = app
        .delete(&format!("/api/meetings/{id}"), Some(&dono.token))
        .await;
    assert_eq!(st, 204);

    let msgs = caixa(&app).await;
    let cancel = msgs
        .iter()
        .find(|(_, _, m, _)| m.as_deref() == Some("CANCEL"))
        .unwrap_or_else(|| panic!("nenhum CANCEL para quem tem conta: {msgs:?}"));
    let ics = cancel.3.as_deref().expect("leva .ics");
    assert!(ics.contains("METHOD:CANCEL\r\n"), "{ics}");
    assert_eq!(cancel.1, convidado.email);
}

/// A reunião com SÓ gente de fora era o caso que o `return` antigo calava: a
/// lista de convidados com conta vinha vazia e ninguém era avisado.
#[sqlx::test(migrations = "./migrations")]
async fn reuniao_so_com_externos_tambem_cancela(db: sqlx::PgPool) {
    let app = TestApp::spawn_with(db, RELAY).await;
    let dono = app.new_org("interna.ao").await;

    let reuniao = nova_reuniao(&app, &dono, "Com o cliente", &[], &["cliente@fora.ao"]).await;
    let id = reuniao["id"].as_str().expect("id da reunião");

    let (st, _) = app
        .delete(&format!("/api/meetings/{id}"), Some(&dono.token))
        .await;
    assert_eq!(st, 204);

    let msgs = caixa(&app).await;
    assert!(
        msgs.iter()
            .any(|(_, to, m, _)| to == "cliente@fora.ao" && m.as_deref() == Some("CANCEL")),
        "o externo tinha de ser avisado: {msgs:?}"
    );
}
