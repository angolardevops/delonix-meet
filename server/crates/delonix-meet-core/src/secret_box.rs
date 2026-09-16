//! Cifra de segredos EM REPOUSO (auditoria 2026-09-16, S5).
//!
//! Segredos de integração que o servidor tem de voltar a LER — chave RTMP de
//! um destino de emissão, password do WebDAV, `client_secret` do SSO, segredo
//! HMAC de um webhook — não podem ser um hash, e estavam em claro na base.
//! Quem lê um dump ou uma réplica levava as credenciais de terceiros de todos
//! os inquilinos.
//!
//! Formato guardado: `enc:v1:<kid>:<base64url(nonce‖ciphertext‖tag)>`, com
//! AES-256-GCM e nonce aleatório de 96 bits. O `aad` liga o texto cifrado ao
//! seu CONTEXTO (tabela, coluna, id da linha): copiar o valor de uma linha
//! para outra falha a autenticação em vez de entregar o segredo alheio.
//!
//! Rotação: `DATA_ENCRYPTION_KEYS="k2:<b64>,k1:<b64>"` — a primeira cifra, todas
//! decifram. Um valor sem o prefixo `enc:` é texto claro HERDADO: lê-se como
//! está (migração preguiçosa) e passa a cifrado na próxima escrita.

use aes_gcm::{
    aead::{Aead, Payload},
    Aes256Gcm, KeyInit, Nonce,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};

use crate::error::DomainError;

const PREFIX: &str = "enc:v1:";

pub struct SecretBox {
    /// (kid, chave). A primeira é a activa.
    keys: Vec<(String, Aes256Gcm)>,
}

impl std::fmt::Debug for SecretBox {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Nunca as chaves; só quantas e quais ids.
        f.debug_struct("SecretBox")
            .field(
                "kids",
                &self.keys.iter().map(|(k, _)| k).collect::<Vec<_>>(),
            )
            .finish()
    }
}

