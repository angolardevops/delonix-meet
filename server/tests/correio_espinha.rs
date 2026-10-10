//! A espinha do correio (D7, ADR-0025) contra Postgres real.
//!
//! O que importa aqui NÃO é que uma mensagem chegue — isso depende de um relay,
//! e o CI não tem nenhum. É o comportamento da **caixa de saída e da fila**:
//!
//! - com o correio desligado, nada se enfileira e quem tenta sabe-o;
//! - uma reivindicação marca posse com um estado PRÓPRIO, senão a mesma
//!   mensagem sairia a cada volta do worker (foi o defeito que apanhei a
//!   escrever isto);
//! - um relay que não existe dá falha TRANSITÓRIA, com repetição agendada;
//! - um endereço inválido dá falha PERMANENTE, sem gastar tentativas;
//! - uma reivindicação abandonada por um processo morto volta à fila.
mod common;

use common::TestApp;
use delonix_server::{MailOutgoing, MailPurpose};

/// Uma mensagem de prova: a reposição de password é o primeiro consumidor real
/// desta espinha (E3), por isso é o `purpose` que se usa aqui.
fn msg<'a>(to: &'a str) -> MailOutgoing<'a> {
    MailOutgoing {
        org_id: None,
        purpose: MailPurpose::PasswordReset,
        to,
        subject: "Reposição de password",
        body: "Use este código para repor a password.",
    }
}

/// Com `SMTP_HOST` vazio o correio está desligado: `enqueue` recusa e as filas
/// não fazem nada. É o estado por omissão de qualquer servidor desta casa.
#[sqlx::test(migrations = "./migrations")]
async fn sem_relay_o_correio_esta_desligado(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    assert!(
        !delonix_server::mail_enabled(&app.state),
        "sem SMTP_HOST o correio tinha de estar desligado"
    );
    let err = delonix_server::mail_enqueue(&app.state, msg("alguem@exemplo.ao"))
        .await
        .expect_err("enfileirar sem relay tinha de ser recusado");
    assert!(
        format!("{err:?}").contains("mail.disabled"),
        "o erro tinha de ser mail.disabled: {err:?}"
    );
    // E nada ficou na caixa de saída: não se enche uma fila que ninguém esvazia.
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM mail_messages")
        .fetch_one(&app.state.db)
        .await
        .unwrap();
    assert_eq!(n, 0, "não podia ter ficado nada na caixa de saída");
    assert_eq!(
        delonix_server::mail_send_due(&app.state).await.unwrap(),
        0,
        "a fila tinha de ser no-op sem relay"
    );
}

/// Com relay configurado (mesmo um que não existe), a mensagem entra na caixa
/// de saída `pending`.
#[sqlx::test(migrations = "./migrations")]
async fn com_relay_a_mensagem_entra_na_caixa_de_saida(db: sqlx::PgPool) {
    let app = app_com_relay(db).await;
    let id = delonix_server::mail_enqueue(&app.state, msg("alguem@exemplo.ao"))
        .await
        .expect("enfileirar");
    let (status, attempt, retry_at): (String, i32, Option<chrono::DateTime<chrono::Utc>>) =
        sqlx::query_as("SELECT status, attempt, retry_at FROM mail_messages WHERE id = $1")
            .bind(id)
            .fetch_one(&app.state.db)
            .await
            .unwrap();
    assert_eq!(status, "pending");
    assert_eq!(attempt, 1);
    assert!(retry_at.is_none(), "uma mensagem nova não tem repetição");
}

