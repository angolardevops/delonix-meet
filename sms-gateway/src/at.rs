//! Transporte `at_serial`: comandos AT directos numa porta série.
//!
//! Só se usa quando o ModemManager NÃO gere o modem — se gere, as portas já estão
//! ocupadas por ele e falar AT por cima corrompe os dois diálogos.
//!
//! O diálogo é escrito sobre `Read + Write` genérico para poder ser testado com um
//! modem falso; a porta real (`serialport`) é bloqueante e corre sempre dentro de
//! `tokio::task::spawn_blocking`.

use std::io::{self, Read, Write};
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Context, Result};

/// Tempo máximo à espera da resposta a um comando curto.
pub const COMMAND_TIMEOUT: Duration = Duration::from_secs(2);
/// Tempo máximo à espera do `+CMGS` depois de entregar o PDU à rede.
pub const SEND_TIMEOUT: Duration = Duration::from_secs(60);
/// Timeout de cada `read` na porta: curto, para o laço verificar o prazo.
const READ_SLICE: Duration = Duration::from_millis(200);

/// Um PDU SMS-SUBMIT já codificado pelo servidor (`sms_codec`).
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
pub struct Pdu {
    pub hex: String,
    pub tpdu_len: u32,
}

/// O que a sonda AT apurou numa porta.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AtProbe {
    pub port: String,
    /// A porta respondeu `OK` a `AT` — é a porta de comandos do modem.
    pub answered: bool,
    pub capable: bool,
    pub reason: Option<String>,
    pub operator_name: Option<String>,
    pub signal_percent: Option<u8>,
}

// ---------------------------------------------------------------------------
// Analisadores puros
// ---------------------------------------------------------------------------

/// Resultado final de um comando AT.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Final {
    Ok,
    /// `ERROR`, `+CME ERROR: …` ou `+CMS ERROR: …` — guarda a linha.
    Error(String),
}

/// Parte o texto recebido em linhas não vazias, sem `\r`/`\n`.
pub fn lines(buf: &str) -> Vec<String> {
    buf.split(['\r', '\n'])
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(str::to_string)
        .collect()
}

/// Se a linha é um resultado final, qual.
pub fn final_result(line: &str) -> Option<Final> {
    let l = line.trim();
    if l == "OK" {
        Some(Final::Ok)
    } else if l == "ERROR" || l.starts_with("+CME ERROR") || l.starts_with("+CMS ERROR") {
        Some(Final::Error(l.to_string()))
    } else {
        None
    }
}

/// Procura o resultado final no texto acumulado; devolve as linhas
/// intermédias (sem eco de comandos) e o final.
///
/// Só conta linhas COMPLETAS (terminadas em `\r` ou `\n`): a porta série
/// entrega aos bocados, e `+CMS ERROR:` sem o código ainda não é a resposta.
pub fn parse_response(buf: &str) -> Option<(Vec<String>, Final)> {
    let complete = &buf[..=buf.rfind(['\r', '\n'])?];
    let mut body = Vec::new();
    for l in lines(complete) {
        if let Some(f) = final_result(&l) {
            return Some((body, f));
        }
        if !l.to_ascii_uppercase().starts_with("AT") {
            body.push(l);
        }
    }
    None
}

/// `+CPIN: READY` → `true`.
pub fn parse_cpin_ready(body: &[String]) -> bool {
    body.iter()
        .any(|l| l.starts_with("+CPIN:") && l["+CPIN:".len()..].trim() == "READY")
}

/// `+COPS: 0,0,"UNITEL AO",7` → `UNITEL AO`. Sem operador (`+COPS: 0`) → `None`.
pub fn parse_cops(body: &[String]) -> Option<String> {
    let l = body.iter().find(|l| l.starts_with("+COPS:"))?;
    let start = l.find('"')?;
    let rest = &l[start + 1..];
    let end = rest.find('"')?;
    let name = rest[..end].trim();
    (!name.is_empty()).then(|| name.to_string())
}

/// `+CSQ: 20,99` → 64 %. RSSI 0..=31 escala para 0..=100; 99 = desconhecido.
pub fn parse_csq(body: &[String]) -> Option<u8> {
    let l = body.iter().find(|l| l.starts_with("+CSQ:"))?;
    let rssi: u32 = l["+CSQ:".len()..].split(',').next()?.trim().parse().ok()?;
    rssi_to_percent(rssi)
}

