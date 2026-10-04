//! Quem pode pôr uma sala no ar, e para onde (RFC-0001 F0.5, B1 e B2), contra
//! Postgres real e por um WebSocket a sério: a recusa é uma trama de texto
//! entregue DEPOIS do upgrade (ver `broadcast::ws_directo`), por isso é isso
//! que se lê aqui, não um código HTTP.
mod common;

use std::time::Duration;

use common::TestApp;
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tokio_tungstenite::tungstenite::Message;

const CODEC: &str = "video/webm;codecs=h264,opus";

type Ws =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

/// Abre o `/live` da sala. `destinos_no_url` é a forma ANTIGA (os destinos na
/// query); sem ele, o URL leva só o token e o codec, como a consola faz agora.
async fn abrir_live(app: &TestApp, code: &str, token: &str, destinos_no_url: Option<&Value>) -> Ws {
    let base = app.base.replacen("http://", "ws://", 1);
    let mut params = vec![("token", token.to_string()), ("codec", CODEC.to_string())];
    if let Some(d) = destinos_no_url {
        params.push(("destinos", d.to_string()));
    }
    let url =
        url::Url::parse_with_params(&format!("{base}/api/rooms/{code}/live"), &params).unwrap();
    let (ws, _) = tokio_tungstenite::connect_async(url.as_str())
        .await
        .expect("o upgrade tem de ser aceite para a recusa chegar");
    ws
}

/// A primeira trama de texto que o servidor manda: a razão da recusa, ou
/// `(aceite)` se for o estado dos destinos de uma emissão que arrancou (numa
/// máquina com `ffmpeg`, um destino público passa todas as guardas).
async fn primeira_resposta(ws: &mut Ws) -> String {
    let msg = tokio::time::timeout(Duration::from_secs(15), ws.next())
        .await
        .expect("sem resposta do /live")
        .expect("o socket fechou sem razão")
        .expect("erro no socket");
    let v: Value = serde_json::from_str(msg.to_text().unwrap()).unwrap();
    match v["erro"].as_str() {
        Some(e) => e.to_string(),
        None if v["tipo"] == "destinos" => "(aceite)".into(),
        None => panic!("trama inesperada: {v}"),
    }
}

/// A recusa, quando o que se espera é mesmo uma recusa.
async fn ler_recusa(ws: &mut Ws) -> String {
    let r = primeira_resposta(ws).await;
    assert_ne!(r, "(aceite)", "esperava-se uma recusa e a emissão arrancou");
    r
}

/// O caminho da consola: liga, manda os destinos na PRIMEIRA TRAMA
/// (`{"tipo":"iniciar",…}`) e devolve a razão da recusa.
async fn recusa_do_directo(app: &TestApp, code: &str, token: &str, destinos: Value) -> String {
    let mut ws = abrir_live(app, code, token, None).await;
    // Sem `unwrap`: quem não é anfitrião é recusado logo a seguir ao upgrade,
    // e o servidor pode já ter fechado quando este envio sai.
    let _ = ws
        .send(Message::Text(
            json!({"tipo": "iniciar", "destinos": destinos}).to_string(),
        ))
        .await;
    primeira_resposta(&mut ws).await
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

/// R287 — a chave de emissão nunca vai no URL. Um destino com URL e chave na
/// query é recusado com a frase que manda actualizar a aplicação; o mesmo
/// pedido pela primeira trama passa esta guarda. E no URL continua a caber o
/// que não é segredo: um destino guardado, só pelo id.
#[sqlx::test(migrations = "./migrations")]
async fn a_chave_de_emissao_nao_se_aceita_no_url(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let host = app.new_org("alfa.ao").await;
    let room = app.new_room(&host, "Estúdio").await;
    let code = room["code"].as_str().unwrap();
    let owner = room_token(&app, code, &host).await;
    let ad_hoc = json!([{"url": "rtmp://8.8.8.8/live", "chave": "k-123", "rotulo": "x"}]);

    // No URL: recusado, e a recusa não repete a chave.
    let mut ws = abrir_live(&app, code, &owner, Some(&ad_hoc)).await;
    let r = ler_recusa(&mut ws).await;
    assert!(
        r.contains("já não se aceitam no endereço da ligação"),
        "{r}"
    );
    assert!(!r.contains("k-123"), "a recusa repete a chave: {r}");

    // Controlo positivo: o MESMO destino, pela primeira trama, não leva essa
    // recusa (leva outra, ou nenhuma — não é o que se mede).
    let r = recusa_do_directo(&app, code, &owner, ad_hoc).await;
    assert!(!r.contains("endereço da ligação"), "{r}");

    // Um destino guardado, só pelo id, continua a caber no URL: a recusa é a
    // de o id não existir, não a da chave.
    let guardado = json!([{"id": uuid::Uuid::new_v4()}]);
    let mut ws = abrir_live(&app, code, &owner, Some(&guardado)).await;
    let r = ler_recusa(&mut ws).await;
    assert!(!r.contains("endereço da ligação"), "{r}");
}

/// A primeira trama tem de ser o pedido de início. Outra coisa — um pedaço de
/// media, um JSON qualquer — é recusada com a forma que se espera, em vez de
/// ficar à espera ou de arrancar um processo.
#[sqlx::test(migrations = "./migrations")]
async fn a_primeira_trama_tem_de_ser_o_pedido_de_inicio(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let host = app.new_org("alfa.ao").await;
    let room = app.new_room(&host, "Estúdio").await;
    let code = room["code"].as_str().unwrap();
    let owner = room_token(&app, code, &host).await;

    let mut ws = abrir_live(&app, code, &owner, None).await;
    ws.send(Message::Binary(vec![0u8; 16])).await.unwrap();
    let r = ler_recusa(&mut ws).await;
    assert!(r.contains("pedido de início"), "{r}");

    let mut ws = abrir_live(&app, code, &owner, None).await;
    ws.send(Message::Text(json!({"tipo": "outra-coisa"}).to_string()))
        .await
        .unwrap();
    let r = ler_recusa(&mut ws).await;
    assert!(r.contains("iniciar"), "{r}");

    // E o que vem na trama é validado como vinha no URL: malformado é dito.
    let mut ws = abrir_live(&app, code, &owner, None).await;
    ws.send(Message::Text(
        json!({"tipo": "iniciar", "destinos": "nao-e-uma-lista"}).to_string(),
    ))
    .await
    .unwrap();
    let r = ler_recusa(&mut ws).await;
    assert!(r.contains("malformados"), "{r}");
}
