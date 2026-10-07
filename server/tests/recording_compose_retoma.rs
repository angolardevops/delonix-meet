//! **A composição de uma gravação é retomável** (migração 0099), contra
//! Postgres real.
//!
//! O QUE ISTO GUARDA: a composição era um `tokio::spawn` nu cujo directório de
//! segmentos só existia na memória da tarefa. Um reinício do servidor — ou
//! seja, QUALQUER rollout — deixava a gravação em `processing` sem ninguém
//! capaz de a retomar, e `fail_stale_processing` marcava-a `failed` ao fim de
//! `FFMPEG_TIMEOUT_SECS + 600`. Medido a 2026-10-07: o ffmpeg tem 3600 s, o
//! drain 52 s e o K8s manda SIGKILL aos 60 s — a perda era certa, não provável.
//!
//! O QUE ESTES TESTES PROVAM e o que NÃO provam: provam a **máquina de
//! estados** — quem é reivindicado, quem é poupado, quem é fechado e com que
//! causa. NÃO provam a composição a chegar ao fim, porque isso precisa de
//! ffmpeg e **o CI não o tem** (`.github/workflows/ci.yml:397`); a passagem a
//! `ready` numa retoma mede-se à mão, numa máquina com ffmpeg, e está no
//! relatório do PR.

mod common;

use common::TestApp;
use serde_json::json;
use uuid::Uuid;

/// Uma gravação em `processing`, com manifesto e com a reserva no estado que o
/// teste quiser. `dir` é o directório dos segmentos (pode nem existir — é
/// isso que um pod novo com disco efémero vê).
async fn em_processing(
    app: &TestApp,
    room_id: &str,
    user_id: &str,
    dir: &std::path::Path,
    com_manifesto: bool,
    tentativas: i32,
    reserva_expirada: bool,
) -> Uuid {
    let manifesto = json!({
        "dir": dir,
        "tracks": [{"path": dir.join("00-audio.ogg"), "kind": "audio", "starts_at_ms": 0}],
        "expected_ms": 60_000,
        "duration_secs": 60,
        "by_user": user_id,
    });
    let (id,): (Uuid,) = sqlx::query_as(
        "INSERT INTO recordings (room_id, uploader_id, filename, size_bytes, status,
                                 progress_pct, progress_at, kind,
                                 compose_manifest, compose_lease_token,
                                 compose_lease_expires_at, compose_attempts)
         VALUES ($1::uuid, $2::uuid, 'a compor.webm', 0, 'processing', 0,
                 now() - interval '3 hours', 'meeting',
                 CASE WHEN $3 THEN $4::jsonb ELSE NULL END,
                 'tok',
                 CASE WHEN $5 THEN now() - interval '1 minute'
                      ELSE now() + interval '1 hour' END,
                 $6)
         RETURNING id",
    )
    .bind(room_id)
    .bind(user_id)
    .bind(com_manifesto)
    .bind(manifesto.to_string())
    .bind(reserva_expirada)
    .bind(tentativas)
    .fetch_one(&app.db)
    .await
    .unwrap();
    id
}

struct Estado {
    status: String,
    tentativas: i32,
    tem_manifesto: bool,
    causa: Option<String>,
}

async fn estado(app: &TestApp, id: Uuid) -> Estado {
    let (status, tentativas, tem_manifesto, causa): (String, i32, bool, Option<String>) =
        sqlx::query_as(
            "SELECT status, compose_attempts, compose_manifest IS NOT NULL, failure_reason
               FROM recordings WHERE id = $1",
        )
        .bind(id)
        .fetch_one(&app.db)
        .await
        .unwrap();
    Estado {
        status,
        tentativas,
        tem_manifesto,
        causa,
    }
}

/// Prepara um directório de segmentos que EXISTE, dentro do `recordings_dir`
/// do harness, com um ficheiro lá dentro.
async fn segmentos(app: &TestApp) -> std::path::PathBuf {
    let dir = app
        .state
        .config
        .recordings_dir
        .join(format!("tmp-{}", Uuid::new_v4()));
    tokio::fs::create_dir_all(&dir).await.unwrap();
    tokio::fs::write(dir.join("00-audio.ogg"), b"nao e ogg a serio")
        .await
        .unwrap();
    dir
}

/// **O defeito que isto guarda.** Uma composição interrompida há três horas,
/// com os segmentos ainda no disco, NÃO pode ser marcada `failed` pelo
/// varredor: é precisamente o caso que a retoma existe para salvar.
#[sqlx::test(migrations = "./migrations")]
async fn uma_composicao_retomavel_nao_e_fechada_pelo_varredor(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let a = app.new_org("alfa.test").await;
    let room = app.new_room(&a, "gravada").await;
    let room_id = room["id"].as_str().unwrap();
    let dir = segmentos(&app).await;
    let id = em_processing(&app, room_id, &a.user_id, &dir, true, 1, true).await;

    // O varredor corre (progress_at tem 3 h, muito acima de ffmpeg+600 no
    // harness) e TEM de poupar esta linha.
    let e = estado(&app, id).await;
    assert_eq!(e.status, "processing");
    delonix_server::recording_resume_due(&app.state).await;
    let e = estado(&app, id).await;
    assert_ne!(
        e.status, "failed",
        "a gravação com manifesto e segmentos no disco foi dada por perdida"
    );
    // E foi reivindicada: a tentativa subiu.
    assert_eq!(e.tentativas, 2, "a retoma não contou a tentativa");
}

