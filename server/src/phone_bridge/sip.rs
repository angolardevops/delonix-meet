//! UA SIP mínimo da ponte (ADR-0010 §3): o FreeSWITCH liga para
//! `sip:room-<sala>@<ponte>` e a ponte atende com o seu endereço RTP no SDP.
//!
//! Porque existe: o FreeSWITCH da plataforma (`delonix-dev/freeswitch:1.11.3`)
//! não tem `mod_rtp`, por isso não sabe mandar RTP cru para um endereço. O
//! `AfterAnswer::RoomBridge` da telefonia (ADR-0009) faz `bridge` para uma perna
//! SIP — e esta é a outra ponta dessa perna.
//!
//! **Só o que a perna precisa**, e nada mais: UDP, um diálogo por chamada,
//! `INVITE` → `100`/`200` com SDP (G.711 lei A ou μ), retransmissão do `200`
//! até ao `ACK` (RFC 3261 §13.3.1.4), `BYE`, `CANCEL`, `OPTIONS`, e re-INVITE
//! respondido com o mesmo SDP. Sem registo, sem autenticação SIP, sem TCP/TLS,
//! sem SRTP: a ponte só fala com os FreeSWITCH da plataforma, na rede interna,
//! e ignora (sem resposta) qualquer outro IP antes de ler o pedido.

use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use tokio::net::UdpSocket;
use tokio::sync::{mpsc, Mutex};
use uuid::Uuid;

use super::g711::Law;
use super::leg::{self, LegConfig, LegEvent, LegHandle};
use crate::sfu::SfuState;

/// Cabeçalho com o id da chamada da telefonia (`delonix_call_id`), posto pelo
/// `originate` na perna para a ponte. É o que liga a perna SIP ao dial-out.
pub const CALL_ID_HEADER: &str = "X-Delonix-Call-Id";

// ============================================================
//  Mensagens SIP
// ============================================================

#[derive(Debug, Clone, PartialEq)]
pub struct SipMessage {
    /// `INVITE sip:room-x@h:p SIP/2.0` ou `SIP/2.0 200 OK`.
    pub start: String,
    pub headers: Vec<(String, String)>,
    pub body: String,
}

/// Forma longa de um cabeçalho compacto (RFC 3261 §7.3.3).
fn long_name(name: &str) -> &str {
    match name {
        "v" => "Via",
        "f" => "From",
        "t" => "To",
        "i" => "Call-ID",
        "m" => "Contact",
        "l" => "Content-Length",
        "c" => "Content-Type",
        "k" => "Supported",
        other => other,
    }
}

impl SipMessage {
    pub fn parse(raw: &str) -> Option<SipMessage> {
        let (head, body) = match raw.find("\r\n\r\n") {
            Some(i) => (&raw[..i], &raw[i + 4..]),
            None => (raw.trim_end(), ""),
        };
        let mut lines = head.split("\r\n");
        let start = lines.next()?.trim().to_string();
        if start.is_empty() {
            return None;
        }
        let mut headers: Vec<(String, String)> = Vec::new();
        for line in lines {
            // Continuação de linha (RFC 3261 §7.3.1).
            if line.starts_with([' ', '\t']) {
                if let Some(last) = headers.last_mut() {
                    last.1.push(' ');
                    last.1.push_str(line.trim());
                }
                continue;
            }
            let (n, v) = line.split_once(':')?;
            headers.push((long_name(n.trim()).to_string(), v.trim().to_string()));
        }
        let len = headers
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case("Content-Length"))
            .and_then(|(_, v)| v.parse::<usize>().ok())
            .unwrap_or(body.len());
        let body = body.get(..len.min(body.len())).unwrap_or("").to_string();
        Some(SipMessage {
            start,
            headers,
            body,
        })
    }

    pub fn method(&self) -> Option<&str> {
        let first = self.start.split(' ').next()?;
        (!first.starts_with("SIP/")).then_some(first)
    }

    pub fn request_uri(&self) -> Option<&str> {
        self.start.split(' ').nth(1)
    }

    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }

    fn all<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a str> + 'a {
        self.headers
            .iter()
            .filter(move |(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }

    /// O `tag` do `To` (vazio antes de haver diálogo).
    pub fn to_tag(&self) -> Option<&str> {
        tag_of(self.header("To")?)
    }
}

fn tag_of(h: &str) -> Option<&str> {
    h.split(';')
        .skip(1)
        .find_map(|p| p.trim().strip_prefix("tag="))
}

