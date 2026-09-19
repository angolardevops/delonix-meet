//! Cliente SMPP 3.4 mínimo para os operadores móveis (ADR-0005).
//!
//! Só o caminho de SAÍDA: `bind_transmitter` → `submit_sm` por parte → `unbind`,
//! numa ligação por lote. Não há `deliver_sm` (recibos, SMS recebidos) nem TLS —
//! está escrito no ADR o que isso significa: `sent` é «o SMSC aceitou», não
//! «chegou».
//!
//! Nenhum contrato com a Unitel, a Movicel ou a Africell existe ainda. O que está
//! provado é o protocolo contra um SMSC falso (testes abaixo); a interoperação
//! com o SMSC real de cada operador só se prova com o contrato na mão.

use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

use crate::sms_codec::{concat_udh, Encoded, Encoding};

const BIND_TRANSMITTER: u32 = 0x0000_0002;
const SUBMIT_SM: u32 = 0x0000_0004;
const UNBIND: u32 = 0x0000_0006;
const GENERIC_NACK: u32 = 0x8000_0000;
const RESP: u32 = 0x8000_0000;

const IO_TIMEOUT: Duration = Duration::from_secs(10);
/// Um PDU acima disto não é SMPP de SMS: é lixo ou ataque.
const MAX_PDU: u32 = 64 * 1024;

/// Credenciais e origem de uma ligação a um operador. A password não sai em
/// `Debug` — este tipo passa por logs de configuração.
#[derive(Clone)]
pub struct SmppLink {
    pub host: String,
    pub port: u16,
    pub system_id: String,
    password: String,
    pub source_addr: String,
}

impl std::fmt::Debug for SmppLink {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SmppLink")
            .field("host", &self.host)
            .field("port", &self.port)
            .field("system_id", &self.system_id)
            .field("password", &"<redigida>")
            .field("source_addr", &self.source_addr)
            .finish()
    }
}

impl SmppLink {
    /// `smpp://system_id:password@host:2775?source_addr=DELONIX`
    pub fn parse(raw: &str) -> Result<SmppLink, String> {
        let url = url::Url::parse(raw).map_err(|_| "URL SMPP inválido".to_string())?;
        if url.scheme() != "smpp" {
            return Err("o esquema tem de ser smpp://".into());
        }
        let host = url.host_str().ok_or("falta o host")?.to_string();
        let system_id = url.username().to_string();
        if system_id.is_empty() {
            return Err("falta o system_id".into());
        }
        let password = url
            .password()
            .map(|p| {
                url::form_urlencoded::parse(format!("p={p}").as_bytes())
                    .next()
                    .map(|(_, v)| v.into_owned())
                    .unwrap_or_default()
            })
            .unwrap_or_default();
        let source_addr = url
            .query_pairs()
            .find(|(k, _)| k == "source_addr")
            .map(|(_, v)| v.into_owned())
            .unwrap_or_default();
        Ok(SmppLink {
            host,
            port: url.port().unwrap_or(2775),
            system_id,
            password,
            source_addr,
        })
    }
}

#[derive(Debug)]
pub enum SmppError {
    Io(String),
    /// `command_status` diferente de zero, com o comando que o recebeu.
    Status {
        command: &'static str,
        status: u32,
    },
    Protocol(String),
}

impl std::fmt::Display for SmppError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SmppError::Io(e) => write!(f, "ligação SMPP: {e}"),
            SmppError::Status { command, status } => {
                write!(
                    f,
                    "o SMSC recusou {command} (command_status 0x{status:08X})"
                )
            }
            SmppError::Protocol(e) => write!(f, "protocolo SMPP: {e}"),
        }
    }
}

// ---------- PDU ----------

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pdu {
    pub command_id: u32,
    pub status: u32,
    pub sequence: u32,
    pub body: Vec<u8>,
}

impl Pdu {
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(16 + self.body.len());
        out.extend(((16 + self.body.len()) as u32).to_be_bytes());
        out.extend(self.command_id.to_be_bytes());
        out.extend(self.status.to_be_bytes());
        out.extend(self.sequence.to_be_bytes());
        out.extend(&self.body);
        out
    }
}

fn cstr(out: &mut Vec<u8>, s: &str) {
    out.extend(s.as_bytes());
    out.push(0);
}

pub fn bind_transmitter_body(link: &SmppLink) -> Vec<u8> {
    let mut b = Vec::new();
    cstr(&mut b, &link.system_id);
    cstr(&mut b, &link.password);
    cstr(&mut b, ""); // system_type
    b.push(0x34); // interface_version 3.4
    b.push(0); // addr_ton
    b.push(0); // addr_npi
    cstr(&mut b, ""); // address_range
    b
}

