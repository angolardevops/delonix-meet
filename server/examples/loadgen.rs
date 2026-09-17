//! Gerador de carga do Delonix Meet — clientes WebRTC REAIS contra um servidor real.
//!
//! Porque existe: «quantas chamadas aguenta esta máquina» não se responde com
//! o `sfu_e2e.rs` (liga o SFU por chamadas directas, sem HTTP nem `/ws`) nem a
//! abrir browsers à mão. Aqui cada participante faz o caminho completo que o
//! browser faz: `POST /api/rooms/{code}/join` → room token → `/ws` → `joined`
//! → oferta SFU com **simulcast q/h/f** + Opus → trickle ICE → media a sério,
//! cifrada em SRTP, nos dois sentidos.
//!
//! A media vem de ficheiros pré-codificados (IVF VP8 por camada e OGG Opus),
//! lidos para memória e repetidos em ciclo. Nada é codificado em tempo real:
//! o custo do gerador fica no DTLS/SRTP/RTP — o mesmo trabalho que o servidor
//! faz — e não no encoder, que num browser correria no dispositivo do cliente.
//!
//! O que mede, por janela: fluxos de vídeo esperados vs activos, perda por
//! buracos de sequência, jitter RFC 3550, débito recebido, CPU e RSS do
//! servidor (por /proc) e — para não confundir saturação do GERADOR com
//! saturação do servidor — os ticks de envio atrasados do próprio gerador.
//!
//! Media (uma vez, em `.carga/media`):
//!
//! ```text
//! enc(){ ffmpeg -y -f lavfi -i "testsrc2=size=$1:rate=30,noise=alls=12:allf=t" -t 60 \
//!   -c:v libvpx -deadline realtime -cpu-used 8 -b:v $2 -minrate $2 -maxrate $2 \
//!   -g 60 -keyint_min 60 -error-resilient 1 -auto-alt-ref 0 -lag-in-frames 0 -f ivf $3; }
//! enc 320x180 150k q.ivf; enc 640x360 500k h.ivf; enc 1280x720 1700k f.ivf
//! ffmpeg -y -f lavfi -i "sine=frequency=440:sample_rate=48000" -t 60 -ac 1 \
//!   -c:a libopus -b:a 32k -frame_duration 20 -page_duration 20000 audio.ogg
//! ```
//!
//! Uso e resultados medidos: `docs/ops/teste-de-carga-2026-09-17.md`, ou `--help`.

use std::{
    net::IpAddr,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};

use bytes::Bytes;
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message;
use webrtc::{
    api::{
        interceptor_registry::register_default_interceptors, media_engine::MediaEngine,
        setting_engine::SettingEngine, APIBuilder, API,
    },
    ice::network_type::NetworkType,
    ice_transport::ice_candidate::RTCIceCandidateInit,
    interceptor::registry::Registry,
    media::{
        io::{ivf_reader::IVFReader, ogg_reader::OggReader},
        Sample,
    },
    peer_connection::{
        configuration::RTCConfiguration, peer_connection_state::RTCPeerConnectionState,
        sdp::session_description::RTCSessionDescription, RTCPeerConnection,
    },
    rtp_transceiver::{
        rtp_codec::{RTCRtpCodecCapability, RTCRtpHeaderExtensionCapability, RTPCodecType},
        rtp_transceiver_direction::RTCRtpTransceiverDirection,
        RTCRtpTransceiverInit,
    },
    track::track_local::{track_local_static_sample::TrackLocalStaticSample, TrackLocal},
};

// ---------------------------------------------------------------- argumentos

#[derive(Clone, Debug)]
struct Args {
    api: String,
    rooms: usize,
    per_room: usize,
    media: PathBuf,
    client_ip: IpAddr,
    join_gap_ms: u64,
    settle_secs: u64,
    duration_secs: u64,
    sample_secs: u64,
    record_rooms: usize,
    post_secs: u64,
    server_pid: Option<u32>,
    simulcast: bool,
    label: String,
    out: Option<PathBuf>,
}

fn parse_args() -> Args {
    let mut a = Args {
        api: "http://127.0.0.1:8280".into(),
        rooms: 1,
        per_room: 4,
        media: PathBuf::from(".carga/media"),
        client_ip: "192.168.90.1".parse().unwrap(),
        join_gap_ms: 40,
        settle_secs: 10,
        duration_secs: 30,
        sample_secs: 5,
        record_rooms: 0,
        post_secs: 0,
        server_pid: None,
        simulcast: true,
        label: "run".into(),
        out: None,
    };
    let mut it = std::env::args().skip(1);
    while let Some(k) = it.next() {
        let mut v = || it.next().unwrap_or_else(|| panic!("{k} precisa de valor"));
        match k.as_str() {
            "--api" => a.api = v(),
            "--rooms" => a.rooms = v().parse().unwrap(),
            "--per-room" => a.per_room = v().parse().unwrap(),
            "--media" => a.media = v().into(),
            "--client-ip" => a.client_ip = v().parse().unwrap(),
            "--join-gap-ms" => a.join_gap_ms = v().parse().unwrap(),
            "--settle" => a.settle_secs = v().parse().unwrap(),
            "--duration" => a.duration_secs = v().parse().unwrap(),
            "--sample" => a.sample_secs = v().parse().unwrap(),
            "--record-rooms" => a.record_rooms = v().parse().unwrap(),
            "--post" => a.post_secs = v().parse().unwrap(),
            "--server-pid" => a.server_pid = Some(v().parse().unwrap()),
            "--no-simulcast" => a.simulcast = false,
            "--label" => a.label = v(),
            "--out" => a.out = Some(v().into()),
            "--help" | "-h" => {
                eprintln!(
                    "loadgen --api URL --rooms N --per-room P [--media DIR] [--client-ip IP]\n\
                     [--join-gap-ms 40] [--settle 10] [--duration 30] [--sample 5]\n\
                     [--record-rooms K --post S] [--server-pid PID] [--no-simulcast]\n\
                     [--label TXT] [--out resultados.jsonl]"
                );
                std::process::exit(0);
            }
            other => panic!("argumento desconhecido: {other}"),
        }
    }
    a
}

