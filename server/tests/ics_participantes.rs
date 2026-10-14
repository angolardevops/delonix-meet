//! O `.ics` diz quem organiza e quem participa.
//!
//! **O que mudou e porquê.** O `ORGANIZER` entrava só num convite por correio,
//! para a DESCARGA sair byte a byte igual ao que saía antes de o módulo de
//! calendário existir. Mas um evento sem organizador não diz de quem é a
//! reunião: o Outlook e o Google mostram-no vazio e quem o importa fica sem
//! saber a quem responder.
//!
//! **A regra de quantos participantes entram é de QUEM CHAMA, não do domínio:**
//!
//! - a **descarga** leva TODOS — quem a pede já pode ler a lista pela API, com
//!   o mesmo guarda (`is_owner_or_invitee`), por isso não expõe nada de novo;
//! - o **convite por correio** leva UM, o destinatário daquela mensagem, para
//!   ele não ficar a saber a lista dos outros.
//!
//! É essa assimetria que estes testes prendem: trocá-la é uma fuga de dados
//! num sentido e um evento inútil no outro.
mod common;

use common::{Account, TestApp};

async fn ics(app: &TestApp, quem: &Account, id: &str) -> String {
    let auth = format!("Bearer {}", quem.token);
    let r = app
        .raw(
            reqwest::Method::GET,
            &format!("/api/meetings/{id}/calendar.ics"),
            &[("Authorization", &auth)],
            None,
        )
        .await;
    assert_eq!(r.status, 200, "{}", r.text);
    r.text
}

/// A descarga diz quem organiza E quem participa.
#[sqlx::test(migrations = "./migrations")]
async fn a_descarga_leva_o_organizador_e_todos_os_convidados(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let dono = app.new_org("agenda.ao").await;
    let bruno = app.add_member(&dono, "bruno", "member").await;
    let carla = app.add_member(&dono, "carla", "member").await;
    let r = app
        .new_meeting(&dono, "Conselho", &[&bruno.user_id, &carla.user_id])
        .await;
    let id = r["id"].as_str().unwrap().to_string();

    let texto = ics(&app, &dono, &id).await;
    assert!(
        texto.contains(&format!("ORGANIZER:mailto:{}", dono.email)),
        "faltou o organizador: {texto}"
    );
    for e in [&bruno.email, &carla.email] {
        assert!(
            texto.contains(&format!("ATTENDEE:mailto:{e}")),
            "faltou o convidado {e}: {texto}"
        );
    }
    // Numa descarga ninguém responde: `RSVP` é de um convite.
    assert!(
        !texto.contains("RSVP"),
        "uma descarga não pede resposta: {texto}"
    );
    assert!(texto.contains("METHOD:PUBLISH"), "{texto}");
}

/// Um convidado descarrega o mesmo evento — e isso não é uma fuga, porque a
/// lista já lhe é visível pela API com o MESMO guarda.
#[sqlx::test(migrations = "./migrations")]
async fn o_convidado_ve_a_mesma_lista_que_a_api_ja_lhe_da(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let dono = app.new_org("mesma-lista.ao").await;
    let bruno = app.add_member(&dono, "bruno", "member").await;
    let r = app.new_meeting(&dono, "Conselho", &[&bruno.user_id]).await;
    let id = r["id"].as_str().unwrap().to_string();

    // A API já lhe dá a lista…
    let (st, lista) = app
        .get(&format!("/api/meetings/{id}/invitees"), Some(&bruno.token))
        .await;
    assert_eq!(st, 200, "o convidado já podia ler a lista: {lista}");

    // …logo o `.ics` dele levar a mesma coisa não acrescenta exposição.
    let texto = ics(&app, &bruno, &id).await;
    assert!(
        texto.contains(&format!("ATTENDEE:mailto:{}", bruno.email)),
        "{texto}"
    );
    assert!(
        texto.contains(&format!("ORGANIZER:mailto:{}", dono.email)),
        "{texto}"
    );
}

/// Quem não é da reunião continua a levar 404 — o `.ics` não é uma segunda
/// porta para a lista de participantes.
#[sqlx::test(migrations = "./migrations")]
async fn quem_nao_e_da_reuniao_nao_ve_o_ics(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let dono = app.new_org("fechada.ao").await;
    let bruno = app.add_member(&dono, "bruno", "member").await;
    let estranho = app.add_member(&dono, "diogo", "member").await;
    let r = app.new_meeting(&dono, "Fechada", &[&bruno.user_id]).await;
    let id = r["id"].as_str().unwrap().to_string();

    let auth = format!("Bearer {}", estranho.token);
    let resp = app
        .raw(
            reqwest::Method::GET,
            &format!("/api/meetings/{id}/calendar.ics"),
            &[("Authorization", &auth)],
            None,
        )
        .await;
    assert_eq!(resp.status, 404, "{}", resp.text);
    assert!(
        !resp.text.contains(&bruno.email),
        "o endereço de um convidado não podia vazar: {}",
        resp.text
    );
}

/// Uma reunião sem convidados não leva `ATTENDEE` nenhum — e continua a ter
/// organizador.
#[sqlx::test(migrations = "./migrations")]
async fn sem_convidados_nao_ha_attendee_mas_ha_organizador(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let dono = app.new_org("sozinha.ao").await;
    let r = app.new_meeting(&dono, "Trabalho meu", &[]).await;
    let id = r["id"].as_str().unwrap().to_string();

    let texto = ics(&app, &dono, &id).await;
    assert!(!texto.contains("ATTENDEE"), "{texto}");
    assert!(
        texto.contains(&format!("ORGANIZER:mailto:{}", dono.email)),
        "{texto}"
    );
}