/// Corpo de um `submit_sm` para UMA parte. Em GSM, `data_coding 0` com os
/// septetos por empacotar — é o que a maioria dos SMSC entende por «alfabeto por
/// omissão»; confirma-se com cada operador.
pub fn submit_sm_body(
    source: &str,
    dest_digits: &str,
    encoding: Encoding,
    udh: &[u8],
    part: &[u8],
) -> Vec<u8> {
    let alnum = !source.is_empty() && !source.bytes().all(|b| b.is_ascii_digit());
    let mut b = Vec::new();
    cstr(&mut b, ""); // service_type
    b.push(if alnum { 5 } else { 1 }); // source_addr_ton
    b.push(if alnum { 0 } else { 1 }); // source_addr_npi
    cstr(&mut b, source);
    b.push(1); // dest_addr_ton: internacional
    b.push(1); // dest_addr_npi: ISDN
    cstr(&mut b, dest_digits);
    b.push(if udh.is_empty() { 0x00 } else { 0x40 }); // esm_class (UDHI)
    b.push(0); // protocol_id
    b.push(0); // priority_flag
    cstr(&mut b, ""); // schedule_delivery_time
    cstr(&mut b, ""); // validity_period
    b.push(0); // registered_delivery: sem recibos (ADR-0005)
    b.push(0); // replace_if_present_flag
    b.push(match encoding {
        Encoding::Gsm7 => 0x00,
        Encoding::Ucs2 => 0x08,
    });
    b.push(0); // sm_default_msg_id
    let header_len = if udh.is_empty() { 0 } else { udh.len() + 1 };
    b.push((header_len + part.len()) as u8); // sm_length
    if !udh.is_empty() {
        b.push(udh.len() as u8);
        b.extend(udh);
    }
    b.extend(part);
    b
}

async fn write_pdu(stream: &mut TcpStream, pdu: &Pdu) -> Result<(), SmppError> {
    tokio::time::timeout(IO_TIMEOUT, stream.write_all(&pdu.to_bytes()))
        .await
        .map_err(|_| SmppError::Io("tempo esgotado a escrever".into()))?
        .map_err(|e| SmppError::Io(e.to_string()))
}

pub async fn read_pdu(stream: &mut TcpStream) -> Result<Pdu, SmppError> {
    let mut head = [0u8; 16];
    tokio::time::timeout(IO_TIMEOUT, stream.read_exact(&mut head))
        .await
        .map_err(|_| SmppError::Io("tempo esgotado à espera do SMSC".into()))?
        .map_err(|e| SmppError::Io(e.to_string()))?;
    let len = u32::from_be_bytes(head[0..4].try_into().unwrap_or_default());
    if !(16..=MAX_PDU).contains(&len) {
        return Err(SmppError::Protocol(format!(
            "command_length {len} fora dos limites"
        )));
    }
    let mut body = vec![0u8; len as usize - 16];
    tokio::time::timeout(IO_TIMEOUT, stream.read_exact(&mut body))
        .await
        .map_err(|_| SmppError::Io("tempo esgotado à espera do SMSC".into()))?
        .map_err(|e| SmppError::Io(e.to_string()))?;
    let field = |i: usize| u32::from_be_bytes(head[i..i + 4].try_into().unwrap_or_default());
    Ok(Pdu {
        command_id: field(4),
        status: field(8),
        sequence: field(12),
        body,
    })
}

/// Pede e espera a resposta com o MESMO número de sequência. Um `enquire_link`
/// do SMSC pelo meio é respondido e ignorado.
async fn call(stream: &mut TcpStream, pdu: Pdu, name: &'static str) -> Result<Pdu, SmppError> {
    let expected = pdu.command_id | RESP;
    let seq = pdu.sequence;
    write_pdu(stream, &pdu).await?;
    loop {
        let resp = read_pdu(stream).await?;
        if resp.command_id == 0x0000_0015 {
            write_pdu(
                stream,
                &Pdu {
                    command_id: 0x8000_0015,
                    status: 0,
                    sequence: resp.sequence,
                    body: vec![],
                },
            )
            .await?;
            continue;
        }
        if resp.command_id == GENERIC_NACK {
            return Err(SmppError::Status {
                command: name,
                status: resp.status,
            });
        }
        if resp.command_id != expected || resp.sequence != seq {
            return Err(SmppError::Protocol(format!(
                "esperava 0x{expected:08X}#{seq}, recebi 0x{:08X}#{}",
                resp.command_id, resp.sequence
            )));
        }
        if resp.status != 0 {
            return Err(SmppError::Status {
                command: name,
                status: resp.status,
            });
        }
        return Ok(resp);
    }
}

fn read_cstr(body: &[u8]) -> String {
    let end = body.iter().position(|&b| b == 0).unwrap_or(body.len());
    String::from_utf8_lossy(&body[..end]).into_owned()
}