/// Sem manifesto não há retoma possível (linhas nascidas antes da 0099, ou
/// cujo `insert_processing` falhou). Essas o varredor fecha, como antes — e com
/// a causa certa, não em silêncio.
#[sqlx::test(migrations = "./migrations")]
async fn sem_manifesto_a_gravacao_e_fechada_com_causa(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let a = app.new_org("alfa.test").await;
    let room = app.new_room(&a, "gravada").await;
    let room_id = room["id"].as_str().unwrap();
    let dir = segmentos(&app).await;
    let id = em_processing(&app, room_id, &a.user_id, &dir, false, 0, true).await;

    // A retoma não a vê (o `compose_manifest IS NOT NULL` da reivindicação).
    assert_eq!(delonix_server::recording_resume_due(&app.state).await, 0);
    let e = estado(&app, id).await;
    assert_eq!(e.tentativas, 0, "reivindicou uma linha sem manifesto");
    assert_eq!(e.status, "processing");
}

/// Os segmentos já não estão no disco (pod novo, volume efémero): retomar é
/// impossível. Falha DEPRESSA e com a causa própria, em vez de gastar as três
/// tentativas no que não pode funcionar.
#[sqlx::test(migrations = "./migrations")]
async fn sem_os_segmentos_falha_depressa_e_diz_porque(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let a = app.new_org("alfa.test").await;
    let room = app.new_room(&a, "gravada").await;
    let room_id = room["id"].as_str().unwrap();
    let inexistente = app.state.config.recordings_dir.join("tmp-que-nao-existe");
    let id = em_processing(&app, room_id, &a.user_id, &inexistente, true, 1, true).await;

    delonix_server::recording_resume_due(&app.state).await;
    let e = estado(&app, id).await;
    assert_eq!(e.status, "failed");
    assert!(!e.tem_manifesto, "o manifesto morto ficou na linha");
    let causa = e.causa.unwrap_or_default();
    assert!(
        causa.contains("já não estavam disponíveis"),
        "a causa não distingue segmentos perdidos de uma interrupção: {causa}"
    );
}

/// A reserva em vigor é de outro nó. Enquanto não expirar, mais ninguém pega
/// na gravação — é o que impede dois pods de compor a mesma coisa.
#[sqlx::test(migrations = "./migrations")]
async fn uma_reserva_em_vigor_nao_e_roubada(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let a = app.new_org("alfa.test").await;
    let room = app.new_room(&a, "gravada").await;
    let room_id = room["id"].as_str().unwrap();
    let dir = segmentos(&app).await;
    let id = em_processing(&app, room_id, &a.user_id, &dir, true, 1, false).await;

    assert_eq!(delonix_server::recording_resume_due(&app.state).await, 0);
    assert_eq!(estado(&app, id).await.tentativas, 1);
}

/// As tentativas têm tecto (`composition::MAX_ATTEMPTS`). Uma gravação que
/// rebenta o ffmpeg não pode ocupar uma vaga de composição para sempre.
#[sqlx::test(migrations = "./migrations")]
async fn as_tentativas_esgotadas_saem_da_fila(db: sqlx::PgPool) {
    use delonix_meet_domain::content::composition as regras;
    let app = TestApp::spawn(db).await;
    let a = app.new_org("alfa.test").await;
    let room = app.new_room(&a, "gravada").await;
    let room_id = room["id"].as_str().unwrap();
    let dir = segmentos(&app).await;
    let id = em_processing(
        &app,
        room_id,
        &a.user_id,
        &dir,
        true,
        regras::MAX_ATTEMPTS,
        true,
    )
    .await;

    assert_eq!(delonix_server::recording_resume_due(&app.state).await, 0);
    assert_eq!(estado(&app, id).await.tentativas, regras::MAX_ATTEMPTS);
}

/// O varredor de segmentos órfãos apaga o que ninguém reclama e **poupa** o
/// que uma composição em `processing` tem no manifesto. Antes, só o caminho
/// feliz apagava o `tmp-*` e cada processo morto deixava segmentos RTP no
/// volume para sempre.
#[sqlx::test(migrations = "./migrations")]
async fn os_segmentos_orfaos_sao_apagados_e_os_reclamados_poupados(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let a = app.new_org("alfa.test").await;
    let room = app.new_room(&a, "gravada").await;
    let room_id = room["id"].as_str().unwrap();

    let reclamado = segmentos(&app).await;
    em_processing(&app, room_id, &a.user_id, &reclamado, true, 1, true).await;
    let orfao = segmentos(&app).await;
    // Envelhecido para trás da folga (a reserva mais longa + 1 h): um
    // directório recente pode ser de uma composição a arrancar neste instante,
    // cuja linha ainda não foi escrita. Com o `touch` do sistema e não com uma
    // dependência nova — é uma linha de teste, não vale um crate.
    let envelhecido = std::process::Command::new("touch")
        .args(["-d", "30 hours ago"])
        .arg(&orfao)
        .status()
        .expect("touch não existe nesta máquina");
    assert!(
        envelhecido.success(),
        "não foi possível envelhecer o directório"
    );

    let apagados = delonix_server::recording_sweep_orphan_segments(&app.state).await;
    assert_eq!(apagados, 1, "apagou o número errado de directórios");
    assert!(
        tokio::fs::metadata(&reclamado).await.is_ok(),
        "apagou os segmentos de uma composição que ainda vai ser retomada"
    );
    assert!(
        tokio::fs::metadata(&orfao).await.is_err(),
        "o directório órfão ficou no volume"
    );
}