// ---------------------------------------------------------------- media

struct Media {
    /// Camadas por ordem q, h, f. Cada frame: (dados, é keyframe).
    layers: Vec<Vec<Bytes>>,
    audio: Vec<Bytes>,
}

fn load_ivf(p: &Path) -> anyhow::Result<Vec<Bytes>> {
    let f = std::io::BufReader::new(std::fs::File::open(p)?);
    let (mut r, _h) = IVFReader::new(f)?;
    let mut v = Vec::new();
    while let Ok((frame, _)) = r.parse_next_frame() {
        v.push(frame.freeze());
    }
    anyhow::ensure!(!v.is_empty(), "{} sem frames", p.display());
    Ok(v)
}

fn load_ogg(p: &Path) -> anyhow::Result<Vec<Bytes>> {
    let f = std::io::BufReader::new(std::fs::File::open(p)?);
    let (mut r, _h) = OggReader::new(f, true)?;
    let mut v = Vec::new();
    while let Ok((page, _)) = r.parse_next_page() {
        if page.starts_with(b"OpusHead") || page.starts_with(b"OpusTags") {
            continue;
        }
        v.push(page.freeze());
    }
    anyhow::ensure!(!v.is_empty(), "{} sem páginas", p.display());
    Ok(v)
}

// ---------------------------------------------------------------- estatística

#[derive(Default)]
struct StreamStat {
    video: bool,
    packets: u64,
    bytes: u64,
    lost: u64,
    last_seq: Option<u16>,
    jitter: f64, // em unidades de relógio RTP
    last_transit: Option<f64>,
    first_at: Option<Instant>,
}

#[derive(Default)]
struct Global {
    streams: Mutex<Vec<Arc<Mutex<StreamStat>>>>,
    ws_ok: AtomicU64,
    ws_fail: AtomicU64,
    pc_connected: AtomicU64,
    pc_failed: AtomicU64,
    server_errors: AtomicU64,
    ticks: AtomicU64,
    late_ticks: AtomicU64,
    /// ms desde o arranque do cliente até ao 1.º RTP de vídeo recebido.
    first_media_ms: Mutex<Vec<u64>>,
    stop: AtomicBool,
    /// Diagnóstico por cliente: (idx, sala, peer_id, vídeo recebido, ofertas do
    /// servidor, respostas do servidor, erros de sinalização).
    clients: Mutex<Vec<Arc<ClientDiag>>>,
}

#[derive(Default)]
struct ClientDiag {
    idx: usize,
    room: usize,
    peer_id: Mutex<String>,
    video_tracks: AtomicU64,
    srv_offers: AtomicU64,
    srv_answers: AtomicU64,
    nego_errors: Mutex<Vec<String>>,
    /// Da última oferta do servidor: m-lines de vídeo que ENVIAM (sendonly/
    /// sendrecv) e têm `a=ssrc` — o que o SFU diz que nos está a mandar.
    last_offer_video_send: AtomicU64,
}

impl Global {
    fn stopped(&self) -> bool {
        self.stop.load(Ordering::Relaxed)
    }
}

// ---------------------------------------------------------------- HTTP

async fn post(
    http: &reqwest::Client,
    url: String,
    token: Option<&str>,
    body: Value,
) -> anyhow::Result<Value> {
    let mut req = http.post(url).json(&body);
    if let Some(t) = token {
        req = req.bearer_auth(t);
    }
    let res = req.send().await?;
    let st = res.status();
    let txt = res.text().await?;
    anyhow::ensure!(st.is_success(), "HTTP {st}: {txt}");
    Ok(serde_json::from_str(&txt).unwrap_or(Value::Null))
}

// ---------------------------------------------------------------- cliente

