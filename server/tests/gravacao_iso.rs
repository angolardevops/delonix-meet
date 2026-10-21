//! A-02 fatia 1 — a gravação ISO entrega ficheiros por fonte. Contra Postgres
//! real, e com `ffprobe` no ficheiro DESCARREGADO.
//!
//! Porque existe: `studios.iso_recording` é `NOT NULL DEFAULT TRUE` desde a
//! migração 0081, com interruptor e ecrã, e a tabela `recording_tracks` e as
//! duas rotas que o contrato do estúdio nomeia nunca existiram. Um cliente de
//! produção ligava o estúdio, procurava as pistas por câmara e não as
//! encontrava.
//!
//! A observação é o **ffprobe do ficheiro que a rota devolveu** — não a
//! listagem, não o 200. Uma listagem pode mentir sobre o que há no disco; um
//! ficheiro que o ffprobe lê não.

mod common;

use common::{Account, TestApp};
use serde_json::Value;
use std::path::{Path, PathBuf};
use uuid::Uuid;

/// Sem ffmpeg não se faz um OGG verdadeiro nem se lê com ffprobe. Salta-se, e
/// DIZ-SE — um teste que se cala quando não mediu é um falso verde.
fn sem_ffmpeg() -> bool {
    let falta = std::process::Command::new("ffprobe")
        .arg("-version")
        .output()
        .is_err();
    if falta {
        eprintln!("SALTADO: ffprobe não está nesta máquina — a prova do ficheiro não corre");
    }
    falta
}

/// Um OGG/Opus de verdade, de um segundo, escrito em `destino`.
fn escreve_ogg_real(destino: &Path) {
    escreve_ogg(destino, 1, 440);
}

/// Um OGG/Opus de verdade, com a duração e o tom pedidos. Duas fontes com
/// durações diferentes é o que distingue «um ficheiro por fonte» de «o mesmo
/// ficheiro servido duas vezes».
fn escreve_ogg(destino: &Path, segundos: u32, hz: u32) {
    std::fs::create_dir_all(destino.parent().unwrap()).unwrap();
    let fonte = format!("sine=frequency={hz}:duration={segundos}");
    let saida = std::process::Command::new("ffmpeg")
        .args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-y",
            "-f",
            "lavfi",
            "-i",
            &fonte,
            "-c:a",
            "libopus",
            "-b:a",
            "32k",
        ])
        .arg(destino)
        .output()
        .expect("ffmpeg não arrancou");
    assert!(
        saida.status.success() && destino.exists(),
        "ffmpeg não escreveu o OGG: {}",
        String::from_utf8_lossy(&saida.stderr)
    );
}

/// A duração, em segundos, que o ffprobe lê no contentor.
fn duracao(ficheiro: &Path) -> f64 {
    let out = std::process::Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-show_entries",
            "format=duration",
            "-of",
            "csv=p=0",
        ])
        .arg(ficheiro)
        .output()
        .expect("ffprobe não arrancou");
    String::from_utf8_lossy(&out.stdout)
        .trim()
        .parse()
        .unwrap_or_else(|_| panic!("ffprobe não leu a duração de {}", ficheiro.display()))
}

/// Os fluxos que o ffprobe vê num ficheiro: `(codec, tipo)` por fluxo.
fn fluxos(ficheiro: &Path) -> Vec<(String, String)> {
    let out = std::process::Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-show_entries",
            "stream=codec_name,codec_type",
            "-of",
            "csv=p=0",
        ])
        .arg(ficheiro)
        .output()
        .expect("ffprobe não arrancou");
    assert!(
        out.status.success(),
        "ffprobe recusou o ficheiro: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| {
            let mut p = l.trim().split(',');
            (
                p.next().unwrap_or_default().to_string(),
                p.next().unwrap_or_default().to_string(),
            )
        })
        .collect()
}

