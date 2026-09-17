//! Plano de marcação: regras ordenadas, «a primeira regra que casar vale».
//!
//! ## Padrões
//!
//! A sintaxe é a dos PBX (FreeSWITCH/Asterisk), aplicada ao número já
//! normalizado ([`super::number::parse_dialed`]):
//!
//! | Símbolo | Casa com |
//! |---|---|
//! | `0`–`9` | esse dígito |
//! | `X` | qualquer dígito `0-9` |
//! | `Z` | `1-9` |
//! | `N` | `2-9` |
//! | `[1-5]`, `[135]` | um dígito da classe |
//! | `.` (só no fim) | um ou mais dígitos |
//! | `!` (só no fim) | zero ou mais dígitos |
//! | `112,113,115` | lista: qualquer das alternativas |
//!
//! O padrão casa com o número INTEIRO: `1XX` casa `123` e não `1234`.
//!
//! ## Emergência — invariante do servidor
//!
//! Os números de emergência (dado da instalação, `112,113,115` por omissão) são
//! resolvidos ANTES das regras ordenadas:
//!
//! - **nunca gravados**: uma regra `emergency` com `record: true` é recusada, e
//!   a resolução devolve sempre `record: false`, mesmo que uma regra anterior
//!   dissesse o contrário;
//! - **nunca bloqueados**: uma regra `block` que case um número de emergência é
//!   recusada ao gravar o plano; e a resolução junta, depois dos troncos da
//!   regra de emergência, TODOS os outros troncos activos como reserva — o
//!   limite de canais ou um tronco em baixo não deixa uma chamada de emergência
//!   sem caminho enquanto houver outro.
//!
//! O que o servidor NÃO consegue garantir: se não há nenhum tronco activo, não
//! há caminho — a resolução diz `no_available_trunk` em vez de fingir um.

use delonix_meet_core::DomainError;
use serde::Serialize;
use uuid::Uuid;

pub const MAX_RULES: usize = 100;
pub const MAX_PATTERN_LEN: usize = 128;
pub const MAX_ALTERNATIVES: usize = 20;
pub const MAX_DESCRIPTION: usize = 120;

#[derive(Debug, Clone, PartialEq, Eq)]
enum Token {
    Digit(u8),
    Class([bool; 10]),
    /// `.` — um ou mais.
    OneOrMore,
    /// `!` — zero ou mais.
    ZeroOrMore,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pattern {
    source: String,
    alternatives: Vec<Vec<Token>>,
}

fn class(from: u8, to: u8) -> Token {
    let mut c = [false; 10];
    for d in from..=to {
        c[d as usize] = true;
    }
    Token::Class(c)
}

fn invalid_pattern(source: &str, why: &str) -> DomainError {
    DomainError::invalid(
        "telephony.invalid_pattern",
        format!("padrão «{source}» inválido: {why}"),
    )
    .with_field(
        "pattern",
        "dígitos, X, Z, N, [1-5], «.» ou «!» no fim, lista com vírgulas",
    )
}

impl Pattern {
    pub fn parse(source: &str) -> Result<Self, DomainError> {
        let s = source.trim();
        if s.is_empty() {
            return Err(invalid_pattern(source, "vazio"));
        }
        if s.len() > MAX_PATTERN_LEN {
            return Err(invalid_pattern(source, "demasiado longo"));
        }
        let parts: Vec<&str> = s.split(',').map(str::trim).collect();
        if parts.len() > MAX_ALTERNATIVES {
            return Err(invalid_pattern(source, "demasiadas alternativas"));
        }
        let mut alternatives = Vec::with_capacity(parts.len());
        for part in parts {
            alternatives.push(Self::parse_one(source, part)?);
        }
        Ok(Self {
            source: s.to_string(),
            alternatives,
        })
    }

