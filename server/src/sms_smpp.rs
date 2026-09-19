//! Cliente SMPP 3.4 mínimo para os operadores móveis (ADR-0005).
//!
//! Só o caminho de SAÍDA: `bind_transmitter` → `submit_sm` por parte → `unbind`,
//! numa ligação por lote. Não há `deliver_sm` (recibos, SMS recebidos) — está
//! escrito no ADR o que isso significa: `sent` é «o SMSC aceitou», não
//! «chegou».
//!
//! **TLS (`smpps://`)** está implementado: a ligação TCP é envolvida em
//! `tokio-rustls` antes do bind quando o URL usa o esquema `smpps://`, com o
//! certificado do SMSC validado contra as raízes do sistema (`webpki-roots`)
//! por omissão, ou contra uma CA própria do operador (`ca_file`,
//! `SMS_<OPERADOR>_SMPP_CA`) quando configurada. O framing do PDU
//! (`Pdu::to_bytes`, `read_pdu`, `write_pdu`) é o mesmo código para as duas
//! ligações — é genérico sobre `AsyncRead + AsyncWrite`, não uma cópia por
//! transporte.
//!
//! Nenhum contrato com a Unitel, a Movicel ou a Africell existe ainda. O que está
//! provado é o protocolo (texto simples e TLS) contra um SMSC falso (testes
//! abaixo); a interoperação com o SMSC real de cada operador só se prova com o
//! contrato na mão.

use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
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
    /// `true` para `smpps://` — a ligação TCP é envolvida em TLS antes do
    /// bind. `false` para `smpp://`, texto simples (o que já existia).
    pub tls: bool,
    /// Feixe de CA (caminho de um ficheiro PEM) para validar o certificado do
    /// SMSC quando não é emitido por uma CA pública. `None` valida contra as
    /// raízes do sistema (`webpki-roots`). Sem efeito com `tls: false`.
    pub ca_file: Option<String>,
}

impl std::fmt::Debug for SmppLink {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SmppLink")
            .field("host", &self.host)
            .field("port", &self.port)
            .field("system_id", &self.system_id)
            .field("password", &"<redigida>")
            .field("source_addr", &self.source_addr)
            .field("tls", &self.tls)
            .field("ca_file", &self.ca_file)
            .finish()
    }
}

