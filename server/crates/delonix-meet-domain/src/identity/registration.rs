//! Quem pode criar conta, e o que acontece quando cria (ADR-0005 §2).
//!
//! A regra é PURA: recebe a política da instalação, o pedido e um retrato do
//! que a instalação já tem (lido pelo adaptador dentro de uma transação com
//! trinco), e devolve um PLANO. Quem executa o plano (SQL) não decide nada.
//!
//! Antes desta regra só havia um caminho: registo aberto, e cada registo
//! criava uma organização nova para o domínio do email. Esse caminho é o perfil
//! `saas` + `open` + `multi`, e continua igual (mesmas mensagens, mesmos
//! códigos HTTP).

use delonix_meet_core::edition::{Edition, RegistrationMode, TenancyMode};
use delonix_meet_core::DomainError;
use uuid::Uuid;

use super::validation;

#[derive(Debug, Clone)]
pub struct RegistrationPolicy {
    pub edition: Edition,
    pub mode: RegistrationMode,
    pub tenancy: TenancyMode,
    /// Domínios aceites em `RegistrationMode::Domain` (minúsculas).
    pub allowed_domains: Vec<String>,
}

/// O que a instalação já tem, no momento da decisão.
#[derive(Debug, Clone, Default)]
pub struct InstallationSnapshot {
    /// Há pelo menos uma conta de pessoa (as contas técnicas não contam).
    pub has_users: bool,
    /// Já existe uma organização com o domínio deste email.
    pub domain_taken: bool,
    /// Em tenancy `single`: a organização da instalação, se já existe.
    pub single_org: Option<Uuid>,
}

