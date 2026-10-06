//! Gravações G4–G6 contra Postgres real: metadados e estado derivado,
//! capítulos e comentários, pesquisa na transcrição — e a regra de acesso
//! única (S3: um membro arquivado perde tudo de uma vez).
//!
//! As gravações entram por SQL e SEM ficheiro (`insert_recording`), excepto no
//! teste do upload, que prova que o ficheiro vai para `config.recordings_dir`.
mod common;

use common::{Account, TestApp, INVENTED_ID};
use serde_json::{json, Value};

struct Fixture {
    app: TestApp,
    /// Admin da org A e dono da gravação.
    a: Account,
    /// Membro da A que participou na sala.
    carla: Account,
    /// Membro da A que NÃO participou.
    duarte: Account,
    /// Admin da A que não participou.
    eva: Account,
    /// Admin de outra org.
    b: Account,
    room_id: String,
    rec: String,
}

async fn fixture(db: sqlx::PgPool) -> Fixture {
    let app = TestApp::spawn(db).await;
    let a = app.new_org("alfa.test").await;
    let b = app.new_org("beta.test").await;
    let carla = app.add_member(&a, "carla", "member").await;
    let duarte = app.add_member(&a, "duarte", "member").await;
    let eva = app.add_member(&a, "eva", "admin").await;
    let room = app.new_room(&a, "gravada").await;
    let room_id = room["id"].as_str().unwrap().to_string();
    let rec = app.insert_recording(&room_id, &a.user_id).await;
    for who in [&a, &carla] {
        participate(&app, &room_id, &who.user_id).await;
    }
    Fixture {
        app,
        a,
        carla,
        duarte,
        eva,
        b,
        room_id,
        rec,
    }
}

async fn participate(app: &TestApp, room_id: &str, user_id: &str) {
    sqlx::query("INSERT INTO room_participants (room_id, user_id) VALUES ($1::uuid, $2::uuid)")
        .bind(room_id)
        .bind(user_id)
        .execute(&app.db)
        .await
        .unwrap();
}

async fn sql(app: &TestApp, q: &str, rec: &str) {
    sqlx::query(q).bind(rec).execute(&app.db).await.unwrap();
}

/// As linhas de uma resposta de listagem, seja ela a LISTA (o que a biblioteca
/// devolve sem `page_size`/`page_token`, também com `q` e `scope`) ou a PÁGINA
/// (`{items, next_page_token}`). A forma exacta de cada uma é afirmada onde
/// importa, em `library_keeps_bare_array_and_paginates_on_request`.
fn items(v: &Value) -> &Vec<Value> {
    if let Some(a) = v.as_array() {
        return a;
    }
    v["items"]
        .as_array()
        .unwrap_or_else(|| panic!("sem items: {v}"))
}

// ---------------------------------------------------------------------------
//  G4 — metadados e estado
// ---------------------------------------------------------------------------

#[sqlx::test(migrations = "./migrations")]
async fn state_and_transcript_status_are_derived_for_each_state(db: sqlx::PgPool) {
    let f = fixture(db).await;
    let (app, a) = (&f.app, &f.a);
    // Os metadados são o próprio recurso (o ficheiro está em `/content`).
    let meta = format!("/api/recordings/{}", f.rec);

    let (st, m) = app.get(&meta, Some(&a.token)).await;
    assert_eq!(st, 200, "{m}");
    assert_eq!(m["status"], "ready");
    assert_eq!(
        m["state"], "ready",
        "não publicada: o estado é o do ficheiro"
    );
    assert_eq!(m["transcript_status"], "none");
    assert_eq!(m["kind"], "meeting");
    assert_eq!(m["visibility"], "private");
    assert!(m["published_at"].is_null());
    assert_eq!(m["description"], "");
    assert_eq!(m["tags"], json!([]));
    // `null` é «não foi possível medir» — nunca um valor inventado.
    assert!(m["duration_ms"].is_null() && m["width"].is_null());
    assert_eq!(m["can_manage"], true);
    assert!(m.get("snippet").is_none());
    // Contagens: a gravação acabou de nascer.
    for c in ["chapter_count", "comment_count", "view_count"] {
        assert_eq!(m[c], 0, "{c}");
    }
    assert_eq!(m["caption_languages"], json!([]));

    // O estado do FICHEIRO e o da TRANSCRIÇÃO são dois eixos, não um só.
    let cases = [
        (
            "UPDATE recordings SET transcription_lease_token = 't', transcription_lease_expires_at = now() + interval '1 hour' WHERE id = $1::uuid",
            "transcribing",
            "transcribing",
        ),
        (
            // Uma reserva expirada é trabalho devolvido à fila, não «a transcrever».
            "UPDATE recordings SET transcription_lease_expires_at = now() - interval '1 minute' WHERE id = $1::uuid",
            "ready",
            "none",
        ),
        (
            "UPDATE recordings SET transcription_failed_at = now() WHERE id = $1::uuid",
            "ready",
            "failed",
        ),
        (
            "UPDATE recordings SET transcribed_at = now() WHERE id = $1::uuid",
            "ready",
            "ready",
        ),
        (
            "UPDATE recordings SET status = 'failed', failure_reason = 'sem espaço' WHERE id = $1::uuid",
            "failed",
            "none",
        ),
    ];
    for (update, file_state, transcript) in cases {
        sql(app, update, &f.rec).await;
        let (_, m) = app.get(&meta, Some(&a.token)).await;
        assert_eq!(m["status"], file_state, "{update}: {m}");
        assert_eq!(m["transcript_status"], transcript, "{update}: {m}");
        // A biblioteca diz o mesmo.
        let (_, lib) = app.get("/api/recordings", Some(&a.token)).await;
        assert_eq!(lib[0]["status"], file_state);
        assert_eq!(lib[0]["transcript_status"], transcript);
    }
}