/// Envia uma mensagem (todas as partes) numa ligação nova. Devolve os
/// `message_id` do SMSC, um por parte, separados por vírgula.
pub async fn send(
    link: &SmppLink,
    dest_e164: &str,
    enc: &Encoded,
    reference: u8,
) -> Result<String, SmppError> {
    let dest =
        crate::sms_codec::e164_digits(dest_e164).map_err(|e| SmppError::Protocol(e.to_string()))?;
    let addr = format!("{}:{}", link.host, link.port);
    let mut stream = tokio::time::timeout(IO_TIMEOUT, TcpStream::connect(&addr))
        .await
        .map_err(|_| SmppError::Io(format!("tempo esgotado a ligar a {addr}")))?
        .map_err(|e| SmppError::Io(e.to_string()))?;

    let mut seq = 1u32;
    call(
        &mut stream,
        Pdu {
            command_id: BIND_TRANSMITTER,
            status: 0,
            sequence: seq,
            body: bind_transmitter_body(link),
        },
        "bind_transmitter",
    )
    .await?;

    let total = enc.parts.len();
    let mut ids = Vec::with_capacity(total);
    for (i, part) in enc.parts.iter().enumerate() {
        seq += 1;
        let udh = concat_udh(reference, total, i + 1);
        let body = submit_sm_body(&link.source_addr, &dest, enc.encoding, &udh, part);
        let resp = call(
            &mut stream,
            Pdu {
                command_id: SUBMIT_SM,
                status: 0,
                sequence: seq,
                body,
            },
            "submit_sm",
        )
        .await?;
        ids.push(read_cstr(&resp.body));
    }

    // O unbind é cortesia: as partes já foram aceites, uma falha aqui não as desfaz.
    seq += 1;
    let _ = call(
        &mut stream,
        Pdu {
            command_id: UNBIND,
            status: 0,
            sequence: seq,
            body: vec![],
        },
        "unbind",
    )
    .await;
    Ok(ids.join(","))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sms_codec::encode;
    use tokio::net::TcpListener;

    /// SMSC falso: aceita (ou recusa) o bind, responde a cada submit_sm com um
    /// id, e devolve os corpos dos submit_sm que recebeu.
    async fn fake_smsc(bind_status: u32) -> (u16, tokio::task::JoinHandle<Vec<Vec<u8>>>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let handle = tokio::spawn(async move {
            let (mut s, _) = listener.accept().await.unwrap();
            let mut submits = Vec::new();
            loop {
                let Ok(p) = read_pdu(&mut s).await else { break };
                let (status, body) = match p.command_id {
                    BIND_TRANSMITTER => (bind_status, b"SMSC\0".to_vec()),
                    SUBMIT_SM => {
                        submits.push(p.body.clone());
                        (0, format!("id{}\0", submits.len()).into_bytes())
                    }
                    _ => (0, vec![]),
                };
                let resp = Pdu {
                    command_id: p.command_id | RESP,
                    status,
                    sequence: p.sequence,
                    body,
                };
                s.write_all(&resp.to_bytes()).await.unwrap();
                if p.command_id == UNBIND || status != 0 {
                    break;
                }
            }
            submits
        });
        (port, handle)
    }

    fn link(port: u16) -> SmppLink {
        SmppLink::parse(&format!(
            "smpp://delonix:s%40gredo@127.0.0.1:{port}?source_addr=DELONIX"
        ))
        .unwrap()
    }

    #[test]
    fn parse_decodes_password_and_hides_it_in_debug() {
        let l = SmppLink::parse("smpp://sys:p%40ss@smsc.example:2776?source_addr=DELONIX").unwrap();
        assert_eq!(
            (l.host.as_str(), l.port, l.system_id.as_str()),
            ("smsc.example", 2776, "sys")
        );
        assert_eq!(l.password, "p@ss");
        assert_eq!(l.source_addr, "DELONIX");
        assert!(!format!("{l:?}").contains("p@ss"));
        assert!(SmppLink::parse("http://sys:p@h").is_err());
        assert!(SmppLink::parse("smpp://h:2775").is_err(), "sem system_id");
    }

    #[test]
    fn bind_body_has_version_34() {
        let l = SmppLink::parse("smpp://sys:pw@h").unwrap();
        assert_eq!(bind_transmitter_body(&l), b"sys\0pw\0\0\x34\0\0\0".to_vec());
    }

    #[tokio::test]
    async fn sends_every_part_against_a_fake_smsc() {
        let (port, smsc) = fake_smsc(0).await;
        let enc = encode(&"a".repeat(200)).unwrap();
        let ids = send(&link(port), "+244923000000", &enc, 7).await.unwrap();
        assert_eq!(ids, "id1,id2");
        let submits = smsc.await.unwrap();
        assert_eq!(submits.len(), 2);
        let first = &submits[0];
        // service_type "" · ton 5 npi 0 (alfanumérico) · "DELONIX" · ton 1 npi 1 · destino
        assert!(first.starts_with(b"\0\x05\x00DELONIX\0\x01\x01244923000000\0\x40"));
        let sm_len = first[first.len() - 1 - 153 - 6] as usize;
        assert_eq!(sm_len, 6 + 153);
    }

    #[tokio::test]
    async fn a_refused_bind_is_an_error_with_the_status() {
        let (port, _smsc) = fake_smsc(0x0000_000E).await; // ESME_RINVPASWD
        let enc = encode("ola").unwrap();
        let err = send(&link(port), "+244923000000", &enc, 1)
            .await
            .unwrap_err();
        assert!(
            matches!(
                err,
                SmppError::Status {
                    command: "bind_transmitter",
                    status: 0x0E
                }
            ),
            "{err}"
        );
    }
}
