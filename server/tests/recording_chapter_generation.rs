//! O estado da geração de capítulos (`recording_chapter_generations`, migração
//! 0087), contra Postgres real e um servidor a sério, com um Ollama FALSO
//! dentro do teste (`common::fake_ollama`):
//!
//! - `GET /api/recordings/{id}/chapters/generation` — `idle`, `succeeded`,
//!   `failed` com o código, e um `running` sem sinal lido como interrompido;
//! - `POST …/chapters/generate` — a resposta do modelo sem capítulos
//!   utilizáveis dá `503`, NÃO apaga os automáticos que havia e NÃO marca a
//!   gravação como gerada; os manuais nunca se apagam; uma geração a decorrer
//!   recusa a segunda com `409`;
//! - `PATCH …/chapters/{chapter_id}` sem campos não converte um capítulo
//!   automático em manual (R232).
mod common;

use common::fake_ollama::{FakeOllama, Reply};
use common::{Account, TestApp};
use serde_json::{json, Value};

const MODEL: &str = "qwen2.5:1.5b";

const CHAPTERS: &str = r#"Aqui estão: [
    {"start": "00:00:00", "title": "Rede de Luanda"},
    {"start": "00:00:20", "title": "Troço do Kilamba"},
    {"start": "00:01:00", "title": "Migração"},
    {"start": "05:00:00", "title": "Depois do fim"}]"#;

fn segments() -> Value {
    json!([
        {"start_ms": 0, "end_ms": 20000, "text": "Bom dia, vamos falar da rede de Luanda."},
        {"start_ms": 20000, "end_ms": 60000, "text": "O troço do Kilamba está em manutenção."},
        {"start_ms": 60000, "end_ms": 120000, "text": "Decidimos adiar a migração para Outubro."}
    ])
}

struct Rec {
    app: TestApp,
    fake: FakeOllama,
    /// Admin da org A e dono da gravação.
    a: Account,
    /// Membro da A que participou (só vê).
    carla: Account,
    /// Admin de outra org.
    b: Account,
    rec: String,
}

async fn recording(db: sqlx::PgPool, reply: Reply) -> Rec {
    let fake = FakeOllama::start(&[MODEL], reply).await;
    let app = TestApp::spawn_with(
        db,
        &[
            ("OLLAMA_URL", fake.url.as_str()),
            ("OLLAMA_MODEL_SUMMARY", MODEL),
        ],
    )
    .await;
    let a = app.new_org("alfa-cap.test").await;
    let b = app.new_org("beta-cap.test").await;
    let carla = app.add_member(&a, "carla", "member").await;
    let room = app.new_room(&a, "gravada").await;
    let room_id = room["id"].as_str().unwrap().to_string();
    let rec = app.insert_recording(&room_id, &a.user_id).await;
    sqlx::query("INSERT INTO room_participants (room_id, user_id) VALUES ($1::uuid, $2::uuid)")
        .bind(&room_id)
        .bind(&carla.user_id)
        .execute(&app.db)
        .await
        .unwrap();
    Rec {
        app,
        fake,
        a,
        carla,
        b,
        rec,
    }
}

async fn transcribe(app: &TestApp, rec: &str) {
    sqlx::query(
        "UPDATE recordings SET transcribed_at = now(), transcript = 'rede de Luanda',
                transcript_language = 'pt', duration_ms = 180000, transcript_segments = $2
          WHERE id = $1::uuid",
    )
    .bind(rec)
    .bind(segments())
    .execute(&app.db)
    .await
    .unwrap();
}

async fn generated_at_is_set(app: &TestApp, rec: &str) -> bool {
    let (set,): (bool,) = sqlx::query_as(
        "SELECT chapters_generated_at IS NOT NULL FROM recordings WHERE id = $1::uuid",
    )
    .bind(rec)
    .fetch_one(&app.db)
    .await
    .unwrap();
    set
}

/// `(t_ms, title, source)` por ordem de instante.
async fn stored_chapters(app: &TestApp, rec: &str) -> Vec<(i64, String, String)> {
    sqlx::query_as(
        "SELECT t_ms, title, source FROM recording_chapters
          WHERE recording_id = $1::uuid ORDER BY t_ms",
    )
    .bind(rec)
    .fetch_all(&app.db)
    .await
    .unwrap()
}