/// A linha de uma gravação do servidor nasce em `processing` ANTES de o ffmpeg
/// correr (`recorder::insert_processing`), e passa a `ready` quando a
/// composição acaba. Entre uma coisa e outra a API tem de dizer «a compor»,
/// com o progresso — dizia `failed` sem causa, e quem parava uma gravação via-a
/// falhada até a composição acabar.
#[sqlx::test(migrations = "./migrations")]
async fn a_recording_being_composed_is_processing_not_failed(db: sqlx::PgPool) {
    let f = fixture(db).await;
    let (app, a) = (&f.app, &f.a);
    // As colunas e os valores com que o `recorder::insert_processing` a cria.
    let (id,): (uuid::Uuid,) = sqlx::query_as(
        "INSERT INTO recordings (room_id, uploader_id, filename, size_bytes, status,
                                 progress_pct, progress_at, kind)
         VALUES ($1::uuid, $2::uuid, 'a compor.webm', 0, 'processing', 0, now(), 'meeting')
         RETURNING id",
    )
    .bind(&f.room_id)
    .bind(&a.user_id)
    .fetch_one(&app.db)
    .await
    .unwrap();
    let rec = id.to_string();
    let meta = format!("/api/recordings/{rec}");
    let in_library = |lib: &Value| -> Value {
        items(lib)
            .iter()
            .find(|r| r["id"] == rec.as_str())
            .unwrap_or_else(|| panic!("a gravação a compor não está na biblioteca: {lib}"))
            .clone()
    };

    // As três leituras dizem o mesmo: o recurso, o `/details` e a biblioteca.
    let (st, m) = app.get(&meta, Some(&a.token)).await;
    assert_eq!(st, 200, "{m}");
    let (_, d) = app.get(&format!("{meta}/details"), Some(&a.token)).await;
    let (_, lib) = app.get("/api/recordings", Some(&a.token)).await;
    for (onde, r) in [
        ("recurso", m),
        ("details", d),
        ("biblioteca", in_library(&lib)),
    ] {
        assert_eq!(r["status"], "processing", "{onde}: {r}");
        assert_eq!(r["state"], "processing", "{onde}: {r}");
        assert_eq!(r["progress_pct"], 0, "{onde}: {r}");
        assert!(r["failure_reason"].is_null(), "{onde}: {r}");
        assert_eq!(r["transcript_status"], "none", "{onde}: {r}");
    }

    // O progresso que o gravador vai escrevendo chega a quem lê.
    sql(
        app,
        "UPDATE recordings SET progress_pct = 42, progress_at = now() WHERE id = $1::uuid",
        &rec,
    )
    .await;
    let (_, lib) = app.get("/api/recordings", Some(&a.token)).await;
    assert_eq!(in_library(&lib)["progress_pct"], 42);

    // Ainda não há ficheiro, e a recusa não diz que «falhou» (R59: nada se
    // oferece sobre uma gravação que não está pronta).
    let (st, e) = app.get(&format!("{meta}/content"), Some(&a.token)).await;
    assert_eq!(st, 409, "{e}");
    assert_eq!(e["code"], "recording.processing", "{e}");
    assert!(
        !e["error"].as_str().unwrap_or_default().contains("falhou"),
        "{e}"
    );
    // O `409` é só para quem chega à gravação. Quem não chega — outra
    // organização, ou um colega sem relação com ela — recebe o `404` de «não
    // existe», como antes: o estado não confirma que o id existe.
    for (quem, conta) in [
        ("outra organização", &f.b),
        ("colega sem relação", &f.duarte),
    ] {
        let (st, e) = app
            .get(&format!("{meta}/content"), Some(&conta.token))
            .await;
        assert_eq!(st, 404, "{quem}: {e}");
        let (st, e) = app.get(&meta, Some(&conta.token)).await;
        assert_eq!(st, 404, "{quem}: {e}");
    }
    let (st, e) = app
        .post(
            &format!("{meta}/publish"),
            Some(&a.token),
            json!({"visibility": "org"}),
        )
        .await;
    assert_eq!(st, 409, "{e}");
    let (st, e) = app
        .post(&format!("{meta}/views"), Some(&a.token), json!({}))
        .await;
    assert_eq!(st, 409, "{e}");

    // Publicada à força (por SQL): continua a compor, não «publicada».
    sql(
        app,
        "UPDATE recordings SET visibility = 'org', published_at = now() WHERE id = $1::uuid",
        &rec,
    )
    .await;
    let (_, m) = app.get(&meta, Some(&a.token)).await;
    assert_eq!(m["state"], "processing", "{m}");

    // Uma falha a sério continua a ler-se como antes: `failed`, com a causa,
    // e o `400` do ficheiro traz essa causa.
    sql(
        app,
        "UPDATE recordings SET status = 'failed', failure_reason = 'sem espaço',
                progress_pct = NULL, progress_at = NULL WHERE id = $1::uuid",
        &rec,
    )
    .await;
    let (_, m) = app.get(&meta, Some(&a.token)).await;
    assert_eq!(m["status"], "failed", "{m}");
    assert_eq!(m["state"], "failed", "{m}");
    assert_eq!(m["failure_reason"], "sem espaço", "{m}");
    assert!(m["progress_pct"].is_null(), "{m}");
    let (st, e) = app.get(&format!("{meta}/content"), Some(&a.token)).await;
    assert_eq!(st, 400, "{e}");
    assert_eq!(e["error"], "sem espaço", "{e}");
}

/// O `state` da UI é o do ficheiro, com `published` quando está pronta E
/// publicada. Publicar uma gravação FALHADA não a promove: não há o que ver.
#[sqlx::test(migrations = "./migrations")]
async fn state_says_published_only_when_ready_and_published(db: sqlx::PgPool) {
    let f = fixture(db).await;
    let (app, a) = (&f.app, &f.a);
    let meta = format!("/api/recordings/{}", f.rec);

    let (st, _) = app
        .post(
            &format!("/api/recordings/{}/publish", f.rec),
            Some(&a.token),
            json!({"visibility": "org"}),
        )
        .await;
    assert_eq!(st, 200);
    let (_, m) = app.get(&meta, Some(&a.token)).await;
    assert_eq!(m["state"], "published");
    assert_eq!(m["status"], "ready", "o ficheiro continua pronto");
    assert_eq!(m["visibility"], "org");
    assert!(m["published_at"].is_string());

    sql(
        app,
        "UPDATE recordings SET status = 'failed' WHERE id = $1::uuid",
        &f.rec,
    )
    .await;
    let (_, m) = app.get(&meta, Some(&a.token)).await;
    assert_eq!(m["state"], "failed", "publicada mas sem ficheiro: falhada");
}

#[sqlx::test(migrations = "./migrations")]
async fn patch_metadata_owner_admin_member_and_other_org(db: sqlx::PgPool) {
    let f = fixture(db).await;
    let app = &f.app;
    let path = format!("/api/recordings/{}", f.rec);

    let (st, m) = app
        .patch(
            &path,
            Some(&f.a.token),
            json!({
                "filename": "  Aula de Química ",
                "description": " Revisão do trimestre ",
                "tags": ["#Química", "quimica", " ", "Revisão"],
                "kind": "training",
            }),
        )
        .await;
    assert_eq!(st, 200, "{m}");
    assert_eq!(m["filename"], "Aula de Química");
    assert_eq!(m["description"], "Revisão do trimestre");
    assert_eq!(
        m["tags"],
        json!(["química", "quimica", "revisão"]),
        "apara, tira o cardinal, baixa a caixa, e não repete"
    );
    assert_eq!(m["kind"], "training");

    // Valida tudo antes de escrever.
    let (st, v) = app
        .patch(&path, Some(&f.a.token), json!({"kind": "podcast"}))
        .await;
    assert_eq!(st, 400);
    assert_eq!(v["code"], "recording.invalid_kind");
    let (st, v) = app
        .patch(
            &path,
            Some(&f.a.token),
            json!({"filename": "x".repeat(201)}),
        )
        .await;
    assert_eq!(st, 400);
    assert_eq!(v["code"], "recording.invalid_filename");
    let (st, v) = app
        .patch(&path, Some(&f.a.token), json!({"tags": ["a,b"]}))
        .await;
    assert_eq!(st, 400);
    assert_eq!(v["code"], "recording.invalid_tags");
    // O contrato antigo (`title`/`category`) é recusado em voz alta, não
    // aceite e ignorado em silêncio.
    let (st, v) = app
        .patch(&path, Some(&f.a.token), json!({"title": "Aula"}))
        .await;
    assert_eq!(st, 422, "{v}");
    assert!(
        v["error"]
            .as_str()
            .unwrap()
            .contains("unknown field `title`"),
        "a recusa diz QUAL o campo e quais os válidos: {v}"
    );
    let (_, m) = app.get(&path, Some(&f.a.token)).await;
    assert_eq!(m["filename"], "Aula de Química", "sem escrita parcial");

    // Participante que não é dono: vê, não gere.
    let (st, v) = app
        .patch(&path, Some(&f.carla.token), json!({"kind": "meeting"}))
        .await;
    assert_eq!(st, 403, "{v}");
    assert_eq!(v["code"], "recording.not_manager");
    let (_, m) = app.get(&path, Some(&f.carla.token)).await;
    assert_eq!(m["can_manage"], false);
    // Membro da mesma org sem acesso nenhum: nem sabe que existe.
    let (st, _) = app
        .patch(&path, Some(&f.duarte.token), json!({"kind": "meeting"}))
        .await;
    assert_eq!(st, 404);
    let (st, _) = app.get(&path, Some(&f.duarte.token)).await;
    assert_eq!(st, 404);
    // Outra org: 404, igual a um id inventado.
    let (st, v) = app
        .patch(&path, Some(&f.b.token), json!({"kind": "meeting"}))
        .await;
    assert_eq!(st, 404);
    assert!(!v.to_string().contains("Química"));
    let (st, v) = app.get(&path, Some(&f.b.token)).await;
    assert_eq!(st, 404, "metadados para outra org: {v}");
    assert!(!v.to_string().contains("Química"));
    let (st, _) = app
        .patch(
            &format!("/api/recordings/{INVENTED_ID}"),
            Some(&f.a.token),
            json!({"kind": "meeting"}),
        )
        .await;
    assert_eq!(st, 404);
    let (st, _) = app.patch(&path, None, json!({"kind": "meeting"})).await;
    assert_eq!(st, 401);

    // Admin activo da org do dono, sem ter participado: gere (é quem descarrega).
    let (st, m) = app
        .patch(
            &path,
            Some(&f.eva.token),
            json!({"kind": "broadcast", "description": ""}),
        )
        .await;
    assert_eq!(st, 200, "{m}");
    assert_eq!(m["kind"], "broadcast");
    assert_eq!(m["description"], "", "\"\" apaga a descrição");
    let kind: String = sqlx::query_scalar("SELECT kind FROM recordings WHERE id = $1::uuid")
        .bind(&f.rec)
        .fetch_one(&app.db)
        .await
        .unwrap();
    assert_eq!(kind, "broadcast");
}