pub fn rssi_to_percent(rssi: u32) -> Option<u8> {
    (rssi <= 31).then(|| (rssi * 100 / 31) as u8)
}

/// `+CMGS: 17` → 17 (referência da mensagem).
pub fn parse_cmgs(body: &[String]) -> Option<u32> {
    let l = body.iter().find(|l| l.starts_with("+CMGS:"))?;
    l["+CMGS:".len()..].trim().parse().ok()
}

/// Valida um PDU antes de o escrever no modem: hexadecimal, tamanho par, e
/// `tpdu_len` coerente (o PDU inclui o SMSC; o TPDU tem de caber nele).
pub fn validate_pdu(pdu: &Pdu) -> Result<()> {
    let hex = pdu.hex.as_bytes();
    if hex.is_empty() || !hex.len().is_multiple_of(2) || !hex.iter().all(u8::is_ascii_hexdigit) {
        bail!("PDU inválido: não é hexadecimal de tamanho par");
    }
    let octets = (hex.len() / 2) as u32;
    if pdu.tpdu_len == 0 || pdu.tpdu_len > octets || pdu.tpdu_len > 175 {
        bail!(
            "PDU inválido: tpdu_len {} incoerente com {octets} octetos",
            pdu.tpdu_len
        );
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Diálogo sobre Read + Write
// ---------------------------------------------------------------------------

pub struct AtSession<T: Read + Write> {
    io: T,
}

impl<T: Read + Write> AtSession<T> {
    pub fn new(io: T) -> Self {
        AtSession { io }
    }

    /// Lê até `done(buffer)` devolver algo, ou até ao prazo.
    fn read_until<R>(
        &mut self,
        timeout: Duration,
        mut done: impl FnMut(&str) -> Option<R>,
    ) -> Result<R> {
        let deadline = Instant::now() + timeout;
        let mut acc = Vec::new();
        let mut chunk = [0u8; 256];
        loop {
            if let Some(r) = done(&String::from_utf8_lossy(&acc)) {
                return Ok(r);
            }
            if Instant::now() >= deadline {
                let seen = String::from_utf8_lossy(&acc).trim().to_string();
                bail!(
                    "sem resposta do modem em {} s (recebido: {:?})",
                    timeout.as_secs(),
                    seen
                );
            }
            match self.io.read(&mut chunk) {
                Ok(0) => std::thread::sleep(Duration::from_millis(10)),
                Ok(n) => acc.extend_from_slice(&chunk[..n]),
                Err(e)
                    if matches!(
                        e.kind(),
                        io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock
                    ) => {}
                Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                Err(e) => return Err(e).context("erro a ler da porta série"),
            }
        }
    }

    /// Envia um comando (sem `\r`) e devolve as linhas intermédias se `OK`.
    pub fn command(&mut self, cmd: &str, timeout: Duration) -> Result<Vec<String>> {
        self.io
            .write_all(format!("{cmd}\r").as_bytes())
            .and_then(|_| self.io.flush())
            .with_context(|| format!("erro a escrever {cmd} na porta série"))?;
        let (body, fin) = self.read_until(timeout, parse_response)?;
        match fin {
            Final::Ok => Ok(body),
            Final::Error(e) => Err(anyhow!("{cmd} → {e}")),
        }
    }

    /// Sonda: AT, SIM, operador, sinal e modo PDU.
    pub fn probe(&mut self, port: &str) -> AtProbe {
        let mut out = AtProbe {
            port: port.to_string(),
            answered: false,
            capable: false,
            reason: None,
            operator_name: None,
            signal_percent: None,
        };
        if let Err(e) = self.command("AT", COMMAND_TIMEOUT) {
            out.reason = Some(format!("a porta não responde a AT: {e}"));
            return out;
        }
        out.answered = true;
        // Sem eco: não é obrigatório para o analisador, mas limpa o diálogo.
        let _ = self.command("ATE0", COMMAND_TIMEOUT);
        match self.command("AT+CPIN?", COMMAND_TIMEOUT) {
            Ok(body) if parse_cpin_ready(&body) => {}
            _ => {
                out.reason = Some("SIM bloqueado ou ausente".to_string());
                return out;
            }
        }
        if let Ok(body) = self.command("AT+COPS?", COMMAND_TIMEOUT) {
            out.operator_name = parse_cops(&body);
        }
        if let Ok(body) = self.command("AT+CSQ", COMMAND_TIMEOUT) {
            out.signal_percent = parse_csq(&body);
        }
        match self.command("AT+CMGF=0", COMMAND_TIMEOUT) {
            Ok(_) => out.capable = true,
            Err(e) => out.reason = Some(format!("o modem não aceita SMS em modo PDU: {e}")),
        }
        out
    }

    /// Envia um PDU. Devolve a referência (`+CMGS: <mr>`).
    pub fn send_pdu(&mut self, pdu: &Pdu, send_timeout: Duration) -> Result<u32> {
        validate_pdu(pdu)?;
        self.command("AT+CMGF=0", COMMAND_TIMEOUT)?;
        self.io
            .write_all(format!("AT+CMGS={}\r", pdu.tpdu_len).as_bytes())
            .and_then(|_| self.io.flush())
            .context("erro a escrever AT+CMGS na porta série")?;
        enum Prompt {
            Ready,
            Refused(String),
        }
        let prompt = self.read_until(COMMAND_TIMEOUT, |buf| {
            if buf.contains('>') {
                return Some(Prompt::Ready);
            }
            lines(buf).iter().find_map(|l| match final_result(l) {
                Some(Final::Error(e)) => Some(Prompt::Refused(e)),
                Some(Final::Ok) => Some(Prompt::Refused("OK sem prompt".into())),
                None => None,
            })
        })?;
        if let Prompt::Refused(e) = prompt {
            bail!("AT+CMGS recusado: {e}");
        }
        let mut payload = pdu.hex.clone().into_bytes();
        payload.push(0x1A); // Ctrl-Z
        self.io
            .write_all(&payload)
            .and_then(|_| self.io.flush())
            .context("erro a escrever o PDU na porta série")?;
        let (body, fin) = self
            .read_until(send_timeout, parse_response)
            .context("o modem não confirmou o envio")?;
        match fin {
            Final::Ok => parse_cmgs(&body).ok_or_else(|| anyhow!("OK sem +CMGS: {body:?}")),
            Final::Error(e) => bail!("envio recusado: {e}"),
        }
    }
}

// ---------------------------------------------------------------------------
// Porta real
// ---------------------------------------------------------------------------

fn open(port: &str) -> Result<Box<dyn serialport::SerialPort>> {
    serialport::new(port, 115_200)
        .data_bits(serialport::DataBits::Eight)
        .parity(serialport::Parity::None)
        .stop_bits(serialport::StopBits::One)
        .flow_control(serialport::FlowControl::None)
        .timeout(READ_SLICE)
        .open()
        .map_err(|e| {
            let hint = if e.to_string().to_lowercase().contains("permission") {
                " (o utilizador tem de estar no grupo dialout)"
            } else {
                ""
            };
            anyhow!("não foi possível abrir {port}: {e}{hint}")
        })
}

/// Sonda as portas por ordem e fica com a primeira que responde a AT.
/// BLOQUEANTE — chamar dentro de `spawn_blocking`.
pub fn probe_ports_blocking(ports: &[String]) -> AtProbe {
    let mut last_reason = None;
    for port in ports {
        let io = match open(port) {
            Ok(io) => io,
            Err(e) => {
                last_reason = Some(e.to_string());
                continue;
            }
        };
        let probe = AtSession::new(io).probe(port);
        if probe.answered {
            return probe;
        }
        last_reason = probe.reason;
    }
    AtProbe {
        port: ports.first().cloned().unwrap_or_default(),
        answered: false,
        capable: false,
        reason: Some(
            last_reason.unwrap_or_else(|| "nenhuma porta série respondeu a AT".to_string()),
        ),
        operator_name: None,
        signal_percent: None,
    }
}

/// Envia todos os PDUs de uma mensagem numa porta. BLOQUEANTE.
/// Devolve as referências separadas por vírgula (uma por segmento).
pub fn send_blocking(port: &str, pdus: &[Pdu]) -> Result<String> {
    if pdus.is_empty() {
        bail!("mensagem sem PDUs");
    }
    let mut session = AtSession::new(open(port)?);
    let mut refs = Vec::with_capacity(pdus.len());
    for (i, pdu) in pdus.iter().enumerate() {
        let mr = session
            .send_pdu(pdu, SEND_TIMEOUT)
            .with_context(|| format!("segmento {}/{}", i + 1, pdus.len()))?;
        refs.push(mr.to_string());
    }
    Ok(refs.join(","))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;

    /// Modem falso: para cada escrita que comece por `expect`, põe `reply` na
    /// fila de leitura. Escritas fora do guião falham o teste.
    struct FakeModem {
        script: VecDeque<(&'static str, &'static str)>,
        pending: Vec<u8>,
        output: Vec<u8>,
        written: Vec<String>,
    }

    impl FakeModem {
        fn new(script: &[(&'static str, &'static str)]) -> Self {
            FakeModem {
                script: script.iter().copied().collect(),
                pending: Vec::new(),
                output: Vec::new(),
                written: Vec::new(),
            }
        }
    }

    impl Write for FakeModem {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.pending.extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            let chunk = String::from_utf8_lossy(&self.pending).into_owned();
            self.pending.clear();
            let (expect, reply) = self
                .script
                .pop_front()
                .unwrap_or_else(|| panic!("escrita fora do guião: {chunk:?}"));
            assert!(
                chunk.starts_with(expect),
                "esperava {expect:?}, recebeu {chunk:?}"
            );
            self.written.push(chunk);
            self.output.extend_from_slice(reply.as_bytes());
            Ok(())
        }
    }

    impl Read for FakeModem {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            if self.output.is_empty() {
                return Err(io::Error::new(io::ErrorKind::TimedOut, "vazio"));
            }
            // entrega aos bocados, como uma porta série
            let n = buf.len().min(self.output.len()).min(7);
            buf[..n].copy_from_slice(&self.output[..n]);
            self.output.drain(..n);
            Ok(n)
        }
    }

    fn body(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn response_parser_strips_echo_and_finds_final() {
        let (b, f) = parse_response("AT+CSQ\r\r\n+CSQ: 20,99\r\n\r\nOK\r\n").unwrap();
        assert_eq!(b, body(&["+CSQ: 20,99"]));
        assert_eq!(f, Final::Ok);
        let (_, f) = parse_response("\r\n+CMS ERROR: 500\r\n").unwrap();
        assert_eq!(f, Final::Error("+CMS ERROR: 500".into()));
        assert_eq!(parse_response("\r\n+CSQ: 20,99\r\n"), None);
        // linha final ainda incompleta: não é resposta
        assert_eq!(parse_response("\r\n+CMS ERROR:"), None);
        assert_eq!(parse_response("\r\nOK"), None);
    }

    #[test]
    fn cpin_cops_csq_cmgs() {
        assert!(parse_cpin_ready(&body(&["+CPIN: READY"])));
        assert!(!parse_cpin_ready(&body(&["+CPIN: SIM PIN"])));
        assert_eq!(
            parse_cops(&body(&["+COPS: 0,0,\"UNITEL AO\",7"])),
            Some("UNITEL AO".into())
        );
        assert_eq!(parse_cops(&body(&["+COPS: 0"])), None);
        assert_eq!(parse_csq(&body(&["+CSQ: 31,99"])), Some(100));
        assert_eq!(parse_csq(&body(&["+CSQ: 20,99"])), Some(64));
        assert_eq!(parse_csq(&body(&["+CSQ: 0,0"])), Some(0));
        assert_eq!(parse_csq(&body(&["+CSQ: 99,99"])), None);
        assert_eq!(parse_cmgs(&body(&["+CMGS: 17"])), Some(17));
    }

    #[test]
    fn pdu_validation() {
        let ok = Pdu {
            hex: "0011000C9144".into(),
            tpdu_len: 5,
        };
        assert!(validate_pdu(&ok).is_ok());
        for bad in [
            Pdu {
                hex: "0G".into(),
                tpdu_len: 1,
            },
            Pdu {
                hex: "001".into(),
                tpdu_len: 1,
            },
            Pdu {
                hex: "0011".into(),
                tpdu_len: 3,
            },
            Pdu {
                hex: "0011".into(),
                tpdu_len: 0,
            },
        ] {
            assert!(validate_pdu(&bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn probe_happy_path() {
        let modem = FakeModem::new(&[
            ("AT\r", "AT\r\r\nOK\r\n"),
            ("ATE0\r", "ATE0\r\r\nOK\r\n"),
            ("AT+CPIN?\r", "\r\n+CPIN: READY\r\n\r\nOK\r\n"),
            ("AT+COPS?\r", "\r\n+COPS: 0,0,\"MOVICEL\",2\r\n\r\nOK\r\n"),
            ("AT+CSQ\r", "\r\n+CSQ: 15,99\r\n\r\nOK\r\n"),
            ("AT+CMGF=0\r", "\r\nOK\r\n"),
        ]);
        let p = AtSession::new(modem).probe("/dev/ttyUSB0");
        assert!(p.capable, "{p:?}");
        assert_eq!(p.operator_name.as_deref(), Some("MOVICEL"));
        assert_eq!(p.signal_percent, Some(48));
        assert_eq!(p.reason, None);
    }

    #[test]
    fn probe_without_sim_is_not_capable() {
        let modem = FakeModem::new(&[
            ("AT\r", "\r\nOK\r\n"),
            ("ATE0\r", "\r\nOK\r\n"),
            ("AT+CPIN?\r", "\r\n+CME ERROR: 10\r\n"),
        ]);
        let p = AtSession::new(modem).probe("/dev/ttyUSB0");
        assert!(!p.capable);
        assert_eq!(p.reason.as_deref(), Some("SIM bloqueado ou ausente"));
    }

    #[test]
    fn send_dialogue_writes_pdu_with_ctrl_z_and_reads_reference() {
        let modem = FakeModem::new(&[
            ("AT+CMGF=0\r", "\r\nOK\r\n"),
            ("AT+CMGS=5\r", "\r\n> "),
            ("0011000C9144", "\r\n+CMGS: 42\r\n\r\nOK\r\n"),
        ]);
        let mut s = AtSession::new(modem);
        let pdu = Pdu {
            hex: "0011000C9144".into(),
            tpdu_len: 5,
        };
        assert_eq!(s.send_pdu(&pdu, Duration::from_secs(1)).unwrap(), 42);
        assert_eq!(s.io.written[2], "0011000C9144\u{1a}");
    }

    #[test]
    fn send_reports_cms_error_text() {
        let modem = FakeModem::new(&[
            ("AT+CMGF=0\r", "\r\nOK\r\n"),
            ("AT+CMGS=5\r", "\r\n> "),
            ("0011000C9144", "\r\n+CMS ERROR: 38\r\n"),
        ]);
        let pdu = Pdu {
            hex: "0011000C9144".into(),
            tpdu_len: 5,
        };
        let err = AtSession::new(modem)
            .send_pdu(&pdu, Duration::from_secs(1))
            .unwrap_err();
        assert!(format!("{err:#}").contains("+CMS ERROR: 38"), "{err:#}");
    }

    #[test]
    fn send_refused_before_prompt() {
        let modem = FakeModem::new(&[
            ("AT+CMGF=0\r", "\r\nOK\r\n"),
            ("AT+CMGS=5\r", "\r\nERROR\r\n"),
        ]);
        let pdu = Pdu {
            hex: "0011000C9144".into(),
            tpdu_len: 5,
        };
        let err = AtSession::new(modem)
            .send_pdu(&pdu, Duration::from_secs(1))
            .unwrap_err();
        assert!(err.to_string().contains("AT+CMGS recusado"), "{err:#}");
    }

    #[test]
    fn send_times_out_without_confirmation() {
        let modem = FakeModem::new(&[
            ("AT+CMGF=0\r", "\r\nOK\r\n"),
            ("AT+CMGS=5\r", "\r\n> "),
            ("0011000C9144", ""),
        ]);
        let pdu = Pdu {
            hex: "0011000C9144".into(),
            tpdu_len: 5,
        };
        let err = AtSession::new(modem)
            .send_pdu(&pdu, Duration::from_millis(50))
            .unwrap_err();
        assert!(format!("{err:#}").contains("não confirmou"), "{err:#}");
    }
}
