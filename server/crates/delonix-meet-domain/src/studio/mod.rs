//! Contexto **studio**: o estúdio de TV num PC (ADR-0014).
//!
//! Só regras de forma e decisões puras. O corte, a mistura e a correcção de
//! imagem correm no browser (ADR-0014 §1); aqui vive o que o servidor tem de
//! garantir: o código de emparelhamento, os comandos que chegam ao telefone, o
//! estado que ele devolve, o tally, os documentos do estúdio e os comandos de
//! luz para o agente local.

pub mod command;
pub mod document;
pub mod light;
pub mod pairing;
pub mod tally;

use delonix_meet_core::DomainError;

pub const MAX_NAME: usize = 80;
pub const MAX_LABEL: usize = 80;
/// CAM 1 … CAM 16.
pub const MAX_SOURCE_NUMBER: i32 = 16;

/// Capacidades PROPOSTAS para o catálogo de papéis (frente A, ADR-0008). Hoje a
/// verificação usa `org::role_in_org`; os nomes ficam aqui para a ligação ser
/// uma troca de função e não uma renomeação.
pub mod capability {
    pub const VIEW: &str = "studio.view";
    pub const OPERATE: &str = "studio.operate";
    pub const MANAGE: &str = "studio.manage";
}

/// Nome de um estúdio: 1–80 caracteres visíveis, aparado.
pub fn validate_name(name: &str) -> Result<String, DomainError> {
    let n = name.trim();
    if n.is_empty() || n.chars().count() > MAX_NAME {
        return Err(DomainError::invalid(
            "studio.invalid_name",
            format!("o nome tem de ter entre 1 e {MAX_NAME} caracteres"),
        )
        .with_field("name", format!("1–{MAX_NAME} caracteres")));
    }
    Ok(n.to_string())
}

/// Rótulo de uma fonte («telefone da Ana»): 0–80 caracteres, aparado.
pub fn validate_label(label: &str) -> Result<String, DomainError> {
    let l = label.trim();
    if l.chars().count() > MAX_LABEL || l.chars().any(char::is_control) {
        return Err(DomainError::invalid(
            "studio.invalid_label",
            format!("o rótulo tem no máximo {MAX_LABEL} caracteres, sem controlo"),
        )
        .with_field("label", format!("0–{MAX_LABEL} caracteres")));
    }
    Ok(l.to_string())
}

/// Número da fonte (CAM n).
pub fn validate_number(n: i32) -> Result<i32, DomainError> {
    if !(1..=MAX_SOURCE_NUMBER).contains(&n) {
        return Err(DomainError::invalid(
            "studio.invalid_source_number",
            format!("o número da câmara vai de 1 a {MAX_SOURCE_NUMBER}"),
        )
        .with_field("number", format!("1–{MAX_SOURCE_NUMBER}")));
    }
    Ok(n)
}

/// O menor número livre, dado os ocupados. `None` = estúdio cheio.
pub fn first_free_number(taken: &[i32]) -> Option<i32> {
    (1..=MAX_SOURCE_NUMBER).find(|n| !taken.contains(n))
}

/// Como a fonte aparece em todo o lado: `CAM 2 · telefone da Ana`.
pub fn display_label(number: i32, label: &str) -> String {
    if label.trim().is_empty() {
        format!("CAM {number}")
    } else {
        format!("CAM {number} · {}", label.trim())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nome_vazio_ou_comprido_e_recusado() {
        assert_eq!(validate_name("  Régie  ").unwrap(), "Régie");
        assert_eq!(validate_name(" ").unwrap_err().code, "studio.invalid_name");
        assert!(validate_name(&"é".repeat(81)).is_err());
        assert!(validate_name(&"é".repeat(80)).is_ok());
    }

    #[test]
    fn numero_da_camara_tem_tecto() {
        assert!(validate_number(0).is_err());
        assert!(validate_number(17).is_err());
        assert_eq!(validate_number(16).unwrap(), 16);
        assert_eq!(first_free_number(&[1, 2, 4]), Some(3));
        assert_eq!(first_free_number(&(1..=16).collect::<Vec<_>>()), None);
    }

    #[test]
    fn rotulo_de_apresentacao() {
        assert_eq!(
            display_label(2, "telefone da Ana"),
            "CAM 2 · telefone da Ana"
        );
        assert_eq!(display_label(3, " "), "CAM 3");
        assert!(validate_label("a\u{0007}b").is_err());
    }
}
