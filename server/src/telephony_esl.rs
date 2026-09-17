//! Adaptadores REAIS das portas `SipControl` e `CallOriginator` (ADR-0009 §4):
//! o Event Socket (ESL, modo *inbound*) do FreeSWITCH e o JSON-RPC do Kamailio.
//!
//! # Protocolo ESL (o que se usa)
//!
//! ```text
//! ← Content-Type: auth/request\n\n
//! → auth <password>\n\n
//! ← Content-Type: command/reply\nReply-Text: +OK accepted\n\n
//! → api <comando>\n\n
//! ← Content-Type: api/response\nContent-Length: N\n\n<corpo>
//! ```
//!
//! Uma ligação por operação: o `originate` bloqueia até ao atendimento (ou à
//! recusa), e partilhar a ligação prenderia o estado do registo atrás de uma
//! chamada a tocar. Os comandos são montados só com valores que nós validamos
//! (dígitos, UUIDs, códigos de sala `[a-z0-9-]`): nenhum texto do cliente chega
//! cru ao ESL, e um `\n` num valor é recusado antes de escrever.
//!
//! # O que está provado e o que não
//!
//! Provado nos testes contra um servidor ESL FALSO (`tests/telephony.rs`): o
//! enquadramento das mensagens, a autenticação, a leitura do corpo por
//! `Content-Length`, a interpretação de `+OK`/`-ERR`, o tempo esgotado e a
//! ligação recusada. **Não provado contra um FreeSWITCH real** (não havia
//! nenhum nesta máquina): a forma exacta do XML de `sofia xmlstatus gateway`
//! na versão instalada, e o comportamento do `originate` com failover `|` e
//! `origination_uuid` — ver ADR-0009 §EXTERNAL.

use std::{collections::HashMap, sync::Arc, time::Duration};

use async_trait::async_trait;
use chrono::Utc;
use delonix_meet_domain::telephony::{
    cost::RegistrationState,
    ports::{
        AfterAnswer, CallEvent, CallEventSink, CallOriginator, GatewayStatus, MediaServerStatus,
        OriginateOutcome, OriginateRequest, PortError, SbcStatus, SipControl, SipSnapshot,
    },
};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    net::TcpStream,
    time::{timeout, Instant},
};
use uuid::Uuid;

use crate::net_guard::Outbound;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(3);
const COMMAND_TIMEOUT: Duration = Duration::from_secs(4);
/// Um corpo ESL maior do que isto é tratado como protocolo avariado.
const MAX_BODY: usize = 1024 * 1024;

#[derive(Clone)]
pub struct EslConfig {
    pub addr: String,
    pub password: String,
    pub sofia_profile: String,
}

pub struct EslConn {
    reader: BufReader<tokio::net::tcp::OwnedReadHalf>,
    writer: tokio::net::tcp::OwnedWriteHalf,
}

fn unavailable(e: impl std::fmt::Display) -> PortError {
    PortError::Unavailable(e.to_string())
}

impl EslConn {
    pub async fn connect(cfg: &EslConfig) -> Result<Self, PortError> {
        let stream = timeout(CONNECT_TIMEOUT, TcpStream::connect(&cfg.addr))
            .await
            .map_err(|_| unavailable(format!("{}: tempo de ligação esgotado", cfg.addr)))?
            .map_err(|e| unavailable(format!("{}: {e}", cfg.addr)))?;
        let (r, w) = stream.into_split();
        let mut conn = Self {
            reader: BufReader::new(r),
            writer: w,
        };
        let (h, _) = conn.read_message(COMMAND_TIMEOUT).await?;
        if h.get("content-type").map(String::as_str) != Some("auth/request") {
            return Err(PortError::Protocol(
                "o servidor ESL não pediu autenticação".into(),
            ));
        }
        conn.send(&format!("auth {}", cfg.password)).await?;
        let (h, _) = conn.read_message(COMMAND_TIMEOUT).await?;
        let reply = h.get("reply-text").cloned().unwrap_or_default();
        if !reply.starts_with("+OK") {
            return Err(PortError::Rejected(
                "o ESL recusou a password (TELEPHONY_ESL_PASSWORD)".into(),
            ));
        }
        Ok(conn)
    }

    async fn send(&mut self, line: &str) -> Result<(), PortError> {
        if line.contains('\n') || line.contains('\r') {
            return Err(PortError::Protocol(
                "comando ESL com quebra de linha recusado".into(),
            ));
        }
        self.writer
            .write_all(format!("{line}\n\n").as_bytes())
            .await
            .map_err(unavailable)
    }

