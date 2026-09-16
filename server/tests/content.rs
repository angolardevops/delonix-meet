//! Caracterização do CONTEÚDO: salas (criar, ler, entrar, chat, convite, QoS,
//! tempos), gravações (biblioteca, download, partilha por utilizador e por
//! link público) e quadros brancos (guardar, listar, PNG, partilha pública).
//!
//! Portado de `web/e2e/isolamento.mjs` («salas da org B: o código é uma
//! CAPABILITY», «o que o código NÃO abre» e quadros).
//!
//! As gravações entram por SQL e SEM ficheiro (ver `insert_recording`): com
//! acesso, o download passa a autorização e dá 404 na leitura do ficheiro;
//! sem acesso, é recusado antes (401). É isso que distingue os dois casos.
mod common;

use common::{assert_denied, jwt_claims, TestApp, INVENTED_ID, PNG_1X1};
use serde_json::json;

// ---------------------------------------------------------------------------
//  Salas
// ---------------------------------------------------------------------------

#[sqlx::test(migrations = "./migrations")]
async fn create_and_get_room(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let a = app.new_org("alfa.test").await;
    let t = Some(a.token.as_str());
    for bad in [
        json!({"name": " "}),
        json!({"name": "n".repeat(101)}),
        json!({"name": "x", "topology": "p2p"}),
        json!({"name": "x", "format": "webinar"}),
    ] {
        let (st, body) = app.post("/api/rooms", t, bad.clone()).await;
        assert_eq!(st, 400, "{bad}: {body}");
    }
    let (st, room) = app
        .post(
            "/api/rooms",
            t,
            json!({"name": "  Sala  ", "e2ee": true, "format": "training", "topology": "mesh"}),
        )
        .await;
    assert_eq!(st, 200, "{room}");
    let code = room["code"].as_str().unwrap();
    assert_eq!(code.len(), 12);
    assert_eq!(room["name"], "Sala");
    assert_eq!(room["owner_id"], a.user_id.as_str());
    assert_eq!(room["topology"], "mesh");
    assert_eq!(room["format"], "training");
    assert_eq!(room["e2ee"], true);
    assert_eq!(room["waiting_room"], false);
    let (_, d) = app.post("/api/rooms", t, json!({"name": "omissões"})).await;
    assert_eq!(d["topology"], "sfu");
    assert_eq!(d["format"], "normal");

    let (st, got) = app
        .get(&format!("/api/rooms/{}", code.to_uppercase()), t)
        .await;
    assert_eq!(st, 200);
    assert_eq!(got["id"], room["id"]);
    let (st, _) = app.get("/api/rooms/aaa-bbbb-ccc", t).await;
    assert_eq!(st, 404);
    let (st, _) = app.get(&format!("/api/rooms/{code}"), None).await;
    assert_eq!(st, 401);
    let (st, _) = app.post("/api/rooms", None, json!({"name": "x"})).await;
    assert_eq!(st, 401);
}