/// `sip:room-<sala>@host:port` → `<sala>`. Só letras, dígitos e `-`: é o mesmo
/// alfabeto dos códigos de sala, e tudo o resto é recusado.
pub fn room_code_from_uri(uri: &str) -> Option<String> {
    let user = uri.strip_prefix("sip:")?.split(['@', ';', '>']).next()?;
    let code = user.strip_prefix("room-")?;
    let ok = !code.is_empty()
        && code.len() <= 64
        && code.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-');
    ok.then(|| code.to_ascii_lowercase())
}

/// Resposta a um pedido: copia Via, From, Call-ID, CSeq; To com o nosso tag.
fn response(
    req: &SipMessage,
    status: &str,
    to_tag: Option<&str>,
    contact: Option<&str>,
    sdp: Option<&str>,
) -> String {
    let mut out = format!("SIP/2.0 {status}\r\n");
    for via in req.all("Via") {
        out.push_str(&format!("Via: {via}\r\n"));
    }
    if let Some(f) = req.header("From") {
        out.push_str(&format!("From: {f}\r\n"));
    }
    if let Some(t) = req.header("To") {
        match (to_tag, tag_of(t)) {
            (Some(tag), None) => out.push_str(&format!("To: {t};tag={tag}\r\n")),
            _ => out.push_str(&format!("To: {t}\r\n")),
        }
    }
    for h in ["Call-ID", "CSeq"] {
        if let Some(v) = req.header(h) {
            out.push_str(&format!("{h}: {v}\r\n"));
        }
    }
    if let Some(c) = contact {
        out.push_str(&format!("Contact: {c}\r\n"));
    }
    out.push_str("User-Agent: delonix-meet-bridge\r\n");
    out.push_str("Allow: INVITE, ACK, BYE, CANCEL, OPTIONS\r\n");
    match sdp {
        Some(body) => {
            out.push_str("Content-Type: application/sdp\r\n");
            out.push_str(&format!("Content-Length: {}\r\n\r\n{body}", body.len()));
        }
        None => out.push_str("Content-Length: 0\r\n\r\n"),
    }
    out
}

// ============================================================
//  SDP
// ============================================================

/// O que interessa de uma oferta SDP: onde mandar o RTP e que G.711 usar.
#[derive(Debug, Clone, PartialEq)]
pub struct AudioOffer {
    pub remote: Option<SocketAddr>,
    pub law: Law,
}

/// Lê a oferta. `None` se não houver G.711 (PCMA/PCMU) na linha de áudio —
/// a ponte não transcodifica mais nada, e responde `488`.
pub fn parse_sdp_offer(sdp: &str) -> Option<AudioOffer> {
    let mut session_ip: Option<IpAddr> = None;
    let mut media_ip: Option<IpAddr> = None;
    let mut port: Option<u16> = None;
    let mut pts: Vec<u8> = Vec::new();
    let mut in_audio = false;
    let mut rtpmap: HashMap<u8, String> = HashMap::new();
    for line in sdp.lines().map(str::trim) {
        if let Some(rest) = line.strip_prefix("m=") {
            let mut it = rest.split_whitespace();
            in_audio = it.next() == Some("audio");
            if in_audio && port.is_none() {
                port = it.next().and_then(|p| p.parse().ok());
                let _proto = it.next();
                pts = it.filter_map(|p| p.parse().ok()).collect();
            }
        } else if let Some(rest) = line.strip_prefix("c=") {
            let ip = rest.split_whitespace().nth(2).and_then(|a| a.parse().ok());
            if port.is_some() && in_audio {
                media_ip = ip;
            } else if port.is_none() {
                session_ip = ip;
            }
        } else if let Some(rest) = line.strip_prefix("a=rtpmap:") {
            if in_audio {
                if let Some((pt, enc)) = rest.split_once(' ') {
                    if let Ok(pt) = pt.parse::<u8>() {
                        rtpmap.insert(pt, enc.to_ascii_uppercase());
                    }
                }
            }
        }
    }
    let law = pts.iter().find_map(|pt| match (*pt, rtpmap.get(pt)) {
        (_, Some(enc)) if enc.starts_with("PCMA/") => Some(Law::A),
        (_, Some(enc)) if enc.starts_with("PCMU/") => Some(Law::Mu),
        (8, None) => Some(Law::A),
        (0, None) => Some(Law::Mu),
        _ => None,
    })?;
    let ip = media_ip.or(session_ip);
    let remote = match (ip, port) {
        (Some(ip), Some(p)) if p != 0 => Some(SocketAddr::new(ip, p)),
        _ => None,
    };
    Some(AudioOffer { remote, law })
}

