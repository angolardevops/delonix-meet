//! Perfis de instalação (ADR-0005 §2).
//!
//! Uma edição fixa **valores por omissão**; cada política pode ser sobreposta
//! à parte. Não é licenciamento: nenhuma capacidade fica fechada no código por
//! edição. Sem `DELONIX_EDITION`, o perfil é `saas`, que é exactamente o
//! comportamento anterior (registo aberto, uma org por domínio de email).

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Edition {
    /// Multi-inquilino, registo aberto, superfície de operador.
    Saas,
    /// On-premise de uma empresa: uma org, registo por convite/SSO.
    Enterprise,
    /// Uma pessoa (ou uma equipa pequena) num só binário.
    Personal,
}

/// Quem pode criar conta.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RegistrationMode {
    /// Qualquer pessoa; cria (ou junta-se à) org do seu domínio de email.
    Open,
    /// Só emails de domínios permitidos (`REGISTRATION_DOMAINS`).
    Domain,
    /// Só por convite (um admin adiciona o funcionário) ou por SSO.
    Invite,
    /// Ninguém — excepto o primeiro utilizador numa instalação vazia.
    Closed,
}

/// Quantas organizações a instalação tem.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TenancyMode {
    /// Uma org por empresa (domínio de email).
    Multi,
    /// Uma só org na instalação; toda a gente entra nela.
    Single,
}

impl Edition {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "saas" => Some(Self::Saas),
            "enterprise" | "on-premise" | "onprem" => Some(Self::Enterprise),
            "personal" | "single-user" => Some(Self::Personal),
            _ => None,
        }
    }

    pub fn default_registration(self) -> RegistrationMode {
        match self {
            Self::Saas => RegistrationMode::Open,
            Self::Enterprise => RegistrationMode::Invite,
            Self::Personal => RegistrationMode::Closed,
        }
    }

    pub fn default_tenancy(self) -> TenancyMode {
        match self {
            Self::Saas => TenancyMode::Multi,
            Self::Enterprise | Self::Personal => TenancyMode::Single,
        }
    }

    /// A superfície de operador da plataforma faz sentido onde há mais de um
    /// inquilino a operar.
    pub fn operator_surface(self) -> bool {
        !matches!(self, Self::Personal)
    }
}

impl RegistrationMode {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "open" => Some(Self::Open),
            "domain" => Some(Self::Domain),
            "invite" => Some(Self::Invite),
            "closed" => Some(Self::Closed),
            _ => None,
        }
    }
}

impl TenancyMode {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "multi" => Some(Self::Multi),
            "single" => Some(Self::Single),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presets() {
        assert_eq!(Edition::Saas.default_registration(), RegistrationMode::Open);
        assert_eq!(Edition::Saas.default_tenancy(), TenancyMode::Multi);
        assert_eq!(
            Edition::Enterprise.default_registration(),
            RegistrationMode::Invite
        );
        assert_eq!(Edition::Personal.default_tenancy(), TenancyMode::Single);
        assert!(!Edition::Personal.operator_surface());
        assert_eq!(Edition::parse("On-Premise"), Some(Edition::Enterprise));
        assert_eq!(Edition::parse("x"), None);
    }
}
