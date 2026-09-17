//! Autorização de PESSOAS dentro de uma organização (ADR-0008).
//!
//! - Catálogo FECHADO de capacidades ([`Capability`]), versionado.
//! - Papéis = conjuntos nomeados de valores por capacidade, com herança.
//! - A policy [`can`] é pura: recebe o papel da pertença, o departamento da
//!   pertença e o âmbito do recurso, e devolve uma [`Evaluation`] com o porquê.
//!
//! Os escopos das chaves de API (`api_key::Scope`) são de MÁQUINAS e não se
//! misturam com isto: copia-se o padrão (catálogo no domínio, `parse` que
//! recusa desconhecidos com código estável), não o catálogo.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use delonix_meet_core::DomainError;
use serde::Serialize;
use uuid::Uuid;

/// Versão do catálogo. Sobe quando entra ou sai uma capacidade.
pub const CATALOG_VERSION: u32 = 1;

/// Profundidade máxima da cadeia de herança (o papel e 4 antepassados).
pub const MAX_INHERITANCE_DEPTH: usize = 5;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub enum Capability {
    SessionsCreate,
    SessionsAdmitWaitingRoom,
    SessionsMuteRemove,
    SessionsBreakoutRooms,
    RecordingsRecord4k,
    RecordingsViewOthers,
    RecordingsPublish,
    RecordingsDelete,
    BroadcastPublicDestinations,
    BroadcastManageRtmpKeys,
    BroadcastHighlightQuestions,
    StudioEditTimeline,
    StudioGenerateCaptions,
    StudioExport4k,
    AdminManageAccounts,
    AdminManageRoles,
    AdminViewAudit,
    AdminChangeRetention,
    OrgAdminister,
}

/// Descrição estática de uma capacidade (o que `GET /api/capabilities` serve).
#[derive(Debug, Clone, Copy, Serialize)]
pub struct CapabilityInfo {
    pub code: &'static str,
    pub group: &'static str,
    pub group_label: &'static str,
    pub label: &'static str,
    pub hint: &'static str,
    /// O servidor impõe-na. Se `false`, a matriz só aceita o valor por omissão.
    pub enforced: bool,
    /// Onde é imposta (rotas ou funções).
    pub enforced_at: &'static [&'static str],
    /// Só papéis de sistema a têm; nunca um personalizado.
    pub system_only: bool,
    /// Aceita `requires_approval` (um pedido aprovável faz sentido no ponto de imposição).
    pub approval_supported: bool,
}

impl Capability {
    pub const ALL: [Capability; 19] = [
        Capability::SessionsCreate,
        Capability::SessionsAdmitWaitingRoom,
        Capability::SessionsMuteRemove,
        Capability::SessionsBreakoutRooms,
        Capability::RecordingsRecord4k,
        Capability::RecordingsViewOthers,
        Capability::RecordingsPublish,
        Capability::RecordingsDelete,
        Capability::BroadcastPublicDestinations,
        Capability::BroadcastManageRtmpKeys,
        Capability::BroadcastHighlightQuestions,
        Capability::StudioEditTimeline,
        Capability::StudioGenerateCaptions,
        Capability::StudioExport4k,
        Capability::AdminManageAccounts,
        Capability::AdminManageRoles,
        Capability::AdminViewAudit,
        Capability::AdminChangeRetention,
        Capability::OrgAdminister,
    ];

