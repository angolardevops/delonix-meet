//! «Os meus dados» contra Postgres e servidor reais: pedido assíncrono, ZIP com
//! os dados da própria pessoa (e só dela), link assinado temporário, limite de
//! pedidos e expiração.
mod common;

use common::TestApp;
use serde_json::{json, Value};
use std::io::Read;
use std::time::Duration;

const EXPORTS: &str = "/api/users/me/data-exports";

async fn wait_ready(app: &TestApp, token: &str, id: &str) -> Value {
    for _ in 0..100 {
        let (st, e) = app.get(&format!("{EXPORTS}/{id}"), Some(token)).await;
        assert_eq!(st, 200, "{e}");
        match e["status"].as_str() {
            Some("ready") | Some("failed") => return e,
            _ => tokio::time::sleep(Duration::from_millis(100)).await,
        }
    }
    panic!("a exportação {id} não terminou");
}

fn unzip(bytes: &[u8]) -> std::collections::BTreeMap<String, String> {
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes)).expect("é um ZIP");
    let mut out = std::collections::BTreeMap::new();
    for i in 0..archive.len() {
        let mut f = archive.by_index(i).unwrap();
        let mut s = String::new();
        f.read_to_string(&mut s).unwrap();
        out.insert(f.name().to_string(), s);
    }
    out
}

