//! Lugares da organização (ADR-0008 §7) — o mesmo padrão da `storage_quota`:
//! a regra é pura e vive aqui; o uso é MEDIDO no adaptador (membros humanos
//! activos que ocupam lugar), nunca guardado num contador.
//!
//! O tecto é `organizations.max_seats`; `None` = sem tecto. O utilizador de
//! serviço e o `external_guest` não ocupam lugar.

use delonix_meet_core::DomainError;

/// Lugares ainda livres. `None` = sem tecto; nunca negativo.
pub fn remaining(used: i64, limit: Option<i64>) -> Option<i64> {
    limit.map(|max| max.saturating_sub(used).max(0))
}

/// Activar `incoming` pessoas que ocupam lugar cabe no tecto?
///
/// Recusa com `seats.limit_reached` (422): o pedido está bem formado e
/// repeti-lo não o corrige — libertar lugares ou subir o tecto, sim. Só se
/// impõe a activações NOVAS: uma org acima do tecto não perde ninguém.
pub fn check_activation(used: i64, incoming: i64, limit: Option<i64>) -> Result<(), DomainError> {
    let Some(max) = limit else {
        return Ok(());
    };
    if incoming <= 0 {
        return Ok(());
    }
    if used.saturating_add(incoming) > max {
        return Err(DomainError::precondition(
            "seats.limit_reached",
            format!("a organização não tem lugares livres ({used} de {max} ocupados)"),
        ));
    }
    Ok(())
}

/// Valida um tecto escrito pelo operador.
pub fn validate_limit(limit: Option<i64>) -> Result<Option<i64>, DomainError> {
    match limit {
        Some(n) if !(1..=1_000_000).contains(&n) => Err(DomainError::invalid(
            "seats.invalid_limit",
            "o tecto de lugares é 1–1000000 ou nulo (sem tecto)",
        )
        .with_field("max_seats", "1–1000000 ou null")),
        other => Ok(other),
    }
}

/// Dias de inactividade aceites para «libertar lugares».
pub fn validate_inactive_days(days: i64) -> Result<i64, DomainError> {
    if !(7..=3650).contains(&days) {
        return Err(
            DomainError::invalid("seats.invalid_inactive_days", "inactive_days é 7–3650")
                .with_field("inactive_days", "7–3650"),
        );
    }
    Ok(days)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_limit_is_unlimited() {
        assert!(check_activation(1_000_000, 1, None).is_ok());
        assert_eq!(remaining(5, None), None);
    }

    #[test]
    fn fills_exactly_and_refuses_past_the_limit() {
        assert!(check_activation(149, 1, Some(150)).is_ok());
        let e = check_activation(150, 1, Some(150)).unwrap_err();
        assert_eq!(e.code, "seats.limit_reached");
        assert_eq!(e.kind, delonix_meet_core::ErrorKind::FailedPrecondition);
        assert_eq!(remaining(142, Some(150)), Some(8));
        assert_eq!(
            remaining(160, Some(150)),
            Some(0),
            "acima do tecto resta zero"
        );
    }

    #[test]
    fn over_the_limit_keeps_people_but_blocks_new_ones() {
        assert!(check_activation(160, 0, Some(150)).is_ok());
        assert!(check_activation(160, 1, Some(150)).is_err());
    }

    #[test]
    fn validations() {
        assert!(validate_limit(Some(0)).is_err());
        assert_eq!(validate_limit(None).unwrap(), None);
        assert!(validate_inactive_days(3).is_err());
        assert_eq!(validate_inactive_days(60).unwrap(), 60);
    }
}
