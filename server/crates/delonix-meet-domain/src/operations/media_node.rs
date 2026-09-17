//! Estado de um nó de media (G10), derivado do último batimento.
//!
//! Cada pod escreve um batimento periódico; o estado não se guarda, deriva-se
//! na leitura — um nó que morreu não consegue escrever «morri».

use chrono::{DateTime, Duration, Utc};
use serde::Serialize;

/// Intervalo entre batimentos. O limiar de «sem sinal» é 4× isto: tolera uma
/// pausa longa de GC do SO ou um batimento perdido sem alarmar o operador.
pub const HEARTBEAT_SECS: i64 = 15;
pub const STALE_AFTER_SECS: i64 = HEARTBEAT_SECS * 4;
/// Registos de nós sem sinal há mais do que isto são apagados.
pub const FORGET_AFTER_HOURS: i64 = 24;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeStatus {
    /// A aceitar salas novas.
    Serving,
    /// Recebeu SIGTERM: não aceita salas novas, as actuais migram (ADR-0001).
    Draining,
    /// Sem batimento há mais de `STALE_AFTER_SECS`.
    Unreachable,
}

pub fn status(last_seen: DateTime<Utc>, draining: bool, now: DateTime<Utc>) -> NodeStatus {
    if now - last_seen > Duration::seconds(STALE_AFTER_SECS) {
        NodeStatus::Unreachable
    } else if draining {
        NodeStatus::Draining
    } else {
        NodeStatus::Serving
    }
}

/// Ocupação em [0, 1] face a uma capacidade declarada de participantes. Sem
/// capacidade declarada não se inventa um número: `None`.
pub fn load_ratio(peers: i64, capacity: Option<i64>) -> Option<f64> {
    match capacity {
        Some(c) if c > 0 => Some((peers.max(0) as f64 / c as f64).min(1.0)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_is_derived_from_age_and_drain() {
        let now = Utc::now();
        assert_eq!(status(now, false, now), NodeStatus::Serving);
        assert_eq!(status(now, true, now), NodeStatus::Draining);
        let old = now - Duration::seconds(STALE_AFTER_SECS + 1);
        assert_eq!(
            status(old, true, now),
            NodeStatus::Unreachable,
            "morto ganha a drenar"
        );
    }

    #[test]
    fn load_needs_a_declared_capacity() {
        assert_eq!(load_ratio(50, Some(200)), Some(0.25));
        assert_eq!(load_ratio(500, Some(200)), Some(1.0));
        assert_eq!(load_ratio(10, None), None);
        assert_eq!(load_ratio(10, Some(0)), None);
    }
}
