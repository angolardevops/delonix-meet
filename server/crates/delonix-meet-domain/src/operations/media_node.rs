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

/// Ocupação, em % da capacidade declarada, a partir da qual o nó entra na zona
/// em que só aceita salas novas de quem está abaixo da sua parte justa.
/// Os 15% acima desta linha são a margem para as salas existentes crescerem.
///
/// É uma ESCOLHA, não uma medida: o teste de carga de 2026-09-17 mediu o
/// colapso (perda de 13–48%) entre ~120 e ~200 pessoas por nó, sem marcar onde
/// começa a degradação. O operador declara a capacidade (`NODE_PEER_CAPACITY`) a
/// partir dos seus testes; esta fracção decide a margem.
pub const NEW_ROOM_LOAD_PERCENT: i64 = 85;

/// O nó aceita salas novas de QUALQUER inquilino? É a pergunta do inventário:
/// abaixo do limite mole sim; acima, só os que estão abaixo da sua parte (ver
/// [`admit_new_room`]). Sem capacidade declarada, sim: não se inventa um limite
/// que o operador não deu (a mesma regra de `load_ratio`).
pub fn accepts_new_rooms(peers: i64, capacity: Option<i64>) -> bool {
    match capacity {
        Some(c) if c > 0 => peers.max(0) * 100 < c * NEW_ROOM_LOAD_PERCENT,
        _ => true,
    }
}

/// O que o nó sabe da ocupação no momento de decidir uma sala NOVA.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NodeOccupancy {
    /// Participantes de todos os inquilinos neste nó.
    pub node_peers: i64,
    /// Participantes do inquilino que pede (a organização do dono da sala, ou o
    /// próprio dono se for um utilizador individual).
    pub tenant_peers: i64,
    /// Inquilinos com participantes neste nó, contando o que pede.
    pub active_tenants: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NewRoomAdmission {
    Admit,
    /// O nó está na capacidade: não aceita salas novas de ninguém.
    NodeFull,
    /// O nó está na zona de margem e este inquilino já usa a sua parte justa.
    OverFairShare,
}

/// Decide uma sala NOVA (as que já estão no nó entram sempre).
///
/// - **Abaixo do limite mole** (85%): aceita.
/// - **Zona de margem** (85% até à capacidade): recusa só o inquilino que já tem
///   a sua parte justa, `capacidade ÷ inquilinos activos`. Um inquilino sozinho
///   no nó tem a capacidade toda e não é penalizado; com dois, cada um tem
///   metade, e quem tem menos do que a metade continua a poder abrir salas.
/// - **Na capacidade**: recusa toda a gente.
///
/// Uma conta ou organização não consegue, por si, fechar o nó às salas novas
/// dos outros ENQUANTO está na zona de margem. Não impede que as salas que já
/// tem cresçam até à capacidade — isso pede um tecto por inquilino (quota).
pub fn admit_new_room(o: NodeOccupancy, capacity: Option<i64>) -> NewRoomAdmission {
    let Some(c) = capacity.filter(|c| *c > 0) else {
        return NewRoomAdmission::Admit;
    };
    let node = o.node_peers.max(0);
    if node >= c {
        return NewRoomAdmission::NodeFull;
    }
    if node * 100 < c * NEW_ROOM_LOAD_PERCENT {
        return NewRoomAdmission::Admit;
    }
    // «parte justa» sem divisões: tenant × activos ≥ capacidade.
    if o.tenant_peers.max(0) * o.active_tenants.max(1) >= c {
        NewRoomAdmission::OverFairShare
    } else {
        NewRoomAdmission::Admit
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

    fn occ(node: i64, tenant: i64, active: i64) -> NodeOccupancy {
        NodeOccupancy {
            node_peers: node,
            tenant_peers: tenant,
            active_tenants: active,
        }
    }

    #[test]
    fn below_the_soft_limit_everyone_is_admitted() {
        // Capacidade 100, limite mole 85.
        assert_eq!(admit_new_room(occ(84, 80, 2), Some(100)), NewRoomAdmission::Admit);
        assert_eq!(admit_new_room(occ(0, 0, 1), Some(100)), NewRoomAdmission::Admit);
    }

    #[test]
    fn in_the_margin_only_the_tenant_over_its_share_is_refused() {
        // 90 de 100: A tem 80, B tem 10 → dois inquilinos, parte justa 50.
        assert_eq!(
            admit_new_room(occ(90, 80, 2), Some(100)),
            NewRoomAdmission::OverFairShare,
            "A tem 80 ≥ 50"
        );
        assert_eq!(
            admit_new_room(occ(90, 10, 2), Some(100)),
            NewRoomAdmission::Admit,
            "B tem 10 < 50: continua a poder abrir salas"
        );
        // Um inquilino novo, ainda sem ninguém, entra.
        assert_eq!(admit_new_room(occ(90, 0, 3), Some(100)), NewRoomAdmission::Admit);
    }

    #[test]
    fn a_tenant_alone_on_the_node_is_not_penalised_before_the_capacity() {
        assert_eq!(admit_new_room(occ(95, 95, 1), Some(100)), NewRoomAdmission::Admit);
        assert_eq!(admit_new_room(occ(100, 100, 1), Some(100)), NewRoomAdmission::NodeFull);
    }

    #[test]
    fn at_the_capacity_nobody_gets_a_new_room() {
        assert_eq!(admit_new_room(occ(100, 0, 5), Some(100)), NewRoomAdmission::NodeFull);
        assert_eq!(admit_new_room(occ(500, 0, 5), Some(100)), NewRoomAdmission::NodeFull);
    }

    #[test]
    fn the_fair_share_boundary_is_exact() {
        // Capacidade 100, 2 inquilinos: parte justa 50. 49 passa, 50 não.
        assert_eq!(admit_new_room(occ(90, 49, 2), Some(100)), NewRoomAdmission::Admit);
        assert_eq!(admit_new_room(occ(90, 50, 2), Some(100)), NewRoomAdmission::OverFairShare);
        // Capacidade ímpar (7), 2 inquilinos: 7/2 = 3,5 → 3 passa (6 < 7), 4 não (8 ≥ 7).
        assert_eq!(admit_new_room(occ(6, 3, 2), Some(7)), NewRoomAdmission::Admit);
        assert_eq!(admit_new_room(occ(6, 4, 2), Some(7)), NewRoomAdmission::OverFairShare);
    }

    #[test]
    fn without_a_declared_capacity_nothing_is_refused_by_admit_new_room() {
        assert_eq!(admit_new_room(occ(10_000, 9_000, 3), None), NewRoomAdmission::Admit);
        assert_eq!(admit_new_room(occ(10_000, 9_000, 3), Some(0)), NewRoomAdmission::Admit);
    }
}
