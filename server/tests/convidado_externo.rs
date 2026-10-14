//! Convidar um endereço de email que NÃO tem conta aqui.
//!
//! **O que isto guarda.** O `.ics` no convite (#348) só chegava a quem já era
//! utilizador: o centro de notificações exige conta, preferência por canal e
//! endereço provado. O caso que se perde para o Teams é o de fora — «convida a
//! maria@outra-empresa.ao» — e é esse que estes testes exercitam: o email sai,
//! leva o calendário com o ATTENDEE DAQUELE endereço, e só o anfitrião convida.

mod common;

use common::TestApp;
use serde_json::json;

/// Relay de mentira: basta estar CONFIGURADO para a mensagem entrar na fila. O
/// envio é de outro passo (o worker), e não é isto que se está a medir.
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

async fn nova_reuniao_com_externo(
    app: &TestApp,
    dono: &common::Account,
    titulo: &str,
    externos: &[&str],
) -> serde_json::Value {
    let starts = (chrono::Utc::now() + chrono::Duration::hours(2)).to_rfc3339();
    let (st, body) = app
        .post(
            "/api/meetings",
            Some(&dono.token),
            json!({"title": titulo, "kind": "video", "starts_at": starts,
                   "duration_min": 45, "invitee_ids": [], "external_emails": externos}),
        )
        .await;
    assert_eq!(st, 200, "criar reunião com externos: {body}");
    body
}

#[sqlx::test(migrations = "./migrations")]
async fn convidar_de_fora_manda_email_com_o_calendario(db: sqlx::PgPool) {
    let app = TestApp::spawn_with(db, RELAY).await;
    let dono = app.new_org("interna.ao").await;

    nova_reuniao_com_externo(&app, &dono, "Revisão trimestral", &["Maria@Outra-Empresa.AO"]).await;

    let msgs = caixa(&app).await;
    assert_eq!(msgs.len(), 1, "uma mensagem, para o externo: {msgs:?}");
    let (purpose, to, metodo, ics) = &msgs[0];
    assert_eq!(purpose, "meeting_invitation");
    // Normalizado: o endereço entra em minúsculas, como em todo o resto da casa.
    assert_eq!(to, "maria@outra-empresa.ao");
    assert_eq!(metodo.as_deref(), Some("REQUEST"), "é um convite");
    let ics = ics.as_deref().expect("tinha de levar o .ics");
    assert!(ics.contains("METHOD:REQUEST\r\n"), "{ics}");
    assert!(ics.contains("SUMMARY:Revisão trimestral\r\n"), "{ics}");
    // O ATTENDEE é DESTE endereço: é o que faz o cliente mostrar «aceitar».
    assert!(
        ics.contains("ATTENDEE;RSVP=TRUE;PARTSTAT=NEEDS-ACTION:mailto:maria@outra-empresa.ao"),
        "{ics}"
    );
    // E o link absoluto, que é o que serve a quem está fora.
    assert!(ics.contains("URL:https://meet.exemplo.ao/#/r/"), "{ics}");
}

/// A lista é do anfitrião: um convidado COM conta vê 403 (sabe que a reunião
/// existe, porque está nela) e um estranho vê 404 (não fica a saber nada).
#[sqlx::test(migrations = "./migrations")]
async fn so_o_anfitriao_convida_de_fora(db: sqlx::PgPool) {
    let app = TestApp::spawn_with(db, RELAY).await;
    let dono = app.new_org("interna.ao").await;
    let membro = app.add_member(&dono, "bruno", "member").await;
    let estranho = app.new_org("outra-casa.ao").await;

    let reuniao = app
        .new_meeting(&dono, "Conselho", &[&membro.user_id])
        .await;
    let id = reuniao["id"].as_str().unwrap().to_string();

    let (st, body) = app
        .post(
            &format!("/api/meetings/{id}/external-invitees"),
            Some(&membro.token),
            json!({"email": "x@fora.ao"}),
        )
        .await;
    assert_eq!(st, 403, "o convidado não convida: {body}");
    assert_eq!(body["error"], "meeting.not_host", "{body}");

    let (st, body) = app
        .post(
            &format!("/api/meetings/{id}/external-invitees"),
            Some(&estranho.token),
            json!({"email": "x@fora.ao"}),
        )
        .await;
    assert_eq!(st, 404, "um estranho nem fica a saber que existe: {body}");

    // E nada disto mandou email nenhum.
    assert!(caixa(&app).await.is_empty(), "não podia ter saído correio");
}

