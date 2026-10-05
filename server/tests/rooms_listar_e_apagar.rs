//! `GET /api/rooms` e `DELETE /api/rooms/{code}` contra Postgres real.
//!
//! As duas rotas que faltavam para uma sala ter ciclo de vida: criavam-se
//! salas de três sítios e não havia onde as ver nem como as tirar da frente.
//!
//! O que se mede aqui, por ordem do que mais custa se partir:
//!
//! 1. **A lista é de QUEM PEDE.** Um código de sala é uma credencial: quem o
//!    conhece lê os metadados. A LISTA não é isso — ver as salas de outra
//!    pessoa é ver os códigos todos dela de uma vez.
//! 2. **O apagar recusa quando leva alguma coisa por à frente.** A tabela
//!    `rooms` é a raiz de oito `ON DELETE CASCADE`: uma gravação some com a
//!    linha e o ficheiro fica órfão no disco. A agenda é pior ainda, porque
//!    `meetings.room_code` é TEXTO sem chave estrangeira — ninguém a protege.
//! 3. **O cursor não perde nem repete linhas.** Uma listagem que corta aos N
//!    sem dizer é como não ter listagem.
mod common;

use common::{jwt_claims, TestApp};
use serde_json::json;

const ROOMS: &str = "/api/rooms";

/// Cria uma sala e devolve o código.
async fn criar(app: &TestApp, token: &str, nome: &str) -> String {
    let (st, r) = app
        .post(ROOMS, Some(token), json!({"name": nome, "topology": "sfu"}))
        .await;
    assert_eq!(st, 200, "criar {nome}: {r}");
    r["code"].as_str().unwrap().to_string()
}

