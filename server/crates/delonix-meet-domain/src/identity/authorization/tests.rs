use super::*;
use Capability::*;
use CapabilityValue::*;

fn id(n: u128) -> Uuid {
    Uuid::from_u128(1000 + n)
}

const OWNER: u128 = 1;
const ADMIN: u128 = 2;
const MEMBER: u128 = 3;
const GUEST: u128 = 4;
const EMISSAO: u128 = 10; // departamento, herda de member
const FORMADOR: u128 = 11; // organização, herda de member
const SUBFORM: u128 = 12; // herda de FORMADOR
const DEPT_COM: u128 = 500;
const DEPT_RH: u128 = 501;

fn custom(
    n: u128,
    parent: u128,
    scope: RoleScope,
    vals: &[(Capability, CapabilityValue)],
) -> RoleDef {
    RoleDef {
        id: id(n),
        name: format!("r{n}"),
        system: None,
        inherits_from: Some(id(parent)),
        scope,
        values: vals.iter().copied().collect(),
        limits: RoleLimits::default(),
    }
}

fn set() -> RoleSet {
    RoleSet::new(vec![
        RoleDef::system(id(OWNER), SystemRole::Owner),
        RoleDef::system(id(ADMIN), SystemRole::Admin),
        RoleDef::system(id(MEMBER), SystemRole::Member),
        RoleDef::system(id(GUEST), SystemRole::ExternalGuest),
        custom(
            EMISSAO,
            MEMBER,
            RoleScope::Department {
                department_id: Some(Uuid::from_u128(DEPT_COM)),
            },
            &[
                (BroadcastPublicDestinations, Allow),
                (BroadcastManageRtmpKeys, Allow),
                (SessionsCreate, Deny),
            ],
        ),
        custom(
            FORMADOR,
            MEMBER,
            RoleScope::Organization,
            &[
                (RecordingsPublish, RequiresApproval),
                (AdminViewAudit, Allow),
                (RecordingsViewOthers, Allow),
            ],
        ),
        custom(
            SUBFORM,
            FORMADOR,
            RoleScope::Organization,
            &[(AdminViewAudit, Deny)],
        ),
    ])
    .unwrap()
}

fn subj(role: u128, dept: Option<u128>) -> Subject {
    Subject {
        role_id: id(role),
        department_id: dept.map(Uuid::from_u128),
    }
}

fn dept(d: u128) -> ResourceScope {
    ResourceScope::Department {
        department_id: Uuid::from_u128(d),
    }
}

#[test]
fn catalog_is_closed_and_round_trips() {
    for c in Capability::ALL {
        assert_eq!(Capability::parse(c.as_str()).unwrap(), c);
    }
    let e = Capability::parse("admin.*").unwrap_err();
    assert_eq!(e.code, "authz.unknown_capability");
    assert!(Capability::parse("").is_err());
    let codes: BTreeSet<_> = Capability::ALL.iter().map(|c| c.as_str()).collect();
    assert_eq!(codes.len(), Capability::ALL.len(), "códigos únicos");
    assert!(OrgAdminister.info().system_only);
    assert!(!RecordingsDelete.info().enforced, "não há rota que apague");
}

/// (papel, departamento da pertença, âmbito, capacidade, decisão, razão).
type PolicyCase = (
    u128,
    Option<u128>,
    ResourceScope,
    Capability,
    Decision,
    Reason,
);

