//! Gravações contra Postgres real, no contrato de dados da UI (R183):
//! metadados e estados derivados, publicação para a organização, capítulos e
//! comentários em milissegundos, visualizações, participantes, transcrição com
//! tempos, legendas, medição no upload, pesquisa — e a regra de acesso única
//! (S3: um membro arquivado perde tudo de uma vez).
//!
//! As gravações entram por SQL e SEM ficheiro (`insert_recording`), excepto nos
//! testes do upload, que provam que o ficheiro vai para `config.recordings_dir`.
mod common;

use common::{Account, TestApp, INVENTED_ID};
use delonix_meet_domain::content::recording::{AccessFacts, LibraryScope};
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
    fixture_with(db, &[]).await
}

async fn fixture_with(db: sqlx::PgPool, extra: &[(&str, &str)]) -> Fixture {
    let app = TestApp::spawn_with(db, extra).await;
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

/// A biblioteca na forma lista (sem `page_size`/`page_token`).
fn list(v: &Value) -> &Vec<Value> {
    v.as_array()
        .unwrap_or_else(|| panic!("a biblioteca sem paginação é uma lista: {v}"))
}

fn ids(v: &[Value]) -> Vec<String> {
    v.iter()
        .map(|r| r["id"].as_str().unwrap().to_string())
        .collect()
}

async fn room_code(app: &TestApp, room_id: &str) -> String {
    sqlx::query_scalar("SELECT code FROM rooms WHERE id = $1::uuid")
        .bind(room_id)
        .fetch_one(&app.db)
        .await
        .unwrap()
}

// ---------------------------------------------------------------------------
//  Metadados e estados derivados
// ---------------------------------------------------------------------------

#[sqlx::test(migrations = "./migrations")]
async fn item_has_ui_contract_and_states_are_derived(db: sqlx::PgPool) {
    let f = fixture(db).await;
    let (app, a) = (&f.app, &f.a);
    // Os metadados são o próprio recurso (o ficheiro está em `/content`).
    let meta = format!("/api/recordings/{}", f.rec);

    let (st, m) = app.get(&meta, Some(&a.token)).await;
    assert_eq!(st, 200, "{m}");
    // Os campos do `RecordingLibraryItem` da UI, com os valores por omissão.
    assert_eq!(m["status"], "ready");
    assert_eq!(m["state"], "ready");
    assert_eq!(m["transcript_status"], "none");
    assert_eq!(m["kind"], "meeting");
    assert_eq!(m["visibility"], "private");
    assert_eq!(m["description"], "");
    assert_eq!(m["tags"], json!([]));
    assert_eq!(m["caption_languages"], json!([]));
    assert_eq!(m["has_thumbnail"], false);
    for k in [
        "chapter_count",
        "comment_count",
        "view_count",
        "share_count",
    ] {
        assert_eq!(m[k], 0, "{k}");
    }
    assert_eq!(m["participant_count"], 2);
    for k in [
        "duration_ms",
        "width",
        "height",
        "fps",
        "video_codec",
        "audio_codec",
        "progress_pct",
        "published_at",
        "transcript_language",
        "transcribed_at",
        "failure_reason",
    ] {
        assert!(m[k].is_null(), "{k} devia ser null: {m}");
    }
    assert_eq!(m["can_manage"], true);
    assert_eq!(m["uploader_org_id"], a.org());
    assert!(m["uploader_org_name"].is_string());
    // O que saiu do contrato não volta a aparecer.
    for gone in [
        "processing_state",
        "category",
        "title",
        "duration_secs",
        "snippet",
    ] {
        assert!(m.get(gone).is_none(), "{gone}: {m}");
    }

    let cases = [
        (
            "UPDATE recordings SET transcription_lease_token = 't', transcription_lease_expires_at = now() + interval '1 hour' WHERE id = $1::uuid",
            ("transcribing", "transcribing", "transcribing"),
        ),
        (
            "UPDATE recordings SET transcription_lease_expires_at = now() - interval '1 minute' WHERE id = $1::uuid",
            ("ready", "ready", "none"),
        ),
        (
            "UPDATE recordings SET transcription_failed_at = now() WHERE id = $1::uuid",
            ("ready", "ready", "failed"),
        ),
        (
            "UPDATE recordings SET transcribed_at = now() WHERE id = $1::uuid",
            ("ready", "ready", "ready"),
        ),
        (
            "UPDATE recordings SET visibility = 'org', published_at = now() WHERE id = $1::uuid",
            ("ready", "published", "ready"),
        ),
        (
            "UPDATE recordings SET status = 'failed', failure_reason = 'sem espaço' WHERE id = $1::uuid",
            ("failed", "failed", "none"),
        ),
    ];
    for (update, (status, state, transcript)) in cases {
        sql(app, update, &f.rec).await;
        let (_, m) = app.get(&meta, Some(&a.token)).await;
        assert_eq!(
            (&m["status"], &m["state"], &m["transcript_status"]),
            (&json!(status), &json!(state), &json!(transcript)),
            "{update}: {m}"
        );
        // A biblioteca diz o mesmo.
        let (_, lib) = app.get("/api/recordings", Some(&a.token)).await;
        assert_eq!(list(&lib)[0]["state"], state);
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
            json!({"filename": "  Aula de Química ", "description": " sobre ácidos\nparte 1 ",
                   "tags": ["#Química", "aula", "AULA", " "]}),
        )
        .await;
    assert_eq!(st, 200, "{m}");
    assert_eq!(m["filename"], "Aula de Química");
    assert_eq!(m["description"], "sobre ácidos\nparte 1");
    assert_eq!(m["tags"], json!(["química", "aula"]));

    // Valida tudo antes de escrever.
    for (body, code) in [
        (
            json!({"description": "x", "filename": "a\nb"}),
            "recording.invalid_filename",
        ),
        (
            json!({"filename": "x".repeat(201)}),
            "recording.invalid_filename",
        ),
        (
            json!({"filename": "ok", "description": "x".repeat(8001)}),
            "recording.invalid_description",
        ),
        (
            json!({"filename": "ok", "tags": ["a,b"]}),
            "recording.invalid_tags",
        ),
    ] {
        let (st, v) = app.patch(&path, Some(&f.a.token), body.clone()).await;
        assert_eq!(st, 400, "{body}: {v}");
        assert_eq!(v["code"], code);
    }
    let (_, m) = app.get(&path, Some(&f.a.token)).await;
    assert_eq!(m["filename"], "Aula de Química", "sem escrita parcial");
    assert_eq!(m["description"], "sobre ácidos\nparte 1");

    // Participante que não é dono: vê, não gere.
    let (st, v) = app
        .patch(&path, Some(&f.carla.token), json!({"description": "x"}))
        .await;
    assert_eq!(st, 403, "{v}");
    assert_eq!(v["code"], "recording.not_manager");
    let (_, m) = app.get(&path, Some(&f.carla.token)).await;
    assert_eq!(m["can_manage"], false);
    // Membro da mesma org sem acesso nenhum: nem sabe que existe.
    let (st, _) = app
        .patch(&path, Some(&f.duarte.token), json!({"description": "x"}))
        .await;
    assert_eq!(st, 404);
    let (st, _) = app.get(&path, Some(&f.duarte.token)).await;
    assert_eq!(st, 404);
    // Outra org: 404, igual a um id inventado.
    let (st, v) = app
        .patch(&path, Some(&f.b.token), json!({"description": "x"}))
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
            json!({"description": "x"}),
        )
        .await;
    assert_eq!(st, 404);
    let (st, _) = app.patch(&path, None, json!({"description": "x"})).await;
    assert_eq!(st, 401);

    // Admin activo da org do dono, sem ter participado: gere (é quem descarrega).
    let (st, m) = app
        .patch(
            &path,
            Some(&f.eva.token),
            json!({"description": "", "tags": []}),
        )
        .await;
    assert_eq!(st, 200, "{m}");
    assert_eq!(m["description"], "", "\"\" apaga a descrição");
    assert_eq!(m["tags"], json!([]));
    assert_eq!(m["filename"], "Aula de Química", "o omitido não muda");
}