    /// Uma linha de comando com cabeçalhos extra (`bgapi … \nJob-UUID: …`).
    async fn send_with_headers(
        &mut self,
        line: &str,
        headers: &[(&str, String)],
    ) -> Result<(), PortError> {
        let mut msg = String::new();
        for part in std::iter::once(line.to_string())
            .chain(headers.iter().map(|(k, v)| format!("{k}: {v}")))
        {
            if part.contains('\n') || part.contains('\r') {
                return Err(PortError::Protocol(
                    "comando ESL com quebra de linha recusado".into(),
                ));
            }
            msg.push_str(&part);
            msg.push('\n');
        }
        msg.push('\n');
        self.writer
            .write_all(msg.as_bytes())
            .await
            .map_err(unavailable)
    }

    /// Comando de sessão (`event`, `filter`, `bgapi`): espera o `command/reply`.
    async fn command(
        &mut self,
        line: &str,
        headers: &[(&str, String)],
    ) -> Result<String, PortError> {
        self.send_with_headers(line, headers).await?;
        loop {
            let (h, _) = self.read_message(COMMAND_TIMEOUT).await?;
            if h.get("content-type").map(String::as_str) == Some("command/reply") {
                let reply = h.get("reply-text").cloned().unwrap_or_default();
                if reply.starts_with("-ERR") {
                    return Err(PortError::Rejected(format!("{line}: {reply}")));
                }
                return Ok(reply);
            }
        }
    }

    /// Lê uma mensagem: cabeçalhos até à linha vazia e, com `Content-Length`,
    /// o corpo. Cabeçalhos em minúsculas.
    async fn read_message(
        &mut self,
        limit: Duration,
    ) -> Result<(HashMap<String, String>, String), PortError> {
        timeout(limit, async {
            let mut headers = HashMap::new();
            loop {
                let mut line = String::new();
                let n = self
                    .reader
                    .read_line(&mut line)
                    .await
                    .map_err(unavailable)?;
                if n == 0 {
                    return Err(unavailable("o ESL fechou a ligação"));
                }
                let line = line.trim_end_matches(['\n', '\r']);
                if line.is_empty() {
                    if headers.is_empty() {
                        continue;
                    }
                    break;
                }
                if let Some((k, v)) = line.split_once(':') {
                    headers.insert(k.trim().to_ascii_lowercase(), v.trim().to_string());
                }
            }
            let mut body = String::new();
            if let Some(len) = headers.get("content-length") {
                let len: usize = len
                    .parse()
                    .map_err(|_| PortError::Protocol("Content-Length inválido".into()))?;
                if len > MAX_BODY {
                    return Err(PortError::Protocol("corpo ESL demasiado grande".into()));
                }
                let mut buf = vec![0u8; len];
                self.reader
                    .read_exact(&mut buf)
                    .await
                    .map_err(unavailable)?;
                body = String::from_utf8_lossy(&buf).into_owned();
            }
            Ok((headers, body))
        })
        .await
        .map_err(|_| unavailable("tempo de resposta do ESL esgotado"))?
    }

    /// `api <cmd>` → corpo da resposta. Mensagens de outros tipos (eventos,
    /// avisos de desligar) são ignoradas até chegar a `api/response`.
    pub async fn api(&mut self, cmd: &str, limit: Duration) -> Result<String, PortError> {
        self.send(&format!("api {cmd}")).await?;
        let deadline = Instant::now() + limit;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Err(unavailable("tempo de resposta do ESL esgotado"));
            }
            let (h, body) = self.read_message(left).await?;
            match h.get("content-type").map(String::as_str) {
                Some("api/response") => return Ok(body),
                Some("text/disconnect-notice") => {
                    return Err(unavailable("o ESL desligou a sessão"))
                }
                _ => continue,
            }
        }
    }
}

// ============================================================
//  Interpretação das respostas (puras, testadas em unidade)
// ============================================================

/// `FreeSWITCH Version 1.10.11-release+git~… (…)` → `1.10.11-release+git~…`.
pub fn parse_version(body: &str) -> Option<String> {
    let rest = body.split("Version").nth(1)?.trim();
    rest.split_whitespace().next().map(str::to_string)
}

