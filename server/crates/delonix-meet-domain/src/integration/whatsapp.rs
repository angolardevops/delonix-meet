//! Porta do WhatsApp Business (ADR-0010 §4).
//!
//! O convite pelo WhatsApp é uma mensagem de TEMPLATE aprovada pela Meta (uma
//! empresa não pode abrir conversa com texto livre) com um parâmetro: o link da
//! sala. O adaptador real fala com a WhatsApp Business Cloud API; nos testes, um
//! servidor HTTP falso. Sem configuração da organização, a API diz
//! `channels.whatsapp_not_configured` — nunca «enviado».

use async_trait::async_trait;

use crate::telephony::ports::PortError;

/// Conta WhatsApp Business de UMA organização, já com o token aberto.
#[derive(Clone, PartialEq, Eq)]
pub struct WhatsAppAccount {
    pub phone_number_id: String,
    pub access_token: String,
    pub invite_template: String,
    pub template_language: String,
}

impl std::fmt::Debug for WhatsAppAccount {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // O token nunca entra num log.
        f.debug_struct("WhatsAppAccount")
            .field("phone_number_id", &self.phone_number_id)
            .field("access_token", &"<redigido>")
            .field("invite_template", &self.invite_template)
            .finish()
    }
}

#[async_trait]
pub trait WhatsAppProvider: Send + Sync {
    /// Envia o convite (template com o link). Devolve o id da mensagem do
    /// fornecedor (`wamid.…`).
    async fn send_invite(
        &self,
        account: &WhatsAppAccount,
        to_e164: &str,
        join_link: &str,
    ) -> Result<String, PortError>;
}

/// Validação do que o admin escreve na configuração.
pub fn validate_account_fields(
    phone_number_id: &str,
    invite_template: &str,
    template_language: &str,
) -> Result<(), delonix_meet_core::DomainError> {
    use delonix_meet_core::DomainError;
    if phone_number_id.is_empty()
        || phone_number_id.len() > 32
        || !phone_number_id.bytes().all(|b| b.is_ascii_digit())
    {
        return Err(DomainError::invalid(
            "whatsapp.invalid_phone_number_id",
            "o «phone number ID» da Meta são só dígitos",
        )
        .with_field("phone_number_id", "dígitos"));
    }
    let name_ok = !invite_template.is_empty()
        && invite_template.len() <= 512
        && invite_template
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_');
    if !name_ok {
        return Err(DomainError::invalid(
            "whatsapp.invalid_template",
            "nome de template da Meta: minúsculas, dígitos e _",
        )
        .with_field("invite_template", "[a-z0-9_]+"));
    }
    let lang_ok = (2..=8).contains(&template_language.len())
        && template_language
            .bytes()
            .all(|b| b.is_ascii_alphabetic() || b == b'_');
    if !lang_ok {
        return Err(DomainError::invalid(
            "whatsapp.invalid_language",
            "código de idioma do template, p.ex. pt_PT",
        )
        .with_field("template_language", "pt_PT"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_nunca_no_debug() {
        let a = WhatsAppAccount {
            phone_number_id: "1".into(),
            access_token: "EAAG-segredo".into(),
            invite_template: "convite".into(),
            template_language: "pt_PT".into(),
        };
        assert!(!format!("{a:?}").contains("EAAG"));
    }

    #[test]
    fn campos() {
        assert!(validate_account_fields("106540352242922", "convite_reuniao", "pt_PT").is_ok());
        assert_eq!(
            validate_account_fields("abc", "x", "pt_PT")
                .unwrap_err()
                .code,
            "whatsapp.invalid_phone_number_id"
        );
        assert_eq!(
            validate_account_fields("1", "Convite Reunião", "pt_PT")
                .unwrap_err()
                .code,
            "whatsapp.invalid_template"
        );
        assert_eq!(
            validate_account_fields("1", "c", "pt-PT;x")
                .unwrap_err()
                .code,
            "whatsapp.invalid_language"
        );
    }
}
