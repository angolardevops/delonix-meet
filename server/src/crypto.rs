//! Cifra de segredos em repouso — o primeiro habitante do `crypto.rs` que o
//! ADR-0004 §6 (passo 3) e a catraca da arquitectura (regra 4) pedem.
//!
//! PORQUÊ EXISTE. Até aqui os segredos de integração guardavam-se em claro na
//! base (S5 da auditoria de 2026-09-16). A primeira necessidade nova — a chave
//! de emissão dos destinos de directo guardados por organização — não podia
//! entrar da mesma forma: uma cópia da base (um backup, um `pg_dump` para
//! depurar) entregaria a chave do canal de YouTube de todas as empresas.
//!
//! O DESENHO, curto de propósito:
//!
//! - **AES-256-GCM**, que já é dependência (`aes-gcm`, usado no `recorder.rs`).
//! - **A chave vem da configuração** (`SECRETS_KEY`: 32 bytes em base64 ou em
//!   hex), nunca da base. Quem tem a base e não tem a config não lê nada.
//! - **Formato do blob:** `versão (1 byte) ‖ nonce (12) ‖ texto cifrado + tag (16)`.
//!   A versão existe para uma rotação futura poder coexistir com o formato de hoje.
//! - **Dados associados (AAD) obrigatórios.** Quem chama liga o blob ao SÍTIO
//!   onde ele vive (ex.: `stream_destination/<org>/<id>`). Assim um blob copiado
//!   de uma linha para outra — de outra organização — não decifra: a tag falha.
//!
//! O QUE NÃO FAZ (dito para ninguém o presumir): não há rotação de chave nem
//! chave anterior para decifrar; não há KMS/HSM. Perder a `SECRETS_KEY` é perder
//! os segredos guardados — têm de ser reintroduzidos.

use aes_gcm::aead::Aead;
use aes_gcm::{Aes256Gcm, KeyInit};
use base64::Engine;
use rand::{rngs::OsRng, RngCore};
use sha2::{Digest, Sha256};
use std::sync::Arc;

/// Versão do formato do blob. Muda só com uma migração de rotação.
const VERSION: u8 = 1;
const NONCE_LEN: usize = 12;
const TAG_LEN: usize = 16;

#[derive(Debug, PartialEq, Eq)]
pub enum CryptoError {
    /// O blob não tem o tamanho mínimo, ou a versão é desconhecida.
    Malformed,
    /// A tag não confere: chave errada, AAD errado, ou blob alterado.
    Unauthentic,
}

impl std::fmt::Display for CryptoError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CryptoError::Malformed => f.write_str("segredo cifrado malformado"),
            CryptoError::Unauthentic => f.write_str(
                "o segredo não decifra com a SECRETS_KEY actual (chave trocada ou dado alterado)",
            ),
        }
    }
}

/// A chave de cifra de segredos. `Clone` barato (partilhada por `Arc`) e com
/// `Debug` redigido: é material de chave (R43).
#[derive(Clone)]
pub struct SecretsKey(Arc<Aes256Gcm>);

impl std::fmt::Debug for SecretsKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SecretsKey([segredo redigido])")
    }
}

impl SecretsKey {
    /// Lê a chave de `SECRETS_KEY`: 32 bytes em base64 (padrão, com ou sem
    /// padding) ou 64 caracteres hex. Qualquer outra coisa é recusada com a
    /// razão — uma chave curta aceite em silêncio seria pior do que nenhuma.
    pub fn parse(raw: &str) -> Result<Self, String> {
        let raw = raw.trim();
        let bytes = if raw.len() == 64 && raw.chars().all(|c| c.is_ascii_hexdigit()) {
            hex::decode(raw).map_err(|e| format!("SECRETS_KEY hex inválida: {e}"))?
        } else {
            base64::engine::general_purpose::STANDARD
                .decode(raw)
                .or_else(|_| base64::engine::general_purpose::STANDARD_NO_PAD.decode(raw))
                .map_err(|_| {
                    "SECRETS_KEY tem de ser 32 bytes em base64 ou 64 caracteres hex \
                     (gera com `openssl rand -base64 32`)"
                        .to_string()
                })?
        };
        if bytes.len() != 32 {
            return Err(format!(
                "SECRETS_KEY tem {} bytes; tem de ter exactamente 32",
                bytes.len()
            ));
        }
        Self::from_bytes(&bytes)
    }

    fn from_bytes(bytes: &[u8]) -> Result<Self, String> {
        Aes256Gcm::new_from_slice(bytes)
            .map(|k| Self(Arc::new(k)))
            .map_err(|_| "SECRETS_KEY com tamanho inválido".to_string())
    }

    /// Chave FIXA de desenvolvimento (só com `DELONIX_ALLOW_INSECURE=1`).
    /// Derivada de uma frase pública: não protege nada, e é esse o ponto —
    /// existe para o fluxo funcionar em dev sem configuração.
    pub fn insecure_dev() -> Self {
        let d = Sha256::digest(b"delonix-meet dev-only SECRETS_KEY - never in production");
        Self::from_bytes(&d).expect("32 bytes")
    }

    /// Cifra `plaintext` ligado a `aad`.
    pub fn seal(&self, plaintext: &[u8], aad: &[u8]) -> Vec<u8> {
        let mut nonce = [0u8; NONCE_LEN];
        rand::thread_rng().fill_bytes(&mut nonce);
        let n = aes_gcm::Nonce::try_from(&nonce[..]).expect("nonce de 12 bytes");
        let ct = self
            .0
            .encrypt(&n, aead_payload(plaintext, aad))
            .expect("AES-GCM não falha a cifrar com nonce de 12 bytes");
        let mut out = Vec::with_capacity(1 + NONCE_LEN + ct.len());
        out.push(VERSION);
        out.extend_from_slice(&nonce);
        out.extend_from_slice(&ct);
        out
    }

