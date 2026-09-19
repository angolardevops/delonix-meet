//! Histórico do chat da sala contra Postgres real: as conversas directas só
//! voltam ao par (o filtro é na consulta), e fios e reacções vêm na resposta.
//! A entrega em tempo real está nos testes do hub (`signaling.rs`).
mod common;

use common::TestApp;
use serde_json::{json, Value};

async fn mensagem(
    app: &TestApp,
    room_id: &str,
    de: &str,
    nome: &str,
    texto: &str,
    para: Option<(&str, &str)>,
) -> String {
    let id: uuid::Uuid = sqlx::query_scalar(
        "INSERT INTO room_chat_messages (room_id, user_id, username, message, to_user_id, to_username)
         VALUES ($1::uuid, $2::uuid, $3, $4, $5::uuid, $6) RETURNING id",
    )
    .bind(room_id)
    .bind(de)
    .bind(nome)
    .bind(texto)
    .bind(para.map(|p| p.0))
    .bind(para.map(|p| p.1))
    .fetch_one(&app.db)
    .await
    .unwrap();
    id.to_string()
}

fn textos(v: &Value) -> Vec<String> {
    v.as_array()
        .unwrap()
        .iter()
        .map(|m| m["message"].as_str().unwrap().to_string())
        .collect()
}

#[sqlx::test(migrations = "./migrations")]
async fn a_conversa_directa_so_volta_ao_par(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let ana = app.new_org("alfa.test").await;
    let bia = app.add_member(&ana, "bia", "member").await;
    let caio = app.add_member(&ana, "caio", "member").await;
    let room = app.new_room(&ana, "equipa").await;
    let (room_id, code) = (room["id"].as_str().unwrap(), room["code"].as_str().unwrap());

    let publica = mensagem(&app, room_id, &ana.user_id, "ana", "bom dia a todos", None).await;
    mensagem(
        &app,
        room_id,
        &ana.user_id,
        "ana",
        "só para a bia",
        Some((&bia.user_id, "bia")),
    )
    .await;
    // Resposta em fio à pública, e duas reacções iguais de contas diferentes.
    sqlx::query(
        "INSERT INTO room_chat_messages (room_id, user_id, username, message, parent_id)
         VALUES ($1::uuid, $2::uuid, 'caio', 'bom dia', $3::uuid)",
    )
    .bind(room_id)
    .bind(&caio.user_id)
    .bind(&publica)
    .execute(&app.db)
    .await
    .unwrap();
    for u in [&bia.user_id, &caio.user_id] {
        sqlx::query(
            "INSERT INTO room_chat_reactions (message_id, user_id, emoji) VALUES ($1::uuid, $2::uuid, '👍')",
        )
        .bind(&publica)
        .bind(u)
        .execute(&app.db)
        .await
        .unwrap();
    }

    let path = format!("/api/rooms/{code}/messages");
    // Quem enviou e quem recebeu vêem a privada.
    for (quem, tok) in [("ana", &ana.token), ("bia", &bia.token)] {
        let (st, body) = app.get(&path, Some(tok)).await;
        assert_eq!(st, 200, "{quem}: {body}");
        assert_eq!(
            textos(&body),
            ["bom dia a todos", "só para a bia", "bom dia"],
            "{quem}"
        );
        let privada = &body[1];
        assert_eq!(privada["to_user_id"], bia.user_id.as_str());
        assert_eq!(privada["to_username"], "bia");
    }
    // Um terceiro na mesma sala e organização não a vê — nem o anfitrião a leria
    // se não fosse o autor.
    let (st, body) = app.get(&path, Some(&caio.token)).await;
    assert_eq!(st, 200);
    assert_eq!(textos(&body), ["bom dia a todos", "bom dia"]);
    assert!(
        !body.to_string().contains("só para a bia"),
        "a privada vazou: {body}"
    );

    // Fio e reacções vêm na resposta.
    assert_eq!(body[0]["reactions"], json!({"👍": 2}));
    assert!(body[0]["to_user_id"].is_null());
    assert_eq!(body[1]["parent_id"], publica.as_str());
    assert_eq!(body[1]["reactions"], json!({}));
}
