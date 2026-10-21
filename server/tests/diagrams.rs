//! Diagramas no servidor (ADR-0020), contra Postgres real.
//!
//! O que isto prova, e porque cada peça está aqui:
//!  - um diagrama é da PESSOA: ninguém o vê, nem um colega da mesma
//!    organização, nem alguém de outra. O `id` é escolhido pelo cliente, pelo
//!    que o caso que importa é **duas pessoas com o MESMO id** — é o que a
//!    chave `(owner_id, id)` existe para separar;
//!  - o documento sobrevive à volta: o que sai do `GET` é o que entrou no `PUT`;
//!  - gravar duas vezes a partir do mesmo estado dá `409` e não escolhe um
//!    vencedor em silêncio;
//!  - apagar o de outra pessoa dá `404` e **não apaga nada**.
mod common;

use common::{assert_denied, Account, TestApp};
use serde_json::{json, Value};

fn doc(titulo: &str, nos: usize) -> Value {
    json!({
        "v": 1,
        "id": "d1",
        "title": titulo,
        "notation": "bpmn",
        "roomCode": "",
        "nodes": (0..nos).map(|i| json!({"id": format!("n{i}"), "type": "task", "x": i, "y": 0}))
            .collect::<Vec<_>>(),
        "edges": [],
        "strokes": [],
        "createdAt": "2026-10-06T00:00:00.000Z",
        "updatedAt": "2026-10-06T00:00:00.000Z",
    })
}

fn pedido(titulo: &str, nos: usize) -> Value {
    json!({
        "title": titulo,
        "notation": "bpmn",
        "room_code": "",
        "elements": nos,
        "doc": doc(titulo, nos),
    })
}

async fn gravar(app: &TestApp, quem: &Account, id: &str, titulo: &str, nos: usize) -> (u16, Value) {
    app.put(
        &format!("/api/diagrams/{id}"),
        Some(&quem.token),
        pedido(titulo, nos),
    )
    .await
}

#[sqlx::test(migrations = "./migrations")]
async fn um_diagrama_e_da_pessoa_e_o_mesmo_id_de_outra_nao_se_toca(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let alfa = app.new_org("alfa.test").await;
    let beta = app.new_org("beta.test").await;
    // O colega da MESMA organização é o controlo que falta à maior parte dos
    // testes de isolamento: quem só prova «outra empresa não vê» deixa passar
    // uma biblioteca partilhada por engano dentro da casa.
    let colega = app.add_member(&alfa, "colega", "member").await;

    // O MESMO id em três contas. É o caso que o `uid('d')` do cliente torna
    // provável, não hipotético.
    let (st, _) = gravar(&app, &alfa, "d-partilhado", "da alfa", 3).await;
    assert_eq!(st, 200);
    let (st, _) = gravar(&app, &colega, "d-partilhado", "do colega", 5).await;
    assert_eq!(st, 200, "o id do outro não pode ocupar o id deste");
    let (st, _) = gravar(&app, &beta, "d-partilhado", "da beta", 7).await;
    assert_eq!(st, 200);

    // Cada um lê o SEU.
    for (quem, titulo, nos) in [
        (&alfa, "da alfa", 3),
        (&colega, "do colega", 5),
        (&beta, "da beta", 7),
    ] {
        let (st, v) = app
            .get("/api/diagrams/d-partilhado", Some(&quem.token))
            .await;
        assert_eq!(st, 200);
        assert_eq!(v["title"], titulo, "leu o diagrama de outra pessoa");
        assert_eq!(v["elements"], nos as i64);
        // O documento volta inteiro e igual: é o que faz valer a pena ter
        // servidor em vez do IndexedDB.
        assert_eq!(v["doc"], doc(titulo, nos));
    }

    // E a LISTA de cada um tem um só.
    for quem in [&alfa, &colega, &beta] {
        let (st, v) = app.get("/api/diagrams", Some(&quem.token)).await;
        assert_eq!(st, 200);
        assert_eq!(
            v.as_array().unwrap().len(),
            1,
            "a lista trouxe o de outro: {v}"
        );
    }
    // Nenhuma lista traz o título de outro — nem a do colega de casa.
    let (_, v) = app.get("/api/diagrams", Some(&colega.token)).await;
    assert_denied("lista do colega não traz o da dona", 404, &v, "da alfa");
    let (_, v) = app.get("/api/diagrams", Some(&beta.token)).await;
    assert_denied("lista da beta não traz o da alfa", 404, &v, "da alfa");
}

