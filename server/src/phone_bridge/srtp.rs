//! SRTP e lista de origens da ponte (ADR-0010 §5).
//!
//! **De onde vem este código.** É a metade de SEGURANÇA do `pstn_bridge.rs`
//! (Abordagem B), escrito noutra sessão: perfil SRTP, chaves efémeras,
//! protecção contra repetição e allowlist fail-closed. Essa metade é melhor do
//! que a que a ponte telefone↔sala tinha (RTP em claro na rede interna) e
//! fica; o que saiu foi o TRANSPORTE dela (o FreeSWITCH a mandar SRTP para um
//! `host:porta` arbitrário, mecanismo que nunca se conseguiu verificar) e o
//! MISTURADOR dela (`mix_pcm`, que presumia Opus na perna PSTN). Ver
//! `docs/pstn-sfu-bridge-design.md` §Superseded e o ADR-0010 §5.
//!
//! **O que muda no modelo de chaves, e porquê.** Na Abordagem B as chaves eram
//! POR SALA e saíam por um canal lateral (a resposta JSON do IVR). Com o UA SIP
//! (R222) há um diálogo SIP por chamada, e é nele que as chaves viajam: SDES
//! (`a=crypto`, RFC 4568) no `INVITE` e na resposta. Isso torna-as **por
//! chamada** em vez de por sala, e tira-lhes o canal lateral — as duas coisas
//! sobem a barra em vez de a baixar:
//!
//! - uma chave comprometida expõe UMA chamada, não todas as chamadas daquela
//!   sala enquanto a ponte estiver activa;
//! - a chave deixa de existir num corpo JSON que o Lua do IVR passava por
//!   variáveis de canal do FreeSWITCH (e portanto nos logs dele).
//!
//! O que NÃO muda: SRTP obrigatório nos dois sentidos (uma oferta sem
//! `a=crypto` é recusada com `488` — ver `sip.rs`), chave e sal diferentes por
//! sentido, e allowlist de IP fail-closed.

use std::net::IpAddr;

use base64::Engine as _;
use webrtc_srtp::{context::Context as SrtpContext, protection_profile::ProtectionProfile};

/// Perfil SRTP usado nos dois sentidos — o mesmo default que o `webrtc-rs`
/// negoceia por DTLS-SRTP nas ligações normais do SFU (ver `SettingEngine` em
/// `sfu.rs::new_api`; não é configurável aí, e não há razão para o ser aqui).
pub const SRTP_PROFILE: ProtectionProfile = ProtectionProfile::Aes128CmHmacSha1_80;
pub const SRTP_PROFILE_NAME: &str = "AES_CM_128_HMAC_SHA1_80";
pub const SRTP_KEY_LEN: usize = 16;
pub const SRTP_SALT_LEN: usize = 14;
/// Janela do detector de repetição SRTP (pacotes). 64 é o valor de referência
/// usado noutras pilhas SRTP (libsrtp); não há aqui negociação a copiar.
const SRTP_REPLAY_WINDOW: usize = 64;

/// A perna só aceita pacotes cujo IP de origem é um dos FreeSWITCH
/// configurados. **Fail-closed**: lista vazia recusa TODOS — o mesmo padrão que
/// `voice_internal_secret` vazio usa em `voice.rs` (funcionalidade
/// indisponível é mais seguro que aberta por omissão). Uma porta UDP que
/// publica numa reunião o que lhe chegar seria uma porta para dentro de
/// qualquer sala.
///
/// Vinha do `pstn_bridge.rs` com um `Option<IpAddr>` (um só FreeSWITCH);
/// generalizou-se para lista porque a perna e o UA SIP já a tinham assim e
/// duas funções de allowlist é uma a mais. `&[]` é o antigo `None`.
pub fn ip_allowed(allowed: &[IpAddr], remote: IpAddr) -> bool {
    allowed.contains(&remote)
}

/// Par de chaves SRTP efémeras (AES_CM_128_HMAC_SHA1_80: chave de 16 bytes +
/// sal de 14). Uma instância por SENTIDO — nunca a mesma chave nos dois
/// sentidos, a mesma disciplina que DTLS-SRTP aplica com
/// client-write-key/server-write-key distintas.
#[derive(Clone)]
pub struct SrtpKeyPair {
    pub master_key: [u8; SRTP_KEY_LEN],
    pub master_salt: [u8; SRTP_SALT_LEN],
}

