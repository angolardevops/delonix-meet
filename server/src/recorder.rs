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
    /// `BufWriter` e não `File` directo: cada pacote RTP fazia uma `write(2)`
    /// própria, e essa chamada é SÍNCRONA dentro da task async que reencaminha
    /// RTP — com uma gravação a 30 fps por publicador são milhares de syscalls
    /// por segundo a bloquear um worker do Tokio. Com buffer, é uma escrita a
    /// cada 64 KiB. O `close()` faz `seek` para corrigir o cabeçalho, e o
    /// `BufWriter` esvazia o buffer antes de qualquer `seek` — por isso o
    /// cabeçalho continua a ser corrigido correctamente.
    w: std::io::BufWriter<std::fs::File>,
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
    pub fn new(w: std::fs::File) -> std::io::Result<Self> {
        let mut w = std::io::BufWriter::with_capacity(64 * 1024, w);
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

/// O que escreve mesmo no disco. Vive numa thread dedicada — ver `RecWriter`.
enum RecSink {
    Video(Vp8IvfWriter),
    Audio {
        w: OggWriter<std::io::BufWriter<std::fs::File>>,
        key: Option<Arc<Aes256Gcm>>,
    },
}

impl RecSink {
    fn write_rtp(&mut self, pkt: &webrtc::rtp::packet::Packet) {
        match self {
            RecSink::Video(w) => {
                let _ = w.write_rtp(pkt);
            }
            RecSink::Audio { w, key } => {
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
    fn close(&mut self) {
        match self {
            RecSink::Video(w) => {
                let _ = w.close();
            }
            RecSink::Audio { w, .. } => {
                let _ = w.close();
            }
        }
    }
}

/// Writer de uma track em gravação — um **handle** para uma thread de escrita.
///
/// Porquê uma thread e não escrita directa: o `write_rtp` era chamado de dentro
/// da task async que reencaminha RTP, e escrevia com `std::fs::File`, que é
/// SÍNCRONO. Com o volume de gravações lento ou cheio, uma escrita bloqueava um
/// worker do Tokio — e um worker bloqueado não serve só a gravação, serve todas
/// as salas que calharem naquela thread. O `BufWriter` (que já lá estava) reduziu
/// a frequência das syscalls; não tirou a escrita do executor.
///
/// A fila é LIMITADA (`REC_QUEUE_CAP`), pela mesma razão que todas as outras o
/// são: um disco que não acompanha não pode virar consumo de memória sem fim.
/// Cheia, PERDEM-SE pacotes — e isso é registado e contado, nunca silencioso:
/// uma gravação corrompida em silêncio é a R18, e é o pior resultado possível.
pub struct RecWriter {
    tx: Option<std::sync::mpsc::SyncSender<Box<webrtc::rtp::packet::Packet>>>,
    join: Option<std::thread::JoinHandle<()>>,
    dropped: Arc<std::sync::atomic::AtomicU64>,
    metrics: Arc<crate::metrics::Metrics>,
    label: String,
}

impl RecWriter {
    fn spawn(
        sink: RecSink,
        cap: usize,
        metrics: Arc<crate::metrics::Metrics>,
        label: String,
    ) -> Self {
        let (tx, rx) = std::sync::mpsc::sync_channel::<Box<webrtc::rtp::packet::Packet>>(cap);
        let join = std::thread::Builder::new()
            .name(format!("dlx-rec-{label}"))
            .spawn(move || {
                let mut sink = sink;
                // O laço termina quando TODOS os emissores caem (o `close`
                // larga o `tx`), e só então se fecha o ficheiro. É isto que
                // garante que o que estava em fila chega ao disco antes de o
                // ffmpeg abrir o ficheiro.
                while let Ok(pkt) = rx.recv() {
                    sink.write_rtp(&pkt);
                }
                sink.close();
            })
            .expect("thread de gravação");
        Self {
            tx: Some(tx),
            join: Some(join),
            dropped: Arc::new(std::sync::atomic::AtomicU64::new(0)),
            metrics,
            label,
        }
    }

    /// Entrega um pacote à thread de escrita. NUNCA bloqueia o executor.
    pub fn write_rtp(&self, pkt: &webrtc::rtp::packet::Packet) {
        let Some(tx) = &self.tx else { return };
        if tx.try_send(Box::new(pkt.clone())).is_err() {
            // Fila cheia (o disco não acompanha) ou thread morta. Perde-se o
            // pacote — a alternativa era bloquear o executor, que é pior. Conta-se
            // SEMPRE: é isto que transforma «a gravação saiu estranha» num número.
            let n = self
                .dropped
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            crate::metrics::Metrics::bump(&self.metrics.recording_packets_dropped_total);
            // Um aviso por cada 500 perdidos: o primeiro diz que começou, e os
            // seguintes dão a escala sem encher o log a milhares de linhas.
            if n.is_multiple_of(500) {
                tracing::warn!(
                    track = %self.label,
                    perdidos = n + 1,
                    "gravação: fila de escrita cheia — o disco não acompanha"
                );
            }
        }
    }

    /// Pacotes perdidos por fila cheia nesta track. `> 0` = gravação degradada.
    pub fn dropped(&self) -> u64 {
        self.dropped.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Fecha o writer e **espera** que a thread esvazie a fila e feche o ficheiro.
    ///
    /// É `async` de propósito. O `join()` bloqueia, e bloquear o executor é
    /// exactamente o que esta mudança existe para evitar — por isso o `join`
    /// corre em `spawn_blocking`. E é preciso ESPERAR: o `finalize` invoca o
    /// ffmpeg logo a seguir, e um ficheiro ainda por esvaziar dá uma gravação
    /// truncada sem um único erro pelo caminho.
    pub async fn close(mut self) -> u64 {
        let perdidos = self.dropped();
        drop(self.tx.take()); // fecha o canal → o laço da thread termina
        if let Some(h) = self.join.take() {
            let _ = tokio::task::spawn_blocking(move || h.join()).await;
        }
        perdidos
    }
}

impl Drop for RecWriter {
    fn drop(&mut self) {
        // Rede de segurança: se alguém largar o writer sem `close().await` (um
        // caminho de erro, um `?` pelo meio), o canal fecha e a thread ainda
        // esvazia o que tem e fecha o ficheiro. Não se faz `join` aqui — o
        // `Drop` pode correr no executor, e bloqueá-lo é o problema original.
        drop(self.tx.take());
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
        // Os bytes crus da chave passam a `Aes256Gcm` (cuja tabela interna é
        // limpa no Drop, via a feature `zeroize` do `cipher`) e o `Vec` de
        // origem é sobrescrito à mão — sem isto ficaria a chave AES-256 em
        // claro numa alocação libertada, à espera de quem leia a heap.
        let e2ee_key = {
            use zeroize::Zeroize;
            let mut raw = e2ee_key;
            let k = raw
                .as_deref()
                .and_then(|r| Aes256Gcm::new_from_slice(r).ok())
                .map(Arc::new);
            if let Some(v) = raw.as_mut() {
                v.zeroize();
            }
            k
        };
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
    pub fn open_track(
        &mut self,
        kind: &str,
        cap: usize,
        metrics: Arc<crate::metrics::Metrics>,
    ) -> Option<RecWriter> {
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
        let sink = if is_audio {
            match OggWriter::new(std::io::BufWriter::with_capacity(64 * 1024, file), 48000, 2) {
                Ok(w) => RecSink::Audio {
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
                    RecSink::Video(w)
                }
                Err(e) => {
                    tracing::error!(path = %path.display(), error = %e, "falha a criar IvfWriter");
                    return None;
                }
            }
        };
        let writer = RecWriter::spawn(sink, cap, metrics, format!("{n:02}-{kind}"));
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

/// Corre um processo externo com **tecto de tempo**, matando-o se o exceder.
///
/// Existe separado para ser testável sem um `ffmpeg` instalado: o
/// comportamento que interessa — não ficar pendurado para sempre, e matar o
/// processo em vez de o deixar órfão — é o mesmo seja qual for o binário.
async fn run_bounded(
    cmd: &mut tokio::process::Command,
    limit: std::time::Duration,
) -> anyhow::Result<std::process::ExitStatus> {
    let mut child = cmd.spawn()?;
    match tokio::time::timeout(limit, child.wait()).await {
        Ok(res) => Ok(res?),
        Err(_) => {
            // `kill` e depois `wait`: sem colher o filho ficava zombie.
            let _ = child.kill().await;
            let _ = child.wait().await;
            anyhow::bail!(
                "processo excedeu {}s e foi terminado (sobe FFMPEG_TIMEOUT_SECS \
                 se as gravações forem legitimamente mais longas)",
                limit.as_secs()
            )
        }
    }
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
    // `-nostdin`: sem isto o ffmpeg herda o stdin do servidor e pode ficar à
    // espera de input que nunca chega. `-threads`: travão de CPU — a
    // composição de uma gravação não pode degradar as chamadas VIVAS do mesmo
    // pod. `kill_on_drop`: se este future for cancelado, o processo morre com
    // ele em vez de ficar órfão a consumir o nó.
    cmd.arg("-nostdin");
    cmd.args(["-threads", &state.config.ffmpeg_threads.to_string()]);
    cmd.kill_on_drop(true);

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
    // Tecto de tempo. Um input malformado (ou um codec inesperado — ver R18)
    // pendurava o ffmpeg indefinidamente: o directório `tmp-<uuid>` ficava no
    // volume, a gravação nunca chegava à biblioteca, e não havia erro nenhum
    // para ver. Falhar em tempo limitado é a única resposta honesta.
    let limit = std::time::Duration::from_secs(state.config.ffmpeg_timeout_secs);
    let status = run_bounded(&mut cmd, limit).await?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    // ------------------------------------------------------------------
    //  Formato E2EE: o Rust tem de decifrar o que o browser cifra
    // ------------------------------------------------------------------
    //
    // O cifrador vive em JavaScript (`web/src/e2ee.ts`, dentro de um worker) e
    // o decifrador em Rust (`decrypt_e2ee`). São duas implementações do MESMO
    // formato, em linguagens diferentes, sem nada que as obrigue a concordar.
    // Se divergirem, as gravações de salas E2EE saem em RUÍDO — e ninguém dá
    // por isso, porque o `Vp8IvfWriter` limita-se a descartar o que não
    // autentica e o ficheiro sai vazio ou truncado, sem erro nenhum.
    //
    // Estes testes reconstroem em Rust, byte a byte, o que o worker produz:
    //     [ header em claro | ciphertext+tag | IV(12) ]   AAD = header

    /// Cifra como o worker do browser cifra. Se este helper e o `e2ee.ts`
    /// divergirem, é sinal de que o formato mudou de um lado só.
    fn cifra_como_o_browser(
        key: &Aes256Gcm,
        header: &[u8],
        payload: &[u8],
        iv: &[u8; 12],
    ) -> Vec<u8> {
        use aes_gcm::aead::Aead;
        let nonce = aes_gcm::Nonce::try_from(&iv[..]).expect("nonce de 12 bytes");
        let ct = key
            .encrypt(
                &nonce,
                Payload {
                    msg: payload,
                    aad: header,
                },
            )
            .expect("cifrar");
        let mut out = Vec::with_capacity(header.len() + ct.len() + 12);
        out.extend_from_slice(header);
        out.extend_from_slice(&ct);
        out.extend_from_slice(iv);
        out
    }

    fn chave_de_teste(b: u8) -> Aes256Gcm {
        use aes_gcm::KeyInit;
        Aes256Gcm::new_from_slice(&[b; 32]).unwrap()
    }

    #[test]
    fn decifra_o_que_o_browser_cifrou_em_video_e_audio() {
        let key = chave_de_teste(7);
        // Os três offsets que o `cryptoOffset` do worker produz:
        // vídeo keyframe = 10, vídeo delta = 3, áudio = 1.
        for offset in [10usize, 3, 1] {
            let header: Vec<u8> = (0..offset as u8).collect();
            let payload: Vec<u8> = (0..200u8).collect();
            let frame = cifra_como_o_browser(&key, &header, &payload, &[9u8; 12]);

            let claro = decrypt_e2ee(&key, &frame, offset).expect("tem de autenticar");
            assert_eq!(&claro[..offset], &header[..], "o header sai intacto");
            assert_eq!(
                &claro[offset..],
                &payload[..],
                "o payload sai igual ao original"
            );
        }
    }

    #[test]
    fn o_header_vai_autenticado_nao_so_em_claro() {
        // O header fica legível de propósito (os packetizers precisam dele),
        // mas entra como AAD. Adulterá-lo tem de fazer a autenticação falhar —
        // senão um intermediário podia reescrever metadados de frame à vontade.
        let key = chave_de_teste(7);
        let header = vec![0u8, 1, 2, 3, 4, 5, 6, 7, 8, 9];
        let payload: Vec<u8> = (0..64u8).collect();
        let mut frame = cifra_como_o_browser(&key, &header, &payload, &[1u8; 12]);

        frame[2] ^= 0xff; // um bit trocado no header
        assert!(
            decrypt_e2ee(&key, &frame, 10).is_none(),
            "header adulterado TEM de falhar a autenticação"
        );
    }

    #[test]
    fn chave_errada_nao_decifra() {
        let header = vec![0u8, 1, 2];
        let payload: Vec<u8> = (0..64u8).collect();
        let frame = cifra_como_o_browser(&chave_de_teste(7), &header, &payload, &[2u8; 12]);
        assert!(decrypt_e2ee(&chave_de_teste(8), &frame, 3).is_none());
    }

    #[test]
    fn ciphertext_adulterado_nao_decifra() {
        let key = chave_de_teste(7);
        let header = vec![0u8, 1, 2];
        let payload: Vec<u8> = (0..64u8).collect();
        let mut frame = cifra_como_o_browser(&key, &header, &payload, &[3u8; 12]);
        let meio = frame.len() / 2;
        frame[meio] ^= 0x01;
        assert!(decrypt_e2ee(&key, &frame, 3).is_none());
    }

    #[test]
    fn iv_trocado_nao_decifra() {
        // O IV vai no FIM do frame, em claro. Trocá-lo tem de invalidar a tag.
        let key = chave_de_teste(7);
        let header = vec![0u8, 1, 2];
        let payload: Vec<u8> = (0..64u8).collect();
        let mut frame = cifra_como_o_browser(&key, &header, &payload, &[4u8; 12]);
        let n = frame.len();
        frame[n - 1] ^= 0xff;
        assert!(decrypt_e2ee(&key, &frame, 3).is_none());
    }

    #[test]
    fn frames_pequenos_demais_passam_intactos_dos_dois_lados() {
        // Abaixo de header+IV+tag não pode haver payload cifrado. O worker
        // também não os cifra — o importante é que as duas implementações
        // concordem no MESMO limiar, senão uma cifra e a outra não decifra.
        let key = chave_de_teste(7);
        let curto = vec![1u8, 2, 3, 4, 5];
        assert_eq!(decrypt_e2ee(&key, &curto, 3).unwrap(), curto);
    }

    // ------------------------------------------------------------------
    //  Integridade da gravação: a fila de escrita e o fecho
    // ------------------------------------------------------------------
    //
    // A escrita saiu do executor do Tokio para uma thread dedicada. Isso resolve
    // o bloqueio, mas abre a porta ao pior defeito possível numa gravação:
    // fechar o ficheiro ANTES de a fila estar esvaziada dá um vídeo truncado, sem
    // um único erro pelo caminho, e o ffmpeg compõe-no na mesma. É a família da
    // R18 — corrupção silenciosa. Estes testes existem para isso.

    /// Um pacote VP8 mínimo que o `Vp8IvfWriter` aceita como frame completo.
    ///
    /// `0x10` no descritor VP8 = início de partição (bit S). No payload, bit 0
    /// a zero = keyframe. `marker` fecha o frame.
    fn vp8_keyframe(seq: u16, ts: u32) -> webrtc::rtp::packet::Packet {
        let mut header = webrtc::rtp::header::Header {
            sequence_number: seq,
            timestamp: ts,
            marker: true,
            ..Default::default()
        };
        header.payload_type = 96;
        // descritor (S=1) + payload VP8 com bit0=0 e a assinatura de keyframe
        let payload: Vec<u8> = vec![
            0x10, // descritor VP8: S=1
            0x00, 0x00, 0x00, // tag do frame (bit0=0 ⇒ keyframe)
            0x9d, 0x01, 0x2a, // sync de keyframe
            0x40, 0x01, 0xf0, 0x00, // 320x240
            0xde, 0xad, 0xbe, 0xef,
        ];
        webrtc::rtp::packet::Packet {
            header,
            payload: payload.into(),
        }
    }

    /// Lê o contador de frames do cabeçalho IVF (u32 LE no offset 24).
    fn ivf_frame_count(path: &std::path::Path) -> u32 {
        let bytes = std::fs::read(path).expect("ficheiro de gravação");
        assert!(
            bytes.len() >= 32,
            "cabeçalho IVF incompleto: {} bytes",
            bytes.len()
        );
        assert_eq!(&bytes[0..4], b"DKIF", "não é um ficheiro IVF");
        u32::from_le_bytes([bytes[24], bytes[25], bytes[26], bytes[27]])
    }

    fn writer_de_teste(dir: &std::path::Path, cap: usize) -> (RecWriter, std::path::PathBuf) {
        let path = dir.join("teste.ivf");
        let file = std::fs::File::create(&path).unwrap();
        let sink = RecSink::Video(Vp8IvfWriter::new(file).unwrap());
        let w = RecWriter::spawn(
            sink,
            cap,
            Arc::new(crate::metrics::Metrics::default()),
            "teste".into(),
        );
        (w, path)
    }

    #[tokio::test]
    async fn close_espera_a_fila_esvaziar_ate_ao_ultimo_pacote() {
        let dir = std::env::temp_dir().join(format!("dlx-rec-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let (w, path) = writer_de_teste(&dir, 4096);

        const N: u16 = 300;
        for i in 0..N {
            w.write_rtp(&vp8_keyframe(i, i as u32 * 3000));
        }
        // Fecha IMEDIATAMENTE a seguir a enfileirar: a thread quase de certeza
        // ainda tem pacotes por escrever neste instante. Se o `close` não
        // esperasse, o ficheiro sairia truncado — e ninguém daria por isso.
        let perdidos = w.close().await;

        assert_eq!(perdidos, 0, "com fila folgada não se perde nada");
        assert_eq!(
            ivf_frame_count(&path),
            N as u32,
            "o ficheiro tem de conter TODOS os frames enfileirados antes do fecho"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn o_cabecalho_ivf_e_corrigido_no_fecho() {
        // O contador de frames e as dimensões só se sabem no fim: o `close`
        // volta atrás no ficheiro (`seek`) para os escrever. Se o `BufWriter`
        // não esvaziasse antes do `seek`, o cabeçalho ficaria por corrigir.
        let dir = std::env::temp_dir().join(format!("dlx-rec-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let (w, path) = writer_de_teste(&dir, 256);
        for i in 0..10u16 {
            w.write_rtp(&vp8_keyframe(i, i as u32 * 3000));
        }
        w.close().await;

        let bytes = std::fs::read(&path).unwrap();
        assert_eq!(ivf_frame_count(&path), 10);
        // Dimensões reais lidas do keyframe (320x240), não as nominais 1280x720.
        let w_px = u16::from_le_bytes([bytes[12], bytes[13]]);
        let h_px = u16::from_le_bytes([bytes[14], bytes[15]]);
        assert_eq!((w_px, h_px), (320, 240));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn fila_cheia_perde_pacotes_mas_conta_os() {
        // Perder pacotes é aceitável; perdê-los EM SILÊNCIO não é.
        // Um disco que não acompanha tem de ser VISÍVEL. A alternativa —
        // bloquear o executor até ele alcançar — é pior, mas perder em silêncio
        // é o pior de todos: dá um ficheiro que parece bom e não é.
        let dir = std::env::temp_dir().join(format!("dlx-rec-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let (w, path) = writer_de_teste(&dir, 1);

        for i in 0..5_000u16 {
            w.write_rtp(&vp8_keyframe(i, i as u32 * 3000));
        }
        let perdidos = w.close().await;

        assert!(
            perdidos > 0,
            "com fila de 1 e 5000 pacotes, tem de haver perdas"
        );
        // E o que passou continua a ser um ficheiro válido — degradado, não corrompido.
        let escritos = ivf_frame_count(&path);
        assert!(escritos > 0);
        assert_eq!(
            escritos as u64 + perdidos,
            5_000,
            "todo o pacote ou foi escrito ou foi contado"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn write_rtp_nunca_bloqueia_quem_o_chama() {
        // É a razão de existir de toda esta mudança: o `write_rtp` corre dentro
        // da task async que reencaminha RTP. Se bloqueasse, prendia um worker do
        // Tokio — e um worker preso não serve só esta gravação, serve todas as
        // salas que calharem naquela thread.
        let dir = std::env::temp_dir().join(format!("dlx-rec-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let (w, _path) = writer_de_teste(&dir, 1);

        let inicio = std::time::Instant::now();
        for i in 0..20_000u16 {
            w.write_rtp(&vp8_keyframe(i, i as u32 * 3000));
        }
        let decorrido = inicio.elapsed();
        assert!(
            decorrido < Duration::from_secs(5),
            "20 000 escritas com a fila cheia demoraram {decorrido:?} — está a bloquear"
        );
        w.close().await;
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn run_bounded_returns_when_the_process_finishes_in_time() {
        let mut cmd = tokio::process::Command::new("true");
        let st = run_bounded(&mut cmd, Duration::from_secs(30))
            .await
            .expect("devia ter terminado dentro do tecto");
        assert!(st.success());
    }

    #[tokio::test]
    async fn run_bounded_kills_a_process_that_overruns() {
        // O caso que já pendurou uma composição: o processo nunca termina.
        let mut cmd = tokio::process::Command::new("sleep");
        cmd.arg("60");
        let started = std::time::Instant::now();
        let err = run_bounded(&mut cmd, Duration::from_millis(150))
            .await
            .expect_err("tinha de falhar por exceder o tecto");
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "voltou tarde demais — o tecto não está a ser imposto"
        );
        assert!(
            err.to_string().contains("excedeu"),
            "o erro tem de dizer o que aconteceu, e não um código opaco: {err}"
        );
    }

    #[tokio::test]
    async fn run_bounded_reports_a_failing_process_instead_of_hanging() {
        let mut cmd = tokio::process::Command::new("false");
        let st = run_bounded(&mut cmd, Duration::from_secs(30))
            .await
            .unwrap();
        assert!(!st.success(), "um ffmpeg que falha tem de ser visível");
    }
}