/// Uma sala, uma gravação pronta, e uma pista ISO com ficheiro REAL no disco.
/// Devolve `(recording_id, track_id, caminho)`.
async fn gravacao_com_pista(
    app: &TestApp,
    dono: &Account,
    ficheiro: &str,
    numero: Option<i32>,
    etiqueta: Option<&str>,
) -> (Uuid, Uuid, PathBuf) {
    let sala = app.new_room(dono, "Sala do estúdio").await;
    let room_id = sala["id"].as_str().unwrap();
    let rec: Uuid = sqlx::query_scalar(
        "INSERT INTO recordings (room_id, uploader_id, filename, size_bytes, status)
         VALUES ($1::uuid, $2::uuid, 'composta.webm', 4, 'ready') RETURNING id",
    )
    .bind(room_id)
    .bind(&dono.user_id)
    .fetch_one(&app.db)
    .await
    .unwrap();

    // O ficheiro vai para onde o `finalize` o deixa: `iso-<gravação>/`, FORA
    // do `tmp-` que o varredor de órfãos apaga.
    let caminho =
        delonix_server::recording_iso_dir(&app.state.config.recordings_dir, rec).join(ficheiro);
    escreve_ogg_real(&caminho);
    let bytes = std::fs::metadata(&caminho).unwrap().len() as i64;

    // A pasta entra no REGISTO de artefactos, porque é o que o
    // `registar_pistas_iso` faz em produção — e antes do `rename`, não depois
    // (migração 0126). Sem esta linha a fixture montava um estado que a
    // produção não produz: uma pasta no disco que nada nomeia. A retenção
    // apaga PELO registo, logo é daqui que ela sabe o que levar.
    sqlx::query(
        "INSERT INTO recording_artifacts (recording_id, kind, path, is_dir)
         VALUES ($1, 'iso_tracks', 'iso-' || $1::text, TRUE)
         ON CONFLICT (path) DO NOTHING",
    )
    .bind(rec)
    .execute(&app.db)
    .await
    .unwrap();

    let track: Uuid = sqlx::query_scalar(
        "INSERT INTO recording_tracks
           (recording_id, publisher_id, kind, filename, size_bytes, starts_at_ms, source_number, source_label)
         VALUES ($1, $2, 'audio', $3, $4, 250, $5, $6) RETURNING id",
    )
    .bind(rec)
    .bind(Uuid::new_v4())
    .bind(ficheiro)
    .bind(bytes)
    .bind(numero)
    .bind(etiqueta)
    .fetch_one(&app.db)
    .await
    .unwrap();
    (rec, track, caminho)
}

/// O caso do A-02: a pista existe, é listada, e o ficheiro que a rota devolve
/// é um ficheiro de media a sério com UMA fonte.
#[sqlx::test(migrations = "./migrations")]
async fn a_pista_por_fonte_e_servida_e_o_ffprobe_le_uma_fonte(db: sqlx::PgPool) {
    if sem_ffmpeg() {
        return;
    }
    let app = TestApp::spawn(db).await;
    let a = app.new_org("iso-um.ao").await;
    let (rec, track, _) =
        gravacao_com_pista(&app, &a, "02-audio.ogg", Some(2), Some("CAM 2")).await;

    // 1. A listagem traz a pista, com o número e a etiqueta fotografados.
    let (st, body): (u16, Value) = app
        .get(&format!("/api/recordings/{rec}/tracks"), Some(&a.token))
        .await;
    assert_eq!(st, 200, "{body}");
    let itens = body["items"].as_array().expect("items");
    assert_eq!(itens.len(), 1, "{body}");
    assert_eq!(itens[0]["kind"], "audio");
    assert_eq!(itens[0]["source_number"], 2);
    assert_eq!(itens[0]["source_label"], "CAM 2");
    assert_eq!(itens[0]["starts_at_ms"], 250);
    assert!(
        itens[0]["publisher_id"].as_str().is_some(),
        "o publicador fica SEMPRE — é o que torna uma pista sem etiqueta recuperável: {body}"
    );

    // 2. E o ficheiro. É aqui que a prova vive: a listagem podia mentir.
    let (st, bytes) = app
        .raw_bytes(
            reqwest::Method::GET,
            &format!("/api/recordings/{rec}/tracks/{track}/content"),
            &[("authorization", &format!("Bearer {}", a.token))],
        )
        .await;
    assert_eq!(st, 200, "o conteúdo da pista não foi servido");
    assert!(
        bytes.len() > 100,
        "ficheiro vazio ou truncado: {} bytes",
        bytes.len()
    );

    let descarregado =
        std::env::temp_dir().join(format!("iso-descarregado-{}.ogg", Uuid::new_v4()));
    std::fs::write(&descarregado, &bytes).unwrap();
    let f = fluxos(&descarregado);
    std::fs::remove_file(&descarregado).ok();

    // UMA fonte, e é áudio Opus — o requisito «antes de qualquer mistura».
    assert_eq!(f.len(), 1, "a pista ISO tem de ter UMA fonte, veio {f:?}");
    assert_eq!(f[0].1, "audio", "veio {f:?}");
    assert_eq!(f[0].0, "opus", "veio {f:?}");
}

