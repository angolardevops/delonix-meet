//! Gravação server-side: o SFU alimenta estes writers com os pacotes RTP de
//! cada publicador (VP8 → IVF com PTS reais; Opus → OGG), e ao parar o
//! ffmpeg compõe tudo num único `.webm`:
//!  - 1 publicador  → remux `-c copy` (zero reencode, zero perda);
//!  - N publicadores → grelha xstack em VP9 CRF 30 + Opus 128k (melhor
//!    rácio qualidade/tamanho sem perda percetível).

use aes_gcm::{
    aead::{Aead, Payload},
    Aes256Gcm, KeyInit,
};
use std::{
    io::{Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    sync::Arc,
    time::Instant,
};
use uuid::Uuid;
use webrtc::media::io::{ogg_writer::OggWriter, Writer as MediaWriter};
use webrtc::rtp::{codecs::vp8::Vp8Packet, packetizer::Depacketizer};

use crate::AppState;

/// Desencripta um frame E2EE do cliente: [header claro | ct+tag(16) | IV(12)],
/// AES-256-GCM com o header como additionalData (ver web/src/e2ee.ts).
/// Devolve header‖plaintext; None se não autenticar. Frames minúsculos
/// passaram em claro no emissor e voltam tal-qual.
fn decrypt_e2ee(key: &Aes256Gcm, data: &[u8], offset: usize) -> Option<Vec<u8>> {
    if data.len() <= offset + 12 + 16 {
        return Some(data.to_vec());
    }
    let (header, rest) = data.split_at(offset);
    let (ct, iv) = rest.split_at(rest.len() - 12);
    let nonce = aes_gcm::Nonce::try_from(iv).ok()?;
    let pt = key
        .decrypt(
            &nonce,
            Payload {
                msg: ct,
                aad: header,
            },
        )
        .ok()?;
    let mut out = Vec::with_capacity(offset + pt.len());
    out.extend_from_slice(header);
    out.extend_from_slice(&pt);
    Some(out)
}

/// IVF (VP8) com PTS em milissegundos derivados do timestamp RTP (90 kHz) —
/// o writer da lib usa um contador de frames, o que acelera/atrasa o vídeo
/// quando o fps varia; este mantém o tempo real.
pub struct Vp8IvfWriter {
    w: std::fs::File,
    count: u32,
    first_ts: Option<u32>,
    frame: Vec<u8>,
    seen_key: bool,
    /// Dimensões reais lidas do primeiro keyframe (corrigidas no close).
    dims: Option<(u16, u16)>,
    /// Chave E2EE da sala (cedida pelo anfitrião) — desencripta cada frame.
    key: Option<Arc<Aes256Gcm>>,
}

impl Vp8IvfWriter {
    pub fn new(mut w: std::fs::File) -> std::io::Result<Self> {
        // Cabeçalho IVF de 32 bytes; timebase 1/1000 => PTS em ms.
        w.write_all(b"DKIF")?;
        w.write_all(&0u16.to_le_bytes())?; // versão
        w.write_all(&32u16.to_le_bytes())?; // tamanho do header
        w.write_all(b"VP80")?;
        w.write_all(&1280u16.to_le_bytes())?; // dimensões nominais; o VP8
        w.write_all(&720u16.to_le_bytes())?; //  real vem do bitstream
        w.write_all(&1000u32.to_le_bytes())?; // timebase denominador
        w.write_all(&1u32.to_le_bytes())?; // timebase numerador
        w.write_all(&0u32.to_le_bytes())?; // nº de frames (corrigido no close)
        w.write_all(&0u32.to_le_bytes())?;
        Ok(Self {
            w,
            count: 0,
            first_ts: None,
            frame: Vec::new(),
            seen_key: false,
            dims: None,
            key: None,
        })
    }

    pub fn write_rtp(&mut self, pkt: &webrtc::rtp::packet::Packet) -> std::io::Result<()> {
        if pkt.payload.is_empty() {
            return Ok(());
        }
        let mut depack = Vp8Packet::default();
        let Ok(payload) = depack.depacketize(&pkt.payload) else {
            return Ok(());
        };
        if payload.is_empty() {
            return Ok(());
        }
        let is_key = payload[0] & 0x01 == 0;
        // Espera pelo primeiro keyframe; frames a meio sem início são descartados.
        if !self.seen_key {
            if !(is_key && self.frame.is_empty() && depack.is_partition_head(&pkt.payload)) {
                if !is_key {
                    return Ok(());
                }
            }
            self.seen_key = true;
        }
        if self.frame.is_empty() && !depack.is_partition_head(&pkt.payload) {
            return Ok(()); // meio de um frame que não começámos
        }
        self.frame.extend_from_slice(&payload);
        if !pkt.header.marker {
            return Ok(());
        }
        // Sala E2EE: o frame remontado é [header claro|ct|IV] — desencriptar
        // antes de escrever (frames que não autenticam são descartados).
        if let Some(key) = &self.key {
            let offset = if self.frame[0] & 0x01 == 0 { 10 } else { 3 };
            match decrypt_e2ee(key, &self.frame, offset) {
                Some(clear) => self.frame = clear,
                None => {
                    self.frame.clear();
                    return Ok(());
                }
            }
        }
        // Keyframe VP8 traz as dimensões (sync 9d 01 2a + 2×u14 LE).
        if self.dims.is_none()
            && self.frame.len() >= 10
            && self.frame[0] & 0x01 == 0
            && self.frame[3..6] == [0x9d, 0x01, 0x2a]
        {
            let w = u16::from_le_bytes([self.frame[6], self.frame[7]]) & 0x3fff;
            let h = u16::from_le_bytes([self.frame[8], self.frame[9]]) & 0x3fff;
            if w > 0 && h > 0 {
                self.dims = Some((w, h));
            }
        }
        let first = *self.first_ts.get_or_insert(pkt.header.timestamp);
        let pts_ms = (pkt.header.timestamp.wrapping_sub(first) as u64) / 90;
        self.w.write_all(&(self.frame.len() as u32).to_le_bytes())?;
        self.w.write_all(&pts_ms.to_le_bytes())?;
        self.w.write_all(&self.frame)?;
        self.frame.clear();
        self.count += 1;
        Ok(())
    }

    pub fn close(&mut self) -> std::io::Result<()> {
        if let Some((w, h)) = self.dims {
            self.w.seek(SeekFrom::Start(12))?;
            self.w.write_all(&w.to_le_bytes())?;
            self.w.write_all(&h.to_le_bytes())?;
        }
        self.w.seek(SeekFrom::Start(24))?;
        self.w.write_all(&self.count.to_le_bytes())?;
        self.w.flush()
    }
}

/// Writer de uma track em gravação (vídeo/ecrã ou áudio).
pub enum RecWriter {
    Video(Vp8IvfWriter),
    Audio {
        w: OggWriter<std::fs::File>,
        key: Option<Arc<Aes256Gcm>>,
    },
}

impl RecWriter {
    pub fn write_rtp(&mut self, pkt: &webrtc::rtp::packet::Packet) {
        match self {
            RecWriter::Video(w) => {
                let _ = w.write_rtp(pkt);
            }
            RecWriter::Audio { w, key } => {
                // Opus: 1 frame por pacote — desencripta o payload (offset 1).
                if let Some(key) = key {
                    let Some(clear) = decrypt_e2ee(key, &pkt.payload, 1) else {
                        return;
                    };
                    let mut pkt2 = pkt.clone();
                    pkt2.payload = clear.into();
                    let _ = w.write_rtp(&pkt2);
                } else {
                    let _ = w.write_rtp(pkt);
                }
            }
        }
    }
    pub fn close(&mut self) {
        match self {
            RecWriter::Video(w) => {
                let _ = w.close();
            }
            RecWriter::Audio { w, .. } => {
                let _ = w.close();
            }
        }
    }
}

/// Metadados de uma track gravada (o writer vive na Publication do SFU).
#[derive(Debug, Clone)]
pub struct RecTrackMeta {
    pub path: PathBuf,
    pub kind: String, // "video" | "audio"
    pub offset_ms: u64,
}

/// Sessão de gravação de uma sala.
pub struct RecordingSession {
    pub id: Uuid,
    pub dir: PathBuf,
    pub started: Instant,
    pub by_user: Uuid,
    pub by_name: String,
    pub tracks: Vec<RecTrackMeta>,
    /// Chave E2EE da sala, cedida pelo anfitrião só para esta gravação
    /// (vive apenas em memória; morre com a sessão).
    pub e2ee_key: Option<Arc<Aes256Gcm>>,
}

impl RecordingSession {
    /// `recordings_dir` vem de `state.config.recordings_dir` (lido uma vez no arranque).
    pub async fn new(
        by_user: Uuid,
        by_name: String,
        e2ee_key: Option<Vec<u8>>,
        recordings_dir: &Path,
    ) -> std::io::Result<Self> {
        let id = Uuid::new_v4();
        let dir = recordings_dir.join(format!("tmp-{id}"));
        tokio::fs::create_dir_all(&dir).await?;
        let e2ee_key = e2ee_key
            .as_deref()
            .and_then(|raw| Aes256Gcm::new_from_slice(raw).ok())
            .map(Arc::new);
        Ok(Self {
            id,
            dir,
            started: Instant::now(),
            by_user,
            by_name,
            tracks: Vec::new(),
            e2ee_key,
        })
    }

    /// Cria o writer para uma track nova e regista os metadados.
    /// `kind`: "video" | "screen" | "audio".
    pub fn open_track(&mut self, kind: &str) -> Option<RecWriter> {
        let n = self.tracks.len();
        let offset_ms = self.started.elapsed().as_millis() as u64;
        let is_audio = kind.ends_with("audio");
        let ext = if is_audio { "ogg" } else { "ivf" };
        let path = self.dir.join(format!("{n:02}-{kind}.{ext}"));
        let file = match std::fs::File::create(&path) {
            Ok(f) => f,
            Err(e) => {
                tracing::error!(path = %path.display(), error = %e, "falha a criar ficheiro de gravação");
                return None;
            }
        };
        let writer = if is_audio {
            match OggWriter::new(file, 48000, 2) {
                Ok(w) => RecWriter::Audio {
                    w,
                    key: self.e2ee_key.clone(),
                },
                Err(e) => {
                    tracing::error!(path = %path.display(), error = %e, "falha a criar OggWriter");
                    return None;
                }
            }
        } else {
            match Vp8IvfWriter::new(file) {
                Ok(mut w) => {
                    w.key = self.e2ee_key.clone();
                    RecWriter::Video(w)
                }
                Err(e) => {
                    tracing::error!(path = %path.display(), error = %e, "falha a criar IvfWriter");
                    return None;
                }
            }
        };
        self.tracks.push(RecTrackMeta {
            path,
            kind: kind.to_string(),
            offset_ms,
        });
        Some(writer)
    }
}

/// Compõe a gravação num único webm (em background) e insere-a na biblioteca.
pub fn finalize(state: Arc<AppState>, room_id: Uuid, session: RecordingSession) {
    tokio::spawn(async move {
        if let Err(e) = finalize_inner(&state, room_id, &session).await {
            tracing::error!(%room_id, error = %e, "server recording finalize failed");
        }
        let _ = tokio::fs::remove_dir_all(&session.dir).await;
    });
}

async fn finalize_inner(
    state: &Arc<AppState>,
    room_id: Uuid,
    session: &RecordingSession,
) -> anyhow::Result<()> {
    // Tracks com conteúdo real (ficheiros ~vazios ficam de fora).
    let mut videos: Vec<&RecTrackMeta> = Vec::new();
    let mut audios: Vec<&RecTrackMeta> = Vec::new();
    for t in &session.tracks {
        let is_big = tokio::fs::metadata(&t.path)
            .await
            .map(|m| m.len() > 4096)
            .unwrap_or(false);
        if !is_big {
            continue;
        }
        if t.kind.ends_with("audio") {
            audios.push(t);
        } else {
            videos.push(t);
        }
    }
    if videos.is_empty() && audios.is_empty() {
        anyhow::bail!("nothing recorded");
    }

    let out = session.dir.join("out.webm");
    let mut cmd = tokio::process::Command::new("ffmpeg");
    cmd.arg("-y").arg("-loglevel").arg("error");

    if videos.len() == 1 && audios.len() <= 1 {
        // Caso simples: remux sem reencode — zero perda de qualidade.
        cmd.arg("-i").arg(&videos[0].path);
        if let Some(a) = audios.first() {
            cmd.arg("-i").arg(&a.path);
        }
        cmd.args(["-map", "0:v:0"]);
        if !audios.is_empty() {
            cmd.args(["-map", "1:a:0"]);
        }
        cmd.args(["-c", "copy"]);
    } else {
        // Composição em grelha + mistura de áudio (VP9 CRF 30 + Opus 128k).
        for v in &videos {
            cmd.arg("-i").arg(&v.path);
        }
        for a in &audios {
            cmd.arg("-i").arg(&a.path);
        }
        let n = videos.len();
        let cols = (n as f64).sqrt().ceil() as usize;
        let mut fc = String::new();
        for (i, v) in videos.iter().enumerate() {
            let off = v.offset_ms as f64 / 1000.0;
            fc.push_str(&format!(
                "[{i}:v]scale=640:360:force_original_aspect_ratio=decrease,pad=640:360:(ow-iw)/2:(oh-ih)/2,setsar=1,tpad=start_duration={off:.3}:start_mode=add:color=black[v{i}];"
            ));
        }
        let vout = if n > 1 {
            let layout = (0..n)
                .map(|i| format!("{}_{}", (i % cols) * 640, (i / cols) * 360))
                .collect::<Vec<_>>()
                .join("|");
            let ins = (0..n).map(|i| format!("[v{i}]")).collect::<String>();
            fc.push_str(&format!(
                "{ins}xstack=inputs={n}:layout={layout}:fill=black[vout];"
            ));
            "[vout]"
        } else if n == 1 {
            "[v0]"
        } else {
            ""
        };
        let mut aout = String::new();
        if !audios.is_empty() {
            for (j, a) in audios.iter().enumerate() {
                let idx = n + j;
                fc.push_str(&format!(
                    "[{idx}:a]adelay={ms}:all=1[a{j}];",
                    ms = a.offset_ms
                ));
            }
            if audios.len() > 1 {
                let ins = (0..audios.len())
                    .map(|j| format!("[a{j}]"))
                    .collect::<String>();
                fc.push_str(&format!(
                    "{ins}amix=inputs={}:normalize=0[aout];",
                    audios.len()
                ));
                aout = "[aout]".into();
            } else {
                aout = "[a0]".into();
            }
        }
        let fc = fc.trim_end_matches(';').to_string();
        cmd.arg("-filter_complex").arg(&fc);
        if !vout.is_empty() {
            cmd.args(["-map", vout]);
            cmd.args([
                "-c:v",
                "libvpx-vp9",
                "-b:v",
                "0",
                "-crf",
                "30",
                "-deadline",
                "good",
                "-cpu-used",
                "4",
                "-row-mt",
                "1",
                "-pix_fmt",
                "yuv420p",
            ]);
        }
        if !aout.is_empty() {
            cmd.args(["-map", &aout]);
            cmd.args(["-c:a", "libopus", "-b:a", "128k", "-ar", "48000"]);
        }
    }
    cmd.arg(&out);

    tracing::info!(%room_id, tracks = session.tracks.len(), "server recording: a compor webm…");
    let status = cmd.status().await?;
    if !status.success() {
        anyhow::bail!("ffmpeg exited with {status}");
    }
    let size = tokio::fs::metadata(&out).await?.len() as i64;

    // Nome amigável com o código da sala e a hora.
    let code: Option<(String,)> = sqlx::query_as("SELECT code FROM rooms WHERE id = $1")
        .bind(room_id)
        .fetch_optional(&state.db)
        .await?;
    let code = code.map(|c| c.0).unwrap_or_default();
    let stamp = chrono::Local::now().format("%Y-%m-%d %H:%M");
    let filename = format!("Reunião {code} — servidor — {stamp}.webm");

    let (rec_id,): (Uuid,) = sqlx::query_as(
        "INSERT INTO recordings (room_id, uploader_id, filename, size_bytes)
         VALUES ($1, $2, $3, $4) RETURNING id",
    )
    .bind(room_id)
    .bind(session.by_user)
    .bind(&filename)
    .bind(size)
    .fetch_one(&state.db)
    .await?;

    let final_path = state.config.recordings_dir.join(format!("{rec_id}.webm"));
    // rename falha com EXDEV (errno 18) se out e final_path estiverem em
    // filesystems diferentes (e.g., tmp num volume separado). Fallback: copy+delete.
    match tokio::fs::rename(&out, &final_path).await {
        Ok(()) => {}
        Err(e) if e.raw_os_error() == Some(18) => {
            tokio::fs::copy(&out, &final_path).await?;
            let _ = tokio::fs::remove_file(&out).await;
        }
        Err(e) => return Err(e.into()),
    }
    tracing::info!(%room_id, %rec_id, size, "server recording pronta na biblioteca");

    // Webhook recording.ready para as organizações de quem gravou.
    let orgs = crate::org::orgs_of_user(state, session.by_user).await;
    if !orgs.is_empty() {
        let mb = size / (1024 * 1024);
        let text = format!("Nova gravação disponível: «{filename}» ({mb} MB)");
        let payload = serde_json::json!({
            "recording_id": rec_id,
            "filename": filename,
            "size_bytes": size,
            "room_code": code,
        });
        for org_id in orgs {
            crate::webhooks::fire(
                state.clone(),
                org_id,
                crate::webhooks::Event {
                    name: "recording.ready",
                    title: "Delonix Meet".into(),
                    text: text.clone(),
                    payload: payload.clone(),
                },
            );
        }
    }
    Ok(())
}

/// Cron de retenção (DLP-lite): apaga gravações mais antigas que
/// `organizations.retention_days` (>0) de cada org, ficheiro + registo.
pub async fn retention_sweep(state: &Arc<AppState>) -> usize {
    let rows: Vec<(Uuid, String)> = match sqlx::query_as(
        "SELECT r.id, r.filename FROM recordings r
         JOIN org_members m ON m.user_id = r.uploader_id
         JOIN organizations o ON o.id = m.org_id
         WHERE o.retention_days > 0
           AND r.created_at < now() - make_interval(days => o.retention_days)",
    )
    .fetch_all(&state.db)
    .await
    {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!(error = %e, "retention query failed");
            return 0;
        }
    };
    let mut n = 0;
    for (id, _fname) in rows {
        let path = state.config.recordings_dir.join(format!("{id}.webm"));
        let _ = tokio::fs::remove_file(&path).await;
        if sqlx::query("DELETE FROM recordings WHERE id = $1")
            .bind(id)
            .execute(&state.db)
            .await
            .is_ok()
        {
            n += 1;
        }
    }
    if n > 0 {
        tracing::info!(deleted = n, "retention sweep");
    }
    n
}