// ---------------------------------------------------------------------------
//  G5 — capítulos
// ---------------------------------------------------------------------------

#[sqlx::test(migrations = "./migrations")]
async fn chapters_crud_bounds_and_access(db: sqlx::PgPool) {
    let f = fixture(db).await;
    let app = &f.app;
    sql(
        app,
        "UPDATE recordings SET duration_ms = 600000 WHERE id = $1::uuid",
        &f.rec,
    )
    .await;
    let base = format!("/api/recordings/{}/chapters", f.rec);

    let res = app
        .http
        .post(app.url(&base))
        .bearer_auth(&f.a.token)
        .json(&json!({"t_ms": 300_000, "title": "Orçamento"}))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 201);
    let location = res.headers()["location"].to_str().unwrap().to_string();
    let second: Value = res.json().await.unwrap();
    assert_eq!(
        location,
        format!("{base}/{}", second["id"].as_str().unwrap())
    );
    let (st, first) = app
        .post(
            &base,
            Some(&f.a.token),
            json!({"t_ms": 0, "title": "Abertura"}),
        )
        .await;
    assert_eq!(st, 201, "{first}");
    let (st, one) = app.get(&location, Some(&f.carla.token)).await;
    assert_eq!(st, 200);
    assert_eq!(one["title"], "Orçamento");

    // Limites.
    for (body, code) in [
        (
            json!({"t_ms": 600_001, "title": "x"}),
            "recording.invalid_timestamp",
        ),
        (
            json!({"t_ms": -1, "title": "x"}),
            "recording.invalid_timestamp",
        ),
        (
            json!({"t_ms": 600_000, "title": " "}),
            "recording.invalid_chapter_title",
        ),
        (
            json!({"t_ms": 1, "title": "x".repeat(201)}),
            "recording.invalid_chapter_title",
        ),
    ] {
        let (st, v) = app.post(&base, Some(&f.a.token), body.clone()).await;
        assert_eq!(st, 400, "{body}: {v}");
        assert_eq!(v["code"], code);
    }
    let (st, _) = app
        .post(
            &base,
            Some(&f.a.token),
            json!({"t_ms": 600_000, "title": "Fim"}),
        )
        .await;
    assert_eq!(st, 201, "a duração é inclusiva");

    // Ordem por marca temporal, paginada.
    let (st, p) = app
        .get(&format!("{base}?page_size=2"), Some(&f.carla.token))
        .await;
    assert_eq!(st, 200, "{p}");
    assert_eq!(items(&p)[0]["title"], "Abertura");
    assert_eq!(items(&p)[1]["title"], "Orçamento");
    let token = p["next_page_token"].as_str().unwrap();
    let (_, p2) = app
        .get(
            &format!("{base}?page_size=2&page_token={token}"),
            Some(&f.carla.token),
        )
        .await;
    assert_eq!(items(&p2).len(), 1);
    assert_eq!(items(&p2)[0]["title"], "Fim");
    assert!(p2.get("next_page_token").is_none());

    // Participante lê, não escreve; outra org e membro sem acesso: 404.
    let (st, v) = app
        .post(
            &base,
            Some(&f.carla.token),
            json!({"t_ms": 5_000, "title": "x"}),
        )
        .await;
    assert_eq!(st, 403, "{v}");
    let (st, _) = app.delete(&location, Some(&f.carla.token)).await;
    assert_eq!(st, 403);
    for who in [&f.b, &f.duarte] {
        let (st, v) = app.get(&base, Some(&who.token)).await;
        assert_eq!(st, 404, "{}: {v}", who.email);
        assert!(!v.to_string().contains("Orçamento"));
        let (st, _) = app.get(&location, Some(&who.token)).await;
        assert_eq!(st, 404);
        let (st, _) = app
            .post(
                &base,
                Some(&who.token),
                json!({"t_ms": 5_000, "title": "x"}),
            )
            .await;
        assert_eq!(st, 404);
        let (st, _) = app.delete(&location, Some(&who.token)).await;
        assert_eq!(st, 404);
    }

    // Um capítulo não se alcança pelo id de OUTRA gravação.
    let other = app.insert_recording(&f.room_id, &f.a.user_id).await;
    let chapter_id = second["id"].as_str().unwrap();
    let (st, _) = app
        .delete(
            &format!("/api/recordings/{other}/chapters/{chapter_id}"),
            Some(&f.a.token),
        )
        .await;
    assert_eq!(st, 404);

    // DELETE 204, depois 404.
    let (st, _) = app.delete(&location, Some(&f.a.token)).await;
    assert_eq!(st, 204);
    let (st, v) = app.delete(&location, Some(&f.a.token)).await;
    assert_eq!(st, 404, "{v}");

    // Tecto de capítulos.
    sqlx::query(
        "INSERT INTO recording_chapters (recording_id, t_ms, title, source, created_by)
         SELECT $1::uuid, g * 1000, 'c' || g, 'manual', $2::uuid FROM generate_series(1, 98) g",
    )
    .bind(&f.rec)
    .bind(&f.a.user_id)
    .execute(&app.db)
    .await
    .unwrap();
    let (st, v) = app
        .post(
            &base,
            Some(&f.a.token),
            json!({"t_ms": 5_000, "title": "a mais"}),
        )
        .await;
    assert_eq!(st, 422, "{v}");
    assert_eq!(v["code"], "recording.too_many_chapters");
    // Apagar a gravação leva os capítulos (FK em cascata).
    sql(app, "DELETE FROM recordings WHERE id = $1::uuid", &f.rec).await;
    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM recording_chapters")
        .fetch_one(&app.db)
        .await
        .unwrap();
    assert_eq!(n, 0);
}

// ---------------------------------------------------------------------------
//  G5 — comentários
// ---------------------------------------------------------------------------