// ---------------------------------------------------------------------------
//  Publicação
// ---------------------------------------------------------------------------

#[sqlx::test(migrations = "./migrations")]
async fn publication_opens_to_active_org_members_only(db: sqlx::PgPool) {
    let f = fixture(db).await;
    let app = &f.app;
    let item = format!("/api/recordings/{}", f.rec);
    let publication = format!("{item}/publication");
    let published_lib = "/api/recordings?scope=published";

    // ANTES: o colega da mesma org que não participou não a vê.
    let (st, _) = app.get(&item, Some(&f.duarte.token)).await;
    assert_eq!(st, 404);
    let (_, lib) = app.get(published_lib, Some(&f.a.token)).await;
    assert_eq!(list(&lib).len(), 0, "nada publicado ainda");

    // Só quem gere publica: participante 403, colega/outra org 404.
    let (st, v) = app
        .put(
            &publication,
            Some(&f.carla.token),
            json!({"visibility": "org"}),
        )
        .await;
    assert_eq!(st, 403, "{v}");
    assert_eq!(v["code"], "recording.not_manager");
    for who in [&f.duarte, &f.b] {
        let (st, _) = app
            .put(&publication, Some(&who.token), json!({"visibility": "org"}))
            .await;
        assert_eq!(st, 404, "{}", who.email);
        let (st, _) = app.delete(&publication, Some(&who.token)).await;
        assert_eq!(st, 404, "{}", who.email);
    }
    let (st, v) = app
        .put(
            &publication,
            Some(&f.a.token),
            json!({"visibility": "private"}),
        )
        .await;
    assert_eq!(st, 400);
    assert_eq!(v["code"], "recording.invalid_visibility");
    // Uma gravação falhada não se publica.
    sql(
        app,
        "UPDATE recordings SET status = 'failed' WHERE id = $1::uuid",
        &f.rec,
    )
    .await;
    let (st, v) = app
        .put(&publication, Some(&f.a.token), json!({"visibility": "org"}))
        .await;
    assert_eq!(st, 409, "{v}");
    assert_eq!(v["code"], "recording.no_file");
    sql(
        app,
        "UPDATE recordings SET status = 'ready' WHERE id = $1::uuid",
        &f.rec,
    )
    .await;

    // Publica: 200 com o item.
    let (st, m) = app
        .put(&publication, Some(&f.a.token), json!({"visibility": "org"}))
        .await;
    assert_eq!(st, 200, "{m}");
    assert_eq!(m["visibility"], "org");
    assert_eq!(m["state"], "published");
    let first_published_at = m["published_at"].clone();
    assert!(first_published_at.is_string());
    // Idempotente: publicar outra vez não mexe na data.
    let (st, m) = app
        .put(&publication, Some(&f.a.token), json!({"visibility": "org"}))
        .await;
    assert_eq!(st, 200);
    assert_eq!(m["published_at"], first_published_at);

    // DEPOIS: o colega activo vê, reproduz e comenta; não gere nem descarrega.
    let (st, m) = app.get(&item, Some(&f.duarte.token)).await;
    assert_eq!(st, 200, "{m}");
    assert_eq!(m["can_manage"], false);
    assert_eq!(m["can_download"], false);
    let (_, lib) = app.get(published_lib, Some(&f.duarte.token)).await;
    assert_eq!(ids(list(&lib)), vec![f.rec.clone()]);
    let (_, mine) = app.get("/api/recordings", Some(&f.duarte.token)).await;
    assert_eq!(list(&mine).len(), 0, "publicada não é «minha»");
    let (st, v) = app
        .patch(&item, Some(&f.duarte.token), json!({"description": "x"}))
        .await;
    assert_eq!(st, 403, "vê e não gere: {v}");
    let (st, v) = app
        .get(&format!("{item}/content?dl=1"), Some(&f.duarte.token))
        .await;
    assert_eq!(st, 403, "publicar não dá download: {v}");
    assert_eq!(v["code"], "recording.download_forbidden");
    let (st, _) = app
        .get(&format!("{item}/content"), Some(&f.duarte.token))
        .await;
    assert_eq!(st, 404, "reprodução autorizada; sem ficheiro no disco");
    let (st, _) = app
        .post(
            &format!("{item}/comments"),
            Some(&f.duarte.token),
            json!({"body": "boa aula"}),
        )
        .await;
    assert_eq!(st, 201);
    // Publicar dá reprodução, não a transcrição nem quem esteve na sala —
    // um colega que nunca participou continua sem os ver (R230).
    let (st, v) = app
        .get(&format!("{item}/transcript"), Some(&f.duarte.token))
        .await;
    assert_eq!(st, 404, "publicar não abre a transcrição: {v}");
    let (st, v) = app
        .get(&format!("{item}/participants"), Some(&f.duarte.token))
        .await;
    assert_eq!(st, 404, "publicar não abre quem esteve na sala: {v}");
    // Controlo: quem participou de facto continua a ver os dois.
    let (st, _) = app
        .get(&format!("{item}/transcript"), Some(&f.carla.token))
        .await;
    assert_eq!(st, 200, "participante continua a ver a transcrição");
    let (st, _) = app
        .get(&format!("{item}/participants"), Some(&f.carla.token))
        .await;
    assert_eq!(st, 200, "participante continua a ver quem esteve na sala");

    // Outra org: continua sem saber que existe.
    let (st, v) = app.get(&item, Some(&f.b.token)).await;
    assert_eq!(st, 404, "{v}");
    let (_, lib) = app.get(published_lib, Some(&f.b.token)).await;
    assert_eq!(list(&lib).len(), 0);

    // Membro arquivado (S3): a publicação não o traz de volta.
    app.archive_member(f.a.org(), &f.duarte.user_id).await;
    let (st, _) = app.get(&item, Some(&f.duarte.token)).await;
    assert_eq!(st, 404);
    let (_, lib) = app.get(published_lib, Some(&f.duarte.token)).await;
    assert_eq!(list(&lib).len(), 0);

    // Retirar: 204, depois 404 com código; o colega activo deixa de a ver.
    let gil = app.add_member(&f.a, "gil", "member").await;
    let (st, _) = app.get(&item, Some(&gil.token)).await;
    assert_eq!(st, 200, "controlo: colega activo vê a publicada");
    let (st, v) = app.delete(&publication, Some(&f.carla.token)).await;
    assert_eq!(st, 403, "{v}");
    let (st, _) = app.delete(&publication, Some(&f.eva.token)).await;
    assert_eq!(st, 204, "admin activo da org do dono gere");
    let (st, v) = app.delete(&publication, Some(&f.a.token)).await;
    assert_eq!(st, 404, "{v}");
    assert_eq!(v["code"], "recording.not_published");
    let (st, _) = app.get(&item, Some(&gil.token)).await;
    assert_eq!(st, 404);
    let (_, m) = app.get(&item, Some(&f.a.token)).await;
    assert_eq!(m["visibility"], "private");
    assert!(m["published_at"].is_null());
}

