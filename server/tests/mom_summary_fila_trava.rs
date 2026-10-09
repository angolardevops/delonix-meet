//! A trava "em execução" da fila do resumo da acta (`mom_running_at`,
//! migração 0105), contra Postgres real e um servidor a sério, com um Ollama
//! FALSO dentro do teste (`common::fake_ollama`).
//!
//! O defeito que isto guarda (levantamento de 2026-10-07, fase de robustez da
//! IA de reuniões, PR A): `enqueue_mom_summary` não tinha nenhuma condição que
//! verificasse se já havia uma geração a decorrer, e o `claim` genérico
//! protege duas reivindicações SIMULTÂNEAS da mesma linha mas não protege
//! contra uma reivindicação NOVA enquanto a antiga ainda está a meio da
//! chamada ao Ollama — nada marcava "isto está a correr". Duas chamadas quase
//! ao mesmo tempo (o botão manual e o `beforeunload`, por exemplo) gastavam o
//! dobro do trabalho ao Ollama, e a escrita final não tinha protecção de
//! ordem.
//!
//! O que se prova aqui:
//! - uma reivindicação a decorrer recusa uma segunda (`enqueue_mom_summary` E
//!   a volta seguinte da fila) — só UMA chamada chega ao Ollama;
//! - uma reivindicação presa (sem sinal há mais do que o tecto + a margem)
//!   lê-se como interrompida e pode ser retomada, sem varredor dedicado;
//! - uma reivindicação recente (dentro do tecto) NÃO é retomada.
mod common;

use common::fake_ollama::{FakeOllama, Reply};
use common::{Account, TestApp};
use serde_json::json;

const MODEL: &str = "qwen2.5:1.5b";
const TRANSCRIPT: &str = "Bom dia a todos. Decidimos adiar a migração do troço do Kilamba \
    para Outubro, por causa da chuva prevista para a próxima semana inteira.";
const RESUMO: &str = "## Resumo\nAdiada a migração.\n## Pontos discutidos\n- Chuva\n\
    ## Decisões\nNenhuma registada.\n## Decisões e ações\nNenhuma registada.";

struct Rec {
    app: TestApp,
    fake: FakeOllama,
    a: Account,
    meeting_id: String,
}

async fn setup(db: sqlx::PgPool, reply: Reply) -> Rec {
    let fake = FakeOllama::start(&[MODEL], reply).await;
    let app = TestApp::spawn_with(
        db,
        &[
            ("OLLAMA_URL", fake.url.as_str()),
            ("OLLAMA_MODEL_SUMMARY", MODEL),
        ],
    )
    .await;
    let a = app.new_org("alfa-mom.test").await;
    let m = app.new_meeting(&a, "reunião", &[]).await;
    let meeting_id = m["id"].as_str().unwrap().to_string();
    // Grava a ata: enfileira o resumo (`mom_queued_at`), transcrição >= 80
    // caracteres para a fila a considerar elegível.
    let (st, body) = app
        .put(
            &format!("/api/meetings/{meeting_id}/minutes"),
            Some(&a.token),
            json!({"minutes": "ata por regras", "transcript": TRANSCRIPT}),
        )
        .await;
    assert_eq!(st, 204, "{body}");
    Rec {
        app,
        fake,
        a,
        meeting_id,
    }
}

/// `(mom_attempts, mom_running_at is_some, minutes_ai_at is_some)`.
async fn estado(app: &TestApp, id: &str) -> (i32, bool, bool) {
    let (attempts, running, done): (
        i32,
        Option<chrono::DateTime<chrono::Utc>>,
        Option<chrono::DateTime<chrono::Utc>>,
    ) = sqlx::query_as(
        "SELECT mom_attempts, mom_running_at, minutes_ai_at FROM meetings WHERE id = $1::uuid",
    )
    .bind(id)
    .fetch_one(&app.db)
    .await
    .unwrap();
    (attempts, running.is_some(), done.is_some())
}