fn build_api(client_ip: IpAddr) -> anyhow::Result<API> {
    let mut m = MediaEngine::default();
    m.register_default_codecs()?;
    m.register_header_extension(
        RTCRtpHeaderExtensionCapability {
            uri: "urn:ietf:params:rtp-hdrext:ssrc-audio-level".into(),
        },
        RTPCodecType::Audio,
        None,
    )?;
    for uri in [
        "urn:ietf:params:rtp-hdrext:sdes:mid",
        "urn:ietf:params:rtp-hdrext:sdes:rtp-stream-id",
        "urn:ietf:params:rtp-hdrext:sdes:repaired-rtp-stream-id",
    ] {
        m.register_header_extension(
            RTCRtpHeaderExtensionCapability { uri: uri.into() },
            RTPCodecType::Video,
            None,
        )?;
    }
    let mut reg = Registry::new();
    reg = register_default_interceptors(reg, &mut m)?;
    let mut s = SettingEngine::default();
    // Um só candidato por cliente. Sem isto cada PeerConnection do gerador
    // juntava um candidato por interface desta máquina (bridges docker, virbr…)
    // e as verificações ICE multiplicavam-se — carga que um browser real não dá.
    s.set_ip_filter(Box::new(move |ip| ip == client_ip));
    s.set_network_types(vec![NetworkType::Udp4]);
    Ok(APIBuilder::new()
        .with_media_engine(m)
        .with_interceptor_registry(reg)
        .with_setting_engine(s)
        .build())
}

type WsTx = mpsc::UnboundedSender<String>;

