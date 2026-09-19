//! Chave de API `dlx_` — escopos, expiração e registo de uso (ADR-0004 §4,
//! auditoria S6).
//!
//! Antes disto uma chave `dlx_` era um cheque em branco: servia em TODAS as
//! rotas da v1, para sempre. Uma chave emitida para ler o calendário também
//! apagava reuniões e punha bots em salas.
//!
//! As regras aqui são puras: o adaptador lê a linha da chave e pergunta.
//!
//! # Catálogo fixo
//!
//! Os escopos são um catálogo FECHADO (`Scope::ALL`). Não há `*` nem escopo
//! «tudo»: uma chave guarda a lista explícita que recebeu, e um escopo novo
//! acrescentado amanhã ao catálogo NÃO chega às chaves que já existem.
//!
//! # Compatibilidade (decisão)
//!
//! - **Chaves anteriores à migração 0046** recebem a lista completa do
//!   catálogo de hoje — continuam a fazer exactamente o que faziam.
//! - **Chave criada sem `scopes`** (BFF ou provisionamento) recebe também a
//!   lista completa. O cliente web e o módulo Odoo `nk_delonix_meet` não
//!   enviam `scopes`; mudar o omisso para «só leitura» dentro da v1 partia as
//!   integrações que se criam hoje — e a v1 só quebra com v2. Quem quer
//!   menos privilégio pede-o explicitamente, e a lista vazia é recusada.

use chrono::{DateTime, Duration, Utc};
use delonix_meet_core::{DomainError, ErrorKind};

/// Um escopo do catálogo. A forma textual (`recurso:acção`) é contrato.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Scope {
    /// `GET /api/v1/org`.
    OrgRead,
    /// `GET /api/v1/rooms/{code}`.
    RoomsRead,
    /// `POST /api/v1/rooms`.
    RoomsWrite,
    /// `POST /api/v1/rooms/{code}/join-bot`.
    BotsJoin,
    /// `GET /api/v1/meetings`, `GET /api/v1/meetings/{id}/notes`.
    MeetingsRead,
    /// `POST /api/v1/meetings`, `PATCH`/`DELETE /api/v1/meetings/{id}`,
    /// `POST /api/v1/meetings/{id}/ring`.
    MeetingsWrite,
    /// `GET /api/v1/recordings`.
    RecordingsRead,
}

impl Scope {
    /// O catálogo inteiro, pela ordem canónica.
    pub const ALL: [Scope; 7] = [
        Scope::OrgRead,
        Scope::RoomsRead,
        Scope::RoomsWrite,
        Scope::BotsJoin,
        Scope::MeetingsRead,
        Scope::MeetingsWrite,
        Scope::RecordingsRead,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Scope::OrgRead => "org:read",
            Scope::RoomsRead => "rooms:read",
            Scope::RoomsWrite => "rooms:write",
            Scope::BotsJoin => "bots:join",
            Scope::MeetingsRead => "meetings:read",
            Scope::MeetingsWrite => "meetings:write",
            Scope::RecordingsRead => "recordings:read",
        }
    }

    pub fn parse(raw: &str) -> Option<Scope> {
        Scope::ALL.into_iter().find(|s| s.as_str() == raw)
    }

    /// O catálogo inteiro como texto — o que uma chave sem `scopes` recebe.
    pub fn all_strings() -> Vec<String> {
        Scope::ALL.iter().map(|s| s.as_str().to_string()).collect()
    }
}

/// Validade máxima de uma chave com `expires_at`.
pub const MAX_LIFETIME_DAYS: i64 = 730;

/// Intervalo mínimo entre duas escritas de `last_used_at` da mesma chave.
/// Sem isto, cada pedido da v1 era um `UPDATE` — o registo de uso custava
/// tanto como o próprio pedido.
pub const LAST_USED_THROTTLE_SECS: i64 = 60;

/// Valida os escopos pedidos na criação. `None` ⇒ catálogo inteiro (ver a
/// decisão no cabeçalho). Devolve a lista canónica, sem repetidos e ordenada.
pub fn scopes_for_new_key(requested: Option<&[String]>) -> Result<Vec<Scope>, DomainError> {
    let Some(raw) = requested else {
        return Ok(Scope::ALL.to_vec());
    };
    if raw.is_empty() {
        return Err(DomainError::invalid(
            "api_key.scopes_empty",
            "uma chave sem escopos não serve para nada",
        )
        .with_field("scopes", "pelo menos um escopo do catálogo"));
    }
    let mut out = Vec::with_capacity(raw.len());
    for s in raw {
        match Scope::parse(s.trim()) {
            Some(scope) => out.push(scope),
            None => {
                return Err(DomainError::invalid(
                    "api_key.unknown_scope",
                    format!("escopo desconhecido: {s}"),
                )
                .with_field("scopes", catalogue_description()))
            }
        }
    }
    out.sort();
    out.dedup();
    Ok(out)
}

/// Valida o `expires_at` pedido na criação: no futuro e a no máximo dois anos.
pub fn validate_expiry(
    expires_at: Option<DateTime<Utc>>,
    now: DateTime<Utc>,
) -> Result<(), DomainError> {
    let Some(at) = expires_at else {
        return Ok(());
    };
    if at <= now {
        return Err(DomainError::invalid(
            "api_key.expiry_in_past",
            "expires_at tem de estar no futuro",
        )
        .with_field("expires_at", "instante futuro, RFC 3339"));
    }
    if at > now + Duration::days(MAX_LIFETIME_DAYS) {
        return Err(DomainError::invalid(
            "api_key.expiry_too_far",
            "expires_at não pode passar de dois anos",
        )
        .with_field("expires_at", format!("no máximo {MAX_LIFETIME_DAYS} dias")));
    }
    Ok(())
}