/// `N total.` (de `show channels count`).
pub fn parse_total(body: &str) -> Option<u32> {
    body.split_whitespace()
        .zip(body.split_whitespace().skip(1))
        .find(|(_, w)| w.starts_with("total"))
        .and_then(|(n, _)| n.parse().ok())
}

fn xml_tag<'a>(xml: &'a str, tag: &str) -> Option<&'a str> {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let start = xml.find(&open)? + open.len();
    let end = xml[start..].find(&close)? + start;
    Some(xml[start..end].trim())
}

/// `sofia xmlstatus gateway <nome>` → registo, `UP/DOWN` e tempo de ping.
pub fn parse_gateway_xml(name: &str, body: &str) -> GatewayStatus {
    if body.contains("Invalid Gateway") || !body.contains("<gateway") {
        return GatewayStatus {
            name: name.to_string(),
            registration: RegistrationState::Unknown,
            up: None,
            ping_ms: None,
            channels_in_use: None,
        };
    }
    let registration = match xml_tag(body, "state").unwrap_or("") {
        "REGED" => RegistrationState::Registered,
        "NOREG" => RegistrationState::NotRequired,
        "FAILED" | "FAIL_WAIT" | "EXPIRED" | "NOAVAIL" | "TIMEOUT" => RegistrationState::Failed,
        _ => RegistrationState::Trying,
    };
    let up = match xml_tag(body, "status") {
        Some("UP") => Some(true),
        Some("DOWN") => Some(false),
        _ => None,
    };
    let ping_ms = xml_tag(body, "pingtime")
        .or_else(|| xml_tag(body, "ping-time"))
        .and_then(|v| v.parse::<f64>().ok());
    GatewayStatus {
        name: name.to_string(),
        registration,
        up,
        ping_ms,
        channels_in_use: None,
    }
}

/// Um valor que vai para dentro de um comando ESL: só o alfabeto dado.
fn safe(value: &str, extra: &[u8]) -> Result<String, PortError> {
    if !value.is_empty()
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || extra.contains(&b))
    {
        Ok(value.to_string())
    } else {
        Err(PortError::Protocol(format!(
            "valor recusado para o ESL: «{value}»"
        )))
    }
}

/// Monta o comando `originate` (sem o prefixo `api`). Público para os testes.
pub fn originate_command(
    req: &OriginateRequest,
    bridge_profile: &str,
) -> Result<String, PortError> {
    if req.legs.is_empty() {
        return Err(PortError::Protocol("originate sem troncos".into()));
    }
    // `origination_uuid` NÃO vai nas variáveis globais: com failover (`|`) a
    // segunda tentativa reutilizava o uuid e falhava com
    // DESTINATION_OUT_OF_ORDER — medido contra o FreeSWITCH 1.11.3. Vai só na
    // primeira perna; as seguintes seguem-se por `delonix_call_id`.
    let mut vars = vec![
        format!(
            "originate_timeout={}",
            req.answer_timeout_secs.clamp(5, 120)
        ),
        "ignore_early_media=true".to_string(),
        format!("delonix_org_id={}", req.org_id),
        format!("delonix_call_id={}", req.call_id),
        format!("delonix_record={}", req.record && !req.emergency),
        format!("delonix_emergency={}", req.emergency),
    ];
    if let Some(p) = req.rule_position {
        vars.push(format!("delonix_rule_position={p}"));
    }
    if let Some(cid) = &req.caller_id {
        vars.push(format!("origination_caller_id_number={}", safe(cid, b"+")?));
    }
    match &req.after_answer {
        AfterAnswer::Conference { room_code } | AfterAnswer::RoomBridge { room_code, .. } => {
            vars.push(format!("delonix_room_code={}", safe(room_code, b"-")?));
        }
        AfterAnswer::TestTone { .. } => {}
    }
    if req.record && !req.emergency {
        vars.push(format!(
            "execute_on_answer='record_session ${{recordings_dir}}/delonix-{}.wav'",
            req.call_id
        ));
    }
    let legs: Vec<String> = req
        .legs
        .iter()
        .enumerate()
        .map(|(i, l)| {
            Ok(format!(
                "[{}delonix_trunk_id={}]sofia/gateway/{}/{}",
                if i == 0 {
                    format!("origination_uuid={},", req.call_id)
                } else {
                    String::new()
                },
                l.trunk_id,
                safe(&l.gateway_name, b"-")?,
                safe(&l.number, b"")?
            ))
        })
        .collect::<Result<_, PortError>>()?;
    let app = match &req.after_answer {
        AfterAnswer::TestTone { secs } => format!(
            "&playback(tone_stream://%(1000,0,440);loops={})",
            (*secs).clamp(1, 30)
        ),
        AfterAnswer::Conference { room_code } => {
            format!("&conference({}@delonix)", safe(room_code, b"-")?)
        }
        AfterAnswer::RoomBridge {
            room_code,
            bridge_host,
            bridge_port,
            codec,
        } => {
            let host = match bridge_host {
                std::net::IpAddr::V4(v4) => v4.to_string(),
                std::net::IpAddr::V6(v6) => format!("[{v6}]"),
            };
            let codec = match codec {
                Some(c) => format!("absolute_codec_string={},", safe(c, b"@")?),
                None => String::new(),
            };
            format!(
                "&bridge([{codec}delonix_room_code={room},delonix_leg=room_bridge,delonix_cdr_skip=true]sofia/{profile}/room-{room}@{host}:{bridge_port})",
                room = safe(room_code, b"-")?,
                profile = safe(bridge_profile, b"-_")?,
            )
        }
    };
    Ok(format!(
        "originate {{{}}}{} {app}",
        vars.join(","),
        legs.join("|")
    ))
}