/// O critério 3 do A-02: DUAS fontes dão DOIS ficheiros, cada um com a sua
/// duração. Descarregados pela rota e lidos pelo ffprobe: se a rota servisse o
/// mesmo ficheiro às duas, ou misturasse as fontes, as durações coincidiam ou
/// apareciam dois fluxos num ficheiro.
///
/// O que isto NÃO prova: que uma sessão do SFU produz estes ficheiros. Isso é o
/// `registar_pistas_iso` (testes unitários em `recorder.rs`) e, ponta a ponta,
/// precisa de dois publicadores reais — fica por provar.
#[sqlx::test(migrations = "./migrations")]
async fn duas_fontes_dao_dois_ficheiros_com_duracoes_proprias(db: sqlx::PgPool) {
    if sem_ffmpeg() {
        return;
    }
    let app = TestApp::spawn(db).await;
    let a = app.new_org("iso-duas.ao").await;
    let (rec, pista_1, _) =
        gravacao_com_pista(&app, &a, "01-audio.ogg", Some(1), Some("CAM 1")).await;

    // A segunda fonte, na MESMA gravação: dois segundos, outro tom.
    let segunda = delonix_server::recording_iso_dir(&app.state.config.recordings_dir, rec)
        .join("02-audio.ogg");
    escreve_ogg(&segunda, 2, 880);
    let pista_2: Uuid = sqlx::query_scalar(
        "INSERT INTO recording_tracks
           (recording_id, publisher_id, kind, filename, size_bytes, starts_at_ms, source_number, source_label)
         VALUES ($1, $2, 'audio', '02-audio.ogg', $3, 1500, 2, 'CAM 2') RETURNING id",
    )
    .bind(rec)
    .bind(Uuid::new_v4())
    .bind(std::fs::metadata(&segunda).unwrap().len() as i64)
    .fetch_one(&app.db)
    .await
    .unwrap();

    let (st, lista): (u16, Value) = app
        .get(&format!("/api/recordings/{rec}/tracks"), Some(&a.token))
        .await;
    assert_eq!(st, 200, "{lista}");
    assert_eq!(lista["items"].as_array().map(Vec::len), Some(2), "{lista}");

    let mut duracoes = Vec::new();
    let mut corpos = Vec::new();
    for pista in [pista_1, pista_2] {
        let (st, bytes) = app
            .raw_bytes(
                reqwest::Method::GET,
                &format!("/api/recordings/{rec}/tracks/{pista}/content"),
                &[("authorization", &format!("Bearer {}", a.token))],
            )
            .await;
        assert_eq!(st, 200, "a pista {pista} não foi servida");
        let f = std::env::temp_dir().join(format!("iso-duas-{}.ogg", Uuid::new_v4()));
        std::fs::write(&f, &bytes).unwrap();
        let fl = fluxos(&f);
        let d = duracao(&f);
        std::fs::remove_file(&f).ok();
        assert_eq!(fl.len(), 1, "cada pista tem UMA fonte, veio {fl:?}");
        duracoes.push(d);
        corpos.push(bytes);
    }
    assert_ne!(
        corpos[0], corpos[1],
        "as duas pistas serviram o MESMO ficheiro"
    );
    assert!(
        (duracoes[0] - 1.0).abs() < 0.2 && (duracoes[1] - 2.0).abs() < 0.2,
        "cada fonte tem a sua duração (1 s e 2 s), vieram {duracoes:?}"
    );
}

