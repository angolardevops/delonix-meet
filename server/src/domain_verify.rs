//! Prova de posse do domínio de email de uma organização (A3, revisão de
//! segurança de 2026-10-09).
//!
//! O registo não verifica o email de quem cria uma organização, e
//! `organizations_email_domain_uidx` dá o domínio a quem chegar primeiro --
//! inofensivo enquanto o domínio só identifica a org, perigoso a partir do
//! momento em que activa `enforce_sso` (bloqueia login por password de toda
//! a gente desse domínio) ou o JIT de SSO (cria contas novas a partir dele).
//! Este módulo é o que falta entre as duas coisas: um registo DNS TXT, no
//! mesmo espírito do `_acme-challenge` do Let's Encrypt -- quem controla a
//! zona DNS do domínio é quem o pode activar.
use hickory_resolver::proto::rr::{RData, RecordType};
use hickory_resolver::TokioResolver;

use crate::error::ApiError;

/// Sub-domínio onde o TXT de prova é publicado: `_delonix-challenge.<domínio>`.
/// Prefixo com `_`, como o resto da família de challenges deste estilo
/// (`_acme-challenge`, `_dmarc`) -- não colide com um registo "a sério" do
/// cliente.
const RECORD_PREFIX: &str = "_delonix-challenge";

/// Gera um token novo (32 hex, 128 bits) para a organização provar a posse.
pub fn new_token() -> String {
    delonix_meet_core::crypto::random_hex(16)
}

/// Nome do registo TXT que o admin tem de publicar.
pub fn record_name(domain: &str) -> String {
    format!("{RECORD_PREFIX}.{domain}")
}

/// Valor esperado desse registo.
pub fn expected_value(token: &str) -> String {
    format!("delonix-domain-verification={token}")
}

/// Consulta o TXT de `_delonix-challenge.<domain>` e diz se contém o valor
/// esperado. `Ok(false)` para "domínio sem o registo" (não é erro do
/// servidor, é o estado normal de quem ainda não o publicou); `Err` só para
/// uma falha genuína de resolução (sem rede, resolvedor mal configurado).
pub async fn verify(domain: &str, token: &str) -> Result<bool, ApiError> {
    let resolver = TokioResolver::builder_tokio()
        .map_err(|e| ApiError::Internal(format!("resolvedor DNS: {e}")))?
        .build()
        .map_err(|e| ApiError::Internal(format!("resolvedor DNS: {e}")))?;
    let name = record_name(domain);
    let lookup = match resolver.lookup(name.as_str(), RecordType::TXT).await {
        Ok(l) => l,
        // NXDOMAIN/NoRecordsFound são o caso normal de "ainda não publicou".
        // Qualquer outro erro de rede também cai aqui de propósito: um
        // resolvedor instável não pode ficar a dar 500 a um admin que só
        // está a tentar verificar o domínio -- `false` só atrasa, nunca
        // mente (nunca verifica o que não está lá).
        Err(_) => return Ok(false),
    };
    let expected = expected_value(token);
    let found = lookup.answers().iter().any(|record| {
        let RData::TXT(txt) = &record.data else {
            return false;
        };
        txt.txt_data
            .iter()
            .any(|chunk| chunk.as_ref() == expected.as_bytes())
    });
    Ok(found)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_is_32_hex_chars() {
        let t = new_token();
        assert_eq!(t.len(), 32);
        assert!(t.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn two_tokens_differ() {
        assert_ne!(new_token(), new_token());
    }

    #[test]
    fn record_name_has_the_challenge_prefix() {
        assert_eq!(record_name("empresa.ao"), "_delonix-challenge.empresa.ao");
    }

    #[test]
    fn expected_value_embeds_the_token() {
        assert_eq!(
            expected_value("abc123"),
            "delonix-domain-verification=abc123"
        );
    }
}
