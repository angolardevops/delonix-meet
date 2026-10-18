//! O registo dos ficheiros derivados de uma gravação (migração 0126), contra
//! Postgres real e com ffmpeg real.
//!
//! # O que se prova, e porque é que estas três coisas são a mesma
//!
//! Mediu-se a 2026-10-18 (commit `ece92e6a4`) que os ficheiros derivados de uma
//! gravação não estavam registados em sítio nenhum, e que **cada caminho que
//! apaga tinha de se lembrar deles um a um**. Daí saíram duas fugas:
//!
//!  1. a pasta `iso-<gravação>`, que o `registar_pistas_iso` cria renomeando o
//!     `tmp-<sessão>` ANTES de escrever as linhas das pistas — um processo
//!     morto nessa janela deixava-a sem nenhuma linha que a nomeasse, e o
//!     varredor de segmentos só olha para `tmp-*`;
//!  2. os `clip-<id>.webm`/`.vtt`, que o `ON DELETE CASCADE` da 0118 fazia
//!     parecer limpos — as LINHAS saíam com a gravação e os bytes ficavam.
//!
//! A saída é uma só: um registo que todo o produtor escreve **antes** de pôr
//! bytes no disco, e que quem apaga percorre. Uma linha sem ficheiro é
//! inofensiva; um ficheiro sem linha é a fuga.
//!
//! # A observação, e o que ela NÃO é
//!
//! O critério é o ficheiro **ausente do disco** e o `SELECT count(*)` na base —
//! nunca o número que uma varredura devolve, que é a opinião de quem apaga
//! sobre o seu próprio trabalho. Cada prova de ausência traz o controlo
//! positivo de que o ficheiro ESTAVA lá antes: sem ele, um teste que mede
//! ausência passa com um caminho errado.
//!
//! **O que corre onde.** As provas que precisam de um recorte composto saltam
//! sem ffmpeg (o CI desta casa não o instala) — é o padrão do
//! `recording_clips.rs`. A prova da pasta ISO e a do varredor não precisam de
//! media e correm sempre.

mod common;

use common::{Account, TestApp};
use serde_json::{json, Value};
use sqlx::PgPool;
use std::path::{Path as FsPath, PathBuf};
use uuid::Uuid;

fn ffmpeg_bin() -> String {
    std::env::var("FFMPEG_BIN").unwrap_or_else(|_| "ffmpeg".into())
}