    /// Decifra um blob produzido por `seal` com o MESMO `aad`.
    pub fn open(&self, blob: &[u8], aad: &[u8]) -> Result<Vec<u8>, CryptoError> {
        if blob.len() < 1 + NONCE_LEN + TAG_LEN || blob[0] != VERSION {
            return Err(CryptoError::Malformed);
        }
        let n = aes_gcm::Nonce::try_from(&blob[1..1 + NONCE_LEN])
            .map_err(|_| CryptoError::Malformed)?;
        self.0
            .decrypt(&n, aead_payload(&blob[1 + NONCE_LEN..], aad))
            .map_err(|_| CryptoError::Unauthentic)
    }
}

fn aead_payload<'a>(msg: &'a [u8], aad: &'a [u8]) -> aes_gcm::aead::Payload<'a, 'a> {
    aes_gcm::aead::Payload { msg, aad }
}

/// SHA-256 em hex minúsculo. É o que se guarda de um token de alta entropia —
/// o token em claro só existe na resposta que o cria. (ADR-0004 §5, regra 4:
/// um só sítio para o sha256 de tokens e para a aleatoriedade de credenciais.)
pub(crate) fn sha256_hex(s: &str) -> String {
    hex::encode(Sha256::digest(s.as_bytes()))
}

/// Token com prefixo legível e 256 bits de entropia do SO (`dlx_…`, `dlxg_…`).
pub(crate) fn random_token(prefix: &str) -> String {
    format!("{prefix}{}", random_hex(32))
}

/// `n` bytes de aleatoriedade do SO, em hex minúsculo. A primitiva por trás
/// de `random_token` para quem precisa de outro comprimento — ex.: as
/// credenciais SIP em `ramais.rs`, que não seguem o formato
/// `<prefixo><64 hex>`. Mesma regra 4 do ADR-0004 §5: um só sítio a chamar
/// `OsRng`/`fill_bytes`, para a catraca de arquitectura verificar.
pub(crate) fn random_hex(n_bytes: usize) -> String {
    let mut bytes = vec![0u8; n_bytes];
    OsRng.fill_bytes(&mut bytes);
    hex::encode(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(b: u8) -> SecretsKey {
        SecretsKey::from_bytes(&[b; 32]).unwrap()
    }

    #[test]
    fn round_trip() {
        let k = key(7);
        let blob = k.seal(b"chave-do-youtube", b"stream_destination/a/b");
        assert_eq!(
            k.open(&blob, b"stream_destination/a/b").unwrap(),
            b"chave-do-youtube"
        );
    }

    #[test]
    fn the_plaintext_is_not_in_the_blob_and_nonces_differ() {
        let k = key(7);
        let a = k.seal(b"chave-do-youtube", b"x");
        let b = k.seal(b"chave-do-youtube", b"x");
        assert_ne!(a, b, "o nonce tem de ser aleatório");
        assert!(!a.windows(5).any(|w| w == b"chave"));
    }

    #[test]
    fn a_blob_moved_to_another_row_does_not_decrypt() {
        // O AAD liga o segredo ao sítio onde vive: copiar a coluna de uma org
        // para a linha de outra não entrega a chave.
        let k = key(7);
        let blob = k.seal(b"segredo", b"stream_destination/org-a/1");
        assert_eq!(
            k.open(&blob, b"stream_destination/org-b/1"),
            Err(CryptoError::Unauthentic)
        );
    }

    #[test]
    fn wrong_key_or_tampering_is_refused() {
        let blob = key(7).seal(b"segredo", b"x");
        assert_eq!(key(8).open(&blob, b"x"), Err(CryptoError::Unauthentic));
        let mut mexido = blob.clone();
        let ultimo = mexido.len() - 1;
        mexido[ultimo] ^= 1;
        assert_eq!(key(7).open(&mexido, b"x"), Err(CryptoError::Unauthentic));
        assert_eq!(key(7).open(&blob[..10], b"x"), Err(CryptoError::Malformed));
        let mut versao = blob;
        versao[0] = 9;
        assert_eq!(key(7).open(&versao, b"x"), Err(CryptoError::Malformed));
    }

    #[test]
    fn parse_accepts_base64_and_hex_and_refuses_short_keys() {
        let b64 = base64::engine::general_purpose::STANDARD.encode([3u8; 32]);
        assert!(SecretsKey::parse(&b64).is_ok());
        assert!(SecretsKey::parse(&"ab".repeat(32)).is_ok());
        let curta = base64::engine::general_purpose::STANDARD.encode([3u8; 16]);
        let e = SecretsKey::parse(&curta).unwrap_err();
        assert!(e.contains("32"), "{e}");
        assert!(SecretsKey::parse("não é base64 !!").is_err());
    }

    #[test]
    fn debug_never_prints_key_material() {
        assert_eq!(format!("{:?}", key(1)), "SecretsKey([segredo redigido])");
    }

    #[test]
    fn sha256_hex_matches_known_vector() {
        assert_eq!(
            sha256_hex("abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn random_token_has_prefix_and_256_bits() {
        let a = random_token("dlxg_");
        let b = random_token("dlxg_");
        assert!(a.starts_with("dlxg_"));
        assert_eq!(a.len(), 5 + 64);
        assert_ne!(a, b);
    }
}