/// **Só uma reivindicação chega ao Ollama.** Uma presa a meio da chamada (via
/// `Reply::Hold`) recusa: um segundo `PUT …/minutes` (o `beforeunload` depois
/// do botão manual, por exemplo) NÃO reabre a fila, e uma segunda volta da
/// fila (a varredura seguinte) não reivindica a mesma reunião outra vez.
#[sqlx::test(migrations = "./migrations")]
async fn duas_reivindicacoes_quase_simultaneas_so_uma_chega_ao_ollama(db: sqlx::PgPool) {
    let r = setup(db, Reply::Hold(RESUMO.into())).await;
    let (app, a, id) = (&r.app, &r.a, r.meeting_id.clone());

    let (attempts0, running0, done0) = estado(app, &id).await;
    assert_eq!((attempts0, running0, done0), (0, false, false));

    // Primeira volta da fila: reivindica a reunião (marca `mom_running_at`,
    // conta a tentativa) e FICA presa no Ollama (`Reply::Hold`).
    let state = app.state.clone();
    let primeira = tokio::spawn(async move { delonix_server::mom_summary_due(&state).await });
    r.fake.wait_entered().await;

    // Reivindicada: a tentativa já está contada e a trava está posta.
    let (attempts1, running1, _) = estado(app, &id).await;
    assert_eq!(attempts1, 1, "a reivindicação não contou a tentativa");
    assert!(running1, "a trava não ficou posta depois da reivindicação");

    // "beforeunload" a seguir ao botão manual: um segundo `PUT …/minutes`
    // enquanto a primeira reivindicação ainda está a meio NÃO pode reabrir a
    // fila — é EXACTAMENTE o defeito que isto corrige.
    let (st, body) = app
        .put(
            &format!("/api/meetings/{id}/minutes"),
            Some(&a.token),
            json!({"minutes": "ata por regras (2ª gravação)", "transcript": TRANSCRIPT}),
        )
        .await;
    assert_eq!(st, 204, "{body}");
    let (attempts2, running2, _) = estado(app, &id).await;
    assert_eq!(
        attempts2, 1,
        "o 2º PUT reabriu a reivindicação (zerou as tentativas)"
    );
    assert!(
        running2,
        "o 2º PUT largou a trava da reivindicação em curso"
    );

    // A volta seguinte da fila (outro nó, ou o mesmo ciclo 60s depois): não
    // reivindica a MESMA reunião porque a trava ainda não está "stale".
    let n = delonix_server::mom_summary_due(&app.state)
        .await
        .unwrap_or_else(|_| panic!("a 2ª volta da fila falhou"));
    assert_eq!(n, 0, "uma 2ª reivindicação levou a mesma reunião");
    assert_eq!(
        r.fake.calls(),
        1,
        "mais do que uma chamada chegou ao Ollama"
    );

    // Larga a primeira reivindicação: fecha, grava a ata final e limpa a trava.
    r.fake.release(1);
    primeira
        .await
        .expect("a 1ª reivindicação não terminou")
        .expect("a 1ª reivindicação falhou");

    assert_eq!(
        r.fake.calls(),
        1,
        "o Ollama foi chamado mais do que uma vez"
    );
    let (attempts3, running3, done3) = estado(app, &id).await;
    assert_eq!(attempts3, 1);
    assert!(!running3, "a trava ficou presa depois de terminar");
    assert!(done3, "a ata final não ficou gravada");

    let (minutes,): (String,) = sqlx::query_as("SELECT minutes FROM meetings WHERE id = $1::uuid")
        .bind(&id)
        .fetch_one(&app.db)
        .await
        .unwrap();
    assert!(minutes.contains("Resumo"), "{minutes}");
}

/// **Uma reivindicação presa fica "stale" e pode ser retomada** — sem
/// varredor dedicado, como nos capítulos (`recording_chapter_generations`).
#[sqlx::test(migrations = "./migrations")]
async fn reivindicacao_presa_fica_stale_e_e_retomada(db: sqlx::PgPool) {
    let r = setup(db, Reply::Answer(RESUMO.into())).await;
    let (app, id) = (&r.app, r.meeting_id.clone());

    // Simula um worker morto a meio: reivindicada há mais do que o tecto da
    // chamada (600 s) mais a margem (60 s) — 11 minutos e um segundo.
    sqlx::query(
        "UPDATE meetings
            SET mom_attempts = 1, mom_running_at = now() - interval '11 minutes 1 second'
          WHERE id = $1::uuid",
    )
    .bind(&id)
    .execute(&app.db)
    .await
    .unwrap();

    let n = delonix_server::mom_summary_due(&app.state).await.unwrap();
    assert_eq!(n, 1, "a reivindicação stale não foi retomada");
    assert_eq!(r.fake.calls(), 1);

    let (_, running, done) = estado(app, &id).await;
    assert!(!running, "a trava ficou presa depois de retomar");
    assert!(done, "a ata final não ficou gravada depois de retomar");
}

/// **Uma reivindicação recente NÃO é retomada** — só a estagnada é que o é.
#[sqlx::test(migrations = "./migrations")]
async fn reivindicacao_recente_nao_e_retomada(db: sqlx::PgPool) {
    let r = setup(db, Reply::Answer(RESUMO.into())).await;
    let (app, id) = (&r.app, r.meeting_id.clone());

    // Reivindicada há 10 s: bem dentro do tecto, uma 2ª volta não a leva.
    sqlx::query(
        "UPDATE meetings SET mom_attempts = 1, mom_running_at = now() - interval '10 seconds'
          WHERE id = $1::uuid",
    )
    .bind(&id)
    .execute(&app.db)
    .await
    .unwrap();

    let n = delonix_server::mom_summary_due(&app.state).await.unwrap();
    assert_eq!(n, 0, "uma reivindicação recente foi retomada");
    assert_eq!(r.fake.calls(), 0, "o Ollama foi chamado sem necessidade");
}