    pub fn as_str(self) -> &'static str {
        self.info().code
    }

    /// Recusa o desconhecido (sem `*`, sem prefixos).
    pub fn parse(raw: &str) -> Result<Capability, DomainError> {
        Capability::ALL
            .into_iter()
            .find(|c| c.as_str() == raw.trim())
            .ok_or_else(|| {
                DomainError::invalid(
                    "authz.unknown_capability",
                    format!("capacidade desconhecida: {raw}"),
                )
                .with_field("capability", "um código de GET /api/capabilities")
            })
    }

    pub fn info(self) -> CapabilityInfo {
        use Capability::*;
        const S: (&str, &str) = ("sessions", "Sessões");
        const R: (&str, &str) = ("recordings", "Gravação e biblioteca");
        const B: (&str, &str) = ("broadcast", "Emissão");
        const T: (&str, &str) = ("studio", "Estúdio e quadro");
        const A: (&str, &str) = ("admin", "Administração");
        let mk =
            |code, g: (&'static str, &'static str), label, hint, at: &'static [&'static str]| {
                CapabilityInfo {
                    code,
                    group: g.0,
                    group_label: g.1,
                    label,
                    hint,
                    enforced: !at.is_empty(),
                    enforced_at: at,
                    system_only: false,
                    approval_supported: !at.is_empty(),
                }
            };
        match self {
            SessionsCreate => mk(
                "sessions.create",
                S,
                "Criar e agendar sessões",
                "",
                &[
                    "POST /api/rooms",
                    "POST /api/meetings",
                    "POST /api/v1/rooms",
                    "POST /api/v1/meetings",
                ],
            ),
            SessionsAdmitWaitingRoom => mk(
                "sessions.admit_waiting_room",
                S,
                "Admitir da sala de espera",
                "inclui convidados por telefone",
                &[],
            ),
            SessionsMuteRemove => mk(
                "sessions.mute_remove",
                S,
                "Silenciar e remover pessoas",
                "",
                &[],
            ),
            SessionsBreakoutRooms => mk(
                "sessions.breakout_rooms",
                S,
                "Abrir salas paralelas",
                "",
                &[],
            ),
            RecordingsRecord4k => mk("recordings.record_4k", R, "Gravar em 4K", "", &[]),
            RecordingsViewOthers => CapabilityInfo {
                // Imposta dentro do SQL de visibilidade: só `allow` conta, não
                // há pedido aprovável possível numa query que filtra.
                approval_supported: false,
                ..mk(
                    "recordings.view_others",
                    R,
                    "Ver gravações de outros",
                    "só dentro do âmbito",
                    &["biblioteca: descarregar e gerir a gravação de um colega"],
                )
            },
            RecordingsPublish => mk(
                "recordings.publish",
                R,
                "Publicar gravação",
                "",
                &[
                    "POST /api/recordings/{recording_id}/shares",
                    "PUT /api/recordings/{recording_id}/public-link",
                ],
            ),
            RecordingsDelete => mk(
                "recordings.delete",
                R,
                "Apagar gravação",
                "acção irreversível",
                &[],
            ),
            BroadcastPublicDestinations => mk(
                "broadcast.public_destinations",
                B,
                "Emitir para destinos públicos",
                "",
                &["GET /api/rooms/{room_code}/live (destinos guardados)"],
            ),
            BroadcastManageRtmpKeys => mk(
                "broadcast.manage_rtmp_keys",
                B,
                "Gerir chaves RTMP",
                "credenciais de terceiros",
                &["/api/orgs/{org_id}/stream-destinations"],
            ),
            BroadcastHighlightQuestions => mk(
                "broadcast.highlight_questions",
                B,
                "Destacar perguntas no palco",
                "",
                &[],
            ),
            StudioEditTimeline => mk(
                "studio.edit_timeline",
                T,
                "Editar na linha de tempo",
                "",
                &[],
            ),
            StudioGenerateCaptions => mk(
                "studio.generate_captions",
                T,
                "Gerar legendas e dobragem",
                "consome nó de inferência",
                &[],
            ),
            StudioExport4k => mk("studio.export_4k", T, "Exportar em 4K", "", &[]),
            AdminManageAccounts => mk(
                "admin.manage_accounts",
                A,
                "Convidar e suspender contas",
                "",
                &[
                    "/api/orgs/{org_id}/members",
                    "/api/orgs/{org_id}/invitations",
                    "/api/orgs/{org_id}/users",
                    "/api/orgs/{org_id}/seats",
                ],
            ),
            AdminManageRoles => mk(
                "admin.manage_roles",
                A,
                "Editar papéis e permissões",
                "",
                &[
                    "/api/orgs/{org_id}/roles",
                    "/api/orgs/{org_id}/sod-rules",
                    "/api/orgs/{org_id}/authorization/evaluations",
                ],
            ),
            AdminViewAudit => mk(
                "admin.view_audit",
                A,
                "Ver registo de auditoria",
                "",
                &["/api/orgs/{org_id}/audit-events"],
            ),
            AdminChangeRetention => mk(
                "admin.change_retention",
                A,
                "Mudar retenção e residência",
                "afecta toda a organização",
                &["PATCH /api/orgs/{org_id}"],
            ),
            OrgAdminister => CapabilityInfo {
                system_only: true,
                approval_supported: false,
                ..mk(
                    "org.administer",
                    A,
                    "Administração técnica da organização",
                    "integrações, SSO, chaves de API, webhooks, voz, SMS — só papéis de sistema",
                    &["todas as rotas de administração sem capacidade fina (require_admin)"],
                )
            },
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityValue {
    Allow,
    Deny,
    Inherit,
    RequiresApproval,
}

impl CapabilityValue {
    pub fn as_str(self) -> &'static str {
        match self {
            CapabilityValue::Allow => "allow",
            CapabilityValue::Deny => "deny",
            CapabilityValue::Inherit => "inherit",
            CapabilityValue::RequiresApproval => "requires_approval",
        }
    }
    pub fn parse(raw: &str) -> Result<Self, DomainError> {
        match raw.trim() {
            "allow" => Ok(Self::Allow),
            "deny" => Ok(Self::Deny),
            "inherit" => Ok(Self::Inherit),
            "requires_approval" => Ok(Self::RequiresApproval),
            other => Err(DomainError::invalid(
                "authz.invalid_value",
                format!("valor desconhecido: {other}"),
            )
            .with_field("value", "allow | deny | inherit | requires_approval")),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SystemRole {
    Owner,
    Admin,
    Member,
    ExternalGuest,
}

impl SystemRole {
    pub const ALL: [SystemRole; 4] = [
        SystemRole::Owner,
        SystemRole::Admin,
        SystemRole::Member,
        SystemRole::ExternalGuest,
    ];
    pub fn key(self) -> &'static str {
        match self {
            SystemRole::Owner => "owner",
            SystemRole::Admin => "admin",
            SystemRole::Member => "member",
            SystemRole::ExternalGuest => "external_guest",
        }
    }
    pub fn parse(raw: &str) -> Option<SystemRole> {
        SystemRole::ALL.into_iter().find(|r| r.key() == raw)
    }
    pub fn name(self) -> &'static str {
        match self {
            SystemRole::Owner => "Proprietário",
            SystemRole::Admin => "Administrador",
            SystemRole::Member => "Membro",
            SystemRole::ExternalGuest => "Convidado externo",
        }
    }
    /// A coluna herdada `org_members.role` que este papel produz.
    pub fn legacy_role(key: Option<SystemRole>) -> &'static str {
        match key {
            Some(SystemRole::Owner) | Some(SystemRole::Admin) => "admin",
            _ => "member",
        }
    }
    /// O valor semeado. Semântica de hoje: `owner`/`admin` = o `admin` de hoje
    /// (tudo), `member` = o que um membro faz hoje nas capacidades impostas.
    pub fn default_value(self, cap: Capability) -> CapabilityValue {
        use Capability::*;
        use CapabilityValue::*;
        match self {
            SystemRole::Owner | SystemRole::Admin => Allow,
            SystemRole::Member => match cap {
                SessionsCreate | RecordingsRecord4k => Allow,
                _ => Deny,
            },
            SystemRole::ExternalGuest => Deny,
        }
    }
}