#[allow(clippy::too_many_arguments)]
async fn run_client(
    idx: usize,
    api: Arc<API>,
    http: reqwest::Client,
    args: Args,
    access: String,
    code: String,
    media: Arc<Media>,
    g: Arc<Global>,
    recorder_slot: Option<Arc<Mutex<Option<WsTx>>>>,
) -> anyhow::Result<()> {
    let started = Instant::now();
    let diag = Arc::new(ClientDiag {
        idx,
        room: idx / args.per_room,
        ..Default::default()
    });
    g.clients.lock().unwrap().push(diag.clone());
    let join = post(
        &http,
        format!("{}/api/rooms/{code}/join", args.api),
        Some(&access),
        json!({}),
    )
    .await?;
    let token = join["room_token"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("sem room_token: {join}"))?
        .to_string();
    let ws_url = format!(
        "{}/ws?token={token}",
        args.api.replacen("http", "ws", 1)
    );
    let (ws, _) = match tokio_tungstenite::connect_async(&ws_url).await {
        Ok(x) => x,
        Err(e) => {
            g.ws_fail.fetch_add(1, Ordering::Relaxed);
            return Err(e.into());
        }
    };
    g.ws_ok.fetch_add(1, Ordering::Relaxed);
    let (mut sink, mut stream) = ws.split();
    let (tx, mut rx) = mpsc::unbounded_channel::<String>();
    tokio::spawn(async move {
        while let Some(t) = rx.recv().await {
            if sink.send(Message::Text(t)).await.is_err() {
                break;
            }
        }
    });
    if let Some(slot) = &recorder_slot {
        *slot.lock().unwrap() = Some(tx.clone());
    }

    // Espera pelo `joined` antes de criar a PeerConnection — é o que o
    // `callHolder` do Room.tsx faz (R13: a oferta nasce depois do joined).
    loop {
        match stream.next().await {
            Some(Ok(Message::Text(t))) => {
                let v: Value = serde_json::from_str(&t).unwrap_or(Value::Null);
                if v["type"] == "joined" {
                    *diag.peer_id.lock().unwrap() = v["peer_id"].as_str().unwrap_or("").into();
                    break;
                }
                if v["type"] == "waiting" || v["type"] == "denied" {
                    anyhow::bail!("cliente {idx} ficou em espera/recusado");
                }
            }
            Some(Ok(_)) => {}
            _ => anyhow::bail!("ws fechou antes do joined"),
        }
    }

    let pc: Arc<RTCPeerConnection> =
        Arc::new(api.new_peer_connection(RTCConfiguration::default()).await?);

    {
        let g = g.clone();
        pc.on_peer_connection_state_change(Box::new(move |s| {
            match s {
                RTCPeerConnectionState::Connected => {
                    g.pc_connected.fetch_add(1, Ordering::Relaxed);
                }
                RTCPeerConnectionState::Failed => {
                    g.pc_failed.fetch_add(1, Ordering::Relaxed);
                }
                _ => {}
            }
            Box::pin(async {})
        }));
    }
    {
        let tx = tx.clone();
        pc.on_ice_candidate(Box::new(move |c| {
            let tx = tx.clone();
            Box::pin(async move {
                if let Some(c) = c {
                    if let Ok(init) = c.to_json() {
                        let _ = tx.send(json!({"type":"sfu-ice","candidate": init}).to_string());
                    }
                }
            })
        }));
    }
    {
        let g = g.clone();
        let first_video = Arc::new(AtomicBool::new(false));
        let diag = diag.clone();
        pc.on_track(Box::new(move |remote, _r, _t| {
            let g = g.clone();
            let first_video = first_video.clone();
            let diag = diag.clone();
            Box::pin(async move {
                let video = remote.kind() == RTPCodecType::Video;
                if video {
                    diag.video_tracks.fetch_add(1, Ordering::Relaxed);
                }
                let clock = if video { 90_000.0 } else { 48_000.0 };
                let st = Arc::new(Mutex::new(StreamStat {
                    video,
                    ..Default::default()
                }));
                g.streams.lock().unwrap().push(st.clone());
                tokio::spawn(async move {
                    let t0 = Instant::now();
                    while let Ok((pkt, _)) = remote.read_rtp().await {
                        let now = Instant::now();
                        if video && !first_video.swap(true, Ordering::Relaxed) {
                            g.first_media_ms
                                .lock()
                                .unwrap()
                                .push(started.elapsed().as_millis() as u64);
                        }
                        let mut s = st.lock().unwrap();
                        s.first_at.get_or_insert(now);
                        s.packets += 1;
                        s.bytes += pkt.payload.len() as u64;
                        let seq = pkt.header.sequence_number;
                        if let Some(last) = s.last_seq {
                            let d = seq.wrapping_sub(last);
                            if (2..3000).contains(&d) {
                                s.lost += (d - 1) as u64;
                            }
                            if d < 0x8000 {
                                s.last_seq = Some(seq);
                            }
                        } else {
                            s.last_seq = Some(seq);
                        }
                        let arrival = now.duration_since(t0).as_secs_f64() * clock;
                        let transit = arrival - pkt.header.timestamp as f64;
                        if let Some(prev) = s.last_transit {
                            let d = (transit - prev).abs();
                            // Um salto enorme é troca de camada/SSRC, não jitter.
                            if d < clock {
                                s.jitter += (d - s.jitter) / 16.0;
                            }
                        }
                        s.last_transit = Some(transit);
                    }
                });
            })
        }));
    }

    // --- publicar: áudio + vídeo (simulcast q/h/f como o Chrome) ---
    let stream_id = format!("carga-{idx}");
    let opus = Arc::new(TrackLocalStaticSample::new(
        RTCRtpCodecCapability {
            mime_type: "audio/opus".into(),
            clock_rate: 48000,
            channels: 2,
            ..Default::default()
        },
        "audio".into(),
        stream_id.clone(),
    ));
    let audio_sender = pc
        .add_transceiver_from_track(
            opus.clone() as Arc<dyn TrackLocal + Send + Sync>,
            Some(RTCRtpTransceiverInit {
                direction: RTCRtpTransceiverDirection::Sendrecv,
                send_encodings: vec![],
            }),
        )
        .await?
        .sender()
        .await;
    let vcap = RTCRtpCodecCapability {
        mime_type: "video/VP8".into(),
        clock_rate: 90000,
        ..Default::default()
    };
    let rids: &[&str] = if args.simulcast { &["q", "h", "f"] } else { &["f"] };
    let vtracks: Vec<Arc<TrackLocalStaticSample>> = rids
        .iter()
        .map(|rid| {
            if args.simulcast {
                Arc::new(TrackLocalStaticSample::new_with_rid(
                    vcap.clone(),
                    "video".into(),
                    (*rid).into(),
                    stream_id.clone(),
                ))
            } else {
                Arc::new(TrackLocalStaticSample::new(
                    vcap.clone(),
                    "video".into(),
                    stream_id.clone(),
                ))
            }
        })
        .collect();
    let video_sender = pc
        .add_transceiver_from_track(
            vtracks[0].clone() as Arc<dyn TrackLocal + Send + Sync>,
            Some(RTCRtpTransceiverInit {
                direction: RTCRtpTransceiverDirection::Sendrecv,
                send_encodings: vec![],
            }),
        )
        .await?
        .sender()
        .await;
    for t in &vtracks[1..] {
        video_sender
            .add_encoding(t.clone() as Arc<dyn TrackLocal + Send + Sync>)
            .await?;
    }
    // RTCP de volta (PLI/NACK/REMB) tem de ser lido para os interceptors o tratarem.
    for s in [audio_sender, video_sender] {
        let g = g.clone();
        tokio::spawn(async move {
            let mut buf = vec![0u8; 1500];
            while !g.stopped() && s.read(&mut buf).await.is_ok() {}
        });
    }

    let offer = pc.create_offer(None).await?;
    pc.set_local_description(offer).await?;
    let sdp = pc.local_description().await.unwrap().sdp;
    tx.send(json!({"type":"sfu-offer","sdp": sdp}).to_string())?;

    // --- bomba de media: vídeo a 30 fps, áudio a 50 pps ---
    {
        let g = g.clone();
        let media = media.clone();
        let vtracks = vtracks.clone();
        let simulcast = args.simulcast;
        tokio::spawn(async move {
            let mut iv = tokio::time::interval(Duration::from_micros(33_333));
            iv.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            let mut i = idx * 7; // desfasa os keyframes entre clientes
            let mut last = Instant::now();
            while !g.stopped() {
                iv.tick().await;
                let now = Instant::now();
                g.ticks.fetch_add(1, Ordering::Relaxed);
                if now.duration_since(last) > Duration::from_millis(50) {
                    g.late_ticks.fetch_add(1, Ordering::Relaxed);
                }
                last = now;
                for (li, t) in vtracks.iter().enumerate() {
                    let layer = if simulcast { &media.layers[li] } else { &media.layers[2] };
                    let data = layer[i % layer.len()].clone();
                    let _ = t
                        .write_sample(&Sample {
                            data,
                            duration: Duration::from_micros(33_333),
                            ..Default::default()
                        })
                        .await;
                }
                i += 1;
            }
        });
    }
    {
        let g = g.clone();
        let media = media.clone();
        tokio::spawn(async move {
            let mut iv = tokio::time::interval(Duration::from_millis(20));
            iv.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            let mut i = 0usize;
            while !g.stopped() {
                iv.tick().await;
                let data = media.audio[i % media.audio.len()].clone();
                let _ = opus
                    .write_sample(&Sample {
                        data,
                        duration: Duration::from_millis(20),
                        ..Default::default()
                    })
                    .await;
                i += 1;
            }
        });
    }

    // --- sinalização SFU ---
    let mut pending: Vec<RTCIceCandidateInit> = Vec::new();
    while !g.stopped() {
        let msg = tokio::select! {
            m = stream.next() => m,
            _ = tokio::time::sleep(Duration::from_millis(500)) => continue,
        };
        let t = match msg {
            Some(Ok(Message::Text(t))) => t,
            Some(Ok(_)) => continue,
            _ => break,
        };
        let v: Value = match serde_json::from_str(&t) {
            Ok(v) => v,
            Err(_) => continue,
        };
        match v["type"].as_str().unwrap_or("") {
            "sfu-answer" => {
                diag.srv_answers.fetch_add(1, Ordering::Relaxed);
                let sdp = v["sdp"].as_str().unwrap_or("").to_string();
                if let Err(e) = pc
                    .set_remote_description(RTCSessionDescription::answer(sdp)?)
                    .await
                {
                    let st = pc.signaling_state();
                    diag.nego_errors.lock().unwrap().push(format!("answer em {st}: {e}"));
                    continue;
                }
                for c in pending.drain(..) {
                    let _ = pc.add_ice_candidate(c).await;
                }
            }
            "sfu-offer" => {
                diag.srv_offers.fetch_add(1, Ordering::Relaxed);
                let sdp = v["sdp"].as_str().unwrap_or("").to_string();
                let n = sdp
                    .split("\nm=")
                    .skip(1)
                    .filter(|m| {
                        m.starts_with("video")
                            && (m.contains("a=sendonly") || m.contains("a=sendrecv"))
                            && m.contains("a=ssrc:")
                    })
                    .count();
                diag.last_offer_video_send.store(n as u64, Ordering::Relaxed);
                if let Err(e) = pc
                    .set_remote_description(RTCSessionDescription::offer(sdp)?)
                    .await
                {
                    let st = pc.signaling_state();
                    diag.nego_errors.lock().unwrap().push(format!("offer em {st}: {e}"));
                    continue;
                }
                let ans = pc.create_answer(None).await?;
                pc.set_local_description(ans).await?;
                let sdp = pc.local_description().await.unwrap().sdp;
                tx.send(json!({"type":"sfu-answer","sdp": sdp}).to_string())?;
                for c in pending.drain(..) {
                    let _ = pc.add_ice_candidate(c).await;
                }
            }
            "sfu-ice" => {
                if let Ok(c) = serde_json::from_value::<RTCIceCandidateInit>(v["candidate"].clone())
                {
                    if pc.remote_description().await.is_some() {
                        let _ = pc.add_ice_candidate(c).await;
                    } else {
                        pending.push(c);
                    }
                }
            }
            "error" => {
                g.server_errors.fetch_add(1, Ordering::Relaxed);
                eprintln!("[{idx}] erro do servidor: {v}");
            }
            _ => {}
        }
    }
    let _ = pc.close().await;
    Ok(())
}