/// R209 — a exportação leva o que é da pessoa (perfil, preferências,
/// gravações próprias como links, transcrições delas, actividade como actor,
/// uso G3) e nada de outra pessoa; o link é temporário e assinado.
#[sqlx::test(migrations = "./migrations")]
async fn export_contains_only_own_data_and_link_is_signed(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let ana = app.new_org("alfa.test").await;
    let bento = app.add_member(&ana, "bento", "member").await;
    let t = Some(ana.token.as_str());

    // Dados: uma sala partilhada com uma gravação da Ana e uma do Bento.
    let room = app.new_room(&ana, "Formação").await;
    let room_id = room["id"].as_str().unwrap();
    let rec_ana = app.insert_recording(room_id, &ana.user_id).await;
    let rec_bento = app.insert_recording(room_id, &bento.user_id).await;
    for (id, text) in [
        (&rec_ana, "Transcrição da Ana"),
        (&rec_bento, "SEGREDO do Bento"),
    ] {
        sqlx::query("UPDATE recordings SET transcript = $2 WHERE id = $1::uuid")
            .bind(id)
            .bind(text)
            .execute(&app.db)
            .await
            .unwrap();
    }
    app.patch(
        "/api/users/me/profile",
        t,
        json!({"job_title": "Directora"}),
    )
    .await;
    let (st, created) = app.post(EXPORTS, t, json!({})).await;
    assert_eq!(st, 202, "{created}");
    assert_eq!(created["status"], "queued");
    let id = created["id"].as_str().unwrap().to_string();

    let ready = wait_ready(&app, &ana.token, &id).await;
    assert_eq!(ready["status"], "ready", "{ready}");
    assert!(ready["size_bytes"].as_i64().unwrap() > 0);
    assert_eq!(ready["summary"]["recordings"], 1, "{ready}");
    assert_eq!(ready["summary"]["transcripts"], 1);
    assert!(ready["expires_at"].is_string());

    // O Bento não vê o pedido da Ana, nem pede link para ele.
    let (st, e) = app
        .get(&format!("{EXPORTS}/{id}"), Some(&bento.token))
        .await;
    assert_eq!(
        (st, e["code"].as_str()),
        (404, Some("data_export.not_found"))
    );
    let (st, _) = app
        .post(
            &format!("{EXPORTS}/{id}/download-link"),
            Some(&bento.token),
            json!({}),
        )
        .await;
    assert_eq!(st, 404);
    let (_, list) = app.get(EXPORTS, Some(&bento.token)).await;
    assert!(!list.to_string().contains(&id));

    // Sem link não há conteúdo, nem com assinatura inventada.
    let (st, _) = app.get(&format!("{EXPORTS}/{id}/content"), None).await;
    assert_eq!(st, 404);
    let (st, _) = app
        .get(
            &format!("{EXPORTS}/{id}/content?exp=9999999999&sig=00"),
            None,
        )
        .await;
    assert_eq!(st, 404);

    let (st, link) = app
        .post(&format!("{EXPORTS}/{id}/download-link"), t, json!({}))
        .await;
    assert_eq!(st, 200, "{link}");
    let url = link["url"].as_str().unwrap();
    // A assinatura não serve para outro id.
    let other = url.replacen(&id, &rec_ana, 1);
    assert_eq!(app.get(&other, None).await.0, 404);

    let res = app.http.get(app.url(url)).send().await.unwrap();
    assert_eq!(res.status(), 200, "o link abre sem sessão");
    assert_eq!(res.headers()["content-type"], "application/zip");
    let files = unzip(&res.bytes().await.unwrap());
    for name in [
        "manifest.json",
        "profile.json",
        "preferences.json",
        "recordings.json",
        "activity.json",
        "storage_usage.json",
    ] {
        assert!(
            files.contains_key(name),
            "{name} em falta: {:?}",
            files.keys()
        );
    }
    let profile: Value = serde_json::from_str(&files["profile.json"]).unwrap();
    assert_eq!(profile["id"], ana.user_id.as_str());
    assert_eq!(profile["job_title"], "Directora");
    let recordings: Value = serde_json::from_str(&files["recordings.json"]).unwrap();
    assert_eq!(recordings.as_array().unwrap().len(), 1);
    assert_eq!(
        recordings[0]["link"],
        format!("/api/recordings/{rec_ana}/content")
    );
    assert_eq!(
        files[&format!("transcripts/{rec_ana}.txt")],
        "Transcrição da Ana"
    );
    let usage: Value = serde_json::from_str(&files["storage_usage.json"]).unwrap();
    assert_eq!(usage["recordings"]["count"], 1, "uso G3: {usage}");
    assert_eq!(usage["user_id"], ana.user_id.as_str());
    let activity: Value = serde_json::from_str(&files["activity.json"]).unwrap();
    assert!(activity
        .as_array()
        .unwrap()
        .iter()
        .any(|a| a["action"] == "profile.updated"));
    let added = activity
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["action"] == "member.added")
        .expect("a Ana adicionou o Bento: a acção está na actividade dela");
    assert_eq!(
        added["target_redacted"], true,
        "o alvo é outra pessoa: {added}"
    );
    assert!(added.get("target").is_none());
    // Nada do Bento em ficheiro nenhum.
    let all: String = files.values().cloned().collect();
    assert!(
        !all.contains("SEGREDO do Bento"),
        "transcrição de outra pessoa"
    );
    assert!(!all.contains(&rec_bento), "gravação de outra pessoa");
    assert!(!all.contains(&bento.email), "email de outra pessoa");

    // Expiração: vencido, o ficheiro sai e o link morre.
    sqlx::query(
        "UPDATE data_exports SET expires_at = now() - interval '1 minute' WHERE id = $1::uuid",
    )
    .bind(&id)
    .execute(&app.db)
    .await
    .unwrap();
    assert_eq!(
        app.get(url, None).await.0,
        404,
        "vencido: o link já não abre"
    );
    let n = delonix_server::data_exports::sweep_expired(&app.state)
        .await
        .unwrap();
    assert_eq!(n, 1);
    let (_, e) = app.get(&format!("{EXPORTS}/{id}"), t).await;
    assert_eq!(e["status"], "expired");
    assert!(
        !app.state
            .config
            .data_exports_dir
            .join(format!("{id}.zip"))
            .exists(),
        "o ficheiro foi apagado"
    );
    let (st, e) = app
        .post(&format!("{EXPORTS}/{id}/download-link"), t, json!({}))
        .await;
    assert_eq!(
        (st, e["code"].as_str()),
        (409, Some("data_export.not_ready"))
    );
}