/// A tabela da policy: (papel, departamento da pertença, âmbito, capacidade) → decisão, razão.
#[test]
fn policy_table() {
    let s = set();
    use Decision as D;
    use Reason as R;
    let org = ResourceScope::Organization;
    let cases: &[PolicyCase] = &[
        // sistema: semântica de hoje
        (OWNER, None, org, OrgAdminister, D::Allow, R::RoleValue),
        (
            ADMIN,
            None,
            org,
            AdminChangeRetention,
            D::Allow,
            R::RoleValue,
        ),
        (ADMIN, None, org, OrgAdminister, D::Allow, R::RoleValue),
        (MEMBER, None, org, SessionsCreate, D::Allow, R::RoleValue),
        (
            MEMBER,
            None,
            org,
            BroadcastManageRtmpKeys,
            D::Deny,
            R::RoleValue,
        ),
        (MEMBER, None, org, OrgAdminister, D::Deny, R::RoleValue),
        (GUEST, None, org, SessionsCreate, D::Deny, R::RoleValue),
        // herança: herdado do member
        (FORMADOR, None, org, SessionsCreate, D::Allow, R::Inherited),
        (
            FORMADOR,
            None,
            org,
            BroadcastManageRtmpKeys,
            D::Deny,
            R::Inherited,
        ),
        // deny explícito no filho ganha ao allow herdado do pai
        (SUBFORM, None, org, AdminViewAudit, D::Deny, R::RoleValue),
        (
            SUBFORM,
            None,
            org,
            RecordingsViewOthers,
            D::Allow,
            R::Inherited,
        ),
        // requer aprovação, também herdado
        (
            FORMADOR,
            None,
            org,
            RecordingsPublish,
            D::RequiresApproval,
            R::RoleValue,
        ),
        (
            SUBFORM,
            None,
            org,
            RecordingsPublish,
            D::RequiresApproval,
            R::Inherited,
        ),
        // âmbito departamento: dentro do seu departamento aplica-se
        (
            EMISSAO,
            Some(DEPT_COM),
            dept(DEPT_COM),
            BroadcastManageRtmpKeys,
            D::Allow,
            R::RoleValue,
        ),
        // … e um deny explícito ganha ao allow do member
        (
            EMISSAO,
            Some(DEPT_COM),
            dept(DEPT_COM),
            SessionsCreate,
            D::Deny,
            R::RoleValue,
        ),
        // fora do departamento, é Membro
        (
            EMISSAO,
            Some(DEPT_COM),
            dept(DEPT_RH),
            BroadcastManageRtmpKeys,
            D::Deny,
            R::OutOfScopeMember,
        ),
        (
            EMISSAO,
            Some(DEPT_COM),
            dept(DEPT_RH),
            SessionsCreate,
            D::Allow,
            R::OutOfScopeMember,
        ),
        // um recurso da organização (sem departamento) nunca está no âmbito
        (
            EMISSAO,
            Some(DEPT_COM),
            org,
            BroadcastManageRtmpKeys,
            D::Deny,
            R::OutOfScopeMember,
        ),
        // pertença sem departamento: nunca no âmbito
        (
            EMISSAO,
            None,
            dept(DEPT_COM),
            BroadcastManageRtmpKeys,
            D::Deny,
            R::OutOfScopeMember,
        ),
    ];
    for (role, d, scope, cap, dec, reason) in cases {
        let ev = can(&s, subj(*role, *d), *cap, *scope);
        assert_eq!(
            (ev.decision, ev.reason),
            (*dec, *reason),
            "papel {role} dept {d:?} {scope:?} {}",
            cap.as_str()
        );
    }
}

#[test]
fn inherit_at_root_is_not_granted_and_cycles_fail_closed() {
    let mut orphan = custom(20, 99, RoleScope::Organization, &[]);
    orphan.inherits_from = None;
    let mut a = custom(21, 22, RoleScope::Organization, &[]);
    let b = custom(22, 21, RoleScope::Organization, &[]);
    a.values.clear();
    let s = RoleSet::new(vec![
        RoleDef::system(id(MEMBER), SystemRole::Member),
        orphan,
        a,
        b,
    ])
    .unwrap();
    let ev = can(
        &s,
        subj(20, None),
        SessionsCreate,
        ResourceScope::Organization,
    );
    assert_eq!(
        (ev.decision, ev.reason),
        (Decision::Deny, Reason::NotGranted)
    );
    let ev = can(
        &s,
        subj(21, None),
        SessionsCreate,
        ResourceScope::Organization,
    );
    assert_eq!(ev.decision, Decision::Deny, "ciclo nunca concede");
}

