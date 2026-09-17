//! Dinheiro sem vírgula flutuante.
//!
//! Um preço por minuto de `0,04 USD` ou `9,40 Kz` guarda-se em décimas-milésimas
//! da unidade (`amount_e4`): `0.04` → `400`, `9.40` → `94000`. Somar mil chamadas
//! em `f64` dá cêntimos a mais ou a menos; um livro de custos não pode.
//!
//! Na API, os valores saem como TEXTO decimal (`"9.4000"`) com a moeda ao lado
//! — um número JSON seria lido como `f64` pelo cliente.

use delonix_meet_core::DomainError;
use serde::Serialize;

/// Moedas aceites nos preços dos troncos. Lista fechada: acrescentar uma é uma
/// decisão (há conversão para AOA por trás), não um campo livre.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
pub enum Currency {
    /// Kwanza — a moeda de referência dos relatórios de consumo.
    #[serde(rename = "AOA")]
    Aoa,
    #[serde(rename = "USD")]
    Usd,
}

impl Currency {
    pub const ALL: [&'static str; 2] = ["AOA", "USD"];

    pub fn parse(s: &str) -> Result<Self, DomainError> {
        match s.trim().to_ascii_uppercase().as_str() {
            "AOA" | "KZ" => Ok(Self::Aoa),
            "USD" => Ok(Self::Usd),
            other => Err(DomainError::invalid(
                "telephony.invalid_currency",
                format!("moeda «{other}» não suportada — válidas: AOA, USD"),
            )
            .with_field("currency", "AOA | USD")),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Aoa => "AOA",
            Self::Usd => "USD",
        }
    }
}

pub const SCALE: i64 = 10_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Money {
    pub amount_e4: i64,
    pub currency: Currency,
}

impl Money {
    pub fn new(amount_e4: i64, currency: Currency) -> Self {
        Self {
            amount_e4,
            currency,
        }
    }

    pub fn zero(currency: Currency) -> Self {
        Self::new(0, currency)
    }

    /// `"9.4000"` — sempre quatro casas.
    pub fn amount_string(&self) -> String {
        format_e4(self.amount_e4)
    }
}

pub fn format_e4(v: i64) -> String {
    let sign = if v < 0 { "-" } else { "" };
    let a = v.unsigned_abs();
    format!("{sign}{}.{:04}", a / SCALE as u64, a % SCALE as u64)
}

/// Lê `"9.40"`, `"9,40"`, `"0.0125"` ou `"12"`. Mais de quatro casas decimais,
/// sinal negativo ou lixo → `400 telephony.invalid_amount` (nunca arredonda em
/// silêncio um preço que alguém escreveu).
pub fn parse_amount_e4(s: &str) -> Result<i64, DomainError> {
    let err = || {
        DomainError::invalid(
            "telephony.invalid_amount",
            format!("valor «{s}» inválido — usa um decimal positivo com até 4 casas"),
        )
    };
    let t = s.trim().replace(',', ".");
    if t.is_empty() || t.len() > 20 {
        return Err(err());
    }
    let (int, frac) = match t.split_once('.') {
        Some((i, f)) => (i, f),
        None => (t.as_str(), ""),
    };
    if int.is_empty()
        || !int.bytes().all(|b| b.is_ascii_digit())
        || !frac.bytes().all(|b| b.is_ascii_digit())
        || frac.len() > 4
    {
        return Err(err());
    }
    let int: i64 = int.parse().map_err(|_| err())?;
    let mut frac_v: i64 = if frac.is_empty() {
        0
    } else {
        frac.parse().map_err(|_| err())?
    };
    for _ in frac.len()..4 {
        frac_v *= 10;
    }
    int.checked_mul(SCALE)
        .and_then(|v| v.checked_add(frac_v))
        .ok_or_else(err)
}

/// Taxa de câmbio «Kz por unidade» em milionésimas (`rate_e6`). `1 USD =
/// 912,50 Kz` → `912_500_000`.
pub fn parse_rate_e6(s: &str) -> Result<i64, DomainError> {
    let err = || {
        DomainError::invalid(
            "telephony.invalid_rate",
            format!("taxa «{s}» inválida — decimal positivo com até 6 casas"),
        )
    };
    let t = s.trim().replace(',', ".");
    let (int, frac) = t.split_once('.').unwrap_or((t.as_str(), ""));
    if int.is_empty()
        || int.len() > 9
        || !int.bytes().all(|b| b.is_ascii_digit())
        || !frac.bytes().all(|b| b.is_ascii_digit())
        || frac.len() > 6
    {
        return Err(err());
    }
    let mut f: i64 = if frac.is_empty() {
        0
    } else {
        frac.parse().map_err(|_| err())?
    };
    for _ in frac.len()..6 {
        f *= 10;
    }
    let v = int.parse::<i64>().map_err(|_| err())? * 1_000_000 + f;
    if v <= 0 {
        return Err(err());
    }
    Ok(v)
}

pub fn format_e6(v: i64) -> String {
    format!("{}.{:06}", v / 1_000_000, v % 1_000_000)
}

/// Converte para AOA com uma taxa `rate_e6` (Kz por unidade). AOA passa como está.
pub fn to_aoa(m: Money, rate_e6: Option<i64>) -> Option<Money> {
    match m.currency {
        Currency::Aoa => Some(m),
        _ => rate_e6.map(|r| {
            // i128: 10^4 × 10^6 × montantes grandes não cabe em i64 com folga.
            let v = (m.amount_e4 as i128 * r as i128 + 500_000) / 1_000_000;
            Money::new(v as i64, Currency::Aoa)
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn amounts_round_trip_without_float() {
        assert_eq!(parse_amount_e4("9.40").unwrap(), 94_000);
        assert_eq!(parse_amount_e4("9,40").unwrap(), 94_000);
        assert_eq!(parse_amount_e4("0.04").unwrap(), 400);
        assert_eq!(parse_amount_e4("0.0125").unwrap(), 125);
        assert_eq!(parse_amount_e4("12").unwrap(), 120_000);
        assert_eq!(format_e4(94_000), "9.4000");
        assert_eq!(format_e4(400), "0.0400");
        for bad in ["", "-1", "1.23456", "abc", "1.2.3", ".5"] {
            assert!(parse_amount_e4(bad).is_err(), "{bad} devia falhar");
        }
    }

    #[test]
    fn currency_is_closed_list() {
        assert_eq!(Currency::parse("kz").unwrap(), Currency::Aoa);
        assert_eq!(Currency::parse("usd").unwrap(), Currency::Usd);
        assert_eq!(
            Currency::parse("EUR").unwrap_err().code,
            "telephony.invalid_currency"
        );
    }

    #[test]
    fn conversion_to_aoa() {
        let usd = Money::new(400, Currency::Usd); // 0.04 USD
        let rate = parse_rate_e6("912.5").unwrap();
        assert_eq!(to_aoa(usd, Some(rate)).unwrap().amount_e4, 365_000); // 36.50 Kz
        assert!(to_aoa(usd, None).is_none(), "sem taxa não se inventa");
        let kz = Money::new(94_000, Currency::Aoa);
        assert_eq!(to_aoa(kz, None).unwrap(), kz);
        assert!(parse_rate_e6("0").is_err());
    }
}