/// R209 — uma de cada vez, no máximo 3 pedidos por 24 h; sem sessão 401.
#[sqlx::test(migrations = "./migrations")]
async fn export_is_rate_limited(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let ana = app.new_org("alfa.test").await;
    assert_eq!(app.post(EXPORTS, None, json!({})).await.0, 401);
    // Uma em curso (um trabalho vivo, não abandonado): não se pede outra.
    sqlx::query("INSERT INTO data_exports (user_id, status, started_at) VALUES ($1::uuid, 'running', now())")
        .bind(&ana.user_id)
        .execute(&app.db)
        .await
        .unwrap();
    let (st, e) = app.post(EXPORTS, Some(&ana.token), json!({})).await;
    assert_eq!(
        (st, e["code"].as_str()),
        (409, Some("data_export.already_running")),
        "{e}"
    );
    sqlx::query("UPDATE data_exports SET status = 'failed' WHERE user_id = $1::uuid")
        .bind(&ana.user_id)
        .execute(&app.db)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO data_exports (user_id, status, created_at)
         SELECT $1::uuid, 'expired', now() - interval '1 hour' FROM generate_series(1, 2)",
    )
    .bind(&ana.user_id)
    .execute(&app.db)
    .await
    .unwrap();
    let (st, e) = app.post(EXPORTS, Some(&ana.token), json!({})).await;
    assert_eq!(
        (st, e["code"].as_str()),
        (429, Some("data_export.rate_limited")),
        "{e}"
    );
    // Fora da janela de 24 h volta a poder.
    sqlx::query(
        "UPDATE data_exports SET created_at = now() - interval '25 hours' WHERE user_id = $1::uuid",
    )
    .bind(&ana.user_id)
    .execute(&app.db)
    .await
    .unwrap();
    let (st, e) = app.post(EXPORTS, Some(&ana.token), json!({})).await;
    assert_eq!(st, 202, "{e}");
}

/// **Uma exportação que falha por acidente volta à fila, não à pessoa.**
///
/// O defeito que isto guarda: a fila das exportações tinha reserva, requeue do
/// abandonado e `SKIP LOCKED`, mas **zero tentativas**. Qualquer falha fechava
/// o pedido como `failed` com a mensagem «peça outra» — a pessoa era o
/// mecanismo de retry, e pela regra da casa um passo manual no caminho do
/// cliente é um bloqueio.
///
/// Força-se a falha tornando o directório das exportações impossível de
/// escrever, que é o que um disco cheio faz.
#[sqlx::test(migrations = "./migrations")]
async fn uma_exportacao_que_falha_volta_a_fila_com_espera(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let a = app.new_org("alfa.test").await;

    // Pede a exportação (entra `queued`) e tira-lhe o sítio onde escrever.
    let (s, _) = app
        .post("/api/users/me/data-exports", Some(&a.token), json!({}))
        .await;
    assert_eq!(s, 202, "o pedido não entrou na fila");
    // Põe um FICHEIRO onde o directório devia estar: é o que um disco cheio ou
    // uma permissão em falta fazem — escrever ali deixa de ser possível. O pai
    // tem de existir primeiro; o harness só cria o directório quando precisa.
    let dir = app.state.config.data_exports_dir.clone();
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(
        dir.parent()
            .expect("o directório das exportações não tem pai"),
    )
    .expect("não consegui criar o directório-pai");
    std::fs::write(&dir, b"isto nao e um directorio").expect("não consegui bloquear o directório");

    // A volta da fila: falha a gerar e TEM de voltar a `queued` com espera.
    delonix_server::data_export_run_queue(&app.state).await;
    let (status, attempts, espera): (String, i32, Option<chrono::DateTime<chrono::Utc>>) =
        sqlx::query_as(
            "SELECT status, attempts, next_attempt_at FROM data_exports WHERE user_id = $1::uuid",
        )
        .bind(&a.user_id)
        .fetch_one(&app.db)
        .await
        .unwrap();
    assert_eq!(attempts, 1, "a tentativa não foi contada");
    assert_eq!(
        status, "queued",
        "a exportação foi dada por perdida à primeira falha"
    );
    let espera = espera.expect("voltou à fila sem espera: repetiria em rajada");
    assert!(
        espera > chrono::Utc::now(),
        "a espera do backoff ficou no passado"
    );

    // E a espera é respeitada: a volta seguinte NÃO a leva outra vez.
    delonix_server::data_export_run_queue(&app.state).await;
    let depois: i32 =
        sqlx::query_scalar("SELECT attempts FROM data_exports WHERE user_id = $1::uuid")
            .bind(&a.user_id)
            .fetch_one(&app.db)
            .await
            .unwrap();
    assert_eq!(depois, 1, "a fila ignorou o next_attempt_at e repetiu já");

    let _ = std::fs::remove_file(&dir);
}