/// O DEFEITO QUE ISTO GUARDA. A reivindicação tem de marcar posse com um estado
/// próprio (`sending`). Com a marca a deixar a linha em `pending`, ela
/// continuava a bater na condição de «pronta» e o worker reclamava-a outra vez
/// a cada volta — a mesma mensagem sairia tantas vezes quantas as voltas.
///
/// Prova: com um relay que não existe, uma volta falha a mensagem e agenda
/// repetição no FUTURO; a volta seguinte, logo a seguir, não pega em nada.
#[sqlx::test(migrations = "./migrations")]
async fn uma_volta_nao_reclama_a_mesma_mensagem_duas_vezes(db: sqlx::PgPool) {
    let app = app_com_relay(db).await;
    let id = delonix_server::mail_enqueue(&app.state, msg("alguem@exemplo.ao"))
        .await
        .unwrap();

    // Primeira volta: tenta, falha (o relay não existe) e agenda repetição.
    let enviadas = delonix_server::mail_send_due(&app.state).await.unwrap();
    assert_eq!(enviadas, 0, "o relay não existe: nada podia sair");

    let (status, attempt, retry_at): (String, i32, Option<chrono::DateTime<chrono::Utc>>) =
        sqlx::query_as("SELECT status, attempt, retry_at FROM mail_messages WHERE id = $1")
            .bind(id)
            .fetch_one(&app.state.db)
            .await
            .unwrap();
    assert_eq!(
        status, "failed",
        "a ligação falhada tinha de ficar registada"
    );
    assert_eq!(attempt, 2, "a tentativa tinha de subir");
    let retry_at = retry_at.expect("um relay em baixo é falha TRANSITÓRIA: tinha de reagendar");
    assert!(
        retry_at > chrono::Utc::now(),
        "a repetição tinha de ficar no futuro, não para já: {retry_at}"
    );

    // Segunda volta, imediata: a repetição ainda não venceu, não há nada a fazer.
    assert_eq!(
        delonix_server::mail_send_due(&app.state).await.unwrap(),
        0,
        "a segunda volta não podia pegar na mesma mensagem"
    );
    let attempt_depois: i32 = sqlx::query_scalar("SELECT attempt FROM mail_messages WHERE id = $1")
        .bind(id)
        .fetch_one(&app.state.db)
        .await
        .unwrap();
    assert_eq!(
        attempt_depois, 2,
        "a tentativa não podia ter subido outra vez — era a mensagem a sair a cada volta"
    );
}

/// Um endereço inválido é falha PERMANENTE: falha sem agendar repetição, para
/// não gastar as cinco tentativas numa coisa que não melhora por esperar.
#[sqlx::test(migrations = "./migrations")]
async fn endereco_invalido_nao_gasta_tentativas(db: sqlx::PgPool) {
    let app = app_com_relay(db).await;
    let id = delonix_server::mail_enqueue(&app.state, msg("isto-nao-e-um-endereco"))
        .await
        .unwrap();
    assert_eq!(delonix_server::mail_send_due(&app.state).await.unwrap(), 0);

    let (status, retry_at, error): (
        String,
        Option<chrono::DateTime<chrono::Utc>>,
        Option<String>,
    ) = sqlx::query_as("SELECT status, retry_at, error FROM mail_messages WHERE id = $1")
        .bind(id)
        .fetch_one(&app.state.db)
        .await
        .unwrap();
    assert_eq!(status, "failed");
    assert!(
        retry_at.is_none(),
        "endereço inválido não podia agendar repetição: {retry_at:?}"
    );
    assert!(
        error.unwrap_or_default().contains("destinatário"),
        "o erro tinha de dizer que o problema é o destinatário"
    );
}

/// Uma reivindicação abandonada (o processo morreu a meio do envio) volta à
/// fila pelo varredor — senão ficava `sending` para sempre e a mensagem nunca
/// mais saía.
#[sqlx::test(migrations = "./migrations")]
async fn reivindicacao_abandonada_volta_a_fila(db: sqlx::PgPool) {
    let app = app_com_relay(db).await;
    let id = delonix_server::mail_enqueue(&app.state, msg("alguem@exemplo.ao"))
        .await
        .unwrap();
    // Simula o que um processo morto deixa atrás: reivindicada e esquecida.
    sqlx::query(
        "UPDATE mail_messages
            SET status = 'sending', claimed_at = now() - interval '1 hour' WHERE id = $1",
    )
    .bind(id)
    .execute(&app.state.db)
    .await
    .unwrap();

    let (reagendadas, _) = delonix_server::mail_sweep(&app.state.db).await.unwrap();
    assert_eq!(reagendadas, 1, "a pendurada tinha de ser reagendada");
    let (status, retry_at): (String, Option<chrono::DateTime<chrono::Utc>>) =
        sqlx::query_as("SELECT status, retry_at FROM mail_messages WHERE id = $1")
            .bind(id)
            .fetch_one(&app.state.db)
            .await
            .unwrap();
    assert_eq!(status, "failed");
    assert!(
        retry_at.is_some(),
        "com tentativas por gastar, tinha de voltar à fila"
    );
}

/// Um servidor com relay configurado — um que NÃO existe, de propósito: prova o
/// comportamento da fila sem depender de correio a sair no CI.
async fn app_com_relay(db: sqlx::PgPool) -> TestApp {
    TestApp::spawn_with(
        db,
        &[
            // 127.0.0.1 numa porta onde nada ouve: a ligação falha depressa e
            // não vai à rede.
            ("SMTP_HOST", "127.0.0.1"),
            ("SMTP_PORT", "2"),
            ("SMTP_STARTTLS", "0"),
            ("SMTP_FROM", "Delonix Meet <nao-responda@exemplo.ao>"),
        ],
    )
    .await
}