#[sqlx::test(migrations = "./migrations")]
async fn bad_answer_fails_without_marking_and_good_answer_keeps_manual(db: sqlx::PgPool) {
    let r = recording(
        db,
        Reply::Answer("Não consegui dividir em capítulos.".into()),
    )
    .await;
    let (app, a) = (&r.app, &r.a);
    let generate = format!("/api/recordings/{}/chapters/generate", r.rec);
    let state = format!("/api/recordings/{}/chapters/generation", r.rec);

    // Sem transcrição: 409, o modelo não é chamado e o estado fica `idle`.
    let (st, e) = app.post(&generate, Some(&a.token), json!({})).await;
    assert_eq!(st, 409, "{e}");
    assert_eq!(r.fake.calls(), 0);
    let (st, g) = app.get(&state, Some(&a.token)).await;
    assert_eq!((st, g["status"].as_str()), (200, Some("idle")), "{g}");
    assert_eq!(g["recording_id"], r.rec.as_str());
    transcribe(app, &r.rec).await;

    // Acesso ao estado: quem só vê não o lê; outra organização nem sabe que existe.
    let (st, e) = app.get(&state, Some(&r.carla.token)).await;
    assert_eq!(st, 403, "{e}");
    let (st, e) = app.get(&state, Some(&r.b.token)).await;
    assert_eq!(st, 404, "{e}");
    let (st, _) = app.get(&state, None).await;
    assert_eq!(st, 401);

    // Um capítulo manual e um automático de uma geração anterior.
    sqlx::query(
        "INSERT INTO recording_chapters (recording_id, t_ms, title, source, created_by)
         VALUES ($1::uuid, 5000, 'Escrito à mão', 'manual', $2::uuid),
                ($1::uuid, 90000, 'Automático antigo', 'auto', NULL)",
    )
    .bind(&r.rec)
    .bind(&a.user_id)
    .execute(&app.db)
    .await
    .unwrap();

    // A resposta sem capítulos utilizáveis: 503, estado `failed` com o código,
    // nada apagado e a gravação NÃO fica marcada como gerada.
    let (st, e) = app.post(&generate, Some(&a.token), json!({})).await;
    assert_eq!(st, 503, "{e}");
    assert_eq!(r.fake.calls(), 1);
    let (_, g) = app.get(&state, Some(&a.token)).await;
    assert_eq!(g["status"], "failed", "{g}");
    assert_eq!(g["error_code"], "ai.bad_response", "{g}");
    assert!(g["finished_at"].is_string(), "{g}");
    assert!(g["chapter_count"].is_null(), "{g}");
    assert!(!generated_at_is_set(app, &r.rec).await);
    assert_eq!(
        stored_chapters(app, &r.rec).await,
        vec![
            (5000, "Escrito à mão".to_string(), "manual".to_string()),
            (90000, "Automático antigo".to_string(), "auto".to_string()),
        ]
    );

    // A resposta boa: os automáticos são substituídos, o manual fica, o
    // capítulo depois do fim sai, e o estado diz quantos ficaram.
    r.fake.set_reply(Reply::Answer(CHAPTERS.into()));
    let (st, list) = app.post(&generate, Some(&a.token), json!({})).await;
    assert_eq!(st, 200, "{list}");
    assert_eq!(list.as_array().map(Vec::len), Some(4), "{list}");
    let (_, g) = app.get(&state, Some(&a.token)).await;
    assert_eq!(g["status"], "succeeded", "{g}");
    assert_eq!(g["chapter_count"], 3, "{g}");
    assert!(g["error_code"].is_null(), "{g}");
    assert!(generated_at_is_set(app, &r.rec).await);
    assert_eq!(
        stored_chapters(app, &r.rec).await,
        vec![
            (0, "Rede de Luanda".to_string(), "auto".to_string()),
            (5000, "Escrito à mão".to_string(), "manual".to_string()),
            (20000, "Troço do Kilamba".to_string(), "auto".to_string()),
            (60000, "Migração".to_string(), "auto".to_string()),
        ]
    );

    // PATCH sem campos (resave, retry) não converte um capítulo automático em
    // manual — só uma correcção de facto o faz (R232).
    let auto = list
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["t_ms"] == 60000)
        .unwrap_or_else(|| panic!("o capítulo dos 60 s não veio na lista: {list}"));
    let one = format!(
        "/api/recordings/{}/chapters/{}",
        r.rec,
        auto["id"].as_str().unwrap()
    );
    let (st, v) = app.patch(&one, Some(&a.token), json!({})).await;
    assert_eq!(st, 200, "{v}");
    assert_eq!(v["source"], "auto", "PATCH vazio não mexe na origem");
    assert_eq!(v["t_ms"], 60000, "nem no resto");
    let (st, v) = app
        .patch(
            &one,
            Some(&a.token),
            json!({"title": "Migração para Outubro"}),
        )
        .await;
    assert_eq!(st, 200, "{v}");
    assert_eq!(v["source"], "manual", "corrigir torna-o manual");
}

#[sqlx::test(migrations = "./migrations")]
async fn running_generation_refuses_a_second_and_a_stale_one_reads_as_interrupted(
    db: sqlx::PgPool,
) {
    let r = recording(db, Reply::Answer(CHAPTERS.into())).await;
    let (app, a) = (&r.app, &r.a);
    let generate = format!("/api/recordings/{}/chapters/generate", r.rec);
    let state = format!("/api/recordings/{}/chapters/generation", r.rec);
    transcribe(app, &r.rec).await;

    // Uma geração a decorrer (outro pedido, ou a varredura): a segunda é 409
    // e o modelo não é chamado.
    sqlx::query(
        "INSERT INTO recording_chapter_generations (recording_id, status)
         VALUES ($1::uuid, 'running')",
    )
    .bind(&r.rec)
    .execute(&app.db)
    .await
    .unwrap();
    let (st, e) = app.post(&generate, Some(&a.token), json!({})).await;
    assert_eq!(st, 409, "{e}");
    assert_eq!(r.fake.calls(), 0);
    let (_, g) = app.get(&state, Some(&a.token)).await;
    assert_eq!(g["status"], "running", "{g}");

    // Sem sinal há tempo demais (o pod morreu a meio): lê-se como falhada com
    // `ai.interrupted`, e pode pedir-se outra vez.
    sqlx::query(
        "UPDATE recording_chapter_generations
            SET started_at = now() - interval '2 hours', updated_at = now() - interval '2 hours'
          WHERE recording_id = $1::uuid",
    )
    .bind(&r.rec)
    .execute(&app.db)
    .await
    .unwrap();
    let (_, g) = app.get(&state, Some(&a.token)).await;
    assert_eq!(g["status"], "failed", "{g}");
    assert_eq!(g["error_code"], "ai.interrupted", "{g}");
    let (st, list) = app.post(&generate, Some(&a.token), json!({})).await;
    assert_eq!(st, 200, "{list}");
    let (_, g) = app.get(&state, Some(&a.token)).await;
    assert_eq!(g["status"], "succeeded", "{g}");
    assert_eq!(g["chapter_count"], 3, "{g}");
}
