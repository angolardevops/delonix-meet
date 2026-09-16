//! Regras da fila de transcrição (ADR-0005 §3). Puras: o adaptador Postgres
//! aplica-as numa só instrução com `FOR UPDATE SKIP LOCKED`.

use std::time::Duration;

/// Uma gravação que falha este número de vezes sai da fila: um ficheiro que
/// rebenta o modelo não pode ocupar a GPU para sempre.
pub const MAX_ATTEMPTS: i32 = 5;

const MIN_LEASE: u64 = 60;
const MAX_LEASE: u64 = 2 * 3600;
const DEFAULT_LEASE: u64 = 30 * 60;

/// Prazo efectivo da reserva: o pedido do worker, preso a 1 min..2 h; zero ou
/// negativo dá 30 min.
pub fn lease_duration(requested_secs: i32) -> Duration {
    let secs = if requested_secs <= 0 {
        DEFAULT_LEASE
    } else {
        (requested_secs as u64).clamp(MIN_LEASE, MAX_LEASE)
    };
    Duration::from_secs(secs)
}

/// Depois de uma falha: a gravação volta à fila?
pub fn should_retry(retryable: bool, attempts_so_far: i32) -> bool {
    retryable && attempts_so_far < MAX_ATTEMPTS
}

/// Razão de falha guardada: curta e numa linha (vai para um painel, não para
/// um log).
pub fn sanitize_reason(reason: &str) -> String {
    reason
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(300)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lease_is_bounded() {
        assert_eq!(lease_duration(0), Duration::from_secs(1800));
        assert_eq!(lease_duration(5), Duration::from_secs(60));
        assert_eq!(lease_duration(999_999), Duration::from_secs(7200));
        assert_eq!(lease_duration(600), Duration::from_secs(600));
    }

    #[test]
    fn retry_stops_after_max_or_when_not_retryable() {
        assert!(should_retry(true, 1));
        assert!(!should_retry(true, MAX_ATTEMPTS));
        assert!(!should_retry(false, 1));
    }

    #[test]
    fn reason_is_one_short_line() {
        let r = sanitize_reason("CUDA\n  out of memory\t".repeat(50).as_str());
        assert!(!r.contains('\n'));
        assert!(r.chars().count() <= 300);
    }
}
