//! «Os meus dados» — a exportação pessoal.
//!
//! Um pedido é um trabalho assíncrono (`queued` → `running` → `ready` |
//! `failed`, e `ready` → `expired`). O resultado é UM ficheiro ZIP com:
//! o perfil e as preferências, as gravações que a pessoa carregou (como
//! LINKS, não como bytes — são as maiores coisas da conta e já têm rota
//! própria com as regras de acesso de sempre), as transcrições dessas
//! gravações, o registo de actividade em que a pessoa é o ACTOR, e o uso de
//! armazenamento (G3).
//!
//! O que nunca entra: dados em que a pessoa não é dona nem actora — gravações
//! de outros partilhadas com ela, a trilha de outros membros, contactos.
//!
//! Regras sem IO:
//! - limite de pedidos ([`check_rate`]): um trabalho activo de cada vez e no
//!   máximo [`MAX_PER_DAY`] por 24 h — uma exportação lê a conta inteira;
//! - validade do ficheiro ([`FILE_TTL_HOURS`]) e do link temporário
//!   ([`LINK_TTL_SECS`]), assinado por HMAC como os quadros (G11).

use chrono::{DateTime, Duration, Utc};
use delonix_meet_core::{crypto, DomainError, ErrorKind};
use uuid::Uuid;

pub const MAX_PER_DAY: i64 = 3;
/// Depois disto o ficheiro é apagado e o pedido fica `expired`.
pub const FILE_TTL_HOURS: i64 = 48;
/// Validade de um link de descarga. Pede-se outro enquanto o ficheiro existir.
pub const LINK_TTL_SECS: i64 = 15 * 60;
/// Um trabalho `running` mais velho do que isto foi abandonado por um processo
/// que morreu: volta a `queued`.
pub const LEASE_SECS: i64 = 15 * 60;
/// Tecto de linhas de actividade exportadas (as mais recentes).
pub const MAX_ACTIVITY_ROWS: i64 = 50_000;

pub const KEY_PURPOSE: &str = "delonix.data_export.link.v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Queued,
    Running,
    Ready,
    Failed,
    Expired,
}

impl Status {
    pub fn as_str(self) -> &'static str {
        match self {
            Status::Queued => "queued",
            Status::Running => "running",
            Status::Ready => "ready",
            Status::Failed => "failed",
            Status::Expired => "expired",
        }
    }

    pub fn parse(s: &str) -> Option<Status> {
        [
            Status::Queued,
            Status::Running,
            Status::Ready,
            Status::Failed,
            Status::Expired,
        ]
        .into_iter()
        .find(|x| x.as_str() == s)
    }

    pub fn is_active(self) -> bool {
        matches!(self, Status::Queued | Status::Running)
    }
}

/// `active`: pedidos `queued`/`running` da pessoa; `last_24h`: pedidos criados
/// nas últimas 24 h (qualquer estado). Devolve os segundos a esperar, no erro.
pub fn check_rate(
    active: i64,
    last_24h: i64,
    oldest_in_window: Option<DateTime<Utc>>,
    now: DateTime<Utc>,
) -> Result<(), DomainError> {
    if active > 0 {
        return Err(DomainError::conflict(
            "data_export.already_running",
            "já há uma exportação em curso: espere que termine",
        ));
    }
    if last_24h >= MAX_PER_DAY {
        let retry = oldest_in_window
            .map(|t| (t + Duration::hours(24) - now).num_seconds().max(1))
            .unwrap_or(3600);
        return Err(DomainError::new(
            ErrorKind::ResourceExhausted,
            "data_export.rate_limited",
            format!(
                "no máximo {MAX_PER_DAY} exportações por 24 h; tente daqui a {} min",
                (retry.max(1) as u64).div_ceil(60)
            ),
        ));
    }
    Ok(())
}

fn message(id: Uuid, exp: i64) -> String {
    format!("{KEY_PURPOSE}:{id}:{exp}")
}

pub fn signed_path(key: &[u8], id: Uuid, exp: i64) -> String {
    format!(
        "/api/users/me/data-exports/{id}/content?exp={exp}&sig={}",
        hex::encode(crypto::hmac_sha256(key, message(id, exp)))
    )
}

pub fn verify(key: &[u8], id: Uuid, exp: &str, sig: &str, now: i64) -> bool {
    let Ok(exp) = exp.parse::<i64>() else {
        return false;
    };
    if exp <= now || exp - now > LINK_TTL_SECS {
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

    #[test]
    fn one_active_and_three_per_day() {
        let now = Utc::now();
        assert!(check_rate(0, 0, None, now).is_ok());
        assert!(check_rate(0, 2, Some(now), now).is_ok());
        assert_eq!(
            check_rate(1, 0, None, now).unwrap_err().code,
            "data_export.already_running"
        );
        let e = check_rate(0, 3, Some(now - Duration::hours(23)), now).unwrap_err();
        assert_eq!(e.code, "data_export.rate_limited");
        assert_eq!(e.kind, ErrorKind::ResourceExhausted);
        assert!(e.message.contains("60 min"), "{}", e.message);
    }

    #[test]
    fn signed_link_is_bound_to_id_and_time() {
        let key = [7u8; 32];
        let id = Uuid::new_v4();
        let now = 1_000_000;
        let exp = now + 60;
        let path = signed_path(&key, id, exp);
        let sig = path.rsplit("sig=").next().unwrap();
        assert!(verify(&key, id, &exp.to_string(), sig, now));
        assert!(
            !verify(&key, Uuid::new_v4(), &exp.to_string(), sig, now),
            "outro id"
        );
        assert!(
            !verify(&[8u8; 32], id, &exp.to_string(), sig, now),
            "outra chave"
        );
        assert!(!verify(&key, id, &exp.to_string(), sig, exp), "expirado");
        assert!(!verify(&key, id, "abc", sig, now));
        let far = now + LINK_TTL_SECS + 1;
        let far_sig = signed_path(&key, id, far);
        assert!(
            !verify(
                &key,
                id,
                &far.to_string(),
                far_sig.rsplit("sig=").next().unwrap(),
                now
            ),
            "validade acima do tecto é recusada mesmo bem assinada"
        );
    }

    #[test]
    fn statuses_roundtrip() {
        for s in ["queued", "running", "ready", "failed", "expired"] {
            assert_eq!(Status::parse(s).unwrap().as_str(), s);
        }
        assert!(Status::Queued.is_active() && !Status::Ready.is_active());
    }
}