/// Âmbito de um papel personalizado.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RoleScope {
    Organization,
    /// Só se aplica a recursos do departamento DA PERTENÇA.
    Department {
        department_id: Option<Uuid>,
    },
}

/// Âmbito do RECURSO sobre o qual se decide.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ResourceScope {
    Organization,
    Department { department_id: Uuid },
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct RoleLimits {
    pub max_simultaneous_destinations: Option<i32>,
    pub max_external_guests_per_month: Option<i32>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RoleDef {
    pub id: Uuid,
    pub name: String,
    pub system: Option<SystemRole>,
    pub inherits_from: Option<Uuid>,
    pub scope: RoleScope,
    /// Capacidades ausentes = `inherit` (personalizado) ou o valor semeado (sistema).
    pub values: HashMap<Capability, CapabilityValue>,
    pub limits: RoleLimits,
}

impl RoleDef {
    pub fn value(&self, cap: Capability) -> CapabilityValue {
        match (self.values.get(&cap), self.system) {
            (Some(v), _) => *v,
            (None, Some(sys)) => sys.default_value(cap),
            (None, None) => CapabilityValue::Inherit,
        }
    }

    /// O papel de sistema com os valores semeados (para testes e para semear).
    pub fn system(id: Uuid, role: SystemRole) -> RoleDef {
        RoleDef {
            id,
            name: role.name().into(),
            system: Some(role),
            inherits_from: None,
            scope: RoleScope::Organization,
            values: Capability::ALL
                .into_iter()
                .map(|c| (c, role.default_value(c)))
                .collect(),
            limits: RoleLimits::default(),
        }
    }
}

/// Os papéis de UMA organização.
#[derive(Debug, Clone)]
pub struct RoleSet {
    roles: HashMap<Uuid, RoleDef>,
    member_role: Uuid,
}

impl RoleSet {
    /// Exige o papel `member` de sistema (é o de quem está fora do âmbito).
    pub fn new(roles: Vec<RoleDef>) -> Result<RoleSet, DomainError> {
        let member_role = roles
            .iter()
            .find(|r| r.system == Some(SystemRole::Member))
            .map(|r| r.id)
            .ok_or_else(|| DomainError::internal("organização sem o papel member de sistema"))?;
        Ok(RoleSet {
            roles: roles.into_iter().map(|r| (r.id, r)).collect(),
            member_role,
        })
    }
    pub fn get(&self, id: Uuid) -> Option<&RoleDef> {
        self.roles.get(&id)
    }
    pub fn roles(&self) -> impl Iterator<Item = &RoleDef> {
        self.roles.values()
    }
    pub fn member_role(&self) -> Uuid {
        self.member_role
    }
    pub fn by_system(&self, sys: SystemRole) -> Option<&RoleDef> {
        self.roles.values().find(|r| r.system == Some(sys))
    }
}

/// A pessoa que pede, dentro da organização.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Subject {
    pub role_id: Uuid,
    pub department_id: Option<Uuid>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Decision {
    Allow,
    RequiresApproval,
    Deny,
}

/// Porque é que a decisão é esta.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Reason {
    /// Valor explícito no papel da pertença.
    RoleValue,
    /// Herdado de um antepassado (`via_role_id`).
    Inherited,
    /// Papel de departamento fora do seu âmbito: avaliado como Membro.
    OutOfScopeMember,
    /// Nenhum papel da cadeia define a capacidade.
    NotGranted,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Evaluation {
    pub capability: Capability,
    pub decision: Decision,
    pub reason: Reason,
    /// O papel da pertença.
    pub role_id: Uuid,
    /// O papel de onde veio o valor que decidiu.
    pub via_role_id: Option<Uuid>,
    /// A cadeia percorrida, do papel ao que decidiu.
    pub chain: Vec<Uuid>,
}

/// Resolve o valor de `cap` num papel, subindo a herança. O valor explícito mais
/// próximo ganha — um `deny` no filho ganha a um `allow` herdado.
fn resolve(
    set: &RoleSet,
    role_id: Uuid,
    cap: Capability,
) -> (CapabilityValue, Option<Uuid>, Vec<Uuid>) {
    let mut chain = Vec::new();
    let mut current = Some(role_id);
    while let Some(id) = current {
        if chain.len() >= MAX_INHERITANCE_DEPTH || chain.contains(&id) {
            break; // ciclos e cadeias longas são recusados ao gravar; aqui falha fechado
        }
        chain.push(id);
        let Some(role) = set.get(id) else { break };
        match role.value(cap) {
            CapabilityValue::Inherit => current = role.inherits_from,
            v => return (v, Some(id), chain),
        }
    }
    (CapabilityValue::Deny, None, chain)
}

/// A policy. Sem IO.
pub fn can(set: &RoleSet, subject: Subject, cap: Capability, scope: ResourceScope) -> Evaluation {
    let role = set.get(subject.role_id);
    let in_scope = match role.map(|r| r.scope) {
        None | Some(RoleScope::Organization) => true,
        Some(RoleScope::Department { .. }) => match scope {
            ResourceScope::Organization => false,
            ResourceScope::Department { department_id } => {
                subject.department_id == Some(department_id)
            }
        },
    };
    // `org.administer` nunca vem de um personalizado, nem por herança.
    let evaluated_role = if in_scope {
        subject.role_id
    } else {
        set.member_role()
    };
    let (value, via, chain) = resolve(set, evaluated_role, cap);
    let value = if cap.info().system_only
        && via
            .and_then(|v| set.get(v))
            .is_some_and(|r| r.system.is_none())
    {
        CapabilityValue::Deny
    } else {
        value
    };
    let decision = match value {
        CapabilityValue::Allow => Decision::Allow,
        CapabilityValue::RequiresApproval if cap.info().approval_supported => {
            Decision::RequiresApproval
        }
        _ => Decision::Deny,
    };
    let reason = if !in_scope {
        Reason::OutOfScopeMember
    } else if via.is_none() {
        Reason::NotGranted
    } else if via == Some(subject.role_id) {
        Reason::RoleValue
    } else {
        Reason::Inherited
    };
    Evaluation {
        capability: cap,
        decision,
        reason,
        role_id: subject.role_id,
        via_role_id: via,
        chain,
    }
}

/// Os dois âmbitos que se materializam por papel (ADR-0008 §4): o recurso da
/// organização, e o recurso do departamento DA pertença.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EffectiveValues {
    pub organization: Decision,
    pub own_department: Decision,
}