/// Resposta do `originate`: `+OK <uuid>` ou `-ERR <CAUSA>`.
pub fn parse_originate(body: &str) -> Result<(bool, Option<String>), PortError> {
    let b = body.trim();
    if b.starts_with("+OK") {
        Ok((true, None))
    } else if let Some(cause) = b.strip_prefix("-ERR") {
        let cause = cause.trim();
        Ok((
            false,
            Some(if cause.is_empty() {
                "UNKNOWN".to_string()
            } else {
                cause
                    .split_whitespace()
                    .next()
                    .unwrap_or("UNKNOWN")
                    .to_string()
            }),
        ))
    } else {
        Err(PortError::Protocol(format!(
            "resposta inesperada ao originate: «{}»",
            b.chars().take(80).collect::<String>()
        )))
    }
}

// ============================================================
//  Kamailio (SBC) — JSON-RPC
// ============================================================

#[derive(Clone)]
pub struct KamailioRpc {
    pub url: String,
    pub outbound: Outbound,
}

impl KamailioRpc {
    async fn call(&self, method: &str) -> Result<serde_json::Value, PortError> {
        // URL do operador (ambiente): a guarda de saída aplica-se na mesma.
        self.outbound
            .check_operator_url(&self.url)
            .await
            .map_err(|e| PortError::Rejected(e.to_string()))?;
        let client = self.outbound.operator();
        let body = serde_json::json!({"jsonrpc": "2.0", "method": method, "id": 1});
        let res = client
            .post(&self.url)
            .timeout(COMMAND_TIMEOUT)
            .json(&body)
            .send()
            .await
            .map_err(unavailable)?;
        if !res.status().is_success() {
            return Err(PortError::Rejected(format!(
                "Kamailio respondeu {}",
                res.status()
            )));
        }
        let v: serde_json::Value = res
            .json()
            .await
            .map_err(|e| PortError::Protocol(e.to_string()))?;
        if let Some(err) = v.get("error") {
            return Err(PortError::Rejected(err.to_string()));
        }
        v.get("result")
            .cloned()
            .ok_or_else(|| PortError::Protocol("JSON-RPC sem result".into()))
    }

    pub async fn sbc_status(&self) -> Result<SbcStatus, PortError> {
        let version = self.call("core.version").await?;
        let text = version
            .as_str()
            .map(str::to_string)
            .or_else(|| {
                version
                    .get("Server")
                    .and_then(|s| s.as_str())
                    .map(str::to_string)
            })
            .unwrap_or_default();
        // `kamailio 5.7.4 (x86_64/linux)`
        let version = text
            .split_whitespace()
            .nth(1)
            .map(str::to_string)
            .filter(|v| v.chars().next().is_some_and(|c| c.is_ascii_digit()));
        let uptime = self
            .call("core.uptime")
            .await
            .ok()
            .and_then(|u| u.get("uptime").and_then(|x| x.as_u64()));
        Ok(SbcStatus {
            software: "Kamailio".into(),
            version,
            uptime_secs: uptime,
        })
    }
}

// ============================================================
//  SipControl
// ============================================================