impl SecretBox {
    /// Lê `kid:base64,kid:base64`. Cada chave tem de ter 32 bytes.
    pub fn from_spec(spec: &str) -> Result<Self, String> {
        let mut keys = Vec::new();
        for part in spec.split(',').map(str::trim).filter(|p| !p.is_empty()) {
            let (kid, b64) = part
                .split_once(':')
                .ok_or_else(|| format!("«{part}» não é kid:base64"))?;
            if kid.is_empty() || !kid.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') {
                return Err(format!("kid inválido «{kid}» (letras, dígitos, hífen)"));
            }
            let raw = base64::engine::general_purpose::STANDARD
                .decode(b64)
                .or_else(|_| URL_SAFE_NO_PAD.decode(b64))
                .map_err(|_| format!("chave «{kid}» não é base64"))?;
            let key: [u8; 32] = raw
                .try_into()
                .map_err(|_| format!("chave «{kid}» tem de ter 32 bytes"))?;
            keys.push((kid.to_string(), Aes256Gcm::new(&key.into())));
        }
        if keys.is_empty() {
            return Err("nenhuma chave".into());
        }
        Ok(Self { keys })
    }

    /// Chave única derivada de um material (desenvolvimento: sem
    /// `DATA_ENCRYPTION_KEYS` e com `DELONIX_ALLOW_INSECURE=1`).
    pub fn derived_for_dev(material: &str) -> Self {
        let key = crate::crypto::sha256(format!("delonix-dev-dek:{material}"));
        Self {
            keys: vec![("dev".into(), Aes256Gcm::new(&key.into()))],
        }
    }

    /// Cifra `plaintext` ligado ao contexto `aad`.
    pub fn seal(&self, plaintext: &str, aad: &str) -> String {
        let (kid, key) = &self.keys[0];
        let nonce_bytes = crate::crypto::random_bytes::<12>();
        let nonce = Nonce::from(nonce_bytes);
        let ct = key
            .encrypt(
                &nonce,
                Payload {
                    msg: plaintext.as_bytes(),
                    aad: aad.as_bytes(),
                },
            )
            .expect("AES-GCM não falha a cifrar");
        let mut blob = nonce_bytes.to_vec();
        blob.extend_from_slice(&ct);
        format!("{PREFIX}{kid}:{}", URL_SAFE_NO_PAD.encode(blob))
    }

    /// Decifra. Texto sem prefixo `enc:` é herdado em claro e volta como está.
    pub fn open(&self, stored: &str, aad: &str) -> Result<String, DomainError> {
        let Some(rest) = stored.strip_prefix(PREFIX) else {
            return Ok(stored.to_string());
        };
        let fail = || {
            DomainError::internal("segredo cifrado ilegível (chave em falta ou contexto errado)")
        };
        let (kid, b64) = rest.split_once(':').ok_or_else(fail)?;
        let (_, key) = self.keys.iter().find(|(k, _)| k == kid).ok_or_else(fail)?;
        let blob = URL_SAFE_NO_PAD.decode(b64).map_err(|_| fail())?;
        if blob.len() < 12 + 16 {
            return Err(fail());
        }
        let (n, ct) = blob.split_at(12);
        let nonce = Nonce::try_from(n).map_err(|_| fail())?;
        let pt = key
            .decrypt(
                &nonce,
                Payload {
                    msg: ct,
                    aad: aad.as_bytes(),
                },
            )
            .map_err(|_| fail())?;
        String::from_utf8(pt).map_err(|_| fail())
    }

    /// Está cifrado (com qualquer kid)?
    pub fn is_sealed(stored: &str) -> bool {
        stored.starts_with(PREFIX)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key_b64(byte: u8) -> String {
        base64::engine::general_purpose::STANDARD.encode([byte; 32])
    }

    #[test]
    fn roundtrip_and_context_binding() {
        let sb = SecretBox::from_spec(&format!("k1:{}", key_b64(7))).unwrap();
        let sealed = sb.seal("live_abc123", "stream_destinations.stream_key:row-1");
        assert!(SecretBox::is_sealed(&sealed));
        assert!(!sealed.contains("live_abc123"));
        assert_eq!(
            sb.open(&sealed, "stream_destinations.stream_key:row-1")
                .unwrap(),
            "live_abc123"
        );
        // Copiado para outra linha: não abre.
        assert!(sb
            .open(&sealed, "stream_destinations.stream_key:row-2")
            .is_err());
        // Nonce aleatório: dois selos do mesmo texto diferem.
        assert_ne!(
            sealed,
            sb.seal("live_abc123", "stream_destinations.stream_key:row-1")
        );
    }

    #[test]
    fn rotation_decrypts_old_and_seals_with_new() {
        let old = SecretBox::from_spec(&format!("k1:{}", key_b64(1))).unwrap();
        let sealed_old = old.seal("s", "ctx");
        let rotated =
            SecretBox::from_spec(&format!("k2:{},k1:{}", key_b64(2), key_b64(1))).unwrap();
        assert_eq!(rotated.open(&sealed_old, "ctx").unwrap(), "s");
        assert!(rotated.seal("s", "ctx").starts_with("enc:v1:k2:"));
        // Sem a chave antiga, falha fechado.
        let only_new = SecretBox::from_spec(&format!("k2:{}", key_b64(2))).unwrap();
        assert!(only_new.open(&sealed_old, "ctx").is_err());
    }

    #[test]
    fn legacy_plaintext_reads_as_is() {
        let sb = SecretBox::derived_for_dev("x");
        assert_eq!(
            sb.open("password-antiga", "ctx").unwrap(),
            "password-antiga"
        );
    }

    #[test]
    fn bad_specs_are_refused() {
        assert!(SecretBox::from_spec("").is_err());
        assert!(SecretBox::from_spec("k1:curta").is_err());
        assert!(SecretBox::from_spec(&format!("k 1:{}", key_b64(1))).is_err());
        let dbg = format!(
            "{:?}",
            SecretBox::from_spec(&format!("k1:{}", key_b64(9))).unwrap()
        );
        assert!(!dbg.contains(&key_b64(9)));
    }
}