#[sqlx::test(migrations = "./migrations")]
async fn endereco_mal_formado_recusado_antes_de_escrever(db: sqlx::PgPool) {
    let app = TestApp::spawn_with(db, RELAY).await;
    let dono = app.new_org("interna.ao").await;
    let starts = (chrono::Utc::now() + chrono::Duration::hours(2)).to_rfc3339();

    let (st, body) = app
        .post(
            "/api/meetings",
            Some(&dono.token),
            json!({"title": "Com um erro", "kind": "video", "starts_at": starts,
                   "duration_min": 30, "invitee_ids": [], "external_emails": ["isto-nao-e-email"]}),
        )
        .await;
    assert_eq!(st, 400, "{body}");
    assert_eq!(body["error"], "meeting.invalid_external_email", "{body}");

    // A reunião NÃO ficou criada: a validação é antes de escrever.
    let quantas: i64 = sqlx::query_scalar("SELECT count(*) FROM meetings")
        .fetch_one(&app.state.db)
        .await
        .unwrap();
    assert_eq!(quantas, 0, "nenhuma reunião podia ter ficado");
}

/// Remarcar manda convite NOVO (hora nova na chave de dedupe), e é esse segundo
/// `REQUEST` que substitui o evento na agenda de quem está de fora.
#[sqlx::test(migrations = "./migrations")]
async fn remarcar_manda_convite_novo_ao_externo(db: sqlx::PgPool) {
    let app = TestApp::spawn_with(db, RELAY).await;
    let dono = app.new_org("interna.ao").await;
    let reuniao = nova_reuniao_com_externo(&app, &dono, "Ponto semanal", &["ana@fora.ao"]).await;
    let id = reuniao["id"].as_str().unwrap().to_string();
    assert_eq!(caixa(&app).await.len(), 1, "o primeiro convite");

    let nova_hora = (chrono::Utc::now() + chrono::Duration::hours(26)).to_rfc3339();
    let (st, body) = app
        .post(
            &format!("/api/meetings/{id}/reschedule"),
            Some(&dono.token),
            json!({"starts_at": nova_hora}),
        )
        .await;
    assert_eq!(st, 200, "remarcar: {body}");

    let msgs = caixa(&app).await;
    assert_eq!(msgs.len(), 2, "o convite da hora nova: {msgs:?}");
    for (_, to, metodo, _) in &msgs {
        assert_eq!(to, "ana@fora.ao");
        assert_eq!(metodo.as_deref(), Some("REQUEST"));
    }
    // O `SEQUENCE` subiu com o remarcar: sem isso o Outlook ignora o segundo.
    let (_, _, _, ics2) = &msgs[1];
    let ics2 = ics2.as_deref().unwrap();
    assert!(
        ics2.contains("SEQUENCE:1\r\n"),
        "a revisão tinha de subir: {ics2}"
    );
}

/// Tirar da lista não desconvida quem já recebeu — impede o PRÓXIMO convite.
#[sqlx::test(migrations = "./migrations")]
async fn tirar_da_lista_impede_o_convite_seguinte(db: sqlx::PgPool) {
    let app = TestApp::spawn_with(db, RELAY).await;
    let dono = app.new_org("interna.ao").await;
    let reuniao = nova_reuniao_com_externo(&app, &dono, "Ponto", &["ana@fora.ao"]).await;
    let id = reuniao["id"].as_str().unwrap().to_string();

    let (st, _) = app
        .delete(
            &format!("/api/meetings/{id}/external-invitees/ana@fora.ao"),
            Some(&dono.token),
        )
        .await;
    assert_eq!(st, 204);

    let nova_hora = (chrono::Utc::now() + chrono::Duration::hours(30)).to_rfc3339();
    let (st, _) = app
        .post(
            &format!("/api/meetings/{id}/reschedule"),
            Some(&dono.token),
            json!({"starts_at": nova_hora}),
        )
        .await;
    assert_eq!(st, 200);
    assert_eq!(
        caixa(&app).await.len(),
        1,
        "só o convite inicial; o remarcar já não lhe diz nada"
    );
}
