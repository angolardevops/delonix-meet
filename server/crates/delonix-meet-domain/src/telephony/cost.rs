//! Custo e saúde medidos — nunca inventados.
//!
//! - O custo de uma chamada usa o preço EM VIGOR no instante em que ela
//!   começou ([`price_at`]); um preço novo não reescreve o passado.
//! - O ASR (answer-seizure ratio) é `atendidas / tentativas` numa janela. Sem
//!   tentativas não há ASR: `None`, e quem mostra diz porquê.
//! - O estado de um tronco deriva do registo SIP (medido pelo SBC/FreeSWITCH) e
//!   do ASR; sem nenhum dos dois, é `unknown`.

use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

use super::money::{Currency, Money};

/// Unidade de taxação: começo de minuto (a prática das operadoras angolanas
/// nos contratos que conhecemos; muda-se aqui, com o ADR-0009 actualizado).
pub const BILLING_INCREMENT_SECS: i64 = 60;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PricePoint {
    pub id: Uuid,
    pub valid_from: DateTime<Utc>,
    pub price_per_min: Money,
}

/// O preço em vigor em `at`: o de `valid_from` mais recente que não seja
/// posterior a `at`. `points` em qualquer ordem.
pub fn price_at(points: &[PricePoint], at: DateTime<Utc>) -> Option<&PricePoint> {
    points
        .iter()
        .filter(|p| p.valid_from <= at)
        .max_by_key(|p| (p.valid_from, p.id))
}

/// Custo de `billsec` segundos falados a `price_per_min`, por começo de
/// minuto. Uma chamada não atendida (`billsec == 0`) não custa.
pub fn call_cost(billsec: i64, price_per_min: Money) -> Money {
    if billsec <= 0 {
        return Money::zero(price_per_min.currency);
    }
    let units = (billsec + BILLING_INCREMENT_SECS - 1) / BILLING_INCREMENT_SECS;
    Money::new(units * price_per_min.amount_e4, price_per_min.currency)
}

/// Minutos taxados de uma chamada.
pub fn billed_minutes(billsec: i64) -> i64 {
    if billsec <= 0 {
        0
    } else {
        (billsec + BILLING_INCREMENT_SECS - 1) / BILLING_INCREMENT_SECS
    }
}

/// ASR em `[0, 1]`, ou `None` sem tentativas.
pub fn asr(attempts: i64, answered: i64) -> Option<f64> {
    if attempts <= 0 {
        None
    } else {
        Some((answered.clamp(0, attempts) as f64) / attempts as f64)
    }
}

/// Estado de registo de um tronco, tal como o SBC/FreeSWITCH o reporta.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RegistrationState {
    Registered,
    /// A tentar registar (ou à espera de nova tentativa).
    Trying,
    Failed,
    /// O tronco não regista (autenticação por IP) — não é falha.
    NotRequired,
    /// O media server não conhece este tronco (não foi carregado).
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TrunkState {
    Up,
    Degraded,
    Down,
    /// Sem medida nenhuma — nem do SBC nem de CDRs.
    Unknown,
}

/// Abaixo disto, com amostra suficiente, o tronco está degradado.
pub const DEGRADED_ASR: f64 = 0.90;
/// Tentativas mínimas na janela para o ASR contar para o estado.
pub const MIN_ATTEMPTS_FOR_STATE: i64 = 20;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TrunkHealth {
    pub state: TrunkState,
    /// Razões legíveis por máquina, por ordem de gravidade.
    pub reasons: Vec<&'static str>,
}

/// Deriva o estado. `registration`: `None` quando não há SBC configurado ou
/// ele não respondeu. `gateway_up`: o `ping` SIP OPTIONS do media server.
pub fn trunk_health(
    enabled: bool,
    registration: Option<RegistrationState>,
    gateway_up: Option<bool>,
    attempts: i64,
    answered: i64,
) -> TrunkHealth {
    let mut reasons = Vec::new();
    if !enabled {
        return TrunkHealth {
            state: TrunkState::Down,
            reasons: vec!["disabled"],
        };
    }
    let reg_down = matches!(registration, Some(RegistrationState::Failed))
        || matches!(registration, Some(RegistrationState::Unknown))
        || gateway_up == Some(false);
    if reg_down {
        match registration {
            Some(RegistrationState::Failed) => reasons.push("registration_failed"),
            Some(RegistrationState::Unknown) => reasons.push("not_loaded_on_media_server"),
            _ => {}
        }
        if gateway_up == Some(false) {
            reasons.push("options_ping_failed");
        }
        return TrunkHealth {
            state: TrunkState::Down,
            reasons,
        };
    }
    let ratio = asr(attempts, answered);
    let low_asr = attempts >= MIN_ATTEMPTS_FOR_STATE && ratio.is_some_and(|r| r < DEGRADED_ASR);
    if low_asr {
        reasons.push("low_asr");
    }
    if registration == Some(RegistrationState::Trying) {
        reasons.push("registering");
    }
    let state = match (registration, low_asr) {
        (None, false) if attempts == 0 => {
            reasons.push("no_measurement");
            TrunkState::Unknown
        }
        (None, false) => {
            reasons.push("sip_status_unavailable");
            TrunkState::Unknown
        }
        (_, true) => TrunkState::Degraded,
        (Some(RegistrationState::Trying), _) => TrunkState::Degraded,
        _ => TrunkState::Up,
    };
    TrunkHealth { state, reasons }
}

