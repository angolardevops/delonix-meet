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

/// Ocupação, em % da capacidade declarada, a partir da qual o nó deixa de
/// aceitar salas NOVAS. Os 15% que sobram são para as salas que já cá estão e
/// continuam a crescer: uma sala existente não pode mudar de nó (ADR-0001), por
/// isso não se lhe recusa gente, e recusar as novas cedo é o que lhe dá margem.
///
/// É uma ESCOLHA, não uma medida: o teste de carga de 2026-09-17 mediu o
/// colapso (perda de 13–48%) entre ~120 e ~200 pessoas por nó, sem marcar onde
/// começa a degradação. O operador declara a capacidade (`NODE_PEER_CAPACITY`) a
/// partir dos seus testes; esta fracção decide a margem.
pub const NEW_ROOM_LOAD_PERCENT: i64 = 85;

/// Um nó aceita uma sala NOVA? Sem capacidade declarada, sim: não se inventa um
/// limite que o operador não deu (a mesma regra de `load_ratio`).
///
/// Salas que já existem no nó não passam por aqui — entram sempre.
pub fn accepts_new_rooms(peers: i64, capacity: Option<i64>) -> bool {
    match capacity {
        Some(c) if c > 0 => peers.max(0) * 100 < c * NEW_ROOM_LOAD_PERCENT,
        _ => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_node_stops_taking_new_rooms_at_the_load_limit() {
        // Capacidade 10, limite 85%: aceita até 8 participantes, recusa a partir de 9.
        assert!(accepts_new_rooms(0, Some(10)));
        assert!(accepts_new_rooms(8, Some(10)));
        assert!(!accepts_new_rooms(9, Some(10)));
        assert!(!accepts_new_rooms(10, Some(10)));
        assert!(!accepts_new_rooms(500, Some(10)), "acima da capacidade, recusa");
        // Capacidade grande: 85% de 200 = 170.
        assert!(accepts_new_rooms(169, Some(200)));
        assert!(!accepts_new_rooms(170, Some(200)));
    }

    #[test]
    fn without_a_declared_capacity_nothing_is_refused() {
        assert!(accepts_new_rooms(10_000, None));
        assert!(accepts_new_rooms(10_000, Some(0)));
        assert!(accepts_new_rooms(-5, Some(10)), "contagem negativa não recusa");
    }

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
