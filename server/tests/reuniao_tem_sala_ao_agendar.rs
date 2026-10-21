//! A sala de uma reunião nasce COM a reunião (migração 0114), e a porta dela
//! diz a quem chega cedo que ainda não são horas.
//!
//! O que isto fixa, por ordem do que partiu antes: uma reunião agendada tinha
//! `room_code` NULL até ao `start`, e por isso não havia link para partilhar,
//! o `.ics` saía sem `URL:` e a `join_url` da v1 era `https://<org>/#/r/` —
//! um link partido. E porque `room_code IS NOT NULL` era o que significava
//! «já começou», criar a sala cedo obrigava a dar coluna própria a esse facto
//! (`started_at`): sem ela, tocar-se-ia aos convidados na criação e um
//! `meeting.ended` sairia de uma sala que ninguém tinha aberto.
mod common;

use chrono::Utc;
use common::TestApp;
use serde_json::{json, Value};

fn em_horas(h: i64) -> String {
    (Utc::now() + chrono::Duration::hours(h)).to_rfc3339()
}

async fn agenda(app: &TestApp, dono: &common::Account, titulo: &str, starts: &str) -> Value {
    let (st, m) = app
        .post(
            "/api/meetings",
            Some(&dono.token),
            json!({"title": titulo, "kind": "video", "starts_at": starts, "duration_min": 30}),
        )
        .await;
    assert_eq!(st, 200, "{m}");
    m
}

async fn ics(app: &TestApp, dono: &common::Account, id: &str) -> String {
    let auth = format!("Bearer {}", dono.token);
    let r = app
        .raw(
            reqwest::Method::GET,
            &format!("/api/meetings/{id}/calendar.ics"),
            &[("Authorization", &auth)],
            None,
        )
        .await;
    assert_eq!(r.status, 200, "{}", r.text);
    r.text
}

async fn started_at(app: &TestApp, id: &str) -> Option<chrono::DateTime<Utc>> {
    sqlx::query_scalar("SELECT started_at FROM meetings WHERE id = $1::uuid")
        .bind(id)
        .fetch_one(&app.db)
        .await
        .unwrap()
}

#[sqlx::test(migrations = "./migrations")]
async fn agendar_da_sala_e_a_porta_diz_a_hora(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let a = app.new_org("alfa.test").await;
    let m = agenda(&app, &a, "Daqui a duas horas", &em_horas(2)).await;

    // 1. A reunião nasce COM sala, e a sala existe mesmo.
    let code = m["room_code"].as_str().expect("sem room_code ao agendar");
    assert!(!code.is_empty());
    let existe: i64 = sqlx::query_scalar("SELECT count(*) FROM rooms WHERE code = $1")
        .bind(code)
        .fetch_one(&app.db)
        .await
        .unwrap();
    assert_eq!(existe, 1, "a sala do código não existe");
    assert_eq!(started_at(&app, m["id"].as_str().unwrap()).await, None);

    // 2. A porta é pública — sem token nenhum — e diz que ainda não abriu.
    let (st, porta) = app.get(&format!("/api/rooms/{code}/entrance"), None).await;
    assert_eq!(st, 200, "{porta}");
    assert_eq!(porta["open"], json!(false), "aberta antes da hora: {porta}");
    assert_eq!(porta["name"], json!("Daqui a duas horas"));
    assert!(porta["starts_at"].is_string(), "{porta}");
    assert_eq!(porta["duration_min"], json!(30));
    // E não deixa escapar nada de dentro: nem dono, nem organização.
    for proibido in ["owner_id", "org_id", "owner_name", "id"] {
        assert!(porta.get(proibido).is_none(), "{proibido} saiu: {porta}");
    }

    // 3. O anfitrião arranca: `started_at` fica gravado e a porta abre.
    let (st, s) = app
        .post(
            &format!("/api/meetings/{}/start", m["id"].as_str().unwrap()),
            Some(&a.token),
            json!({}),
        )
        .await;
    assert_eq!(st, 200, "{s}");
    assert_eq!(s["code"], json!(code), "arrancar criou OUTRA sala");
    let t1 = started_at(&app, m["id"].as_str().unwrap()).await;
    assert!(t1.is_some(), "o arranque não ficou registado");

    let (_, porta) = app.get(&format!("/api/rooms/{code}/entrance"), None).await;
    assert_eq!(porta["open"], json!(true), "{porta}");

    // 4. Arrancar duas vezes não é arrancar duas vezes: o instante não se mexe
    //    (é ele que impede um segundo toque aos convidados e um segundo
    //    `meeting.started`).
    let (st, s2) = app
        .post(
            &format!("/api/meetings/{}/start", m["id"].as_str().unwrap()),
            Some(&a.token),
            json!({}),
        )
        .await;
    assert_eq!(st, 200, "{s2}");
    assert_eq!(s2["code"], json!(code));
    assert_eq!(started_at(&app, m["id"].as_str().unwrap()).await, t1);
}

#[sqlx::test(migrations = "./migrations")]
async fn uma_sala_sem_reuniao_esta_sempre_aberta(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let a = app.new_org("alfa.test").await;
    let sala = app.new_room(&a, "Sala solta").await;
    let code = sala["code"].as_str().unwrap();

    let (st, porta) = app.get(&format!("/api/rooms/{code}/entrance"), None).await;
    assert_eq!(st, 200, "{porta}");
    assert_eq!(porta["open"], json!(true), "sala ad-hoc fechada: {porta}");
    assert_eq!(porta["starts_at"], Value::Null);

    // Um código que não existe não revela que não existe de outra maneira.
    let (st, _) = app.get("/api/rooms/nao-existe-ca/entrance", None).await;
    assert_eq!(st, 404);
}

