//! Medição de uma gravação com `ffprobe`: duração, resolução, fps, codecs e
//! miniatura (R183; portado do servidor da UI).
//!
//! Corre no fim da composição do gravador do servidor e depois de cada upload.
//! Um valor que não se consegue medir fica `None` e vai para a base como NULL —
//! nunca se inventa uma duração nem uma resolução.
//!
//! **Sem ffprobe/ffmpeg** (o CI não os tem, e uma instalação pode não os ter)
//! nada falha: o `spawn` dá erro, fica um aviso no log, os campos ficam NULL e
//! `has_thumbnail = false`. A gravação entra e reproduz na mesma.
//!
//! O caso difícil é o do browser: o `MediaRecorder` escreve webm SEM duração
//! no cabeçalho (é um ficheiro «ao vivo»), e o `ffprobe` devolve-a vazia. Aí
//! lê-se o ficheiro inteiro uma vez em modo cópia (`-c copy -f null`), que não
//! descodifica nada e custa segundos, não minutos.
//!
//! Nenhum processo daqui abre ligações de rede: lêem um ficheiro local e
//! escrevem outro.

use std::path::Path;
use std::time::Duration;

use serde::Deserialize;
use uuid::Uuid;

use crate::AppState;

/// Tecto de cada passo de medição. Um ficheiro corrompido não pode prender a
/// tarefa de upload nem a de composição.
const PROBE_TIMEOUT: Duration = Duration::from_secs(60);

/// Largura da miniatura (a altura segue a proporção).
const THUMB_WIDTH: u32 = 480;

#[derive(Debug, Default, Clone, PartialEq)]
pub struct MediaInfo {
    pub duration_ms: Option<i64>,
    pub width: Option<i32>,
    pub height: Option<i32>,
    pub fps: Option<f32>,
    pub video_codec: Option<String>,
    pub audio_codec: Option<String>,
}

#[derive(Deserialize)]
struct ProbeOut {
    #[serde(default)]
    streams: Vec<ProbeStream>,
    #[serde(default)]
    format: Option<ProbeFormat>,
}

#[derive(Deserialize)]
struct ProbeStream {
    codec_type: Option<String>,
    codec_name: Option<String>,
    width: Option<i32>,
    height: Option<i32>,
    avg_frame_rate: Option<String>,
    r_frame_rate: Option<String>,
    duration: Option<String>,
    #[serde(default)]
    tags: std::collections::HashMap<String, String>,
}

#[derive(Deserialize)]
struct ProbeFormat {
    duration: Option<String>,
}

/// `"30000/1001"` → 29.97. Recusa `0/0` e taxas absurdas: o webm do browser
/// declara `r_frame_rate = 1000/1` (a base de tempo, não a cadência).
fn parse_rate(s: &str) -> Option<f32> {
    let (n, d) = s.split_once('/').unwrap_or((s, "1"));
    let n: f64 = n.trim().parse().ok()?;
    let d: f64 = d.trim().parse().ok()?;
    if d <= 0.0 || n <= 0.0 {
        return None;
    }
    let r = n / d;
    (r > 0.0 && r <= 240.0).then_some(((r * 100.0).round() / 100.0) as f32)
}

/// Segundos em texto (`"4.001000"`) → milissegundos.
fn parse_secs(s: &str) -> Option<i64> {
    let v: f64 = s.trim().parse().ok()?;
    (v.is_finite() && v > 0.0).then_some((v * 1000.0).round() as i64)
}

/// Duração no formato das tags Matroska: `"00:01:02.500000000"`.
fn parse_hms(s: &str) -> Option<i64> {
    let mut parts = s.trim().split(':');
    let h: f64 = parts.next()?.parse().ok()?;
    let m: f64 = parts.next()?.parse().ok()?;
    let sec: f64 = parts.next()?.parse().ok()?;
    let total = h * 3600.0 + m * 60.0 + sec;
    (total > 0.0).then_some((total * 1000.0).round() as i64)
}