    fn parse_one(source: &str, part: &str) -> Result<Vec<Token>, DomainError> {
        if part.is_empty() {
            return Err(invalid_pattern(source, "alternativa vazia"));
        }
        let bytes = part.as_bytes();
        let mut tokens = Vec::new();
        let mut i = 0;
        while i < bytes.len() {
            let b = bytes[i];
            let last = i == bytes.len() - 1;
            match b {
                b'0'..=b'9' => tokens.push(Token::Digit(b - b'0')),
                b'X' | b'x' => tokens.push(class(0, 9)),
                b'Z' | b'z' => tokens.push(class(1, 9)),
                b'N' | b'n' => tokens.push(class(2, 9)),
                b'.' | b'!' => {
                    if !last {
                        return Err(invalid_pattern(source, "«.» e «!» só no fim"));
                    }
                    tokens.push(if b == b'.' {
                        Token::OneOrMore
                    } else {
                        Token::ZeroOrMore
                    });
                }
                b'[' => {
                    let end = part[i..]
                        .find(']')
                        .map(|e| i + e)
                        .ok_or_else(|| invalid_pattern(source, "«[» sem «]»"))?;
                    let inner = &bytes[i + 1..end];
                    if inner.is_empty() {
                        return Err(invalid_pattern(source, "classe vazia"));
                    }
                    let mut c = [false; 10];
                    let mut j = 0;
                    while j < inner.len() {
                        let d = inner[j];
                        if !d.is_ascii_digit() {
                            return Err(invalid_pattern(source, "classe só com dígitos"));
                        }
                        if j + 2 < inner.len() && inner[j + 1] == b'-' {
                            let to = inner[j + 2];
                            if !to.is_ascii_digit() || to < d {
                                return Err(invalid_pattern(source, "intervalo inválido"));
                            }
                            for x in d..=to {
                                c[(x - b'0') as usize] = true;
                            }
                            j += 3;
                        } else {
                            c[(d - b'0') as usize] = true;
                            j += 1;
                        }
                    }
                    tokens.push(Token::Class(c));
                    i = end;
                }
                _ => return Err(invalid_pattern(source, "carácter não permitido")),
            }
            i += 1;
        }
        if matches!(tokens.as_slice(), [Token::OneOrMore | Token::ZeroOrMore]) {
            return Err(invalid_pattern(source, "casaria com tudo"));
        }
        Ok(tokens)
    }

    pub fn as_str(&self) -> &str {
        &self.source
    }

    pub fn matches(&self, digits: &str) -> bool {
        let d = digits.as_bytes();
        if !d.iter().all(u8::is_ascii_digit) {
            return false;
        }
        self.alternatives.iter().any(|alt| match_tokens(alt, d))
    }

    /// Expressão regular equivalente (sintaxe PCRE do FreeSWITCH), ancorada.
    pub fn to_regex(&self) -> String {
        let alts: Vec<String> = self
            .alternatives
            .iter()
            .map(|alt| {
                alt.iter()
                    .map(|t| match t {
                        Token::Digit(d) => d.to_string(),
                        Token::Class(c) => {
                            let ds: String = (0..10)
                                .filter(|i| c[*i])
                                .map(|i| char::from(b'0' + i as u8))
                                .collect();
                            if ds == "0123456789" {
                                "\\d".to_string()
                            } else {
                                format!("[{ds}]")
                            }
                        }
                        Token::OneOrMore => "\\d+".to_string(),
                        Token::ZeroOrMore => "\\d*".to_string(),
                    })
                    .collect()
            })
            .collect();
        format!("^(?:{})$", alts.join("|"))
    }
}

fn match_tokens(tokens: &[Token], d: &[u8]) -> bool {
    let mut i = 0;
    for t in tokens {
        match t {
            Token::Digit(x) => {
                if i >= d.len() || d[i] - b'0' != *x {
                    return false;
                }
                i += 1;
            }
            Token::Class(c) => {
                if i >= d.len() || !c[(d[i] - b'0') as usize] {
                    return false;
                }
                i += 1;
            }
            Token::OneOrMore => return d.len() > i,
            Token::ZeroOrMore => return true,
        }
    }
    i == d.len()
}

/// O que a regra faz com a chamada.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RuleAction {
    /// Sai por um tronco (operadora).
    External,
    /// Entra numa sala pelo PIN (IVR de dial-in).
    RoomPin,
    /// Ramal interno — não sai para a rede pública.
    Extension,
    /// Recusada.
    Block,
}