#[sqlx::test(migrations = "./migrations")]
async fn join_room_token_reflects_access(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let a = app.new_org("alfa.test").await;
    let b = app.new_org("beta.test").await;
    let c = app.add_member(&a, "carla", "member").await;
    let room = app.new_room(&a, "sala da A").await;
    let code = room["code"].as_str().unwrap();
    let join = |tok: String| {
        let app = &app;
        let path = format!("/api/rooms/{code}/join");
        async move {
            let (st, body) = app.post(&path, Some(&tok), json!({})).await;
            assert_eq!(st, 200, "{body}");
            body
        }
    };

    // Dono: entra directo e admite.
    let j = join(a.token.clone()).await;
    assert_eq!(j["room"]["code"], code);
    assert_eq!(j["scheduled"], false);
    assert!(j["ws_path"].as_str().unwrap().starts_with("/ws?token="));
    let cl = jwt_claims(j["room_token"].as_str().unwrap());
    assert_eq!(cl["typ"], "room");
    assert_eq!(cl["owner"], true);
    assert_eq!(cl["adm"], true);
    assert!(
        cl.get("wait").is_none(),
        "wait=false não é serializado: {cl}"
    );
    assert_eq!(cl["room"], room["id"]);
    assert_eq!(cl["name"], "admin-alfa.test");
    assert_eq!(cl["topo"], "sfu");

    // Colega da mesma org: autorizado, mas por sala de espera.
    let cl = jwt_claims(join(c.token.clone()).await["room_token"].as_str().unwrap());
    assert!(cl.get("owner").is_none());
    assert_eq!(cl["wait"], true);
    // Outra org: o código é uma capability — recebe token, mas com espera.
    let cl = jwt_claims(join(b.token.clone()).await["room_token"].as_str().unwrap());
    assert_eq!(cl["wait"], true);
    assert!(cl.get("adm").is_none());

    // Só os AUTORIZADOS ficam como participantes.
    let parts: Vec<(uuid::Uuid,)> = sqlx::query_as(
        "SELECT user_id FROM room_participants WHERE room_id = $1::uuid ORDER BY user_id",
    )
    .bind(room["id"].as_str().unwrap())
    .fetch_all(&app.db)
    .await
    .unwrap();
    let parts: Vec<String> = parts.into_iter().map(|p| p.0.to_string()).collect();
    assert!(parts.contains(&a.user_id) && parts.contains(&c.user_id));
    assert!(!parts.contains(&b.user_id), "a org B não é participante");

    let (st, _) = app
        .post("/api/rooms/aaa-bbbb-ccc/join", Some(&a.token), json!({}))
        .await;
    assert_eq!(st, 404);
}

#[sqlx::test(migrations = "./migrations")]
async fn join_scheduled_room_invitee_enters_directly(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let a = app.new_org("alfa.test").await;
    let c = app.add_member(&a, "carla", "member").await;
    let m = app.new_meeting(&a, "agendada", &[&c.user_id]).await;
    let (_, s) = app
        .post(
            &format!("/api/meetings/{}/start", m["id"].as_str().unwrap()),
            Some(&a.token),
            json!({}),
        )
        .await;
    let code = s["code"].as_str().unwrap();
    let (st, j) = app
        .post(
            &format!("/api/rooms/{code}/join"),
            Some(&c.token),
            json!({}),
        )
        .await;
    assert_eq!(st, 200);
    assert_eq!(j["scheduled"], true);
    let cl = jwt_claims(j["room_token"].as_str().unwrap());
    assert!(cl.get("wait").is_none(), "convidado entra directo: {cl}");
}

#[sqlx::test(migrations = "./migrations")]
async fn waiting_room_applies_even_to_owner_token(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let a = app.new_org("alfa.test").await;
    let (_, room) = app
        .post(
            "/api/rooms",
            Some(&a.token),
            json!({"name": "com espera", "waiting_room": true}),
        )
        .await;
    let (_, j) = app
        .post(
            &format!("/api/rooms/{}/join", room["code"].as_str().unwrap()),
            Some(&a.token),
            json!({}),
        )
        .await;
    let cl = jwt_claims(j["room_token"].as_str().unwrap());
    // `wait = waiting_room || !direct`: o token do dono também leva `wait`;
    // é a sinalização que decide com `owner`.
    assert_eq!(cl["wait"], true);
    assert_eq!(cl["owner"], true);
}

