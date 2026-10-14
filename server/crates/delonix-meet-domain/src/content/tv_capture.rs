//! Captura de TV por HDMI (RFC-0002, ADR-0031) — as regras de FORMA de uma
//! caixa de captura e das suas entradas, sem IO.
//!
//! O que vive aqui, e porque não no handler: a identidade de uma entrada. Um
//! capturador HDMI→USB é **dois** dispositivos no sistema operativo (um nó de
//! vídeo e um cartão de áudio) e nenhum dos identificadores óbvios serve de
//! chave: `/dev/videoN` muda com a ordem de ligação, `vendor:product` é igual
//! em dois dongles do mesmo modelo, e a «série» chega a ser uma constante do
//! fabricante (medido a 2026-10-14: `SN0001` na webcam da máquina de
//! desenvolvimento). O que é estável é a **porta** — e é com ela que se
//! emparelha o vídeo com o áudio do mesmo aparelho.

use delonix_meet_core::DomainError;

pub const MAX_LABEL: usize = 120;
pub const MAX_LOCATION: usize = 200;
pub const MAX_NAME: usize = 120;
pub const MAX_DEVICE_PATH: usize = 300;
pub const MAX_NODE: usize = 120;
pub const MAX_IDENT: usize = 8;
/// Tecto de entradas por relatório. Uma caixa com mais do que isto não é um
/// erro de desenho nosso — é um relatório que não se aceita sem decisão.
pub const MAX_INPUTS_PER_REPORT: usize = 32;

/// O que a entrada está a entregar, como vai no fio (`as_str`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Signal {
    /// Há imagem.
    Live,
    /// Nada ligado, ou aparelho desligado.
    NoSignal,
    /// Imagem preta COM áudio: a assinatura do HDCP. Não se resolve em
    /// software, e por isso tem nome próprio em vez de se disfarçar de avaria.
    Protected,
    /// O agente tentou e falhou (nó ocupado, formato recusado).
    Error,
}

impl Signal {
    pub const ALL: [&'static str; 4] = ["live", "no_signal", "protected", "error"];

    pub fn parse(s: &str) -> Result<Self, DomainError> {
        Ok(match s {
            "live" => Self::Live,
            "no_signal" => Self::NoSignal,
            "protected" => Self::Protected,
            "error" => Self::Error,
            other => {
                return Err(DomainError::invalid(
                    "tv.capture.invalid_signal",
                    format!(
                        "sinal inválido «{other}» — válidos: {}",
                        Self::ALL.join(", ")
                    ),
                )
                .with_field("signal", Self::ALL.join(" | ")))
            }
        })
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Live => "live",
            Self::NoSignal => "no_signal",
            Self::Protected => "protected",
            Self::Error => "error",
        }
    }

    /// Esta entrada pode alimentar um canal agora.
    pub fn usable(self) -> bool {
        matches!(self, Self::Live)
    }
}

/// Por onde se muda de canal nesta entrada.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TunerKind {
    /// Entrada fixa num canal: ninguém a sintoniza. É a omissão, e é legítima.
    None,
    /// HDMI-CEC, por um adaptador que exponha a linha — um dongle UVC não a tem.
    Cec,
    /// Emissor de infravermelhos a imitar o telecomando.
    Ir,
}

impl TunerKind {
    pub const ALL: [&'static str; 3] = ["none", "cec", "ir"];

    pub fn parse(s: &str) -> Result<Self, DomainError> {
        Ok(match s {
            "none" => Self::None,
            "cec" => Self::Cec,
            "ir" => Self::Ir,
            other => {
                return Err(DomainError::invalid(
                    "tv.capture.invalid_tuner_kind",
                    format!(
                        "tipo de sintonia inválido «{other}» — válidos: {}",
                        Self::ALL.join(", ")
                    ),
                )
                .with_field("tuner_kind", Self::ALL.join(" | ")))
            }
        })
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Cec => "cec",
            Self::Ir => "ir",
        }
    }
}

fn texto(
    raw: &str,
    max: usize,
    codigo: &'static str,
    campo: &'static str,
) -> Result<String, DomainError> {
    let v = raw.trim();
    if v.chars().count() > max || v.chars().any(char::is_control) {
        return Err(DomainError::invalid(
            codigo,
            format!("«{campo}» tem no máximo {max} caracteres, sem controlo"),
        )
        .with_field(campo, format!("até {max} caracteres")));
    }
    Ok(v.to_string())
}

/// O nome da caixa. Vazio não serve: é por ele que se escolhe na consola.
pub fn validate_name(raw: &str) -> Result<String, DomainError> {
    let v = texto(raw, MAX_NAME, "tv.capture.invalid_name", "name")?;
    if v.is_empty() {
        return Err(DomainError::invalid(
            "tv.capture.invalid_name",
            format!("o nome tem de ter entre 1 e {MAX_NAME} caracteres"),
        )
        .with_field("name", format!("1 a {MAX_NAME} caracteres")));
    }
    Ok(v)
}

pub fn validate_location(raw: &str) -> Result<String, DomainError> {
    texto(raw, MAX_LOCATION, "tv.capture.invalid_location", "location")
}

pub fn validate_label(raw: &str) -> Result<String, DomainError> {
    texto(raw, MAX_LABEL, "tv.capture.invalid_label", "label")
}

