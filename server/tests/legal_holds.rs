//! Legal hold (A-08): o que está retido por ordem legal não se apaga.
//!
//! **O critério do A-08, como teste:** com um hold activo, a varredura de
//! retenção corre com o prazo largamente passado e as linhas CONTINUAM na base;
//! e o controlo negativo — sem o hold, a MESMA varredura apaga. A observação é
//! um `SELECT count(*)` na base, não a resposta de uma rota.
//!
//! E a guarda que não depende de quem apaga: o gatilho da migração 0129 recusa
//! o `DELETE` de uma linha retida venha de onde vier — aqui, a cascata de
//! apagar uma sala, que sai como 409 `legal_hold.active` e não como 500.

mod common;

use common::{Account, TestApp};
use serde_json::{json, Value};
use uuid::Uuid;

/// Uma gravação com 400 dias, gravada por `dono` numa sala dele.
async fn gravacao_antiga(app: &TestApp, dono: &Account) -> (Uuid, Uuid, String) {
    let sala = app.new_room(dono, "Sala da auditoria").await;
    let room_id: Uuid = sala["id"].as_str().unwrap().parse().unwrap();
    let code = sala["code"].as_str().unwrap().to_string();
    let rec: Uuid = sqlx::query_scalar(
        "INSERT INTO recordings (room_id, uploader_id, filename, size_bytes, status, created_at)
         VALUES ($1, $2::uuid, 'antiga.webm', 4, 'ready', now() - interval '400 days') RETURNING id",
    )
    .bind(room_id)
    .bind(&dono.user_id)
    .fetch_one(&app.db)
    .await
    .unwrap();
    (rec, room_id, code)
}

/// Mensagens de chat com 400 dias numa sala.
async fn chat_antigo(app: &TestApp, room_id: Uuid, autor: &Account, n: i32) {
    sqlx::query(
        "INSERT INTO room_chat_messages (room_id, user_id, username, message, created_at)
         SELECT $1, $2::uuid, 'autor', 'mensagem ' || g, now() - interval '400 days'
           FROM generate_series(1, $3) g",
    )
    .bind(room_id)
    .bind(&autor.user_id)
    .bind(n)
    .execute(&app.db)
    .await
    .unwrap();
}

/// Retenção de 30 dias para gravações e chat, como um administrador a poria.
async fn retencao_curta(app: &TestApp, org: &str) {
    sqlx::query(
        "UPDATE organizations SET retention_days = 30, chat_retention_days = 30 WHERE id = $1::uuid",
    )
    .bind(org)
    .execute(&app.db)
    .await
    .unwrap();
}

async fn existe_gravacao(app: &TestApp, id: Uuid) -> bool {
    sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM recordings WHERE id = $1)")
        .bind(id)
        .fetch_one(&app.db)
        .await
        .unwrap()
}

async fn mensagens(app: &TestApp, room: Uuid) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM room_chat_messages WHERE room_id = $1")
        .bind(room)
        .fetch_one(&app.db)
        .await
        .unwrap()
}

async fn abre(app: &TestApp, admin: &Account, corpo: Value) -> (u16, Value) {
    app.post(
        &format!("/api/orgs/{}/legal-holds", admin.org()),
        Some(&admin.token),
        corpo,
    )
    .await
}