fn tem_ffmpeg() -> bool {
    std::process::Command::new(ffmpeg_bin())
        .arg("-version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Um webm com vídeo e áudio, para servir de origem a um recorte.
fn fonte(dst: &FsPath, segundos: u32) {
    let out = std::process::Command::new(ffmpeg_bin())
        .args(["-y", "-loglevel", "error", "-nostdin"])
        .args(["-f", "lavfi", "-i", "testsrc2=size=320x240:rate=15"])
        .args(["-f", "lavfi", "-i", "sine=frequency=440:sample_rate=48000"])
        .args(["-t", &segundos.to_string()])
        .args(["-c:v", "libvpx", "-b:v", "200k", "-deadline", "realtime"])
        .args(["-c:a", "libopus", "-b:a", "48k"])
        .arg(dst)
        .output()
        .expect("ffmpeg não arrancou");
    assert!(
        out.status.success() && dst.exists(),
        "a fonte não se gerou: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// O que está no `recordings_dir`, por nome e ordenado.
///
/// Salta a pasta `exports`: o harness põe o `data_exports_dir` DENTRO do
/// `recordings_dir` (`tests/common/mod.rs`), o que em produção não acontece.
/// É a única excepção, e está aqui em vez de numa lista que cresce sozinha.
fn no_disco(raiz: &FsPath) -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(raiz)
        .map(|d| {
            d.filter_map(|e| e.ok())
                .map(|e| e.file_name().to_string_lossy().to_string())
                .filter(|n| n != "exports")
                .collect()
        })
        .unwrap_or_default();
    v.sort();
    v
}

/// O que o registo diz que existe desta gravação, por nome e ordenado.
async fn no_registo(db: &PgPool, rec: &str) -> Vec<String> {
    let mut v: Vec<String> =
        sqlx::query_scalar("SELECT path FROM recording_artifacts WHERE recording_id = $1::uuid")
            .bind(rec)
            .fetch_all(db)
            .await
            .unwrap();
    v.sort();
    v
}

async fn linhas_no_registo(db: &PgPool) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM recording_artifacts")
        .fetch_one(db)
        .await
        .unwrap()
}

/// Uma sala, uma gravação `ready` com ficheiro real, e os artefactos canónicos
/// registados pela mesma porta que a produção usa.
///
/// A gravação entra pelo **upload**, que é um produtor a sério: escreve a linha
/// de `recordings` e as duas linhas canónicas do registo na MESMA transacção, e
/// só depois põe o ficheiro no disco.
struct Cenario {
    app: TestApp,
    a: Account,
    rec: String,
    raiz: PathBuf,
}

async fn cenario(db: PgPool, segundos: u32) -> Cenario {
    let app = TestApp::spawn(db).await;
    let a = app.new_org("artefactos.test").await;
    let sala = app.new_room(&a, "gravada").await;
    let room_id = sala["id"].as_str().unwrap().to_string();
    let code = sala["code"].as_str().unwrap().to_string();
    // O upload exige ter participado na sala (`is_participant`), e criá-la não
    // conta.
    sqlx::query("INSERT INTO room_participants (room_id, user_id) VALUES ($1::uuid, $2::uuid)")
        .bind(&room_id)
        .bind(&a.user_id)
        .execute(&app.db)
        .await
        .unwrap();
    let raiz = app.state.config.recordings_dir.clone();
    tokio::fs::create_dir_all(&raiz).await.unwrap();

    // O corpo do upload é um webm a sério: é o que faz o `ffprobe` reconhecer
    // a gravação, dar-lhe duração e deixar o recorte ser pedido.
    let origem = std::env::temp_dir().join(format!("fonte-{}.webm", Uuid::new_v4()));
    fonte(&origem, segundos);
    let bytes = std::fs::read(&origem).unwrap();
    std::fs::remove_file(&origem).ok();

    // O upload é um POST com o ficheiro em bruto no corpo: nenhum dos
    // atalhos do `TestApp` manda bytes, por isso vai à mão.
    let res = reqwest::Client::new()
        .post(app.url(&format!("/api/rooms/{code}/recordings")))
        .header("authorization", format!("Bearer {}", a.token))
        .header("content-type", "application/octet-stream")
        .body(bytes)
        .send()
        .await
        .expect("o pedido de upload não saiu");
    let st = res.status().as_u16();
    let texto = res.text().await.unwrap_or_default();
    assert_eq!(st, 200, "o upload falhou: {texto}");
    let body: Value = serde_json::from_str(&texto).expect("o upload não devolveu JSON");
    let rec = body["id"]
        .as_str()
        .unwrap_or_else(|| panic!("o upload não devolveu id: {body}"))
        .to_string();

    Cenario { app, a, rec, raiz }
}

/// Pede um recorte e corre a fila. Devolve o id.
async fn recorta(c: &Cenario, body: Value) -> String {
    let (st, clip) = c
        .app
        .post(
            &format!("/api/recordings/{}/clips", c.rec),
            Some(&c.a.token),
            body,
        )
        .await;
    assert_eq!(st, 202, "pedido de recorte: {clip}");
    let id = clip["id"].as_str().unwrap().to_string();
    let n = delonix_server::recording_clip_render_due(&c.app.state)
        .await
        .expect("a volta da fila falhou");
    assert_eq!(n, 1, "a fila não levou o recorte");
    let (st, clip) = c
        .app
        .get(
            &format!("/api/recordings/{}/clips/{id}", c.rec),
            Some(&c.a.token),
        )
        .await;
    assert_eq!(st, 200, "{clip}");
    assert_eq!(clip["status"], "ready", "o recorte não ficou pronto: {clip}");
    id
}

/// Põe a org em retenção de `dias` e envelhece a gravação para lá do prazo.
async fn vence_a_retencao(app: &TestApp, org: &str, rec: &str, dias: i32) {
    sqlx::query("UPDATE organizations SET retention_days = $2 WHERE id = $1::uuid")
        .bind(org)
        .bind(dias)
        .execute(&app.db)
        .await
        .unwrap();
    sqlx::query(
        "UPDATE recordings SET created_at = now() - make_interval(days => $2 + 1)
          WHERE id = $1::uuid",
    )
    .bind(rec)
    .bind(dias)
    .execute(&app.db)
    .await
    .unwrap();
}

// ===========================================================================
//  1. Nenhum ficheiro no disco sem linha no registo
// ===========================================================================

/// A invariante do registo, medida contra o DISCO e não contra a lista de
/// produtores que eu me lembrei de visitar: percorre-se o `recordings_dir`
/// inteiro e exige-se que **cada entrada** tenha linha.
///
/// É esta a prova que apanha o derivado seguinte. Um produtor novo que escreva
/// um ficheiro e se esqueça de registar faz este teste cair, mesmo que ninguém
/// se lembre de voltar aqui — porque o teste não enumera nomes, varre a pasta.
#[sqlx::test]
async fn nenhum_ficheiro_no_disco_fica_sem_linha_no_registo(db: PgPool) {
    if !tem_ffmpeg() {
        eprintln!("SALTADO: sem ffmpeg não há recorte composto nesta máquina");
        return;
    }
    let c = cenario(db, 6).await;
    // Uma legenda, para o recorte produzir também o `.vtt` — é o quinto
    // produtor, e o que a retenção esquecia.
    let (st, body) = c
        .app
        .put(
            &format!("/api/recordings/{}/captions/pt", c.rec),
            Some(&c.a.token),
            json!({"vtt": "WEBVTT\n\n00:00:02.000 --> 00:00:04.000\ndentro\n"}),
        )
        .await;
    assert!(st < 300, "a legenda não entrou: {st} {body}");
    let clip = recorta(
        &c,
        json!({"kind": "clip", "label": "com legendas",
               "start_ms": 1_000, "end_ms": 5_000, "captions_lang": "pt"}),
    )
    .await;

    let disco = no_disco(&c.raiz);
    // CONTROLO POSITIVO: há de facto ficheiros para medir. Sem isto, um
    // `recordings_dir` vazio — um caminho mal montado no harness — dava verde
    // a um registo que não registava nada.
    assert!(
        disco.len() >= 3,
        "o cenário não produziu ficheiros suficientes para a prova valer: {disco:?}"
    );
    assert!(
        disco.contains(&format!("{}.webm", c.rec)),
        "a gravação composta não está no disco: {disco:?}"
    );
    assert!(
        disco.contains(&format!("clip-{clip}.webm")),
        "o recorte não está no disco: {disco:?}"
    );
    assert!(
        disco.contains(&format!("clip-{clip}.vtt")),
        "o .vtt do recorte não está no disco: {disco:?}"
    );

    // A INVARIANTE: tudo o que está no disco tem linha. A pergunta é feita por
    // caminho, não por tipo — é o que faz este teste não precisar de saber
    // quantos produtores existem.
    let registados: Vec<String> = sqlx::query_scalar("SELECT path FROM recording_artifacts")
        .fetch_all(&c.app.db)
        .await
        .unwrap();
    let sem_linha: Vec<&String> = disco.iter().filter(|f| !registados.contains(f)).collect();
    assert!(
        sem_linha.is_empty(),
        "ficheiros no disco sem linha em recording_artifacts: {sem_linha:?} \
         (registados: {registados:?})"
    );
}

// ===========================================================================
//  2. A pasta ISO que um processo morto deixa
// ===========================================================================

/// O estado EXACTO que um processo morto deixava: a pasta `iso-<gravação>` no
/// disco, a linha do registo escrita (porque ela nasce antes do `rename`), e
/// ZERO linhas em `recording_tracks` — os `INSERT` das pistas nunca correram.
///
/// Antes, nada recolhia isto: o `sweep_orphan_segments` só olha para `tmp-*`,
/// não há rota de `DELETE` de gravação, e a varredura por ausência da linha em
/// `recordings` foi tentada e revertida por três razões medidas. Agora a pasta
/// é recolhida porque há uma LINHA que a nomeia — não por se inferir nada do
/// nome dela nem da sua idade.
///
/// A escrita da linha antes do `rename` é provada onde a função vive
/// (`recorder::tests::a_pasta_das_pistas_fica_registada_antes_de_existir` e o
/// seu par, que corre com a tabela em falta); aqui prova-se a RECOLHA.
#[sqlx::test]
async fn a_pasta_iso_sem_pistas_e_recolhida_pela_retencao(db: PgPool) {
    let app = TestApp::spawn(db).await;
    let a = app.new_org("iso-orfa.test").await;
    let sala = app.new_room(&a, "estúdio").await;
    let room_id = sala["id"].as_str().unwrap();
    let rec: Uuid = sqlx::query_scalar(
        "INSERT INTO recordings (room_id, uploader_id, filename, size_bytes, status)
         VALUES ($1::uuid, $2::uuid, 'composta.webm', 4, 'ready') RETURNING id",
    )
    .bind(room_id)
    .bind(&a.user_id)
    .fetch_one(&app.db)
    .await
    .unwrap();

    // A pasta com pistas dentro, como o `rename` a deixa.
    let pasta = delonix_server::recording_iso_dir(&app.state.config.recordings_dir, rec);
    std::fs::create_dir_all(&pasta).unwrap();
    std::fs::write(pasta.join("00-video.ivf"), b"pista").unwrap();
    // A linha do registo — a única coisa que nomeia a pasta neste estado.
    sqlx::query(
        "INSERT INTO recording_artifacts (recording_id, kind, path, is_dir)
         VALUES ($1, 'iso_tracks', 'iso-' || $1::text, TRUE)",
    )
    .bind(rec)
    .execute(&app.db)
    .await
    .unwrap();
    // E o estado do crash: NENHUMA pista registada.
    let pistas: i64 = sqlx::query_scalar("SELECT count(*) FROM recording_tracks")
        .fetch_one(&app.db)
        .await
        .unwrap();
    assert_eq!(pistas, 0, "o cenário não é o do processo morto");
    // CONTROLO POSITIVO: está lá antes.
    assert!(pasta.exists(), "a pasta não foi semeada em {pasta:?}");

    vence_a_retencao(&app, a.org(), &rec.to_string(), 7).await;
    delonix_server::recordings_retention_sweep(&app.state).await;

    assert!(
        !pasta.exists(),
        "a pasta das pistas sobreviveu à gravação: {:?}",
        no_disco(&app.state.config.recordings_dir)
    );
    assert_eq!(
        linhas_no_registo(&app.db).await,
        0,
        "ficaram linhas no registo de uma gravação apagada"
    );
}

// ===========================================================================
//  3. Os ficheiros do recorte
// ===========================================================================

/// A fuga 2: a retenção apagava a gravação, o `ON DELETE CASCADE` da 0118
/// levava a linha do recorte, e os ficheiros `clip-<id>.webm`/`.vtt` ficavam no
/// disco para sempre. Confirmou-se que nenhum varredor os conhecia:
/// `grep -rn "clip-" server/src/recorder.rs` não dava um único acerto.
#[sqlx::test]
async fn a_retencao_leva_os_ficheiros_do_recorte(db: PgPool) {
    if !tem_ffmpeg() {
        eprintln!("SALTADO: sem ffmpeg não há recorte composto nesta máquina");
        return;
    }
    let c = cenario(db, 6).await;
    let (st, _) = c
        .app
        .put(
            &format!("/api/recordings/{}/captions/pt", c.rec),
            Some(&c.a.token),
            json!({"vtt": "WEBVTT\n\n00:00:02.000 --> 00:00:04.000\ndentro\n"}),
        )
        .await;
    assert!(st < 300);
    let clip = recorta(
        &c,
        json!({"kind": "clip", "label": "recorte",
               "start_ms": 1_000, "end_ms": 5_000, "captions_lang": "pt"}),
    )
    .await;

    let webm = c.raiz.join(format!("clip-{clip}.webm"));
    let vtt = c.raiz.join(format!("clip-{clip}.vtt"));
    let composta = c.raiz.join(format!("{}.webm", c.rec));
    // CONTROLO POSITIVO: os três ficheiros ESTÃO lá antes da retenção.
    for f in [&webm, &vtt, &composta] {
        assert!(f.exists(), "não foi semeado: {f:?}");
    }

    vence_a_retencao(&c.app, c.a.org(), &c.rec, 7).await;
    let apagadas = delonix_server::recordings_retention_sweep(&c.app.state).await;
    assert_eq!(apagadas, 1, "a retenção não apagou a gravação");

    for f in [&webm, &vtt, &composta] {
        assert!(
            !f.exists(),
            "o ficheiro sobreviveu à gravação: {f:?} (no disco: {:?})",
            no_disco(&c.raiz)
        );
    }
    assert_eq!(
        linhas_no_registo(&c.app.db).await,
        0,
        "ficaram linhas no registo"
    );
    let clips: i64 = sqlx::query_scalar("SELECT count(*) FROM recording_clips")
        .fetch_one(&c.app.db)
        .await
        .unwrap();
    assert_eq!(clips, 0, "o CASCADE da 0118 não levou a linha do recorte");
}

/// O caminho que NINGUÉM mandou apagar, e a razão de o registo não ter chave
/// estrangeira: a linha da gravação sai por uma cascata (`rooms`, `users`) e
/// leva com ela a do recorte. Com `ON DELETE CASCADE` no registo, estas linhas
/// morriam no mesmo instante em que passam a ser a única coisa que sabe que os
/// ficheiros existem — a fuga 2 uma camada acima.
#[sqlx::test]
async fn os_ficheiros_de_uma_gravacao_que_a_cascata_levou_sao_recolhidos(db: PgPool) {
    if !tem_ffmpeg() {
        eprintln!("SALTADO: sem ffmpeg não há recorte composto nesta máquina");
        return;
    }
    let c = cenario(db, 6).await;
    let clip = recorta(
        &c,
        json!({"kind": "clip", "label": "recorte", "start_ms": 1_000, "end_ms": 5_000}),
    )
    .await;
    let webm = c.raiz.join(format!("clip-{clip}.webm"));
    let composta = c.raiz.join(format!("{}.webm", c.rec));
    assert!(webm.exists() && composta.exists(), "não foram semeados");

    // A cascata: a linha da gravação sai sem passar pela retenção.
    sqlx::query("DELETE FROM recordings WHERE id = $1::uuid")
        .bind(&c.rec)
        .execute(&c.app.db)
        .await
        .unwrap();
    // As linhas do registo SOBREVIVEM — é isso que as torna úteis aqui.
    assert!(
        !no_registo(&c.app.db, &c.rec).await.is_empty(),
        "o registo foi levado pela cascata: já não há nada que nomeie os ficheiros"
    );

    let n = delonix_server::recording_artifacts_sweep_orphans(&c.app.state).await;
    assert!(n >= 2, "o varredor não recolheu os artefactos órfãos (n = {n})");
    for f in [&webm, &composta] {
        assert!(
            !f.exists(),
            "o ficheiro de uma gravação que já não existe ficou: {f:?}"
        );
    }
    assert_eq!(linhas_no_registo(&c.app.db).await, 0, "ficaram linhas");
}

/// CONTROLO POSITIVO do varredor, e o caso que o apanha como FALSO POSITIVO:
/// uma gravação VIVA não perde os ficheiros. Sem esta prova, «apagar tudo o que
/// o registo nomeia» passava o teste de cima — foi exactamente assim que uma
/// versão anterior desta frente autorizava `remove_dir_all` em todas as pastas
/// do volume.
#[sqlx::test]
async fn o_varredor_nao_toca_nos_ficheiros_de_uma_gravacao_viva(db: PgPool) {
    let app = TestApp::spawn(db).await;
    let a = app.new_org("viva.test").await;
    let sala = app.new_room(&a, "gravada").await;
    let room_id = sala["id"].as_str().unwrap().to_string();
    let raiz = app.state.config.recordings_dir.clone();
    std::fs::create_dir_all(&raiz).unwrap();

    // Duas gravações com um ficheiro e uma linha cada. Uma delas vai morrer.
    let mut ficheiros = Vec::new();
    for _ in 0..2 {
        let id: Uuid = sqlx::query_scalar(
            "INSERT INTO recordings (room_id, uploader_id, filename, size_bytes, status)
             VALUES ($1::uuid, $2::uuid, 'c.webm', 4, 'ready') RETURNING id",
        )
        .bind(&room_id)
        .bind(&a.user_id)
        .fetch_one(&app.db)
        .await
        .unwrap();
        let f = raiz.join(format!("{id}.webm"));
        std::fs::write(&f, b"bytes").unwrap();
        sqlx::query(
            "INSERT INTO recording_artifacts (recording_id, kind, path, is_dir)
             VALUES ($1, 'recording', $1::text || '.webm', FALSE)",
        )
        .bind(id)
        .execute(&app.db)
        .await
        .unwrap();
        ficheiros.push((id, f));
    }
    let (morta, f_morta) = ficheiros.remove(0);
    let (_viva, f_viva) = ficheiros.remove(0);
    sqlx::query("DELETE FROM recordings WHERE id = $1")
        .bind(morta)
        .execute(&app.db)
        .await
        .unwrap();

    let n = delonix_server::recording_artifacts_sweep_orphans(&app.state).await;
    assert_eq!(n, 1, "o varredor apagou um número errado de artefactos");
    assert!(!f_morta.exists(), "o artefacto órfão ficou: {f_morta:?}");
    assert!(
        f_viva.exists(),
        "o varredor levou o ficheiro de uma gravação VIVA: {f_viva:?}"
    );
    assert_eq!(
        linhas_no_registo(&app.db).await,
        1,
        "a linha da gravação viva saiu do registo"
    );
}

// ===========================================================================
//  4. Os três defeitos que a revisão do commit encontrou
// ===========================================================================

/// **A linha só morre depois do ficheiro.** Se o `unlink` não correr, a linha
/// FICA — e o contador sobe.
///
/// Era o defeito da primeira versão, em três sítios: um
/// `let _ = remove_file(...)` que descartava o erro, seguido de um `forget`
/// incondicional que apagava a linha. Num volume em leitura, ou com `EACCES`,
/// o ficheiro ficava no disco e a ÚNICA linha que o nomeava desaparecia: a fuga
/// que este módulo fecha, reaberta no caminho de erro e sem contador que a
/// denunciasse.
///
/// Como se provoca: no Linux, apagar um ficheiro exige escrita no DIRECTÓRIO.
/// Tira-se a escrita à raiz e o `unlink` leva `EACCES` — nenhuma simulação,
/// o erro é o do sistema.
#[sqlx::test]
async fn um_unlink_recusado_nao_apaga_a_linha(db: PgPool) {
    use std::os::unix::fs::PermissionsExt;

    let app = TestApp::spawn(db).await;
    let a = app.new_org("unlink-recusado.test").await;
    let sala = app.new_room(&a, "gravada").await;
    let room_id = sala["id"].as_str().unwrap().to_string();
    let raiz = app.state.config.recordings_dir.clone();
    std::fs::create_dir_all(&raiz).unwrap();

    let rec: Uuid = sqlx::query_scalar(
        "INSERT INTO recordings (room_id, uploader_id, filename, size_bytes, status)
         VALUES ($1::uuid, $2::uuid, 'c.webm', 4, 'ready') RETURNING id",
    )
    .bind(&room_id)
    .bind(&a.user_id)
    .fetch_one(&app.db)
    .await
    .unwrap();
    let ficheiro = raiz.join(format!("{rec}.webm"));
    std::fs::write(&ficheiro, b"bytes").unwrap();
    sqlx::query(
        "INSERT INTO recording_artifacts (recording_id, kind, path, is_dir)
         VALUES ($1, 'recording', $1::text || '.webm', FALSE)",
    )
    .bind(rec)
    .execute(&app.db)
    .await
    .unwrap();
    // A gravação sai da tabela: é o varredor de órfãos que vai tentar apagar.
    sqlx::query("DELETE FROM recordings WHERE id = $1")
        .bind(rec)
        .execute(&app.db)
        .await
        .unwrap();

    // Raiz sem escrita: o `unlink` passa a ser recusado pelo sistema.
    let antes = std::fs::metadata(&raiz).unwrap().permissions();
    std::fs::set_permissions(&raiz, std::fs::Permissions::from_mode(0o555)).unwrap();
    let apagados = delonix_server::recording_artifacts_sweep_orphans(&app.state).await;
    std::fs::set_permissions(&raiz, antes).unwrap();

    assert_eq!(apagados, 0, "o varredor disse ter apagado o que não apagou");
    // CONTROLO POSITIVO do cenário: o ficheiro de facto não saiu. Sem isto,
    // uma raiz onde o `chmod` não pega (root, um tmpfs permissivo) daria verde
    // a provar coisa nenhuma.
    assert!(
        ficheiro.exists(),
        "o cenário não se provocou: o ficheiro saiu apesar da raiz sem escrita"
    );
    assert_eq!(
        linhas_no_registo(&app.db).await,
        1,
        "a linha do ficheiro que NÃO saiu foi apagada — nada volta a encontrá-lo"
    );
    // E é alertável: um contador, não um aviso perdido no log.
    let m = app.state.metrics.render(1);
    assert!(
        m.lines()
            .any(|l| l.starts_with("delonix_recording_artifacts_stuck_total ")
                && !l.ends_with(" 0")),
        "o contador de artefactos presos não subiu:\n{m}"
    );
}

/// **A retenção falha FECHADO.** Se o registo não responde, a gravação NÃO é
/// apagada: sem a linha de `recordings` a retenção nunca mais volta a esta
/// gravação, e o que ficou no disco passaria a depender só do varredor.
///
/// A primeira versão documentava este contrato e não o cumpria — o `purge`
/// devolvia `u64`, o valor era descartado e o `DELETE FROM recordings` corria
/// sempre.
#[sqlx::test]
async fn sem_registo_a_retencao_nao_apaga_a_gravacao(db: PgPool) {
    let app = TestApp::spawn(db).await;
    let a = app.new_org("retencao-fechada.test").await;
    let sala = app.new_room(&a, "gravada").await;
    let room_id = sala["id"].as_str().unwrap().to_string();
    let rec: Uuid = sqlx::query_scalar(
        "INSERT INTO recordings (room_id, uploader_id, filename, size_bytes, status)
         VALUES ($1::uuid, $2::uuid, 'c.webm', 4, 'ready') RETURNING id",
    )
    .bind(&room_id)
    .bind(&a.user_id)
    .fetch_one(&app.db)
    .await
    .unwrap();
    vence_a_retencao(&app, a.org(), &rec.to_string(), 7).await;

    // A base deixa de poder responder sobre artefactos. É a única maneira de
    // provocar um erro de leitura contra Postgres real.
    sqlx::query("DROP TABLE recording_artifacts")
        .execute(&app.db)
        .await
        .unwrap();

    let apagadas = delonix_server::recordings_retention_sweep(&app.state).await;
    assert_eq!(apagadas, 0, "a retenção contou uma gravação que não apagou");
    let ainda: i64 = sqlx::query_scalar("SELECT count(*) FROM recordings WHERE id = $1")
        .bind(rec)
        .fetch_one(&app.db)
        .await
        .unwrap();
    assert_eq!(
        ainda, 1,
        "a gravação foi apagada sem se saber o que ela tinha no disco — a retenção nunca mais cá volta"
    );
}

/// **Um `is_dir` errado na base não deixa nada no disco.** O valor é escrito a
/// partir do `kind` (`is_dir_of`), mas uma linha posta à mão — ou um `kind` que
/// essa função ainda não conheça — marcaria a pasta das pistas como ficheiro, e
/// quem recolhe ficava a chamar `remove_file` num directório para sempre:
/// `EISDIR` a cada tique, o contador a subir e o disco a não libertar.
#[sqlx::test]
async fn uma_pasta_registada_como_ficheiro_ainda_e_apagada(db: PgPool) {
    let app = TestApp::spawn(db).await;
    let a = app.new_org("is-dir-errado.test").await;
    let sala = app.new_room(&a, "estúdio").await;
    let room_id = sala["id"].as_str().unwrap();
    let rec: Uuid = sqlx::query_scalar(
        "INSERT INTO recordings (room_id, uploader_id, filename, size_bytes, status)
         VALUES ($1::uuid, $2::uuid, 'c.webm', 4, 'ready') RETURNING id",
    )
    .bind(room_id)
    .bind(&a.user_id)
    .fetch_one(&app.db)
    .await
    .unwrap();
    let pasta = delonix_server::recording_iso_dir(&app.state.config.recordings_dir, rec);
    std::fs::create_dir_all(&pasta).unwrap();
    std::fs::write(pasta.join("00-video.ivf"), b"pista").unwrap();
    // A linha MENTE: diz que é um ficheiro.
    sqlx::query(
        "INSERT INTO recording_artifacts (recording_id, kind, path, is_dir)
         VALUES ($1, 'iso_tracks', 'iso-' || $1::text, FALSE)",
    )
    .bind(rec)
    .execute(&app.db)
    .await
    .unwrap();
    assert!(pasta.exists(), "a pasta não foi semeada");

    vence_a_retencao(&app, a.org(), &rec.to_string(), 7).await;
    delonix_server::recordings_retention_sweep(&app.state).await;

    assert!(
        !pasta.exists(),
        "a pasta ficou no disco porque a linha dizia que era um ficheiro: {:?}",
        no_disco(&app.state.config.recordings_dir)
    );
    assert_eq!(linhas_no_registo(&app.db).await, 0, "ficaram linhas");
}

/// Um ficheiro que o registo NÃO conhece nunca é tocado. É a diferença entre
/// varrer um registo e varrer o disco, e é a razão pela qual este varredor é
/// seguro onde a varredura por nome de pasta não era: uma base restaurada para
/// um ponto anterior, ou outro ambiente apontado ao mesmo volume
/// `ReadWriteMany`, não tem nas SUAS linhas os ficheiros do outro — logo não
/// lhes chega.
#[sqlx::test]
async fn o_que_o_registo_nao_conhece_nao_e_apagado(db: PgPool) {
    let app = TestApp::spawn(db).await;
    let raiz = app.state.config.recordings_dir.clone();
    std::fs::create_dir_all(&raiz).unwrap();
    // Ficheiros e uma pasta de um ambiente que partilha o volume: nomes com a
    // forma exacta dos nossos, e nenhuma linha nesta base.
    let outro = Uuid::new_v4();
    let alheios = [raiz.join(format!("{outro}.webm")), raiz.join("clip-x.webm")];
    for f in &alheios {
        std::fs::write(f, b"de outro ambiente").unwrap();
    }
    let pasta_alheia = delonix_server::recording_iso_dir(&raiz, outro);
    std::fs::create_dir_all(&pasta_alheia).unwrap();
    std::fs::write(pasta_alheia.join("00-video.ivf"), b"pista").unwrap();

    assert_eq!(linhas_no_registo(&app.db).await, 0, "o registo não está vazio");
    assert_eq!(
        delonix_server::recording_artifacts_sweep_orphans(&app.state).await,
        0,
        "o varredor apagou algo que o registo não nomeia"
    );
    for f in &alheios {
        assert!(f.exists(), "um ficheiro sem linha foi apagado: {f:?}");
    }
    assert!(
        pasta_alheia.exists(),
        "uma pasta sem linha foi apagada: {pasta_alheia:?}"
    );
}