/// O SQL das duas bibliotecas concorda com `AccessFacts::listed_in`, para
/// actores com factos diferentes, com a gravação privada e publicada.
#[sqlx::test(migrations = "./migrations")]
async fn library_scopes_agree_with_access_facts(db: sqlx::PgPool) {
    let f = fixture(db).await;
    let app = &f.app;
    let frank = app.add_member(&f.a, "frank", "member").await;
    app.archive_member(f.a.org(), &frank.user_id).await;
    // De outra organização, sem partilha: a publicação não lhe chega.
    let gama = app.new_org("gama.test").await;
    // Partilhada com alguém de fora.
    sqlx::query(
        "INSERT INTO recording_shares (recording_id, user_id, shared_by)
         VALUES ($1::uuid, $2::uuid, $3::uuid)",
    )
    .bind(&f.rec)
    .bind(&f.b.user_id)
    .bind(&f.a.user_id)
    .execute(&app.db)
    .await
    .unwrap();

    let member = |admin: bool| AccessFacts {
        active_member: true,
        org_admin: admin,
        ..Default::default()
    };
    let actors: Vec<(&Account, AccessFacts)> = vec![
        (
            &f.a,
            AccessFacts {
                is_uploader: true,
                participant: true,
                ..member(true)
            },
        ),
        (
            &f.carla,
            AccessFacts {
                participant: true,
                ..member(false)
            },
        ),
        (&f.duarte, member(false)),
        (&f.eva, member(true)),
        (
            &f.b,
            AccessFacts {
                shared: true,
                ..Default::default()
            },
        ),
        (
            &frank,
            AccessFacts {
                archived_member: true,
                ..Default::default()
            },
        ),
        (&gama, AccessFacts::default()),
    ];
    for published in [false, true] {
        if published {
            sql(
                app,
                "UPDATE recordings SET visibility = 'org', published_at = now() WHERE id = $1::uuid",
                &f.rec,
            )
            .await;
        }
        for (who, base) in &actors {
            let facts = AccessFacts {
                published_to_my_org: published && base.active_member,
                ..*base
            };
            for (scope, query) in [
                (LibraryScope::Mine, "/api/recordings?scope=mine"),
                (LibraryScope::Published, "/api/recordings?scope=published"),
            ] {
                let (st, lib) = app.get(query, Some(&who.token)).await;
                assert_eq!(st, 200, "{lib}");
                let listed = ids(list(&lib)).contains(&f.rec);
                assert_eq!(
                    listed,
                    facts.listed_in(scope, published),
                    "{} {query} publicada={published}",
                    who.email
                );
                // E a paginada diz o mesmo que a lista.
                let (_, page) = app
                    .get(&format!("{query}&page_size=10"), Some(&who.token))
                    .await;
                assert_eq!(ids(items(&page)).contains(&f.rec), listed);
            }
            let (st, _) = app
                .get(&format!("/api/recordings/{}", f.rec), Some(&who.token))
                .await;
            assert_eq!(
                st == 200,
                facts.can_see(),
                "{} GET publicada={published}",
                who.email
            );
        }
    }
    let (st, v) = app
        .get("/api/recordings?scope=todas", Some(&f.a.token))
        .await;
    assert_eq!(st, 400);
    assert_eq!(v["code"], "recording.invalid_scope");
}

// ---------------------------------------------------------------------------
//  Capítulos
// ---------------------------------------------------------------------------

#[sqlx::test(migrations = "./migrations")]
async fn chapters_crud_bounds_uniqueness_and_access(db: sqlx::PgPool) {
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
    assert_eq!(second["t_ms"], 300_000);
    assert_eq!(second["source"], "manual");
    assert!(second.get("at_secs").is_none());
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

    // Dois capítulos no mesmo milissegundo: 409 com código.
    let (st, v) = app
        .post(
            &base,
            Some(&f.a.token),
            json!({"t_ms": 300_000, "title": "repetido"}),
        )
        .await;
    assert_eq!(st, 409, "{v}");
    assert_eq!(v["code"], "recording.chapter_timestamp_taken");

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
    let (st, last) = app
        .post(
            &base,
            Some(&f.a.token),
            json!({"t_ms": 600_000, "title": "x".repeat(200)}),
        )
        .await;
    assert_eq!(st, 201, "a duração é inclusiva e o título vai até 200");

    // A lista é o índice inteiro, por `t_ms`, em array (a UI lê-o assim).
    let (st, l) = app.get(&base, Some(&f.carla.token)).await;
    assert_eq!(st, 200, "{l}");
    let titles: Vec<&str> = l
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["title"].as_str().unwrap())
        .collect();
    assert_eq!(titles[..2], ["Abertura", "Orçamento"]);
    assert_eq!(titles.len(), 3);

    // PATCH: corrige e fica manual; conflito também dá 409.
    sql(
        app,
        "UPDATE recording_chapters SET source = 'auto' WHERE recording_id = $1::uuid",
        &f.rec,
    )
    .await;
    let (st, v) = app
        .patch(
            &location,
            Some(&f.a.token),
            json!({"t_ms": 0, "title": "choca"}),
        )
        .await;
    assert_eq!(st, 409, "{v}");
    assert_eq!(v["code"], "recording.chapter_timestamp_taken");
    let (st, v) = app
        .patch(
            &location,
            Some(&f.eva.token),
            json!({"t_ms": 310_000, "title": "Orçamento 2027"}),
        )
        .await;
    assert_eq!(st, 200, "{v}");
    assert_eq!(v["t_ms"], 310_000);
    assert_eq!(v["source"], "manual", "corrigido à mão deixa de ser auto");
    let (st, v) = app
        .patch(&location, Some(&f.a.token), json!({"t_ms": 999_999}))
        .await;
    assert_eq!(st, 400, "{v}");

    // Participante lê, não escreve; outra org e membro sem acesso: 404.
    let (st, v) = app
        .post(
            &base,
            Some(&f.carla.token),
            json!({"t_ms": 5, "title": "x"}),
        )
        .await;
    assert_eq!(st, 403, "{v}");
    let (st, _) = app
        .patch(&location, Some(&f.carla.token), json!({"title": "x"}))
        .await;
    assert_eq!(st, 403);
    let (st, _) = app.delete(&location, Some(&f.carla.token)).await;
    assert_eq!(st, 403);
    for who in [&f.b, &f.duarte] {
        let (st, v) = app.get(&base, Some(&who.token)).await;
        assert_eq!(st, 404, "{}: {v}", who.email);
        assert!(!v.to_string().contains("Orçamento"));
        let (st, _) = app.get(&location, Some(&who.token)).await;
        assert_eq!(st, 404);
        let (st, _) = app
            .post(&base, Some(&who.token), json!({"t_ms": 5, "title": "x"}))
            .await;
        assert_eq!(st, 404);
        let (st, _) = app
            .patch(&location, Some(&who.token), json!({"title": "x"}))
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
    let (_, m) = app
        .get(&format!("/api/recordings/{}", f.rec), Some(&f.a.token))
        .await;
    assert_eq!(m["chapter_count"], 2);
    assert!(last["id"].is_string());

    // Tecto de capítulos.
    sqlx::query(
        "INSERT INTO recording_chapters (recording_id, t_ms, title, source, created_by)
         SELECT $1::uuid, g * 1000, 'c' || g, 'auto', NULL FROM generate_series(1, 98) g",
    )
    .bind(&f.rec)
    .execute(&app.db)
    .await
    .unwrap();
    let (st, v) = app
        .post(
            &base,
            Some(&f.a.token),
            json!({"t_ms": 500, "title": "a mais"}),
        )
        .await;
    assert_eq!(st, 422, "{v}");
    assert_eq!(v["code"], "recording.too_many_chapters");
    let (_, l) = app.get(&base, Some(&f.a.token)).await;
    assert_eq!(l.as_array().unwrap().len(), 100);
    // Apagar a gravação leva os capítulos (FK em cascata).
    sql(app, "DELETE FROM recordings WHERE id = $1::uuid", &f.rec).await;
    let n: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM recording_chapters WHERE recording_id = $1::uuid")
            .bind(&f.rec)
            .fetch_one(&app.db)
            .await
            .unwrap();
    assert_eq!(n, 0);
}

