//! O canal de **correio** do centro de notificações (E2, D7/ADR-0025), contra
//! Postgres real.
//!
//! O centro de notificações (G8) já tinha preferências por tipo e por canal,
//! com o correio **ligado por omissão** para cinco tipos — e o canal nunca
//! entregava nada. Pior: a API dizia `delivery: "not_configured"` para o
//! correio **mesmo com relay configurado**, porque isso estava escrito à mão no
//! domínio. Isto liga o canal e põe a declaração a dizer a verdade.
//!
//! O que estes testes guardam, por ordem de importância:
//!
//! 1. **o correio é independente do `in_app`** — quem desliga a caixa de
//!    entrada e deixa o correio ligado recebe o email (era o defeito: o
//!    `notify` saía antes de chegar ao correio);
//! 2. **só endereços PROVADOS** — um corpo de notificação leva o título da
//!    reunião;
//! 3. **a preferência manda** — desligado é desligado;
//! 4. **o dedupe** — um produtor repetível não manda dois emails;
//! 5. **sem relay, nada** — e a API di-lo.
mod common;

use common::{Account, TestApp};
use serde_json::{json, Value};

const RELAY: &[(&str, &str)] = &[
    ("SMTP_HOST", "127.0.0.1"),
    ("SMTP_PORT", "2"),
    ("SMTP_STARTTLS", "0"),
    ("SMTP_FROM", "Delonix Meet <nao-responda@exemplo.ao>"),
    ("PUBLIC_URL", "https://meet.exemplo.ao"),
];

/// Marca o endereço como provado, que é o que a prova de endereço (PR2) faz
/// quando a pessoa abre o link. Sem isto nada sai, de propósito.
async fn provar_email(app: &TestApp, user_id: &str) {
    sqlx::query("UPDATE users SET email_verified_at = now() WHERE id = $1::uuid")
        .bind(user_id)
        .execute(&app.state.db)
        .await
        .unwrap();
}

async fn preferir(app: &TestApp, who: &Account, kind: &str, channel: &str, enabled: bool) {
    let (st, body) = app
        .put(
            "/api/users/me/notification-preferences",
            Some(&who.token),
            json!({"preferences": [{"kind": kind, "channel": channel, "enabled": enabled}]}),
        )
        .await;
    assert!(st < 300, "preferências: {st} {body}");
}

async fn caixa_de_saida(app: &TestApp) -> Vec<(String, String, String, Option<String>)> {
    sqlx::query_as(
        "SELECT purpose, to_address, body_text, dedupe_key FROM mail_messages
          ORDER BY created_at",
    )
    .fetch_all(&app.state.db)
    .await
    .unwrap()
}

/// O calendário que viajou com a mensagem: o texto iCalendar e o `METHOD`.
async fn calendario(app: &TestApp) -> Vec<(Option<String>, Option<String>)> {
    sqlx::query_as("SELECT calendar_ics, calendar_method FROM mail_messages ORDER BY created_at")
        .fetch_all(&app.state.db)
        .await
        .unwrap()
}

/// O convite para uma reunião sai por correio: o canal está ligado por omissão
/// para este tipo, o endereço está provado, e há relay.
#[sqlx::test(migrations = "./migrations")]
async fn convite_de_reuniao_sai_por_correio(db: sqlx::PgPool) {
    let app = TestApp::spawn_with(db, RELAY).await;
    let dono = app.new_org("notif-mail.ao").await;
    let convidado = app.add_member(&dono, "bruno", "member").await;
    provar_email(&app, &convidado.user_id).await;

    app.new_meeting(&dono, "Conselho de direcção", &[&convidado.user_id])
        .await;

    let caixa = caixa_de_saida(&app).await;
    assert_eq!(caixa.len(), 1, "uma mensagem, para o convidado: {caixa:?}");
    let (purpose, to, body, dedupe) = &caixa[0];
    assert_eq!(purpose, "notification");
    assert_eq!(to, &convidado.email);
    assert!(
        body.contains("Conselho de direcção"),
        "o corpo tinha de levar o título: {body}"
    );
    // O link é absoluto, do PUBLIC_URL — nunca do Host do pedido.
    assert!(
        body.contains("https://meet.exemplo.ao/#/calendar"),
        "o corpo tinha de levar o link absoluto: {body}"
    );
    // E diz como desligar: um email que não o diz é um email marcado como spam.
    assert!(
        body.contains("preferências"),
        "o corpo tinha de dizer onde se desliga: {body}"
    );
    assert!(
        dedupe.as_deref().unwrap_or_default().starts_with("notif:"),
        "a mensagem tinha de levar chave de dedupe: {dedupe:?}"
    );

    // O CONVITE DE CALENDÁRIO. Sem ele, isto é um email com um link que a
    // pessoa tem de copiar para a agenda à mão.
    let cal = calendario(&app).await;
    let (ics, metodo) = &cal[0];
    let ics = ics.as_deref().expect("a mensagem tinha de levar o .ics");
    assert_eq!(
        metodo.as_deref(),
        Some("REQUEST"),
        "é um convite, não um aviso"
    );
    assert!(ics.starts_with("BEGIN:VCALENDAR\r\n"), "{ics}");
    assert!(ics.contains("METHOD:REQUEST\r\n"), "{ics}");
    assert!(ics.contains("SUMMARY:Conselho de direcção\r\n"), "{ics}");
    // O endereço de quem recebe, para o cliente mostrar «aceitar».
    assert!(
        ics.contains(&format!(
            "ATTENDEE;RSVP=TRUE;PARTSTAT=NEEDS-ACTION:mailto:{}",
            convidado.email
        )),
        "{ics}"
    );
    // E quem organiza: sem ORGANIZER o Outlook mostra o evento e não mostra o botão.
    assert!(
        ics.contains(&format!("ORGANIZER:mailto:{}", dono.email)),
        "{ics}"
    );
    // O link da sala vai no URL, absoluto — um .ics é lido noutro programa.
    assert!(ics.contains("URL:https://meet.exemplo.ao/#/r/"), "{ics}");
}