/// CONTROLO: um id de pista de OUTRA gravação não é servido por aqui. É o que
/// prova que o `recording_id` entra na consulta — sem ele, quem chega a uma
/// gravação servia ficheiros de outra a que não chega.
#[sqlx::test(migrations = "./migrations")]
async fn uma_pista_de_outra_gravacao_nao_e_servida(db: sqlx::PgPool) {
    if sem_ffmpeg() {
        return;
    }
    let app = TestApp::spawn(db).await;
    let a = app.new_org("iso-dois.ao").await;
    let (rec1, _t1, _) = gravacao_com_pista(&app, &a, "00-audio.ogg", Some(1), Some("CAM 1")).await;
    let (_rec2, t2, _) = gravacao_com_pista(&app, &a, "00-audio.ogg", Some(1), Some("CAM 1")).await;

    // A pista `t2` existe, mas não é da gravação `rec1`.
    let (st, _) = app
        .raw_bytes(
            reqwest::Method::GET,
            &format!("/api/recordings/{rec1}/tracks/{t2}/content"),
            &[("authorization", &format!("Bearer {}", a.token))],
        )
        .await;
    assert_eq!(
        st, 404,
        "uma pista de outra gravação foi servida — o recording_id saiu da consulta"
    );
}

/// CONTROLO: quem não chega à gravação não chega às pistas, e leva o 404 de
/// «não existe» — não um 403 que confirmasse que ela existe.
#[sqlx::test(migrations = "./migrations")]
async fn outra_organizacao_nao_ve_as_pistas(db: sqlx::PgPool) {
    if sem_ffmpeg() {
        return;
    }
    let app = TestApp::spawn(db).await;
    let a = app.new_org("iso-tres.ao").await;
    let b = app.new_org("iso-quatro.ao").await;
    let (rec, track, _) = gravacao_com_pista(&app, &a, "00-audio.ogg", None, None).await;

    let (st, _) = app
        .get(&format!("/api/recordings/{rec}/tracks"), Some(&b.token))
        .await;
    assert_eq!(st, 404, "a listagem de pistas de outra org não é 404");

    let (st, _) = app
        .raw_bytes(
            reqwest::Method::GET,
            &format!("/api/recordings/{rec}/tracks/{track}/content"),
            &[("authorization", &format!("Bearer {}", b.token))],
        )
        .await;
    assert_eq!(st, 404, "o ficheiro de outra org não é 404");
}

/// Uma pista sem fonte resolvida não inventa nome: `source_number` e
/// `source_label` vêm nulos, e o `publisher_id` fica. É o estado de uma câmara
/// que se desligou antes de o operador parar a gravação.
#[sqlx::test(migrations = "./migrations")]
async fn uma_pista_sem_fonte_resolvida_nao_inventa_nome(db: sqlx::PgPool) {
    if sem_ffmpeg() {
        return;
    }
    let app = TestApp::spawn(db).await;
    let a = app.new_org("iso-cinco.ao").await;
    let (rec, _t, _) = gravacao_com_pista(&app, &a, "00-audio.ogg", None, None).await;

    let (st, body): (u16, Value) = app
        .get(&format!("/api/recordings/{rec}/tracks"), Some(&a.token))
        .await;
    assert_eq!(st, 200, "{body}");
    let it = &body["items"][0];
    assert!(it["source_number"].is_null(), "{body}");
    assert!(it["source_label"].is_null(), "{body}");
    assert!(it["publisher_id"].as_str().is_some(), "{body}");
}

