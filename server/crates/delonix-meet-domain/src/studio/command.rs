//! Comandos do estúdio para o telefone e estado do telefone para o estúdio
//! (ADR-0014 §2.2).
//!
//! O telefone EXECUTA, o servidor VALIDA: um intervalo absurdo não chega a
//! sair da sala. Os dois tipos recusam campos desconhecidos — um campo que o
//! cliente escreve e o sistema ignora é pior do que um campo que não existe.

use serde::{Deserialize, Serialize};

/// Um comando para a app Delonix Câmara.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum SourceCommand {
    FocusFace,
    FocusDistance { meters: f64 },
    LockExposureFocus { locked: bool },
    Exposure { ev: f64 },
    WhiteBalance { kelvin: u32 },
    Iso { value: u32 },
    Zoom { factor: f64 },
    Mirror { on: bool },
    LocalRecording { on: bool },
}

fn range(name: &str, v: f64, lo: f64, hi: f64) -> Result<(), String> {
    if !v.is_finite() || v < lo || v > hi {
        return Err(format!("{name} fora de {lo}–{hi}"));
    }
    Ok(())
}

impl SourceCommand {
    /// `Err` traz a razão legível; o código estável é `studio.invalid_command`.
    pub fn validate(&self) -> Result<(), String> {
        match self {
            Self::FocusFace
            | Self::LockExposureFocus { .. }
            | Self::Mirror { .. }
            | Self::LocalRecording { .. } => Ok(()),
            Self::FocusDistance { meters } => range("meters", *meters, 0.1, 100.0),
            Self::Exposure { ev } => range("ev", *ev, -3.0, 3.0),
            Self::WhiteBalance { kelvin } => range("kelvin", f64::from(*kelvin), 2000.0, 10000.0),
            Self::Iso { value } => range("value", f64::from(*value), 50.0, 12800.0),
            Self::Zoom { factor } => range("factor", *factor, 0.5, 10.0),
        }
    }
}

/// Identificador do comando escolhido pelo operador, para casar o resultado.
pub fn validate_command_id(id: &str) -> Result<(), String> {
    if id.is_empty()
        || id.len() > 64
        || !id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    {
        return Err("command_id: 1–64 caracteres [A-Za-z0-9_-]".into());
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ThermalState {
    Nominal,
    Fair,
    Serious,
    Critical,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NetworkKind {
    Wifi,
    Cellular,
    Usb,
    Ethernet,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NetworkStatus {
    pub kind: NetworkKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub link_mbps: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rtt_ms: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VideoFormat {
    pub width: u32,
    pub height: u32,
    pub fps: f64,
}

/// O estado que o telefone reporta. Tudo opcional: um telefone que não sabe a
/// temperatura não a inventa.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceStatus {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub battery_percent: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub charging: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub temperature_c: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thermal_state: Option<ThermalState>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub network: Option<NetworkStatus>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub video: Option<VideoFormat>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub local_recording: Option<bool>,
}

impl SourceStatus {
    pub fn validate(&self) -> Result<(), String> {
        if let Some(b) = self.battery_percent {
            range("battery_percent", b, 0.0, 100.0)?;
        }
        if let Some(t) = self.temperature_c {
            range("temperature_c", t, -20.0, 90.0)?;
        }
        if let Some(n) = &self.network {
            if let Some(l) = n.link_mbps {
                range("network.link_mbps", l, 0.0, 100_000.0)?;
            }
            if let Some(r) = n.rtt_ms {
                range("network.rtt_ms", r, 0.0, 60_000.0)?;
            }
        }
        if let Some(v) = &self.video {
            range("video.width", f64::from(v.width), 16.0, 7680.0)?;
            range("video.height", f64::from(v.height), 16.0, 4320.0)?;
            range("video.fps", v.fps, 1.0, 240.0)?;
        }
        Ok(())
    }
}

/// Resultado de um comando, devolvido pelo telefone.
pub fn validate_result_error(e: Option<&str>) -> Result<(), String> {
    match e {
        Some(s) if s.chars().count() > 200 => Err("error: máximo 200 caracteres".into()),
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn comandos_leem_se_como_o_contrato() {
        let c: SourceCommand = serde_json::from_value(json!({"kind":"focus-face"})).unwrap();
        assert_eq!(c, SourceCommand::FocusFace);
        let c: SourceCommand =
            serde_json::from_value(json!({"kind":"lock-exposure-focus","locked":true})).unwrap();
        assert_eq!(c, SourceCommand::LockExposureFocus { locked: true });
        let c: SourceCommand =
            serde_json::from_value(json!({"kind":"white-balance","kelvin":5200})).unwrap();
        assert!(c.validate().is_ok());
        assert_eq!(
            serde_json::to_value(SourceCommand::Exposure { ev: -0.5 }).unwrap(),
            json!({"kind":"exposure","ev":-0.5})
        );
    }

    #[test]
    fn campos_e_tipos_desconhecidos_sao_recusados() {
        assert!(serde_json::from_value::<SourceCommand>(json!({"kind":"reboot"})).is_err());
        assert!(serde_json::from_value::<SourceCommand>(
            json!({"kind":"mirror","on":true,"extra":1})
        )
        .is_err());
        assert!(serde_json::from_value::<SourceStatus>(json!({"battery":50})).is_err());
    }

    #[test]
    fn intervalos_validados_no_servidor() {
        assert!(SourceCommand::Exposure { ev: 3.1 }.validate().is_err());
        assert!(SourceCommand::Exposure { ev: f64::NAN }.validate().is_err());
        assert!(SourceCommand::WhiteBalance { kelvin: 1999 }
            .validate()
            .is_err());
        assert!(SourceCommand::Iso { value: 12800 }.validate().is_ok());
        assert!(SourceCommand::Zoom { factor: 0.4 }.validate().is_err());
        assert!(SourceCommand::FocusDistance { meters: 2.4 }
            .validate()
            .is_ok());
    }

    #[test]
    fn estado_do_telefone() {
        let s: SourceStatus = serde_json::from_value(json!({
            "battery_percent": 76, "charging": true, "temperature_c": 41,
            "thermal_state": "fair", "network": {"kind":"usb","link_mbps":5000},
            "video": {"width":1920,"height":1080,"fps":30}, "local_recording": true
        }))
        .unwrap();
        assert!(s.validate().is_ok());
        let quente = SourceStatus {
            temperature_c: Some(120.0),
            ..Default::default()
        };
        assert!(quente.validate().is_err());
        assert!(SourceStatus {
            battery_percent: Some(-1.0),
            ..Default::default()
        }
        .validate()
        .is_err());
    }

    #[test]
    fn command_id_e_erro() {
        assert!(validate_command_id("cmd-1_a").is_ok());
        assert!(validate_command_id("").is_err());
        assert!(validate_command_id("a b").is_err());
        assert!(validate_command_id(&"a".repeat(65)).is_err());
        assert!(validate_result_error(Some(&"x".repeat(201))).is_err());
        assert!(validate_result_error(None).is_ok());
    }
}
