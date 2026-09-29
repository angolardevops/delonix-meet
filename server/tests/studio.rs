//! Estúdio de TV (ADR-0014) contra Postgres e servidor reais: estúdios,
//! emparelhamento da app Delonix Câmara, token de fonte, tally/comandos/estado
//! pelo WebSocket da sala, e isolamento entre organizações.
mod common;

use std::time::Duration;

use common::{TestApp, INVENTED_ID};
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tokio_tungstenite::tungstenite::Message;

type Ws =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

async fn ws_connect(app: &TestApp, path: &str) -> Ws {
    let url = format!("{}{path}", app.base.replacen("http://", "ws://", 1));
    let (ws, _) = tokio_tungstenite::connect_async(url)
        .await
        .expect("ligar ao /ws");
    ws
}

/// Lê até uma mensagem com `type` == `ty` (e que passe `pred`), com tecto.
async fn recv_until(ws: &mut Ws, ty: &str, pred: impl Fn(&Value) -> bool) -> Value {
    tokio::time::timeout(Duration::from_secs(10), async {
        while let Some(m) = ws.next().await {
            let Ok(m) = m else { break };
            let Ok(text) = m.to_text() else { continue };
            let Ok(v) = serde_json::from_str::<Value>(text) else {
                continue;
            };
            if v["type"] == ty && pred(&v) {
                return v;
            }
        }
        panic!("o socket fechou antes de «{ty}»");
    })
    .await
    .unwrap_or_else(|_| panic!("sem «{ty}» em 10 s"))
}

async fn send(ws: &mut Ws, v: Value) {
    ws.send(Message::Text(v.to_string())).await.unwrap();
}

async fn create_studio(app: &TestApp, a: &common::Account) -> Value {
    let (st, s) = app
        .post(
            &format!("/api/orgs/{}/studios", a.org()),
            Some(&a.token),
            json!({"name": "Régie principal"}),
        )
        .await;
    assert_eq!(st, 201, "{s}");
    s
}

async fn new_code(app: &TestApp, a: &common::Account, studio_id: &str, body: Value) -> Value {
    let (st, c) = app
        .post(
            &format!("/api/orgs/{}/studios/{studio_id}/pairing-codes", a.org()),
            Some(&a.token),
            body,
        )
        .await;
    assert_eq!(st, 201, "{c}");
    c
}

#[sqlx::test(migrations = "./migrations")]
async fn studio_crud_and_who_can(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let a = app.new_org("alfa.ao").await;
    let colega = app.add_member(&a, "rui", "member").await;
    let b = app.new_org("beta.ao").await;

    let s = create_studio(&app, &a).await;
    let id = s["id"].as_str().unwrap();
    assert_eq!(s["iso_recording"], true);
    assert!(s["room_code"].as_str().unwrap().len() >= 6);
    let base = format!("/api/orgs/{}/studios", a.org());

    // Membro vê; não cria, não apaga, não opera (não o criou).
    let (st, page) = app.get(&base, Some(&colega.token)).await;
    assert_eq!(st, 200);
    assert_eq!(page["items"].as_array().unwrap().len(), 1);
    let (st, e) = app
        .post(&base, Some(&colega.token), json!({"name": "outro"}))
        .await;
    assert_eq!((st, e["code"].as_str()), (403, Some("studio.not_manager")));
    let (st, e) = app
        .patch(
            &format!("{base}/{id}"),
            Some(&colega.token),
            json!({"name": "x"}),
        )
        .await;
    assert_eq!((st, e["code"].as_str()), (403, Some("studio.not_operator")));
    let (st, _) = app
        .delete(&format!("{base}/{id}"), Some(&colega.token))
        .await;
    assert_eq!(st, 403);

    // Outra organização: 404 em tudo, nem pelo caminho da sua própria org.
    let (st, _) = app.get(&format!("{base}/{id}"), Some(&b.token)).await;
    assert_eq!(st, 404);
    let (st, _) = app
        .get(
            &format!("/api/orgs/{}/studios/{id}", b.org()),
            Some(&b.token),
        )
        .await;
    assert_eq!(st, 404);

    // Forma: nome vazio e campo desconhecido.
    let (st, e) = app.post(&base, Some(&a.token), json!({"name": " "})).await;
    assert_eq!((st, e["code"].as_str()), (400, Some("studio.invalid_name")));
    let (st, _) = app
        .post(&base, Some(&a.token), json!({"name": "x", "e2ee": true}))
        .await;
    assert_eq!(st, 422, "um campo desconhecido não é engolido");

    let (st, upd) = app
        .patch(
            &format!("{base}/{id}"),
            Some(&a.token),
            json!({"iso_recording": false}),
        )
        .await;
    assert_eq!(st, 200, "{upd}");
    assert_eq!(upd["iso_recording"], false);

    // Destino das gravações: espaço livre real e MinIO honesto.
    let (st, t) = app
        .get(
            &format!("{base}/{id}/recording-target"),
            Some(&colega.token),
        )
        .await;
    assert_eq!(st, 200, "{t}");
    assert_eq!(t["kind"], "local");
    assert_eq!(t["object_storage"]["state"], "not_configured");
    assert!(t["total_bytes"].as_u64().unwrap() > 0, "{t}");

    let (st, _) = app.delete(&format!("{base}/{id}"), Some(&a.token)).await;
    assert_eq!(st, 204);
    let (st, _) = app.delete(&format!("{base}/{id}"), Some(&a.token)).await;
    assert_eq!(st, 404);
}

