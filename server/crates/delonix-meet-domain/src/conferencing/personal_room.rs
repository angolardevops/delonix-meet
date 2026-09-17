//! «A minha sala» (G2): a sala permanente de cada pessoa, com um código
//! estável (o equivalente ao PMI do Zoom).
//!
//! A sala pessoal NÃO tem regras de acesso próprias: é uma sala como as outras
//! e entra-se nela por `rooms::room_access`. O que é dela está aqui:
//!
//! - há UMA por dono (a unicidade é da base — índice parcial — porque só a base
//!   a garante entre pedidos simultâneos);
//! - nasce com a sala de espera LIGADA: é um link permanente, que circula e
//!   fica em calendários antigos, e quem o tiver não deve entrar sem ser visto;
//! - o nome segue a forma de uma sala (1–100 caracteres depois de `trim`);
//! - rodar o código invalida o link antigo — é a única forma de o revogar.

use delonix_meet_core::DomainError;

pub const MAX_NAME: usize = 100;

/// A sala de espera de uma sala pessoal nova.
pub const DEFAULT_WAITING_ROOM: bool = true;

/// Topologia de uma sala pessoal nova (a mesma omissão de `POST /api/rooms`).
pub const DEFAULT_TOPOLOGY: &str = "sfu";

/// Nome de uma sala pessoal nova.
pub fn default_name(username: &str) -> String {
    let who = username.trim();
    let name = if who.is_empty() {
        "A minha sala".to_string()
    } else {
        format!("Sala de {who}")
    };
    name.chars().take(MAX_NAME).collect()
}

pub fn validate_name(name: &str) -> Result<String, DomainError> {
    let n = name.trim();
    if n.is_empty() || n.chars().count() > MAX_NAME {
        return Err(DomainError::invalid(
            "personal_room.invalid_name",
            format!("o nome da sala tem de ter 1-{MAX_NAME} caracteres"),
        )
        .with_field("name", format!("1-{MAX_NAME} caracteres")));
    }
    Ok(n.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_name_uses_the_username_and_fits() {
        assert_eq!(default_name(" ana "), "Sala de ana");
        assert_eq!(default_name(""), "A minha sala");
        assert_eq!(default_name(&"x".repeat(300)).chars().count(), MAX_NAME);
        assert!(validate_name(&default_name(&"é".repeat(300))).is_ok());
    }

    #[test]
    fn name_shape() {
        assert_eq!(validate_name("  Reuniões  ").unwrap(), "Reuniões");
        assert_eq!(
            validate_name("   ").unwrap_err().code,
            "personal_room.invalid_name"
        );
        assert!(validate_name(&"é".repeat(100)).is_ok(), "conta caracteres");
        assert!(validate_name(&"x".repeat(101)).is_err());
    }
}
