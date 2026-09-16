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
use rand::{rngs::OsRng, RngCore};
use sha2::{Digest, Sha256};

/// SHA-256 em hexadecimal minúsculo. É o que se guarda de tokens e chaves de
/// API: o segredo nunca fica em claro na base.
pub fn sha256_hex(data: impl AsRef<[u8]>) -> String {
    hex::encode(Sha256::digest(data.as_ref()))
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