/// Calcula, para um papel, a decisão efectiva de cada capacidade nos dois
/// âmbitos. É o que se grava em `org_role_effective_capabilities`.
pub fn effective_for_role(set: &RoleSet, role_id: Uuid) -> BTreeMap<Capability, EffectiveValues> {
    // Um departamento fictício que é o da própria pertença.
    let dept = Uuid::from_u128(1);
    let subject = Subject {
        role_id,
        department_id: Some(dept),
    };
    Capability::ALL
        .into_iter()
        .map(|c| {
            (
                c,
                EffectiveValues {
                    organization: can(set, subject, c, ResourceScope::Organization).decision,
                    own_department: can(
                        set,
                        subject,
                        c,
                        ResourceScope::Department {
                            department_id: dept,
                        },
                    )
                    .decision,
                },
            )
        })
        .collect()
}

/// Limites efectivos (o `None` de um personalizado herda do pai).
pub fn effective_limits(set: &RoleSet, role_id: Uuid) -> RoleLimits {
    let mut out = RoleLimits::default();
    let mut seen = Vec::new();
    let mut current = Some(role_id);
    while let Some(id) = current {
        if seen.len() >= MAX_INHERITANCE_DEPTH || seen.contains(&id) {
            break;
        }
        seen.push(id);
        let Some(r) = set.get(id) else { break };
        out.max_simultaneous_destinations = out
            .max_simultaneous_destinations
            .or(r.limits.max_simultaneous_destinations);
        out.max_external_guests_per_month = out
            .max_external_guests_per_month
            .or(r.limits.max_external_guests_per_month);
        current = r.inherits_from;
    }
    out
}