// ---------------------------------------------------------------- /proc

fn proc_cpu_ticks(pid: u32) -> Option<u64> {
    let s = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let rest = &s[s.rfind(')')? + 2..];
    let f: Vec<&str> = rest.split_whitespace().collect();
    // campos 14,15 (utime, stime) → índices 11,12 depois do estado
    Some(f.get(11)?.parse::<u64>().ok()? + f.get(12)?.parse::<u64>().ok()?)
}

fn proc_rss_mb(pid: u32) -> Option<f64> {
    let s = std::fs::read_to_string(format!("/proc/{pid}/status")).ok()?;
    let l = s.lines().find(|l| l.starts_with("VmRSS:"))?;
    Some(l.split_whitespace().nth(1)?.parse::<f64>().ok()? / 1024.0)
}

/// Filhos `ffmpeg` do servidor: (quantos, ticks de CPU somados).
fn ffmpeg_children(server: u32) -> (usize, u64) {
    let mut n = 0;
    let mut ticks = 0;
    if let Ok(rd) = std::fs::read_dir("/proc") {
        for e in rd.flatten() {
            let Ok(pid) = e.file_name().to_string_lossy().parse::<u32>() else {
                continue;
            };
            let Ok(stat) = std::fs::read_to_string(format!("/proc/{pid}/stat")) else {
                continue;
            };
            if !stat.contains("(ffmpeg)") {
                continue;
            }
            let rest = &stat[stat.rfind(')').unwrap() + 2..];
            let f: Vec<&str> = rest.split_whitespace().collect();
            if f.get(1).and_then(|p| p.parse::<u32>().ok()) == Some(server) {
                n += 1;
                ticks += proc_cpu_ticks(pid).unwrap_or(0);
            }
        }
    }
    (n, ticks)
}