/// Lê a saída JSON do `ffprobe -show_streams -show_format`.
pub fn parse_ffprobe(json: &str) -> Option<MediaInfo> {
    let out: ProbeOut = serde_json::from_str(json).ok()?;
    let mut info = MediaInfo::default();
    let mut stream_duration: Option<i64> = None;
    for s in &out.streams {
        let d = s
            .duration
            .as_deref()
            .and_then(parse_secs)
            .or_else(|| s.tags.get("DURATION").and_then(|t| parse_hms(t)));
        if let Some(d) = d {
            stream_duration = Some(stream_duration.map_or(d, |cur| cur.max(d)));
        }
        match s.codec_type.as_deref() {
            Some("video") if info.video_codec.is_none() => {
                info.video_codec = s.codec_name.clone();
                info.width = s.width.filter(|w| *w > 0);
                info.height = s.height.filter(|h| *h > 0);
                info.fps = s
                    .avg_frame_rate
                    .as_deref()
                    .and_then(parse_rate)
                    .or_else(|| s.r_frame_rate.as_deref().and_then(parse_rate));
            }
            Some("audio") if info.audio_codec.is_none() => {
                info.audio_codec = s.codec_name.clone();
            }
            _ => {}
        }
    }
    info.duration_ms = out
        .format
        .and_then(|f| f.duration)
        .as_deref()
        .and_then(parse_secs)
        .or(stream_duration);
    Some(info)
}

/// Último `out_time_us=` da saída `-progress` do ffmpeg, em milissegundos.
pub fn parse_progress_duration(progress: &str) -> Option<i64> {
    progress
        .lines()
        .rev()
        .filter_map(|l| l.strip_prefix("out_time_us="))
        .find_map(|v| v.trim().parse::<i64>().ok().filter(|us| *us > 0))
        .map(|us| us / 1000)
}

async fn run_capture(mut cmd: tokio::process::Command) -> anyhow::Result<String> {
    cmd.stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true);
    let child = cmd.spawn()?;
    let out = tokio::time::timeout(PROBE_TIMEOUT, child.wait_with_output())
        .await
        .map_err(|_| anyhow::anyhow!("medição excedeu {}s", PROBE_TIMEOUT.as_secs()))??;
    if !out.status.success() {
        anyhow::bail!("terminou com {}", out.status);
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Mede um ficheiro. `Err` só quando o `ffprobe` não corre ou não reconhece o
/// ficheiro; campos que o ficheiro não declara ficam `None`.
pub async fn probe(ffprobe: &str, ffmpeg: &str, path: &Path) -> anyhow::Result<MediaInfo> {
    let mut cmd = tokio::process::Command::new(ffprobe);
    cmd.args([
        "-v",
        "error",
        "-print_format",
        "json",
        "-show_streams",
        "-show_format",
    ])
    .arg(path);
    let json = run_capture(cmd).await?;
    let mut info =
        parse_ffprobe(&json).ok_or_else(|| anyhow::anyhow!("saída do ffprobe ilegível"))?;
    if info.duration_ms.is_none() && (info.video_codec.is_some() || info.audio_codec.is_some()) {
        // Webm do browser: sem duração no cabeçalho. Lê-se o ficheiro em cópia.
        let mut cmd = tokio::process::Command::new(ffmpeg);
        cmd.args(["-nostdin", "-v", "error", "-i"]).arg(path).args([
            "-map",
            "0",
            "-c",
            "copy",
            "-f",
            "null",
            "-",
            "-progress",
            "pipe:1",
        ]);
        if let Ok(p) = run_capture(cmd).await {
            info.duration_ms = parse_progress_duration(&p);
        }
    }
    Ok(info)
}

/// Instante da miniatura: 1 s, ou o meio de uma gravação mais curta.
pub fn thumbnail_at_ms(duration_ms: Option<i64>) -> i64 {
    match duration_ms {
        Some(d) if d < 2000 => d / 2,
        _ => 1000,
    }
}

/// Extrai um fotograma para `dst` (JPEG). Devolve `true` se o ficheiro ficou escrito.
pub async fn thumbnail(ffmpeg: &str, src: &Path, dst: &Path, at_ms: i64) -> bool {
    let mut cmd = tokio::process::Command::new(ffmpeg);
    cmd.args(["-nostdin", "-y", "-v", "error", "-i"])
        .arg(src)
        .args([
            "-ss",
            &format!("{:.3}", at_ms as f64 / 1000.0),
            "-frames:v",
            "1",
            "-vf",
            &format!("scale={THUMB_WIDTH}:-2"),
            "-q:v",
            "4",
        ])
        .arg(dst);
    if run_capture(cmd).await.is_err() {
        return false;
    }
    tokio::fs::metadata(dst)
        .await
        .map(|m| m.len() > 0)
        .unwrap_or(false)
}

/// Caminho da miniatura de uma gravação.
pub fn thumbnail_path(state: &AppState, rec_id: Uuid) -> std::path::PathBuf {
    state.config.recordings_dir.join(format!("{rec_id}.jpg"))
}

/// O que ficou medido e guardado.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Probed {
    pub info: MediaInfo,
    pub has_thumbnail: bool,
}