/// O critério 1 do A-08, para as GRAVAÇÕES: retida, a varredura não a apaga;
/// levantado o hold, a mesma varredura apaga.
#[sqlx::test(migrations = "./migrations")]
async fn com_hold_a_retencao_nao_apaga_a_gravacao_e_sem_hold_apaga(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let a = app.new_org("banco.ao").await;
    let ana = app.add_member(&a, "ana", "member").await;
    retencao_curta(&app, a.org()).await;
    let (rec_ana, _, _) = gravacao_antiga(&app, &ana).await;
    let (rec_admin, _, _) = gravacao_antiga(&app, &a).await;
    // O FICHEIRO da gravação retida, registado como a produção o regista
    // (migração 0126). É ele que prova o filtro da varredura: sem o filtro, o
    // gatilho da 0129 ainda impedia a linha de sair — mas os ficheiros já
    // tinham sido purgados antes do DELETE.
    let dir = app.state.config.recordings_dir.clone();
    std::fs::create_dir_all(&dir).unwrap();
    let ficheiro = dir.join(format!("{rec_ana}.webm"));
    std::fs::write(&ficheiro, b"webm").unwrap();
    sqlx::query(
        "INSERT INTO recording_artifacts (recording_id, kind, path, is_dir)
         VALUES ($1, 'recording', $2, FALSE)",
    )
    .bind(rec_ana)
    .bind(format!("{rec_ana}.webm"))
    .execute(&app.db)
    .await
    .unwrap();

    let (st, hold) = abre(
        &app,
        &a,
        json!({"name": "Processo 123/2026", "reason": "Ofício do tribunal de Luanda, ref. 123/2026",
               "scope": "user", "user_id": ana.user_id}),
    )
    .await;
    assert_eq!(st, 201, "{hold}");

    delonix_server::recordings_retention_sweep(&app.state).await;
    assert!(
        existe_gravacao(&app, rec_ana).await,
        "a gravação da Ana estava retida e foi apagada"
    );
    assert!(
        ficheiro.exists(),
        "a linha ficou, mas o FICHEIRO da gravação retida foi purgado"
    );
    // CONTROLO na mesma volta: a do administrador, sem hold, saiu.
    assert!(
        !existe_gravacao(&app, rec_admin).await,
        "a varredura não apagou a gravação que NÃO estava retida — o teste mediria uma varredura parada"
    );

    // Levantado o hold, a retenção volta a valer.
    let (st, solto) = app
        .post(
            &format!(
                "/api/orgs/{}/legal-holds/{}/release",
                a.org(),
                hold["id"].as_str().unwrap()
            ),
            Some(&a.token),
            json!({}),
        )
        .await;
    assert_eq!(st, 200, "{solto}");
    assert!(solto["released_at"].is_string(), "{solto}");
    delonix_server::recordings_retention_sweep(&app.state).await;
    assert!(
        !existe_gravacao(&app, rec_ana).await,
        "levantado o hold, a retenção tinha de a apagar"
    );
}

/// O critério 1 para o CHAT: um hold sobre a sala guarda as mensagens dela; o
/// chat de outra sala, sem hold, sai na mesma volta.
#[sqlx::test(migrations = "./migrations")]
async fn com_hold_a_retencao_nao_apaga_o_chat_da_sala(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let a = app.new_org("seguros.ao").await;
    retencao_curta(&app, a.org()).await;
    let retida = app.new_room(&a, "Comité de risco").await;
    let livre = app.new_room(&a, "Café").await;
    let retida_id: Uuid = retida["id"].as_str().unwrap().parse().unwrap();
    let livre_id: Uuid = livre["id"].as_str().unwrap().parse().unwrap();
    chat_antigo(&app, retida_id, &a, 5).await;
    chat_antigo(&app, livre_id, &a, 3).await;

    let (st, _) = abre(
        &app,
        &a,
        json!({"name": "Inquérito BNA", "reason": "Pedido do Banco Nacional de Angola",
               "scope": "room", "room_code": retida["code"]}),
    )
    .await;
    assert_eq!(st, 201);

    delonix_server::room_chat_retention_sweep(&app.state)
        .await
        .unwrap();
    assert_eq!(
        mensagens(&app, retida_id).await,
        5,
        "o chat retido foi apagado"
    );
    assert_eq!(
        mensagens(&app, livre_id).await,
        0,
        "o chat sem hold não saiu — a varredura não correu"
    );
}

