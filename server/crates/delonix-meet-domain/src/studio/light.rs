//! Iluminação: o agente local, os aparelhos que ele reporta e os comandos que
//! reclama (ADR-0014 §3). Mesmo padrão do agente USB do SMS (ADR-0005).

use delonix_meet_core::DomainError;
use serde::{Deserialize, Serialize};

use super::document::{validate_fixture_levels, FixtureLevel};

/// Prefixo do token do agente de estúdio (`dlxg_` é o do SMS).
pub const AGENT_TOKEN_PREFIX: &str = "dlxs_";
/// Visto há menos disto → `online`.
pub const ONLINE_WINDOW_SECS: i64 = 30;
/// Reclamado e não confirmado neste tempo → `expired`, e não volta à fila.
pub const CLAIM_TIMEOUT_SECS: i64 = 60;
pub const MAX_CLAIM: i64 = 10;
pub const POLL_INTERVAL_MS: u64 = 500;
pub const MAX_FIXTURES: usize = 256;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Protocol {
    Artnet,
    Hue,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Capability {
    Level,
    Cct,
    Rgb,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FixtureState {
    Ok,
    Unreachable,
    Warning,
}

impl FixtureState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::Unreachable => "unreachable",
            Self::Warning => "warning",
        }
    }
}

/// Um aparelho tal como o agente o reporta.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FixtureReport {
    pub fixture_key: String,
    pub name: String,
    pub protocol: Protocol,
    /// Endereço legível (`{"universe":1,"start_channel":1}`, `{"bridge":"…","light":"3"}`).
    pub address: serde_json::Value,
    pub capabilities: Vec<Capability>,
    #[serde(default)]
    pub level: Option<f64>,
    #[serde(default)]
    pub cct_k: Option<u32>,
    pub state: FixtureState,
    #[serde(default)]
    pub warning: Option<String>,
}

/// `dmx:<universo>:<canal>` ou `hue:<bridge>:<luz>`: estável entre arranques
/// do agente, porque é o endereço e não um contador.
pub fn validate_fixture_key(k: &str) -> Result<(), DomainError> {
    let parts: Vec<&str> = k.split(':').collect();
    let ok = k.len() <= 64
        && parts.len() == 3
        && matches!(parts[0], "dmx" | "hue")
        && parts[1..].iter().all(|p| {
            !p.is_empty()
                && p.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        });
    if !ok {
        return Err(DomainError::invalid(
            "studio.invalid_fixture_key",
            "chave de aparelho: dmx:<universo>:<canal> ou hue:<bridge>:<luz>",
        )
        .with_field("fixture_key", "dmx:u:c | hue:b:l"));
    }
    Ok(())
}

pub fn validate_reports(v: &[FixtureReport]) -> Result<(), DomainError> {
    if v.len() > MAX_FIXTURES {
        return Err(DomainError::invalid(
            "studio.too_many_fixtures",
            format!("no máximo {MAX_FIXTURES} aparelhos por agente"),
        ));
    }
    let mut seen: Vec<&str> = Vec::new();
    for f in v {
        validate_fixture_key(&f.fixture_key)?;
        if seen.contains(&f.fixture_key.as_str()) {
            return Err(DomainError::invalid(
                "studio.invalid_fixture_key",
                format!("aparelho repetido: {}", f.fixture_key),
            ));
        }
        seen.push(&f.fixture_key);
        let proto_ok = match f.protocol {
            Protocol::Artnet => f.fixture_key.starts_with("dmx:"),
            Protocol::Hue => f.fixture_key.starts_with("hue:"),
        };
        if !proto_ok {
            return Err(DomainError::invalid(
                "studio.invalid_fixture_key",
                "o prefixo da chave tem de corresponder ao protocolo",
            ));
        }
        if f.name.chars().count() > 80 || f.address.to_string().len() > 512 {
            return Err(DomainError::invalid(
                "studio.invalid_fixture",
                "nome até 80 caracteres e endereço até 512 bytes",
            ));
        }
    }
    Ok(())
}

/// O pedido do operador.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case", deny_unknown_fields)]
pub enum LightRequest {
    SetLevels {
        fixtures: Vec<FixtureLevel>,
        #[serde(default)]
        transition_ms: u32,
    },
    ApplyScene {
        document_id: uuid::Uuid,
    },
    Blackout {
        #[serde(default)]
        transition_ms: u32,
    },
}

/// O que o agente recebe: só `set-levels` e `blackout` — uma cena é resolvida no
/// servidor, para o agente não precisar de conhecer documentos.
pub fn validate_request(r: &LightRequest) -> Result<(), DomainError> {
    let t = match r {
        LightRequest::SetLevels {
            fixtures,
            transition_ms,
        } => {
            validate_fixture_levels("fixtures", fixtures)?;
            *transition_ms
        }
        LightRequest::Blackout { transition_ms } => *transition_ms,
        LightRequest::ApplyScene { .. } => 0,
    };
    if t > 60_000 {
        return Err(DomainError::invalid(
            "studio.invalid_light_command",
            "transition_ms até 60000",
        )
        .with_field("transition_ms", "0–60000"));
    }
    Ok(())
}

/// Estados de um comando de luz.
pub mod status {
    pub const QUEUED: &str = "queued";
    pub const CLAIMED: &str = "claimed";
    pub const DONE: &str = "done";
    pub const FAILED: &str = "failed";
    pub const EXPIRED: &str = "expired";
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn chaves_de_aparelho() {
        assert!(validate_fixture_key("dmx:1:13").is_ok());
        assert!(validate_fixture_key("hue:bridge-1:3").is_ok());
        for bad in ["dmx:1", "dali:1:1", "dmx::1", "hue:a b:1", "dmx:1:1:1"] {
            assert!(validate_fixture_key(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn relatorio_do_agente() {
        let r: FixtureReport = serde_json::from_value(json!({
            "fixture_key": "dmx:1:1", "name": "Chave · painel LED 1", "protocol": "artnet",
            "address": {"universe": 1, "start_channel": 1, "profile": "dimmer-cct"},
            "capabilities": ["level", "cct"], "level": 86, "cct_k": 5200, "state": "ok"
        }))
        .unwrap();
        assert!(validate_reports(std::slice::from_ref(&r)).is_ok());
        let mut hue_com_dmx = r.clone();
        hue_com_dmx.protocol = Protocol::Hue;
        assert!(validate_reports(&[hue_com_dmx]).is_err());
        assert!(validate_reports(&[r.clone(), r]).is_err());
    }

    #[test]
    fn pedidos_de_luz() {
        let r: LightRequest = serde_json::from_value(json!({
            "type": "set-levels", "transition_ms": 2000,
            "fixtures": [{"fixture_key": "dmx:1:1", "level": 100}]
        }))
        .unwrap();
        assert!(validate_request(&r).is_ok());
        let r: LightRequest =
            serde_json::from_value(json!({"type": "blackout", "transition_ms": 70000})).unwrap();
        assert!(validate_request(&r).is_err());
        assert!(serde_json::from_value::<LightRequest>(json!({"type": "strobe"})).is_err());
        assert_eq!(
            serde_json::to_value(LightRequest::Blackout { transition_ms: 0 }).unwrap(),
            json!({"type": "blackout", "transition_ms": 0})
        );
    }
}