#[sqlx::test(migrations = "./migrations")]
async fn apagar_o_de_outra_pessoa_da_404_e_nao_apaga_nada(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let dona = app.new_org("alfa.test").await;
    let outra = app.new_org("beta.test").await;
    gravar(&app, &dona, "d9", "o meu", 2).await;

    let (st, v) = app.delete("/api/diagrams/d9", Some(&outra.token)).await;
    assert_eq!(st, 404, "apagar o de outra pessoa: {v}");

    // O controlo que dá valor ao 404: a linha CONTINUA lá. Sem isto, um
    // `DELETE` que apagasse e respondesse 404 passava por correcto.
    let (st, v) = app.get("/api/diagrams/d9", Some(&dona.token)).await;
    assert_eq!(st, 200, "o diagrama da dona desapareceu: {v}");

    // E a dona apaga o seu.
    let (st, _) = app.delete("/api/diagrams/d9", Some(&dona.token)).await;
    assert_eq!(st, 204);
    let (st, _) = app.get("/api/diagrams/d9", Some(&dona.token)).await;
    assert_eq!(st, 404);
}

#[sqlx::test(migrations = "./migrations")]
async fn duas_abas_a_gravar_do_mesmo_estado_dao_conflito(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let a = app.new_org("alfa.test").await;

    let (st, primeiro) = gravar(&app, &a, "d1", "v1", 1).await;
    assert_eq!(st, 200);
    let visto = primeiro["updated_at"].as_str().unwrap().to_string();

    // A primeira aba grava com o que viu: passa.
    let mut p = pedido("v2", 2);
    p["expected_updated_at"] = json!(visto);
    let (st, segundo) = app.put("/api/diagrams/d1", Some(&a.token), p.clone()).await;
    assert_eq!(st, 200, "{segundo}");

    // A segunda aba ainda tem o `updated_at` ANTIGO: 409, e o servidor não
    // escolhe um vencedor.
    let (st, v) = app.put("/api/diagrams/d1", Some(&a.token), p).await;
    assert_eq!(st, 409, "{v}");
    assert_eq!(v["error"], "diagram.conflict");

    // E o documento gravado é o da primeira — o 409 não deixou nada a meio.
    let (st, actual) = app.get("/api/diagrams/d1", Some(&a.token)).await;
    assert_eq!(st, 200);
    assert_eq!(actual["title"], "v2");
    assert_eq!(actual["elements"], 2);

    // Sem `expected_updated_at` grava — é o cliente que não participa no
    // controlo (o primeiro envio de um diagrama que nasceu offline).
    let (st, _) = gravar(&app, &a, "d1", "v3", 3).await;
    assert_eq!(st, 200);
}

#[sqlx::test(migrations = "./migrations")]
async fn o_que_o_servidor_recusa_escrever(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let a = app.new_org("alfa.test").await;

    // Documento que não é objecto.
    let mut p = pedido("x", 1);
    p["doc"] = json!([1, 2, 3]);
    let (st, _) = app.put("/api/diagrams/d1", Some(&a.token), p).await;
    assert_eq!(st, 400);

    // Notação que o cliente não sabe desenhar.
    let mut p = pedido("x", 1);
    p["notation"] = json!("esquema-do-joao");
    let (st, _) = app.put("/api/diagrams/d2", Some(&a.token), p).await;
    assert_eq!(st, 400);

    // Sem sessão não se lê nem se escreve.
    let (st, _) = app.get("/api/diagrams", None).await;
    assert_eq!(st, 401);
    let (st, _) = app.put("/api/diagrams/d3", None, pedido("x", 1)).await;
    assert_eq!(st, 401);

    // Nada do que foi recusado ficou gravado.
    let (st, v) = app.get("/api/diagrams", Some(&a.token)).await;
    assert_eq!(st, 200);
    assert_eq!(v.as_array().unwrap().len(), 0, "{v}");
}