/// A guarda na BASE: apagar uma sala com chat retido cai na cascata, o gatilho
/// recusa, e a rota responde 409 `legal_hold.active` — não um 500.
#[sqlx::test(migrations = "./migrations")]
async fn apagar_uma_sala_retida_e_recusado_com_razao(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let a = app.new_org("estado.ao").await;
    let sala = app.new_room(&a, "Gabinete").await;
    let sala_id: Uuid = sala["id"].as_str().unwrap().parse().unwrap();
    let code = sala["code"].as_str().unwrap();
    chat_antigo(&app, sala_id, &a, 2).await;
    let (st, _) = abre(
        &app,
        &a,
        json!({"name": "Ordem", "reason": "Despacho do Ministério Público", "scope": "org"}),
    )
    .await;
    assert_eq!(st, 201);

    let (st, e) = app
        .delete(&format!("/api/rooms/{code}"), Some(&a.token))
        .await;
    assert_eq!(st, 409, "{e}");
    assert_eq!(e["code"], "legal_hold.active", "{e}");
    assert_eq!(mensagens(&app, sala_id).await, 2);

    // E um DELETE directo na base também não passa.
    let r = sqlx::query("DELETE FROM room_chat_messages WHERE room_id = $1")
        .bind(sala_id)
        .execute(&app.db)
        .await;
    assert!(r.is_err(), "o gatilho tinha de recusar o DELETE directo");
}

/// Quem pode, e sobre o quê: a capacidade, a fronteira entre organizações, e as
/// validações. Um hold levantado não se levanta duas vezes.
#[sqlx::test(migrations = "./migrations")]
async fn so_quem_muda_a_retencao_gere_holds_e_so_na_sua_organizacao(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let a = app.new_org("alfa.ao").await;
    let b = app.new_org("beta.ao").await;
    let membro = app.add_member(&a, "membro", "member").await;
    let de_b = app.add_member(&b, "bruno", "member").await;

    // Um membro comum não gere holds.
    let (st, _) = abre(
        &app,
        &membro,
        json!({"name": "x", "reason": "y", "scope": "org"}),
    )
    .await;
    assert_eq!(st, 403);
    // Uma pessoa de OUTRA organização não se põe sob hold a partir daqui.
    let (st, _) = abre(
        &app,
        &a,
        json!({"name": "x", "reason": "y", "scope": "user", "user_id": de_b.user_id}),
    )
    .await;
    assert_eq!(st, 404);
    // Validações.
    for corpo in [
        json!({"name": "", "reason": "y", "scope": "org"}),
        json!({"name": "x", "reason": "  ", "scope": "org"}),
        json!({"name": "x", "reason": "y", "scope": "user"}),
        json!({"name": "x", "reason": "y", "scope": "planeta"}),
    ] {
        let (st, e) = abre(&app, &a, corpo.clone()).await;
        assert_eq!(st, 400, "{corpo}: {e}");
        assert_eq!(e["code"], "legal_hold.invalid", "{corpo}: {e}");
    }

    let (st, hold) = abre(
        &app,
        &a,
        json!({"name": "Ordem", "reason": "Despacho", "scope": "org"}),
    )
    .await;
    assert_eq!(st, 201, "{hold}");
    let id = hold["id"].as_str().unwrap();

    // A organização B não vê nem levanta os holds de A.
    let (st, lista) = app
        .get(
            &format!("/api/orgs/{}/legal-holds", b.org()),
            Some(&b.token),
        )
        .await;
    assert_eq!(st, 200);
    assert_eq!(lista["items"].as_array().map(Vec::len), Some(0), "{lista}");
    let (st, _) = app
        .post(
            &format!("/api/orgs/{}/legal-holds/{id}/release", b.org()),
            Some(&b.token),
            json!({}),
        )
        .await;
    assert_eq!(st, 404);

    // Levantar duas vezes é 409, e a lista de A mostra-o levantado.
    let release = format!("/api/orgs/{}/legal-holds/{id}/release", a.org());
    assert_eq!(app.post(&release, Some(&a.token), json!({})).await.0, 200);
    let (st, e) = app.post(&release, Some(&a.token), json!({})).await;
    assert_eq!(st, 409, "{e}");
    assert_eq!(e["code"], "legal_hold.already_released", "{e}");

    // Cada passo ficou na auditoria encadeada.
    let n: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM audit_logs WHERE org_id = $1::uuid AND action LIKE 'legal_hold.%'",
    )
    .bind(a.org())
    .fetch_one(&app.db)
    .await
    .unwrap();
    assert_eq!(n, 2, "abrir e levantar ficam na auditoria");
}
