//! IA local (Ollama) e sondagem de rede, contra Postgres real e um servidor a
//! sério, com um Ollama FALSO dentro do teste (`common::fake_ollama`):
//!
//! - `GET /api/orgs/{org_id}/ai/status` — o estado honesto em cada caso;
//! - `POST /api/orgs/{org_id}/ai/suggestions` — as três tarefas, termos
//!   inventados descartados, JSON inválido, tecto de tempo, modelo em falta,
//!   `429` por organização, outra organização `404`;
//! - `POST /api/recordings/{id}/chapters/generate` e `…/captions/generate` —
//!   trabalho assíncrono, capítulos `auto` sem tocar nos manuais, a resposta
//!   sem JSON utilizável que NÃO marca a gravação (B12), a tradução com
//!   progresso e o VTT a `409` enquanto gera ou se falhou.
mod common;

use std::time::Duration;

use common::fake_ollama::{FakeOllama, Reply};
use common::{Account, TestApp};
use serde_json::{json, Value};

const MODEL: &str = "qwen2.5:1.5b";

fn segments() -> Value {
    json!([
        {"start_ms": 0, "end_ms": 20000, "text": "Bom dia, tipo, vamos falar da rede de Luanda."},
        {"start_ms": 20000, "end_ms": 60000, "text": "Pá, o troço do Kilamba está, hum, em manutenção."},
        {"start_ms": 60000, "end_ms": 120000, "text": "Decidimos, tipo, adiar a migração para Outubro."}
    ])
}

fn suggestion(task: &str) -> Value {
    json!({"task": task, "language": "pt", "title": "Reunião de rede", "segments": segments()})
}

fn ai_env(url: &str) -> Vec<(&'static str, String)> {
    vec![
        ("OLLAMA_URL", url.to_string()),
        ("OLLAMA_MODEL_STUDIO", MODEL.to_string()),
        ("OLLAMA_MODEL_TRANSLATE", MODEL.to_string()),
        ("OLLAMA_TIMEOUT_SECS", "5".to_string()),
    ]
}