#[sqlx::test(migrations = "./migrations")]
async fn pairing_code_burns_after_five_wrong_attempts(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let a = app.new_org("alfa.ao").await;
    let s = create_studio(&app, &a).await;
    let sid = s["id"].as_str().unwrap();
    let c = new_code(
        &app,
        &a,
        sid,
        json!({"label": "telefone da Ana", "number": 2}),
    )
    .await;
    let code = c["code"].as_str().unwrap().to_string();
    assert_eq!(code.len(), 9);
    assert_eq!(c["max_attempts"], 5);

    // Um segredo errado com o localizador certo: 5 vezes.
    let wrong_secret = if &code[5..] == "0000" { "1111" } else { "0000" };
    let wrong = format!("{}-{wrong_secret}", &code[..4]);
    for _ in 0..5 {
        let (st, e) = app
            .post("/api/studio-pairings", None, json!({"code": wrong}))
            .await;
        assert_eq!(
            (st, e["code"].as_str()),
            (404, Some("studio.pairing_invalid")),
            "{e}"
        );
    }
    // Agora nem o código certo entra — e a resposta é a mesma.
    let (st, e) = app
        .post("/api/studio-pairings", None, json!({"code": code}))
        .await;
    assert_eq!(
        (st, e["code"].as_str()),
        (404, Some("studio.pairing_invalid"))
    );
    let (_, list) = app
        .get(
            &format!("/api/orgs/{}/studios/{sid}/pairing-codes", a.org()),
            Some(&a.token),
        )
        .await;
    assert_eq!(list["items"][0]["state"], "burned");
    assert_eq!(list["items"][0]["attempts"], 5);
    assert!(
        !list.to_string().contains(&code),
        "o código em claro não volta a sair"
    );

    // Forma errada é 400, não conta como tentativa.
    let (st, e) = app
        .post("/api/studio-pairings", None, json!({"code": "abc"}))
        .await;
    assert_eq!(
        (st, e["code"].as_str()),
        (400, Some("studio.pairing_malformed"))
    );

    // Um código revogado também deixa de servir.
    let c2 = new_code(&app, &a, sid, json!({})).await;
    let (st, _) = app
        .delete(
            &format!(
                "/api/orgs/{}/studios/{sid}/pairing-codes/{}",
                a.org(),
                c2["id"].as_str().unwrap()
            ),
            Some(&a.token),
        )
        .await;
    assert_eq!(st, 204);
    let (st, _) = app
        .post("/api/studio-pairings", None, json!({"code": c2["code"]}))
        .await;
    assert_eq!(st, 404);
}