#[test]
fn system_only_never_reaches_custom_role() {
    // Mesmo que alguém grave `org.administer` na base à mão, a policy recusa.
    let s = RoleSet::new(vec![
        RoleDef::system(id(MEMBER), SystemRole::Member),
        custom(
            30,
            MEMBER,
            RoleScope::Organization,
            &[(OrgAdminister, Allow)],
        ),
    ])
    .unwrap();
    let ev = can(
        &s,
        subj(30, None),
        OrgAdminister,
        ResourceScope::Organization,
    );
    assert_eq!(ev.decision, Decision::Deny);
}

#[test]
fn approval_only_where_supported() {
    let s = RoleSet::new(vec![
        RoleDef::system(id(MEMBER), SystemRole::Member),
        custom(
            31,
            MEMBER,
            RoleScope::Organization,
            &[(RecordingsViewOthers, RequiresApproval)],
        ),
    ])
    .unwrap();
    // Gravado à mão: a policy falha fechado (Deny), nunca Allow.
    let ev = can(
        &s,
        subj(31, None),
        RecordingsViewOthers,
        ResourceScope::Organization,
    );
    assert_eq!(ev.decision, Decision::Deny);
}

#[test]
fn role_write_validation() {
    let s = set();
    let admin_allowed = allowed_in_org(&s, subj(ADMIN, None));
    let formador_allowed = allowed_in_org(&s, subj(FORMADOR, None));
    let ok = |vals: &[(Capability, CapabilityValue)], actor: &BTreeSet<Capability>| {
        validate_role_write(
            &s,
            &ProposedRole {
                id: id(40),
                inherits_from: Some(id(MEMBER)),
                values: vals.iter().copied().collect(),
            },
            actor,
        )
    };
    assert!(ok(&[(BroadcastManageRtmpKeys, Allow)], &admin_allowed).is_ok());
    assert_eq!(
        ok(&[(StudioExport4k, Allow)], &admin_allowed)
            .unwrap_err()
            .code,
        "authz.capability_not_enforced"
    );
    assert!(ok(&[(StudioExport4k, Inherit)], &admin_allowed).is_ok());
    assert_eq!(
        ok(&[(OrgAdminister, Allow)], &admin_allowed)
            .unwrap_err()
            .code,
        "authz.system_only_capability"
    );
    assert_eq!(
        ok(&[(RecordingsViewOthers, RequiresApproval)], &admin_allowed)
            .unwrap_err()
            .code,
        "authz.approval_not_supported"
    );
    // Sem escalada: o Formador (que tem a auditoria) não dá chaves RTMP.
    assert_eq!(
        ok(&[(BroadcastManageRtmpKeys, Allow)], &formador_allowed)
            .unwrap_err()
            .code,
        "authz.escalation"
    );
    // … mas nega o que quiser.
    assert!(ok(&[(BroadcastManageRtmpKeys, Deny)], &formador_allowed).is_ok());
    // Papéis de sistema não se alteram.
    let e = validate_role_write(
        &s,
        &ProposedRole {
            id: id(ADMIN),
            inherits_from: None,
            values: HashMap::new(),
        },
        &admin_allowed,
    )
    .unwrap_err();
    assert_eq!(e.code, "role.system_immutable");
}

#[test]
fn inheritance_validation() {
    let s = set();
    let all: BTreeSet<_> = Capability::ALL.into_iter().collect();
    let p = |role: u128, parent: u128| ProposedRole {
        id: id(role),
        inherits_from: Some(id(parent)),
        values: HashMap::new(),
    };
    // FORMADOR a herdar de SUBFORM (que herda de FORMADOR) → ciclo.
    assert_eq!(
        validate_role_write(&s, &p(FORMADOR, SUBFORM), &all)
            .unwrap_err()
            .code,
        "role.inheritance_cycle"
    );
    assert_eq!(
        validate_role_write(&s, &p(50, 777), &all).unwrap_err().code,
        "role.parent_not_found"
    );
    assert_eq!(
        validate_role_write(&s, &p(50, OWNER), &all)
            .unwrap_err()
            .code,
        "role.inherit_from_owner"
    );
    // Cadeia longa: 60→61→62→63→64→member tem 6 papéis.
    let mut roles = vec![RoleDef::system(id(MEMBER), SystemRole::Member)];
    roles.push(custom(61, 62, RoleScope::Organization, &[]));
    roles.push(custom(62, 63, RoleScope::Organization, &[]));
    roles.push(custom(63, 64, RoleScope::Organization, &[]));
    roles.push(custom(64, MEMBER, RoleScope::Organization, &[]));
    let long = RoleSet::new(roles).unwrap();
    assert_eq!(
        validate_role_write(&long, &p(60, 61), &all)
            .unwrap_err()
            .code,
        "role.inheritance_too_deep"
    );
    assert!(validate_role_write(&long, &p(60, 62), &all).is_ok());
}

