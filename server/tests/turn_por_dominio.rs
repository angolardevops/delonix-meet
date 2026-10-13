//! O TURN por domínio (`TURN_HOST_BY_HOST`): quem entra por um site recebe o
//! nome do TURN desse site; os outros recebem o `TURN_HOST`.
//!
//! Mede as DUAS portas por onde a configuração ICE sai para um browser — a
//! rota com sessão e a entrada de convidado, que não tem sessão para chamar a
//! primeira — porque uma delas a escolher o nome e a outra não daria uma sala
//! em que o anfitrião e o convidado recebem relays com nomes diferentes sem
//! ninguém o ter decidido.
mod common;

use common::TestApp;
use serde_json::{json, Value};

const CANONICO: &str = "turn.exemplo.com:3478";
const DO_AO: &str = "turn.exemplo.ao:3478";

fn urls(cfg: &Value) -> Vec<String> {
    cfg["iceServers"]
        .as_array()
        .expect("iceServers")
        .iter()
        .flat_map(|s| s["urls"].as_array().cloned().unwrap_or_default())
        .map(|u| u.as_str().unwrap().to_string())
        .collect()
}

async fn ice_com_host(app: &TestApp, token: &str, host: Option<&str>) -> Value {
    let mut req = app.http.get(app.url("/api/ice-servers")).bearer_auth(token);
    if let Some(h) = host {
        req = req.header("host", h);
    }
    let r = req.send().await.unwrap();
    assert_eq!(r.status(), 200);
    r.json().await.unwrap()
}

#[sqlx::test(migrations = "./migrations")]
async fn each_site_gets_its_own_turn_name(db: sqlx::PgPool) {
    let app = TestApp::spawn_with(
        db,
        &[
            ("TURN_HOST", CANONICO),
            ("TURN_HOST_BY_HOST", "meet.exemplo.ao=turn.exemplo.ao:3478"),
        ],
    )
    .await;
    let dono = app.new_org("exemplo.ao").await;

    // Quem entra pelo `.ao` recebe o TURN do `.ao` — e SÓ esse: dois nomes para
    // o mesmo relay são duas alocações por browser.
    let ao = urls(&ice_com_host(&app, &dono.token, Some("meet.exemplo.ao")).await);
    assert_eq!(
        ao,
        [format!("stun:{DO_AO}"), format!("turn:{DO_AO}")],
        "{ao:?}"
    );
    // Como o proxy o entrega: com porta e maiúsculas.
    let ao = urls(&ice_com_host(&app, &dono.token, Some("Meet.Exemplo.AO:443")).await);
    assert_eq!(
        ao,
        [format!("stun:{DO_AO}"), format!("turn:{DO_AO}")],
        "{ao:?}"
    );

    // O outro site, e o pedido sem nome de site, recebem o `TURN_HOST`.
    for host in [Some("meet.exemplo.com"), None] {
        let u = urls(&ice_com_host(&app, &dono.token, host).await);
        assert_eq!(
            u,
            [format!("stun:{CANONICO}"), format!("turn:{CANONICO}")],
            "{host:?}: {u:?}"
        );
    }

    // O cabeçalho é do cliente: um nome que a instalação não configurou não
    // escolhe nada, e muito menos aparece na resposta.
    for host in [
        "atacante.example",
        "meet.exemplo.ao.atacante.example",
        "x.meet.exemplo.ao",
    ] {
        let cfg = ice_com_host(&app, &dono.token, Some(host)).await;
        let u = urls(&cfg);
        assert_eq!(
            u,
            [format!("stun:{CANONICO}"), format!("turn:{CANONICO}")],
            "{host}: {u:?}"
        );
        assert!(!cfg.to_string().contains("atacante"), "{host}: {cfg}");
    }
}

#[sqlx::test(migrations = "./migrations")]
async fn a_guest_gets_the_turn_of_the_site_they_came_through(db: sqlx::PgPool) {
    let app = TestApp::spawn_with(
        db,
        &[
            ("TURN_HOST", CANONICO),
            ("TURN_HOST_BY_HOST", "meet.exemplo.ao=turn.exemplo.ao:3478"),
        ],
    )
    .await;
    let dono = app.new_org("exemplo.ao").await;
    let sala = app.new_room(&dono, "Reunião com externos").await;
    let code = sala["code"].as_str().unwrap();

    for (host, esperado) in [("meet.exemplo.ao", DO_AO), ("meet.exemplo.com", CANONICO)] {
        let r = app
            .http
            .post(app.url(&format!("/api/rooms/{code}/guest-join")))
            .header("host", host)
            .json(&json!({"display_name": "Maria Externa"}))
            .send()
            .await
            .unwrap();
        assert_eq!(r.status(), 200, "{host}");
        let body: Value = r.json().await.unwrap();
        let u = urls(&body["ice_servers"]);
        assert_eq!(
            u,
            [format!("stun:{esperado}"), format!("turn:{esperado}")],
            "convidado por {host}: {u:?}"
        );
    }
}

#[sqlx::test(migrations = "./migrations")]
async fn without_the_list_nothing_changes(db: sqlx::PgPool) {
    let app = TestApp::spawn_with(db, &[("TURN_HOST", CANONICO)]).await;
    let dono = app.new_org("exemplo.ao").await;
    let u = urls(&ice_com_host(&app, &dono.token, Some("meet.exemplo.ao")).await);
    assert_eq!(
        u,
        [format!("stun:{CANONICO}"), format!("turn:{CANONICO}")],
        "{u:?}"
    );
}