pub struct FreeswitchSipControl {
    pub esl: Option<EslConfig>,
    pub kamailio: Option<KamailioRpc>,
}

impl FreeswitchSipControl {
    async fn media(
        &self,
        cfg: &EslConfig,
        gateways: &[String],
    ) -> Result<(MediaServerStatus, Vec<GatewayStatus>), PortError> {
        let mut c = EslConn::connect(cfg).await?;
        let version = parse_version(&c.api("version", COMMAND_TIMEOUT).await?);
        let uptime_secs = c
            .api("uptime s", COMMAND_TIMEOUT)
            .await
            .ok()
            .and_then(|b| b.trim().parse().ok());
        let sessions_active = c
            .api("show channels count", COMMAND_TIMEOUT)
            .await
            .ok()
            .and_then(|b| parse_total(&b));
        let codecs = c
            .api("global_getvar outbound_codec_prefs", COMMAND_TIMEOUT)
            .await
            .map(|b| {
                b.trim()
                    .split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty() && !s.starts_with("-ERR"))
                    .collect()
            })
            .unwrap_or_default();
        let mut out = Vec::with_capacity(gateways.len());
        for g in gateways {
            let name = safe(g, b"-")?;
            let body = c
                .api(&format!("sofia xmlstatus gateway {name}"), COMMAND_TIMEOUT)
                .await?;
            let mut st = parse_gateway_xml(&name, &body);
            if let Some(trunk) = delonix_meet_domain::telephony::ports::trunk_id_from_gateway(&name)
                .filter(|_| st.registration != RegistrationState::Unknown)
            {
                st.channels_in_use = c
                    .api(
                        &format!("limit_usage hash delonix_trunk {trunk}"),
                        COMMAND_TIMEOUT,
                    )
                    .await
                    .ok()
                    .and_then(|b| b.trim().parse().ok());
            }
            out.push(st);
        }
        Ok((
            MediaServerStatus {
                software: "FreeSWITCH".into(),
                version,
                uptime_secs,
                sessions_active,
                codecs,
            },
            out,
        ))
    }
}

#[async_trait]
impl SipControl for FreeswitchSipControl {
    async fn snapshot(&self, gateway_names: &[String]) -> Result<SipSnapshot, PortError> {
        if self.esl.is_none() && self.kamailio.is_none() {
            return Err(PortError::NotConfigured(
                "sem TELEPHONY_ESL_ADDR nem TELEPHONY_KAMAILIO_RPC_URL".into(),
            ));
        }
        let (media, media_error, gateways) = match &self.esl {
            None => (None, Some("not_configured".to_string()), Vec::new()),
            Some(cfg) => match self.media(cfg, gateway_names).await {
                Ok((m, g)) => (Some(m), None, g),
                Err(e) => (None, Some(e.to_string()), Vec::new()),
            },
        };
        let (sbc, sbc_error) = match &self.kamailio {
            None => (None, Some("not_configured".to_string())),
            Some(k) => match k.sbc_status().await {
                Ok(s) => (Some(s), None),
                Err(e) => (None, Some(e.to_string())),
            },
        };
        Ok(SipSnapshot {
            measured_at: Utc::now(),
            media,
            media_error,
            sbc,
            sbc_error,
            gateways,
        })
    }

    async fn restart_registration(&self, gateway_names: &[String]) -> Result<(), PortError> {
        let cfg = self
            .esl
            .as_ref()
            .ok_or_else(|| PortError::NotConfigured("sem TELEPHONY_ESL_ADDR".into()))?;
        let profile = safe(&cfg.sofia_profile, b"-_")?;
        let mut c = EslConn::connect(cfg).await?;
        for g in gateway_names {
            let name = safe(g, b"-")?;
            // `killgw` num gateway que não existe responde -ERR; o `rescan`
            // volta a carregar os que a configuração (xml_curl) declara.
            let _ = c
                .api(
                    &format!("sofia profile {profile} killgw {name}"),
                    COMMAND_TIMEOUT,
                )
                .await?;
        }
        let body = c
            .api(&format!("sofia profile {profile} rescan"), COMMAND_TIMEOUT)
            .await?;
        if body.trim_start().starts_with("-ERR") {
            return Err(PortError::Rejected(body.trim().to_string()));
        }
        Ok(())
    }
}

// ============================================================
//  CallOriginator
// ============================================================

pub struct FreeswitchOriginator {
    pub esl: EslConfig,
}