impl std::fmt::Debug for SrtpKeyPair {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Uma chave SRTP num log é uma chave comprometida.
        f.write_str("SrtpKeyPair(<redigido>)")
    }
}

impl SrtpKeyPair {
    /// Chave nova, aleatória — via `crypto::random_bytes` (CSPRNG do SO), não
    /// um `rand::thread_rng()` próprio: ADR-0004 §5 regra 4 quer toda a
    /// aleatoriedade de segurança num só sítio (`crypto.rs`), verificado pela
    /// catraca de arquitectura (`scripts/check-arquitectura-catraca.sh`).
    /// Chamar uma vez por SENTIDO e por CHAMADA — nunca reutilizar entre
    /// chamadas ou entre salas.
    pub fn generate() -> Self {
        let master_key: [u8; SRTP_KEY_LEN] = crate::crypto::random_bytes(SRTP_KEY_LEN)
            .try_into()
            .expect("random_bytes devolve exactamente o comprimento pedido");
        let master_salt: [u8; SRTP_SALT_LEN] = crate::crypto::random_bytes(SRTP_SALT_LEN)
            .try_into()
            .expect("random_bytes devolve exactamente o comprimento pedido");
        Self {
            master_key,
            master_salt,
        }
    }

    /// `master_key || master_salt`, base64 — a convenção do `inline:` do
    /// `a=crypto` do SDES-SRTP (RFC 4568 §9.1).
    pub fn to_b64(&self) -> String {
        let mut buf = Vec::with_capacity(SRTP_KEY_LEN + SRTP_SALT_LEN);
        buf.extend_from_slice(&self.master_key);
        buf.extend_from_slice(&self.master_salt);
        base64::engine::general_purpose::STANDARD.encode(buf)
    }

    /// O inverso: lê o `inline:` que a outra ponta ofereceu. `None` se não for
    /// base64 ou se não tiver exactamente 30 bytes — nunca se deriva um
    /// contexto de material do tamanho errado.
    pub fn from_b64(s: &str) -> Option<Self> {
        // O `inline:` pode trazer parâmetros de tempo de vida e MKI depois de
        // `|` (RFC 4568 §9.1); nenhum é suportado, e o material é o que vem
        // antes do primeiro `|`.
        let material = s.split('|').next()?.trim();
        let raw = base64::engine::general_purpose::STANDARD
            .decode(material)
            .ok()?;
        if raw.len() != SRTP_KEY_LEN + SRTP_SALT_LEN {
            return None;
        }
        Some(Self {
            master_key: raw[..SRTP_KEY_LEN].try_into().ok()?,
            master_salt: raw[SRTP_KEY_LEN..].try_into().ok()?,
        })
    }

    /// Um `Context` SRTP novo a partir desta chave. Cada `Context` só deve ser
    /// usado num sentido (o próprio crate documenta isto — o estado de
    /// repetição/ROC é por direcção).
    pub fn context(&self) -> Result<SrtpContext, webrtc_srtp::Error> {
        SrtpContext::new(
            &self.master_key,
            &self.master_salt,
            SRTP_PROFILE,
            Some(webrtc_srtp::option::srtp_replay_protection(
                SRTP_REPLAY_WINDOW,
            )),
            Some(webrtc_srtp::option::srtcp_replay_protection(
                SRTP_REPLAY_WINDOW,
            )),
        )
    }
}

/// Uma linha `a=crypto` do SDP (RFC 4568 §9.1): `<tag> <suite> inline:<chave>`.
#[derive(Debug, Clone)]
pub struct SdesCrypto {
    pub tag: u32,
    pub keys: SrtpKeyPair,
}

impl SdesCrypto {
    /// Lê a PRIMEIRA linha `a=crypto` com a suite que sabemos fazer. Linhas com
    /// outra suite (`AES_256_*`, `AEAD_AES_128_GCM`, …) saltam-se: responder com
    /// uma suite que não se implementa daria uma chamada muda. `None` = a
    /// oferta não traz SRTP utilizável, e quem chama recusa (`488`).
    pub fn from_sdp(sdp: &str) -> Option<SdesCrypto> {
        for line in sdp.lines().map(str::trim) {
            let Some(rest) = line.strip_prefix("a=crypto:") else {
                continue;
            };
            let mut it = rest.split_whitespace();
            let tag: u32 = it.next()?.parse().ok()?;
            if !it.next().is_some_and(|s| s == SRTP_PROFILE_NAME) {
                continue;
            }
            let Some(keys) = it
                .next()
                .and_then(|p| p.strip_prefix("inline:"))
                .and_then(SrtpKeyPair::from_b64)
            else {
                continue;
            };
            return Some(SdesCrypto { tag, keys });
        }
        None
    }