/// Os escopos guardados numa chave. Texto desconhecido (um escopo retirado
/// do catálogo) é ignorado: falha fechado, nunca concede.
pub fn granted_from_stored(stored: &[String]) -> Vec<Scope> {
    stored.iter().filter_map(|s| Scope::parse(s)).collect()
}

/// A chave ainda serve? Uma chave expirada é `401 api_key.expired` — distinto
/// da chave desconhecida, para quem integra saber que tem de a rodar.
pub fn ensure_not_expired(
    expires_at: Option<DateTime<Utc>>,
    now: DateTime<Utc>,
) -> Result<(), DomainError> {
    match expires_at {
        Some(at) if at <= now => Err(DomainError::new(
            ErrorKind::Unauthenticated,
            "api_key.expired",
            "a chave de API expirou",
        )
        .with_field("expires_at", at.to_rfc3339())),
        _ => Ok(()),
    }
}

/// A chave tem o escopo que a rota exige? `403 api_key.scope_missing`, com o
/// escopo em falta em `details` — é a única coisa que quem integra precisa
/// para pedir a chave certa.
pub fn require_scope(granted: &[Scope], needed: Scope) -> Result<(), DomainError> {
    if granted.contains(&needed) {
        return Ok(());
    }
    Err(DomainError::new(
        ErrorKind::PermissionDenied,
        "api_key.scope_missing",
        format!("a chave de API não tem o escopo {}", needed.as_str()),
    )
    .with_field("scope", needed.as_str()))
}

/// Deve registar-se o uso agora? Só se nunca foi registado ou se o último
/// registo tem mais de `LAST_USED_THROTTLE_SECS`.
pub fn should_record_use(last_used_at: Option<DateTime<Utc>>, now: DateTime<Utc>) -> bool {
    match last_used_at {
        None => true,
        Some(at) => now - at >= Duration::seconds(LAST_USED_THROTTLE_SECS),
    }
}

fn catalogue_description() -> String {
    format!("um de: {}", Scope::all_strings().join(", "))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strs(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn catalogo_ida_e_volta() {
        for s in Scope::ALL {
            assert_eq!(Scope::parse(s.as_str()), Some(s));
        }
        assert_eq!(Scope::parse("*"), None);
        assert_eq!(Scope::parse("MEETINGS:READ"), None);
    }

    #[test]
    fn omisso_da_o_catalogo_inteiro_e_vazio_e_recusado() {
        assert_eq!(scopes_for_new_key(None).unwrap(), Scope::ALL.to_vec());
        let e = scopes_for_new_key(Some(&[])).unwrap_err();
        assert_eq!(e.code, "api_key.scopes_empty");
        assert_eq!(e.kind, ErrorKind::InvalidArgument);
    }

    #[test]
    fn escopos_pedidos_sao_canonicos_e_desconhecidos_recusados() {
        let got = scopes_for_new_key(Some(&strs(&[
            "meetings:write",
            " org:read ",
            "meetings:write",
        ])))
        .unwrap();
        assert_eq!(got, vec![Scope::OrgRead, Scope::MeetingsWrite]);
        let e = scopes_for_new_key(Some(&strs(&["org:read", "admin:all"]))).unwrap_err();
        assert_eq!(e.code, "api_key.unknown_scope");
        assert!(e.details[0].description.contains("meetings:read"));
    }

    #[test]
    fn expiracao_futura_e_ate_dois_anos() {
        let now = Utc::now();
        assert!(validate_expiry(None, now).is_ok());
        assert!(validate_expiry(Some(now + Duration::days(30)), now).is_ok());
        assert!(validate_expiry(Some(now + Duration::days(MAX_LIFETIME_DAYS)), now).is_ok());
        assert_eq!(
            validate_expiry(Some(now - Duration::seconds(1)), now)
                .unwrap_err()
                .code,
            "api_key.expiry_in_past"
        );
        assert_eq!(
            validate_expiry(Some(now + Duration::days(MAX_LIFETIME_DAYS + 1)), now)
                .unwrap_err()
                .code,
            "api_key.expiry_too_far"
        );
    }

    #[test]
    fn chave_expirada_e_401_com_codigo_proprio() {
        let now = Utc::now();
        assert!(ensure_not_expired(None, now).is_ok());
        assert!(ensure_not_expired(Some(now + Duration::minutes(1)), now).is_ok());
        let e = ensure_not_expired(Some(now), now).unwrap_err();
        assert_eq!(e.code, "api_key.expired");
        assert_eq!(e.kind, ErrorKind::Unauthenticated);
    }

    #[test]
    fn escopo_em_falta_diz_qual() {
        let granted = vec![Scope::MeetingsRead];
        assert!(require_scope(&granted, Scope::MeetingsRead).is_ok());
        let e = require_scope(&granted, Scope::MeetingsWrite).unwrap_err();
        assert_eq!(e.code, "api_key.scope_missing");
        assert_eq!(e.kind, ErrorKind::PermissionDenied);
        assert_eq!(e.details[0].field, "scope");
        assert_eq!(e.details[0].description, "meetings:write");
    }

    #[test]
    fn escopo_guardado_desconhecido_nao_concede() {
        assert_eq!(
            granted_from_stored(&strs(&["meetings:read", "retirado:all"])),
            vec![Scope::MeetingsRead]
        );
    }

    #[test]
    fn registo_de_uso_no_maximo_um_por_minuto() {
        let now = Utc::now();
        assert!(should_record_use(None, now));
        assert!(!should_record_use(Some(now - Duration::seconds(59)), now));
        assert!(should_record_use(Some(now - Duration::seconds(60)), now));
    }
}