#[sqlx::test(migrations = "./migrations")]
async fn comments_crud_author_only_soft_delete_and_dlp(db: sqlx::PgPool) {
    let f = fixture(db).await;
    let app = &f.app;
    sql(
        app,
        "UPDATE recordings SET duration_ms = 120000 WHERE id = $1::uuid",
        &f.rec,
    )
    .await;
    let base = format!("/api/recordings/{}/comments", f.rec);
    let key = format!("sk-{}", "abcdefghij".repeat(3) + "ab");

    // A participante comenta com uma chave de API colada: o DLP censura.
    let res = app
        .http
        .post(app.url(&base))
        .bearer_auth(&f.carla.token)
        .json(&json!({"body": format!("usa esta: {key}"), "t_ms": 90_000}))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 201);
    let location = res.headers()["location"].to_str().unwrap().to_string();
    let c1: Value = res.json().await.unwrap();
    assert!(!c1["body"].as_str().unwrap().contains(&key), "{c1}");
    assert!(c1["body"].as_str().unwrap().contains("CENSURADA"), "{c1}");
    assert_eq!(c1["username"], "carla-alfa.test");
    let stored: String =
        sqlx::query_scalar("SELECT body FROM recording_comments WHERE id = $1::uuid")
            .bind(c1["id"].as_str().unwrap())
            .fetch_one(&app.db)
            .await
            .unwrap();
    assert!(!stored.contains(&key), "a chave não chega à base: {stored}");

    // O dono comenta sem marca e com marca anterior; o admin que não participou também.
    let (st, general) = app
        .post(&base, Some(&f.a.token), json!({"body": "geral"}))
        .await;
    assert_eq!(st, 201, "{general}");
    assert!(general["t_ms"].is_null());
    let (st, _) = app
        .post(
            &base,
            Some(&f.a.token),
            json!({"body": "no início", "t_ms": 5_000}),
        )
        .await;
    assert_eq!(st, 201);
    let (st, v) = app
        .post(
            &base,
            Some(&f.eva.token),
            json!({"body": "do admin", "t_ms": 5_000}),
        )
        .await;
    assert_eq!(st, 201, "quem descarrega também comenta: {v}");

    // Limites.
    for (body, code) in [
        (json!({"body": ""}), "recording.invalid_comment"),
        (
            json!({"body": "x".repeat(2001)}),
            "recording.invalid_comment",
        ),
        (
            json!({"body": "x", "t_ms": 120_001}),
            "recording.invalid_timestamp",
        ),
    ] {
        let (st, v) = app.post(&base, Some(&f.a.token), body).await;
        assert_eq!(st, 400, "{v}");
        assert_eq!(v["code"], code);
    }

    // Ordem: marca temporal (5, 5, 90), sem marca no fim; empates por criação.
    let (st, p) = app.get(&base, Some(&f.carla.token)).await;
    assert_eq!(st, 200, "{p}");
    let bodies: Vec<&str> = items(&p)
        .iter()
        .map(|c| c["body"].as_str().unwrap())
        .collect();
    assert_eq!(bodies[0], "no início");
    assert_eq!(bodies[1], "do admin");
    assert_eq!(bodies[3], "geral");
    // Paginação percorre tudo sem repetir.
    let mut seen = Vec::new();
    let mut token: Option<String> = None;
    loop {
        let url = match &token {
            Some(t) => format!("{base}?page_size=1&page_token={t}"),
            None => format!("{base}?page_size=1"),
        };
        let (st, p) = app.get(&url, Some(&f.a.token)).await;
        assert_eq!(st, 200, "{p}");
        assert!(items(&p).len() <= 1);
        seen.extend(
            items(&p)
                .iter()
                .map(|c| c["id"].as_str().unwrap().to_string()),
        );
        token = p["next_page_token"].as_str().map(str::to_string);
        if token.is_none() {
            break;
        }
    }
    assert_eq!(seen.len(), 4);
    assert_eq!(
        seen,
        items(&p)
            .iter()
            .map(|c| c["id"].as_str().unwrap().to_string())
            .collect::<Vec<_>>()
    );

    // Só o autor altera e apaga — nem o dono da gravação.
    let (st, v) = app
        .patch(&location, Some(&f.a.token), json!({"body": "forjado"}))
        .await;
    assert_eq!(st, 403, "{v}");
    assert_eq!(v["code"], "recording.not_comment_author");
    let (st, _) = app.delete(&location, Some(&f.a.token)).await;
    assert_eq!(st, 403);
    let (st, v) = app
        .patch(
            &location,
            Some(&f.carla.token),
            json!({"body": "corrigido", "t_ms": 100_000}),
        )
        .await;
    assert_eq!(st, 200, "{v}");
    assert_eq!(v["body"], "corrigido");
    assert_eq!(v["t_ms"], 100_000);
    assert!(!v["edited_at"].is_null());
    let (st, _) = app
        .patch(&location, Some(&f.carla.token), json!({"t_ms": 999_000}))
        .await;
    assert_eq!(st, 400);

    // Outra org e membro sem acesso: 404 em tudo.
    for who in [&f.b, &f.duarte] {
        let (st, v) = app.get(&base, Some(&who.token)).await;
        assert_eq!(st, 404, "{v}");
        assert!(!v.to_string().contains("corrigido"));
        let (st, _) = app.get(&location, Some(&who.token)).await;
        assert_eq!(st, 404);
        let (st, _) = app
            .post(&base, Some(&who.token), json!({"body": "x"}))
            .await;
        assert_eq!(st, 404);
        let (st, _) = app
            .patch(&location, Some(&who.token), json!({"body": "x"}))
            .await;
        assert_eq!(st, 404);
        let (st, _) = app.delete(&location, Some(&who.token)).await;
        assert_eq!(st, 404);
    }

    // Apagar é lógico: some da lista e do GET, fica na base.
    let (st, _) = app.delete(&location, Some(&f.carla.token)).await;
    assert_eq!(st, 204);
    let (st, _) = app.get(&location, Some(&f.carla.token)).await;
    assert_eq!(st, 404);
    let (st, _) = app.delete(&location, Some(&f.carla.token)).await;
    assert_eq!(st, 404);
    let (st, _) = app
        .patch(
            &location,
            Some(&f.carla.token),
            json!({"body": "ressuscitar"}),
        )
        .await;
    assert_eq!(st, 404);
    let (_, p) = app.get(&base, Some(&f.a.token)).await;
    assert_eq!(items(&p).len(), 3);
    assert!(!p.to_string().contains("corrigido"));
    let deleted: bool = sqlx::query_scalar(
        "SELECT deleted_at IS NOT NULL FROM recording_comments WHERE id = $1::uuid",
    )
    .bind(c1["id"].as_str().unwrap())
    .fetch_one(&app.db)
    .await
    .unwrap();
    assert!(deleted);
}

// ---------------------------------------------------------------------------
//  S3 — o membro arquivado
// ---------------------------------------------------------------------------