#[sqlx::test(migrations = "./migrations")]
async fn room_chat_invite_qos_timings_access(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let a = app.new_org("alfa.test").await;
    let b = app.new_org("beta.test").await;
    let c = app.add_member(&a, "carla", "member").await;
    let room = app.new_room(&a, "sala privada").await;
    let code = room["code"].as_str().unwrap();

    // Chat
    for tok in [&a.token, &c.token] {
        let (st, body) = app
            .get(&format!("/api/rooms/{code}/messages"), Some(tok))
            .await;
        assert_eq!(st, 200);
        assert_eq!(body, json!([]));
    }
    let (st, _) = app
        .get(&format!("/api/rooms/{code}/messages"), Some(&b.token))
        .await;
    assert_eq!(st, 403);
    let (st, _) = app
        .get("/api/rooms/aaa-bbbb-ccc/messages", Some(&a.token))
        .await;
    assert_eq!(st, 404);

    // Convite
    let inv = format!("/api/rooms/{code}/invitations");
    let (st, _) = app
        .post(&inv, Some(&b.token), json!({"targets": [a.user_id]}))
        .await;
    assert_eq!(st, 403);
    let (st, _) = app.post(&inv, Some(&a.token), json!({"targets": []})).await;
    assert_eq!(st, 400);
    let (st, _) = app
        .post(
            &inv,
            Some(&a.token),
            json!({"targets": [c.user_id], "kind": "fax"}),
        )
        .await;
    assert_eq!(st, 400);
    let (st, body) = app
        .post(&inv, Some(&a.token), json!({"targets": [b.user_id]}))
        .await;
    assert_eq!(st, 400, "só colegas de org são alvos válidos: {body}");
    // Corpo com a forma antiga (`user_ids`) é recusado pelo extractor.
    let (st, _) = app
        .post(&inv, Some(&a.token), json!({"user_ids": []}))
        .await;
    assert_eq!(st, 422);
    let (st, body) = app
        .post(
            &inv,
            Some(&a.token),
            json!({"targets": [c.user_id, b.user_id, a.user_id]}),
        )
        .await;
    assert_eq!(st, 200, "{body}");
    assert_eq!(body, json!({"ringing": [], "offline": [c.user_id]}));

    // QoS e tempos
    let qos = format!("/api/rooms/{code}/quality-samples");
    let sample = json!({"rtt_ms": 40, "loss_pct": 1.5, "up_kbps": 900, "score": 250,
                        "turn_relay": true, "limited_by": "cpu", "candidate_pair": "relay/srflx"});
    let (st, _) = app.post(&qos, Some(&b.token), sample.clone()).await;
    assert_eq!(st, 403);
    let (st, body) = app.post(&qos, Some(&c.token), sample).await;
    assert_eq!(st, 200);
    assert_eq!(body, json!({"ok": true}));
    let timings = format!("/api/rooms/{code}/join-timings");
    let (st, _) = app
        .post(&timings, Some(&b.token), json!({"join_ms": 1}))
        .await;
    assert_eq!(st, 403);
    let (st, body) = app
        .post(&timings, Some(&a.token), json!({"join_ms": 900_000}))
        .await;
    assert_eq!(st, 200);
    assert_eq!(body, json!({"ok": true}));
    let join_ms: Option<i32> = sqlx::query_scalar("SELECT join_ms FROM call_timings LIMIT 1")
        .fetch_one(&app.db)
        .await
        .unwrap();
    assert_eq!(join_ms, Some(600_000), "clamp a 10 min");

    // A amostra entra nas estatísticas da org (score preso a 100).
    let (_, s) = app
        .get(&format!("/api/orgs/{}/stats", a.org()), Some(&a.token))
        .await;
    assert_eq!(s["quality_samples_30d"], 1);
    assert_eq!(s["avg_score"], 100);
    assert_eq!(s["pct_turn_relay"], 100);
    assert_eq!(s["pct_cpu_limited"], 100);
    assert_eq!(s["pct_good"], 100);
}

// ---------------------------------------------------------------------------
//  Gravações
// ---------------------------------------------------------------------------