// ===========================================================================
//  Fuga 2: a pasta das pistas sobrevivia à gravação
// ===========================================================================
//
// A retenção apagava `{id}.webm`, a miniatura e a linha de `recordings` — e o
// `ON DELETE CASCADE` limpava `recording_tracks`, o que fazia a fuga parecer
// fechada. Os FICHEIROS ficavam em `iso-{id}/` indefinidamente, sem nenhum
// registo na base que os nomeasse: o varredor de órfãos só olhava para `tmp-*`,
// e não existe rota de `DELETE` de gravação. A retenção é o ÚNICO caminho que
// apaga, logo era o único sítio onde isto podia sair.

/// Põe a org em retenção de `dias` e envelhece a gravação para lá do prazo.
async fn vence_a_retencao(app: &TestApp, org: &str, rec: Uuid, dias: i32) {
    sqlx::query("UPDATE organizations SET retention_days = $2 WHERE id = $1::uuid")
        .bind(org)
        .bind(dias)
        .execute(&app.db)
        .await
        .unwrap();
    sqlx::query(
        "UPDATE recordings SET created_at = now() - make_interval(days => $2 + 1) WHERE id = $1",
    )
    .bind(rec)
    .bind(dias)
    .execute(&app.db)
    .await
    .unwrap();
}

/// O critério não é o número que a varredura devolve: é a PASTA AUSENTE do
/// disco e o `SELECT count(*)` a zero.
#[sqlx::test(migrations = "./migrations")]
async fn a_retencao_apaga_a_pasta_das_pistas(db: sqlx::PgPool) {
    if sem_ffmpeg() {
        return;
    }
    let app = TestApp::spawn(db).await;
    let a = app.new_org("iso-retencao.ao").await;
    let (rec, _track, caminho) =
        gravacao_com_pista(&app, &a, "00-audio.ogg", Some(1), Some("CAM 1")).await;
    let pasta = caminho.parent().unwrap().to_path_buf();
    // CONTROLO POSITIVO: antes da varredura o ficheiro ESTÁ lá. Sem isto, um
    // teste que mede ausência passa com um caminho errado.
    assert!(caminho.exists(), "o ficheiro não foi semeado em {pasta:?}");

    vence_a_retencao(&app, a.org(), rec, 7).await;
    let apagadas = delonix_server::recordings_retention_sweep(&app.state).await;
    assert_eq!(apagadas, 1, "a retenção não apagou a gravação");

    assert!(
        !pasta.exists(),
        "a pasta das pistas sobreviveu à gravação: {:?}",
        std::fs::read_dir(&pasta)
            .map(|d| d
                .filter_map(|e| e.ok())
                .map(|e| e.file_name())
                .collect::<Vec<_>>())
            .unwrap_or_default()
    );
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM recording_tracks")
        .fetch_one(&app.db)
        .await
        .unwrap();
    assert_eq!(n, 0, "ficaram linhas de pistas de uma gravação apagada");
}

/// CONTROLO POSITIVO da retenção: uma gravação DENTRO do prazo não perde as
/// pistas. Sem este caso, «apagar sempre» passava o teste de cima.
#[sqlx::test(migrations = "./migrations")]
async fn a_retencao_nao_toca_numa_gravacao_dentro_do_prazo(db: sqlx::PgPool) {
    if sem_ffmpeg() {
        return;
    }
    let app = TestApp::spawn(db).await;
    let a = app.new_org("iso-retencao-viva.ao").await;
    let (_rec, _track, caminho) = gravacao_com_pista(&app, &a, "00-audio.ogg", None, None).await;
    sqlx::query("UPDATE organizations SET retention_days = 30 WHERE id = $1::uuid")
        .bind(a.org())
        .execute(&app.db)
        .await
        .unwrap();

    assert_eq!(
        delonix_server::recordings_retention_sweep(&app.state).await,
        0
    );
    assert!(
        caminho.exists(),
        "a retenção levou pistas de uma gravação viva"
    );
}