// ---------------------------------------------------------------------------
//  Comentários
// ---------------------------------------------------------------------------

#[sqlx::test(migrations = "./migrations")]
async fn comments_t_ms_can_delete_moderation_soft_delete_and_dlp(db: sqlx::PgPool) {
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
    assert_eq!(c1["user_id"], f.carla.user_id.as_str());
    assert_eq!(c1["username"], "carla-alfa.test");
    assert_eq!(c1["t_ms"], 90_000);
    assert_eq!(c1["can_delete"], true, "a autora apaga o seu");
    for gone in ["author_id", "author_name", "at_secs"] {
        assert!(c1.get(gone).is_none(), "{gone}: {c1}");
    }
    let stored: String =
        sqlx::query_scalar("SELECT body FROM recording_comments WHERE id = $1::uuid")
            .bind(c1["id"].as_str().unwrap())
            .fetch_one(&app.db)
            .await
            .unwrap();
    assert!(!stored.contains(&key), "a chave não chega à base: {stored}");

    // O dono comenta sem marca e com marca anterior; o admin que não participou também.
    let (st, general) = app
        .post(
            &base,
            Some(&f.a.token),
            json!({"body": "geral", "t_ms": null}),
        )
        .await;
    assert_eq!(st, 201, "{general}");
    assert!(general["t_ms"].is_null());
    let (st, _) = app
        .post(
            &base,
            Some(&f.a.token),
            json!({"body": "no início", "t_ms": 5000}),
        )
        .await;
    assert_eq!(st, 201);
    let (st, v) = app
        .post(
            &base,
            Some(&f.eva.token),
            json!({"body": "do admin", "t_ms": 5000}),
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

    // Ordem: marca temporal (5 s, 5 s, 90 s), sem marca no fim; empates por criação.
    // `can_delete` é de quem pede: a participante só apaga o seu.
    let (st, p) = app.get(&base, Some(&f.carla.token)).await;
    assert_eq!(st, 200, "{p}");
    let seen: Vec<(&str, bool)> = items(&p)
        .iter()
        .map(|c| {
            (
                c["body"].as_str().unwrap(),
                c["can_delete"].as_bool().unwrap(),
            )
        })
        .collect();
    assert_eq!(seen[0], ("no início", false));
    assert_eq!(seen[1], ("do admin", false));
    assert!(seen[2].1, "o da própria");
    assert_eq!(seen[3], ("geral", false));
    let (_, p_owner) = app.get(&base, Some(&f.a.token)).await;
    assert!(
        items(&p_owner).iter().all(|c| c["can_delete"] == true),
        "quem gere modera: {p_owner}"
    );
    // Paginação percorre tudo sem repetir.
    let mut ids_seen = Vec::new();
    let mut token: Option<String> = None;
    loop {
        let url = match &token {
            Some(t) => format!("{base}?page_size=1&page_token={t}"),
            None => format!("{base}?page_size=1"),
        };
        let (st, p) = app.get(&url, Some(&f.a.token)).await;
        assert_eq!(st, 200, "{p}");
        assert!(items(&p).len() <= 1);
        ids_seen.extend(ids(items(&p)));
        token = p["next_page_token"].as_str().map(str::to_string);
        if token.is_none() {
            break;
        }
    }
    assert_eq!(ids_seen, ids(items(&p)));

    // Só a autora altera — nem o dono da gravação.
    let (st, v) = app
        .patch(&location, Some(&f.a.token), json!({"body": "forjado"}))
        .await;
    assert_eq!(st, 403, "{v}");
    assert_eq!(v["code"], "recording.not_comment_author");
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
        .patch(&location, Some(&f.carla.token), json!({"t_ms": 999_999}))
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

    // A participante não apaga o do dono (403); o dono modera o dela (204).
    let general_loc = format!("{base}/{}", general["id"].as_str().unwrap());
    let (st, v) = app.delete(&general_loc, Some(&f.carla.token)).await;
    assert_eq!(st, 403, "{v}");
    assert_eq!(v["code"], "recording.comment_delete_forbidden");
    let (st, _) = app.delete(&location, Some(&f.a.token)).await;
    assert_eq!(st, 204, "quem gere apaga o comentário de outra pessoa");

    // Apagar é lógico: some da lista e do GET, fica na base.
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
    let (_, m) = app
        .get(&format!("/api/recordings/{}", f.rec), Some(&f.a.token))
        .await;
    assert_eq!(m["comment_count"], 3, "os apagados não contam");
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
//  Visualizações, participantes, miniatura, transcrição
// ---------------------------------------------------------------------------

#[sqlx::test(migrations = "./migrations")]
async fn views_count_once_per_person_per_day(db: sqlx::PgPool) {
    let f = fixture(db).await;
    let app = &f.app;
    let views = format!("/api/recordings/{}/views", f.rec);
    let item = format!("/api/recordings/{}", f.rec);

    for _ in 0..3 {
        let (st, v) = app.post(&views, Some(&f.carla.token), json!({})).await;
        assert_eq!(st, 204, "{v}");
    }
    let (_, m) = app.get(&item, Some(&f.a.token)).await;
    assert_eq!(m["view_count"], 1, "recarregar não é audiência");
    let (st, _) = app.post(&views, Some(&f.a.token), json!({})).await;
    assert_eq!(st, 204);
    let (_, m) = app.get(&item, Some(&f.a.token)).await;
    assert_eq!(m["view_count"], 2);
    // No dia seguinte a mesma pessoa conta outra vez.
    sqlx::query(
        "UPDATE recording_views SET viewed_on = viewed_on - 1
          WHERE recording_id = $1::uuid AND user_id = $2::uuid",
    )
    .bind(&f.rec)
    .bind(&f.carla.user_id)
    .execute(&app.db)
    .await
    .unwrap();
    let (st, _) = app.post(&views, Some(&f.carla.token), json!({})).await;
    assert_eq!(st, 204);
    let (_, m) = app.get(&item, Some(&f.a.token)).await;
    assert_eq!(m["view_count"], 3);

    // Sem acesso: 404, e nada conta.
    for who in [&f.b, &f.duarte] {
        let (st, _) = app.post(&views, Some(&who.token), json!({})).await;
        assert_eq!(st, 404);
    }
    let (_, m) = app.get(&item, Some(&f.a.token)).await;
    assert_eq!(m["view_count"], 3);
    // Gravação falhada: não há o que ver.
    sql(
        app,
        "UPDATE recordings SET status = 'failed' WHERE id = $1::uuid",
        &f.rec,
    )
    .await;
    let (st, v) = app.post(&views, Some(&f.carla.token), json!({})).await;
    assert_eq!(st, 409, "{v}");
    assert_eq!(v["code"], "recording.no_file");
}

#[sqlx::test(migrations = "./migrations")]
async fn participants_paginated_for_recording_and_room(db: sqlx::PgPool) {
    let f = fixture(db).await;
    let app = &f.app;
    participate(app, &f.room_id, &f.eva.user_id).await;
    let code = room_code(app, &f.room_id).await;

    for path in [
        format!("/api/recordings/{}/participants", f.rec),
        format!("/api/rooms/{code}/participants"),
    ] {
        let mut seen = Vec::new();
        let mut token: Option<String> = None;
        loop {
            let url = match &token {
                Some(t) => format!("{path}?page_size=2&page_token={t}"),
                None => format!("{path}?page_size=2"),
            };
            let (st, p) = app.get(&url, Some(&f.carla.token)).await;
            assert_eq!(st, 200, "{url}: {p}");
            assert!(items(&p).len() <= 2);
            for it in items(&p) {
                assert!(it["joined_at"].is_string() && it["username"].is_string());
                seen.push(it["user_id"].as_str().unwrap().to_string());
            }
            token = p["next_page_token"].as_str().map(str::to_string);
            if token.is_none() {
                break;
            }
        }
        let mut expected = vec![
            f.a.user_id.clone(),
            f.carla.user_id.clone(),
            f.eva.user_id.clone(),
        ];
        let mut got = seen.clone();
        got.sort();
        expected.sort();
        assert_eq!(got, expected, "{path}: todos, sem repetir");

        let (st, v) = app
            .get(&format!("{path}?page_token=lixo"), Some(&f.carla.token))
            .await;
        assert_eq!(st, 400, "{v}");
        assert_eq!(v["code"], "page.invalid_token");
        // Quem não participou (e não chega à gravação) e outra org: 404.
        for who in [&f.duarte, &f.b] {
            let (st, v) = app.get(&path, Some(&who.token)).await;
            assert_eq!(st, 404, "{path} {}: {v}", who.email);
            assert!(!v.to_string().contains("carla"));
        }
    }
    let (st, _) = app
        .get("/api/rooms/nao-existe/participants", Some(&f.a.token))
        .await;
    assert_eq!(st, 404);
}

#[sqlx::test(migrations = "./migrations")]
async fn thumbnail_served_only_when_there_is_one(db: sqlx::PgPool) {
    let f = fixture(db).await;
    let app = &f.app;
    let thumb = format!("/api/recordings/{}/thumbnail", f.rec);
    let (st, _) = app.get(&thumb, Some(&f.a.token)).await;
    assert_eq!(st, 404, "sem miniatura");

    let dir = &app.state.config.recordings_dir;
    std::fs::create_dir_all(dir).unwrap();
    std::fs::write(dir.join(format!("{}.jpg", f.rec)), [0xff, 0xd8, 0xff, 0xd9]).unwrap();
    let (st, _) = app.get(&thumb, Some(&f.a.token)).await;
    assert_eq!(
        st, 404,
        "o ficheiro sozinho não chega: a base diz has_thumbnail"
    );
    sql(
        app,
        "UPDATE recordings SET has_thumbnail = true WHERE id = $1::uuid",
        &f.rec,
    )
    .await;
    let bearer = format!("Bearer {}", f.carla.token);
    let r = app
        .raw(
            reqwest::Method::GET,
            &thumb,
            &[("authorization", &bearer)],
            None,
        )
        .await;
    assert_eq!(r.status, 200);
    assert_eq!(r.header("content-type").as_deref(), Some("image/jpeg"));
    for who in [&f.b, &f.duarte] {
        let (st, _) = app.get(&thumb, Some(&who.token)).await;
        assert_eq!(st, 404);
    }
    let _ = std::fs::remove_dir_all(dir);
}

#[sqlx::test(migrations = "./migrations")]
async fn transcript_states_segments_and_access(db: sqlx::PgPool) {
    let f = fixture(db).await;
    let app = &f.app;
    let path = format!("/api/recordings/{}/transcript", f.rec);

    let (st, t) = app.get(&path, Some(&f.carla.token)).await;
    assert_eq!(st, 200, "{t}");
    assert_eq!(t["status"], "none");
    assert_eq!(t["text"], "");
    assert_eq!(t["segments"], json!([]));
    assert_eq!(t["recording_id"], f.rec.as_str());

    // Uma falha a repetir não é mostrada; uma desistência é.
    sql(
        app,
        "UPDATE recordings SET transcription_error = 'GPU ocupada' WHERE id = $1::uuid",
        &f.rec,
    )
    .await;
    let (_, t) = app.get(&path, Some(&f.carla.token)).await;
    assert!(t["error"].is_null(), "{t}");
    sql(
        app,
        "UPDATE recordings SET transcription_failed_at = now() WHERE id = $1::uuid",
        &f.rec,
    )
    .await;
    let (_, t) = app.get(&path, Some(&f.carla.token)).await;
    assert_eq!(t["status"], "failed");
    assert_eq!(t["error"], "GPU ocupada");

    sql(
        app,
        r#"UPDATE recordings SET transcription_failed_at = NULL, transcription_error = NULL,
                  transcribed_at = now(), transcript = 'olá a todos boa tarde',
                  transcript_language = 'pt', transcript_confidence = 0.85,
                  transcript_segments = '[
                    {"start_ms": 2000, "end_ms": 3500, "text": "boa tarde", "confidence": 0.5},
                    {"start_ms": 0, "end_ms": 1800, "text": " olá a todos "},
                    {"start_ms": 4000, "end_ms": 3000, "text": "incoerente"},
                    {"start_ms": "x", "text": "forma errada"}
                  ]'::jsonb
            WHERE id = $1::uuid"#,
        &f.rec,
    )
    .await;
    let (st, t) = app.get(&path, Some(&f.carla.token)).await;
    assert_eq!(st, 200, "{t}");
    assert_eq!(t["status"], "ready");
    assert_eq!(t["language"], "pt");
    assert!((t["confidence"].as_f64().unwrap() - 0.85).abs() < 1e-4);
    assert_eq!(t["text"], "olá a todos boa tarde");
    assert!(t["transcribed_at"].is_string());
    assert_eq!(
        t["segments"],
        json!([
            {"start_ms": 0, "end_ms": 1800, "text": "olá a todos", "confidence": null},
            {"start_ms": 2000, "end_ms": 3500, "text": "boa tarde", "confidence": 0.5},
        ])
    );
    let (_, m) = app
        .get(&format!("/api/recordings/{}", f.rec), Some(&f.carla.token))
        .await;
    assert_eq!(m["transcript_status"], "ready");
    assert_eq!(m["transcript_language"], "pt");

    for who in [&f.b, &f.duarte] {
        let (st, v) = app.get(&path, Some(&who.token)).await;
        assert_eq!(st, 404);
        assert!(!v.to_string().contains("boa tarde"));
    }
}

// ---------------------------------------------------------------------------
//  Legendas
// ---------------------------------------------------------------------------

const VTT: &str = "WEBVTT\n\n00:00:01.000 --> 00:00:03.000\nOlá a todos\n";

#[sqlx::test(migrations = "./migrations")]
async fn captions_viewer_sees_only_published_manager_crud(db: sqlx::PgPool) {
    let f = fixture(db).await;
    let app = &f.app;
    let base = format!("/api/recordings/{}/captions", f.rec);
    let pt = format!("{base}/pt");

    // PUT de quem gere: 201 + Location, rascunho.
    let bearer_a = format!("Bearer {}", f.a.token);
    let r = app
        .raw(
            reqwest::Method::PUT,
            &pt,
            &[("authorization", &bearer_a)],
            Some(json!({"vtt": VTT})),
        )
        .await;
    assert_eq!(r.status, 201, "{}", r.text);
    assert_eq!(r.header("location").as_deref(), Some(pt.as_str()));
    let c = r.json();
    assert_eq!(c["status"], "draft");
    assert_eq!(c["source"], "upload");
    assert!(c["published_at"].is_null());
    // Substituir: 200.
    let (st, c) = app
        .put(&pt, Some(&f.a.token), json!({"vtt": VTT, "publish": false}))
        .await;
    assert_eq!(st, 200, "{c}");

    // Quem só vê: nem na lista, nem por língua, nem o VTT.
    let (st, l) = app.get(&base, Some(&f.carla.token)).await;
    assert_eq!(st, 200);
    assert_eq!(l, json!([]));
    let (st, _) = app.get(&pt, Some(&f.carla.token)).await;
    assert_eq!(st, 404);
    let (st, _) = app.get(&format!("{pt}/vtt"), Some(&f.carla.token)).await;
    assert_eq!(st, 404);
    // Quem gere vê o rascunho.
    let (_, l) = app.get(&base, Some(&f.eva.token)).await;
    assert_eq!(l.as_array().unwrap().len(), 1);
    let (st, full) = app.get(&pt, Some(&f.eva.token)).await;
    assert_eq!(st, 200);
    assert_eq!(full["vtt"], VTT);
    assert_eq!(full["lang"], "pt");

    // Escrever é de quem gere: participante 403; outra org e colega 404.
    let (st, v) = app
        .put(&pt, Some(&f.carla.token), json!({"vtt": VTT}))
        .await;
    assert_eq!(st, 403, "{v}");
    assert_eq!(v["code"], "recording.not_manager");
    let (st, _) = app
        .patch(&pt, Some(&f.carla.token), json!({"status": "published"}))
        .await;
    assert_eq!(st, 403);
    let (st, _) = app.delete(&pt, Some(&f.carla.token)).await;
    assert_eq!(st, 403);
    for who in [&f.b, &f.duarte] {
        for (st, v) in [
            app.get(&base, Some(&who.token)).await,
            app.get(&pt, Some(&who.token)).await,
            app.get(&format!("{pt}/vtt"), Some(&who.token)).await,
            app.put(&pt, Some(&who.token), json!({"vtt": VTT})).await,
            app.patch(&pt, Some(&who.token), json!({"status": "published"}))
                .await,
            app.delete(&pt, Some(&who.token)).await,
        ] {
            assert_eq!(st, 404, "{}: {v}", who.email);
            assert!(!v.to_string().contains("Olá"));
        }
    }

    // Validação: língua, VTT (com a linha), tamanho, estado.
    for (path, body, code) in [
        (
            format!("{base}/PT_ao"),
            json!({"vtt": VTT}),
            "recording.invalid_caption_lang",
        ),
        (
            pt.clone(),
            json!({"vtt": "SRT\n\n1\n00:00:01,000 --> 00:00:02,000\nx"}),
            "recording.invalid_vtt",
        ),
        (
            pt.clone(),
            json!({"vtt": format!("WEBVTT\n\n{}", "x".repeat(2 * 1024 * 1024))}),
            "recording.caption_too_large",
        ),
    ] {
        let (st, v) = app.put(&path, Some(&f.a.token), body).await;
        assert_eq!(st, 400, "{path}: {v}");
        assert_eq!(v["code"], code);
    }
    let (st, v) = app
        .put(
            &pt,
            Some(&f.a.token),
            json!({"vtt": "WEBVTT\n\n00:00:05.000 --> 00:00:01.000\nx\n"}),
        )
        .await;
    assert_eq!(st, 400);
    assert!(v["error"].as_str().unwrap().contains("linha 3"), "{v}");
    let (st, v) = app
        .patch(&pt, Some(&f.a.token), json!({"status": "generating"}))
        .await;
    assert_eq!(st, 400);
    assert_eq!(v["code"], "recording.invalid_caption_status");

    // Publicar: a participante passa a ver, e o item lista a língua.
    let (st, c) = app
        .patch(&pt, Some(&f.a.token), json!({"status": "published"}))
        .await;
    assert_eq!(st, 200, "{c}");
    assert_eq!(c["status"], "published");
    assert!(c["published_at"].is_string());
    let (_, l) = app.get(&base, Some(&f.carla.token)).await;
    assert_eq!(l.as_array().unwrap().len(), 1);
    let bearer_c = format!("Bearer {}", f.carla.token);
    let r = app
        .raw(
            reqwest::Method::GET,
            &format!("{pt}/vtt"),
            &[("authorization", &bearer_c)],
            None,
        )
        .await;
    assert_eq!(r.status, 200);
    assert_eq!(
        r.header("content-type").as_deref(),
        Some("text/vtt; charset=utf-8")
    );
    assert_eq!(r.text, VTT);
    let (_, m) = app
        .get(&format!("/api/recordings/{}", f.rec), Some(&f.carla.token))
        .await;
    assert_eq!(m["caption_languages"], json!(["pt"]));

    // Uma legenda a gerar: não se publica nem se serve (409), e quem só vê não a vê.
    sqlx::query(
        "INSERT INTO recording_captions (recording_id, lang, source, status, progress_pct)
         VALUES ($1::uuid, 'en', 'translation', 'generating', 40)",
    )
    .bind(&f.rec)
    .execute(&app.db)
    .await
    .unwrap();
    let en = format!("{base}/en");
    let (st, v) = app
        .patch(&en, Some(&f.a.token), json!({"status": "published"}))
        .await;
    assert_eq!(st, 409, "{v}");
    assert_eq!(v["code"], "recording.caption_not_ready");
    let (st, v) = app.get(&format!("{en}/vtt"), Some(&f.a.token)).await;
    assert_eq!(st, 409, "{v}");
    let (st, _) = app.get(&en, Some(&f.carla.token)).await;
    assert_eq!(st, 404);
    let (_, m) = app
        .get(&format!("/api/recordings/{}", f.rec), Some(&f.a.token))
        .await;
    assert_eq!(m["caption_languages"], json!(["pt"]), "só as publicadas");

    // Despublicar e apagar.
    let (st, c) = app
        .patch(&pt, Some(&f.a.token), json!({"status": "draft"}))
        .await;
    assert_eq!(st, 200);
    assert!(c["published_at"].is_null());
    let (st, _) = app.get(&pt, Some(&f.carla.token)).await;
    assert_eq!(st, 404);
    let (st, _) = app.delete(&pt, Some(&f.a.token)).await;
    assert_eq!(st, 204);
    let (st, _) = app.delete(&pt, Some(&f.a.token)).await;
    assert_eq!(st, 404);
    let (st, _) = app.get(&pt, Some(&f.a.token)).await;
    assert_eq!(st, 404);
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
    assert_eq!(list(&lib).len(), 1);
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
    for sub in ["chapters", "captions", "transcript", "participants"] {
        let (st, _) = app
            .get(
                &format!("/api/recordings/{}/{sub}", f.rec),
                Some(&f.carla.token),
            )
            .await;
        assert_eq!(st, 200, "controlo positivo: {sub}");
    }

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
    for sub in [
        "chapters",
        "captions",
        "transcript",
        "participants",
        "thumbnail",
    ] {
        let (st, _) = app
            .get(
                &format!("/api/recordings/{}/{sub}", f.rec),
                Some(&f.carla.token),
            )
            .await;
        assert_eq!(st, 404, "{sub}");
    }
    let (st, _) = app
        .post(
            &format!("/api/recordings/{}/views", f.rec),
            Some(&f.carla.token),
            json!({}),
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
        "a biblioteca concorda com a regra (LIBRARY_VISIBLE_MINE)"
    );
    let (_, lib) = app
        .get("/api/recordings?q=teste&page_size=10", Some(&f.carla.token))
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
//  Pesquisa e as duas formas da biblioteca
// ---------------------------------------------------------------------------

#[sqlx::test(migrations = "./migrations")]
async fn search_finds_transcript_description_and_tags_only_in_visible_recordings(db: sqlx::PgPool) {
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

    // A encontra a sua, com excerto; nunca a da B. Sem paginação: lista (a UI).
    let (st, p) = app
        .get("/api/recordings?q=or%C3%A7amento", Some(&f.a.token))
        .await;
    assert_eq!(st, 200, "{p}");
    assert_eq!(ids(list(&p)), vec![f.rec.clone()]);
    let snippet = list(&p)[0]["snippet"].as_str().unwrap();
    assert!(snippet.contains("«orçamento»"), "{snippet}");
    assert!(!p.to_string().contains("segredo"));
    // A B encontra a dela, e só a dela.
    let (_, p) = app
        .get("/api/recordings?q=or%C3%A7amento", Some(&f.b.token))
        .await;
    assert_eq!(ids(list(&p)), vec![rec_b.clone()]);
    // Membro da A sem acesso à gravação: nada.
    let (_, p) = app
        .get("/api/recordings?q=or%C3%A7amento", Some(&f.duarte.token))
        .await;
    assert_eq!(list(&p).len(), 0);

    // Prefixo, maiúsculas, vários termos (todos obrigatórios).
    let (_, p) = app
        .get("/api/recordings?q=OR%C3%87AM%20trimes", Some(&f.a.token))
        .await;
    assert_eq!(list(&p).len(), 1, "{p}");
    let (_, p) = app
        .get(
            "/api/recordings?q=or%C3%A7amento%20inexistente",
            Some(&f.a.token),
        )
        .await;
    assert_eq!(list(&p).len(), 0);

    // Nome, descrição e etiquetas também contam, com excerto.
    let (st, _) = app
        .patch(
            &format!("/api/recordings/{}", f.rec),
            Some(&f.a.token),
            json!({"filename": "Comité de Química", "description": "reagentes perigosos",
                   "tags": ["laboratório"]}),
        )
        .await;
    assert_eq!(st, 200);
    for (q, mark) in [
        ("qu%C3%ADmica", "«Química»"),
        ("reagentes", "«reagentes»"),
        ("laborat", "«laboratório»"),
    ] {
        let (_, p) = app
            .get(&format!("/api/recordings?q={q}"), Some(&f.a.token))
            .await;
        assert_eq!(list(&p).len(), 1, "{q}: {p}");
        let snippet = list(&p)[0]["snippet"].as_str().unwrap();
        assert!(snippet.contains(mark), "{q}: {snippet}");
    }
    // Sintaxe de tsquery no texto não é interpretada.
    let (st, p) = app
        .get(
            "/api/recordings?q=or%C3%A7amento%20%26%20!%7C",
            Some(&f.a.token),
        )
        .await;
    assert_eq!(st, 200, "{p}");
    assert_eq!(list(&p).len(), 1);
    let (st, v) = app
        .get("/api/recordings?q=%21%21%21", Some(&f.a.token))
        .await;
    assert_eq!(st, 400);
    assert_eq!(v["code"], "recording.invalid_query");
}

#[sqlx::test(migrations = "./migrations")]
async fn library_is_a_list_and_paginates_on_request(db: sqlx::PgPool) {
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

    // Sem paginação: a lista inteira, como a UI a lê — também com `q`.
    let (st, lib) = app.get("/api/recordings", Some(&f.a.token)).await;
    assert_eq!(st, 200);
    assert_eq!(list(&lib).len(), 5);
    let (_, found) = app
        .get("/api/recordings?q=planeamento", Some(&f.a.token))
        .await;
    assert_eq!(list(&found).len(), 4);

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
            seen.extend(ids(items(&p)));
            token = p["next_page_token"].as_str().map(str::to_string);
            if token.is_none() {
                break;
            }
        }
        assert_eq!(seen.len(), expected, "{query}");
        let all: Vec<String> = ids(list(&lib))
            .into_iter()
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
    assert_eq!(list(&p).len(), 5, "q vazio não filtra");
}

// ---------------------------------------------------------------------------
//  Upload: onde escreve, o tipo de sessão, e a medição
// ---------------------------------------------------------------------------

async fn upload(
    app: &TestApp,
    token: &str,
    code: &str,
    query: &str,
    bytes: Vec<u8>,
) -> (u16, Value) {
    let res = app
        .http
        .post(app.url(&format!("/api/rooms/{code}/recordings?{query}")))
        .bearer_auth(token)
        .body(bytes)
        .send()
        .await
        .unwrap();
    let st = res.status().as_u16();
    (st, res.json().await.unwrap_or(Value::Null))
}

/// Sem ffprobe/ffmpeg (o caso do CI, forçado aqui com binários inexistentes),
/// o upload entra na mesma: campos `null`, `has_thumbnail = false`.
#[sqlx::test(migrations = "./migrations")]
async fn upload_writes_to_configured_dir_and_degrades_without_ffprobe(db: sqlx::PgPool) {
    let f = fixture_with(
        db,
        &[
            ("FFPROBE_BIN", "/nao-existe/ffprobe"),
            ("FFMPEG_BIN", "/nao-existe/ffmpeg"),
        ],
    )
    .await;
    let app = &f.app;
    let code = room_code(app, &f.room_id).await;
    let bytes = vec![0x1a, 0x45, 0xdf, 0xa3, 1, 2, 3];
    let (st, rec) = upload(app, &f.a.token, &code, "name=up.webm", bytes.clone()).await;
    assert_eq!(st, 200, "{rec}");
    let id = rec["id"].as_str().unwrap();
    assert_eq!(rec["filename"], "up.webm");
    assert_eq!(rec["kind"], "meeting", "sala normal = reunião");
    assert_eq!(rec["status"], "ready");
    assert_eq!(rec["has_thumbnail"], false);
    for k in [
        "duration_ms",
        "width",
        "height",
        "fps",
        "video_codec",
        "audio_codec",
    ] {
        assert!(rec[k].is_null(), "{k}: {rec}");
    }
    let on_disk = app.state.config.recordings_dir.join(format!("{id}.webm"));
    assert_eq!(
        std::fs::read(&on_disk).unwrap(),
        bytes,
        "{}",
        on_disk.display()
    );
    let probed: bool =
        sqlx::query_scalar("SELECT probed_at IS NOT NULL FROM recordings WHERE id = $1::uuid")
            .bind(id)
            .fetch_one(&app.db)
            .await
            .unwrap();
    assert!(probed, "tentou medir");

    let res = app
        .http
        .get(app.url(&format!("/api/recordings/{id}/content?dl=1")))
        .bearer_auth(&f.a.token)
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 200);
    assert_eq!(res.bytes().await.unwrap().to_vec(), bytes);

    // Tipo declarado; tipo inválido e nome inválido recusados antes de escrever.
    let (st, rec) = upload(app, &f.a.token, &code, "kind=broadcast", vec![1, 2]).await;
    assert_eq!(st, 200, "{rec}");
    assert_eq!(rec["kind"], "broadcast");
    let before: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM recordings")
        .fetch_one(&app.db)
        .await
        .unwrap();
    let (st, v) = upload(app, &f.a.token, &code, "kind=lecture", vec![1, 2]).await;
    assert_eq!(st, 400, "{v}");
    assert_eq!(v["code"], "recording.invalid_kind");
    let (st, v) = upload(app, &f.a.token, &code, "name=a%0Ab", vec![1, 2]).await;
    assert_eq!(st, 400, "{v}");
    assert_eq!(v["code"], "recording.invalid_filename");
    let after: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM recordings")
        .fetch_one(&app.db)
        .await
        .unwrap();
    assert_eq!(before, after, "nada escrito");
    let _ = std::fs::remove_dir_all(&app.state.config.recordings_dir);
}