#[sqlx::test(migrations = "./migrations")]
async fn a_lista_e_so_das_minhas_salas(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let a = app.new_org("alfa.test").await;
    let b = app.new_org("beta.test").await;

    let (st, _) = app.get(ROOMS, None).await;
    assert_eq!(st, 401, "sem sessão não há lista");

    let minha = criar(&app, &a.token, "Alfa um").await;
    let dela = criar(&app, &b.token, "Beta um").await;

    let (st, lista) = app.get(ROOMS, Some(&a.token)).await;
    assert_eq!(st, 200, "{lista}");
    let codigos: Vec<&str> = lista["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["code"].as_str().unwrap())
        .collect();
    assert!(
        codigos.contains(&minha.as_str()),
        "a minha sala está: {lista}"
    );
    assert!(
        !codigos.contains(&dela.as_str()),
        "a sala de OUTRA pessoa não pode aparecer: {lista}"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn a_lista_pagina_por_cursor_sem_perder_nem_repetir(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let a = app.new_org("alfa.test").await;

    let mut criadas = Vec::new();
    for i in 0..5 {
        criadas.push(criar(&app, &a.token, &format!("Sala {i}")).await);
    }

    let mut vistas: Vec<String> = Vec::new();
    let mut url = format!("{ROOMS}?page_size=2");
    loop {
        let (st, p) = app.get(&url, Some(&a.token)).await;
        assert_eq!(st, 200, "{p}");
        let itens = p["items"].as_array().unwrap();
        assert!(itens.len() <= 2, "page_size respeitado: {p}");
        for r in itens {
            vistas.push(r["code"].as_str().unwrap().to_string());
        }
        match p["next_page_token"].as_str() {
            Some(t) => url = format!("{ROOMS}?page_size=2&page_token={t}"),
            None => break,
        }
    }
    let mut unicas = vistas.clone();
    unicas.sort();
    unicas.dedup();
    assert_eq!(
        unicas.len(),
        vistas.len(),
        "nenhuma linha repetida: {vistas:?}"
    );
    for c in &criadas {
        assert!(
            vistas.contains(c),
            "a sala {c} não apareceu em página nenhuma"
        );
    }

    let (st, erro) = app
        .get(
            &format!("{ROOMS}?page_token=isto-nao-e-um-cursor"),
            Some(&a.token),
        )
        .await;
    assert_eq!(
        st, 400,
        "um cursor corrompido é erro do cliente, não a primeira página: {erro}"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn apagar_e_so_do_dono_e_devolve_204(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let a = app.new_org("alfa.test").await;
    let b = app.new_org("beta.test").await;
    let code = criar(&app, &a.token, "Para apagar").await;

    let (st, erro) = app.delete(&format!("{ROOMS}/{code}"), Some(&b.token)).await;
    assert_eq!(st, 403, "quem não é dono não apaga: {erro}");

    let (st, _) = app.delete(&format!("{ROOMS}/{code}"), Some(&a.token)).await;
    assert_eq!(st, 204, "apagada sem corpo");

    let (st, _) = app.get(&format!("{ROOMS}/{code}"), Some(&a.token)).await;
    assert_eq!(st, 404, "e deixa de existir");

    let (st, _) = app.delete(&format!("{ROOMS}/{code}"), Some(&a.token)).await;
    assert_eq!(st, 404, "apagar o que já não existe é 404");
}

#[sqlx::test(migrations = "./migrations")]
async fn apagar_recusa_com_gravacoes(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let a = app.new_org("alfa.test").await;
    let code = criar(&app, &a.token, "Com gravação").await;

    let room_id: uuid::Uuid = sqlx::query_scalar("SELECT id FROM rooms WHERE code = $1")
        .bind(&code)
        .fetch_one(&app.db)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO recordings (room_id, uploader_id, filename, size_bytes)
         VALUES ($1, $2::uuid, 'aula.webm', 1024)",
    )
    .bind(room_id)
    .bind(&a.user_id)
    .execute(&app.db)
    .await
    .unwrap();

    let (st, erro) = app.delete(&format!("{ROOMS}/{code}"), Some(&a.token)).await;
    assert_eq!(st, 409, "{erro}");
    assert_eq!(
        erro["code"], "room.has_recordings",
        "a razão vai no code, que é contrato: {erro}"
    );

    // E a sala continua lá — a recusa não pode ser meia-feita.
    let (st, _) = app.get(&format!("{ROOMS}/{code}"), Some(&a.token)).await;
    assert_eq!(st, 200, "a sala sobreviveu à recusa");
}

#[sqlx::test(migrations = "./migrations")]
async fn apagar_recusa_com_reuniao_futura_na_agenda(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let a = app.new_org("alfa.test").await;
    let code = criar(&app, &a.token, "Marcada").await;

    // `meetings.room_code` é texto SEM chave estrangeira: a base não protege
    // nada aqui, e sem esta guarda a agenda ficava a apontar a um código morto.
    sqlx::query(
        "INSERT INTO meetings (owner_id, title, starts_at, room_code)
         VALUES ($1::uuid, 'Reunião de quinta', now() + interval '2 days', $2)",
    )
    .bind(&a.user_id)
    .bind(&code)
    .execute(&app.db)
    .await
    .unwrap();

    let (st, erro) = app.delete(&format!("{ROOMS}/{code}"), Some(&a.token)).await;
    assert_eq!(st, 409, "{erro}");
    assert_eq!(erro["code"], "room.has_scheduled_meeting", "{erro}");

    // Uma reunião PASSADA não impede: o que se protege é o que ainda vai acontecer.
    sqlx::query("UPDATE meetings SET starts_at = now() - interval '2 days' WHERE room_code = $1")
        .bind(&code)
        .execute(&app.db)
        .await
        .unwrap();
    let (st, _) = app.delete(&format!("{ROOMS}/{code}"), Some(&a.token)).await;
    assert_eq!(st, 204, "com a reunião no passado, apaga");
}

#[sqlx::test(migrations = "./migrations")]
async fn a_sala_pessoal_nao_se_apaga(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let a = app.new_org("alfa.test").await;

    let (st, pessoal) = app.get("/api/users/me/room", Some(&a.token)).await;
    assert_eq!(st, 200, "{pessoal}");
    let code = pessoal["code"].as_str().unwrap();

    let (st, erro) = app.delete(&format!("{ROOMS}/{code}"), Some(&a.token)).await;
    assert_eq!(st, 409, "{erro}");
    assert_eq!(erro["code"], "room.personal_cannot_be_deleted", "{erro}");

    // E aparece na lista como qualquer outra — é uma sala, não um fantasma.
    let (st, lista) = app.get(ROOMS, Some(&a.token)).await;
    assert_eq!(st, 200);
    let tem = lista["items"]
        .as_array()
        .unwrap()
        .iter()
        .any(|r| r["code"] == code);
    assert!(tem, "a sala pessoal está na lista: {lista}");
}

/// O token é usado pelo `jwt_claims` noutros testes; aqui só se confirma que
/// a rota de lista não aceita um token de SALA, que é outra credencial.
#[sqlx::test(migrations = "./migrations")]
async fn um_token_de_sala_nao_lista_salas(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let a = app.new_org("alfa.test").await;
    let code = criar(&app, &a.token, "Alfa").await;

    let (st, entrada) = app
        .post(&format!("{ROOMS}/{code}/join"), Some(&a.token), json!({}))
        .await;
    assert_eq!(st, 200, "{entrada}");
    let room_token = entrada["room_token"].as_str().unwrap();
    assert!(
        jwt_claims(room_token).get("room").is_some(),
        "o token de sala traz a sala"
    );

    let (st, _) = app.get(ROOMS, Some(room_token)).await;
    assert_eq!(st, 401, "um token de sala não é uma sessão");
}