#[sqlx::test(migrations = "./migrations")]
async fn archived_member_cannot_read_comments_nor_see_library(db: sqlx::PgPool) {
    let f = fixture(db).await;
    let app = &f.app;
    let base = format!("/api/recordings/{}/comments", f.rec);
    let (st, _) = app
        .post(
            &base,
            Some(&f.a.token),
            json!({"body": "decisão confidencial"}),
        )
        .await;
    assert_eq!(st, 201);

    // ANTES: a participante lê, vê na biblioteca e reproduz (404 = sem ficheiro,
    // passou a autorização); o admin descarrega.
    let (st, p) = app.get(&base, Some(&f.carla.token)).await;
    assert_eq!(st, 200);
    assert!(p.to_string().contains("confidencial"));
    let (_, lib) = app.get("/api/recordings", Some(&f.carla.token)).await;
    assert_eq!(lib.as_array().unwrap().len(), 1);
    let (st, _) = app
        .get(
            &format!("/api/recordings/{}/content", f.rec),
            Some(&f.carla.token),
        )
        .await;
    assert_eq!(st, 404);
    let (st, _) = app
        .get(
            &format!("/api/recordings/{}/content?dl=1", f.rec),
            Some(&f.eva.token),
        )
        .await;
    assert_eq!(st, 404);

    app.archive_member(f.a.org(), &f.carla.user_id).await;
    app.archive_member(f.a.org(), &f.eva.user_id).await;

    // DEPOIS: tudo recusado, com o mesmo token.
    let (st, v) = app.get(&base, Some(&f.carla.token)).await;
    assert_eq!(st, 404, "{v}");
    assert!(!v.to_string().contains("confidencial"));
    let (st, _) = app
        .post(&base, Some(&f.carla.token), json!({"body": "ainda aqui"}))
        .await;
    assert_eq!(st, 404);
    let (st, _) = app
        .get(
            &format!("/api/recordings/{}/chapters", f.rec),
            Some(&f.carla.token),
        )
        .await;
    assert_eq!(st, 404);
    let (st, _) = app
        .get(&format!("/api/recordings/{}", f.rec), Some(&f.carla.token))
        .await;
    assert_eq!(st, 404);
    let (_, lib) = app.get("/api/recordings", Some(&f.carla.token)).await;
    assert_eq!(
        lib,
        json!([]),
        "a biblioteca concorda com a regra (LIBRARY_VISIBLE)"
    );
    let (_, lib) = app
        .get("/api/recordings?q=teste", Some(&f.carla.token))
        .await;
    assert_eq!(items(&lib).len(), 0);
    let (st, _) = app
        .get(
            &format!("/api/recordings/{}/content", f.rec),
            Some(&f.carla.token),
        )
        .await;
    assert_eq!(st, 404, "reproduzir também: quem saiu não chega à gravação");
    let (st, _) = app
        .get(
            &format!("/api/recordings/{}/content?dl=1", f.rec),
            Some(&f.eva.token),
        )
        .await;
    assert_eq!(st, 404);
    let (st, _) = app.get(&base, Some(&f.eva.token)).await;
    assert_eq!(st, 404);

    // O SUJEITO não se filtra: com o dono arquivado, o admin activo que resta
    // continua a chegar à gravação da empresa.
    let admin2 = app.add_member(&f.a, "rui", "admin").await;
    // ADR-0008 §5: a org não fica sem Proprietário enquanto tiver humanos
    // activos (o gatilho recusa). O dono sai depois de passar a propriedade.
    sqlx::query(
        "UPDATE org_members SET role_id = (SELECT id FROM org_roles WHERE org_id = $1::uuid AND system_key = 'owner')
          WHERE org_id = $1::uuid AND user_id = $2::uuid",
    )
    .bind(f.a.org())
    .bind(&admin2.user_id)
    .execute(&app.db)
    .await
    .unwrap();
    app.archive_member(f.a.org(), &f.a.user_id).await;
    let (st, p) = app.get(&base, Some(&admin2.token)).await;
    assert_eq!(st, 200, "{p}");
    let (st, _) = app
        .get(
            &format!("/api/recordings/{}/content?dl=1", f.rec),
            Some(&admin2.token),
        )
        .await;
    assert_eq!(st, 404, "autorizado; sem ficheiro");
    // …e o dono arquivado deixou de ser «quem pede» válido.
    let (st, _) = app.get(&base, Some(&f.a.token)).await;
    assert_eq!(st, 404);
}

// ---------------------------------------------------------------------------
//  G6 — pesquisa
// ---------------------------------------------------------------------------

#[sqlx::test(migrations = "./migrations")]
async fn search_finds_transcript_words_only_in_visible_recordings(db: sqlx::PgPool) {
    let f = fixture(db).await;
    let app = &f.app;
    sql(
        app,
        "UPDATE recordings SET transcript = 'Hoje discutimos o orçamento trimestral e a contratação.' WHERE id = $1::uuid",
        &f.rec,
    )
    .await;
    let room_b = app.new_room(&f.b, "sala-beta").await;
    let rec_b = app
        .insert_recording(room_b["id"].as_str().unwrap(), &f.b.user_id)
        .await;
    sql(
        app,
        "UPDATE recordings SET transcript = 'O orçamento da beta é segredo.' WHERE id = $1::uuid",
        &rec_b,
    )
    .await;

    // A encontra a sua, com excerto; nunca a da B.
    let (st, p) = app
        .get("/api/recordings?q=or%C3%A7amento", Some(&f.a.token))
        .await;
    assert_eq!(st, 200, "{p}");
    assert_eq!(items(&p).len(), 1, "{p}");
    assert_eq!(items(&p)[0]["id"], f.rec.as_str());
    let snippet = items(&p)[0]["snippet"].as_str().unwrap();
    assert!(snippet.contains("«orçamento»"), "{snippet}");
    assert!(!p.to_string().contains("segredo"));
    // A B encontra a dela, e só a dela.
    let (_, p) = app
        .get("/api/recordings?q=or%C3%A7amento", Some(&f.b.token))
        .await;
    assert_eq!(items(&p).len(), 1);
    assert_eq!(items(&p)[0]["id"], rec_b.as_str());
    // Membro da A sem acesso à gravação: nada.
    let (_, p) = app
        .get("/api/recordings?q=or%C3%A7amento", Some(&f.duarte.token))
        .await;
    assert_eq!(items(&p).len(), 0);

    // Prefixo, maiúsculas, vários termos (todos obrigatórios), e o título.
    let (_, p) = app
        .get("/api/recordings?q=OR%C3%87AM%20trimes", Some(&f.a.token))
        .await;
    assert_eq!(items(&p).len(), 1, "{p}");
    let (_, p) = app
        .get(
            "/api/recordings?q=or%C3%A7amento%20inexistente",
            Some(&f.a.token),
        )
        .await;
    assert_eq!(items(&p).len(), 0);
    let (st, _) = app
        .patch(
            &format!("/api/recordings/{}", f.rec),
            Some(&f.a.token),
            json!({"filename": "Comité de Química"}),
        )
        .await;
    assert_eq!(st, 200);
    let (_, p) = app
        .get("/api/recordings?q=qu%C3%ADmica", Some(&f.a.token))
        .await;
    assert_eq!(items(&p).len(), 1, "{p}");
    assert!(items(&p)[0]["snippet"]
        .as_str()
        .unwrap()
        .contains("«Química»"));
    // Sintaxe de tsquery no texto não é interpretada.
    let (st, p) = app
        .get(
            "/api/recordings?q=or%C3%A7amento%20%26%20!%7C",
            Some(&f.a.token),
        )
        .await;
    assert_eq!(st, 200, "{p}");
    assert_eq!(items(&p).len(), 1);
    let (st, v) = app
        .get("/api/recordings?q=%21%21%21", Some(&f.a.token))
        .await;
    assert_eq!(st, 400);
    assert_eq!(v["code"], "recording.invalid_query");
}