/// Um evento `text/event-plain`: cabeçalhos (valores descodificados de URL)
/// e o corpo (no `BACKGROUND_JOB`, o resultado do comando).
pub fn parse_plain_event(body: &str) -> (HashMap<String, String>, String) {
    let (head, rest) = body.split_once("\n\n").unwrap_or((body, ""));
    let mut h = HashMap::new();
    for line in head.lines() {
        if let Some((k, v)) = line.split_once(": ") {
            let v = url::form_urlencoded::parse(format!("v={}", v.replace('+', "%2B")).as_bytes())
                .next()
                .map(|(_, v)| v.into_owned())
                .unwrap_or_else(|| v.to_string());
            h.insert(k.to_string(), v);
        }
    }
    (h, rest.to_string())
}

/// Máximo que se segue uma chamada atendida antes de largar o ESL.
const MAX_CALL: Duration = Duration::from_secs(6 * 3600);

async fn follow_call(
    mut c: EslConn,
    call_id: Uuid,
    started: Instant,
    events: Arc<dyn CallEventSink>,
    outcome_tx: tokio::sync::oneshot::Sender<OriginateOutcome>,
) {
    let mut outcome_tx = Some(outcome_tx);
    let trunk_of = |h: &HashMap<String, String>| {
        h.get("variable_delonix_trunk_id")
            .and_then(|v| Uuid::parse_str(v).ok())
    };
    let mut answered: Option<String> = None;
    let mut last_cause = String::from("UNKNOWN");
    let deadline = Instant::now() + MAX_CALL;
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        let (outer, body) = match c.read_message(left.max(Duration::from_millis(1))).await {
            Ok(m) => m,
            Err(_) => break,
        };
        match outer.get("content-type").map(String::as_str) {
            Some("text/event-plain") => {}
            Some("text/disconnect-notice") => break,
            _ => continue,
        }
        let (h, job_body) = parse_plain_event(&body);
        let uuid = h.get("Unique-ID").cloned().unwrap_or_default();
        match h.get("Event-Name").map(String::as_str) {
            Some("CHANNEL_CREATE") => events.on_event(
                call_id,
                CallEvent::Dialing {
                    trunk_id: trunk_of(&h),
                },
            ),
            Some("CHANNEL_PROGRESS") => events.on_event(
                call_id,
                CallEvent::Ringing {
                    trunk_id: trunk_of(&h),
                    early_media: false,
                },
            ),
            Some("CHANNEL_PROGRESS_MEDIA") => events.on_event(
                call_id,
                CallEvent::Ringing {
                    trunk_id: trunk_of(&h),
                    early_media: true,
                },
            ),
            Some("CHANNEL_ANSWER") if answered.is_none() => {
                let latency_ms = started.elapsed().as_millis() as u64;
                answered = Some(uuid);
                events.on_event(
                    call_id,
                    CallEvent::Answered {
                        trunk_id: trunk_of(&h),
                        latency_ms,
                    },
                );
                if let Some(tx) = outcome_tx.take() {
                    let _ = tx.send(OriginateOutcome {
                        answered: true,
                        answer_latency_ms: Some(latency_ms),
                        hangup_cause: None,
                    });
                }
            }
            // Tentativa falhada: no CHANNEL_HANGUP, que chega na hora. O
            // HANGUP_COMPLETE de uma perna falhada chega DEPOIS de a seguinte
            // ter atendido (medido no FreeSWITCH 1.11.3).
            Some("CHANNEL_HANGUP") if answered.as_deref() != Some(uuid.as_str()) => {
                let cause = h
                    .get("Hangup-Cause")
                    .cloned()
                    .unwrap_or_else(|| "UNKNOWN".into());
                last_cause = cause.clone();
                events.on_event(
                    call_id,
                    CallEvent::AttemptFailed {
                        trunk_id: trunk_of(&h),
                        cause,
                    },
                );
            }
            // Fim: o HANGUP_COMPLETE da perna atendida (traz o billsec).
            Some("CHANNEL_HANGUP_COMPLETE") if answered.as_deref() == Some(uuid.as_str()) => {
                let cause = h
                    .get("Hangup-Cause")
                    .cloned()
                    .unwrap_or_else(|| "UNKNOWN".into());
                let billsec = h.get("variable_billsec").and_then(|v| v.parse().ok());
                events.on_event(
                    call_id,
                    CallEvent::Ended {
                        answered: true,
                        cause,
                        billsec,
                    },
                );
                return;
            }
            Some("BACKGROUND_JOB") => {
                let (ok, cause) =
                    parse_originate(&job_body).unwrap_or((false, Some(last_cause.clone())));
                if !ok {
                    let cause = cause.unwrap_or_else(|| last_cause.clone());
                    if let Some(tx) = outcome_tx.take() {
                        let _ = tx.send(OriginateOutcome {
                            answered: false,
                            answer_latency_ms: None,
                            hangup_cause: Some(cause.clone()),
                        });
                    }
                    events.on_event(
                        call_id,
                        CallEvent::Ended {
                            answered: false,
                            cause,
                            billsec: None,
                        },
                    );
                    return;
                }
                // `+OK <uuid>`: atendida. Se o CHANNEL_ANSWER não chegou
                // (filtro), o uuid do job é o canal que atendeu.
                if answered.is_none() {
                    let latency_ms = started.elapsed().as_millis() as u64;
                    answered = job_body
                        .trim()
                        .strip_prefix("+OK")
                        .map(|u| u.trim().to_string());
                    events.on_event(
                        call_id,
                        CallEvent::Answered {
                            trunk_id: None,
                            latency_ms,
                        },
                    );
                    if let Some(tx) = outcome_tx.take() {
                        let _ = tx.send(OriginateOutcome {
                            answered: true,
                            answer_latency_ms: Some(latency_ms),
                            hangup_cause: None,
                        });
                    }
                }
            }
            _ => {}
        }
    }
    // O fluxo de eventos acabou sem o fim da chamada: diz-se, não se inventa.
    events.on_event(
        call_id,
        CallEvent::Ended {
            answered: answered.is_some(),
            cause: "EVENT_STREAM_LOST".into(),
            billsec: None,
        },
    );
}

