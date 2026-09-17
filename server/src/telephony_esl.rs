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

use std::{collections::HashMap, time::Duration};

use async_trait::async_trait;
use chrono::Utc;
use delonix_meet_domain::telephony::{
    cost::RegistrationState,
    ports::{
        AfterAnswer, CallOriginator, GatewayStatus, MediaServerStatus, OriginateOutcome,
        OriginateRequest, PortError, SbcStatus, SipControl, SipSnapshot,
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
pub fn originate_command(req: &OriginateRequest) -> Result<String, PortError> {
    if req.legs.is_empty() {
        return Err(PortError::Protocol("originate sem troncos".into()));
    }
    let mut vars = vec![
        format!("origination_uuid={}", req.call_id),
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
    if let AfterAnswer::Conference { room_code } = &req.after_answer {
        vars.push(format!("delonix_room_code={}", safe(room_code, b"-")?));
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
        .map(|l| {
            Ok(format!(
                "[delonix_trunk_id={}]sofia/gateway/{}/{}",
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

    pub async fn status(&self) -> Result<SbcStatus, PortError> {
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
            Some(k) => match k.status().await {
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

#[async_trait]
impl CallOriginator for FreeswitchOriginator {
    async fn originate(&self, req: &OriginateRequest) -> Result<OriginateOutcome, PortError> {
        let cmd = originate_command(req)?;
        let mut c = EslConn::connect(&self.esl).await?;
        let started = Instant::now();
        let limit = Duration::from_secs(u64::from(req.answer_timeout_secs.clamp(5, 120)) + 10);
        let body = c.api(&cmd, limit).await?;
        let (answered, hangup_cause) = parse_originate(&body)?;
        Ok(OriginateOutcome {
            answered,
            answer_latency_ms: answered.then(|| started.elapsed().as_millis() as u64),
            hangup_cause,
        })
    }

    async fn hangup(&self, call_id: Uuid) -> Result<(), PortError> {
        let mut c = EslConn::connect(&self.esl).await?;
        let body = c
            .api(&format!("uuid_kill {call_id}"), COMMAND_TIMEOUT)
            .await?;
        if body.trim_start().starts_with("-ERR") && !body.contains("No such channel") {
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
        let cmd = originate_command(&req()).unwrap();
        assert!(cmd.starts_with("originate {origination_uuid=00000000-0000-0000-0000-000000000000,originate_timeout=30,"));
        assert!(cmd.contains("[delonix_trunk_id=00000000-0000-0000-0000-000000000000]sofia/gateway/dlx-00000000-0000-0000-0000-000000000000/244923447108"));
        assert!(cmd.ends_with(" &playback(tone_stream://%(1000,0,440);loops=3)"));
        assert!(!cmd.contains("record_session"));

        let mut r = req();
        r.record = true;
        r.emergency = true;
        let cmd = originate_command(&r).unwrap();
        assert!(!cmd.contains("record_session"), "emergência nunca gravada");
        assert!(cmd.contains("delonix_record=false"));

        let mut r = req();
        r.legs[0].number = "923\nhangup".into();
        assert!(originate_command(&r).is_err());
        let mut r = req();
        r.after_answer = AfterAnswer::Conference {
            room_code: "abc;evil".into(),
        };
        assert!(originate_command(&r).is_err());
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