#[sqlx::test(migrations = "./migrations")]
async fn library_keeps_bare_array_and_paginates_on_request(db: sqlx::PgPool) {
    let f = fixture(db).await;
    let app = &f.app;
    for _ in 0..4 {
        let id = app.insert_recording(&f.room_id, &f.a.user_id).await;
        sql(
            app,
            "UPDATE recordings SET transcript = 'reunião de planeamento' WHERE id = $1::uuid",
            &id,
        )
        .await;
    }

    // Sem parâmetros: a lista inteira, como o web a lê.
    let (st, lib) = app.get("/api/recordings", Some(&f.a.token)).await;
    assert_eq!(st, 200);
    assert_eq!(lib.as_array().unwrap().len(), 5);

    // Paginada: percorre tudo, sem repetir, pela ordem da lista.
    for (query, expected) in [("", 5), ("&q=planeamento", 4)] {
        let mut seen = Vec::new();
        let mut token: Option<String> = None;
        loop {
            let url = match &token {
                Some(t) => format!("/api/recordings?page_size=2{query}&page_token={t}"),
                None => format!("/api/recordings?page_size=2{query}"),
            };
            let (st, p) = app.get(&url, Some(&f.a.token)).await;
            assert_eq!(st, 200, "{url}: {p}");
            assert!(items(&p).len() <= 2);
            seen.extend(
                items(&p)
                    .iter()
                    .map(|r| r["id"].as_str().unwrap().to_string()),
            );
            token = p["next_page_token"].as_str().map(str::to_string);
            if token.is_none() {
                break;
            }
        }
        assert_eq!(seen.len(), expected, "{query}");
        let all: Vec<String> = lib
            .as_array()
            .unwrap()
            .iter()
            .map(|r| r["id"].as_str().unwrap().to_string())
            .filter(|id| query.is_empty() || *id != f.rec)
            .collect();
        assert_eq!(seen, all, "{query}");
    }
    let (st, v) = app
        .get("/api/recordings?page_token=%%%lixo", Some(&f.a.token))
        .await;
    assert_eq!(st, 400, "{v}");
    // `q` SOZINHO não pagina: devolve a lista, que é o que o web lê
    // (`recordingsLibraryMeta({q})` espera um array). Só `page_size` e
    // `page_token` pedem uma página.
    let (st, p) = app.get("/api/recordings?q=", Some(&f.a.token)).await;
    assert_eq!(st, 200);
    assert_eq!(p.as_array().unwrap().len(), 5, "q vazio não filtra");
    let (st, p) = app
        .get("/api/recordings?q=planeamento", Some(&f.a.token))
        .await;
    assert_eq!(st, 200);
    assert_eq!(p.as_array().unwrap().len(), 4, "lista, não página: {p}");
    // Um `q` só com pontuação é erro do cliente, não «tudo» nem «nada».
    let (st, v) = app.get("/api/recordings?q=%%%", Some(&f.a.token)).await;
    assert_eq!(st, 400, "{v}");
    assert_eq!(v["code"], "recording.invalid_query");
    // E um `scope` desconhecido também.
    let (st, v) = app
        .get("/api/recordings?scope=todas", Some(&f.a.token))
        .await;
    assert_eq!(st, 400, "{v}");
    assert_eq!(v["code"], "recording.invalid_scope");
}

// ---------------------------------------------------------------------------
//  Dívida fechada: o upload escreve onde a configuração diz
// ---------------------------------------------------------------------------

#[sqlx::test(migrations = "./migrations")]
async fn upload_writes_to_configured_recordings_dir(db: sqlx::PgPool) {
    let f = fixture(db).await;
    let app = &f.app;
    let code: String = sqlx::query_scalar("SELECT code FROM rooms WHERE id = $1::uuid")
        .bind(&f.room_id)
        .fetch_one(&app.db)
        .await
        .unwrap();
    let bytes = vec![0x1a, 0x45, 0xdf, 0xa3, 1, 2, 3];
    let res = app
        .http
        .post(app.url(&format!("/api/rooms/{code}/recordings?name=up.webm")))
        .bearer_auth(&f.a.token)
        .body(bytes.clone())
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 200);
    let rec: Value = res.json().await.unwrap();
    let id = rec["id"].as_str().unwrap();
    let on_disk = app.state.config.recordings_dir.join(format!("{id}.webm"));
    assert_eq!(
        std::fs::read(&on_disk).unwrap(),
        bytes,
        "{}",
        on_disk.display()
    );

    let res = app
        .http
        .get(app.url(&format!("/api/recordings/{id}/content?dl=1")))
        .bearer_auth(&f.a.token)
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 200);
    assert_eq!(res.bytes().await.unwrap().to_vec(), bytes);
    let _ = std::fs::remove_dir_all(&app.state.config.recordings_dir);
}

// ---------------------------------------------------------------------------
//  R235 — publicar para a organização não mostrava a gravação a ninguém
// ---------------------------------------------------------------------------

/// `GET /api/recordings?scope=published` mostra a gravação publicada aos
/// membros ACTIVOS da organização de quem a carregou — incluindo a quem nunca
/// esteve na sala — e a mais ninguém.
///
/// Antes, `scope` não existia: publicar escrevia `visibility` e `published_at`
/// e nenhuma leitura os usava. A dona carregava em «publicar», a consola dizia
/// que estava publicada, e nenhum colega a via em lado nenhum.
#[sqlx::test(migrations = "./migrations")]
async fn published_library_reaches_the_org_and_nobody_else(db: sqlx::PgPool) {
    let f = fixture(db).await;
    let app = &f.app;
    let publish = format!("/api/recordings/{}/publish", f.rec);

    // CONTROLO: antes de publicar, a biblioteca publicada está vazia para
    // toda a gente, incluindo a dona.
    for who in [&f.a, &f.duarte, &f.carla, &f.b] {
        let (st, p) = app
            .get("/api/recordings?scope=published", Some(&who.token))
            .await;
        assert_eq!(st, 200, "{p}");
        assert_eq!(items(&p).len(), 0, "{}: {p}", who.email);
    }
    // E o Duarte (membro da org que não participou) não a vê de todo.
    let (st, _) = app
        .get(&format!("/api/recordings/{}", f.rec), Some(&f.duarte.token))
        .await;
    assert_eq!(st, 404, "antes de publicar nem sabe que existe");

    let (st, m) = app
        .post(&publish, Some(&f.a.token), json!({"visibility": "org"}))
        .await;
    assert_eq!(st, 200, "{m}");

    // O colega da MESMA org que nunca esteve na sala passa a vê-la.
    let (st, p) = app
        .get("/api/recordings?scope=published", Some(&f.duarte.token))
        .await;
    assert_eq!(st, 200, "{p}");
    assert_eq!(items(&p).len(), 1, "publicar tem de mostrar a alguém: {p}");
    assert_eq!(items(&p)[0]["id"], f.rec.as_str());
    assert_eq!(items(&p)[0]["visibility"], "org");
    assert!(items(&p)[0]["published_at"].is_string());
    // Reproduz, mas não descarrega nem gere: publicar dá o vídeo, não o poder.
    assert_eq!(items(&p)[0]["can_download"], false);
    assert_eq!(items(&p)[0]["can_manage"], false);
    let (st, _) = app
        .get(&format!("/api/recordings/{}", f.rec), Some(&f.duarte.token))
        .await;
    assert_eq!(st, 200, "agora chega ao recurso");

    // A biblioteca PESSOAL do Duarte continua vazia: publicar não entope a
    // biblioteca de quem não teve nada a ver com a reunião.
    let (_, mine) = app.get("/api/recordings", Some(&f.duarte.token)).await;
    assert_eq!(items(&mine).len(), 0, "{mine}");

    // OUTRA organização não a vê, nem na publicada nem no recurso.
    let (st, p) = app
        .get("/api/recordings?scope=published", Some(&f.b.token))
        .await;
    assert_eq!(st, 200);
    assert_eq!(
        items(&p).len(),
        0,
        "publicar é para a org, não para o mundo"
    );
    let (st, _) = app
        .get(&format!("/api/recordings/{}", f.rec), Some(&f.b.token))
        .await;
    assert_eq!(st, 404);

    // Um membro ARQUIVADO (S3) perde-a, mesmo publicada.
    sqlx::query("UPDATE org_members SET archived_at = now() WHERE user_id = $1::uuid")
        .bind(&f.duarte.user_id)
        .execute(&app.db)
        .await
        .unwrap();
    let (_, p) = app
        .get("/api/recordings?scope=published", Some(&f.duarte.token))
        .await;
    assert_eq!(items(&p).len(), 0, "quem saiu da empresa deixa de ver: {p}");
    let (st, _) = app
        .get(&format!("/api/recordings/{}", f.rec), Some(&f.duarte.token))
        .await;
    assert_eq!(st, 404);

    // Despublicar devolve tudo ao estado anterior.
    let (st, _) = app
        .post(
            &format!("/api/recordings/{}/unpublish", f.rec),
            Some(&f.a.token),
            json!({}),
        )
        .await;
    assert_eq!(st, 200);
    let (_, p) = app
        .get("/api/recordings?scope=published", Some(&f.carla.token))
        .await;
    assert_eq!(items(&p).len(), 0, "despublicada sai da biblioteca: {p}");
}

