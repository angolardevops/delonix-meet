//! Primitivas criptográficas partilhadas (ADR-0004 §5, regra 4).
//!
//! Um só sítio para o sha256 de tokens e para a aleatoriedade de credenciais. A
//! catraca da arquitectura conta as chamadas a estas primitivas FORA deste
//! módulo; código novo chama as funções daqui em vez de as reescrever.

use rand::{rngs::OsRng, RngCore};
use sha2::{Digest, Sha256};

/// SHA-256 em hex minúsculo. É o que se guarda de um token de alta entropia —
/// o token em claro só existe na resposta que o cria.
pub(crate) fn sha256_hex(s: &str) -> String {
    hex::encode(Sha256::digest(s.as_bytes()))
}

/// Token com prefixo legível e 256 bits de entropia do SO (`dlx_…`, `dlxg_…`).
pub(crate) fn random_token(prefix: &str) -> String {
    let mut bytes = [0u8; 32];
    OsRng.fill_bytes(&mut bytes);
    format!("{prefix}{}", hex::encode(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

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