pub fn sdp_answer(local: SocketAddr, law: Law, session: u64) -> String {
    let (name, pt) = match law {
        Law::A => ("PCMA", 8),
        Law::Mu => ("PCMU", 0),
    };
    let family = if local.is_ipv4() { "IP4" } else { "IP6" };
    let ip = local.ip();
    format!(
        "v=0\r\no=delonix {session} {session} IN {family} {ip}\r\ns=delonix-bridge\r\n\
         c=IN {family} {ip}\r\nt=0 0\r\nm=audio {port} RTP/AVP {pt}\r\n\
         a=rtpmap:{pt} {name}/8000\r\na=ptime:20\r\na=sendrecv\r\n",
        port = local.port()
    )
}

// ============================================================
//  O UA
// ============================================================

/// Quem decide se uma chamada entra, e em que sala. Implementado pelos canais
/// da sala (`room_channels`); nos testes, por um mapa.
#[async_trait]
pub trait BridgeAdmission: Send + Sync {
    /// A sala `room_code` (id do SFU/sinalização) e o id da perna. `None`
    /// recusa com `404` — sala inexistente ou chamada que ninguém pediu.
    async fn admit(&self, room_code: &str, call_id: Option<Uuid>) -> Option<Admitted>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Admitted {
    pub room_id: Uuid,
    /// Id da perna: é o `peer_id` com que a chamada aparece na sala.
    pub leg_id: Uuid,
}

/// O que acontece às pernas SIP, para quem as põe na sala.
#[derive(Debug, Clone, PartialEq)]
pub enum BridgeEvent {
    /// `200 OK` enviado: a media já pode circular.
    Started {
        leg_id: Uuid,
        room_id: Uuid,
        room_code: String,
        call_id: Option<Uuid>,
    },
    Leg {
        leg_id: Uuid,
        room_id: Uuid,
        event: LegEvent,
    },
    /// `BYE`/`CANCEL` recebido, ou a ponte fechou a perna.
    Ended { leg_id: Uuid, room_id: Uuid },
}

#[derive(Debug, Clone)]
pub struct SipBridgeConfig {
    /// Onde o UA escuta SIP (UDP).
    pub sip_bind: SocketAddr,
    /// IP anunciado no SDP e onde as pernas abrem o RTP.
    pub rtp_ip: IpAddr,
    /// Portas RTP, inclusive. `None` = efémera do SO.
    pub rtp_ports: Option<(u16, u16)>,
    /// IPs dos FreeSWITCH. Vazio recusa tudo.
    pub allowed_sources: Vec<IpAddr>,
}

struct Dialog {
    leg_id: Uuid,
    room_id: Uuid,
    to_tag: String,
    last_response: String,
    acked: Arc<std::sync::atomic::AtomicBool>,
    leg: Option<LegHandle>,
}

pub struct SipBridge {
    cfg: SipBridgeConfig,
    pub local_sip: SocketAddr,
    dialogs: Mutex<HashMap<String, Dialog>>,
    socket: Arc<UdpSocket>,
}

impl SipBridge {
    /// Arranca o UA. Os eventos saem em `events`.
    pub async fn start(
        cfg: SipBridgeConfig,
        sfu: Arc<SfuState>,
        admission: Arc<dyn BridgeAdmission>,
        events: mpsc::Sender<BridgeEvent>,
    ) -> std::io::Result<Arc<SipBridge>> {
        let socket = Arc::new(UdpSocket::bind(cfg.sip_bind).await?);
        let local_sip = socket.local_addr()?;
        let bridge = Arc::new(SipBridge {
            cfg,
            local_sip,
            dialogs: Mutex::new(HashMap::new()),
            socket: socket.clone(),
        });
        tracing::info!(%local_sip, "ponte: UA SIP à escuta");
        let b = bridge.clone();
        tokio::spawn(async move {
            let mut buf = vec![0u8; 8192];
            loop {
                let Ok((n, from)) = socket.recv_from(&mut buf).await else {
                    break;
                };
                if !b.cfg.allowed_sources.contains(&from.ip()) {
                    // Nem se responde: não se confirma a quem varre portas
                    // que aqui há um UA.
                    tracing::warn!(%from, "ponte: SIP de origem não autorizada — ignorado");
                    continue;
                }
                let Ok(text) = std::str::from_utf8(&buf[..n]) else {
                    continue;
                };
                let Some(msg) = SipMessage::parse(text) else {
                    continue;
                };
                b.handle(msg, from, &sfu, admission.as_ref(), &events).await;
            }
        });
        Ok(bridge)
    }

