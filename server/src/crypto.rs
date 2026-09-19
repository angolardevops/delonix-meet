//! Primitivas criptográficas do monólito — delegam no dono único,
//! `delonix_meet_core::crypto` (ADR-0004 §5 regra 4; ADR-0006 §1).
//!
//! Existe para os módulos que chamam `crate::crypto::…` continuarem a
//! compilar sem uma segunda implementação. A cifra de segredos em repouso
//! (destinos de directo guardados: `stream_destinations.rs`, S5) usa
//! `delonix_meet_core::secret_box::SecretBox` directamente — não este módulo.

/// SHA-256 em hex minúsculo. É o que se guarda de um token de alta entropia —
/// o token em claro só existe na resposta que o cria.
pub(crate) fn sha256_hex(s: &str) -> String {
    delonix_meet_core::crypto::sha256_hex(s)
}

/// Token com prefixo legível e 256 bits de entropia do SO (`dlx_…`, `dlxg_…`).
pub(crate) fn random_token(prefix: &str) -> String {
    delonix_meet_core::crypto::prefixed_token(prefix)
}

/// `n` bytes de aleatoriedade do SO, em hex minúsculo. A primitiva por trás
/// de `random_token` para quem precisa de outro comprimento — ex.: as
/// credenciais SIP em `ramais.rs`, que não seguem o formato
/// `<prefixo><64 hex>`. Mesma regra 4 do ADR-0004 §5: um só sítio a chamar
/// `OsRng`/`fill_bytes`, para a catraca de arquitectura verificar.
pub(crate) fn random_hex(n_bytes: usize) -> String {
    delonix_meet_core::crypto::random_hex(n_bytes)
}

/// `n` bytes crus de aleatoriedade do SO — a primitiva de que `random_hex`
/// (e `random_token`) já são um formato. Existe em separado para quem
/// precisa dos bytes em si, não de texto: as chaves SRTP efémeras da ponte
/// PSTN↔SFU (`pstn_bridge::SrtpKeyPair`), que nunca passam por hex. Um
/// comprimento em runtime (não `const N`) — `delonix_meet_core::crypto`
/// só tem a versão de tamanho fixo, que não serve aqui. MESMA regra 4 do
/// ADR-0004 §5 — um só sítio a chamar `OsRng`/`fill_bytes`.
pub(crate) fn random_bytes(n_bytes: usize) -> Vec<u8> {
    use rand::{rngs::OsRng, RngCore};
    let mut bytes = vec![0u8; n_bytes];
    OsRng.fill_bytes(&mut bytes);
    bytes
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

    #[test]
    fn random_hex_and_bytes_have_the_right_length_and_vary() {
        let a = random_hex(15);
        assert_eq!(a.len(), 30);
        assert_ne!(a, random_hex(15));
        assert_eq!(random_bytes(16).len(), 16);
        assert_ne!(random_bytes(16), random_bytes(16));
    }
}