/// Publicar abre a REPRODUÇÃO, não a transcrição nem a lista de presentes: um
/// colega que nunca esteve na reunião recebe `403` (não `404` — já a viu na
/// biblioteca publicada).
#[sqlx::test(migrations = "./migrations")]
async fn publishing_does_not_open_transcript_nor_participants(db: sqlx::PgPool) {
    let f = fixture(db).await;
    let app = &f.app;
    sqlx::query("UPDATE recordings SET transcript = 'texto da reunião' WHERE id = $1::uuid")
        .bind(&f.rec)
        .execute(&app.db)
        .await
        .unwrap();
    let (st, _) = app
        .post(
            &format!("/api/recordings/{}/publish", f.rec),
            Some(&f.a.token),
            json!({"visibility": "org"}),
        )
        .await;
    assert_eq!(st, 200);

    for (path, code) in [
        ("transcript", "recording.transcript_forbidden"),
        ("participants", "recording.participants_forbidden"),
    ] {
        let url = format!("/api/recordings/{}/{path}", f.rec);
        let (st, v) = app.get(&url, Some(&f.duarte.token)).await;
        assert_eq!(st, 403, "{path}: {v}");
        assert_eq!(v["code"], code);
        assert!(
            !v.to_string().contains("texto da reunião"),
            "a recusa não devolve o conteúdo: {v}"
        );
        // CONTROLO: quem participou continua a ler.
        let (st, v) = app.get(&url, Some(&f.carla.token)).await;
        assert_eq!(st, 200, "{path} para quem participou: {v}");
    }
}

// ---------------------------------------------------------------------------
//  R236 — a duração de uma gravação carregada nunca aparecia
// ---------------------------------------------------------------------------

/// O upload mede o ficheiro com `ffprobe` e a LISTAGEM serve o que ele mediu.
///
/// Antes, o upload escrevia `duration_ms` e a listagem servia `duration_secs`
/// — uma coluna que só o gravador do servidor preenchia. Uma gravação
/// carregada pelo browser aparecia sempre sem duração e sem resolução, por
/// muito que o servidor as tivesse medido e guardado.
#[sqlx::test(migrations = "./migrations")]
async fn uploaded_recording_shows_measured_duration_and_resolution(db: sqlx::PgPool) {
    // Sem ffmpeg não há ficheiro real para medir, e medir é o que este teste
    // prova. Salta com a razão à vista em vez de falhar — o CI não tem ffmpeg,
    // e um vermelho ali diria «a duração está partida» quando diz «a máquina
    // não tem a ferramenta». O mesmo aviso que o `media_probe` já usa.
    if std::process::Command::new("ffmpeg")
        .arg("-version")
        .output()
        .is_err()
    {
        eprintln!("ffmpeg indisponível — medição real NÃO verificada");
        return;
    }
    let f = fixture(db).await;
    let app = &f.app;
    let code: String = sqlx::query_scalar("SELECT code FROM rooms WHERE id = $1::uuid")
        .bind(&f.room_id)
        .fetch_one(&app.db)
        .await
        .unwrap();

    // Um webm a sério, de 2 segundos e 320x240, feito pelo ffmpeg: medir um
    // ficheiro inventado não provava nada sobre o caminho real.
    let dir = std::env::temp_dir().join(format!("dlx-dur-{}", uuid_like()));
    std::fs::create_dir_all(&dir).unwrap();
    let src = dir.join("amostra.webm");
    let out = std::process::Command::new("ffmpeg")
        .args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-y",
            "-f",
            "lavfi",
            "-i",
            "testsrc=size=320x240:rate=10:duration=2",
            "-c:v",
            "libvpx-vp9",
            "-b:v",
            "50k",
            "-deadline",
            "realtime",
            "-cpu-used",
            "8",
        ])
        .arg(&src)
        .output()
        .expect("ffmpeg tem de estar disponível (é o que o servidor usa)");
    assert!(
        out.status.success(),
        "ffmpeg falhou: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let bytes = std::fs::read(&src).unwrap();

    let res = app
        .http
        .post(app.url(&format!("/api/rooms/{code}/recordings?name=medida.webm")))
        .bearer_auth(&f.a.token)
        .body(bytes)
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 200);
    let up: Value = res.json().await.unwrap();
    let id = up["id"].as_str().unwrap().to_string();
    // A resposta do upload já traz o que foi medido.
    assert_eq!(up["duration_ms"], 2000, "duração medida no upload: {up}");
    assert_eq!(up["width"], 320);
    assert_eq!(up["height"], 240);

    // E — o que falhava — a LISTAGEM e o recurso servem o mesmo.
    let (st, m) = app
        .get(&format!("/api/recordings/{id}"), Some(&f.a.token))
        .await;
    assert_eq!(st, 200, "{m}");
    assert_eq!(m["duration_ms"], 2000, "o recurso serve a duração: {m}");
    assert_eq!(m["width"], 320);
    assert_eq!(m["height"], 240);
    assert!(m["video_codec"].is_string(), "{m}");

    let (_, lib) = app.get("/api/recordings", Some(&f.a.token)).await;
    let row = items(&lib)
        .iter()
        .find(|r| r["id"] == id.as_str())
        .unwrap_or_else(|| panic!("a gravação carregada não está na biblioteca: {lib}"));
    assert_eq!(
        row["duration_ms"], 2000,
        "a biblioteca serve a duração: {row}"
    );
    assert_eq!(row["width"], 320);
    assert_eq!(row["height"], 240);

    // A marca temporal de um capítulo passa a caber na duração MEDIDA.
    let base = format!("/api/recordings/{id}/chapters");
    let (st, v) = app
        .post(
            &base,
            Some(&f.a.token),
            json!({"t_ms": 2000, "title": "Fim"}),
        )
        .await;
    assert_eq!(st, 201, "a duração medida é inclusiva: {v}");
    let (st, v) = app
        .post(
            &base,
            Some(&f.a.token),
            json!({"t_ms": 2001, "title": "Além"}),
        )
        .await;
    assert_eq!(st, 400, "{v}");
    assert_eq!(v["code"], "recording.invalid_timestamp");

    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(&app.state.config.recordings_dir);
}

/// Nome único para a pasta temporária do teste, sem dependência nova.
fn uuid_like() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    format!(
        "{}-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos(),
        std::process::id()
    )
}

// ---------------------------------------------------------------------------
//  R237 — o ficheiro não honrava o `Range`
// ---------------------------------------------------------------------------

