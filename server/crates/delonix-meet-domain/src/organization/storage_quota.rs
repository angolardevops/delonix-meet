//! Quota de armazenamento da organização (G3).
//!
//! O que conta: os bytes das gravações da organização e os PNG dos seus
//! quadros. O tecto é `organizations.max_storage_bytes`; `None` = ilimitado,
//! como as outras quotas (migração 0013).
//!
//! **Uma regra, um sítio.** Quem carrega bytes novos pergunta aqui se cabem
//! ([`check_upload`]); quem mostra o uso pergunta aqui o que resta
//! ([`remaining`]). A regra só se impõe a carregamentos NOVOS: uma organização
//! que já passou o tecto (porque o tecto desceu) não perde nada — deixa de
//! poder acrescentar.

use delonix_meet_core::DomainError;

/// Bytes ocupados, por tipo de conteúdo.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Usage {
    pub recordings_bytes: i64,
    pub whiteboards_bytes: i64,
}

impl Usage {
    pub fn used_bytes(&self) -> i64 {
        self.recordings_bytes.saturating_add(self.whiteboards_bytes)
    }
}

/// Bytes que ainda cabem. `None` = sem tecto. Nunca negativo: acima do tecto
/// resta zero.
pub fn remaining(usage: Usage, limit: Option<i64>) -> Option<i64> {
    limit.map(|max| max.saturating_sub(usage.used_bytes()).max(0))
}

/// Um carregamento de `incoming` bytes cabe na quota?
///
/// Recusa com `storage.quota_exceeded` (422): o pedido está bem formado, e
/// repeti-lo não o corrige — libertar espaço ou subir o tecto, sim.
pub fn check_upload(usage: Usage, incoming: i64, limit: Option<i64>) -> Result<(), DomainError> {
    let Some(max) = limit else {
        return Ok(());
    };
    let after = usage.used_bytes().saturating_add(incoming.max(0));
    if after > max {
        return Err(DomainError::precondition(
            "storage.quota_exceeded",
            format!(
                "a organização ultrapassava a quota de armazenamento ({} de {max} bytes ocupados, o ficheiro tem {incoming})",
                usage.used_bytes()
            ),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const USED: Usage = Usage {
        recordings_bytes: 600,
        whiteboards_bytes: 100,
    };

    #[test]
    fn no_limit_is_unlimited() {
        assert!(check_upload(USED, i64::MAX, None).is_ok());
        assert_eq!(remaining(USED, None), None);
    }

    #[test]
    fn counts_recordings_and_whiteboards() {
        assert_eq!(USED.used_bytes(), 700);
        assert!(check_upload(USED, 300, Some(1000)).is_ok(), "enche à justa");
        let e = check_upload(USED, 301, Some(1000)).unwrap_err();
        assert_eq!(e.code, "storage.quota_exceeded");
        assert_eq!(e.kind, delonix_meet_core::ErrorKind::FailedPrecondition);
        assert_eq!(remaining(USED, Some(1000)), Some(300));
    }

    #[test]
    fn above_the_limit_keeps_what_it_has_and_adds_nothing() {
        assert_eq!(remaining(USED, Some(500)), Some(0));
        assert!(check_upload(USED, 1, Some(500)).is_err());
        assert!(check_upload(USED, 0, Some(500)).is_err());
        assert!(check_upload(Usage::default(), 0, Some(0)).is_ok());
    }
}