/// CPU agregada da máquina inteira (0–1) — para mostrar quanto do ruído vem
/// de outros processos que não o servidor nem o gerador.
fn system_busy() -> Option<(u64, u64)> {
    let s = std::fs::read_to_string("/proc/stat").ok()?;
    let l = s.lines().next()?;
    let v: Vec<u64> = l
        .split_whitespace()
        .skip(1)
        .filter_map(|x| x.parse().ok())
        .collect();
    let idle = v.get(3)? + v.get(4)?;
    let total: u64 = v.iter().take(8).sum();
    Some((total - idle, total))
}

// ---------------------------------------------------------------- main

#[derive(Default, Clone, Copy)]
struct Snap {
    v_packets: u64,
    v_bytes: u64,
    v_lost: u64,
    a_packets: u64,
    a_lost: u64,
}

fn snapshot(g: &Global) -> (Snap, Vec<(u64, f64)>) {
    let streams = g.streams.lock().unwrap().clone();
    let mut s = Snap::default();
    let mut per = Vec::with_capacity(streams.len());
    for st in streams {
        let x = st.lock().unwrap();
        if x.video {
            s.v_packets += x.packets;
            s.v_bytes += x.bytes;
            s.v_lost += x.lost;
            per.push((x.packets, x.jitter / 90.0));
        } else {
            s.a_packets += x.packets;
            s.a_lost += x.lost;
        }
    }
    (s, per)
}

fn pct(v: &mut [u64], p: f64) -> u64 {
    if v.is_empty() {
        return 0;
    }
    v.sort_unstable();
    v[((v.len() as f64 - 1.0) * p).round() as usize]
}