/// Com o ffmpeg instalado, mede um webm «ao vivo» a sério e gera a miniatura.
/// Sem ele, o teste diz que NÃO correu em vez de fingir que passou.
#[sqlx::test(migrations = "./migrations")]
async fn upload_measures_a_real_webm_when_ffmpeg_exists(db: sqlx::PgPool) {
    let f = fixture(db).await;
    let app = &f.app;
    let dir = std::env::temp_dir().join(format!("dlx-upload-probe-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let src = dir.join("live.webm");
    let made = std::process::Command::new(&app.state.config.ffmpeg_bin)
        .args(["-y", "-loglevel", "error", "-f", "lavfi", "-i"])
        .arg("testsrc=size=320x240:rate=25")
        .args(["-f", "lavfi", "-i", "sine=frequency=440", "-t", "2"])
        .args([
            "-c:v", "libvpx", "-c:a", "libopus", "-live", "1", "-f", "webm",
        ])
        .arg(&src)
        .status();
    if !matches!(made, Ok(s) if s.success()) {
        eprintln!("ffmpeg indisponível — medição real no upload NÃO verificada");
        let _ = std::fs::remove_dir_all(&dir);
        return;
    }
    let code = room_code(app, &f.room_id).await;
    let bytes = std::fs::read(&src).unwrap();
    let (st, rec) = upload(app, &f.a.token, &code, "name=real.webm", bytes).await;
    assert_eq!(st, 200, "{rec}");
    assert_eq!(rec["width"], 320, "{rec}");
    assert_eq!(rec["height"], 240);
    assert_eq!(rec["video_codec"], "vp8");
    assert_eq!(rec["audio_codec"], "opus");
    let d = rec["duration_ms"].as_i64().expect("duração medida");
    assert!((1900..=2100).contains(&d), "duração medida {d} ms");
    assert_eq!(rec["has_thumbnail"], true);
    let id = rec["id"].as_str().unwrap();
    let bearer = format!("Bearer {}", f.carla.token);
    let r = app
        .raw(
            reqwest::Method::GET,
            &format!("/api/recordings/{id}/thumbnail"),
            &[("authorization", &bearer)],
            None,
        )
        .await;
    assert_eq!(r.status, 200);
    assert_eq!(r.header("content-type").as_deref(), Some("image/jpeg"));
    let (_, m) = app
        .get(&format!("/api/recordings/{id}"), Some(&f.a.token))
        .await;
    assert_eq!(m["duration_ms"], d);
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(&app.state.config.recordings_dir);
}

// ---------------------------------------------------------------------------
//  Migrações 0052–0054: conversão dos dados na forma antiga
// ---------------------------------------------------------------------------

#[sqlx::test(migrations = false)]
async fn migrations_convert_old_shape_data(db: sqlx::PgPool) {
    use sqlx::migrate::Migrator;
    let path = std::path::Path::new("./migrations");
    let mut before = Migrator::new(path).await.unwrap();
    before.migrations = std::borrow::Cow::Owned(
        before
            .migrations
            .iter()
            .filter(|m| m.version < 52)
            .cloned()
            .collect(),
    );
    before.run(&db).await.unwrap();

    // Dados na forma da 0045: segundos, `category`, `title`, dois capítulos
    // no mesmo segundo, comentário com `author_id`.
    for q in [
        "INSERT INTO users (id, email, username, password_hash) VALUES
          ('00000000-0000-4000-8000-000000000001', 'm@x.test', 'm', 'x')",
        "INSERT INTO rooms (id, code, name, owner_id) VALUES
          ('00000000-0000-4000-8000-000000000002', 'mig-rati-ons', 'r',
           '00000000-0000-4000-8000-000000000001')",
        "INSERT INTO recordings (id, room_id, uploader_id, filename, size_bytes, duration_secs, category, title, transcript) VALUES
          ('00000000-0000-4000-8000-000000000003', '00000000-0000-4000-8000-000000000002',
           '00000000-0000-4000-8000-000000000001', 'a.webm', 1, 90, 'lecture', 'Aula', 'mitocôndria'),
          ('00000000-0000-4000-8000-000000000004', '00000000-0000-4000-8000-000000000002',
           '00000000-0000-4000-8000-000000000001', 'b.webm', 1, NULL, 'other', NULL, ''),
          ('00000000-0000-4000-8000-000000000005', '00000000-0000-4000-8000-000000000002',
           '00000000-0000-4000-8000-000000000001', 'c.webm', 1, 5, 'broadcast', NULL, ''),
          ('00000000-0000-4000-8000-000000000006', '00000000-0000-4000-8000-000000000002',
           '00000000-0000-4000-8000-000000000001', 'd.webm', 1, 0, 'meeting', NULL, '')",
        "INSERT INTO recording_chapters (recording_id, at_secs, title, created_by, created_at) VALUES
          ('00000000-0000-4000-8000-000000000003', 5, 'primeiro', '00000000-0000-4000-8000-000000000001', now() - interval '1 minute'),
          ('00000000-0000-4000-8000-000000000003', 5, 'segundo',  '00000000-0000-4000-8000-000000000001', now())",
        "INSERT INTO recording_comments (recording_id, at_secs, body, author_id) VALUES
          ('00000000-0000-4000-8000-000000000003', 7, 'oi', '00000000-0000-4000-8000-000000000001'),
          ('00000000-0000-4000-8000-000000000003', NULL, 'geral', '00000000-0000-4000-8000-000000000001')",
    ] {
        sqlx::query(q).execute(&db).await.unwrap();
    }

    Migrator::new(path).await.unwrap().run(&db).await.unwrap();

    let recs: Vec<(String, Option<i64>, String, String)> = sqlx::query_as(
        "SELECT filename, duration_ms, kind, visibility FROM recordings ORDER BY id",
    )
    .fetch_all(&db)
    .await
    .unwrap();
    let expected: Vec<(String, Option<i64>, String, String)> = vec![
        (
            "Aula".into(),
            Some(90_000),
            "training".into(),
            "private".into(),
        ),
        ("b.webm".into(), None, "meeting".into(), "private".into()),
        (
            "c.webm".into(),
            Some(5_000),
            "broadcast".into(),
            "private".into(),
        ),
        ("d.webm".into(), Some(0), "meeting".into(), "private".into()),
    ];
    assert_eq!(
        recs, expected,
        "segundos→ms, lecture→training, other→meeting, título→nome"
    );
    let chapters: Vec<(String, i64, String)> =
        sqlx::query_as("SELECT title, t_ms, source FROM recording_chapters ORDER BY t_ms")
            .fetch_all(&db)
            .await
            .unwrap();
    let expected: Vec<(String, i64, String)> = vec![
        ("primeiro".into(), 5_000, "manual".into()),
        ("segundo".into(), 5_001, "manual".into()),
    ];
    assert_eq!(
        chapters, expected,
        "dois no mesmo segundo ficam os dois, afastados 1 ms"
    );
    let comments: Vec<(String, Option<i64>, String)> = sqlx::query_as(
        "SELECT body, t_ms, user_id::text FROM recording_comments ORDER BY body DESC",
    )
    .fetch_all(&db)
    .await
    .unwrap();
    let uid = "00000000-0000-4000-8000-000000000001".to_string();
    assert_eq!(
        comments,
        vec![
            ("oi".to_string(), Some(7_000), uid.clone()),
            ("geral".to_string(), None, uid),
        ]
    );
    // A pesquisa foi reconstruída sobre as colunas novas.
    let found: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM recordings WHERE search_vector @@ to_tsquery('simple', 'mitoc:* | aula:*')",
    )
    .fetch_one(&db)
    .await
    .unwrap();
    assert_eq!(found, 1);
    // As colunas antigas saíram.
    let old: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM information_schema.columns
          WHERE (table_name = 'recordings' AND column_name IN ('duration_secs', 'category', 'title'))
             OR (table_name IN ('recording_chapters', 'recording_comments') AND column_name = 'at_secs')
             OR (table_name = 'recording_comments' AND column_name = 'author_id')",
    )
    .fetch_one(&db)
    .await
    .unwrap();
    assert_eq!(old, 0);
    // Publicada ⇔ `org`: a base recusa a contradição.
    let bad = sqlx::query(
        "UPDATE recordings SET visibility = 'org' WHERE id = '00000000-0000-4000-8000-000000000004'",
    )
    .execute(&db)
    .await;
    assert!(bad.is_err(), "visibility=org sem published_at");
}