    /// Silencia (na ponte) o áudio de uma perna. `false` se não existe.
    pub async fn set_muted(&self, leg_id: Uuid, muted: bool) -> bool {
        let dialogs = self.dialogs.lock().await;
        for d in dialogs.values() {
            if d.leg_id == leg_id {
                if let Some(l) = &d.leg {
                    l.set_muted(muted);
                    return true;
                }
            }
        }
        false
    }

    /// Pernas activas (para métricas e testes).
    pub async fn active_legs(&self) -> usize {
        self.dialogs
            .lock()
            .await
            .values()
            .filter(|d| d.leg.is_some())
            .count()
    }

    async fn send(&self, to: SocketAddr, text: &str) {
        let _ = self.socket.send_to(text.as_bytes(), to).await;
    }

    async fn handle(
        &self,
        msg: SipMessage,
        from: SocketAddr,
        sfu: &Arc<SfuState>,
        admission: &dyn BridgeAdmission,
        events: &mpsc::Sender<BridgeEvent>,
    ) {
        let Some(method) = msg.method().map(str::to_string) else {
            return; // respostas: o UA nunca faz pedidos
        };
        let Some(call_id) = msg.header("Call-ID").map(str::to_string) else {
            return;
        };
        match method.as_str() {
            "INVITE" => {
                self.invite(msg, from, call_id, sfu, admission, events)
                    .await
            }
            "ACK" => {
                if let Some(d) = self.dialogs.lock().await.get(&call_id) {
                    d.acked.store(true, std::sync::atomic::Ordering::SeqCst);
                }
            }
            "BYE" | "CANCEL" => {
                let d = self.dialogs.lock().await.remove(&call_id);
                match d {
                    Some(mut d) => {
                        d.acked.store(true, std::sync::atomic::Ordering::SeqCst);
                        self.send(from, &response(&msg, "200 OK", Some(&d.to_tag), None, None))
                            .await;
                        if let Some(l) = d.leg.take() {
                            l.stop().await;
                        }
                        let _ = events
                            .send(BridgeEvent::Ended {
                                leg_id: d.leg_id,
                                room_id: d.room_id,
                            })
                            .await;
                    }
                    None => {
                        self.send(
                            from,
                            &response(
                                &msg,
                                "481 Call/Transaction Does Not Exist",
                                None,
                                None,
                                None,
                            ),
                        )
                        .await
                    }
                }
            }
            "OPTIONS" => {
                self.send(from, &response(&msg, "200 OK", None, None, None))
                    .await
            }
            _ => {
                self.send(
                    from,
                    &response(&msg, "405 Method Not Allowed", None, None, None),
                )
                .await
            }
        }
    }