impl SmppLink {
    /// `smpp://system_id:password@host:2775?source_addr=DELONIX` (texto
    /// simples) ou `smpps://…` (a mesma sintaxe, envolvida em TLS antes do
    /// bind).
    pub fn parse(raw: &str) -> Result<SmppLink, String> {
        let url = url::Url::parse(raw).map_err(|_| "URL SMPP inválido".to_string())?;
        let tls = match url.scheme() {
            "smpp" => false,
            "smpps" => true,
            other => {
                return Err(format!(
                    "esquema '{other}' desconhecido — tem de ser smpp:// ou smpps://"
                ))
            }
        };
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
            tls,
            ca_file: None,
        })
    }

    /// Define a CA própria do operador (chamar depois de `parse`, tipicamente
    /// a partir de `SMS_<OPERADOR>_SMPP_CA`). `None` mantém a validação pelas
    /// raízes do sistema.
    pub fn with_ca_file(mut self, ca_file: Option<String>) -> SmppLink {
        self.ca_file = ca_file;
        self
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

async fn write_pdu<S: AsyncWrite + Unpin>(stream: &mut S, pdu: &Pdu) -> Result<(), SmppError> {
    tokio::time::timeout(IO_TIMEOUT, stream.write_all(&pdu.to_bytes()))
        .await
        .map_err(|_| SmppError::Io("tempo esgotado a escrever".into()))?
        .map_err(|e| SmppError::Io(e.to_string()))
}

/// Genérico sobre o transporte: o mesmo código lê PDUs de um `TcpStream` em
/// texto simples ou de um `TlsStream<TcpStream>` — o framing SMPP não muda.
pub async fn read_pdu<S: AsyncRead + Unpin>(stream: &mut S) -> Result<Pdu, SmppError> {
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
async fn call<S: AsyncRead + AsyncWrite + Unpin>(
    stream: &mut S,
    pdu: Pdu,
    name: &'static str,
) -> Result<Pdu, SmppError> {
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

/// Monta o `ClientConfig` de TLS para `link`: a CA própria do operador
/// (`link.ca_file`, um ficheiro PEM com um ou mais certificados) quando
/// configurada, senão as raízes do sistema (`webpki-roots`). Nunca aceita
/// qualquer certificado — não há um modo "sem verificação".
fn tls_connector(link: &SmppLink) -> Result<tokio_rustls::TlsConnector, SmppError> {
    let mut roots = rustls::RootCertStore::empty();
    match &link.ca_file {
        Some(path) => {
            let raw = std::fs::read(path)
                .map_err(|e| SmppError::Io(format!("a ler a CA '{path}': {e}")))?;
            let pems = pem::parse_many(&raw)
                .map_err(|e| SmppError::Io(format!("CA inválida em '{path}': {e}")))?;
            if pems.is_empty() {
                return Err(SmppError::Io(format!("nenhum certificado PEM em '{path}'")));
            }
            for p in pems {
                let der = rustls::pki_types::CertificateDer::from(p.into_contents());
                roots
                    .add(der)
                    .map_err(|e| SmppError::Io(format!("CA inválida em '{path}': {e}")))?;
            }
        }
        None => roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned()),
    }
    let config = rustls::ClientConfig::builder()
        .with_root_certificates(roots)
        .with_no_client_auth();
    Ok(tokio_rustls::TlsConnector::from(Arc::new(config)))
}

/// Envia uma mensagem (todas as partes) numa ligação nova. Devolve os
/// `message_id` do SMSC, um por parte, separados por vírgula.
///
/// `smpp://` liga por TCP simples; `smpps://` (`link.tls`) envolve a mesma
/// ligação em TLS antes do bind — o resto da sequência (`bind_transmitter` →
/// `submit_sm`* → `unbind`) é código único, partilhado pelos dois transportes
/// em `send_over`.
pub async fn send(
    link: &SmppLink,
    dest_e164: &str,
    enc: &Encoded,
    reference: u8,
) -> Result<String, SmppError> {
    let dest =
        crate::sms_codec::e164_digits(dest_e164).map_err(|e| SmppError::Protocol(e.to_string()))?;
    let addr = format!("{}:{}", link.host, link.port);
    let tcp = tokio::time::timeout(IO_TIMEOUT, TcpStream::connect(&addr))
        .await
        .map_err(|_| SmppError::Io(format!("tempo esgotado a ligar a {addr}")))?
        .map_err(|e| SmppError::Io(e.to_string()))?;

    if link.tls {
        let connector = tls_connector(link)?;
        let server_name = rustls::pki_types::ServerName::try_from(link.host.clone())
            .map_err(|e| SmppError::Io(format!("nome de anfitrião TLS inválido: {e}")))?;
        let mut tls_stream = tokio::time::timeout(IO_TIMEOUT, connector.connect(server_name, tcp))
            .await
            .map_err(|_| SmppError::Io("tempo esgotado no TLS".into()))?
            .map_err(|e| SmppError::Io(format!("TLS: {e}")))?;
        send_over(&mut tls_stream, link, &dest, enc, reference).await
    } else {
        let mut tcp = tcp;
        send_over(&mut tcp, link, &dest, enc, reference).await
    }
}

/// A sequência SMPP em si — `bind_transmitter` → `submit_sm` por parte →
/// `unbind` —, igual para texto simples e para TLS: `S` é só
/// `AsyncRead + AsyncWrite`, o protocolo não sabe (nem precisa saber) qual dos
/// dois é.
async fn send_over<S: AsyncRead + AsyncWrite + Unpin>(
    stream: &mut S,
    link: &SmppLink,
    dest: &str,
    enc: &Encoded,
    reference: u8,
) -> Result<String, SmppError> {
    let mut seq = 1u32;
    call(
        stream,
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
        let body = submit_sm_body(&link.source_addr, dest, enc.encoding, &udh, part);
        let resp = call(
            stream,
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
        stream,
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
    use uuid::Uuid;

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

    /// SMSC falso sobre TLS: mesma sequência do `fake_smsc`, mas a ligação
    /// aceite é envolvida num `TlsStream` com um certificado auto-assinado
    /// gerado na hora (`rcgen`). Devolve a porta, o certificado em PEM (para
    /// servir de CA própria ao cliente do teste) e o handle do servidor.
    /// Uma falha no aperto de mão TLS não é pânico — devolve submits vazios,
    /// para o teste que prova a REJEIÇÃO de uma CA errada não poluir a saída.
    async fn fake_tls_smsc(
        bind_status: u32,
    ) -> (u16, String, tokio::task::JoinHandle<Vec<Vec<u8>>>) {
        let rcgen::CertifiedKey { cert, key_pair } =
            rcgen::generate_simple_self_signed(vec!["127.0.0.1".to_string()]).unwrap();
        let cert_pem = cert.pem();
        let cert_der = cert.der().clone();
        let key_der = rustls::pki_types::PrivateKeyDer::Pkcs8(key_pair.serialize_der().into());
        let server_config = rustls::ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(vec![cert_der], key_der)
            .unwrap();
        let acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(server_config));

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let handle = tokio::spawn(async move {
            let (tcp, _) = listener.accept().await.unwrap();
            let mut s = match acceptor.accept(tcp).await {
                Ok(s) => s,
                Err(_) => return Vec::new(),
            };
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
        (port, cert_pem, handle)
    }

    /// Escreve `pem` num ficheiro temporário e devolve o caminho — simula o
    /// `SMS_<OPERADOR>_SMPP_CA` a apontar para um feixe no disco.
    fn write_temp_pem(pem: &str) -> String {
        let path =
            std::env::temp_dir().join(format!("delonix-smpps-test-ca-{}.pem", Uuid::new_v4()));
        std::fs::write(&path, pem).unwrap();
        path.to_string_lossy().into_owned()
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
        assert!(!l.tls);
        assert!(l.ca_file.is_none());
        assert!(!format!("{l:?}").contains("p@ss"));
        assert!(SmppLink::parse("http://sys:p@h").is_err());
        assert!(SmppLink::parse("smpp://h:2775").is_err(), "sem system_id");
    }

    #[test]
    fn parse_accepts_smpp_and_smpps_schemes() {
        let plain = SmppLink::parse("smpp://sys:pw@h:2775").unwrap();
        assert!(!plain.tls, "smpp:// é texto simples");

        let tls = SmppLink::parse("smpps://sys:pw@h:2775").unwrap();
        assert!(tls.tls, "smpps:// liga por TLS");
        // host/porta/credenciais são interpretados da mesma forma nos dois esquemas.
        assert_eq!(
            (tls.host.as_str(), tls.port),
            (plain.host.as_str(), plain.port)
        );
    }

    #[test]
    fn parse_rejects_unknown_scheme() {
        let err = SmppLink::parse("smpps2://sys:pw@h:2775").unwrap_err();
        assert!(err.contains("smpp"), "{err}");
        let err = SmppLink::parse("ftp://sys:pw@h:2775").unwrap_err();
        assert!(err.contains("smpp"), "{err}");
    }

    #[test]
    fn with_ca_file_only_sets_the_ca_leaves_the_rest() {
        let l = SmppLink::parse("smpps://sys:pw@h:2775")
            .unwrap()
            .with_ca_file(Some("/etc/delonix/unitel-ca.pem".to_string()));
        assert_eq!(l.ca_file.as_deref(), Some("/etc/delonix/unitel-ca.pem"));
        assert!(l.tls);
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

    /// A mesma sequência bind→submit×2→unbind que `sends_every_part_against_a_fake_smsc`,
    /// mas com o TCP envolvido em TLS antes do bind — prova que `send_over`
    /// (o framing do PDU) é mesmo o MESMO código nos dois transportes: só
    /// `send()` decide se há um aperto de mão TLS pelo meio.
    #[tokio::test]
    async fn sends_every_part_against_a_fake_smsc_over_tls() {
        let (port, cert_pem, smsc) = fake_tls_smsc(0).await;
        let ca_path = write_temp_pem(&cert_pem);
        let enc = encode(&"a".repeat(200)).unwrap();
        let link = SmppLink::parse(&format!(
            "smpps://delonix:s%40gredo@127.0.0.1:{port}?source_addr=DELONIX"
        ))
        .unwrap()
        .with_ca_file(Some(ca_path.clone()));

        let ids = send(&link, "+244923000000", &enc, 7).await.unwrap();

        assert_eq!(ids, "id1,id2");
        let submits = smsc.await.unwrap();
        assert_eq!(submits.len(), 2);
        let first = &submits[0];
        assert!(first.starts_with(b"\0\x05\x00DELONIX\0\x01\x01244923000000\0\x40"));
        let _ = std::fs::remove_file(&ca_path);
    }

    /// A parte que mais importa nesta tarefa: sem a CA certa (nem uma raiz
    /// pública reconhece um certificado de teste auto-assinado), a ligação
    /// TEM de falhar — nunca aceitar em silêncio. Prova que não há um modo
    /// "confia em tudo" escondido por omissão.
    #[tokio::test]
    async fn tls_without_a_trusted_ca_is_rejected_not_silently_accepted() {
        let (port, _cert_pem, smsc) = fake_tls_smsc(0).await;
        let enc = encode("ola").unwrap();
        // Sem `with_ca_file`: valida contra as raízes do sistema — e nenhuma
        // delas conhece este certificado gerado na hora.
        let link = SmppLink::parse(&format!("smpps://delonix:segredo@127.0.0.1:{port}")).unwrap();

        let err = send(&link, "+244923000000", &enc, 1).await.unwrap_err();

        assert!(matches!(err, SmppError::Io(_)), "{err}");
        // O SMSC falso nunca recebeu um bind válido: o aperto de mão TLS
        // morreu antes disso.
        let submits = smsc.await.unwrap();
        assert!(submits.is_empty());
    }

    /// A CA errada (não a que assinou o certificado do SMSC) tem de falhar
    /// tal como a ausência de CA — não basta "ter alguma CA configurada".
    #[tokio::test]
    async fn tls_with_the_wrong_ca_is_rejected() {
        let (port, _cert_pem, smsc) = fake_tls_smsc(0).await;
        // Uma CA auto-assinada DIFERENTE da que o SMSC falso está a usar.
        let rcgen::CertifiedKey {
            cert: other_cert, ..
        } = rcgen::generate_simple_self_signed(vec!["127.0.0.1".to_string()]).unwrap();
        let wrong_ca_path = write_temp_pem(&other_cert.pem());
        let enc = encode("ola").unwrap();
        let link = SmppLink::parse(&format!("smpps://delonix:segredo@127.0.0.1:{port}"))
            .unwrap()
            .with_ca_file(Some(wrong_ca_path.clone()));

        let err = send(&link, "+244923000000", &enc, 1).await.unwrap_err();

        assert!(matches!(err, SmppError::Io(_)), "{err}");
        let submits = smsc.await.unwrap();
        assert!(submits.is_empty());
        let _ = std::fs::remove_file(&wrong_ca_path);
    }
}