/// O caminho físico do barramento, como o udev o dá (`ID_PATH`). É a chave
/// estável de uma entrada, e por isso **não se normaliza em silêncio**: um
/// caminho vazio ou com controlo é recusado.
pub fn validate_device_path(raw: &str) -> Result<String, DomainError> {
    let v = raw.trim();
    let ok = !v.is_empty()
        && v.len() <= MAX_DEVICE_PATH
        && !v.chars().any(char::is_control)
        && !v.chars().any(char::is_whitespace);
    if !ok {
        return Err(DomainError::invalid(
            "tv.capture.invalid_device_path",
            format!(
                "o caminho físico tem 1 a {MAX_DEVICE_PATH} caracteres, sem espaços nem controlo \
                 (ex.: «pci-0000:00:14.0-usb-0:3:1.0»)"
            ),
        )
        .with_field("device_path", "caminho ID_PATH do udev"));
    }
    Ok(v.to_string())
}

pub fn validate_node(raw: &str) -> Result<String, DomainError> {
    texto(raw, MAX_NODE, "tv.capture.invalid_node", "node")
}

pub fn validate_ident(raw: &str) -> Result<String, DomainError> {
    texto(raw, MAX_IDENT, "tv.capture.invalid_ident", "vendor_id")
}

/// A PORTA de um caminho físico: o caminho sem a interface USB final.
///
/// Um capturador expõe o vídeo numa interface (`…-usb-0:3:1.0`) e o áudio
/// noutra (`…-usb-0:3:1.2`) da **mesma** porta. Cortar no último `:` dá a
/// porta, e é isso que emparelha os dois. Um caminho sem `:` é a sua própria
/// porta — não se inventa estrutura que não está lá.
pub fn port_key(device_path: &str) -> &str {
    match device_path.rsplit_once(':') {
        Some((porta, _)) => porta,
        None => device_path,
    }
}

/// O nó de áudio que pertence ao MESMO capturador que este vídeo, de entre os
/// que o agente reportou. `None` quando o par não se forma — e isso é um
/// estado a mostrar («sem áudio»), não um silêncio a adivinhar.
pub fn pair_audio<'a>(video_path: &str, audio: &'a [(String, String)]) -> Option<&'a str> {
    let porta = port_key(video_path);
    audio
        .iter()
        .find(|(path, _)| port_key(path) == porta)
        .map(|(_, node)| node.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_porta_ignora_a_interface_usb() {
        // Vídeo e áudio do mesmo dongle: interfaces diferentes, porta igual.
        assert_eq!(
            port_key("pci-0000:00:14.0-usb-0:3:1.0"),
            port_key("pci-0000:00:14.0-usb-0:3:1.2")
        );
        // Portas diferentes do mesmo controlador não se confundem.
        assert_ne!(
            port_key("pci-0000:00:14.0-usb-0:3:1.0"),
            port_key("pci-0000:00:14.0-usb-0:4:1.0")
        );
        // Sem `:` o caminho é a sua própria porta.
        assert_eq!(
            port_key("platform-sem-dois-pontos"),
            "platform-sem-dois-pontos"
        );
    }

    #[test]
    fn o_audio_emparelha_pela_porta_e_nao_pela_ordem() {
        let audio = vec![
            (
                "pci-0000:00:14.0-usb-0:4:1.2".to_string(),
                "hw:2".to_string(),
            ),
            (
                "pci-0000:00:14.0-usb-0:3:1.2".to_string(),
                "hw:1".to_string(),
            ),
        ];
        // O par certo é o da porta 3, mesmo estando em segundo lugar na lista.
        assert_eq!(
            pair_audio("pci-0000:00:14.0-usb-0:3:1.0", &audio),
            Some("hw:1")
        );
        // Uma porta sem áudio não herda o do vizinho.
        assert_eq!(pair_audio("pci-0000:00:14.0-usb-0:9:1.0", &audio), None);
    }

    #[test]
    fn dois_dongles_iguais_distinguem_se_pelo_caminho() {
        // O mesmo modelo, a mesma «série» do fabricante, portas diferentes.
        let a = validate_device_path("pci-0000:00:14.0-usb-0:3:1.0").unwrap();
        let b = validate_device_path("pci-0000:00:14.0-usb-0:4:1.0").unwrap();
        assert_ne!(a, b);
    }

    #[test]
    fn caminho_fisico_vazio_ou_com_espaco_e_recusado() {
        assert!(validate_device_path("").is_err());
        assert!(validate_device_path("  ").is_err());
        assert!(validate_device_path("pci-0000 usb-0:3").is_err());
        assert!(validate_device_path(&"x".repeat(MAX_DEVICE_PATH + 1)).is_err());
    }

    #[test]
    fn sinal_e_sintonia_recusam_valores_desconhecidos() {
        assert_eq!(Signal::parse("protected").unwrap(), Signal::Protected);
        assert!(Signal::parse("preto").is_err());
        assert!(!Signal::Protected.usable(), "HDCP não alimenta canal");
        assert!(Signal::Live.usable());
        assert_eq!(TunerKind::parse("ir").unwrap(), TunerKind::Ir);
        assert!(TunerKind::parse("rf").is_err());
    }

    #[test]
    fn nome_vazio_recusado_localizacao_vazia_aceita() {
        assert!(validate_name("  ").is_err());
        assert_eq!(validate_name("  Caixa A ").unwrap(), "Caixa A");
        assert_eq!(validate_location("").unwrap(), "");
    }
}