/// Mede o ficheiro de uma gravação, gera a miniatura e grava tudo na base.
///
/// Falhar a medir não falha a gravação: o ficheiro existe e reproduz; só a
/// lista fica sem duração. O motivo vai para o log de quem opera.
///
/// Um campo que a medição não deu NÃO apaga o que a base já tinha
/// (`COALESCE`): o gravador do servidor escreve a duração da sessão e as
/// dimensões da grelha antes de medir, e um ffprobe ausente não as deita fora.
pub async fn probe_and_store(state: &AppState, rec_id: Uuid, path: &Path) -> Probed {
    let info = match probe(&state.config.ffprobe_bin, &state.config.ffmpeg_bin, path).await {
        Ok(i) => i,
        Err(e) => {
            tracing::warn!(%rec_id, error = %e, "ffprobe não mediu a gravação");
            MediaInfo::default()
        }
    };
    let has_thumbnail = info.video_codec.is_some()
        && thumbnail(
            &state.config.ffmpeg_bin,
            path,
            &thumbnail_path(state, rec_id),
            thumbnail_at_ms(info.duration_ms),
        )
        .await;
    let stored: Result<Option<StoredMedia>, sqlx::Error> = sqlx::query_as(
        "UPDATE recordings SET duration_ms = COALESCE($2, duration_ms),
                width = COALESCE($3, width), height = COALESCE($4, height),
                fps = COALESCE($5, fps), video_codec = COALESCE($6, video_codec),
                audio_codec = COALESCE($7, audio_codec), has_thumbnail = $8, probed_at = now()
          WHERE id = $1
         RETURNING duration_ms, width, height, fps, video_codec, audio_codec",
    )
    .bind(rec_id)
    .bind(info.duration_ms)
    .bind(info.width)
    .bind(info.height)
    .bind(info.fps)
    .bind(&info.video_codec)
    .bind(&info.audio_codec)
    .bind(has_thumbnail)
    .fetch_optional(&state.db)
    .await;
    match stored {
        Ok(Some(m)) => Probed {
            info: MediaInfo {
                duration_ms: m.duration_ms,
                width: m.width,
                height: m.height,
                fps: m.fps,
                video_codec: m.video_codec,
                audio_codec: m.audio_codec,
            },
            has_thumbnail,
        },
        Ok(None) => Probed {
            info,
            has_thumbnail,
        },
        Err(e) => {
            tracing::error!(%rec_id, error = %e, "não foi possível gravar os metadados da gravação");
            Probed {
                info,
                has_thumbnail,
            }
        }
    }
}