#[sqlx::test(migrations = "./migrations")]
async fn recordings_library_and_room_list(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let a = app.new_org("alfa.test").await;
    let b = app.new_org("beta.test").await;
    let (st, lib) = app.get("/api/recordings", Some(&a.token)).await;
    assert_eq!(st, 200);
    assert_eq!(lib, json!([]));
    let (st, _) = app.get("/api/recordings", None).await;
    assert_eq!(st, 401);

    let room = app.new_room(&a, "gravada").await;
    let code = room["code"].as_str().unwrap();
    let rec = app
        .insert_recording(room["id"].as_str().unwrap(), &a.user_id)
        .await;

    let (_, lib) = app.get("/api/recordings", Some(&a.token)).await;
    assert_eq!(lib.as_array().unwrap().len(), 1);
    assert_eq!(lib[0]["id"], rec.as_str());
    assert_eq!(lib[0]["room_code"], code);
    assert_eq!(lib[0]["uploader_name"], "admin-alfa.test");
    assert_eq!(lib[0]["owned"], false, "owned = participante da sala");
    assert_eq!(lib[0]["can_download"], true);
    assert_eq!(lib[0]["status"], "ready");
    assert_eq!(lib[0]["share_count"], 0);
    let (_, lib) = app.get("/api/recordings", Some(&b.token)).await;
    assert_eq!(lib, json!([]));

    // Lista por sala: só participantes (o dono que nunca entrou também não).
    let path = format!("/api/rooms/{code}/recordings");
    let (st, _) = app.get(&path, Some(&a.token)).await;
    assert_eq!(st, 401);
    app.post(
        &format!("/api/rooms/{code}/join"),
        Some(&a.token),
        json!({}),
    )
    .await;
    let (st, list) = app.get(&path, Some(&a.token)).await;
    assert_eq!(st, 200);
    assert_eq!(list[0]["id"], rec.as_str());
    let (st, body) = app.get(&path, Some(&b.token)).await;
    assert_denied("lista de gravações da sala da A", st, &body, &rec);

    // Upload: corpo vazio 400; não-participante 401. (O caminho feliz não se
    // exercita aqui: escreveria em `./recordings` — ver DÍVIDA no relatório.)
    let r = app
        .http
        .post(app.url(&format!("{path}?name=x.webm")))
        .bearer_auth(&a.token)
        .body(Vec::<u8>::new())
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 400);
    let r = app
        .http
        .post(app.url(&format!("{path}?name=x.webm")))
        .bearer_auth(&b.token)
        .body(vec![0x1a, 0x45, 0xdf, 0xa3])
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 401);
}