async fn spawn(db: sqlx::PgPool, env: &[(&'static str, String)]) -> TestApp {
    let pairs: Vec<(&str, &str)> = env.iter().map(|(k, v)| (*k, v.as_str())).collect();
    TestApp::spawn_with(db, &pairs).await
}

/// Não há URL do Ollama em nenhuma resposta.
fn no_url_leak(v: &Value, url: &str) {
    assert!(!v.to_string().contains(url), "o URL do Ollama vazou: {v}");
    assert!(
        !v.to_string().contains("127.0.0.1"),
        "um endereço vazou: {v}"
    );
}

// ---------------------------------------------------------------------------
//  Estado
// ---------------------------------------------------------------------------

#[sqlx::test(migrations = "./migrations")]
async fn status_is_honest_in_every_case_and_other_org_is_404(db: sqlx::PgPool) {
    // Sem OLLAMA_URL.
    let app = spawn(db.clone(), &[]).await;
    let a = app.new_org("alfa-ai.test").await;
    let b = app.new_org("beta-ai.test").await;
    let path = format!("/api/orgs/{}/ai/status", a.org());
    let (st, s) = app.get(&path, Some(&a.token)).await;
    assert_eq!(st, 200, "{s}");
    assert_eq!(s["configured"], false);
    assert_eq!(s["reachable"], false);
    assert_eq!(s["ready"], false);
    assert!(s["model_installed"].is_null());
    assert_eq!(s["reason"], "not_configured");
    assert_eq!(
        s["error"],
        "IA local não configurada neste servidor (OLLAMA_URL)"
    );
    // Outra organização e sem sessão.
    let (st, v) = app.get(&path, Some(&b.token)).await;
    assert_eq!(st, 404, "{v}");
    let (st, _) = app.get(&path, None).await;
    assert_eq!(st, 401);
    drop(app);

    // Pronto.
    let fake = FakeOllama::start(&[MODEL], Reply::Answer("x".into())).await;
    let app = spawn(db.clone(), &ai_env(&fake.url)).await;
    let (st, s) = app.get(&path, Some(&a.token)).await;
    assert_eq!(st, 200, "{s}");
    assert_eq!(
        s,
        json!({"configured": true, "reachable": true, "model": MODEL,
               "model_installed": true, "ready": true, "reason": null, "error": null})
    );
    // Modelo em falta.
    fake.set_models(&["llama3:latest"]);
    let (_, s) = app.get(&path, Some(&a.token)).await;
    assert_eq!(s["reachable"], true);
    assert_eq!(s["model_installed"], false);
    assert_eq!(s["ready"], false);
    assert_eq!(s["reason"], "model_missing");
    assert_eq!(
        s["error"],
        format!("o modelo «{MODEL}» não está instalado no Ollama")
    );
    no_url_leak(&s, &fake.url);
    // Tecto de tempo (o estado tem 3 s).
    fake.set_models(&[MODEL]);
    fake.set_tags_delay(Some(Duration::from_secs(5)));
    let t0 = std::time::Instant::now();
    let (st, s) = app.get(&path, Some(&a.token)).await;
    assert_eq!(st, 200, "{s}");
    assert_eq!(s["reason"], "timeout");
    assert!(t0.elapsed() < Duration::from_secs(5), "o estado pendurou");
    drop(app);

    // Serviço que não responde.
    let closed = FakeOllama::closed_url().await;
    let app = spawn(db, &ai_env(&closed)).await;
    let (st, s) = app.get(&path, Some(&a.token)).await;
    assert_eq!(st, 200, "{s}");
    assert_eq!(s["configured"], true);
    assert_eq!(s["reachable"], false);
    assert_eq!(s["reason"], "unreachable");
    assert_eq!(s["error"], "o serviço Ollama não responde");
    no_url_leak(&s, &closed);
}

// ---------------------------------------------------------------------------
//  Sugestões
// ---------------------------------------------------------------------------

#[sqlx::test(migrations = "./migrations")]
async fn suggestions_three_tasks_and_invented_terms_are_dropped(db: sqlx::PgPool) {
    let fake = FakeOllama::start(&[MODEL], Reply::Answer(String::new())).await;
    let app = spawn(db, &ai_env(&fake.url)).await;
    let a = app.new_org("alfa-sug.test").await;
    let path = format!("/api/orgs/{}/ai/suggestions", a.org());

    fake.set_reply(Reply::Answer(
        r###"Claro! {"summary": "## Resumo\n**A equipa** adiou a migração.",
          "chapters": [{"start": "00:00:00", "title": "Rede de Luanda"},
                       {"start": "00:01:00", "title": "Migração"},
                       {"start": "00:30:00", "title": "Depois do fim"}]} Espero que ajude."###
            .into(),
    ));
    let (st, s) = app.post(&path, Some(&a.token), suggestion("summary")).await;
    assert_eq!(st, 200, "{s}");
    assert_eq!(
        s,
        json!({"summary": "Resumo\nA equipa adiou a migração.",
               "chapters": [{"t_ms": 0, "title": "Rede de Luanda"},
                            {"t_ms": 60000, "title": "Migração"}]})
    );
    // O pedido ao modelo: JSON forçado, modelo do Estúdio, transcrição com tempos.
    let sent = fake.prompts().pop().unwrap();
    assert_eq!(sent["format"], "json");
    assert_eq!(sent["model"], MODEL);
    assert!(sent["prompt"]
        .as_str()
        .unwrap()
        .contains("[00:00:20] Pá, o troço do Kilamba"));

    fake.set_reply(Reply::Answer(
        r##"{"title": "Rede de Luanda: migração adiada", "description": "A equipa adiou a migração.",
             "tags": ["#Rede", "luanda", "a,b", "rede"]}"##
            .into(),
    ));
    let (st, p) = app
        .post(&path, Some(&a.token), suggestion("publication"))
        .await;
    assert_eq!(st, 200, "{p}");
    assert_eq!(
        p,
        json!({"title": "Rede de Luanda: migração adiada",
               "description": "A equipa adiou a migração.", "tags": ["rede", "luanda"]})
    );

    fake.set_reply(Reply::Answer(
        r#"{"terms": ["Tipo", "pá", "hum", "basically", "né", "em manutenção"]}"#.into(),
    ));
    let (st, f) = app.post(&path, Some(&a.token), suggestion("fillers")).await;
    assert_eq!(st, 200, "{f}");
    // «basically» e «né» ninguém os disse: saem.
    assert_eq!(f, json!({"terms": ["tipo", "pá", "hum", "em manutenção"]}));
}

