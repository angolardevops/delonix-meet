//! Primitivas criptográficas com um só dono.
//!
//! A auditoria de 2026-09-16 contou 4 cópias de sha256, 7 de tokens
//! aleatórios, 3 de comparação em tempo constante e 3 de argon2. Uma cópia que
//! diverge numa primitiva de segurança não se nota até ser explorada; por isso
//! a catraca da arquitectura conta as primitivas fora de `crypto.rs`.

use argon2::{
    password_hash::{
        rand_core::OsRng as ArgonRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString,
    },
    Argon2,
};
use hmac::{Hmac, Mac};
use rand::{rngs::OsRng, RngCore};
use sha2::{Digest, Sha256};

/// SHA-256 em hexadecimal minúsculo. É o que se guarda de tokens e chaves de
/// API: o segredo nunca fica em claro na base.
pub fn sha256_hex(data: impl AsRef<[u8]>) -> String {
    hex::encode(Sha256::digest(data.as_ref()))
}

/// SHA-256 em bytes (derivação de chaves de desenvolvimento, etc.).
pub fn sha256(data: impl AsRef<[u8]>) -> [u8; 32] {
    Sha256::digest(data.as_ref()).into()
}

/// HMAC-SHA256 de `data` com `key`. É a base dos URLs assinados: quem não tem
/// a chave não consegue produzir uma assinatura que [`ct_eq`] aceite.
pub fn hmac_sha256(key: impl AsRef<[u8]>, data: impl AsRef<[u8]>) -> [u8; 32] {
    let mut mac = <Hmac<Sha256> as Mac>::new_from_slice(key.as_ref())
        .expect("o HMAC aceita chaves de qualquer tamanho");
    mac.update(data.as_ref());
    mac.finalize().into_bytes().into()
}

/// Deriva uma subchave de `secret` para UM propósito (`purpose`). Uma
/// assinatura feita com a subchave de um propósito não vale noutro, e o
/// segredo de origem (p.ex. o do JWT) nunca assina nada fora do seu uso.
pub fn derive_key(secret: impl AsRef<[u8]>, purpose: &str) -> [u8; 32] {
    hmac_sha256(secret, format!("delonix-meet/derive/{purpose}"))
}

/// `N` bytes do gerador do sistema operativo.
pub fn random_bytes<const N: usize>() -> [u8; N] {
    let mut b = [0u8; N];
    OsRng.fill_bytes(&mut b);
    b
}

/// `n` bytes aleatórios do SO, em hexadecimal (`2n` caracteres).
pub fn random_hex(n: usize) -> String {
    let mut b = vec![0u8; n];
    OsRng.fill_bytes(&mut b);
    hex::encode(b)
}

/// Token opaco com prefixo legível (`dlx_…`, `dlxo_…`), 256 bits de entropia.
/// O prefixo diz a quem o encontra num log de que credencial se trata.
pub fn prefixed_token(prefix: &str) -> String {
    format!("{prefix}{}", random_hex(32))
}

/// Comparação em tempo constante (para o tamanho dado). Não abre um oráculo de
/// temporização sobre segredos partilhados.
pub fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

#[derive(Debug, thiserror::Error)]
#[error("falha a derivar o hash da password: {0}")]
pub struct HashError(String);

/// Hash argon2id (parâmetros por omissão da crate) com sal aleatório.
pub fn hash_password(password: &str) -> Result<String, HashError> {
    let salt = SaltString::generate(&mut ArgonRng);
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|h| h.to_string())
        .map_err(|e| HashError(e.to_string()))
}

/// Verifica uma password contra um hash PHC. Um hash ilegível é `false`, nunca
/// `true`: falha fechado.
pub fn verify_password(password: &str, hash: &str) -> bool {
    PasswordHash::new(hash)
        .map(|parsed| {
            Argon2::default()
                .verify_password(password.as_bytes(), &parsed)
                .is_ok()
        })
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha256_known_vector() {
        assert_eq!(
            sha256_hex("abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn hmac_sha256_rfc4231_case_2() {
        assert_eq!(
            hex::encode(hmac_sha256("Jefe", "what do ya want for nothing?")),
            "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843"
        );
    }

    #[test]
    fn derived_keys_are_per_purpose() {
        assert_ne!(derive_key("s", "a"), derive_key("s", "b"));
        assert_ne!(derive_key("s", "a"), derive_key("t", "a"));
        assert_eq!(derive_key("s", "a"), derive_key("s", "a"));
    }

    #[test]
    fn ct_eq_only_identical() {
        assert!(ct_eq(b"abc", b"abc"));
        assert!(!ct_eq(b"abc", b"abd"));
        assert!(!ct_eq(b"abc", b"ab"));
        assert!(ct_eq(b"", b""));
    }

    #[test]
    fn tokens_are_distinct_and_sized() {
        let a = prefixed_token("dlx_");
        let b = prefixed_token("dlx_");
        assert_ne!(a, b);
        assert_eq!(a.len(), 4 + 64);
        assert_eq!(random_hex(16).len(), 32);
    }

    #[test]
    fn password_roundtrip_and_fail_closed() {
        let h = hash_password("UmaPasswordForte123!").unwrap();
        assert!(verify_password("UmaPasswordForte123!", &h));
        assert!(!verify_password("outra", &h));
        assert!(!verify_password("x", "não-é-um-hash"));
    }
}