#[derive(sqlx::FromRow)]
struct StoredMedia {
    duration_ms: Option<i64>,
    width: Option<i32>,
    height: Option<i32>,
    fps: Option<f32>,
    video_codec: Option<String>,
    audio_codec: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn webm_do_browser_sem_duracao_no_cabecalho() {
        // Forma real do `ffprobe` sobre um webm `-live 1` (como o MediaRecorder).
        let json = r#"{"streams":[
            {"codec_type":"video","codec_name":"vp8","width":1280,"height":720,
             "r_frame_rate":"1000/1","avg_frame_rate":"0/0"},
            {"codec_type":"audio","codec_name":"opus","r_frame_rate":"0/0"}],
            "format":{"format_name":"matroska,webm"}}"#;
        let i = parse_ffprobe(json).unwrap();
        assert_eq!(i.width, Some(1280));
        assert_eq!(i.height, Some(720));
        assert_eq!(i.video_codec.as_deref(), Some("vp8"));
        assert_eq!(i.audio_codec.as_deref(), Some("opus"));
        // 1000/1 é a base de tempo, não a cadência: não se inventa 1000 fps.
        assert_eq!(i.fps, None);
        assert_eq!(i.duration_ms, None, "sem duração declarada fica None");
    }

    #[test]
    fn duracao_do_formato_e_cadencia_media() {
        let json = r#"{"streams":[{"codec_type":"video","codec_name":"vp9","width":3840,
            "height":2160,"avg_frame_rate":"30000/1001"}],
            "format":{"duration":"2892.480000"}}"#;
        let i = parse_ffprobe(json).unwrap();
        assert_eq!(i.duration_ms, Some(2_892_480));
        assert_eq!(i.fps, Some(29.97));
        assert_eq!(i.audio_codec, None);
    }

    #[test]
    fn duracao_pela_tag_matroska_quando_o_formato_nao_a_tem() {
        let json = r#"{"streams":[{"codec_type":"audio","codec_name":"opus",
            "tags":{"DURATION":"00:01:02.500000000"}}],"format":{}}"#;
        assert_eq!(parse_ffprobe(json).unwrap().duration_ms, Some(62_500));
    }

    #[test]
    fn json_invalido_nao_e_um_ficheiro_medido() {
        assert!(parse_ffprobe("não é json").is_none());
    }

    #[test]
    fn progresso_da_passagem_em_copia() {
        let p = "out_time_us=2000000\nprogress=continue\nout_time_us=4001000\nprogress=end\n";
        assert_eq!(parse_progress_duration(p), Some(4001));
        assert_eq!(parse_progress_duration("out_time_us=N/A\n"), None);
    }

    #[test]
    fn miniatura_nunca_depois_do_fim() {
        assert_eq!(thumbnail_at_ms(Some(800)), 400);
        assert_eq!(thumbnail_at_ms(Some(60_000)), 1000);
        assert_eq!(thumbnail_at_ms(None), 1000);
    }

    /// Sem os binários (o caso do CI): erro para quem mede, `false` para a
    /// miniatura — nunca um pânico nem um valor inventado.
    #[tokio::test]
    async fn sem_ffprobe_nem_ffmpeg_nao_ha_medicao_nem_miniatura() {
        let nope = Path::new("/nao-existe/gravacao.webm");
        assert!(probe("/nao-existe/ffprobe", "/nao-existe/ffmpeg", nope)
            .await
            .is_err());
        assert!(
            !thumbnail(
                "/nao-existe/ffmpeg",
                nope,
                Path::new("/nao-existe/t.jpg"),
                1000
            )
            .await
        );
    }

    /// Com o ffmpeg instalado, mede um webm «ao vivo» a sério. Sem ele, o
    /// teste diz que não correu em vez de fingir que passou.
    #[tokio::test]
    async fn mede_um_webm_real_sem_duracao_no_cabecalho() {
        let dir = std::env::temp_dir().join(format!("dlx-probe-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let src = dir.join("live.webm");
        let made = std::process::Command::new("ffmpeg")
            .args(["-y", "-loglevel", "error", "-f", "lavfi", "-i"])
            .arg("testsrc=size=640x360:rate=25")
            .args(["-f", "lavfi", "-i", "sine=frequency=440", "-t", "3"])
            .args([
                "-c:v", "libvpx", "-c:a", "libopus", "-live", "1", "-f", "webm",
            ])
            .arg(&src)
            .status();
        if !matches!(made, Ok(s) if s.success()) {
            eprintln!("ffmpeg indisponível — medição real NÃO verificada");
            let _ = std::fs::remove_dir_all(&dir);
            return;
        }
        let i = probe("ffprobe", "ffmpeg", &src).await.unwrap();
        assert_eq!((i.width, i.height), (Some(640), Some(360)));
        assert_eq!(i.fps, Some(25.0));
        let d = i
            .duration_ms
            .expect("a passagem em cópia tem de dar a duração");
        assert!((2900..=3100).contains(&d), "duração medida {d} ms");
        let thumb = dir.join("t.jpg");
        assert!(thumbnail("ffmpeg", &src, &thumb, thumbnail_at_ms(i.duration_ms)).await);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
