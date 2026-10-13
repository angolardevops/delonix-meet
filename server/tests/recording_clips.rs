//! Recortes e *shorts* contra Postgres, servidor e **ffmpeg reais**.
//!
//! O que se prova:
//!
//! 1. **o corte é exacto** — a duração do recorte mede-se com `ffprobe` sobre
//!    as PISTAS, não pela duração do contentor (a lição da R295: um webm diz a
//!    duração que quer);
//! 2. **o short é 9:16** — e a dimensão vem medida do ficheiro, não do pedido;
//! 3. **as legendas saem ao lado**, com as deixas da janela a começar em zero
//!    (não se queima nada na imagem: o ffmpeg desta imagem não tem libass);
//! 4. **o portão é o da gravação** — quem só vê não recorta, e uma gravação de
//!    outra organização não existe;
//! 5. **o ficheiro só se descarrega quando está pronto** (`clip.not_ready`);
//! 6. **a fila fecha com a posse certa** — uma reivindicação velha não fecha um
//!    render que já é de outra.
//!
//! **O que corre onde.** As quatro provas que COMPÕEM (1, 2, 3 e a de apagar)
//! precisam de ffmpeg e saltam sem ele — é o padrão do `recorder.rs`, e o CI
//! desta casa não instala ffmpeg. As outras quatro (portões, contrato do
//! pedido, descarregar antes de estar pronto, posse da fila) correm sempre:
//! a origem é um ficheiro de fachada, que é tudo o que elas precisam.
//! Consequência honesta: **o render só está provado na máquina de quem
//! desenvolve**, com ffmpeg a sério, não no CI.
mod common;

use common::{Account, TestApp};
use serde_json::{json, Value};
use sqlx::PgPool;
use std::path::{Path as FsPath, PathBuf};

fn ffmpeg_bin() -> String {
    std::env::var("FFMPEG_BIN").unwrap_or_else(|_| "ffmpeg".into())
}

fn ffprobe_bin() -> String {
    std::env::var("FFPROBE_BIN").unwrap_or_else(|_| "ffprobe".into())
}

