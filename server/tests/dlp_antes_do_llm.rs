//! DLP à entrada da acta (R231): o que o cliente acumula na sala e envia no
//! fim nunca passava pelo filtro que a sinalização já aplica ao que é
//! DIFUNDIDO. Ficava gravado, ia no prompt do resumo para o LLM local, e saía
//! no webhook `meeting.mom_ready`.
//!
//! O prompt em si é provado nos testes unitários de `ai.rs` (`caption_prompt`,
//! `minutes_prompt`); aqui prova-se o que fica na BASE, contra Postgres real.
mod common;

use common::TestApp;
use serde_json::json;

const CARTAO: &str = "4111 1111 1111 1111";
const NIF: &str = "123456789";

#[sqlx::test(migrations = "./migrations")]
async fn a_acta_e_a_transcricao_sao_censuradas_ao_gravar(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let a = app.new_org("alfa.test").await;
    let m = app.new_meeting(&a, "Fecho do trimestre", &[]).await;
    let id = m["id"].as_str().unwrap();

    let (st, body) = app
        .put(
            &format!("/api/meetings/{id}/minutes"),
            Some(&a.token),
            json!({
                "minutes": format!("O cliente pagou com o cartão {CARTAO}."),
                "transcript": format!("Disse que o NIF dele é {NIF} e combinámos rever na sexta."),
            }),
        )
        .await;
    assert_eq!(st, 204, "{body}");

    let (minutes, transcript): (String, String) =
        sqlx::query_as("SELECT minutes, transcript FROM meetings WHERE id = $1::uuid")
            .bind(id)
            .fetch_one(&app.db)
            .await
            .unwrap();

    assert!(!minutes.contains("4111"), "cartão gravado: {minutes}");
    assert!(minutes.contains("BLOQUEADO PELO DLP"), "{minutes}");
    assert!(!transcript.contains(NIF), "NIF gravado: {transcript}");
    assert!(transcript.contains("BLOQUEADO PELO DLP"), "{transcript}");

    // Controlo: o resto do texto sobrevive — censurar não é apagar a acta.
    assert!(minutes.contains("O cliente pagou"), "{minutes}");
    assert!(
        transcript.contains("combinámos rever na sexta"),
        "{transcript}"
    );
}

/// Estado da fila do resumo, lido da base.
async fn fila_mom(app: &TestApp, id: &str) -> (bool, i32, bool) {
    let (queued, attempts, next): (
        Option<chrono::DateTime<chrono::Utc>>,
        i32,
        Option<chrono::DateTime<chrono::Utc>>,
    ) = sqlx::query_as(
        "SELECT mom_queued_at, mom_attempts, mom_next_attempt_at
           FROM meetings WHERE id = $1::uuid",
    )
    .bind(id)
    .fetch_one(&app.db)
    .await
    .unwrap();
    (queued.is_some(), attempts, next.is_some())
}

/// **Gravar a ata ENFILEIRA o resumo** (trabalho nº4), em vez de o disparar
/// numa tarefa que se perde.
///
/// O defeito que isto guarda: o resumo era um `tokio::spawn` com um só
/// chamador. Se o Ollama estivesse em baixo ou o pod reiniciasse, não havia
/// coluna de estado, nem varredor, nem rota — a ata por regras ficava, mas o
/// Odoo passava a ver para sempre uma reunião sem ata final (lê o
/// `minutes_ai_at`), sem ninguém poder corrigir.
#[sqlx::test(migrations = "./migrations")]
async fn gravar_a_ata_enfileira_o_resumo(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let a = app.new_org("alfa.test").await;
    let m = app.new_meeting(&a, "reunião", &[]).await;
    let id = m["id"].as_str().unwrap();

    // Antes: não está na fila.
    assert_eq!(fila_mom(&app, id).await, (false, 0, false));

    let (st, _) = app
        .put(
            &format!("/api/meetings/{id}/minutes"),
            Some(&a.token),
            json!({
                "minutes": "Decidimos adiar a migração.",
                "transcript": "Bom dia a todos. Decidimos adiar a migração do troço do Kilamba para Outubro, por causa da chuva.",
            }),
        )
        .await;
    assert_eq!(st, 204);

    // Depois: enfileirado, zero tentativas, sem espera. Sem `OLLAMA_URL` neste
    // harness a fila é um no-op — o que se prova aqui é o PEDIDO ficar.
    assert_eq!(
        fila_mom(&app, id).await,
        (true, 0, false),
        "a ata foi gravada e o resumo não ficou pedido"
    );
}

/// **A rota pede o resumo outra vez**, que era o que não existia.
#[sqlx::test(migrations = "./migrations")]
async fn a_rota_repete_o_pedido_e_recusa_quem_nao_chega(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let a = app.new_org("alfa.test").await;
    let b = app.new_org("beta.test").await;
    let m = app.new_meeting(&a, "reunião", &[]).await;
    let id = m["id"].as_str().unwrap();

    // Sem transcrição: `422` com o código do domínio, não um 500 nem um 200.
    let (st, body) = app
        .post(
            &format!("/api/meetings/{id}/minutes/summary"),
            Some(&a.token),
            json!({}),
        )
        .await;
    assert_eq!(st, 422, "{body}");
    assert_eq!(body["code"], "mom.no_transcript");

    // Com transcrição: entra na fila.
    sqlx::query(
        "UPDATE meetings SET transcript = $2, mom_attempts = 3,
                mom_next_attempt_at = now() + interval '1 hour'
          WHERE id = $1::uuid",
    )
    .bind(id)
    .bind(
        "Bom dia. Decidimos adiar a migração do troço do Kilamba para Outubro, por causa da chuva.",
    )
    .execute(&app.db)
    .await
    .unwrap();
    let (st, body) = app
        .post(
            &format!("/api/meetings/{id}/minutes/summary"),
            Some(&a.token),
            json!({}),
        )
        .await;
    assert_eq!(st, 202, "{body}");
    assert_eq!(body["attempts"], 0, "o pedido não limpou as tentativas");
    assert!(
        body["next_attempt_at"].is_null(),
        "o pedido não limpou a espera: ficaria uma hora à espera"
    );
    assert!(body["queued_at"].is_string());
    assert_eq!(fila_mom(&app, id).await, (true, 0, false));

    // Outra organização não chega: `404`, e não se confirma que existe.
    let (st, _) = app
        .post(
            &format!("/api/meetings/{id}/minutes/summary"),
            Some(&b.token),
            json!({}),
        )
        .await;
    assert_eq!(st, 404, "a reunião de outra org foi alcançada");
}