    async fn invite(
        &self,
        msg: SipMessage,
        from: SocketAddr,
        call_id: String,
        sfu: &Arc<SfuState>,
        admission: &dyn BridgeAdmission,
        events: &mpsc::Sender<BridgeEvent>,
    ) {
        // Retransmissão do INVITE, ou re-INVITE no mesmo diálogo: a mesma
        // resposta (o SDP não muda — a perna é a mesma).
        if let Some(d) = self.dialogs.lock().await.get(&call_id) {
            let again = if msg.to_tag().is_some() {
                let sdp = d.last_response.split("\r\n\r\n").nth(1).unwrap_or("");
                response(
                    &msg,
                    "200 OK",
                    Some(&d.to_tag),
                    Some(&self.contact()),
                    Some(sdp),
                )
            } else {
                d.last_response.clone()
            };
            self.send(from, &again).await;
            return;
        }
        self.send(from, &response(&msg, "100 Trying", None, None, None))
            .await;
        let reject = |status: &'static str| response(&msg, status, None, None, None);
        let Some(room_code) = msg.request_uri().and_then(room_code_from_uri) else {
            self.send(from, &reject("404 Not Found")).await;
            return;
        };
        let Some(offer) = parse_sdp_offer(&msg.body) else {
            self.send(from, &reject("488 Not Acceptable Here")).await;
            return;
        };
        let delonix_call = msg
            .header(CALL_ID_HEADER)
            .and_then(|v| Uuid::parse_str(v.trim()).ok());
        let Some(adm) = admission.admit(&room_code, delonix_call).await else {
            self.send(from, &reject("404 Not Found")).await;
            return;
        };
        let (leg_tx, mut leg_rx) = mpsc::channel(16);
        let leg = match self
            .open_leg(sfu, adm, offer.clone(), from.ip(), leg_tx)
            .await
        {
            Ok(l) => l,
            Err(e) => {
                tracing::warn!(error = %e, "ponte: sem porta RTP para a perna");
                self.send(from, &reject("503 Service Unavailable")).await;
                return;
            }
        };
        let to_tag = format!("{:x}", rand::random::<u64>());
        let sdp = sdp_answer(leg.local_addr, offer.law, rand::random::<u32>() as u64);
        let ok = response(
            &msg,
            "200 OK",
            Some(&to_tag),
            Some(&self.contact()),
            Some(&sdp),
        );
        let acked = Arc::new(std::sync::atomic::AtomicBool::new(false));
        self.dialogs.lock().await.insert(
            call_id,
            Dialog {
                leg_id: adm.leg_id,
                room_id: adm.room_id,
                to_tag,
                last_response: ok.clone(),
                acked: acked.clone(),
                leg: Some(leg),
            },
        );
        self.send(from, &ok).await;
        // Retransmite o 200 até ao ACK: T1 = 500 ms, dobra até T2 = 4 s,
        // desiste aos 32 s (RFC 3261 §13.3.1.4).
        {
            let socket = self.socket.clone();
            tokio::spawn(async move {
                let mut wait = Duration::from_millis(500);
                let mut total = Duration::ZERO;
                while total < Duration::from_secs(32) {
                    tokio::time::sleep(wait).await;
                    total += wait;
                    if acked.load(std::sync::atomic::Ordering::SeqCst) {
                        return;
                    }
                    let _ = socket.send_to(ok.as_bytes(), from).await;
                    wait = (wait * 2).min(Duration::from_secs(4));
                }
            });
        }
        let _ = events
            .send(BridgeEvent::Started {
                leg_id: adm.leg_id,
                room_id: adm.room_id,
                room_code,
                call_id: delonix_call,
            })
            .await;
        let ev = events.clone();
        tokio::spawn(async move {
            while let Some(event) = leg_rx.recv().await {
                let _ = ev
                    .send(BridgeEvent::Leg {
                        leg_id: adm.leg_id,
                        room_id: adm.room_id,
                        event,
                    })
                    .await;
            }
        });
    }

    fn contact(&self) -> String {
        format!("<sip:bridge@{}>", self.local_sip)
    }