/// Capacidades `allow` de uma pertença no âmbito da organização.
pub fn allowed_in_org(set: &RoleSet, subject: Subject) -> BTreeSet<Capability> {
    Capability::ALL
        .into_iter()
        .filter(|c| can(set, subject, *c, ResourceScope::Organization).decision == Decision::Allow)
        .collect()
}

/// Capacidades `allow` em QUALQUER âmbito da pertença (organização ∪ o seu departamento).
pub fn allowed_anywhere(set: &RoleSet, subject: Subject) -> BTreeSet<Capability> {
    let dept = subject.department_id.unwrap_or(Uuid::from_u128(1));
    let s = Subject {
        department_id: Some(dept),
        ..subject
    };
    Capability::ALL
        .into_iter()
        .filter(|c| {
            can(set, s, *c, ResourceScope::Organization).decision == Decision::Allow
                || can(
                    set,
                    s,
                    *c,
                    ResourceScope::Department {
                        department_id: dept,
                    },
                )
                .decision
                    == Decision::Allow
        })
        .collect()
}

// ---------------------------------------------------------------------------
//  Validação de escritas de papéis (invariantes do ADR-0008 §3 e §5)
// ---------------------------------------------------------------------------

/// Um papel personalizado proposto (criar ou alterar).
#[derive(Debug, Clone)]
pub struct ProposedRole {
    pub id: Uuid,
    pub inherits_from: Option<Uuid>,
    pub values: HashMap<Capability, CapabilityValue>,
}