#[sqlx::test(migrations = "./migrations")]
async fn recording_download_share_and_links(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let a = app.new_org("alfa.test").await;
    let b = app.new_org("beta.test").await;
    let c = app.add_member(&a, "carla", "member").await;
    let d = app.add_member(&a, "dario", "admin").await;
    let room = app.new_room(&a, "gravada").await;
    let rec = app
        .insert_recording(room["id"].as_str().unwrap(), &a.user_id)
        .await;
    let base = format!("/api/recordings/{rec}");
    // O ficheiro vive em `/content`; `GET {base}` são os metadados.
    let content = format!("{base}/content");

    // Metadados: o dono lê; a org B não vê que existe (404).
    let (st, meta) = app.get(&base, Some(&a.token)).await;
    assert_eq!(st, 200, "{meta}");
    assert_eq!(meta["id"], rec.as_str());
    let (st, body) = app.get(&base, Some(&b.token)).await;
    assert_eq!(st, 404, "metadados da gravação da A para a org B: {body}");
    assert_denied("metadados da gravação da A", st, &body, "teste.webm");

    // Download: dono e admin da org passam a autorização (404 = sem ficheiro);
    // a org B e um membro sem partilha são recusados antes.
    let (st, _) = app.get(&content, Some(&a.token)).await;
    assert_eq!(st, 404);
    let (st, _) = app.get(&format!("{content}?dl=1"), Some(&d.token)).await;
    assert_eq!(st, 404);
    let (st, _) = app.get(&format!("{content}?dl=1"), Some(&c.token)).await;
    assert_eq!(st, 401);
    let (st, _) = app.get(&content, Some(&c.token)).await;
    assert_eq!(st, 401);
    let (st, _) = app.get(&content, Some(&b.token)).await;
    assert_eq!(st, 401);
    let (st, _) = app.get(&format!("{content}?dl=1"), Some(&b.token)).await;
    assert_eq!(st, 401);
    let (st, _) = app
        .get(&format!("/api/recordings/{INVENTED_ID}/content"), Some(&a.token))
        .await;
    assert_eq!(st, 404);
    // Gravação falhada: 400 com o motivo, antes da autorização.
    sqlx::query("UPDATE recordings SET status = 'failed', failure_reason = 'sem espaço' WHERE id = $1::uuid")
        .bind(&rec)
        .execute(&app.db)
        .await
        .unwrap();
    let (st, body) = app.get(&content, Some(&b.token)).await;
    assert_eq!(st, 400);
    assert_eq!(body["error"], "sem espaço");
    sqlx::query(
        "UPDATE recordings SET status = 'ready', failure_reason = NULL WHERE id = $1::uuid",
    )
    .bind(&rec)
    .execute(&app.db)
    .await
    .unwrap();

    // Partilha por utilizador.
    let (st, _) = app
        .post(
            &format!("{base}/shares"),
            Some(&b.token),
            json!({"user_id": b.user_id}),
        )
        .await;
    assert_eq!(st, 401);
    let (st, _) = app
        .post(
            &format!("{base}/shares"),
            Some(&a.token),
            json!({"user_id": a.user_id}),
        )
        .await;
    assert_eq!(st, 400, "partilhar consigo próprio");
    let (st, body) = app
        .post(
            &format!("{base}/shares"),
            Some(&a.token),
            json!({"user_id": c.user_id}),
        )
        .await;
    assert_eq!(st, 200);
    assert_eq!(body, json!({"ok": true}));
    let (_, shares) = app.get(&format!("{base}/shares"), Some(&a.token)).await;
    assert_eq!(shares[0]["id"], c.user_id.as_str());
    let (st, _) = app.get(&format!("{base}/shares"), Some(&c.token)).await;
    assert_eq!(st, 401);
    // Com partilha, C vê inline (404 = autorizada) mas não descarrega.
    let (st, _) = app.get(&content, Some(&c.token)).await;
    assert_eq!(st, 404);
    let (st, _) = app.get(&format!("{content}?dl=1"), Some(&c.token)).await;
    assert_eq!(st, 401);
    let (_, lib) = app.get("/api/recordings", Some(&c.token)).await;
    assert_eq!(lib[0]["can_download"], false);
    let (st, _) = app
        .delete(&format!("{base}/shares/{}", c.user_id), Some(&b.token))
        .await;
    assert_eq!(st, 401);
    let (st, _) = app
        .delete(&format!("{base}/shares/{}", c.user_id), Some(&a.token))
        .await;
    assert_eq!(st, 200);
    let (_, shares) = app.get(&format!("{base}/shares"), Some(&a.token)).await;
    assert_eq!(shares, json!([]));
    let (st, _) = app
        .delete(
            &format!("/api/recordings/{INVENTED_ID}/shares/{INVENTED_ID}"),
            Some(&a.token),
        )
        .await;
    assert_eq!(st, 404);

    // Link público com password.
    let link = format!("{base}/public-link");
    let (st, _) = app.put(&link, Some(&b.token), json!({})).await;
    assert_eq!(st, 401);
    let (st, body) = app.get(&link, Some(&a.token)).await;
    assert_eq!(st, 200);
    assert!(body.is_null());
    let (st, l) = app
        .put(&link, Some(&a.token), json!({"password": "abrir"}))
        .await;
    assert_eq!(st, 200, "{l}");
    let token = l["token"].as_str().unwrap().to_string();
    assert_eq!(token.len(), 32);
    assert!(l.get("password_hash").is_none());
    let (_, got) = app.get(&link, Some(&a.token)).await;
    assert_eq!(got["token"], token.as_str());

    let (st, _) = app
        .get(&format!("/api/public/recordings/{token}"), None)
        .await;
    assert_eq!(st, 401, "sem password");
    let (st, _) = app
        .get(
            &format!("/api/public/recordings/{token}?password=errada"),
            None,
        )
        .await;
    assert_eq!(st, 401);
    let (st, pubv) = app
        .get(
            &format!("/api/public/recordings/{token}?password=abrir"),
            None,
        )
        .await;
    assert_eq!(st, 200, "{pubv}");
    assert_eq!(pubv["recording_id"], rec.as_str());
    assert_eq!(pubv["filename"], "teste.webm");
    assert_eq!(pubv["has_password"], true);
    assert_eq!(
        pubv["download_url"],
        format!("/api/public/recordings/{token}/content")
    );
    let (st, _) = app
        .get(
            &format!("/api/public/recordings/{token}/content?password=abrir"),
            None,
        )
        .await;
    assert_eq!(st, 404, "autorizado; sem ficheiro");

    // Recriar roda o token; o antigo morre.
    let (_, l2) = app.put(&link, Some(&a.token), json!({})).await;
    let token2 = l2["token"].as_str().unwrap().to_string();
    assert_ne!(token, token2);
    let (st, _) = app
        .get(&format!("/api/public/recordings/{token}"), None)
        .await;
    assert_eq!(st, 404);
    let (st, pubv) = app
        .get(&format!("/api/public/recordings/{token2}"), None)
        .await;
    assert_eq!(st, 200);
    assert_eq!(pubv["has_password"], false);

    // Expirado: 404.
    sqlx::query("UPDATE recording_share_links SET expires_at = now() - interval '1 minute'")
        .execute(&app.db)
        .await
        .unwrap();
    let (st, _) = app
        .get(&format!("/api/public/recordings/{token2}"), None)
        .await;
    assert_eq!(st, 404);

    let (st, _) = app.delete(&link, Some(&b.token)).await;
    assert_eq!(st, 401);
    let (st, body) = app.delete(&link, Some(&a.token)).await;
    assert_eq!(st, 200);
    assert_eq!(body, json!({"ok": true}));
    let (_, got) = app.get(&link, Some(&a.token)).await;
    assert!(got.is_null());
    let (st, _) = app
        .put(
            &format!("/api/recordings/{INVENTED_ID}/public-link"),
            Some(&a.token),
            json!({}),
        )
        .await;
    assert_eq!(st, 404);
}

