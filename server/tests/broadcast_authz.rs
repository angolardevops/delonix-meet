//! Quem pode pôr uma sala no ar, e para onde (RFC-0001 F0.5, B1 e B2), contra
//! Postgres real e por um WebSocket a sério: a recusa é uma trama de texto
//! entregue DEPOIS do upgrade (ver `broadcast::ws_directo`), por isso é isso
//! que se lê aqui, não um código HTTP.
mod common;

use std::time::Duration;

use common::TestApp;
use futures_util::StreamExt;
use serde_json::{json, Value};

/// Liga ao `/live` da sala com `token` e devolve a razão da recusa (`erro`).
async fn recusa_do_directo(app: &TestApp, code: &str, token: &str, destinos: Value) -> String {
    let base = app.base.replacen("http://", "ws://", 1);
    let url = url::Url::parse_with_params(
        &format!("{base}/api/rooms/{code}/live"),
        &[
            ("token", token),
            ("destinos", &destinos.to_string()),
            ("codec", "video/webm;codecs=h264,opus"),
        ],
    )
    .unwrap();
    let (mut ws, _) = tokio_tungstenite::connect_async(url.as_str())
        .await
        .expect("o upgrade tem de ser aceite para a recusa chegar");
    let msg = tokio::time::timeout(Duration::from_secs(5), ws.next())
        .await
        .expect("sem resposta do /live")
        .expect("o socket fechou sem razão")
        .expect("erro no socket");
    let v: Value = serde_json::from_str(msg.to_text().unwrap()).unwrap();
    v["erro"]
        .as_str()
        .unwrap_or_else(|| panic!("a trama não é uma recusa: {v}"))
        .to_string()
}

async fn room_token(app: &TestApp, code: &str, who: &common::Account) -> String {
    let (st, join) = app
        .post(
            &format!("/api/rooms/{code}/join"),
            Some(&who.token),
            json!({}),
        )
        .await;
    assert_eq!(st, 200, "{join}");
    join["room_token"].as_str().unwrap().to_string()
}

#[sqlx::test(migrations = "./migrations")]
async fn so_o_anfitriao_poe_a_sala_no_ar(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let host = app.new_org("alfa.ao").await;
    let outsider = app.new_org("beta.ao").await;
    let room = app.new_room(&host, "Estúdio").await;
    let code = room["code"].as_str().unwrap();
    let destinos = json!([{"url": "rtmp://8.8.8.8/live", "chave": "k-123", "rotulo": "x"}]);

    // Quem entra com o código mas não é o anfitrião nem admissor: recusado.
    let guest = room_token(&app, code, &outsider).await;
    let r = recusa_do_directo(&app, code, &guest, destinos.clone()).await;
    assert!(r.contains("só o anfitrião"), "{r}");

    // O anfitrião passa a guarda de autoridade: a recusa, se houver, é outra.
    let owner = room_token(&app, code, &host).await;
    let r = recusa_do_directo(&app, code, &owner, destinos).await;
    assert!(!r.contains("só o anfitrião"), "{r}");
}

#[sqlx::test(migrations = "./migrations")]
async fn destinos_internos_sao_recusados_antes_de_arrancar_o_ffmpeg(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let host = app.new_org("alfa.ao").await;
    let room = app.new_room(&host, "Estúdio").await;
    let code = room["code"].as_str().unwrap();
    let owner = room_token(&app, code, &host).await;

    for url in [
        "rtmp://127.0.0.1/live",
        "rtmp://10.0.0.5:1935/live",
        "rtmps://169.254.169.254/latest",
    ] {
        let r = recusa_do_directo(
            &app,
            code,
            &owner,
            json!([{"url": url, "chave": "k-123", "rotulo": "interno"}]),
        )
        .await;
        assert!(r.contains("não é alcançável"), "{url}: {r}");
    }
}

#[sqlx::test(migrations = "./migrations")]
async fn destino_guardado_nao_aponta_a_rede_interna(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let a = app.new_org("alfa.ao").await;
    for url in ["rtmp://10.0.0.5/live", "rtmp://127.0.0.1:1935/live"] {
        let (st, v) = app
            .post(
                &format!("/api/orgs/{}/stream-destinations", a.org()),
                Some(&a.token),
                json!({"kind": "rtmp", "label": "interno", "url": url}),
            )
            .await;
        assert_eq!(st, 400, "{url}: {v}");
    }
}