#[sqlx::test(migrations = "./migrations")]
async fn source_token_is_minimal_and_single_use(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let a = app.new_org("alfa.ao").await;
    let s = create_studio(&app, &a).await;
    let sid = s["id"].as_str().unwrap();
    let c = new_code(&app, &a, sid, json!({"label": "telefone da Ana"})).await;

    let (st, p) = app
        .post(
            "/api/studio-pairings",
            None,
            json!({"code": c["code"].as_str().unwrap().to_lowercase().replace('-', ""),
                   "device": {"model": "Galaxy S23", "platform": "android", "app_version": "1.0.0"}}),
        )
        .await;
    assert_eq!(st, 201, "{p}");
    assert_eq!(p["number"], 1, "o menor livre");
    assert_eq!(p["label"], "CAM 1 · telefone da Ana");
    let token = p["source_token"].as_str().unwrap();
    let claims = common::jwt_claims(token);
    assert_eq!(claims["typ"], "source");
    assert!(claims.get("owner").is_none());
    assert!(p["ws_path"].as_str().unwrap().contains("&room="));

    // Uso único.
    let (st, _) = app
        .post("/api/studio-pairings", None, json!({"code": c["code"]}))
        .await;
    assert_eq!(st, 404);

    // O token de fonte não abre a REST nem o directo.
    let (st, _) = app.get("/api/users/me", Some(token)).await;
    assert_eq!(st, 401);
    let (st, _) = app
        .get(&format!("/api/orgs/{}/studios", a.org()), Some(token))
        .await;
    assert_eq!(st, 401);
    let live = format!(
        "{}/api/rooms/{}/live?token={token}&codec=video/h264",
        app.base.replacen("http://", "ws://", 1),
        s["room_code"].as_str().unwrap()
    );
    assert!(
        tokio_tungstenite::connect_async(live).await.is_err(),
        "o /live não pode aceitar um token de fonte"
    );

    // Número pedido e já ocupado.
    let (st, e) = app
        .post(
            &format!("/api/orgs/{}/studios/{sid}/pairing-codes", a.org()),
            Some(&a.token),
            json!({"number": 1}),
        )
        .await;
    assert_eq!(
        (st, e["code"].as_str()),
        (409, Some("studio.source_number_taken"))
    );

    // Fontes: listagem, patch, e revogação fecha o /ws.
    let base = format!("/api/orgs/{}/studios/{sid}/sources", a.org());
    let (st, list) = app.get(&base, Some(&a.token)).await;
    assert_eq!(st, 200);
    let src = &list["items"][0];
    assert_eq!(src["device"]["model"], "Galaxy S23");
    assert_eq!(src["connected"], false);
    let src_id = src["id"].as_str().unwrap().to_string();

    let (st, e) = app
        .patch(
            &format!("{base}/{src_id}"),
            Some(&a.token),
            json!({"number": 17}),
        )
        .await;
    assert_eq!(
        (st, e["code"].as_str()),
        (400, Some("studio.invalid_source_number"))
    );

    let (st, _) = app
        .delete(&format!("{base}/{src_id}"), Some(&a.token))
        .await;
    assert_eq!(st, 204);
    // Revogada: o token deixa de abrir o /ws.
    let r = tokio_tungstenite::connect_async(format!(
        "{}{}",
        app.base.replacen("http://", "ws://", 1),
        p["ws_path"].as_str().unwrap()
    ))
    .await;
    assert!(r.is_err(), "fonte revogada não entra");
    let (st, _) = app
        .get(&format!("{base}/{INVENTED_ID}"), Some(&a.token))
        .await;
    assert_eq!(st, 404);
}

