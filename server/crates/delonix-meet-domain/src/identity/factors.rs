//! Segundos factores de uma conta: TOTP e chaves de acesso (WebAuthn).
//!
//! A regra que aqui vive é a do ÚLTIMO factor: numa organização que exige 2FA,
//! a pessoa não pode ficar sem nenhum. Remover o TOTP com uma chave de acesso
//! registada é permitido (e vice-versa); remover o que resta é
//! `409 security.last_factor_required`.
//!
//! O que é «a organização exige 2FA»: qualquer organização ACTIVA da pessoa com
//! `require_mfa`. É a leitura mais restritiva — uma pessoa em duas
//! organizações cumpre a regra da mais exigente.

use delonix_meet_core::DomainError;

pub const LAST_FACTOR_REQUIRED: &str = "security.last_factor_required";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Factor {
    Totp,
    Passkey,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Factors {
    pub totp_enabled: bool,
    pub passkeys: i64,
}

impl Factors {
    pub fn count(&self) -> i64 {
        i64::from(self.totp_enabled) + self.passkeys.max(0)
    }

    /// Há segundo factor? É o que decide se o login pede o desafio de MFA.
    pub fn any(&self) -> bool {
        self.count() > 0
    }
}

pub fn check_removal(
    org_requires_mfa: bool,
    factors: Factors,
    removing: Factor,
) -> Result<(), DomainError> {
    let removes = match removing {
        Factor::Totp => i64::from(factors.totp_enabled),
        Factor::Passkey => i64::from(factors.passkeys > 0),
    };
    if org_requires_mfa && removes > 0 && factors.count() - removes < 1 {
        return Err(DomainError::conflict(
            LAST_FACTOR_REQUIRED,
            "a sua organização exige autenticação de dois factores: registe outro factor antes de remover este",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_last_factor_stays_when_the_org_requires_mfa() {
        let only_totp = Factors {
            totp_enabled: true,
            passkeys: 0,
        };
        let only_key = Factors {
            totp_enabled: false,
            passkeys: 1,
        };
        let both = Factors {
            totp_enabled: true,
            passkeys: 1,
        };
        let two_keys = Factors {
            totp_enabled: false,
            passkeys: 2,
        };

        assert_eq!(
            check_removal(true, only_totp, Factor::Totp)
                .unwrap_err()
                .code,
            LAST_FACTOR_REQUIRED
        );
        assert!(check_removal(true, only_key, Factor::Passkey).is_err());
        assert!(check_removal(true, both, Factor::Totp).is_ok());
        assert!(check_removal(true, both, Factor::Passkey).is_ok());
        assert!(check_removal(true, two_keys, Factor::Passkey).is_ok());
        // sem exigência da org, tudo se pode remover
        assert!(check_removal(false, only_totp, Factor::Totp).is_ok());
        assert!(check_removal(false, only_key, Factor::Passkey).is_ok());
        // remover o que não existe não é «o último»
        assert!(check_removal(true, only_key, Factor::Totp).is_ok());
    }

    #[test]
    fn counting() {
        assert!(!Factors {
            totp_enabled: false,
            passkeys: 0
        }
        .any());
        assert_eq!(
            Factors {
                totp_enabled: true,
                passkeys: 3
            }
            .count(),
            4
        );
    }
}