/// **O DEFEITO QUE ISTO GUARDA.** O correio é independente do `in_app`: quem
/// desliga a caixa de entrada e deixa o correio ligado **recebe o email**.
///
/// Antes desta mudança o `notify` devolvia `None` logo que o `in_app` estivesse
/// desligado — e o correio, que vinha depois, nunca era alcançado. A API
/// oferecia a preferência e ela não fazia nada.
#[sqlx::test(migrations = "./migrations")]
async fn sem_caixa_de_entrada_o_correio_ainda_sai(db: sqlx::PgPool) {
    let app = TestApp::spawn_with(db, RELAY).await;
    let dono = app.new_org("so-correio.ao").await;
    let convidado = app.add_member(&dono, "carla", "member").await;
    provar_email(&app, &convidado.user_id).await;
    preferir(&app, &convidado, "meeting.invited", "in_app", false).await;

    app.new_meeting(&dono, "Revisão trimestral", &[&convidado.user_id])
        .await;

    // Nada na caixa de entrada...
    let n_app: i64 =
        sqlx::query_scalar("SELECT count(*) FROM notifications WHERE user_id = $1::uuid")
            .bind(&convidado.user_id)
            .fetch_one(&app.state.db)
            .await
            .unwrap();
    assert_eq!(n_app, 0, "o in_app estava desligado: não podia nascer nada");
    // ...e o email saiu, que é a escolha dela.
    let caixa = caixa_de_saida(&app).await;
    assert_eq!(
        caixa.len(),
        1,
        "o correio estava ligado: o email tinha de sair: {caixa:?}"
    );
    assert!(caixa[0].2.contains("Revisão trimestral"));
}

/// Desligar o correio para um tipo desliga-o de verdade — e a caixa de entrada
/// continua a funcionar.
#[sqlx::test(migrations = "./migrations")]
async fn correio_desligado_nao_manda_e_o_in_app_fica(db: sqlx::PgPool) {
    let app = TestApp::spawn_with(db, RELAY).await;
    let dono = app.new_org("sem-correio.ao").await;
    let convidado = app.add_member(&dono, "diogo", "member").await;
    provar_email(&app, &convidado.user_id).await;
    preferir(&app, &convidado, "meeting.invited", "email", false).await;

    app.new_meeting(&dono, "Comité", &[&convidado.user_id])
        .await;

    assert!(
        caixa_de_saida(&app).await.is_empty(),
        "o correio estava desligado para este tipo"
    );
    let n_app: i64 =
        sqlx::query_scalar("SELECT count(*) FROM notifications WHERE user_id = $1::uuid")
            .bind(&convidado.user_id)
            .fetch_one(&app.state.db)
            .await
            .unwrap();
    assert_eq!(n_app, 1, "a caixa de entrada não foi afectada");
}

