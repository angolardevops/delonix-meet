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

fn items(v: &Value) -> &Vec<Value> {
    v["items"]
        .as_array()
        .unwrap_or_else(|| panic!("sem items: {v}"))
}

// ---------------------------------------------------------------------------
//  G4 — metadados e estado
// ---------------------------------------------------------------------------

#[sqlx::test(migrations = "./migrations")]
async fn processing_state_is_derived_for_each_state(db: sqlx::PgPool) {
    let f = fixture(db).await;
    let (app, a) = (&f.app, &f.a);
    // Os metadados são o próprio recurso (o ficheiro está em `/content`).
    let meta = format!("/api/recordings/{}", f.rec);

    let (st, m) = app.get(&meta, Some(&a.token)).await;
    assert_eq!(st, 200, "{m}");
    assert_eq!(m["processing_state"], "ready");
    assert_eq!(m["status"], "ready", "o campo herdado continua lá");
    assert_eq!(m["category"], "meeting");
    assert!(m["duration_secs"].is_null() && m["width"].is_null() && m["title"].is_null());
    assert_eq!(m["can_manage"], true);
    assert!(m.get("snippet").is_none());

    let cases = [
        (
            "UPDATE recordings SET transcription_lease_token = 't', transcription_lease_expires_at = now() + interval '1 hour' WHERE id = $1::uuid",
            "transcribing",
        ),
        (
            "UPDATE recordings SET transcription_lease_expires_at = now() - interval '1 minute' WHERE id = $1::uuid",
            "ready",
        ),
        (
            "UPDATE recordings SET transcription_failed_at = now() WHERE id = $1::uuid",
            "transcription_failed",
        ),
        (
            "UPDATE recordings SET transcribed_at = now() WHERE id = $1::uuid",
            "transcribed",
        ),
        (
            "UPDATE recordings SET status = 'failed', failure_reason = 'sem espaço' WHERE id = $1::uuid",
            "failed",
        ),
    ];
    for (update, expected) in cases {
        sql(app, update, &f.rec).await;
        let (_, m) = app.get(&meta, Some(&a.token)).await;
        assert_eq!(m["processing_state"], expected, "{update}: {m}");
        // A biblioteca diz o mesmo.
        let (_, lib) = app.get("/api/recordings", Some(&a.token)).await;
        assert_eq!(lib[0]["processing_state"], expected);
    }
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
            json!({"title": "  Aula de Química ", "category": "lecture"}),
        )
        .await;
    assert_eq!(st, 200, "{m}");
    assert_eq!(m["title"], "Aula de Química");
    assert_eq!(m["category"], "lecture");
    assert_eq!(m["filename"], "teste.webm", "o nome do ficheiro não muda");

    // Valida tudo antes de escrever.
    let (st, v) = app
        .patch(
            &path,
            Some(&f.a.token),
            json!({"title": "outro", "category": "podcast"}),
        )
        .await;
    assert_eq!(st, 400);
    assert_eq!(v["code"], "recording.invalid_category");
    let (st, v) = app
        .patch(&path, Some(&f.a.token), json!({"title": "x".repeat(121)}))
        .await;
    assert_eq!(st, 400);
    assert_eq!(v["code"], "recording.invalid_title");
    let (_, m) = app.get(&path, Some(&f.a.token)).await;
    assert_eq!(m["title"], "Aula de Química", "sem escrita parcial");

    // Participante que não é dono: vê, não gere.
    let (st, v) = app
        .patch(&path, Some(&f.carla.token), json!({"category": "other"}))
        .await;
    assert_eq!(st, 403, "{v}");
    assert_eq!(v["code"], "recording.not_manager");
    let (_, m) = app.get(&path, Some(&f.carla.token)).await;
    assert_eq!(m["can_manage"], false);
    // Membro da mesma org sem acesso nenhum: nem sabe que existe.
    let (st, _) = app
        .patch(&path, Some(&f.duarte.token), json!({"category": "other"}))
        .await;
    assert_eq!(st, 404);
    let (st, _) = app.get(&path, Some(&f.duarte.token)).await;
    assert_eq!(st, 404);
    // Outra org: 404, igual a um id inventado.
    let (st, v) = app
        .patch(&path, Some(&f.b.token), json!({"category": "other"}))
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
            json!({"category": "other"}),
        )
        .await;
    assert_eq!(st, 404);
    let (st, _) = app.patch(&path, None, json!({"category": "other"})).await;
    assert_eq!(st, 401);

    // Admin activo da org do dono, sem ter participado: gere (é quem descarrega).
    let (st, m) = app
        .patch(
            &path,
            Some(&f.eva.token),
            json!({"category": "broadcast", "title": ""}),
        )
        .await;
    assert_eq!(st, 200, "{m}");
    assert_eq!(m["category"], "broadcast");
    assert!(m["title"].is_null(), "\"\" apaga o título");
    let cat: String = sqlx::query_scalar("SELECT category FROM recordings WHERE id = $1::uuid")
        .bind(&f.rec)
        .fetch_one(&app.db)
        .await
        .unwrap();
    assert_eq!(cat, "broadcast");
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
        "UPDATE recordings SET duration_secs = 600 WHERE id = $1::uuid",
        &f.rec,
    )
    .await;
    let base = format!("/api/recordings/{}/chapters", f.rec);

    let res = app
        .http
        .post(app.url(&base))
        .bearer_auth(&f.a.token)
        .json(&json!({"at_secs": 300, "title": "Orçamento"}))
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
            json!({"at_secs": 0, "title": "Abertura"}),
        )
        .await;
    assert_eq!(st, 201, "{first}");
    let (st, one) = app.get(&location, Some(&f.carla.token)).await;
    assert_eq!(st, 200);
    assert_eq!(one["title"], "Orçamento");

    // Limites.
    for (body, code) in [
        (
            json!({"at_secs": 601, "title": "x"}),
            "recording.invalid_timestamp",
        ),
        (
            json!({"at_secs": -1, "title": "x"}),
            "recording.invalid_timestamp",
        ),
        (
            json!({"at_secs": 600, "title": " "}),
            "recording.invalid_chapter_title",
        ),
        (
            json!({"at_secs": 1, "title": "x".repeat(121)}),
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
            json!({"at_secs": 600, "title": "Fim"}),
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
            json!({"at_secs": 5, "title": "x"}),
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
            .post(&base, Some(&who.token), json!({"at_secs": 5, "title": "x"}))
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
        "INSERT INTO recording_chapters (recording_id, at_secs, title, created_by)
         SELECT $1::uuid, g, 'c' || g, $2::uuid FROM generate_series(1, 98) g",
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
            json!({"at_secs": 5, "title": "a mais"}),
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
        "UPDATE recordings SET duration_secs = 120 WHERE id = $1::uuid",
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
        .json(&json!({"body": format!("usa esta: {key}"), "at_secs": 90}))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 201);
    let location = res.headers()["location"].to_str().unwrap().to_string();
    let c1: Value = res.json().await.unwrap();
    assert!(!c1["body"].as_str().unwrap().contains(&key), "{c1}");
    assert!(c1["body"].as_str().unwrap().contains("CENSURADA"), "{c1}");
    assert_eq!(c1["author_name"], "carla-alfa.test");
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
    assert!(general["at_secs"].is_null());
    let (st, _) = app
        .post(
            &base,
            Some(&f.a.token),
            json!({"body": "no início", "at_secs": 5}),
        )
        .await;
    assert_eq!(st, 201);
    let (st, v) = app
        .post(
            &base,
            Some(&f.eva.token),
            json!({"body": "do admin", "at_secs": 5}),
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
            json!({"body": "x", "at_secs": 121}),
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
            json!({"body": "corrigido", "at_secs": 100}),
        )
        .await;
    assert_eq!(st, 200, "{v}");
    assert_eq!(v["body"], "corrigido");
    assert_eq!(v["at_secs"], 100);
    assert!(!v["edited_at"].is_null());
    let (st, _) = app
        .patch(&location, Some(&f.carla.token), json!({"at_secs": 999}))
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
            json!({"title": "Comité de Química"}),
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
    let (st, p) = app.get("/api/recordings?q=", Some(&f.a.token)).await;
    assert_eq!(st, 200);
    assert_eq!(items(&p).len(), 5, "q vazio não filtra");
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