#[sqlx::test(migrations = "./migrations")]
async fn suggestions_errors_are_honest_codes_never_500(db: sqlx::PgPool) {
    let fake = FakeOllama::start(&[MODEL], Reply::Answer("Não consigo ajudar.".into())).await;
    let app = spawn(db.clone(), &ai_env(&fake.url)).await;
    let a = app.new_org("alfa-err.test").await;
    let b = app.new_org("beta-err.test").await;
    let path = format!("/api/orgs/{}/ai/suggestions", a.org());

    // Resposta sem JSON utilizável.
    for task in ["summary", "publication", "fillers"] {
        let (st, e) = app.post(&path, Some(&a.token), suggestion(task)).await;
        assert_eq!(st, 503, "{task}: {e}");
        assert_eq!(e["code"], "ai.bad_response");
        no_url_leak(&e, &fake.url);
    }
    // Modelo em falta (o Ollama responde 404 ao generate).
    fake.set_reply(Reply::Status(
        404,
        format!(r#"{{"error":"model \"{MODEL}\" not found, try pulling it first"}}"#),
    ));
    let (st, e) = app.post(&path, Some(&a.token), suggestion("summary")).await;
    assert_eq!(st, 503, "{e}");
    assert_eq!(e["code"], "ai.model_missing");
    assert_eq!(
        e["error"],
        format!("o modelo «{MODEL}» não está instalado no Ollama")
    );
    // Erro do upstream.
    fake.set_reply(Reply::Status(500, r#"{"error":"out of memory"}"#.into()));
    let (st, e) = app.post(&path, Some(&a.token), suggestion("summary")).await;
    assert_eq!((st, e["code"].as_str()), (503, Some("ai.upstream_error")));
    // Tecto de tempo (OLLAMA_TIMEOUT_SECS=5, o mínimo).
    fake.set_reply(Reply::Sleep(Duration::from_secs(8)));
    let t0 = std::time::Instant::now();
    let (st, e) = app.post(&path, Some(&a.token), suggestion("fillers")).await;
    assert_eq!(st, 503, "{e}");
    assert_eq!(e["code"], "ai.timeout");
    assert_eq!(e["error"], "o modelo não respondeu em 5 s");
    assert!(t0.elapsed() < Duration::from_secs(8));

    // Corpo inválido: 400 com código — para quem é membro.
    let cases = [
        (
            json!({"task": "translate", "segments": segments()}),
            "ai.invalid_task",
        ),
        (
            json!({"task": "summary", "segments": []}),
            "ai.invalid_segments",
        ),
        (
            json!({"task": "summary", "language": "ignore all", "segments": segments()}),
            "ai.invalid_language",
        ),
        (
            json!({"task": "summary", "segments": [{"start_ms": 0, "end_ms": 1, "text": "olá"}]}),
            "ai.transcript_too_short",
        ),
        (json!({"segments": "não"}), "ai.invalid_body"),
    ];
    for (body, code) in cases {
        let (st, e) = app.post(&path, Some(&a.token), body).await;
        assert_eq!((st, e["code"].as_str()), (400, Some(code)), "{e}");
    }
    // A pertença decide-se ANTES do corpo: outra org com corpo inválido é 404.
    let (st, e) = app
        .post(&path, Some(&b.token), json!({"task": "nada"}))
        .await;
    assert_eq!(st, 404, "{e}");
    let (st, _) = app.post(&path, Some(&b.token), suggestion("summary")).await;
    assert_eq!(st, 404);
    let (st, _) = app.post(&path, None, suggestion("summary")).await;
    assert_eq!(st, 401);
    drop(app);

    // Sem configuração: 503 honesto, e o modelo nunca é chamado.
    let calls = fake.calls();
    let app = spawn(db, &[]).await;
    let (st, e) = app.post(&path, Some(&a.token), suggestion("summary")).await;
    assert_eq!(st, 503, "{e}");
    assert_eq!(e["code"], "ai.not_configured");
    assert_eq!(fake.calls(), calls);
}

#[sqlx::test(migrations = "./migrations")]
async fn suggestions_are_limited_per_org_and_other_orgs_are_not_blocked(db: sqlx::PgPool) {
    let fake = FakeOllama::start(&[MODEL], Reply::Hold(r#"{"terms": ["tipo"]}"#.into())).await;
    let app = spawn(db, &ai_env(&fake.url)).await;
    let a = app.new_org("alfa-lim.test").await;
    let b = app.new_org("beta-lim.test").await;
    let post = |who: &Account| {
        let (http, url, token) = (
            app.http.clone(),
            app.url(&format!("/api/orgs/{}/ai/suggestions", who.org())),
            who.token.clone(),
        );
        tokio::spawn(async move {
            let r = http
                .post(url)
                .bearer_auth(token)
                .json(&suggestion("fillers"))
                .send()
                .await
                .unwrap();
            r.status().as_u16()
        })
    };
    let first_a = post(&a);
    fake.wait_entered().await;
    // A mesma org, com a vaga ocupada: 429 com Retry-After curto.
    let bearer = format!("Bearer {}", a.token);
    let r = app
        .raw(
            reqwest::Method::POST,
            &format!("/api/orgs/{}/ai/suggestions", a.org()),
            &[("authorization", &bearer)],
            Some(suggestion("fillers")),
        )
        .await;
    assert_eq!(r.status, 429, "{}", r.text);
    assert_eq!(r.json()["code"], "ai.busy");
    assert_eq!(r.header("retry-after").as_deref(), Some("10"));
    // Outra org não espera pela A: o pedido dela chega ao modelo.
    let first_b = post(&b);
    fake.wait_entered().await;
    assert_eq!(fake.calls(), 2);
    fake.release(2);
    assert_eq!(first_a.await.unwrap(), 200);
    assert_eq!(first_b.await.unwrap(), 200);
    // A vaga voltou.
    fake.set_reply(Reply::Answer(r#"{"terms": []}"#.into()));
    let (st, v) = app
        .post(
            &format!("/api/orgs/{}/ai/suggestions", a.org()),
            Some(&a.token),
            suggestion("fillers"),
        )
        .await;
    assert_eq!(st, 200, "{v}");
}

// ---------------------------------------------------------------------------
//  Gravações: capítulos e legendas gerados
// ---------------------------------------------------------------------------

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
    let app = spawn(db, &ai_env(&fake.url)).await;
    let a = app.new_org("alfa-rec.test").await;
    let b = app.new_org("beta-rec.test").await;
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

async fn transcribe(app: &TestApp, rec: &str, language: &str) {
    sqlx::query(
        "UPDATE recordings SET transcribed_at = now(), transcript = 'rede de Luanda',
                transcript_language = $2, duration_ms = 180000, transcript_segments = $3
          WHERE id = $1::uuid",
    )
    .bind(rec)
    .bind(language)
    .bind(segments())
    .execute(&app.db)
    .await
    .unwrap();
}

/// Lê `path` até `pred` ser verdade (no máximo ~20 s).
async fn poll(app: &TestApp, path: &str, token: &str, pred: impl Fn(&Value) -> bool) -> Value {
    for _ in 0..200 {
        let (st, v) = app.get(path, Some(token)).await;
        assert_eq!(st, 200, "{path}: {v}");
        if pred(&v) {
            return v;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!("{path} não chegou ao estado esperado");
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

const CHAPTERS: &str = r#"{"chapters": [
    {"start": "00:00:00", "title": "Rede de Luanda"},
    {"start": "00:00:20", "title": "Troço do Kilamba"},
    {"start": "00:01:00", "title": "Migração"},
    {"start": "05:00:00", "title": "Depois do fim"}]}"#;

#[sqlx::test(migrations = "./migrations")]
async fn chapters_generate_is_async_keeps_manual_and_bad_json_does_not_mark(db: sqlx::PgPool) {
    let r = recording(
        db,
        Reply::Answer("Não consegui dividir em capítulos.".into()),
    )
    .await;
    let (app, a) = (&r.app, &r.a);
    let generate = format!("/api/recordings/{}/chapters/generate", r.rec);
    let state = format!("/api/recordings/{}/chapters/generation", r.rec);
    let chapters = format!("/api/recordings/{}/chapters", r.rec);

    // Sem transcrição: 409 com código, e o modelo não é chamado.
    let (st, e) = app.post(&generate, Some(&a.token), json!({})).await;
    assert_eq!(
        (st, e["code"].as_str()),
        (409, Some("recording.no_transcript")),
        "{e}"
    );
    assert_eq!(r.fake.calls(), 0);
    let (st, g) = app.get(&state, Some(&a.token)).await;
    assert_eq!((st, g["status"].as_str()), (200, Some("idle")), "{g}");
    transcribe(app, &r.rec, "pt").await;

    // Acesso: quem só vê 403; outra org 404 (também no estado).
    let (st, e) = app.post(&generate, Some(&r.carla.token), json!({})).await;
    assert_eq!(
        (st, e["code"].as_str()),
        (403, Some("recording.not_manager")),
        "{e}"
    );
    for p in [&generate, &state] {
        let (st, _) = if p == &generate {
            app.post(p, Some(&r.b.token), json!({})).await
        } else {
            app.get(p, Some(&r.b.token)).await
        };
        assert_eq!(st, 404, "{p}");
    }

    // B12: a resposta sem JSON utilizável falha e NÃO marca a gravação.
    let bearer = format!("Bearer {}", a.token);
    let res = app
        .raw(
            reqwest::Method::POST,
            &generate,
            &[("authorization", &bearer)],
            None,
        )
        .await;
    assert_eq!(res.status, 202, "{}", res.text);
    assert_eq!(res.header("location").as_deref(), Some(state.as_str()));
    assert_eq!(res.json()["status"], "running");
    let g = poll(app, &state, &a.token, |g| g["status"] != "running").await;
    assert_eq!(g["status"], "failed", "{g}");
    assert_eq!(g["error_code"], "ai.bad_response");
    assert!(g["finished_at"].is_string());
    assert!(
        !generated_at_is_set(app, &r.rec).await,
        "B12: a falha marcou a gravação"
    );
    let (_, list) = app.get(&chapters, Some(&a.token)).await;
    assert_eq!(list, json!([]));

    // Um manual no instante que o modelo vai propor.
    let (st, manual) = app
        .post(
            &chapters,
            Some(&a.token),
            json!({"t_ms": 20000, "title": "Escrito à mão"}),
        )
        .await;
    assert_eq!(st, 201, "{manual}");

    // Sucesso: os automáticos entram, o manual fica intacto, o de depois do fim sai.
    r.fake.set_reply(Reply::Answer(CHAPTERS.into()));
    let (st, g) = app.post(&generate, Some(&a.token), json!({})).await;
    assert_eq!(st, 202, "{g}");
    let g = poll(app, &state, &a.token, |g| g["status"] != "running").await;
    assert_eq!(g["status"], "succeeded", "{g}");
    assert_eq!(g["chapter_count"], 2);
    assert!(g["error_code"].is_null());
    assert!(generated_at_is_set(app, &r.rec).await);
    let (_, list) = app.get(&chapters, Some(&a.token)).await;
    let got: Vec<(i64, &str, &str)> = list
        .as_array()
        .unwrap()
        .iter()
        .map(|c| {
            (
                c["t_ms"].as_i64().unwrap(),
                c["title"].as_str().unwrap(),
                c["source"].as_str().unwrap(),
            )
        })
        .collect();
    assert_eq!(
        got,
        [
            (0, "Rede de Luanda", "auto"),
            (20000, "Escrito à mão", "manual"),
            (60000, "Migração", "auto"),
        ]
    );
    // Quem só vê a gravação vê os capítulos (mas não o estado do trabalho).
    let (st, _) = app.get(&chapters, Some(&r.carla.token)).await;
    assert_eq!(st, 200);
    let (st, _) = app.get(&state, Some(&r.carla.token)).await;
    assert_eq!(st, 403);

    // Voltar a gerar substitui só os automáticos.
    r.fake.set_reply(Reply::Answer(
        r#"{"chapters": [{"start": 90, "title": "Só um"}]}"#.into(),
    ));
    app.post(&generate, Some(&a.token), json!({})).await;
    poll(app, &state, &a.token, |g| {
        g["status"] == "succeeded" && g["chapter_count"] == 1
    })
    .await;
    let (_, list) = app.get(&chapters, Some(&a.token)).await;
    let titles: Vec<&str> = list
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["title"].as_str().unwrap())
        .collect();
    assert_eq!(titles, ["Escrito à mão", "Só um"]);

    // Uma geração a correr não se atropela; e a vaga da org fica ocupada.
    r.fake.set_reply(Reply::Hold(CHAPTERS.into()));
    let (st, _) = app.post(&generate, Some(&a.token), json!({})).await;
    assert_eq!(st, 202);
    r.fake.wait_entered().await;
    let (st, e) = app.post(&generate, Some(&a.token), json!({})).await;
    assert_eq!(
        (st, e["code"].as_str()),
        (409, Some("recording.chapter_generation_running")),
        "{e}"
    );
    let (st, e) = app
        .post(
            &format!("/api/orgs/{}/ai/suggestions", a.org()),
            Some(&a.token),
            suggestion("fillers"),
        )
        .await;
    assert_eq!((st, e["code"].as_str()), (429, Some("ai.busy")), "{e}");
    r.fake.release(1);
    poll(app, &state, &a.token, |g| g["status"] == "succeeded").await;

    // Modelo em falta: 503 antes de aceitar (nada fica `running`).
    r.fake.set_models(&["outro:latest"]);
    let (st, e) = app.post(&generate, Some(&a.token), json!({})).await;
    assert_eq!(
        (st, e["code"].as_str()),
        (503, Some("ai.model_missing")),
        "{e}"
    );
    let (_, g) = app.get(&state, Some(&a.token)).await;
    assert_eq!(g["status"], "succeeded");
}

#[sqlx::test(migrations = "./migrations")]
async fn chapter_generation_interrupted_is_shown_as_failed_and_can_restart(db: sqlx::PgPool) {
    let r = recording(db, Reply::Answer(CHAPTERS.into())).await;
    let (app, a) = (&r.app, &r.a);
    transcribe(app, &r.rec, "pt").await;
    // Um trabalho que ficou `running` quando o pod morreu.
    sqlx::query(
        "INSERT INTO recording_chapter_generations (recording_id, status, started_at, updated_at)
         VALUES ($1::uuid, 'running', now() - interval '1 hour', now() - interval '1 hour')",
    )
    .bind(&r.rec)
    .execute(&app.db)
    .await
    .unwrap();
    let state = format!("/api/recordings/{}/chapters/generation", r.rec);
    let (_, g) = app.get(&state, Some(&a.token)).await;
    assert_eq!(g["status"], "failed", "{g}");
    assert_eq!(g["error_code"], "ai.interrupted");
    let (st, _) = app
        .post(
            &format!("/api/recordings/{}/chapters/generate", r.rec),
            Some(&a.token),
            json!({}),
        )
        .await;
    assert_eq!(st, 202);
    poll(app, &state, &a.token, |g| g["status"] == "succeeded").await;
}

#[sqlx::test(migrations = "./migrations")]
async fn captions_generate_same_language_translation_and_vtt_409(db: sqlx::PgPool) {
    let r = recording(
        db,
        Reply::Translate {
            prefix: "EN: ".into(),
            fail_at: None,
        },
    )
    .await;
    let (app, a) = (&r.app, &r.a);
    let generate = format!("/api/recordings/{}/captions/generate", r.rec);
    let bearer = format!("Bearer {}", a.token);
    let vtt = |lang: &str| format!("/api/recordings/{}/captions/{lang}/vtt", r.rec);
    let get_vtt = |lang: String| {
        let bearer = bearer.clone();
        async move {
            app.raw(
                reqwest::Method::GET,
                &lang,
                &[("authorization", &bearer)],
                None,
            )
            .await
        }
    };

    let (st, e) = app.post(&generate, Some(&a.token), json!({})).await;
    assert_eq!(
        (st, e["code"].as_str()),
        (409, Some("recording.no_transcript")),
        "{e}"
    );
    transcribe(app, &r.rec, "pt").await;

    // Acesso.
    let (st, e) = app.post(&generate, Some(&r.carla.token), json!({})).await;
    assert_eq!(
        (st, e["code"].as_str()),
        (403, Some("recording.not_manager")),
        "{e}"
    );
    let (st, _) = app.post(&generate, Some(&r.b.token), json!({})).await;
    assert_eq!(st, 404);

    // Na língua da transcrição: já, dos segmentos, sem chamar o modelo.
    let res = app
        .raw(
            reqwest::Method::POST,
            &generate,
            &[("authorization", &bearer)],
            None,
        )
        .await;
    assert_eq!(res.status, 201, "{}", res.text);
    let pt = format!("/api/recordings/{}/captions/pt", r.rec);
    assert_eq!(res.header("location").as_deref(), Some(pt.as_str()));
    let c = res.json();
    assert_eq!(
        (c["status"].as_str(), c["source"].as_str()),
        (Some("draft"), Some("transcript"))
    );
    assert_eq!(r.fake.calls(), 0);
    let v = get_vtt(vtt("pt")).await;
    assert_eq!(v.status, 200, "{}", v.text);
    assert!(v.text.starts_with("WEBVTT"));
    assert!(v
        .text
        .contains("00:00:20.000 --> 00:01:00.000\nPá, o troço do Kilamba"));
    // Já existe: não se apaga o trabalho de ninguém sem `replace`.
    let (st, e) = app
        .post(&generate, Some(&a.token), json!({"lang": "pt"}))
        .await;
    assert_eq!(
        (st, e["code"].as_str()),
        (409, Some("recording.caption_exists")),
        "{e}"
    );
    let (st, c) = app
        .post(
            &generate,
            Some(&a.token),
            json!({"lang": "pt", "replace": true}),
        )
        .await;
    assert_eq!(st, 200, "{c}");

    // Língua sem suporte e língua inválida.
    let (st, e) = app
        .post(&generate, Some(&a.token), json!({"lang": "umb"}))
        .await;
    assert_eq!(
        (st, e["code"].as_str()),
        (400, Some("ai.unsupported_language")),
        "{e}"
    );
    let (st, e) = app
        .post(&generate, Some(&a.token), json!({"lang": "PT_pt"}))
        .await;
    assert_eq!(
        (st, e["code"].as_str()),
        (400, Some("recording.invalid_caption_lang")),
        "{e}"
    );

    // Noutra língua: 202, a gerar; o VTT dá 409 enquanto gera.
    r.fake.set_reply(Reply::Hold("EN: held".into()));
    let res = app
        .raw(
            reqwest::Method::POST,
            &generate,
            &[("authorization", &bearer)],
            Some(json!({"lang": "en"})),
        )
        .await;
    assert_eq!(res.status, 202, "{}", res.text);
    let en = format!("/api/recordings/{}/captions/en", r.rec);
    assert_eq!(res.header("location").as_deref(), Some(en.as_str()));
    let c = res.json();
    assert_eq!(c["status"], "generating");
    assert_eq!(c["source"], "translation");
    assert_eq!(c["progress_pct"], 0);
    r.fake.wait_entered().await;
    let v = get_vtt(vtt("en")).await;
    assert_eq!(v.status, 409, "{}", v.text);
    assert_eq!(v.json()["code"], "recording.caption_not_ready");
    let (st, e) = app
        .post(&generate, Some(&a.token), json!({"lang": "en"}))
        .await;
    assert_eq!(
        (st, e["code"].as_str()),
        (409, Some("recording.caption_generation_running")),
        "{e}"
    );
    // Quem só vê não vê uma legenda a gerar.
    let (st, _) = app.get(&en, Some(&r.carla.token)).await;
    assert_eq!(st, 404);
    r.fake.set_reply(Reply::Translate {
        prefix: "EN: ".into(),
        fail_at: None,
    });
    r.fake.release(1);
    let c = poll(app, &en, &a.token, |c| c["status"] != "generating").await;
    assert_eq!(c["status"], "draft", "{c}");
    assert!(c["progress_pct"].is_null());
    let v = get_vtt(vtt("en")).await;
    assert_eq!(v.status, 200, "{}", v.text);
    assert!(v.text.contains("\nEN: held\n"), "{}", v.text);
    assert!(
        v.text
            .contains("\nEN: Decidimos, tipo, adiar a migração para Outubro.\n"),
        "{}",
        v.text
    );
    let sent = r.fake.prompts();
    assert!(
        sent.iter().all(|p| p["format"].is_null()),
        "a tradução não pede JSON"
    );

    // Uma linha que o modelo não traduz faz falhar a legenda inteira.
    r.fake.set_reply(Reply::Translate {
        prefix: "FR: ".into(),
        fail_at: Some(r.fake.calls() + 2),
    });
    let (st, _) = app
        .post(&generate, Some(&a.token), json!({"lang": "fr"}))
        .await;
    assert_eq!(st, 202);
    let fr = format!("/api/recordings/{}/captions/fr", r.rec);
    let c = poll(app, &fr, &a.token, |c| c["status"] != "generating").await;
    assert_eq!(c["status"], "failed", "{c}");
    assert_eq!(
        c["error"],
        "o modelo devolveu uma resposta que não se consegue usar (segmento 2 de 3)"
    );
    let v = get_vtt(vtt("fr")).await;
    assert_eq!(v.status, 409, "{}", v.text);
    assert_eq!(v.json()["code"], "recording.caption_not_ready");
    // Uma falhada volta a pedir-se sem `replace`.
    r.fake.set_reply(Reply::Translate {
        prefix: "FR: ".into(),
        fail_at: None,
    });
    let (st, _) = app
        .post(&generate, Some(&a.token), json!({"lang": "fr"}))
        .await;
    assert_eq!(st, 202);
    poll(app, &fr, &a.token, |c| c["status"] == "draft").await;

    // Modelo em falta: 503 antes de aceitar.
    r.fake.set_models(&[]);
    let (st, e) = app
        .post(&generate, Some(&a.token), json!({"lang": "de"}))
        .await;
    assert_eq!(
        (st, e["code"].as_str()),
        (503, Some("ai.model_missing")),
        "{e}"
    );
    let (st, _) = app
        .get(
            &format!("/api/recordings/{}/captions/de", r.rec),
            Some(&a.token),
        )
        .await;
    assert_eq!(st, 404, "nada fica criado");
}