/// `None` quando a máquina não tem ffmpeg — a prova salta em vez de falhar.
fn tem_ffmpeg() -> bool {
    std::process::Command::new(ffmpeg_bin())
        .arg("-version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// A origem dos recortes: um webm de `segundos` com vídeo e áudio.
///
/// **Sem ffmpeg na máquina escreve um ficheiro de fachada** e devolve `false`.
/// É o que permite às provas de PORTÃO e de contrato (que nunca compõem nada)
/// correr no CI, onde não há ffmpeg: elas só precisam que a gravação tenha
/// ficheiro para o `require_file()` passar. As que compõem de verdade saltam
/// pelo seu próprio `tem_ffmpeg()`.
fn fonte(dst: &FsPath, segundos: u32, largura: u32, altura: u32) -> bool {
    if !tem_ffmpeg() {
        std::fs::write(dst, b"nao-e-um-webm").expect("escrever a fachada");
        return false;
    }
    let out = std::process::Command::new(ffmpeg_bin())
        .args(["-y", "-loglevel", "error", "-nostdin"])
        .args([
            "-f",
            "lavfi",
            "-i",
            &format!("testsrc2=size={largura}x{altura}:rate=15"),
        ])
        .args(["-f", "lavfi", "-i", "sine=frequency=440:sample_rate=48000"])
        .args(["-t", &segundos.to_string()])
        .args(["-c:v", "libvpx", "-b:v", "300k", "-deadline", "realtime"])
        .args(["-c:a", "libopus", "-b:a", "64k"])
        .arg(dst)
        .output()
        .expect("ffmpeg");
    assert!(
        out.status.success(),
        "a fonte não se gerou: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    true
}

/// Duração das PISTAS (ms), medida pacote a pacote — não a do contentor.
///
/// É a lição da R295: um webm escrito «ao vivo» mente na duração do cabeçalho,
/// e medir por lá dá verde a um recorte vazio.
fn duracao_das_pistas_ms(path: &FsPath) -> i64 {
    let out = std::process::Command::new(ffprobe_bin())
        .args([
            "-v",
            "error",
            "-select_streams",
            "v:0",
            "-count_packets",
            "-show_entries",
            "stream=nb_read_packets,r_frame_rate",
            "-of",
            "json",
        ])
        .arg(path)
        .output()
        .expect("ffprobe");
    let v: Value = serde_json::from_slice(&out.stdout).unwrap_or(Value::Null);
    let s = &v["streams"][0];
    let pacotes = s["nb_read_packets"]
        .as_str()
        .and_then(|x| x.parse::<f64>().ok())
        .unwrap_or(0.0);
    let (num, den) = s["r_frame_rate"]
        .as_str()
        .and_then(|r| r.split_once('/'))
        .and_then(|(a, b)| Some((a.parse::<f64>().ok()?, b.parse::<f64>().ok()?)))
        .unwrap_or((15.0, 1.0));
    let fps = if den > 0.0 { num / den } else { 15.0 };
    ((pacotes / fps) * 1000.0) as i64
}

/// `(largura, altura)` do vídeo.
fn dimensao(path: &FsPath) -> (i64, i64) {
    let out = std::process::Command::new(ffprobe_bin())
        .args([
            "-v",
            "error",
            "-select_streams",
            "v:0",
            "-show_entries",
            "stream=width,height",
            "-of",
            "json",
        ])
        .arg(path)
        .output()
        .expect("ffprobe");
    let v: Value = serde_json::from_slice(&out.stdout).unwrap_or(Value::Null);
    (
        v["streams"][0]["width"].as_i64().unwrap_or(0),
        v["streams"][0]["height"].as_i64().unwrap_or(0),
    )
}

struct Cenario {
    app: TestApp,
    /// Admin da org A e dono da gravação.
    a: Account,
    /// Membro da A que só participou (vê, não gere).
    carla: Account,
    /// Admin de outra organização.
    b: Account,
    rec: String,
    dir: PathBuf,
}

async fn cenario(db: PgPool, segundos: u32, largura: u32, altura: u32) -> Cenario {
    cenario_com(db, segundos, largura, altura, &[]).await
}

async fn cenario_com(
    db: PgPool,
    segundos: u32,
    largura: u32,
    altura: u32,
    extra: &[(&str, &str)],
) -> Cenario {
    let app = TestApp::spawn_with(db, extra).await;
    let a = app.new_org("alfa-clip.test").await;
    let b = app.new_org("beta-clip.test").await;
    let carla = app.add_member(&a, "carla", "member").await;
    let room = app.new_room(&a, "gravada").await;
    let room_id = room["id"].as_str().unwrap().to_string();
    let rec = app.insert_recording(&room_id, &a.user_id).await;
    sqlx::query("INSERT INTO room_participants (room_id, user_id) VALUES ($1::uuid, $2::uuid)")
        .bind(&room_id)
        .bind(&carla.user_id)
        .execute(&app.db)
        .await
        .unwrap();

    let dir = app.state.config.recordings_dir.clone();
    tokio::fs::create_dir_all(&dir).await.unwrap();
    let src = dir.join(format!("{rec}.webm"));
    let _real = fonte(&src, segundos, largura, altura);
    let tamanho = std::fs::metadata(&src).unwrap().len() as i64;
    sqlx::query(
        "UPDATE recordings SET status = 'ready', size_bytes = $2, duration_ms = $3,
                width = $4, height = $5 WHERE id = $1::uuid",
    )
    .bind(&rec)
    .bind(tamanho)
    .bind(segundos as i64 * 1000)
    .bind(largura as i32)
    .bind(altura as i32)
    .execute(&app.db)
    .await
    .unwrap();

    Cenario {
        app,
        a,
        carla,
        b,
        rec,
        dir,
    }
}

/// Pede um recorte e corre a fila até ele sair da fila. Devolve o recorte.
async fn recorta(c: &Cenario, body: Value) -> Value {
    let (st, clip) = c
        .app
        .post(
            &format!("/api/recordings/{}/clips", c.rec),
            Some(&c.a.token),
            body,
        )
        .await;
    assert_eq!(st, 202, "pedido de recorte: {clip}");
    assert_eq!(clip["status"], "queued", "{clip}");
    let id = clip["id"].as_str().unwrap().to_string();

    // A fila corre aqui, no teste: o `run()` não é levantado.
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
    clip
}

// ---------------------------------------------------------------------------

#[sqlx::test]
async fn o_corte_e_exacto_e_a_duracao_mede_se_nas_pistas(db: PgPool) {
    if !tem_ffmpeg() {
        eprintln!("sem ffmpeg: prova saltada");
        return;
    }
    let c = cenario(db, 20, 640, 360).await;
    let clip = recorta(
        &c,
        json!({"kind": "clip", "label": "Troço do Kilamba",
               "start_ms": 5_000, "end_ms": 13_000}),
    )
    .await;
    assert_eq!(clip["status"], "ready", "{clip}");
    assert_eq!(clip["progress"], 1.0, "{clip}");
    assert!(clip["size_bytes"].as_i64().unwrap_or(0) > 0, "{clip}");
    assert!(clip["error_code"].is_null(), "{clip}");

    let path = c
        .dir
        .join(format!("clip-{}.webm", clip["id"].as_str().unwrap()));
    assert!(path.exists(), "o ficheiro não saiu: {path:?}");
    let medido = duracao_das_pistas_ms(&path);
    // Oito segundos pedidos. A tolerância é de um quadro e meio a 15 fps.
    assert!(
        (medido - 8_000).abs() < 150,
        "pedi 8000 ms e as pistas têm {medido} ms"
    );
    // O recorte não ficou do tamanho da gravação inteira — é o defeito que um
    // `-to` mal usado produz, e passava se se medisse só «existe ficheiro».
    assert!(
        medido < 15_000,
        "o recorte saiu com a gravação toda: {medido} ms"
    );
}

#[sqlx::test]
async fn o_short_sai_vertical_e_a_dimensao_vem_do_ficheiro(db: PgPool) {
    if !tem_ffmpeg() {
        eprintln!("sem ffmpeg: prova saltada");
        return;
    }
    let c = cenario(db, 12, 640, 360).await;
    let clip = recorta(
        &c,
        json!({"kind": "short", "label": "vertical",
               "start_ms": 1_000, "end_ms": 5_000, "focus": "left"}),
    )
    .await;
    assert_eq!(clip["status"], "ready", "{clip}");
    let path = c
        .dir
        .join(format!("clip-{}.webm", clip["id"].as_str().unwrap()));
    let (w, h) = dimensao(&path);
    assert_eq!(h, 360, "a altura da origem fica inteira");
    assert_eq!(w, 202, "640×360 → corte 9:16 de 202×360");
    // 9:16 a menos de um pixel par.
    assert!(
        ((w as f64) / (h as f64) - 9.0 / 16.0).abs() < 0.01,
        "{w}×{h} não é 9:16"
    );
}

#[sqlx::test]
async fn as_legendas_saem_ao_lado_com_os_tempos_a_comecar_em_zero(db: PgPool) {
    if !tem_ffmpeg() {
        eprintln!("sem ffmpeg: prova saltada");
        return;
    }
    let c = cenario(db, 20, 640, 360).await;
    // Uma legenda da gravação, com deixas dentro e fora da janela do recorte.
    let vtt = "WEBVTT\n\n00:00:02.000 --> 00:00:04.000\nantes\n\n\
               00:00:06.000 --> 00:00:08.000\ndentro\n\n\
               00:00:16.000 --> 00:00:18.000\ndepois\n";
    let (st, body) = c
        .app
        .put(
            &format!("/api/recordings/{}/captions/pt", c.rec),
            Some(&c.a.token),
            json!({"vtt": vtt}),
        )
        .await;
    assert!(st < 300, "a legenda não entrou: {st} {body}");

    let clip = recorta(
        &c,
        json!({"kind": "clip", "label": "com legendas",
               "start_ms": 5_000, "end_ms": 10_000, "captions_lang": "pt"}),
    )
    .await;
    assert_eq!(clip["status"], "ready", "{clip}");
    let id = clip["id"].as_str().unwrap();

    let res = c
        .app
        .raw(
            reqwest::Method::GET,
            &format!("/api/recordings/{}/clips/{id}/captions.vtt", c.rec),
            &[("authorization", &format!("Bearer {}", c.a.token))],
            None,
        )
        .await;
    assert_eq!(res.status, 200, "{}", res.text);
    assert!(res.text.starts_with("WEBVTT"), "{}", res.text);
    assert!(res.text.contains("dentro"), "{}", res.text);
    assert!(!res.text.contains("antes"), "fora da janela: {}", res.text);
    assert!(!res.text.contains("depois"), "fora da janela: {}", res.text);
    // A deixa dos 6 s da gravação cai a 1 s do recorte (que começa aos 5 s).
    assert!(
        res.text.contains("00:00:01.000 --> 00:00:03.000"),
        "os tempos não recomeçaram em zero: {}",
        res.text
    );
}

#[sqlx::test]
async fn o_portao_e_o_da_gravacao(db: PgPool) {
    let c = cenario(db, 4, 320, 180).await;
    let pedido = json!({"kind": "clip", "label": "x", "start_ms": 0, "end_ms": 2_000});

    // Quem só participou VÊ a gravação mas não a gere: 403 (já sabe que existe).
    let (st, body) = c
        .app
        .post(
            &format!("/api/recordings/{}/clips", c.rec),
            Some(&c.carla.token),
            pedido.clone(),
        )
        .await;
    assert_eq!(st, 403, "{body}");

    // De outra organização, a gravação não existe: 404, igual a um id inventado.
    let (st_alheia, _) = c
        .app
        .post(
            &format!("/api/recordings/{}/clips", c.rec),
            Some(&c.b.token),
            pedido.clone(),
        )
        .await;
    let (st_fantasma, _) = c
        .app
        .post(
            &format!("/api/recordings/{}/clips", common::INVENTED_ID),
            Some(&c.b.token),
            pedido.clone(),
        )
        .await;
    assert_eq!(st_alheia, 404);
    assert_eq!(
        st_alheia, st_fantasma,
        "uma gravação de outra org tem de dar a mesma resposta que uma inexistente"
    );

    // E listar os recortes de uma gravação alheia também não existe.
    let (st, _) = c
        .app
        .get(
            &format!("/api/recordings/{}/clips", c.rec),
            Some(&c.b.token),
        )
        .await;
    assert_eq!(st, 404);
}

#[sqlx::test]
async fn o_pedido_sem_sentido_e_recusado_com_o_codigo_certo(db: PgPool) {
    let c = cenario(db, 10, 320, 180).await;
    let casos = [
        (
            json!({"kind": "clip", "label": "x", "start_ms": 0, "end_ms": 500}),
            "clip.invalid_range",
        ),
        (
            json!({"kind": "clip", "label": "x", "start_ms": 2_000, "end_ms": 1_000}),
            "clip.invalid_range",
        ),
        (
            json!({"kind": "clip", "label": "", "start_ms": 0, "end_ms": 2_000}),
            "clip.invalid_label",
        ),
        (
            json!({"kind": "filme", "label": "x", "start_ms": 0, "end_ms": 2_000}),
            "clip.unknown_kind",
        ),
        (
            json!({"kind": "short", "label": "x", "start_ms": 0, "end_ms": 2_000, "focus": "cima"}),
            "clip.unknown_focus",
        ),
        // Um short não passa de três minutos.
        (
            json!({"kind": "short", "label": "x", "start_ms": 0, "end_ms": 200_000}),
            "clip.too_long",
        ),
        // Começar depois do fim da gravação não dá recorte nenhum.
        (
            json!({"kind": "clip", "label": "x", "start_ms": 60_000, "end_ms": 65_000}),
            "clip.invalid_range",
        ),
    ];
    for (body, codigo) in casos {
        let (st, resp) = c
            .app
            .post(
                &format!("/api/recordings/{}/clips", c.rec),
                Some(&c.a.token),
                body.clone(),
            )
            .await;
        assert_eq!(st, 400, "{body} → {resp}");
        assert_eq!(resp["code"], codigo, "{body} → {resp}");
    }
}

#[sqlx::test]
async fn o_ficheiro_so_se_descarrega_quando_esta_pronto(db: PgPool) {
    let c = cenario(db, 6, 320, 180).await;
    let (st, clip) = c
        .app
        .post(
            &format!("/api/recordings/{}/clips", c.rec),
            Some(&c.a.token),
            json!({"kind": "clip", "label": "x", "start_ms": 0, "end_ms": 2_000}),
        )
        .await;
    assert_eq!(st, 202, "{clip}");
    let id = clip["id"].as_str().unwrap();

    // Ainda na fila.
    let (st, body) = c
        .app
        .get(
            &format!("/api/recordings/{}/clips/{id}/content", c.rec),
            Some(&c.a.token),
        )
        .await;
    assert_eq!(st, 422, "{body}");
    assert_eq!(body["code"], "clip.not_ready", "{body}");

    // Falhado: outro código, e outra frase.
    sqlx::query("UPDATE recording_clips SET status = 'failed', error_code = 'clip.ffmpeg_failed' WHERE id = $1::uuid")
        .bind(id)
        .execute(&c.app.db)
        .await
        .unwrap();
    let (st, body) = c
        .app
        .get(
            &format!("/api/recordings/{}/clips/{id}/content", c.rec),
            Some(&c.a.token),
        )
        .await;
    assert_eq!(st, 422, "{body}");
    assert_eq!(body["code"], "clip.failed", "{body}");

    // E as legendas de um recorte que não as pediu não existem.
    let (st, _) = c
        .app
        .get(
            &format!("/api/recordings/{}/clips/{id}/captions.vtt", c.rec),
            Some(&c.a.token),
        )
        .await;
    assert_eq!(st, 404);
}

#[sqlx::test]
async fn uma_reivindicacao_velha_nao_fecha_um_render_de_outra(db: PgPool) {
    let c = cenario(db, 6, 320, 180).await;
    let (st, clip) = c
        .app
        .post(
            &format!("/api/recordings/{}/clips", c.rec),
            Some(&c.a.token),
            json!({"kind": "clip", "label": "x", "start_ms": 0, "end_ms": 2_000}),
        )
        .await;
    assert_eq!(st, 202, "{clip}");
    let id = clip["id"].as_str().unwrap().to_string();

    // Simula um render em curso com posse FRESCA (outro nó a compor agora): a
    // MARCA e o PRAZO, que são duas colunas de propósito.
    sqlx::query(
        "UPDATE recording_clips
            SET status = 'processing', running_at = now(),
                lease_expires_at = now() + interval '30 minutes'
          WHERE id = $1::uuid",
    )
    .bind(&id)
    .execute(&c.app.db)
    .await
    .unwrap();
    let n = delonix_server::recording_clip_render_due(&c.app.state)
        .await
        .unwrap();
    assert_eq!(n, 0, "a fila levou um recorte com posse viva");

    // Com o PRAZO vencido, volta à fila — é o que recupera um pod que morreu.
    // A marca não muda: é ela que a escrita final do outro nó já não vai
    // reconhecer.
    sqlx::query(
        "UPDATE recording_clips SET lease_expires_at = now() - interval '1 minute'
          WHERE id = $1::uuid",
    )
    .bind(&id)
    .execute(&c.app.db)
    .await
    .unwrap();
    let n = delonix_server::recording_clip_render_due(&c.app.state)
        .await
        .unwrap();
    assert_eq!(n, 1, "um render com o prazo vencido tem de voltar à fila");

    // E um `processing` sem prazo escrito (uma linha de antes desta coluna, ou
    // um pod que morreu entre a marca e o prazo) volta: deixá-lo preso seria
    // perder o recorte para sempre.
    //
    // O estado põe-se por inteiro — incluindo limpar o recuo que a volta
    // anterior possa ter escrito. Este teste é sobre a POSSE, e não pode
    // depender de o render ter passado ou falhado (o que muda com a máquina
    // ter ffmpeg ou não). A falha tem o seu próprio teste, logo abaixo.
    sqlx::query(
        "UPDATE recording_clips
            SET status = 'processing', attempts = 0, running_at = now(),
                lease_expires_at = NULL, next_attempt_at = NULL
          WHERE id = $1::uuid",
    )
    .bind(&id)
    .execute(&c.app.db)
    .await
    .unwrap();
    let n = delonix_server::recording_clip_render_due(&c.app.state)
        .await
        .unwrap();
    assert_eq!(n, 1, "um processing sem prazo tem de voltar à fila");
}

#[sqlx::test]
async fn uma_falha_de_render_deixa_o_codigo_estavel_e_respeita_o_recuo(db: PgPool) {
    // O ffmpeg do SERVIDOR aponta para onde não existe: a falha é a mesma em
    // qualquer máquina, com ou sem ffmpeg instalado. É o que torna esta prova
    // determinística — e é também o caso real de uma instalação incompleta.
    let c = cenario_com(db, 6, 320, 180, &[("FFMPEG_BIN", "/nao/existe/ffmpeg")]).await;
    let (st, clip) = c
        .app
        .post(
            &format!("/api/recordings/{}/clips", c.rec),
            Some(&c.a.token),
            json!({"kind": "clip", "label": "x", "start_ms": 0, "end_ms": 2_000}),
        )
        .await;
    assert_eq!(st, 202, "{clip}");
    let id = clip["id"].as_str().unwrap().to_string();

    let n = delonix_server::recording_clip_render_due(&c.app.state)
        .await
        .unwrap();
    assert_eq!(n, 1);

    // Com tentativas a sobrar volta a `queued`, COM recuo e com o código
    // estável — e nunca se apaga o que havia.
    let (status, code, espera, tentativas): (
        String,
        Option<String>,
        Option<chrono::DateTime<chrono::Utc>>,
        i32,
    ) = sqlx::query_as(
        "SELECT status, error_code, next_attempt_at, attempts
           FROM recording_clips WHERE id = $1::uuid",
    )
    .bind(&id)
    .fetch_one(&c.app.db)
    .await
    .unwrap();
    assert_eq!(status, "queued", "com tentativas a sobrar, volta à fila");
    assert_eq!(
        code.as_deref(),
        Some("clip.ffmpeg_missing"),
        "o código é contrato"
    );
    assert_eq!(tentativas, 1);
    assert!(espera.is_some(), "uma falha tem de deixar recuo escrito");

    // E o recuo respeita-se: sem isto, uma avaria do ffmpeg era repetida a cada
    // 20 segundos para sempre — a lição que os capítulos e a acta já
    // aprenderam.
    let n = delonix_server::recording_clip_render_due(&c.app.state)
        .await
        .unwrap();
    assert_eq!(n, 0, "a fila não pode levar um recorte antes do recuo");

    // Esgotadas as tentativas, fica `failed` — e o cliente vê porquê.
    sqlx::query("UPDATE recording_clips SET next_attempt_at = now() - interval '1 minute' WHERE id = $1::uuid")
        .bind(&id)
        .execute(&c.app.db)
        .await
        .unwrap();
    let n = delonix_server::recording_clip_render_due(&c.app.state)
        .await
        .unwrap();
    assert_eq!(n, 1);
    let (st, final_) = c
        .app
        .get(
            &format!("/api/recordings/{}/clips/{id}", c.rec),
            Some(&c.a.token),
        )
        .await;
    assert_eq!(st, 200, "{final_}");
    assert_eq!(final_["status"], "failed", "{final_}");
    assert_eq!(final_["error_code"], "clip.ffmpeg_missing", "{final_}");
    assert_eq!(final_["attempts"], 2, "{final_}");
}

#[sqlx::test]
async fn apagar_leva_a_linha_e_o_ficheiro(db: PgPool) {
    if !tem_ffmpeg() {
        eprintln!("sem ffmpeg: prova saltada");
        return;
    }
    let c = cenario(db, 8, 320, 180).await;
    let clip = recorta(
        &c,
        json!({"kind": "clip", "label": "x", "start_ms": 0, "end_ms": 3_000}),
    )
    .await;
    let id = clip["id"].as_str().unwrap().to_string();
    let path = c.dir.join(format!("clip-{id}.webm"));
    assert!(path.exists());

    // Quem só vê não apaga.
    let (st, _) = c
        .app
        .delete(
            &format!("/api/recordings/{}/clips/{id}", c.rec),
            Some(&c.carla.token),
        )
        .await;
    assert_eq!(st, 403);
    assert!(path.exists(), "um 403 não pode apagar nada");

    let (st, _) = c
        .app
        .delete(
            &format!("/api/recordings/{}/clips/{id}", c.rec),
            Some(&c.a.token),
        )
        .await;
    assert_eq!(st, 204);
    assert!(!path.exists(), "o ficheiro ficou no disco");
    let (st, _) = c
        .app
        .get(
            &format!("/api/recordings/{}/clips/{id}", c.rec),
            Some(&c.a.token),
        )
        .await;
    assert_eq!(st, 404);
}