#[async_trait]
impl CallOriginator for FreeswitchOriginator {
    async fn originate(
        &self,
        req: &OriginateRequest,
        events: Arc<dyn CallEventSink>,
    ) -> Result<OriginateOutcome, PortError> {
        let cmd = originate_command(req, &self.esl.sofia_profile)?;
        let mut c = EslConn::connect(&self.esl).await?;
        c.command(
            "event plain CHANNEL_CREATE CHANNEL_PROGRESS CHANNEL_PROGRESS_MEDIA CHANNEL_ANSWER CHANNEL_HANGUP CHANNEL_HANGUP_COMPLETE BACKGROUND_JOB",
            &[],
        )
        .await?;
        // Só os canais desta chamada (todas as tentativas levam a variável) e o
        // resultado do job desta chamada.
        c.command(
            &format!("filter variable_delonix_call_id {}", req.call_id),
            &[],
        )
        .await?;
        c.command(&format!("filter Job-UUID {}", req.call_id), &[])
            .await?;
        let started = Instant::now();
        c.command(
            &format!("bgapi {cmd}"),
            &[("Job-UUID", req.call_id.to_string())],
        )
        .await?;
        let (tx, rx) = tokio::sync::oneshot::channel();
        tokio::spawn(follow_call(c, req.call_id, started, events, tx));
        let limit = Duration::from_secs(u64::from(req.answer_timeout_secs.clamp(5, 120)) + 15);
        match timeout(limit, rx).await {
            Ok(Ok(o)) => Ok(o),
            Ok(Err(_)) => Err(unavailable("o ESL fechou antes do resultado da chamada")),
            Err(_) => Err(unavailable("sem resultado da chamada no tempo previsto")),
        }
    }