/// Valida a matriz e a herança de um papel personalizado.
///
/// - papéis de sistema não se alteram;
/// - capacidade não imposta só aceita `inherit`;
/// - `org.administer` nunca;
/// - `requires_approval` só onde é suportado;
/// - herança sem ciclos e com profundidade ≤ 5, e só para papéis da org;
/// - sem escalada: quem edita só dá `allow`/`requires_approval` ao que tem `allow`.
pub fn validate_role_write(
    set: &RoleSet,
    proposed: &ProposedRole,
    actor_allowed: &BTreeSet<Capability>,
) -> Result<(), DomainError> {
    if set.get(proposed.id).is_some_and(|r| r.system.is_some()) {
        return Err(DomainError::precondition(
            "role.system_immutable",
            "os papéis de sistema não se alteram",
        ));
    }
    for (cap, value) in &proposed.values {
        let info = cap.info();
        if *value == CapabilityValue::Inherit {
            continue;
        }
        if info.system_only {
            return Err(DomainError::precondition(
                "authz.system_only_capability",
                format!("{} só existe nos papéis de sistema", info.code),
            )
            .with_field("capabilities", info.code));
        }
        if !info.enforced {
            return Err(DomainError::precondition(
                "authz.capability_not_enforced",
                format!(
                    "{} ainda não é imposta pelo servidor — só aceita o valor por omissão (inherit)",
                    info.code
                ),
            )
            .with_field("capabilities", info.code));
        }
        if *value == CapabilityValue::RequiresApproval && !info.approval_supported {
            return Err(DomainError::precondition(
                "authz.approval_not_supported",
                format!("{} não aceita «requer aprovação»", info.code),
            )
            .with_field("capabilities", info.code));
        }
        if matches!(
            value,
            CapabilityValue::Allow | CapabilityValue::RequiresApproval
        ) && !actor_allowed.contains(cap)
        {
            return Err(DomainError::forbidden("authz.escalation")
                .with_message(format!("não pode conceder {} — não a tem", info.code))
                .with_field("capabilities", info.code));
        }
    }
    // Herança: o pai existe na org, sem ciclo, profundidade ≤ 5.
    let mut depth = 1;
    let mut current = proposed.inherits_from;
    while let Some(parent) = current {
        if parent == proposed.id {
            return Err(
                DomainError::invalid("role.inheritance_cycle", "a herança faria um ciclo")
                    .with_field("inherits_from", "outro papel"),
            );
        }
        let Some(p) = set.get(parent) else {
            return Err(DomainError::invalid(
                "role.parent_not_found",
                "o papel de que herda não existe nesta organização",
            )
            .with_field("inherits_from", "id de um papel desta organização"));
        };
        if p.system == Some(SystemRole::Owner) {
            return Err(DomainError::precondition(
                "role.inherit_from_owner",
                "um papel não herda do Proprietário",
            ));
        }
        depth += 1;
        if depth > MAX_INHERITANCE_DEPTH {
            return Err(DomainError::invalid(
                "role.inheritance_too_deep",
                format!("a cadeia de herança passa de {MAX_INHERITANCE_DEPTH} papéis"),
            ));
        }
        current = p.inherits_from;
    }
    Ok(())
}