#[test]
fn assignment_has_no_escalation_and_owner_only_by_owner() {
    let s = set();
    // Admin atribui qualquer papel menos owner.
    assert!(validate_assignment(&s, id(EMISSAO), subj(ADMIN, None)).is_ok());
    assert_eq!(
        validate_assignment(&s, id(OWNER), subj(ADMIN, None))
            .unwrap_err()
            .code,
        "role.owner_assignment"
    );
    assert!(validate_assignment(&s, id(OWNER), subj(OWNER, None)).is_ok());
    // Formador não atribui Administrador (nem a si próprio).
    assert_eq!(
        validate_assignment(&s, id(ADMIN), subj(FORMADOR, None))
            .unwrap_err()
            .code,
        "authz.escalation"
    );
    // Formador atribui Membro (subconjunto).
    assert!(validate_assignment(&s, id(MEMBER), subj(FORMADOR, None)).is_ok());
    assert_eq!(
        validate_assignment(&s, id(999), subj(ADMIN, None))
            .unwrap_err()
            .code,
        "role.not_found"
    );
}

#[test]
fn last_owner() {
    assert_eq!(
        check_last_owner(1, true, false).unwrap_err().code,
        "role.last_owner"
    );
    assert!(check_last_owner(2, true, false).is_ok());
    assert!(check_last_owner(1, true, true).is_ok());
    assert!(check_last_owner(0, false, false).is_ok());
}

#[test]
fn effective_matches_can_for_every_role_and_capability() {
    let s = set();
    for role in s.roles() {
        let eff = effective_for_role(&s, role.id);
        let d = Uuid::from_u128(1);
        for cap in Capability::ALL {
            let sub = Subject {
                role_id: role.id,
                department_id: Some(d),
            };
            assert_eq!(
                eff[&cap].organization,
                can(&s, sub, cap, ResourceScope::Organization).decision
            );
            assert_eq!(
                eff[&cap].own_department,
                can(&s, sub, cap, ResourceScope::Department { department_id: d }).decision
            );
        }
    }
}

#[test]
fn limits_inherit_from_parent() {
    let mut s = set();
    let mut roles: Vec<RoleDef> = s.roles().cloned().collect();
    for r in roles.iter_mut() {
        if r.id == id(FORMADOR) {
            r.limits.max_simultaneous_destinations = Some(5);
            r.limits.max_external_guests_per_month = Some(25);
        }
        if r.id == id(SUBFORM) {
            r.limits.max_external_guests_per_month = Some(3);
        }
    }
    s = RoleSet::new(roles).unwrap();
    let l = effective_limits(&s, id(SUBFORM));
    assert_eq!(l.max_simultaneous_destinations, Some(5));
    assert_eq!(l.max_external_guests_per_month, Some(3));
    assert_eq!(effective_limits(&s, id(MEMBER)), RoleLimits::default());
}

