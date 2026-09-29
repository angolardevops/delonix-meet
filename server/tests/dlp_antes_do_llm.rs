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