/// `GET …/content` responde `206` a um `Range`, com `Content-Range`, e anuncia
/// `Accept-Ranges: bytes` mesmo quando devolve o ficheiro inteiro.
///
/// Antes devolvia sempre `200` com o ficheiro todo e sem `Accept-Ranges`: o
/// `<video>` pede um intervalo, recebe tudo, e desiste de procurar — cada
/// salto na barra puxava a gravação inteira outra vez.
#[sqlx::test(migrations = "./migrations")]
async fn content_honours_range_requests(db: sqlx::PgPool) {
    let f = fixture(db).await;
    let app = &f.app;
    let code: String = sqlx::query_scalar("SELECT code FROM rooms WHERE id = $1::uuid")
        .bind(&f.room_id)
        .fetch_one(&app.db)
        .await
        .unwrap();
    // 4096 bytes com conteúdo distinguível, para se ver QUE fatia voltou.
    let bytes: Vec<u8> = (0..4096u32).map(|i| (i % 251) as u8).collect();
    let res = app
        .http
        .post(app.url(&format!("/api/rooms/{code}/recordings?name=r.webm")))
        .bearer_auth(&f.a.token)
        .body(bytes.clone())
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 200);
    let id = res.json::<Value>().await.unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();
    let url = app.url(&format!("/api/recordings/{id}/content"));
    let total = bytes.len();

    // CONTROLO: sem `Range`, o ficheiro inteiro — mas já a anunciar que
    // aceita intervalos, que é o que faz o leitor voltar a pedi-los.
    let res = app
        .http
        .get(&url)
        .bearer_auth(&f.a.token)
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 200);
    assert_eq!(res.headers()["accept-ranges"], "bytes");
    assert!(res.headers().get("content-range").is_none());
    assert_eq!(res.bytes().await.unwrap().to_vec(), bytes);

    // Um intervalo do princípio.
    for (header, want_start, want_end) in [
        ("bytes=0-1023", 0usize, 1023usize),
        ("bytes=1024-2047", 1024, 2047),
        // Aberto à direita: até ao fim.
        ("bytes=4000-", 4000, total - 1),
        // Sufixo: os últimos N.
        ("bytes=-100", total - 100, total - 1),
        // Um fim para lá do ficheiro corta-se, não invalida o pedido.
        ("bytes=4090-999999", 4090, total - 1),
    ] {
        let res = app
            .http
            .get(&url)
            .bearer_auth(&f.a.token)
            .header("Range", header)
            .send()
            .await
            .unwrap();
        assert_eq!(res.status(), 206, "{header}");
        assert_eq!(res.headers()["accept-ranges"], "bytes", "{header}");
        assert_eq!(
            res.headers()["content-range"],
            format!("bytes {want_start}-{want_end}/{total}"),
            "{header}"
        );
        let got = res.bytes().await.unwrap().to_vec();
        assert_eq!(got.len(), want_end - want_start + 1, "{header}");
        assert_eq!(got, bytes[want_start..=want_end], "{header}");
    }

    // Fora do ficheiro: `416`, com o tamanho real para o cliente se corrigir.
    let res = app
        .http
        .get(&url)
        .bearer_auth(&f.a.token)
        .header("Range", "bytes=99999-")
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 416);
    assert_eq!(res.headers()["content-range"], format!("bytes */{total}"));

    // Várias faixas: não se serve `multipart/byteranges` — responde-se tudo.
    let res = app
        .http
        .get(&url)
        .bearer_auth(&f.a.token)
        .header("Range", "bytes=0-10,20-30")
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 200);
    assert_eq!(res.bytes().await.unwrap().to_vec().len(), total);

    // Um cabeçalho que não se percebe não parte nada: o ficheiro inteiro.
    for bad in ["lixo", "bytes=abc-def", "bytes="] {
        let res = app
            .http
            .get(&url)
            .bearer_auth(&f.a.token)
            .header("Range", bad)
            .send()
            .await
            .unwrap();
        assert_eq!(res.status(), 200, "{bad}");
        assert_eq!(res.bytes().await.unwrap().to_vec().len(), total, "{bad}");
    }

    // O `Range` não contorna o RBAC: quem não descarrega continua sem o
    // ficheiro por `?dl=1`, com ou sem intervalo.
    let res = app
        .http
        .get(app.url(&format!("/api/recordings/{id}/content?dl=1")))
        .bearer_auth(&f.carla.token)
        .header("Range", "bytes=0-10")
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 403);
    // E quem não chega à gravação continua a receber 404, não um 206.
    let res = app
        .http
        .get(&url)
        .bearer_auth(&f.b.token)
        .header("Range", "bytes=0-10")
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 404);

    let _ = std::fs::remove_dir_all(&app.state.config.recordings_dir);
}

/// Os dois predicados SQL da biblioteca (`LIBRARY_VISIBLE_MINE` e
/// `…_PUBLISHED`) dizem o MESMO que `AccessFacts::listed_in`.
///
/// A regra está escrita duas vezes de propósito — em SQL porque filtra antes
/// de paginar, e em Rust porque é o domínio — e é assim que uma delas fica
/// para trás. Este teste percorre as combinações de relação que a fixture
/// sabe construir e exige que as duas concordem, linha a linha.
#[sqlx::test(migrations = "./migrations")]
async fn library_scopes_agree_with_access_facts(db: sqlx::PgPool) {
    let f = fixture(db).await;
    let app = &f.app;

    // Uma gravação por relação, todas da mesma dona (a Alfa).
    // `f.rec` já existe: a Carla participou na sala, o Duarte não.
    let partilhada = app.insert_recording(&f.room_id, &f.a.user_id).await;
    sqlx::query(
        "INSERT INTO recording_shares (recording_id, user_id, shared_by)
         VALUES ($1::uuid, $2::uuid, $3::uuid)",
    )
    .bind(&partilhada)
    .bind(&f.duarte.user_id)
    .bind(&f.a.user_id)
    .execute(&app.db)
    .await
    .unwrap();

    for (rec, publicada) in [(&f.rec, true), (&partilhada, false)] {
        if publicada {
            let (st, _) = app
                .post(
                    &format!("/api/recordings/{rec}/publish"),
                    Some(&f.a.token),
                    json!({"visibility": "org"}),
                )
                .await;
            assert_eq!(st, 200);
        }
    }

    // Para cada pessoa e cada scope, o que o SQL devolve tem de ser o que o
    // domínio diria com os factos que a base tem para essa mesma linha.
    for who in [&f.a, &f.carla, &f.duarte, &f.eva, &f.b] {
        for scope in ["mine", "published"] {
            let (st, listed) = app
                .get(&format!("/api/recordings?scope={scope}"), Some(&who.token))
                .await;
            assert_eq!(st, 200, "{listed}");
            let listed_ids: Vec<&str> = items(&listed)
                .iter()
                .map(|r| r["id"].as_str().unwrap())
                .collect();

            for rec in [&f.rec, &partilhada] {
                // Os factos, lidos da base do mesmo modo que o `ITEM_SELECT`.
                let row: (bool, bool, bool, bool, bool, bool) = sqlx::query_as(
                    "SELECT (r.uploader_id = $2::uuid),
                            EXISTS(SELECT 1 FROM room_participants p
                                    WHERE p.room_id = r.room_id AND p.user_id = $2::uuid),
                            EXISTS(SELECT 1 FROM recording_shares s
                                    WHERE s.recording_id = r.id AND s.user_id = $2::uuid),
                            COALESCE((SELECT bool_or(me.archived_at IS NULL)
                                        FROM org_members me JOIN org_members o ON o.org_id = me.org_id
                                       WHERE me.user_id = $2::uuid AND o.user_id = r.uploader_id), false),
                            COALESCE((SELECT bool_or(me.archived_at IS NOT NULL)
                                        FROM org_members me JOIN org_members o ON o.org_id = me.org_id
                                       WHERE me.user_id = $2::uuid AND o.user_id = r.uploader_id), false),
                            (r.published_at IS NOT NULL)
                       FROM recordings r WHERE r.id = $1::uuid",
                )
                .bind(rec)
                .bind(&who.user_id)
                .fetch_one(&app.db)
                .await
                .unwrap();
                let (is_uploader, participant, shared, active_member, archived_member, published) =
                    row;
                let departed = archived_member && !active_member;
                let published_to_my_org = published && active_member;
                let expected = match scope {
                    "mine" => !departed && (is_uploader || participant || shared),
                    _ => {
                        published
                            && !departed
                            && (is_uploader || participant || shared || published_to_my_org)
                    }
                };
                assert_eq!(
                    listed_ids.contains(&rec.as_str()),
                    expected,
                    "{} / scope={scope} / gravação {rec}: factos \
                     (dona={is_uploader}, participou={participant}, partilhada={shared}, \
                      activa={active_member}, arquivada={archived_member}, publicada={published})",
                    who.email
                );
            }
        }
    }
}