/// Soma por moeda (o consumo do mês tem Kz e USD).
pub fn sum_by_currency(items: impl IntoIterator<Item = Money>) -> Vec<Money> {
    let mut out: Vec<Money> = Vec::new();
    for m in items {
        match out.iter_mut().find(|x| x.currency == m.currency) {
            Some(x) => x.amount_e4 += m.amount_e4,
            None => out.push(m),
        }
    }
    out.sort_by_key(|m| match m.currency {
        Currency::Aoa => 0,
        Currency::Usd => 1,
    });
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn kz(e4: i64) -> Money {
        Money::new(e4, Currency::Aoa)
    }

    #[test]
    fn cost_uses_price_in_force_when_call_happened() {
        let t0 = Utc.with_ymd_and_hms(2026, 9, 1, 0, 0, 0).unwrap();
        let t1 = Utc.with_ymd_and_hms(2026, 9, 15, 0, 0, 0).unwrap();
        let old = PricePoint {
            id: Uuid::new_v4(),
            valid_from: t0,
            price_per_min: kz(94_000),
        };
        let new = PricePoint {
            id: Uuid::new_v4(),
            valid_from: t1,
            price_per_min: kz(99_000),
        };
        let points = [new.clone(), old.clone()];
        let before = Utc.with_ymd_and_hms(2026, 8, 31, 23, 59, 59).unwrap();
        assert!(
            price_at(&points, before).is_none(),
            "antes do 1.º preço: sem preço"
        );
        let call_a = Utc.with_ymd_and_hms(2026, 9, 14, 23, 59, 0).unwrap();
        let call_b = Utc.with_ymd_and_hms(2026, 9, 15, 0, 0, 0).unwrap();
        assert_eq!(price_at(&points, call_a).unwrap().id, old.id);
        assert_eq!(price_at(&points, call_b).unwrap().id, new.id);
        // 2 min 5 s → 3 minutos taxados.
        let a = call_cost(125, price_at(&points, call_a).unwrap().price_per_min);
        let b = call_cost(125, price_at(&points, call_b).unwrap().price_per_min);
        assert_eq!(a, kz(282_000));
        assert_eq!(b, kz(297_000));
    }

    #[test]
    fn unanswered_calls_cost_nothing() {
        assert_eq!(call_cost(0, kz(94_000)), kz(0));
        assert_eq!(call_cost(1, kz(94_000)), kz(94_000));
        assert_eq!(call_cost(60, kz(94_000)), kz(94_000));
        assert_eq!(call_cost(61, kz(94_000)), kz(188_000));
        assert_eq!(billed_minutes(0), 0);
        assert_eq!(billed_minutes(61), 2);
    }

    #[test]
    fn asr_is_none_without_attempts() {
        assert_eq!(asr(0, 0), None);
        assert_eq!(asr(1000, 982), Some(0.982));
        assert_eq!(asr(10, 20), Some(1.0));
    }

    #[test]
    fn health_is_derived() {
        use RegistrationState::*;
        assert_eq!(
            trunk_health(true, None, None, 0, 0).state,
            TrunkState::Unknown
        );
        assert_eq!(
            trunk_health(true, Some(Registered), Some(true), 0, 0).state,
            TrunkState::Up
        );
        assert_eq!(
            trunk_health(true, Some(Failed), None, 100, 99).state,
            TrunkState::Down
        );
        let h = trunk_health(true, Some(NotRequired), Some(true), 100, 89);
        assert_eq!(h.state, TrunkState::Degraded);
        assert_eq!(h.reasons, vec!["low_asr"]);
        // Amostra pequena não degrada.
        assert_eq!(
            trunk_health(true, Some(Registered), Some(true), 5, 1).state,
            TrunkState::Up
        );
        // Sem SBC, mas com CDRs maus: degradado mesmo assim.
        assert_eq!(
            trunk_health(true, None, None, 50, 10).state,
            TrunkState::Degraded
        );
        assert_eq!(
            trunk_health(false, Some(Registered), Some(true), 0, 0).state,
            TrunkState::Down
        );
    }

    #[test]
    fn sums_per_currency() {
        let s = sum_by_currency([kz(10), Money::new(5, Currency::Usd), kz(20)]);
        assert_eq!(s, vec![kz(30), Money::new(5, Currency::Usd)]);
    }
}