#[sqlx::test(migrations = "./migrations")]
async fn public_share_garbage_token_is_404(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    for p in [
        "/api/public/recordings/lixo",
        "/api/public/recordings/lixo/content",
        "/api/public/whiteboards/lixo/image",
    ] {
        let (st, _) = app.get(p, None).await;
        assert_eq!(st, 404, "{p}");
    }
}

/// DÍVIDA: `recordings::share` aceita qualquer `user_id` — incluindo alguém
/// de OUTRA organização — sem verificar que partilha org com o dono. A
/// gravação aparece na biblioteca dessa pessoa e o acesso inline passa a
/// autorização.
#[sqlx::test(migrations = "./migrations")]
async fn recording_share_current_behavior_allows_foreign_org_user(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let a = app.new_org("alfa.test").await;
    let b = app.new_org("beta.test").await;
    let room = app.new_room(&a, "gravada").await;
    let rec = app
        .insert_recording(room["id"].as_str().unwrap(), &a.user_id)
        .await;
    let (st, _) = app
        .post(
            &format!("/api/recordings/{rec}/shares"),
            Some(&a.token),
            json!({"user_id": b.user_id}),
        )
        .await;
    assert_eq!(st, 200);
    let (_, lib) = app.get("/api/recordings", Some(&b.token)).await;
    assert_eq!(lib[0]["id"], rec.as_str());
    let (st, _) = app
        .get(&format!("/api/recordings/{rec}/content"), Some(&b.token))
        .await;
    assert_eq!(st, 404, "passou a autorização (só falta o ficheiro)");
}

// ---------------------------------------------------------------------------
//  Quadros brancos
// ---------------------------------------------------------------------------

