//! Quadro branco: URL assinado de curta duração para o PNG (G11).
//!
//! Porquê: um `<img src>` não manda o cabeçalho `Authorization`, e a sessão é
//! um token em memória, não um cookie. O URL assinado é uma capability
//! estreita: UM quadro, só leitura, no máximo [`SIGNED_URL_TTL_SECS`].
//!
//! - A assinatura é HMAC-SHA256 sobre `(id, exp)` com uma subchave derivada do
//!   segredo do servidor para ESTE propósito — não serve para outro quadro,
//!   outro prazo, nem outro tipo de URL.
//! - Compara-se em tempo constante.
//! - Uma assinatura errada e um prazo expirado dão a MESMA resposta que um
//!   quadro que não existe: não se confirma nada a quem não tem acesso.
//! - Um prazo mais longe do que o TTL permite é recusado mesmo com assinatura
//!   válida: se a chave algum dia assinar um prazo longo por erro, o erro não
//!   vira um link permanente.

use delonix_meet_core::crypto;
use uuid::Uuid;

/// Validade máxima de um URL assinado (15 minutos).
pub const SIGNED_URL_TTL_SECS: i64 = 15 * 60;

/// Propósito da subchave (ver `crypto::derive_key`). Mudar isto invalida todos
/// os URLs em circulação — é o «v1».
pub const KEY_PURPOSE: &str = "whiteboard.png.signed-url.v1";

fn message(id: Uuid, exp: i64) -> String {
    format!("whiteboard-png:{id}:{exp}")
}

/// O prazo de um URL emitido agora.
pub fn expiry_from(now: i64) -> i64 {
    now + SIGNED_URL_TTL_SECS
}

/// A assinatura (hex) de `(id, exp)` com a subchave `key`.
pub fn sign(key: &[u8], id: Uuid, exp: i64) -> String {
    hex::encode(crypto::hmac_sha256(key, message(id, exp)))
}

/// O caminho relativo do PNG assinado.
pub fn signed_path(key: &[u8], id: Uuid, exp: i64) -> String {
    format!(
        "/api/whiteboards/{id}/image?exp={exp}&sig={}",
        sign(key, id, exp)
    )
}

/// O par `(exp, sig)` vale para o quadro `id` no instante `now`?
///
/// Recebe o `exp` em texto, tal como veio no URL: um valor que não é número é
/// só mais uma assinatura inválida.
pub fn verify(key: &[u8], id: Uuid, exp: &str, sig: &str, now: i64) -> bool {
    let Ok(exp) = exp.parse::<i64>() else {
        return false;
    };
    if exp <= now || exp - now > SIGNED_URL_TTL_SECS {
        return false;
    }
    let Ok(given) = hex::decode(sig) else {
        return false;
    };
    crypto::ct_eq(&given, &crypto::hmac_sha256(key, message(id, exp)))
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY: &[u8] = b"subchave-de-teste";
    const NOW: i64 = 1_800_000_000;

    fn id() -> Uuid {
        Uuid::from_u128(7)
    }

    #[test]
    fn a_fresh_signature_verifies() {
        let exp = expiry_from(NOW);
        let sig = sign(KEY, id(), exp);
        assert!(verify(KEY, id(), &exp.to_string(), &sig, NOW));
        assert!(verify(KEY, id(), &exp.to_string(), &sig, exp - 1));
        assert_eq!(
            signed_path(KEY, id(), exp),
            format!("/api/whiteboards/{}/image?exp={exp}&sig={sig}", id())
        );
    }

    #[test]
    fn expired_tampered_or_foreign_is_refused() {
        let exp = expiry_from(NOW);
        let sig = sign(KEY, id(), exp);
        assert!(!verify(KEY, id(), &exp.to_string(), &sig, exp), "expirado");
        assert!(
            !verify(KEY, Uuid::from_u128(8), &exp.to_string(), &sig, NOW),
            "outro quadro"
        );
        assert!(
            !verify(KEY, id(), &(exp - 1).to_string(), &sig, NOW),
            "outro prazo"
        );
        assert!(
            !verify(b"outra", id(), &exp.to_string(), &sig, NOW),
            "outra chave"
        );
        let mut bad = sig.clone();
        bad.replace_range(0..1, if sig.starts_with('0') { "1" } else { "0" });
        assert!(!verify(KEY, id(), &exp.to_string(), &bad, NOW));
        assert!(!verify(KEY, id(), "amanhã", &sig, NOW));
        assert!(!verify(KEY, id(), &exp.to_string(), "zz", NOW));
        assert!(!verify(KEY, id(), &exp.to_string(), "", NOW));
    }

    #[test]
    fn a_deadline_beyond_the_ttl_is_refused_even_if_signed() {
        let far = NOW + SIGNED_URL_TTL_SECS + 1;
        let sig = sign(KEY, id(), far);
        assert!(!verify(KEY, id(), &far.to_string(), &sig, NOW));
    }
}