    async fn open_leg(
        &self,
        sfu: &Arc<SfuState>,
        adm: Admitted,
        offer: AudioOffer,
        source: IpAddr,
        events: mpsc::Sender<LegEvent>,
    ) -> std::io::Result<LegHandle> {
        let ports: Vec<u16> = match self.cfg.rtp_ports {
            Some((a, b)) => (a..=b).collect(),
            None => vec![0],
        };
        let mut last = std::io::Error::other("sem portas RTP configuradas");
        for port in ports {
            let cfg = LegConfig {
                room_id: adm.room_id,
                leg_id: adm.leg_id,
                bind: SocketAddr::new(self.cfg.rtp_ip, port),
                // O RTP vem do FreeSWITCH que mandou o INVITE (e do IP do SDP,
                // se for outro da mesma lista).
                allowed_sources: {
                    let mut v = vec![source];
                    if let Some(r) = offer.remote {
                        if self.cfg.allowed_sources.contains(&r.ip()) && r.ip() != source {
                            v.push(r.ip());
                        }
                    }
                    v
                },
                default_law: offer.law,
                initial_remote: offer.remote,
            };
            match leg::start(sfu.clone(), cfg, events.clone()).await {
                Ok(l) => return Ok(l),
                Err(e) if e.kind() == std::io::ErrorKind::AddrInUse => last = e,
                Err(e) => return Err(e),
            }
        }
        Err(last)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const INVITE_FS: &str = "INVITE sip:room-voz-arq-2026@127.0.0.1:5290 SIP/2.0\r\n\
Via: SIP/2.0/UDP 127.0.0.1:5280;rport;branch=z9hG4bKabc\r\n\
Max-Forwards: 69\r\n\
From: \"+244923447108\" <sip:+244923447108@127.0.0.1>;tag=Fr0m\r\n\
To: <sip:room-voz-arq-2026@127.0.0.1:5290>\r\n\
Call-ID: 1a2b3c\r\n\
CSeq: 12345 INVITE\r\n\
X-Delonix-Call-Id: 7a0ad0a0-5c55-4b2e-9d3e-1d7c9c2b5e11\r\n\
Content-Type: application/sdp\r\n\
Content-Length: 223\r\n\
\r\n\
v=0\r\n\
o=FreeSWITCH 1 2 IN IP4 127.0.0.1\r\n\
s=FreeSWITCH\r\n\
c=IN IP4 127.0.0.1\r\n\
t=0 0\r\n\
m=audio 32810 RTP/AVP 8 101\r\n\
a=rtpmap:8 PCMA/8000\r\n\
a=rtpmap:101 telephone-event/8000\r\n\
a=fmtp:101 0-16\r\n\
a=ptime:20\r\n\
a=sendrecv\r\n";

    #[test]
    fn le_o_invite_do_freeswitch() {
        let m = SipMessage::parse(INVITE_FS).unwrap();
        assert_eq!(m.method(), Some("INVITE"));
        assert_eq!(
            room_code_from_uri(m.request_uri().unwrap()).as_deref(),
            Some("voz-arq-2026")
        );
        assert_eq!(m.header("call-id"), Some("1a2b3c"));
        assert_eq!(
            m.header(CALL_ID_HEADER),
            Some("7a0ad0a0-5c55-4b2e-9d3e-1d7c9c2b5e11")
        );
        let offer = parse_sdp_offer(&m.body).unwrap();
        assert_eq!(offer.law, Law::A);
        assert_eq!(offer.remote, Some("127.0.0.1:32810".parse().unwrap()));
    }

    #[test]
    fn cabecalhos_compactos_e_continuacao() {
        let raw = "BYE sip:bridge@x SIP/2.0\r\nv: SIP/2.0/UDP a;branch=z\r\ni: abc\r\nf: <sip:a@b>;tag=1\r\n t2\r\nl: 0\r\n\r\n";
        let m = SipMessage::parse(raw).unwrap();
        assert_eq!(m.header("Call-ID"), Some("abc"));
        assert_eq!(m.header("From"), Some("<sip:a@b>;tag=1 t2"));
    }

    #[test]
    fn so_aceita_codigos_de_sala_limpos() {
        assert_eq!(
            room_code_from_uri("sip:room-abc-123@h"),
            Some("abc-123".into())
        );
        assert_eq!(
            room_code_from_uri("sip:room-ABC@h;transport=udp"),
            Some("abc".into())
        );
        assert_eq!(room_code_from_uri("sip:room-@h"), None);
        assert_eq!(room_code_from_uri("sip:1000@h"), None);
        assert_eq!(room_code_from_uri("sip:room-a%20b@h"), None);
        assert_eq!(room_code_from_uri("sip:room-a'or'1@h"), None);
    }

    #[test]
    fn sem_g711_nao_ha_oferta() {
        let sdp =
            "v=0\r\nc=IN IP4 1.2.3.4\r\nm=audio 4000 RTP/AVP 111\r\na=rtpmap:111 opus/48000/2\r\n";
        assert!(parse_sdp_offer(sdp).is_none());
        // PT estático sem rtpmap conta; prefere-se a ordem da oferta.
        let sdp = "v=0\r\nc=IN IP4 1.2.3.4\r\nm=audio 4000 RTP/AVP 0 8\r\n";
        assert_eq!(parse_sdp_offer(sdp).unwrap().law, Law::Mu);
    }

    #[test]
    fn resposta_200_mantem_o_dialogo_e_leva_o_sdp() {
        let m = SipMessage::parse(INVITE_FS).unwrap();
        let sdp = sdp_answer("127.0.0.1:32900".parse().unwrap(), Law::A, 7);
        let r = response(
            &m,
            "200 OK",
            Some("T0"),
            Some("<sip:bridge@127.0.0.1:5290>"),
            Some(&sdp),
        );
        let back = SipMessage::parse(&r).unwrap();
        assert_eq!(back.start, "SIP/2.0 200 OK");
        assert_eq!(
            back.header("Via"),
            Some("SIP/2.0/UDP 127.0.0.1:5280;rport;branch=z9hG4bKabc")
        );
        assert_eq!(back.to_tag(), Some("T0"));
        assert_eq!(back.header("CSeq"), Some("12345 INVITE"));
        assert_eq!(
            back.header("Content-Length")
                .unwrap()
                .parse::<usize>()
                .unwrap(),
            sdp.len()
        );
        let answer = parse_sdp_offer(&back.body).unwrap();
        assert_eq!(answer.remote, Some("127.0.0.1:32900".parse().unwrap()));
        assert_eq!(answer.law, Law::A);
    }
}