/// Sem escalada ao ATRIBUIR: todas as capacidades `allow` do papel (em qualquer
/// âmbito) têm de estar no `allow` de quem atribui. `owner` só por `owner`.
pub fn validate_assignment(
    set: &RoleSet,
    target_role: Uuid,
    actor: Subject,
) -> Result<(), DomainError> {
    let Some(role) = set.get(target_role) else {
        return Err(DomainError::not_found("role.not_found"));
    };
    let actor_role = set.get(actor.role_id).and_then(|r| r.system);
    if role.system == Some(SystemRole::Owner) && actor_role != Some(SystemRole::Owner) {
        return Err(DomainError::forbidden("role.owner_assignment")
            .with_message("só um Proprietário atribui o papel de Proprietário"));
    }
    let actor_allowed = allowed_anywhere(set, actor);
    let needed = allowed_anywhere(
        set,
        Subject {
            role_id: target_role,
            department_id: actor.department_id,
        },
    );
    if let Some(missing) = needed.difference(&actor_allowed).next() {
        return Err(DomainError::forbidden("authz.escalation")
            .with_message(format!(
                "não pode atribuir «{}»: o papel tem {} e quem atribui não",
                role.name,
                missing.as_str()
            ))
            .with_field("role_id", missing.as_str()));
    }
    Ok(())
}