#[tokio::main(flavor = "multi_thread")]
async fn main() -> anyhow::Result<()> {
    let args = parse_args();
    let media = Arc::new(Media {
        layers: vec![
            load_ivf(&args.media.join("q.ivf"))?,
            load_ivf(&args.media.join("h.ivf"))?,
            load_ivf(&args.media.join("f.ivf"))?,
        ],
        audio: load_ogg(&args.media.join("audio.ogg"))?,
    });
    let http = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()?;

    // Uma conta por corrida, dona de todas as salas: o dono entra direto (sem
    // sala de espera), e cada ligação `/ws` é um peer distinto.
    let tag = &uuid::Uuid::new_v4().simple().to_string()[..10];
    let reg = post(
        &http,
        format!("{}/api/auth/register", args.api),
        None,
        json!({
            "org_name": format!("Carga {tag}"),
            "email": format!("carga@{tag}.teste.local"),
            "username": format!("carga-{tag}"),
            "password": "carga-password-123",
        }),
    )
    .await?;
    let access = reg["access_token"].as_str().unwrap().to_string();

    let mut codes = Vec::new();
    for r in 0..args.rooms {
        let room = post(
            &http,
            format!("{}/api/rooms", args.api),
            Some(&access),
            json!({"name": format!("carga {r}"), "topology": "sfu"}),
        )
        .await?;
        codes.push(room["code"].as_str().unwrap().to_string());
    }

    let api = Arc::new(build_api(args.client_ip)?);
    let g = Arc::new(Global::default());
    let total = args.rooms * args.per_room;
    let expected_video = args.rooms * args.per_room * (args.per_room - 1);
    let recorder_slots: Vec<Arc<Mutex<Option<WsTx>>>> = (0..args.rooms)
        .map(|_| Arc::new(Mutex::new(None)))
        .collect();

    eprintln!(
        "[{}] {} salas × {} = {} participantes; fluxos de vídeo esperados: {}",
        args.label, args.rooms, args.per_room, total, expected_video
    );
    let join_t0 = Instant::now();
    let mut handles = Vec::new();
    let mut client_fail = 0usize;
    for p in 0..args.per_room {
        for (r, code) in codes.iter().enumerate() {
            let idx = r * args.per_room + p;
            let slot = (p == 0).then(|| recorder_slots[r].clone());
            handles.push(tokio::spawn({
                let (api, http, args, access, code, media, g) = (
                    api.clone(),
                    http.clone(),
                    args.clone(),
                    access.clone(),
                    code.clone(),
                    media.clone(),
                    g.clone(),
                );
                async move {
                    if let Err(e) =
                        run_client(idx, api, http, args, access, code, media, g, slot).await
                    {
                        eprintln!("[cliente {idx}] {e:#}");
                        return false;
                    }
                    true
                }
            }));
            tokio::time::sleep(Duration::from_millis(args.join_gap_ms)).await;
        }
    }
    let join_secs = join_t0.elapsed().as_secs_f64();
    tokio::time::sleep(Duration::from_secs(args.settle_secs)).await;

    if args.record_rooms > 0 {
        for s in recorder_slots.iter().take(args.record_rooms) {
            if let Some(tx) = s.lock().unwrap().as_ref() {
                let _ = tx.send(json!({"type":"server-record","active":true}).to_string());
            }
        }
        eprintln!("[{}] gravação no servidor ligada em {} salas", args.label, args.record_rooms);
    }

    let tck = 100.0; // CLK_TCK
    let mut rows = Vec::new();
    let mut prev = snapshot(&g);
    let mut prev_cpu = args.server_pid.and_then(proc_cpu_ticks).unwrap_or(0);
    let mut prev_self = proc_cpu_ticks(std::process::id()).unwrap_or(0);
    let mut prev_sys = system_busy().unwrap_or((0, 1));
    let mut prev_ticks = (g.ticks.load(Ordering::Relaxed), g.late_ticks.load(Ordering::Relaxed));
    let windows = args.duration_secs.div_ceil(args.sample_secs);
    let mut ff_prev = args.server_pid.map(ffmpeg_children).unwrap_or((0, 0)).1;
    let mut phase = "carga";
    let mut post_started: Option<Instant> = None;
    let mut ffmpeg_peak = 0usize;
    let mut w = 0u64;
    loop {
        if phase == "carga" && w >= windows {
            if args.record_rooms == 0 {
                break;
            }
            for s in recorder_slots.iter().take(args.record_rooms) {
                if let Some(tx) = s.lock().unwrap().as_ref() {
                    let _ = tx.send(json!({"type":"server-record","active":false}).to_string());
                }
            }
            phase = "composição";
            post_started = Some(Instant::now());
            eprintln!("[{}] gravação parada → ffmpeg a compor com as chamadas a decorrer", args.label);
        }
        if phase == "composição" {
            let el = post_started.unwrap().elapsed().as_secs();
            let (n, _) = args.server_pid.map(ffmpeg_children).unwrap_or((0, 0));
            if el >= args.post_secs || (el > 10 && n == 0) {
                break;
            }
        }
        tokio::time::sleep(Duration::from_secs(args.sample_secs)).await;
        w += 1;
        let cur = snapshot(&g);
        let dt = args.sample_secs as f64;
        let active = cur
            .1
            .iter()
            .enumerate()
            .filter(|(i, (p, _))| *p > prev.1.get(*i).map(|x| x.0).unwrap_or(0))
            .count();
        let mut jit: Vec<u64> = cur.1.iter().map(|(_, j)| (*j * 10.0) as u64).collect();
        let jit_p50 = pct(&mut jit, 0.5) as f64 / 10.0;
        let jit_p95 = pct(&mut jit, 0.95) as f64 / 10.0;
        let dvp = cur.0.v_packets - prev.0.v_packets;
        let dvl = cur.0.v_lost - prev.0.v_lost;
        let loss = if dvp + dvl > 0 { dvl as f64 / (dvp + dvl) as f64 * 100.0 } else { 0.0 };
        let dap = cur.0.a_packets - prev.0.a_packets;
        let dal = cur.0.a_lost - prev.0.a_lost;
        let aloss = if dap + dal > 0 { dal as f64 / (dap + dal) as f64 * 100.0 } else { 0.0 };
        let mbps = (cur.0.v_bytes - prev.0.v_bytes) as f64 * 8.0 / dt / 1e6;
        let cpu_now = args.server_pid.and_then(proc_cpu_ticks).unwrap_or(0);
        let srv_cores = (cpu_now - prev_cpu) as f64 / tck / dt;
        let self_now = proc_cpu_ticks(std::process::id()).unwrap_or(0);
        let gen_cores = (self_now - prev_self) as f64 / tck / dt;
        let (ffn, ff_ticks) = args.server_pid.map(ffmpeg_children).unwrap_or((0, 0));
        ffmpeg_peak = ffmpeg_peak.max(ffn);
        let ff_cores = ff_ticks.saturating_sub(ff_prev) as f64 / tck / dt;
        let sys = system_busy().unwrap_or((0, 1));
        let sys_busy = (sys.0 - prev_sys.0) as f64 / ((sys.1 - prev_sys.1).max(1)) as f64 * 100.0;
        let rss = args.server_pid.and_then(proc_rss_mb).unwrap_or(0.0);
        let ticks = (g.ticks.load(Ordering::Relaxed), g.late_ticks.load(Ordering::Relaxed));
        let late = if ticks.0 > prev_ticks.0 {
            (ticks.1 - prev_ticks.1) as f64 / (ticks.0 - prev_ticks.0) as f64 * 100.0
        } else {
            0.0
        };
        let row = json!({
            "fase": phase, "janela": w,
            "video_ativos": active, "video_esperados": expected_video,
            "perda_video_pct": (loss * 100.0).round() / 100.0,
            "perda_audio_pct": (aloss * 100.0).round() / 100.0,
            "jitter_p50_ms": jit_p50, "jitter_p95_ms": jit_p95,
            "video_mbps": (mbps * 10.0).round() / 10.0,
            "srv_cores": (srv_cores * 100.0).round() / 100.0,
            "srv_rss_mb": rss.round(),
            "ffmpeg_proc": ffn, "ffmpeg_cores": (ff_cores * 100.0).round() / 100.0,
            "gerador_cores": (gen_cores * 100.0).round() / 100.0,
            "gerador_ticks_atrasados_pct": (late * 10.0).round() / 10.0,
            "maquina_ocupada_pct": sys_busy.round(),
        });
        eprintln!("[{}] {}", args.label, row);
        rows.push(row);
        prev = cur;
        prev_cpu = cpu_now;
        prev_self = self_now;
        prev_sys = sys;
        prev_ticks = ticks;
        ff_prev = ff_ticks;
    }
    let composition_secs = post_started.map(|t| t.elapsed().as_secs_f64());

    g.stop.store(true, Ordering::Relaxed);
    for h in handles {
        if !h.await.unwrap_or(false) {
            client_fail += 1;
        }
    }

    let carga: Vec<&Value> = rows.iter().filter(|r| r["fase"] == "carga").collect();
    let comp: Vec<&Value> = rows.iter().filter(|r| r["fase"] == "composição").collect();
    let avg = |rs: &[&Value], k: &str| -> f64 {
        if rs.is_empty() {
            return 0.0;
        }
        let v = rs.iter().map(|r| r[k].as_f64().unwrap_or(0.0)).sum::<f64>() / rs.len() as f64;
        (v * 100.0).round() / 100.0
    };
    let maxf = |rs: &[&Value], k: &str| -> f64 {
        rs.iter().map(|r| r[k].as_f64().unwrap_or(0.0)).fold(0.0, f64::max)
    };
    let minf = |rs: &[&Value], k: &str| -> f64 {
        rs.iter().map(|r| r[k].as_f64().unwrap_or(0.0)).fold(f64::MAX, f64::min)
    };
    // Descarta a 1.ª janela de carga (arranque de keyframes/camadas) na média.
    let steady: Vec<&Value> = if carga.len() > 2 { carga[1..].to_vec() } else { carga.clone() };
    let mut fm = g.first_media_ms.lock().unwrap().clone();
    let summary = json!({
        "label": args.label,
        "salas": args.rooms, "por_sala": args.per_room, "participantes": total,
        "simulcast": args.simulcast,
        "video_esperados": expected_video,
        "video_ativos_min": minf(&steady, "video_ativos"),
        "video_ativos_media": avg(&steady, "video_ativos"),
        "perda_video_pct": avg(&steady, "perda_video_pct"),
        "perda_video_pct_max": maxf(&steady, "perda_video_pct"),
        "perda_audio_pct": avg(&steady, "perda_audio_pct"),
        "jitter_p95_ms": avg(&steady, "jitter_p95_ms"),
        "video_mbps": avg(&steady, "video_mbps"),
        "srv_cores": avg(&steady, "srv_cores"),
        "srv_cores_max": maxf(&steady, "srv_cores"),
        "srv_rss_mb": maxf(&steady, "srv_rss_mb"),
        "gerador_cores": avg(&steady, "gerador_cores"),
        "gerador_ticks_atrasados_pct": avg(&steady, "gerador_ticks_atrasados_pct"),
        "maquina_ocupada_pct": avg(&steady, "maquina_ocupada_pct"),
        "ws_ok": g.ws_ok.load(Ordering::Relaxed),
        "ws_falhas": g.ws_fail.load(Ordering::Relaxed),
        "pc_ligadas": g.pc_connected.load(Ordering::Relaxed),
        "pc_falhadas": g.pc_failed.load(Ordering::Relaxed),
        "clientes_com_erro": client_fail,
        "erros_servidor": g.server_errors.load(Ordering::Relaxed),
        "entrada_s": (join_secs * 10.0).round() / 10.0,
        "primeiro_video_ms_p50": pct(&mut fm, 0.5),
        "primeiro_video_ms_p95": pct(&mut fm, 0.95),
        "gravacao_salas": args.record_rooms,
        "clientes_incompletos": g.clients.lock().unwrap().iter()
            .filter(|c| (c.video_tracks.load(Ordering::Relaxed) as usize) < args.per_room - 1)
            .map(|c| json!({
                "idx": c.idx, "sala": c.room, "peer_id": *c.peer_id.lock().unwrap(),
                "video_tracks": c.video_tracks.load(Ordering::Relaxed),
                "ofertas_srv": c.srv_offers.load(Ordering::Relaxed),
                "respostas_srv": c.srv_answers.load(Ordering::Relaxed),
                "oferta_srv_video_a_enviar": c.last_offer_video_send.load(Ordering::Relaxed),
                "erros_nego": c.nego_errors.lock().unwrap().clone(),
            }))
            .collect::<Vec<_>>(),
        "composicao": if args.record_rooms > 0 { json!({
            "duracao_s": composition_secs.map(|s| s.round()),
            "ffmpeg_pico": ffmpeg_peak,
            "ffmpeg_cores": avg(&comp, "ffmpeg_cores"),
            "srv_cores": avg(&comp, "srv_cores"),
            "perda_video_pct": avg(&comp, "perda_video_pct"),
            "perda_video_pct_max": maxf(&comp, "perda_video_pct"),
            "jitter_p95_ms": avg(&comp, "jitter_p95_ms"),
            "video_ativos_min": minf(&comp, "video_ativos"),
        }) } else { Value::Null },
        "janelas": rows,
    });
    println!("{}", serde_json::to_string(&summary)?);
    if let Some(out) = &args.out {
        use std::io::Write;
        let mut f = std::fs::OpenOptions::new().create(true).append(true).open(out)?;
        writeln!(f, "{}", serde_json::to_string(&summary)?)?;
    }
    // Não esperar que todas as tasks webrtc fechem com graça.
    std::process::exit(0);
}