    /// A linha a pôr na resposta, com a NOSSA chave (a que a outra ponta usa
    /// para desencriptar o que lhe mandamos). A tag é a da oferta, como o RFC
    /// exige para uma resposta a uma oferta unária.
    pub fn answer_line(&self) -> String {
        format!(
            "a=crypto:{} {SRTP_PROFILE_NAME} inline:{}\r\n",
            self.tag,
            self.keys.to_b64()
        )
    }
}

/// As duas direcções de uma chamada: cifrar com a nossa chave, desencriptar com
/// a que a outra ponta ofereceu. Construir uma por chamada.
pub struct SrtpSession {
    /// Contexto de saída (a nossa chave, anunciada na resposta SDP).
    pub outbound: SrtpContext,
    /// Contexto de entrada (a chave que o FreeSWITCH ofereceu).
    pub inbound: SrtpContext,
}

impl SrtpSession {
    pub fn new(local: &SrtpKeyPair, remote: &SrtpKeyPair) -> Result<Self, webrtc_srtp::Error> {
        Ok(SrtpSession {
            outbound: local.context()?,
            inbound: remote.context()?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- herdados do `pstn_bridge.rs` (allowlist e chaves) ----

    #[test]
    fn ip_allowlist_fail_closed_sem_config() {
        assert!(!ip_allowed(&[], "10.0.0.1".parse().unwrap()));
        assert!(!ip_allowed(&[], "127.0.0.1".parse().unwrap()));
    }

    #[test]
    fn ip_allowlist_aceita_so_o_ip_configurado() {
        let fs: IpAddr = "10.0.0.7".parse().unwrap();
        assert!(ip_allowed(&[fs], fs));
        assert!(!ip_allowed(&[fs], "10.0.0.8".parse().unwrap()));
        assert!(!ip_allowed(&[fs], "127.0.0.1".parse().unwrap()));
    }

    #[test]
    fn srtp_keypair_tem_o_comprimento_certo() {
        let k = SrtpKeyPair::generate();
        assert_eq!(k.master_key.len(), SRTP_KEY_LEN);
        assert_eq!(k.master_salt.len(), SRTP_SALT_LEN);
        let raw = base64::engine::general_purpose::STANDARD
            .decode(k.to_b64())
            .unwrap();
        assert_eq!(raw.len(), SRTP_KEY_LEN + SRTP_SALT_LEN);
    }

    #[test]
    fn srtp_keypair_e_efemera_e_unica_por_chamada() {
        let a = SrtpKeyPair::generate();
        let b = SrtpKeyPair::generate();
        assert_ne!(a.master_key, b.master_key);
        assert_ne!(a.master_salt, b.master_salt);
    }

    #[test]
    fn srtp_keypair_os_dois_sentidos_sao_chaves_diferentes() {
        let local = SrtpKeyPair::generate();
        let remote = SrtpKeyPair::generate();
        assert_ne!(local.master_key, remote.master_key);
        assert_ne!(local.to_b64(), remote.to_b64());
    }

    #[test]
    fn srtp_context_constroi_com_a_chave_gerada() {
        assert!(SrtpKeyPair::generate().context().is_ok());
        assert!(SrtpSession::new(&SrtpKeyPair::generate(), &SrtpKeyPair::generate()).is_ok());
    }

    #[test]
    fn chave_nunca_no_debug() {
        let k = SrtpKeyPair::generate();
        assert!(!format!("{k:?}").contains(&k.to_b64()[..8]));
    }

    // ---- novos: SDES no SDP ----

    #[test]
    fn le_o_crypto_da_oferta() {
        let nossa = SrtpKeyPair::generate();
        let sdp = format!(
            "v=0\r\nm=audio 4000 RTP/SAVP 8\r\na=rtpmap:8 PCMA/8000\r\n\
             a=crypto:1 {SRTP_PROFILE_NAME} inline:{}\r\n",
            nossa.to_b64()
        );
        let c = SdesCrypto::from_sdp(&sdp).expect("a oferta traz SRTP");
        assert_eq!(c.tag, 1);
        assert_eq!(c.keys.to_b64(), nossa.to_b64());
    }

    #[test]
    fn oferta_sem_crypto_ou_com_suite_que_nao_fazemos_nao_serve() {
        assert!(SdesCrypto::from_sdp("v=0\r\nm=audio 4000 RTP/AVP 8\r\n").is_none());
        let k = SrtpKeyPair::generate().to_b64();
        assert!(
            SdesCrypto::from_sdp(&format!("a=crypto:2 AEAD_AES_128_GCM inline:{k}\r\n")).is_none(),
            "uma suite que não implementamos não se aceita — daria chamada muda"
        );
    }

    #[test]
    fn a_suite_que_sabemos_ganha_mesmo_vindo_depois() {
        let k = SrtpKeyPair::generate();
        let sdp = format!(
            "a=crypto:1 AEAD_AES_128_GCM inline:{}\r\na=crypto:2 {SRTP_PROFILE_NAME} inline:{}\r\n",
            SrtpKeyPair::generate().to_b64(),
            k.to_b64()
        );
        let c = SdesCrypto::from_sdp(&sdp).unwrap();
        assert_eq!(c.tag, 2);
        assert_eq!(c.keys.to_b64(), k.to_b64());
    }

    #[test]
    fn material_do_tamanho_errado_nao_deriva_contexto() {
        use base64::engine::general_purpose::STANDARD;
        assert!(SrtpKeyPair::from_b64(&STANDARD.encode([0u8; 16])).is_none());
        assert!(SrtpKeyPair::from_b64(&STANDARD.encode([0u8; 31])).is_none());
        assert!(SrtpKeyPair::from_b64("não-é-base64!!").is_none());
        assert!(SrtpKeyPair::from_b64(&STANDARD.encode([7u8; 30])).is_some());
    }

    #[test]
    fn inline_com_tempo_de_vida_e_mki_le_se_a_chave() {
        let k = SrtpKeyPair::generate();
        let com_extras = format!("{}|2^20|1:4", k.to_b64());
        assert_eq!(
            SrtpKeyPair::from_b64(&com_extras).map(|p| p.to_b64()),
            Some(k.to_b64())
        );
    }

    #[test]
    fn a_resposta_leva_a_nossa_chave_e_a_tag_da_oferta() {
        let nossa = SrtpKeyPair::generate();
        let linha = SdesCrypto {
            tag: 7,
            keys: nossa.clone(),
        }
        .answer_line();
        assert!(linha.starts_with(&format!("a=crypto:7 {SRTP_PROFILE_NAME} inline:")));
        assert!(linha.contains(&nossa.to_b64()));
        assert!(linha.ends_with("\r\n"));
    }

    /// Ida e volta a sério: cifra com a chave local, desencripta com a mesma
    /// chave do outro lado, e um pacote cifrado com OUTRA chave é recusado.
    #[test]
    fn ida_e_volta_srtp_e_recusa_de_chave_errada() {
        use webrtc::rtp::{header::Header, packet::Packet};
        use webrtc::util::Marshal;
        let nossa = SrtpKeyPair::generate();
        let deles = SrtpKeyPair::generate();
        // Nós: cifra com a nossa, desencripta com a deles.
        let mut nos = SrtpSession::new(&nossa, &deles).unwrap();
        // Eles: cifra com a deles, desencripta com a nossa.
        let mut eles = SrtpSession::new(&deles, &nossa).unwrap();

        let pkt = Packet {
            header: Header {
                version: 2,
                payload_type: 8,
                sequence_number: 1234,
                timestamp: 160,
                ssrc: 0xDEAD_BEEF,
                ..Default::default()
            },
            payload: bytes::Bytes::from_static(&[0x55; 160]),
        };
        let claro = pkt.marshal().unwrap();
        let cifrado = nos.outbound.encrypt_rtp(&claro).unwrap();
        assert_ne!(&cifrado[12..], &claro[12..], "o payload saiu em claro");
        let de_volta = eles.inbound.decrypt_rtp(&cifrado).unwrap();
        assert_eq!(de_volta, claro);

        let mut intruso = SrtpSession::new(&SrtpKeyPair::generate(), &SrtpKeyPair::generate())
            .unwrap();
        let falso = intruso.outbound.encrypt_rtp(&claro).unwrap();
        assert!(
            eles.inbound.decrypt_rtp(&falso).is_err(),
            "um pacote com chave errada foi aceite"
        );
    }
}