/// Último dono: mudar o papel ou desactivar `target` deixa a org sem dono?
pub fn check_last_owner(
    active_owner_count: i64,
    target_is_owner: bool,
    target_stays_owner_and_active: bool,
) -> Result<(), DomainError> {
    if target_is_owner && !target_stays_owner_and_active && active_owner_count <= 1 {
        return Err(DomainError::conflict(
            "role.last_owner",
            "a organização ficava sem Proprietário",
        ));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
//  Segregação de funções
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct SodRule {
    pub id: Uuid,
    pub capabilities: BTreeSet<Capability>,
    pub exempt_roles: BTreeSet<Uuid>,
}

/// Valida uma regra: pelo menos duas capacidades distintas.
pub fn validate_sod_rule(caps: &BTreeSet<Capability>) -> Result<(), DomainError> {
    if caps.len() < 2 {
        return Err(DomainError::invalid(
            "sod.too_few_capabilities",
            "uma regra de segregação combina pelo menos duas capacidades",
        )
        .with_field("capabilities", "≥ 2 códigos distintos"));
    }
    Ok(())
}

/// A pessoa viola a regra? (`allow` em organização ∪ o seu departamento, papel não isento.)
pub fn violates(set: &RoleSet, subject: Subject, rule: &SodRule) -> bool {
    if rule.exempt_roles.contains(&subject.role_id) {
        return false;
    }
    let allowed = allowed_anywhere(set, subject);
    rule.capabilities.is_subset(&allowed)
}

// ---------------------------------------------------------------------------
//  Grupos do Odoo → papel (ADR-0008 §9)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RoleSource {
    Manual,
    OdooGroup,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GroupOutcome {
    NoChange,
    Assign(Uuid),
    RevertToMember,
    Conflict { proposed: Vec<Uuid> },
}

/// Decide o papel de uma pessoa a partir dos grupos que o Odoo diz que ela tem.
///
/// `groups = None` (a leitura falhou) nunca muda nada: ausência de dado não é
/// «saiu de todos os grupos».
pub fn odoo_group_outcome(
    set: &RoleSet,
    current_role: Uuid,
    current_source: RoleSource,
    groups: Option<&BTreeSet<String>>,
    mappings: &BTreeMap<String, Uuid>,
) -> GroupOutcome {
    let Some(groups) = groups else {
        return GroupOutcome::NoChange;
    };
    let current_sys = set.get(current_role).and_then(|r| r.system);
    if current_sys == Some(SystemRole::Owner) {
        return GroupOutcome::NoChange; // um grupo nunca mexe num dono
    }
    let proposed: BTreeSet<Uuid> = groups
        .iter()
        .filter_map(|g| mappings.get(g))
        .copied()
        .filter(|r| {
            set.get(*r)
                .is_some_and(|d| d.system != Some(SystemRole::Owner))
        })
        .collect();
    match proposed.len() {
        0 if current_source == RoleSource::OdooGroup => {
            if current_role == set.member_role() {
                GroupOutcome::NoChange
            } else {
                GroupOutcome::RevertToMember
            }
        }
        0 => GroupOutcome::NoChange,
        1 => {
            let r = *proposed.iter().next().unwrap_or(&current_role);
            if r == current_role {
                GroupOutcome::NoChange
            } else if current_source == RoleSource::OdooGroup || current_role == set.member_role() {
                GroupOutcome::Assign(r)
            } else {
                GroupOutcome::Conflict { proposed: vec![r] }
            }
        }
        // Dois ou mais grupos para papéis diferentes: decide um admin.
        _ => GroupOutcome::Conflict {
            proposed: proposed.into_iter().collect(),
        },
    }
}

/// Valida um identificador de grupo Odoo (id externo `modulo.nome` ou `nome`).
pub fn validate_odoo_group(raw: &str) -> Result<String, DomainError> {
    let g = raw.trim();
    let ok = !g.is_empty()
        && g.len() <= 128
        && g.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-'));
    if !ok {
        return Err(DomainError::invalid(
            "role.invalid_odoo_group",
            "grupo do Odoo inválido (id externo, ex.: modulo.grupo)",
        )
        .with_field("odoo_group", "letras, dígitos, _ . - (até 128)"));
    }
    Ok(g.to_string())
}

/// Valida o nome de um papel.
pub fn validate_role_name(raw: &str) -> Result<String, DomainError> {
    let n = raw.trim();
    if n.is_empty() || n.chars().count() > 60 {
        return Err(DomainError::invalid(
            "role.invalid_name",
            "o nome do papel tem 1–60 caracteres",
        )
        .with_field("name", "1–60 caracteres"));
    }
    Ok(n.to_string())
}

/// Valida um limite (inteiro positivo ou ausente).
pub fn validate_limit(field: &'static str, v: Option<i32>) -> Result<Option<i32>, DomainError> {
    match v {
        Some(n) if !(0..=100_000).contains(&n) => Err(DomainError::invalid(
            "role.invalid_limit",
            format!("{field} fora do intervalo"),
        )
        .with_field(field, "0–100000")),
        other => Ok(other),
    }
}

/// Hash canónico de um alvo JSON já canónico (ADR-0008 §6).
pub fn canonical_json(value: &serde_json::Value) -> String {
    fn walk(v: &serde_json::Value, out: &mut String) {
        match v {
            serde_json::Value::Object(map) => {
                let sorted: BTreeMap<_, _> = map.iter().collect();
                out.push('{');
                for (i, (k, v)) in sorted.into_iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    out.push_str(&serde_json::Value::String(k.clone()).to_string());
                    out.push(':');
                    walk(v, out);
                }
                out.push('}');
            }
            serde_json::Value::Array(items) => {
                out.push('[');
                for (i, v) in items.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    walk(v, out);
                }
                out.push(']');
            }
            other => out.push_str(&other.to_string()),
        }
    }
    let mut s = String::new();
    walk(value, &mut s);
    s
}

#[cfg(test)]
mod tests;