impl RuleAction {
    pub const ALL: [&'static str; 4] = ["external", "room_pin", "extension", "block"];

    pub fn parse(s: &str) -> Result<Self, DomainError> {
        Ok(match s {
            "external" => Self::External,
            "room_pin" => Self::RoomPin,
            "extension" => Self::Extension,
            "block" => Self::Block,
            other => {
                return Err(DomainError::invalid(
                    "telephony.invalid_rule_action",
                    format!(
                        "acção «{other}» inválida — válidas: {}",
                        Self::ALL.join(", ")
                    ),
                )
                .with_field("action", Self::ALL.join(" | ")))
            }
        })
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::External => "external",
            Self::RoomPin => "room_pin",
            Self::Extension => "extension",
            Self::Block => "block",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DialRule {
    pub pattern: Pattern,
    pub description: String,
    pub action: RuleAction,
    pub trunk_id: Option<Uuid>,
    pub fallback_trunk_id: Option<Uuid>,
    pub record: bool,
    pub emergency: bool,
}

/// A regra como chega do cliente, antes de validada.
#[derive(Debug, Clone)]
pub struct DialRuleInput {
    pub pattern: String,
    pub description: String,
    pub action: String,
    pub trunk_id: Option<Uuid>,
    pub fallback_trunk_id: Option<Uuid>,
    pub record: bool,
    pub emergency: bool,
}

/// Lista de números de emergência (dado da instalação).
pub fn parse_emergency_numbers(spec: &str) -> Vec<String> {
    spec.split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()))
        .map(str::to_string)
        .collect()
}

fn rule_err(code: &'static str, position: usize, msg: impl Into<String>) -> DomainError {
    DomainError::invalid(code, msg).with_field(format!("rules[{position}]"), code)
}

/// Valida o plano inteiro. `trunks` são os ids dos troncos DESTA org (um id de
/// outra org conta como desconhecido). Posições nas mensagens começam em 0.
pub fn validate_plan(
    input: &[DialRuleInput],
    trunks: &[Uuid],
    emergency_numbers: &[String],
) -> Result<Vec<DialRule>, DomainError> {
    if input.len() > MAX_RULES {
        return Err(DomainError::invalid(
            "telephony.too_many_rules",
            format!("no máximo {MAX_RULES} regras"),
        ));
    }
    let mut out = Vec::with_capacity(input.len());
    for (pos, r) in input.iter().enumerate() {
        let pattern = Pattern::parse(&r.pattern)
            .map_err(|e| e.with_field(format!("rules[{pos}].pattern"), "inválido"))?;
        let description = r.description.trim().to_string();
        if description.is_empty() || description.chars().count() > MAX_DESCRIPTION {
            return Err(rule_err(
                "telephony.invalid_rule_description",
                pos,
                format!("regra {pos}: descrição obrigatória, até {MAX_DESCRIPTION} caracteres"),
            ));
        }
        let action = RuleAction::parse(&r.action)?;
        for t in [r.trunk_id, r.fallback_trunk_id].into_iter().flatten() {
            if !trunks.contains(&t) {
                return Err(rule_err(
                    "telephony.unknown_trunk",
                    pos,
                    format!("regra {pos}: tronco {t} não existe nesta organização"),
                ));
            }
        }
        match action {
            RuleAction::External => {
                if r.trunk_id.is_none() {
                    return Err(rule_err(
                        "telephony.rule_requires_trunk",
                        pos,
                        format!("regra {pos}: uma regra externa precisa de operadora"),
                    ));
                }
                if r.fallback_trunk_id.is_some() && r.fallback_trunk_id == r.trunk_id {
                    return Err(rule_err(
                        "telephony.fallback_equals_trunk",
                        pos,
                        format!("regra {pos}: a reserva tem de ser outra operadora"),
                    ));
                }
            }
            _ => {
                if r.trunk_id.is_some() || r.fallback_trunk_id.is_some() {
                    return Err(rule_err(
                        "telephony.rule_trunk_not_allowed",
                        pos,
                        format!(
                            "regra {pos}: só as regras externas escolhem operadora ({})",
                            action.as_str()
                        ),
                    ));
                }
            }
        }
        let hits_emergency = emergency_numbers.iter().any(|n| pattern.matches(n));
        if r.emergency {
            if action != RuleAction::External {
                return Err(rule_err(
                    "telephony.emergency_must_be_external",
                    pos,
                    format!("regra {pos}: emergência sai sempre por uma operadora"),
                ));
            }
            if r.record {
                return Err(rule_err(
                    "telephony.emergency_never_recorded",
                    pos,
                    format!("regra {pos}: uma chamada de emergência nunca é gravada"),
                ));
            }
            if !hits_emergency {
                return Err(rule_err(
                    "telephony.emergency_rule_without_emergency_number",
                    pos,
                    format!(
                        "regra {pos}: marcada como emergência mas não casa nenhum número de emergência ({})",
                        emergency_numbers.join(", ")
                    ),
                ));
            }
        }
        if action == RuleAction::Block && hits_emergency {
            return Err(rule_err(
                "telephony.emergency_cannot_be_blocked",
                pos,
                format!(
                    "regra {pos}: «{}» bloquearia um número de emergência",
                    pattern.as_str()
                ),
            ));
        }
        out.push(DialRule {
            pattern,
            description,
            action,
            trunk_id: r.trunk_id,
            fallback_trunk_id: r.fallback_trunk_id,
            record: r.record && !r.emergency,
            emergency: r.emergency,
        });
    }
    Ok(out)
}

/// Um tronco visto pela resolução: id e se está activo, pela ORDEM de
/// encaminhamento da org.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TrunkRef {
    pub id: Uuid,
    pub enabled: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ResolutionOutcome {
    /// Sai pelos troncos em `legs`, por esta ordem.
    Route,
    /// Destino interno (sala por PIN ou ramal): não passa por operadora.
    Internal,
    /// Uma regra `block` casou.
    Blocked,
    /// Nenhuma regra casou.
    NoMatch,
    /// A regra casou mas nenhum dos troncos está activo.
    NoAvailableTrunk,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Resolution {
    pub outcome: ResolutionOutcome,
    /// Posição (0-based) da regra que decidiu, se houve.
    pub rule_position: Option<usize>,
    pub action: Option<RuleAction>,
    /// Troncos a tentar, por ordem (o primeiro é o principal).
    pub legs: Vec<Uuid>,
    pub record: bool,
    pub emergency: bool,
    /// Numa emergência: a regra NÃO-emergência que teria casado primeiro e foi
    /// ultrapassada pelo invariante.
    pub overridden_rule_position: Option<usize>,
}

fn enabled_legs(ids: &[Option<Uuid>], trunks: &[TrunkRef]) -> Vec<Uuid> {
    let mut legs = Vec::new();
    for id in ids.iter().flatten() {
        if trunks.iter().any(|t| t.id == *id && t.enabled) && !legs.contains(id) {
            legs.push(*id);
        }
    }
    legs
}

/// Resolve `digits` (já normalizado) contra o plano.
pub fn resolve(
    rules: &[DialRule],
    digits: &str,
    emergency_numbers: &[String],
    trunks: &[TrunkRef],
) -> Resolution {
    let first = rules.iter().position(|r| r.pattern.matches(digits));
    if emergency_numbers.iter().any(|n| n == digits) {
        let em = rules
            .iter()
            .position(|r| r.emergency && r.pattern.matches(digits));
        let mut legs = match em {
            Some(p) => enabled_legs(&[rules[p].trunk_id, rules[p].fallback_trunk_id], trunks),
            None => Vec::new(),
        };
        // Todos os outros troncos activos, pela ordem da org, como reserva.
        for t in trunks.iter().filter(|t| t.enabled) {
            if !legs.contains(&t.id) {
                legs.push(t.id);
            }
        }
        return Resolution {
            outcome: if legs.is_empty() {
                ResolutionOutcome::NoAvailableTrunk
            } else {
                ResolutionOutcome::Route
            },
            rule_position: em,
            action: Some(RuleAction::External),
            legs,
            record: false,
            emergency: true,
            overridden_rule_position: first.filter(|f| Some(*f) != em),
        };
    }
    let Some(p) = first else {
        return Resolution {
            outcome: ResolutionOutcome::NoMatch,
            rule_position: None,
            action: None,
            legs: Vec::new(),
            record: false,
            emergency: false,
            overridden_rule_position: None,
        };
    };
    let r = &rules[p];
    let (outcome, legs) = match r.action {
        RuleAction::External => {
            let legs = enabled_legs(&[r.trunk_id, r.fallback_trunk_id], trunks);
            if legs.is_empty() {
                (ResolutionOutcome::NoAvailableTrunk, legs)
            } else {
                (ResolutionOutcome::Route, legs)
            }
        }
        RuleAction::RoomPin | RuleAction::Extension => (ResolutionOutcome::Internal, Vec::new()),
        RuleAction::Block => (ResolutionOutcome::Blocked, Vec::new()),
    };
    Resolution {
        outcome,
        rule_position: Some(p),
        action: Some(r.action),
        legs,
        record: r.record && outcome != ResolutionOutcome::Blocked,
        emergency: false,
        overridden_rule_position: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn em() -> Vec<String> {
        parse_emergency_numbers("112,113,115")
    }

    #[test]
    fn pattern_table() {
        // (padrão, número, casa?)
        let cases: &[(&str, &str, bool)] = &[
            ("9XXXXXXXX", "923447108", true),
            ("9XXXXXXXX", "92344710", false),
            ("9XXXXXXXX", "9234471080", false),
            ("9XXXXXXXX", "823447108", false),
            ("2XXXXXXXX", "222640100", true),
            ("84209", "84209", true),
            ("84209", "842090", false),
            ("1XX", "123", true),
            ("1XX", "12", false),
            ("1XX", "1234", false),
            ("00X.", "0027115550192", true),
            ("00X.", "00", false),
            ("00X.", "002", false),
            ("00XXXXXXXXXX", "002711555019", true),
            ("112,113,115", "113", true),
            ("112,113,115", "114", false),
            ("112, 113 ,115", "115", true),
            ("9[1-5]XXXXXXX", "953447108", true),
            ("9[1-5]XXXXXXX", "963447108", false),
            ("9[135]XXXXXXX", "933447108", true),
            ("9[135]XXXXXXX", "943447108", false),
            ("NXX", "123", false),
            ("NXX", "223", true),
            ("ZXX", "023", false),
            ("9!", "9", true),
            ("9!", "91234", true),
            ("x", "7", true),
        ];
        for (pat, num, want) in cases {
            let p = Pattern::parse(pat).unwrap();
            assert_eq!(p.matches(num), *want, "{pat} ~ {num}");
        }
    }

    #[test]
    fn regex_equivalent() {
        assert_eq!(
            Pattern::parse("9XXXXXXXX").unwrap().to_regex(),
            "^(?:9\\d\\d\\d\\d\\d\\d\\d\\d)$"
        );
        assert_eq!(
            Pattern::parse("112,113").unwrap().to_regex(),
            "^(?:112|113)$"
        );
        assert_eq!(
            Pattern::parse("00N.").unwrap().to_regex(),
            "^(?:00[23456789]\\d+)$"
        );
    }

    #[test]
    fn bad_patterns() {
        for bad in [
            "", "9.X", "abc", "9[", "[]", "[5-1]", ".", "!", "1,,2", "+244", "9 X",
        ] {
            assert_eq!(
                Pattern::parse(bad).unwrap_err().code,
                "telephony.invalid_pattern",
                "{bad}"
            );
        }
    }

    fn input(pattern: &str, action: &str, trunk: Option<Uuid>) -> DialRuleInput {
        DialRuleInput {
            pattern: pattern.into(),
            description: "d".into(),
            action: action.into(),
            trunk_id: trunk,
            fallback_trunk_id: None,
            record: true,
            emergency: false,
        }
    }

    #[test]
    fn emergency_invariants_at_validation() {
        let t = Uuid::new_v4();
        let mut r = input("112,113,115", "external", Some(t));
        r.emergency = true;
        let e = validate_plan(std::slice::from_ref(&r), &[t], &em()).unwrap_err();
        assert_eq!(e.code, "telephony.emergency_never_recorded");
        r.record = false;
        assert!(validate_plan(std::slice::from_ref(&r), &[t], &em()).is_ok());

        let e = validate_plan(&[input("11X", "block", None)], &[t], &em()).unwrap_err();
        assert_eq!(e.code, "telephony.emergency_cannot_be_blocked");
        let e = validate_plan(&[input("1X.", "block", None)], &[t], &em()).unwrap_err();
        assert_eq!(e.code, "telephony.emergency_cannot_be_blocked");
        assert!(validate_plan(&[input("0800X.", "block", None)], &[t], &em()).is_ok());

        let mut notem = input("9XXXXXXXX", "external", Some(t));
        notem.emergency = true;
        notem.record = false;
        assert_eq!(
            validate_plan(&[notem], &[t], &em()).unwrap_err().code,
            "telephony.emergency_rule_without_emergency_number"
        );
    }

    #[test]
    fn trunk_rules_at_validation() {
        let t = Uuid::new_v4();
        let other_org = Uuid::new_v4();
        assert_eq!(
            validate_plan(&[input("9XXXXXXXX", "external", None)], &[t], &em())
                .unwrap_err()
                .code,
            "telephony.rule_requires_trunk"
        );
        assert_eq!(
            validate_plan(
                &[input("9XXXXXXXX", "external", Some(other_org))],
                &[t],
                &em()
            )
            .unwrap_err()
            .code,
            "telephony.unknown_trunk"
        );
        assert_eq!(
            validate_plan(&[input("1XX", "extension", Some(t))], &[t], &em())
                .unwrap_err()
                .code,
            "telephony.rule_trunk_not_allowed"
        );
        let mut r = input("9XXXXXXXX", "external", Some(t));
        r.fallback_trunk_id = Some(t);
        assert_eq!(
            validate_plan(&[r], &[t], &em()).unwrap_err().code,
            "telephony.fallback_equals_trunk"
        );
    }

    /// O plano do ecrã (Navegavel3 DelonixTelecom), com quatro troncos.
    fn screen_plan() -> (Vec<DialRule>, Vec<TrunkRef>, [Uuid; 4]) {
        let [uni, afr, mov, int] = [
            Uuid::new_v4(),
            Uuid::new_v4(),
            Uuid::new_v4(),
            Uuid::new_v4(),
        ];
        let rule = |p: &str, a: &str, t: Option<Uuid>, f: Option<Uuid>, rec: bool, e: bool| {
            DialRuleInput {
                pattern: p.into(),
                description: p.into(),
                action: a.into(),
                trunk_id: t,
                fallback_trunk_id: f,
                record: rec,
                emergency: e,
            }
        };
        let rules = validate_plan(
            &[
                rule("9XXXXXXXX", "external", Some(uni), Some(afr), true, false),
                rule("2XXXXXXXX", "external", Some(uni), Some(mov), true, false),
                rule("84209", "room_pin", None, None, true, false),
                rule("1XX", "extension", None, None, false, false),
                rule("00XXXXXXXXXX", "external", Some(int), None, true, false),
                rule("112,113,115", "external", Some(uni), Some(afr), false, true),
            ],
            &[uni, afr, mov, int],
            &em(),
        )
        .unwrap();
        let trunks = [uni, afr, mov, int]
            .iter()
            .map(|id| TrunkRef {
                id: *id,
                enabled: true,
            })
            .collect();
        (rules, trunks, [uni, afr, mov, int])
    }

    #[test]
    fn first_match_wins_on_screen_plan() {
        let (rules, trunks, [uni, afr, mov, int]) = screen_plan();
        let e = em();
        let r = resolve(&rules, "923447108", &e, &trunks);
        assert_eq!(r.outcome, ResolutionOutcome::Route);
        assert_eq!(r.rule_position, Some(0));
        assert_eq!(r.legs, vec![uni, afr]);
        assert!(r.record);

        let r = resolve(&rules, "222640100", &e, &trunks);
        assert_eq!((r.rule_position, r.legs.clone()), (Some(1), vec![uni, mov]));

        let r = resolve(&rules, "84209", &e, &trunks);
        assert_eq!(r.outcome, ResolutionOutcome::Internal);
        assert_eq!(r.action, Some(RuleAction::RoomPin));

        let r = resolve(&rules, "123", &e, &trunks);
        assert_eq!((r.outcome, r.record), (ResolutionOutcome::Internal, false));

        let r = resolve(&rules, "002711555019", &e, &trunks);
        assert_eq!(r.legs, vec![int]);

        let r = resolve(&rules, "555", &e, &trunks);
        assert_eq!(r.outcome, ResolutionOutcome::NoMatch);
    }

    #[test]
    fn emergency_beats_earlier_rule_and_is_never_recorded() {
        let (rules, trunks, [uni, afr, mov, int]) = screen_plan();
        // «1XX» (ramal, posição 3) casa 112 ANTES da regra de emergência.
        let r = resolve(&rules, "112", &em(), &trunks);
        assert!(r.emergency);
        assert!(!r.record);
        assert_eq!(r.outcome, ResolutionOutcome::Route);
        assert_eq!(r.rule_position, Some(5));
        assert_eq!(r.overridden_rule_position, Some(3));
        // Regra primeiro, depois TODOS os outros activos pela ordem.
        assert_eq!(r.legs, vec![uni, afr, mov, int]);
    }

    #[test]
    fn emergency_without_rule_or_with_trunks_down() {
        let (rules, mut trunks, [_, afr, mov, int]) = screen_plan();
        trunks[0].enabled = false; // Unitel em baixo
        let r = resolve(&rules, "113", &em(), &trunks);
        assert_eq!(r.legs, vec![afr, mov, int]);

        // Plano sem nenhuma regra: a emergência continua a ter caminho.
        let r = resolve(&[], "115", &em(), &trunks);
        assert_eq!(r.outcome, ResolutionOutcome::Route);
        assert_eq!(r.rule_position, None);
        assert!(!r.record);

        // Nenhum tronco activo: diz-se, não se inventa.
        for t in &mut trunks {
            t.enabled = false;
        }
        let r = resolve(&rules, "112", &em(), &trunks);
        assert_eq!(r.outcome, ResolutionOutcome::NoAvailableTrunk);
        assert!(r.emergency);
    }

    #[test]
    fn disabled_primary_falls_back() {
        let (rules, mut trunks, [_, afr, _, _]) = screen_plan();
        trunks[0].enabled = false;
        let r = resolve(&rules, "923447108", &em(), &trunks);
        assert_eq!(r.legs, vec![afr]);
        trunks[1].enabled = false;
        let r = resolve(&rules, "923447108", &em(), &trunks);
        assert_eq!(r.outcome, ResolutionOutcome::NoAvailableTrunk);
    }
}