#[derive(Debug, Clone)]
pub struct RegistrationRequest {
    /// Já normalizado (`validation::normalize_email`).
    pub email: String,
    pub org_name: Option<String>,
    pub username: String,
    pub password: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OrgKind {
    Company,
    Personal,
}

impl OrgKind {
    pub fn as_str(self) -> &'static str {
        match self {
            OrgKind::Company => "company",
            OrgKind::Personal => "personal",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RegistrationPlan {
    /// Cria a conta e uma organização nova, com a conta como administradora.
    CreateOrganization {
        name: String,
        /// Domínio que passa a ser da org (único). `None` numa org pessoal ou
        /// num email sem domínio corporativo em tenancy `single`.
        email_domain: Option<String>,
        kind: OrgKind,
    },
    /// Cria a conta e junta-a à organização única da instalação.
    JoinOrganization { org_id: Uuid, as_admin: bool },
}

/// Decide o registo. Os erros de forma mantêm as mensagens que o web já
/// mostra; os de política têm código próprio.
pub fn plan(
    policy: &RegistrationPolicy,
    req: &RegistrationRequest,
    installation: &InstallationSnapshot,
) -> Result<RegistrationPlan, DomainError> {
    validation::validate_email(&req.email)
        .map_err(|m| DomainError::invalid("registration.invalid_email", m))?;
    validation::validate_password(&req.password)
        .map_err(|m| DomainError::invalid("registration.invalid_password", m))?;

    let domain = req.email.split('@').nth(1).unwrap_or("").to_string();
    // A primeira conta de uma instalação vazia entra sempre: é quem a administra.
    let bootstrap = !installation.has_users;

    if !bootstrap {
        match policy.mode {
            RegistrationMode::Open => {}
            RegistrationMode::Domain => {
                if !policy.allowed_domains.iter().any(|d| d == &domain) {
                    return Err(DomainError::forbidden("registration.domain_not_allowed")
                        .with_message(format!(
                            "o registo está limitado a emails de {}",
                            policy.allowed_domains.join(", ")
                        )));
                }
            }
            RegistrationMode::Invite => {
                return Err(
                    DomainError::forbidden("registration.invite_only").with_message(
                        "o registo é por convite — pede ao administrador para te adicionar",
                    ),
                )
            }
            RegistrationMode::Closed => {
                return Err(DomainError::forbidden("registration.closed")
                    .with_message("o registo está fechado nesta instalação"))
            }
        }
    }

    match policy.tenancy {
        TenancyMode::Multi => {
            let name = org_name(req)?;
            let domain = validation::require_corporate_domain(&req.email)
                .map_err(|m| DomainError::invalid("registration.corporate_email_required", m))?;
            if installation.domain_taken {
                return Err(DomainError::conflict(
                    "registration.domain_taken",
                    format!(
                        "o domínio «{domain}» já tem uma organização registada — pede ao teu administrador para te adicionar"
                    ),
                ));
            }
            Ok(RegistrationPlan::CreateOrganization {
                name,
                email_domain: Some(domain),
                kind: OrgKind::Company,
            })
        }
        TenancyMode::Single => {
            if let Some(org_id) = installation.single_org {
                return Ok(RegistrationPlan::JoinOrganization {
                    org_id,
                    as_admin: bootstrap,
                });
            }
            if policy.edition == Edition::Personal {
                let name = req
                    .org_name
                    .as_deref()
                    .map(str::trim)
                    .filter(|n| n.len() >= 2)
                    .map(|n| n.chars().take(80).collect())
                    .unwrap_or_else(|| format!("Espaço de {}", req.username));
                return Ok(RegistrationPlan::CreateOrganization {
                    name,
                    email_domain: None,
                    kind: OrgKind::Personal,
                });
            }
            let name = org_name(req)?;
            Ok(RegistrationPlan::CreateOrganization {
                name,
                email_domain: validation::require_corporate_domain(&req.email).ok(),
                kind: OrgKind::Company,
            })
        }
    }
}

fn org_name(req: &RegistrationRequest) -> Result<String, DomainError> {
    let name = req.org_name.as_deref().unwrap_or("").trim().to_string();
    if name.len() < 2 || name.len() > 80 {
        return Err(DomainError::invalid(
            "registration.invalid_org_name",
            "nome da organização deve ter 2-80 caracteres",
        )
        .with_field("org_name", "2-80 caracteres"));
    }
    Ok(name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use delonix_meet_core::ErrorKind;

    fn policy(edition: Edition) -> RegistrationPolicy {
        RegistrationPolicy {
            edition,
            mode: edition.default_registration(),
            tenancy: edition.default_tenancy(),
            allowed_domains: vec![],
        }
    }

    fn req(email: &str, org: Option<&str>) -> RegistrationRequest {
        RegistrationRequest {
            email: email.into(),
            org_name: org.map(Into::into),
            username: "ana".into(),
            password: "UmaPasswordForte123!".into(),
        }
    }

    fn populated() -> InstallationSnapshot {
        InstallationSnapshot {
            has_users: true,
            ..Default::default()
        }
    }

    #[test]
    fn saas_keeps_the_historic_flow() {
        let p = plan(
            &policy(Edition::Saas),
            &req("ana@empresa.ao", Some("Empresa")),
            &populated(),
        )
        .unwrap();
        assert_eq!(
            p,
            RegistrationPlan::CreateOrganization {
                name: "Empresa".into(),
                email_domain: Some("empresa.ao".into()),
                kind: OrgKind::Company
            }
        );
        let taken = InstallationSnapshot {
            has_users: true,
            domain_taken: true,
            single_org: None,
        };
        let e = plan(
            &policy(Edition::Saas),
            &req("ana@empresa.ao", Some("Empresa")),
            &taken,
        )
        .unwrap_err();
        assert_eq!(e.kind, ErrorKind::Conflict);
        let e = plan(
            &policy(Edition::Saas),
            &req("ana@empresa.ao", None),
            &populated(),
        )
        .unwrap_err();
        assert_eq!(e.code, "registration.invalid_org_name");
    }

    #[test]
    fn personal_bootstraps_once_then_closes() {
        let pol = policy(Edition::Personal);
        let first = plan(
            &pol,
            &req("eu@gmail.com", None),
            &InstallationSnapshot::default(),
        )
        .unwrap();
        assert_eq!(
            first,
            RegistrationPlan::CreateOrganization {
                name: "Espaço de ana".into(),
                email_domain: None,
                kind: OrgKind::Personal
            }
        );
        let e = plan(&pol, &req("outro@gmail.com", None), &populated()).unwrap_err();
        assert_eq!(e.code, "registration.closed");
        assert_eq!(e.kind, ErrorKind::PermissionDenied);
    }

    #[test]
    fn enterprise_is_invite_only_after_bootstrap() {
        let pol = policy(Edition::Enterprise);
        let first = plan(
            &pol,
            &req("admin@banco.ao", Some("Banco")),
            &InstallationSnapshot::default(),
        )
        .unwrap();
        assert!(matches!(
            first,
            RegistrationPlan::CreateOrganization {
                kind: OrgKind::Company,
                ..
            }
        ));
        let e = plan(&pol, &req("joao@banco.ao", Some("x")), &populated()).unwrap_err();
        assert_eq!(e.code, "registration.invite_only");
    }

    #[test]
    fn single_tenancy_with_open_mode_joins_the_org() {
        let mut pol = policy(Edition::Enterprise);
        pol.mode = RegistrationMode::Open;
        let org = Uuid::new_v4();
        let snap = InstallationSnapshot {
            has_users: true,
            domain_taken: false,
            single_org: Some(org),
        };
        let p = plan(&pol, &req("joao@banco.ao", None), &snap).unwrap();
        assert_eq!(
            p,
            RegistrationPlan::JoinOrganization {
                org_id: org,
                as_admin: false
            }
        );
    }

    #[test]
    fn domain_mode_checks_allowlist() {
        let mut pol = policy(Edition::Saas);
        pol.mode = RegistrationMode::Domain;
        pol.allowed_domains = vec!["banco.ao".into()];
        assert!(plan(&pol, &req("a@banco.ao", Some("Banco")), &populated()).is_ok());
        let e = plan(&pol, &req("a@outro.ao", Some("Outro")), &populated()).unwrap_err();
        assert_eq!(e.code, "registration.domain_not_allowed");
    }

    #[test]
    fn form_errors_come_before_policy() {
        let e = plan(
            &policy(Edition::Personal),
            &req("sem-arroba", None),
            &populated(),
        )
        .unwrap_err();
        assert_eq!(e.code, "registration.invalid_email");
    }
}
