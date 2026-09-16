//! Erro de domínio com código estável.
//!
//! O `code` (`meeting.host_not_found`) é **contrato**: o cliente decide por ele,
//! e muda só com versão nova da API. A `message` é para pessoas e pode mudar.
//! O `kind` diz a CLASSE do erro e é o que os adaptadores traduzem — para HTTP
//! (`delonix-meet-api`) e para gRPC — sem que o domínio conheça nenhum dos dois.

use serde::Serialize;

/// Classes de erro, alinhadas com os códigos canónicos do gRPC (e por isso
/// traduzíveis sem perda para HTTP).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorKind {
    /// Forma inválida do pedido (400).
    InvalidArgument,
    /// Regra de negócio violada com um pedido bem formado (422).
    FailedPrecondition,
    /// Sem identidade válida (401).
    Unauthenticated,
    /// Identidade válida sem o papel exigido (403).
    PermissionDenied,
    /// Não existe — ou existe noutro inquilino, que se esconde como tal (404).
    NotFound,
    /// Conflito de estado ou unicidade (409).
    Conflict,
    /// Quota ou limite de ritmo (429).
    ResourceExhausted,
    /// Este nó não serve agora, outro pode (503).
    Unavailable,
    /// Avaria interna; o detalhe nunca sai para o cliente (500).
    Internal,
}

#[derive(Debug, Clone, Serialize)]
pub struct FieldViolation {
    pub field: String,
    pub description: String,
}

#[derive(Debug, Clone, thiserror::Error)]
#[error("{code}: {message}")]
pub struct DomainError {
    pub kind: ErrorKind,
    pub code: &'static str,
    pub message: String,
    pub details: Vec<FieldViolation>,
}

impl DomainError {
    pub fn new(kind: ErrorKind, code: &'static str, message: impl Into<String>) -> Self {
        Self {
            kind,
            code,
            message: message.into(),
            details: Vec::new(),
        }
    }

    pub fn invalid(code: &'static str, message: impl Into<String>) -> Self {
        Self::new(ErrorKind::InvalidArgument, code, message)
    }
    pub fn precondition(code: &'static str, message: impl Into<String>) -> Self {
        Self::new(ErrorKind::FailedPrecondition, code, message)
    }
    pub fn not_found(code: &'static str) -> Self {
        Self::new(ErrorKind::NotFound, code, "não encontrado")
    }
    pub fn conflict(code: &'static str, message: impl Into<String>) -> Self {
        Self::new(ErrorKind::Conflict, code, message)
    }
    pub fn forbidden(code: &'static str) -> Self {
        Self::new(ErrorKind::PermissionDenied, code, "sem permissão")
    }
    pub fn unauthenticated() -> Self {
        Self::new(
            ErrorKind::Unauthenticated,
            "auth.unauthenticated",
            "não autenticado",
        )
    }
    pub fn internal(detail: impl std::fmt::Display) -> Self {
        Self::new(ErrorKind::Internal, "internal", detail.to_string())
    }

    /// Substitui a mensagem para pessoas, mantendo classe e código.
    pub fn with_message(mut self, message: impl Into<String>) -> Self {
        self.message = message.into();
        self
    }

    pub fn with_field(mut self, field: impl Into<String>, description: impl Into<String>) -> Self {
        self.details.push(FieldViolation {
            field: field.into(),
            description: description.into(),
        });
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builder_keeps_code_and_fields() {
        let e = DomainError::invalid("meeting.title_too_long", "título demasiado longo")
            .with_field("title", "máximo 140 caracteres");
        assert_eq!(e.kind, ErrorKind::InvalidArgument);
        assert_eq!(e.code, "meeting.title_too_long");
        assert_eq!(e.details[0].field, "title");
        assert_eq!(
            e.to_string(),
            "meeting.title_too_long: título demasiado longo"
        );
    }
}