/// **Só endereços provados.** Um corpo de notificação leva o título da reunião
/// e o link da sala: mandá-lo para um endereço que ninguém confirmou era
/// entregar o que se passa numa organização a quem tiver aquela caixa.
#[sqlx::test(migrations = "./migrations")]
async fn endereco_nao_provado_nao_recebe_notificacoes(db: sqlx::PgPool) {
    let app = TestApp::spawn_with(db, RELAY).await;
    let dono = app.new_org("sem-prova.ao").await;
    let convidado = app.add_member(&dono, "elsa", "member").await;
    // De propósito: NÃO se prova o endereço.

    app.new_meeting(&dono, "Secretíssima", &[&convidado.user_id])
        .await;

    assert!(
        caixa_de_saida(&app).await.is_empty(),
        "sem endereço provado não se manda nada"
    );
    // Mas a caixa de entrada funciona: a pessoa fica avisada por dentro.
    let n_app: i64 =
        sqlx::query_scalar("SELECT count(*) FROM notifications WHERE user_id = $1::uuid")
            .bind(&convidado.user_id)
            .fetch_one(&app.state.db)
            .await
            .unwrap();
    assert_eq!(n_app, 1);
}

/// O dedupe, provado onde o mecanismo vive. Os produtores de notificações são
/// repetíveis de propósito — o lembrete de reunião é um passo de varrimento e
/// uma notificação pode nascer outra vez depois de um reinício —, e sem chave
/// isso mandava o mesmo email duas vezes.
///
/// A segunda tentativa devolve `None`, que **não é erro**: é a defesa a
/// funcionar.
#[sqlx::test(migrations = "./migrations")]
async fn a_mesma_chave_nao_manda_dois_emails(db: sqlx::PgPool) {
    let app = TestApp::spawn_with(db, RELAY).await;
    let msg = |chave| delonix_server::MailOutgoing {
        org_id: None,
        purpose: delonix_server::MailPurpose::Notification,
        to: "alguem@exemplo.ao",
        subject: "A começar: Operações",
        body: "corpo",
        dedupe_key: chave,
        calendar: None,
    };

    let primeira = delonix_server::mail_enqueue(&app.state, msg(Some("notif:m1")))
        .await
        .expect("primeira");
    assert!(primeira.is_some(), "a primeira tinha de criar a mensagem");

    let segunda = delonix_server::mail_enqueue(&app.state, msg(Some("notif:m1")))
        .await
        .expect("a segunda não é erro: é o dedupe a funcionar");
    assert!(segunda.is_none(), "a segunda não podia criar mensagem nova");
    assert_eq!(caixa_de_saida(&app).await.len(), 1);

    // Chave diferente, mensagem diferente.
    assert!(
        delonix_server::mail_enqueue(&app.state, msg(Some("notif:m2")))
            .await
            .unwrap()
            .is_some(),
        "outra chave é outra mensagem"
    );
    // E SEM chave nunca se coalesce: quem pede à mão quer outra mensagem.
    assert!(delonix_server::mail_enqueue(&app.state, msg(None))
        .await
        .unwrap()
        .is_some());
    assert!(
        delonix_server::mail_enqueue(&app.state, msg(None))
            .await
            .unwrap()
            .is_some(),
        "sem chave não há dedupe"
    );
    assert_eq!(caixa_de_saida(&app).await.len(), 4);
}

/// Sem relay nada sai, e a API **di-lo**: `delivery: "not_configured"` para o
/// correio. Com relay passa a `available` — era isto que estava escrito à mão
/// e mentia desde que a espinha do correio entrou.
#[sqlx::test(migrations = "./migrations")]
async fn a_api_diz_a_verdade_sobre_o_canal(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await; // sem relay
    let dono = app.new_org("verdade.ao").await;
    let convidado = app.add_member(&dono, "gina", "member").await;
    provar_email(&app, &convidado.user_id).await;

    let (st, prefs) = app
        .get("/api/users/me/notification-preferences", Some(&dono.token))
        .await;
    assert_eq!(st, 200, "{prefs}");
    assert_eq!(
        canal(&prefs, "email"),
        "not_configured",
        "sem relay, o correio não entrega: {prefs}"
    );

    app.new_meeting(&dono, "Sem relay", &[&convidado.user_id])
        .await;
    assert!(
        caixa_de_saida(&app).await.is_empty(),
        "sem relay não se enfileira nada"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn com_relay_a_api_diz_available(db: sqlx::PgPool) {
    let app = TestApp::spawn_with(db, RELAY).await;
    let dono = app.new_org("disponivel.ao").await;
    let (st, prefs) = app
        .get("/api/users/me/notification-preferences", Some(&dono.token))
        .await;
    assert_eq!(st, 200, "{prefs}");
    assert_eq!(
        canal(&prefs, "email"),
        "available",
        "com relay, o correio entrega: {prefs}"
    );
    // O SMS continua sem entrega — não se mexeu nele.
    assert_eq!(canal(&prefs, "sms"), "not_configured");
}

fn canal<'a>(prefs: &'a Value, nome: &str) -> &'a str {
    prefs["channels"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["channel"] == nome)
        .unwrap_or_else(|| panic!("canal {nome} em {prefs}"))["delivery"]
        .as_str()
        .unwrap()
}