#[test]
fn segregation_of_duties() {
    let s = set();
    let rule = SodRule {
        id: Uuid::nil(),
        capabilities: [BroadcastManageRtmpKeys, AdminViewAudit]
            .into_iter()
            .collect(),
        exempt_roles: [id(OWNER)].into_iter().collect(),
    };
    assert!(violates(&s, subj(ADMIN, None), &rule));
    assert!(!violates(&s, subj(OWNER, None), &rule), "isento");
    assert!(
        !violates(&s, subj(FORMADOR, None), &rule),
        "só tem a auditoria"
    );
    assert!(!violates(&s, subj(MEMBER, None), &rule));
    assert_eq!(
        validate_sod_rule(&[AdminViewAudit].into_iter().collect())
            .unwrap_err()
            .code,
        "sod.too_few_capabilities"
    );
}

#[test]
fn odoo_groups_table() {
    let s = set();
    let maps: BTreeMap<String, Uuid> = [
        ("g.emissao".to_string(), id(EMISSAO)),
        ("g.formador".to_string(), id(FORMADOR)),
        ("g.dono".to_string(), id(OWNER)),
    ]
    .into_iter()
    .collect();
    let g = |xs: &[&str]| xs.iter().map(|x| x.to_string()).collect::<BTreeSet<_>>();
    use GroupOutcome as O;
    use RoleSource::*;
    let m = id(MEMBER);
    let cases: Vec<(Uuid, RoleSource, Option<BTreeSet<String>>, O)> = vec![
        (m, Manual, None, O::NoChange),
        (m, Manual, Some(g(&[])), O::NoChange),
        (m, Manual, Some(g(&["g.emissao"])), O::Assign(id(EMISSAO))),
        (id(EMISSAO), OdooGroup, Some(g(&["g.emissao"])), O::NoChange),
        (id(EMISSAO), OdooGroup, Some(g(&[])), O::RevertToMember),
        (
            id(EMISSAO),
            OdooGroup,
            Some(g(&["g.formador"])),
            O::Assign(id(FORMADOR)),
        ),
        // papel dado à mão não se decide sozinho
        (
            id(ADMIN),
            Manual,
            Some(g(&["g.formador"])),
            O::Conflict {
                proposed: vec![id(FORMADOR)],
            },
        ),
        (id(ADMIN), Manual, Some(g(&[])), O::NoChange),
        // dois grupos para papéis diferentes
        (
            m,
            Manual,
            Some(g(&["g.formador", "g.emissao"])),
            O::Conflict {
                proposed: {
                    let mut v = vec![id(EMISSAO), id(FORMADOR)];
                    v.sort();
                    v
                },
            },
        ),
        // nunca promove para dono, e nunca mexe num dono
        (m, Manual, Some(g(&["g.dono"])), O::NoChange),
        (id(OWNER), Manual, Some(g(&["g.formador"])), O::NoChange),
        (m, OdooGroup, Some(g(&[])), O::NoChange),
    ];
    for (role, src, groups, want) in cases {
        assert_eq!(
            odoo_group_outcome(&s, role, src, groups.as_ref(), &maps),
            want,
            "{role} {src:?} {groups:?}"
        );
    }
}

#[test]
fn canonical_json_orders_keys() {
    let a = serde_json::json!({"b": 1, "a": {"y": [1, {"d": 2, "c": 3}], "x": "é"}});
    assert_eq!(
        canonical_json(&a),
        r#"{"a":{"x":"é","y":[1,{"c":3,"d":2}]},"b":1}"#
    );
}

#[test]
fn validations() {
    assert_eq!(
        validate_role_name(" ").unwrap_err().code,
        "role.invalid_name"
    );
    assert_eq!(validate_role_name(" Formador ").unwrap(), "Formador");
    assert!(validate_odoo_group("delonix_comunicacao_emissao").is_ok());
    assert!(validate_odoo_group("base.group_user").is_ok());
    assert!(validate_odoo_group("x y").is_err());
    assert!(validate_limit("x", Some(-1)).is_err());
    assert_eq!(CapabilityValue::parse("allow").unwrap(), Allow);
    assert_eq!(
        CapabilityValue::parse("sim").unwrap_err().code,
        "authz.invalid_value"
    );
    assert_eq!(SystemRole::legacy_role(Some(SystemRole::Owner)), "admin");
    assert_eq!(SystemRole::legacy_role(None), "member");
}