    async fn hangup(&self, call_id: Uuid) -> Result<(), PortError> {
        let mut c = EslConn::connect(&self.esl).await?;
        // Pela variável: com failover, o uuid do canal que atendeu não é
        // necessariamente o `origination_uuid`.
        let body = c
            .api(
                &format!("hupall NORMAL_CLEARING delonix_call_id {call_id}"),
                COMMAND_TIMEOUT,
            )
            .await?;
        if body.trim_start().starts_with("-ERR") {
            return Err(PortError::Rejected(body.trim().to_string()));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use delonix_meet_domain::telephony::ports::DialLeg;

    #[test]
    fn parses_fs_bodies() {
        assert_eq!(
            parse_version("FreeSWITCH Version 1.10.11-release+git~20231213 (git 1a2b 64bit)\n"),
            Some("1.10.11-release+git~20231213".into())
        );
        assert_eq!(parse_total("\n3 total.\n"), Some(3));
        assert_eq!(parse_total("\n0 total.\n"), Some(0));
        let xml = "<gateway><name>dlx-x</name><state>REGED</state><status>UP</status><pingtime>12.5</pingtime></gateway>";
        let g = parse_gateway_xml("dlx-x", xml);
        assert_eq!(g.registration, RegistrationState::Registered);
        assert_eq!(g.up, Some(true));
        assert_eq!(g.ping_ms, Some(12.5));
        let g = parse_gateway_xml("dlx-y", "Invalid Gateway!\n");
        assert_eq!(g.registration, RegistrationState::Unknown);
        let g = parse_gateway_xml(
            "dlx-z",
            "<gateway><state>FAIL_WAIT</state><status>DOWN</status></gateway>",
        );
        assert_eq!(
            (g.registration, g.up),
            (RegistrationState::Failed, Some(false))
        );
    }

    fn req() -> OriginateRequest {
        let t = Uuid::nil();
        OriginateRequest {
            call_id: Uuid::nil(),
            org_id: Uuid::nil(),
            legs: vec![DialLeg {
                trunk_id: t,
                gateway_name: format!("dlx-{t}"),
                number: "244923447108".into(),
            }],
            caller_id: None,
            record: false,
            emergency: false,
            rule_position: Some(0),
            answer_timeout_secs: 30,
            after_answer: AfterAnswer::TestTone { secs: 3 },
        }
    }

    #[test]
    fn originate_command_is_built_from_validated_values() {
        let cmd = originate_command(&req(), "external").unwrap();
        assert!(cmd.starts_with("originate {originate_timeout=30,"), "{cmd}");
        assert!(cmd.contains("[origination_uuid=00000000-0000-0000-0000-000000000000,delonix_trunk_id=00000000-0000-0000-0000-000000000000]sofia/gateway/dlx-00000000-0000-0000-0000-000000000000/244923447108"));
        assert!(cmd.ends_with(" &playback(tone_stream://%(1000,0,440);loops=3)"));
        assert!(!cmd.contains("record_session"));

        let mut r = req();
        r.record = true;
        r.emergency = true;
        let cmd = originate_command(&r, "external").unwrap();
        assert!(!cmd.contains("record_session"), "emergência nunca gravada");
        assert!(cmd.contains("delonix_record=false"));

        let mut r = req();
        r.legs[0].number = "923\nhangup".into();
        assert!(originate_command(&r, "external").is_err());
        let mut r = req();
        r.after_answer = AfterAnswer::Conference {
            room_code: "abc;evil".into(),
        };
        assert!(originate_command(&r, "external").is_err());
    }

    #[test]
    fn room_bridge_goes_to_the_bridge_ua_not_the_local_conference() {
        let mut r = req();
        r.after_answer = AfterAnswer::RoomBridge {
            room_code: "voz-arq-2026".into(),
            bridge_host: "127.0.0.1".parse().unwrap(),
            bridge_port: 5190,
            codec: Some("PCMA".into()),
        };
        let cmd = originate_command(&r, "external").unwrap();
        assert!(cmd.contains("delonix_room_code=voz-arq-2026"), "{cmd}");
        assert!(cmd.ends_with(" &bridge([absolute_codec_string=PCMA,delonix_room_code=voz-arq-2026,delonix_leg=room_bridge,delonix_cdr_skip=true]sofia/external/room-voz-arq-2026@127.0.0.1:5190)"), "{cmd}");
        assert!(!cmd.contains("conference"));
    }

    #[test]
    fn plain_events_are_decoded() {
        let (h, body) = parse_plain_event(
            "Event-Name: BACKGROUND_JOB\nJob-UUID: x\nCaller-Caller-ID-Number: %2B244%20923\nContent-Length: 8\n\n-ERR NO\n",
        );
        assert_eq!(h["Event-Name"], "BACKGROUND_JOB");
        assert_eq!(h["Caller-Caller-ID-Number"], "+244 923");
        assert_eq!(body, "-ERR NO\n");
    }

    #[test]
    fn originate_replies() {
        assert_eq!(parse_originate("+OK 1234\n").unwrap(), (true, None));
        assert_eq!(
            parse_originate("-ERR NO_ANSWER\n").unwrap(),
            (false, Some("NO_ANSWER".into()))
        );
        assert!(parse_originate("banana").is_err());
    }
}