#[sqlx::test(migrations = "./migrations")]
async fn whiteboards_save_list_png_share_delete(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let a = app.new_org("alfa.test").await;
    let b = app.new_org("beta.test").await;
    let c = app.add_member(&a, "carla", "member").await;

    for bad in [
        json!({"png_base64": "nao-e-base64!!"}),
        json!({"png_base64": "aGVsbG8="}), // "hello": não é PNG
    ] {
        let (st, _) = app.post("/api/whiteboards", Some(&a.token), bad).await;
        assert_eq!(st, 400);
    }
    let (st, wb) = app
        .post(
            "/api/whiteboards",
            Some(&c.token),
            json!({"title": "", "room_code": "abc-defg-hij",
                   "png_base64": format!("data:image/png;base64,{PNG_1X1}")}),
        )
        .await;
    assert_eq!(st, 200, "{wb}");
    assert_eq!(wb["title"], "Quadro sem título");
    assert_eq!(wb["room_code"], "abc-defg-hij");
    assert_eq!(wb["is_public"], false);
    assert_eq!(wb["share_token"], "", "token mascarado enquanto privado");
    let id = wb["id"].as_str().unwrap().to_string();

    // Lista: toda a org vê; a org B não.
    let (st, list) = app.get("/api/whiteboards", Some(&a.token)).await;
    assert_eq!(st, 200);
    assert_eq!(list[0]["id"], id.as_str());
    let (_, list) = app.get("/api/whiteboards", Some(&b.token)).await;
    assert_eq!(list, json!([]));

    // Metadados do quadro (rota nova): a org lê, a org B não o encontra.
    let (st, meta) = app
        .get(&format!("/api/whiteboards/{id}"), Some(&a.token))
        .await;
    assert_eq!(st, 200, "{meta}");
    assert_eq!(meta["id"], id.as_str());
    assert_eq!(meta["share_token"], "", "token mascarado enquanto privado");
    let (st, body) = app
        .get(&format!("/api/whiteboards/{id}"), Some(&b.token))
        .await;
    assert_eq!(st, 404, "{body}");
    assert_denied("metadados do quadro da A", st, &body, &id);

    // PNG
    let r = app
        .raw(
            reqwest::Method::GET,
            &format!("/api/whiteboards/{id}/image"),
            &[("Authorization", &format!("Bearer {}", a.token))],
            None,
        )
        .await;
    assert_eq!(r.status, 200);
    assert_eq!(r.header("content-type").as_deref(), Some("image/png"));
    let (st, body) = app
        .get(&format!("/api/whiteboards/{id}/image"), Some(&b.token))
        .await;
    assert_denied("PNG do quadro da A", st, &body, &id);
    let (st, _) = app
        .get(
            &format!("/api/whiteboards/{INVENTED_ID}/image"),
            Some(&a.token),
        )
        .await;
    assert_eq!(st, 404);

    // Partilha: a org B não partilha; o dono (C) sim.
    let share = format!("/api/whiteboards/{id}/public-link");
    let (st, _) = app
        .put(&share, Some(&b.token), json!({"public": true}))
        .await;
    assert_eq!(st, 403);
    let (st, pubwb) = app
        .put(&share, Some(&c.token), json!({"public": true}))
        .await;
    assert_eq!(st, 200);
    assert_eq!(pubwb["is_public"], true);
    let token = pubwb["share_token"].as_str().unwrap().to_string();
    assert_eq!(token.len(), 24);
    let r = app
        .raw(
            reqwest::Method::GET,
            &format!("/api/public/whiteboards/{token}/image"),
            &[],
            None,
        )
        .await;
    assert_eq!(r.status, 200, "leitura pública sem sessão");
    assert_eq!(r.header("content-type").as_deref(), Some("image/png"));
    // Despartilhar roda o token; o antigo deixa de servir.
    let (_, priv_wb) = app
        .put(&share, Some(&a.token), json!({"public": false}))
        .await;
    assert_eq!(priv_wb["share_token"], "");
    let (st, _) = app
        .get(&format!("/api/public/whiteboards/{token}/image"), None)
        .await;
    assert_eq!(st, 404);

    // Apagar: a org B não; outro membro sem papel também não; o admin sim.
    let d = app.add_member(&a, "dario", "member").await;
    let del = format!("/api/whiteboards/{id}");
    let (st, _) = app.delete(&del, Some(&b.token)).await;
    assert_eq!(st, 403);
    let (st, _) = app.delete(&del, Some(&d.token)).await;
    assert_eq!(st, 403);
    let (_, list) = app.get("/api/whiteboards", Some(&a.token)).await;
    assert_eq!(list.as_array().unwrap().len(), 1, "o quadro continua lá");
    let (st, body) = app.delete(&del, Some(&a.token)).await;
    assert_eq!(st, 200);
    assert_eq!(body, json!({"ok": true}));
    let (st, _) = app.delete(&del, Some(&a.token)).await;
    assert_eq!(st, 404);
    let (st, _) = app.get("/api/whiteboards", None).await;
    assert_eq!(st, 401);
}
