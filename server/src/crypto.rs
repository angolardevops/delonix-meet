//! Primitivas criptográficas do monólito — delegam no dono único,
//! `delonix_meet_core::crypto` (ADR-0004 §5 regra 4; ADR-0006 §1).
//!
//! Existe para os módulos que chamam `crate::crypto::…` (o gateway de SMS)
//! continuarem a compilar sem uma segunda implementação.

/// SHA-256 em hex minúsculo. É o que se guarda de um token de alta entropia —
/// o token em claro só existe na resposta que o cria.
pub(crate) fn sha256_hex(s: &str) -> String {
    delonix_meet_core::crypto::sha256_hex(s)
}

/// Token com prefixo legível e 256 bits de entropia do SO (`dlx_…`, `dlxg_…`).
pub(crate) fn random_token(prefix: &str) -> String {
    delonix_meet_core::crypto::prefixed_token(prefix)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delegates_to_core() {
        assert_eq!(
            sha256_hex("abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        let a = random_token("dlxg_");
        assert!(a.starts_with("dlxg_"));
        assert_eq!(a.len(), 5 + 64);
        assert_ne!(a, random_token("dlxg_"));
    }
}