#[sqlx::test(migrations = "./migrations")]
async fn convidado_antes_da_hora_recebe_a_contagem_e_nao_a_sala(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let a = app.new_org("alfa.test").await;
    let b = app.add_member(&a, "bea", "member").await;
    let (st, m) = app
        .post(
            "/api/meetings",
            Some(&a.token),
            json!({"title": "Com convidada", "kind": "video", "starts_at": em_horas(3),
                   "duration_min": 30, "invitee_ids": [b.user_id]}),
        )
        .await;
    assert_eq!(st, 200, "{m}");
    let id = m["id"].as_str().unwrap();

    // A convidada pede a sala antes da hora: não entra, e sabe porquê.
    let (st, e) = app
        .post(
            &format!("/api/meetings/{id}/start"),
            Some(&b.token),
            json!({}),
        )
        .await;
    assert_eq!(st, 422, "{e}");
    assert_eq!(e["code"], json!("meeting.not_started_yet"), "{e}");

    // Depois de o anfitrião arrancar, a mesma chamada dá a sala.
    let (st, _) = app
        .post(
            &format!("/api/meetings/{id}/start"),
            Some(&a.token),
            json!({}),
        )
        .await;
    assert_eq!(st, 200);
    let (st, s) = app
        .post(
            &format!("/api/meetings/{id}/start"),
            Some(&b.token),
            json!({}),
        )
        .await;
    assert_eq!(st, 200, "{s}");
    assert_eq!(s["code"], m["room_code"]);
}

#[sqlx::test(migrations = "./migrations")]
async fn passada_a_hora_a_porta_abre_sem_o_anfitriao(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let a = app.new_org("alfa.test").await;
    let b = app.add_member(&a, "bea", "member").await;
    let (st, m) = app
        .post(
            "/api/meetings",
            Some(&a.token),
            json!({"title": "Já devia ter começado", "kind": "video",
                   "starts_at": em_horas(-1), "duration_min": 120, "invitee_ids": [b.user_id]}),
        )
        .await;
    assert_eq!(st, 200, "{m}");
    let code = m["room_code"].as_str().unwrap();

    let (_, porta) = app.get(&format!("/api/rooms/{code}/entrance"), None).await;
    assert_eq!(
        porta["open"],
        json!(true),
        "a hora passou e está fechada: {porta}"
    );

    // E a convidada entra sem esperar pelo anfitrião: a hora é a hora.
    let (st, s) = app
        .post(
            &format!("/api/meetings/{}/start", m["id"].as_str().unwrap()),
            Some(&b.token),
            json!({}),
        )
        .await;
    assert_eq!(st, 200, "{s}");
    assert_eq!(s["code"], json!(code));
}

#[sqlx::test(migrations = "./migrations")]
async fn o_ics_leva_o_link_absoluto(db: sqlx::PgPool) {
    // Um `.ics` é lido NOUTRO programa, dias depois: um URL relativo lá dentro
    // não aponta para lado nenhum. Com `PUBLIC_URL` o link sai absoluto.
    let app = TestApp::spawn_with(db, &[("PUBLIC_URL", "https://meet.exemplo.ao")]).await;
    let a = app.new_org("alfa.test").await;
    let m = agenda(&app, &a, "Com ligação", &em_horas(2)).await;
    let code = m["room_code"].as_str().unwrap();

    let texto = ics(&app, &a, m["id"].as_str().unwrap()).await;
    assert!(
        texto.contains(&format!("URL:https://meet.exemplo.ao/#/r/{code}")),
        "o .ics saiu sem link absoluto: {texto}"
    );
    assert!(texto.contains(&format!("entrar: https://meet.exemplo.ao/#/r/{code}")));
}

#[sqlx::test(migrations = "./migrations")]
async fn sem_endereco_publico_o_ics_nao_inventa_um_link(db: sqlx::PgPool) {
    // Sem domínio da organização nem `PUBLIC_URL` não há link honesto para
    // escrever — e meio link é pior do que nenhum.
    let app = TestApp::spawn(db).await;
    let a = app.new_org("alfa.test").await;
    let m = agenda(&app, &a, "Sem endereço", &em_horas(2)).await;

    let texto = ics(&app, &a, m["id"].as_str().unwrap()).await;
    assert!(!texto.contains("URL:"), "inventou um link: {texto}");
    assert!(!texto.contains("/#/r/"), "link relativo no .ics: {texto}");
}

#[sqlx::test(migrations = "./migrations")]
async fn uma_serie_tem_um_link_so(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let a = app.new_org("alfa.test").await;
    let (st, m) = app
        .post(
            "/api/meetings",
            Some(&a.token),
            json!({"title": "Semanal", "kind": "video", "starts_at": em_horas(2),
                   "duration_min": 30, "recurrence_freq": "weekly", "recurrence_count": 4}),
        )
        .await;
    assert_eq!(st, 200, "{m}");
    let code = m["room_code"].as_str().unwrap();

    // As ocorrências herdam a sala da mãe — um código para a série inteira,
    // como no Zoom, e não 26 salas que ninguém abriria.
    let codigos: Vec<Option<String>> =
        sqlx::query_scalar("SELECT room_code FROM meetings WHERE recurrence_parent_id = $1::uuid")
            .bind(m["id"].as_str().unwrap())
            .fetch_all(&app.db)
            .await
            .unwrap();
    assert!(!codigos.is_empty(), "a série não gerou ocorrências");
    for c in &codigos {
        assert_eq!(c.as_deref(), Some(code), "ocorrência com sala diferente");
    }
}