#[sqlx::test(migrations = "./migrations")]
async fn tally_commands_and_status_over_the_room_socket(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let a = app.new_org("alfa.ao").await;
    let colega = app.add_member(&a, "rui", "member").await;
    let s = create_studio(&app, &a).await;
    let sid = s["id"].as_str().unwrap();
    let room = s["room_code"].as_str().unwrap();

    // O operador (dono da sala) entra.
    let (st, j) = app
        .post(
            &format!("/api/rooms/{room}/join"),
            Some(&a.token),
            json!({}),
        )
        .await;
    assert_eq!(st, 200, "{j}");
    let mut host = ws_connect(&app, j["ws_path"].as_str().unwrap()).await;
    recv_until(&mut host, "joined", |_| true).await;
    let first = recv_until(&mut host, "studio-sources", |_| true).await;
    assert_eq!(first["sources"], json!([]));

    // O telefone emparelha e entra SEM sala de espera.
    let c = new_code(
        &app,
        &a,
        sid,
        json!({"label": "telefone da Ana", "number": 2}),
    )
    .await;
    let (_, p) = app
        .post("/api/studio-pairings", None, json!({"code": c["code"]}))
        .await;
    let source_id = p["source_id"].as_str().unwrap().to_string();
    let mut phone = ws_connect(&app, p["ws_path"].as_str().unwrap()).await;
    let joined = recv_until(&mut phone, "joined", |_| true).await;
    let phone_peer = joined["peer_id"].as_str().unwrap().to_string();
    let t = recv_until(&mut phone, "studio-tally", |_| true).await;
    assert_eq!(t["state"], "free");
    let list = recv_until(&mut host, "studio-sources", |v| {
        v["sources"].as_array().is_some_and(|a| !a.is_empty())
    })
    .await;
    assert_eq!(list["sources"][0]["source_id"], source_id.as_str());
    assert_eq!(list["sources"][0]["peer_id"], phone_peer.as_str());
    assert_eq!(list["sources"][0]["number"], 2);
    assert_eq!(list["sources"][0]["connected"], true);

    // Tally: PRÉ e depois PROGRAMA.
    send(
        &mut host,
        json!({"type": "studio-tally", "program": [], "preview": [source_id]}),
    )
    .await;
    let t = recv_until(&mut phone, "studio-tally", |_| true).await;
    assert_eq!(t["state"], "preview");
    send(
        &mut host,
        json!({"type": "studio-tally", "program": [source_id], "preview": [source_id]}),
    )
    .await;
    let t = recv_until(&mut phone, "studio-tally", |_| true).await;
    assert_eq!(t["state"], "program", "PROGRAMA ganha a PRÉ");

    // Comando do operador chega ao telefone; um fora de intervalo não sai.
    send(
        &mut host,
        json!({"type": "studio-command", "source_id": source_id, "command_id": "c-1",
               "command": {"kind": "exposure", "ev": 9}}),
    )
    .await;
    let e = recv_until(&mut host, "error", |_| true).await;
    assert!(
        e["message"]
            .as_str()
            .unwrap()
            .starts_with("studio.invalid_command"),
        "{e}"
    );
    send(
        &mut host,
        json!({"type": "studio-command", "source_id": source_id, "command_id": "c-2",
               "command": {"kind": "lock-exposure-focus", "locked": true}}),
    )
    .await;
    let cmd = recv_until(&mut phone, "studio-command", |_| true).await;
    assert_eq!(cmd["command_id"], "c-2");
    assert_eq!(
        cmd["command"],
        json!({"kind": "lock-exposure-focus", "locked": true})
    );

    // O telefone responde e manda estado; o operador recebe os dois.
    send(
        &mut phone,
        json!({"type": "studio-command-result", "command_id": "c-2", "ok": true}),
    )
    .await;
    let r = recv_until(&mut host, "studio-command-result", |_| true).await;
    assert_eq!(r["source_id"], source_id.as_str());
    assert_eq!(r["ok"], true);
    send(
        &mut phone,
        json!({"type": "studio-source-status", "status": {"battery_percent": 76, "charging": true,
               "temperature_c": 41, "network": {"kind": "usb", "link_mbps": 5000}}}),
    )
    .await;
    let st = recv_until(&mut host, "studio-source-status", |_| true).await;
    assert_eq!(st["source_id"], source_id.as_str());
    assert_eq!(st["status"]["temperature_c"], 41.0);

    // Uma fonte não conversa nem grava.
    send(&mut phone, json!({"type": "chat", "text": "olá"})).await;
    let e = recv_until(&mut phone, "error", |_| true).await;
    assert_eq!(e["message"], "source.forbidden_message");
    send(&mut phone, json!({"type": "server-record", "active": true})).await;
    let e = recv_until(&mut phone, "error", |_| true).await;
    assert_eq!(e["message"], "source.forbidden_message");
    // Nem se faz passar por operador.
    send(
        &mut phone,
        json!({"type": "studio-tally", "program": [], "preview": []}),
    )
    .await;
    let e = recv_until(&mut phone, "error", |_| true).await;
    assert_eq!(e["message"], "source.forbidden_message");

    // Um participante que não é anfitrião não comanda.
    let (_, jc) = app
        .post(
            &format!("/api/rooms/{room}/join"),
            Some(&colega.token),
            json!({}),
        )
        .await;
    let mut guest = ws_connect(&app, jc["ws_path"].as_str().unwrap()).await;
    let w = recv_until(&mut host, "waiting-join", |_| true).await;
    send(
        &mut host,
        json!({"type": "admit", "to": w["peer"]["peer_id"]}),
    )
    .await;
    recv_until(&mut guest, "joined", |_| true).await;
    send(
        &mut guest,
        json!({"type": "studio-tally", "program": [], "preview": []}),
    )
    .await;
    let e = recv_until(&mut guest, "error", |_| true).await;
    assert_eq!(e["message"], "studio.not_operator");
    send(
        &mut guest,
        json!({"type": "studio-command", "source_id": source_id, "command_id": "x",
               "command": {"kind": "mirror", "on": true}}),
    )
    .await;
    let e = recv_until(&mut guest, "error", |_| true).await;
    assert_eq!(e["message"], "studio.not_operator");
    // E também não finge ser uma fonte.
    send(
        &mut guest,
        json!({"type": "studio-source-status", "status": {}}),
    )
    .await;
    let e = recv_until(&mut guest, "error", |_| true).await;
    assert_eq!(e["message"], "studio.not_a_source");

    // A REST do mesmo pod vê a fonte ligada, em PROGRAMA, com o estado.
    let (_, one) = app
        .get(
            &format!("/api/orgs/{}/studios/{sid}/sources/{source_id}", a.org()),
            Some(&colega.token),
        )
        .await;
    assert_eq!(one["connected"], true, "{one}");
    assert_eq!(one["tally"], "program");
    assert_eq!(one["status"]["battery_percent"], 76.0);

    // Revogar expulsa o telefone: o socket fecha.
    let (code, _) = app
        .delete(
            &format!("/api/orgs/{}/studios/{sid}/sources/{source_id}", a.org()),
            Some(&a.token),
        )
        .await;
    assert_eq!(code, 204);
    let closed = tokio::time::timeout(Duration::from_secs(10), async {
        while let Some(m) = phone.next().await {
            if m.is_err() || matches!(m, Ok(Message::Close(_))) {
                return true;
            }
        }
        true
    })
    .await
    .expect("o socket da fonte revogada tinha de fechar");
    assert!(closed);
    // O comando a uma fonte revogada é recusado com razão.
    send(
        &mut host,
        json!({"type": "studio-command", "source_id": source_id, "command_id": "c-3",
               "command": {"kind": "focus-face"}}),
    )
    .await;
    let e = recv_until(&mut host, "error", |v| {
        v["message"]
            .as_str()
            .is_some_and(|m| m.starts_with("studio.unknown_source"))
    })
    .await;
    assert_eq!(e["message"], "studio.unknown_source");
}
