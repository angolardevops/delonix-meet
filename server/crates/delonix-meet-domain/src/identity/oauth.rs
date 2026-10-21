//! OAuth 2.1 para clientes MCP (ADR-0028) — as regras, puras.
//!
//! O adaptador (`server/src/oauth.rs`) lê o pedido, chama estas funções e
//! grava; nenhuma destas decisões se repete lá. É a mesma divisão que
//! [`super::api_key`] já faz para as chaves `dlx_`.
//!
//! # Porque é um catálogo próprio de escopos
//!
//! Os sete escopos de [`super::api_key::Scope`] descrevem rotas da v1 e um
//! deles põe bots em salas. Um agente que age por uma pessoa precisa de outro
//! vocabulário — legendas, capítulos, recortes — e de um catálogo que possa
//! crescer sem alargar o que uma chave de integração já pode fazer.
//!
//! Como lá, o catálogo é **fechado** ([`McpScope::ALL`]): não há `*`, não há
//! escopo «tudo», e um escopo acrescentado amanhã **não chega** às concessões
//! que já existem.

use chrono::{DateTime, Duration, Utc};
use delonix_meet_core::{DomainError, ErrorKind};

// ---------- Escopos ----------

/// Um escopo do catálogo MCP. A forma textual é contrato: vai no pedido de
/// autorização, no token e no ecrã de consentimento.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum McpScope {
    MeetRead,
    MinutesRead,
    MinutesWrite,
    RecordingsRead,
    CaptionsWrite,
    ChaptersWrite,
    ClipsWrite,
    ShortsPublish,
    LiveTranslate,
}

impl McpScope {
    /// O catálogo inteiro, pela ordem canónica (a do ecrã de consentimento:
    /// ler antes de escrever, e publicar no fim).
    pub const ALL: [McpScope; 9] = [
        McpScope::MeetRead,
        McpScope::MinutesRead,
        McpScope::RecordingsRead,
        McpScope::MinutesWrite,
        McpScope::CaptionsWrite,
        McpScope::ChaptersWrite,
        McpScope::ClipsWrite,
        McpScope::ShortsPublish,
        McpScope::LiveTranslate,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            McpScope::MeetRead => "meet.read",
            McpScope::MinutesRead => "minutes.read",
            McpScope::MinutesWrite => "minutes.write",
            McpScope::RecordingsRead => "recordings.read",
            McpScope::CaptionsWrite => "captions.write",
            McpScope::ChaptersWrite => "chapters.write",
            McpScope::ClipsWrite => "clips.write",
            McpScope::ShortsPublish => "shorts.publish",
            McpScope::LiveTranslate => "live.translate",
        }
    }

    /// O que este escopo abre, em linguagem de pessoa. É o texto do ecrã de
    /// consentimento: quem consente tem de poder dizer a que disse sim.
    pub fn describes(self) -> &'static str {
        match self {
            McpScope::MeetRead => "Ver as reuniões, salas e participantes a que já tens acesso",
            McpScope::MinutesRead => "Ler a acta e a transcrição dessas reuniões",
            McpScope::MinutesWrite => "Gerar e gravar a acta de uma reunião",
            McpScope::RecordingsRead => {
                "Ver a tua biblioteca de gravações, com legendas e capítulos"
            }
            McpScope::CaptionsWrite => "Gerar e traduzir legendas de uma gravação",
            McpScope::ChaptersWrite => "Gerar e editar capítulos de uma gravação",
            McpScope::ClipsWrite => "Criar recortes e vídeos curtos a partir de uma gravação",
            McpScope::ShortsPublish => "Publicar um vídeo curto num destino da organização",
            McpScope::LiveTranslate => "Ligar e configurar a tradução ao vivo numa sala tua",
        }
    }

    /// Escopos que apenas LEEM. Serve ao ecrã de consentimento (separar o que
    /// olha do que mexe) e ao resumo da concessão.
    pub fn is_read_only(self) -> bool {
        matches!(
            self,
            McpScope::MeetRead | McpScope::MinutesRead | McpScope::RecordingsRead
        )
    }

    pub fn parse(raw: &str) -> Option<McpScope> {
        McpScope::ALL.into_iter().find(|s| s.as_str() == raw)
    }
}

/// Lê o parâmetro `scope` do OAuth (separado por espaços) para a lista
/// canónica: ordenada, sem repetidos.
///
/// Recusa o desconhecido em vez de o ignorar, ao contrário do que o RFC 6749
/// permite: ignorar um escopo que o cliente pediu dá-lhe um token que não faz
/// o que ele acha, e a falha aparece mais tarde como um `403` sem explicação.
pub fn parse_scopes(raw: &str) -> Result<Vec<McpScope>, DomainError> {
    let mut out = Vec::new();
    for part in raw.split_whitespace() {
        match McpScope::parse(part) {
            Some(s) => out.push(s),
            None => {
                return Err(DomainError::invalid(
                    "oauth.invalid_scope",
                    format!("escopo desconhecido: {part}"),
                )
                .with_field("scope", catalogue_description()))
            }
        }
    }
    if out.is_empty() {
        return Err(DomainError::invalid(
            "oauth.invalid_scope",
            "um token sem escopos não serve para nada",
        )
        .with_field("scope", catalogue_description()));
    }
    out.sort();
    out.dedup();
    Ok(out)
}

pub fn catalogue_description() -> String {
    format!(
        "um ou mais de: {}",
        McpScope::ALL
            .iter()
            .map(|s| s.as_str())
            .collect::<Vec<_>>()
            .join(" ")
    )
}

pub fn scopes_to_string(scopes: &[McpScope]) -> String {
    scopes
        .iter()
        .map(|s| s.as_str())
        .collect::<Vec<_>>()
        .join(" ")
}

/// Uma concessão anterior cobre o pedido? Se cobrir, não se volta a pedir
/// consentimento; se o cliente pedir **mais**, pede-se outra vez (ADR-0028 §5).
pub fn grant_covers(granted: &[McpScope], requested: &[McpScope]) -> bool {
    requested.iter().all(|r| granted.contains(r))
}

// ---------- Prazos ----------

/// Validade do código de autorização. Curta de propósito: o RFC 6749 recomenda
/// 10 minutos como MÁXIMO, e um código é de uso único trocado de imediato.
pub const CODE_TTL_SECS: i64 = 60;

/// Validade do refresh token. Rotativo, por isso a vida é da cadeia e não de
/// um token: cada uso emite outro e o anterior morre.
pub const REFRESH_TTL_DAYS: i64 = 30;

/// Quantos `redirect_uri` um cliente pode registar. Mais do que isto é um
/// cliente a guardar-se uma porta para depois.
pub const MAX_REDIRECT_URIS: usize = 5;

pub fn code_expires_at(now: DateTime<Utc>) -> DateTime<Utc> {
    now + Duration::seconds(CODE_TTL_SECS)
}

pub fn refresh_expires_at(now: DateTime<Utc>) -> DateTime<Utc> {
    now + Duration::days(REFRESH_TTL_DAYS)
}

// ---------- Cliente ----------

/// Valida o nome que o cliente dá de si. Vai aparecer no ecrã de
/// consentimento e na lista de sessões activas, por isso não pode ser vazio,
/// nem enorme, nem trazer controlo — um nome com `\n` desenha o que quiser no
/// ecrã de quem consente.
pub fn validate_client_name(raw: &str) -> Result<String, DomainError> {
    let name = raw.trim();
    if name.is_empty() {
        return Err(DomainError::invalid(
            "oauth.invalid_client_metadata",
            "o cliente tem de ter nome",
        )
        .with_field("client_name", "1 a 120 caracteres"));
    }
    if name.chars().count() > 120 {
        return Err(DomainError::invalid(
            "oauth.invalid_client_metadata",
            "nome de cliente demasiado longo",
        )
        .with_field("client_name", "1 a 120 caracteres"));
    }
    if name.chars().any(|c| c.is_control()) {
        return Err(DomainError::invalid(
            "oauth.invalid_client_metadata",
            "o nome do cliente não pode ter caracteres de controlo",
        )
        .with_field("client_name", "sem caracteres de controlo"));
    }
    Ok(name.to_string())
}

/// Um `redirect_uri` aceitável para um cliente público (ADR-0028 §3).
///
/// `https://` em qualquer host, ou **loopback** em claro — que é como um
/// cliente de secretária recebe o retorno (RFC 8252 §7.3). Mais nada:
/// `http://` noutro host entrega o código em claro na rede, e um esquema
/// próprio (`app://`) não é verificável por nós.
///
/// Sem `*`, sem fragmento, e sem `localhost` **por nome** — o nome resolve-se
/// onde o cliente estiver, o número não.
pub fn validate_redirect_uri(raw: &str) -> Result<String, DomainError> {
    let bad = |detalhe: &str| {
        DomainError::invalid(
            "oauth.invalid_redirect_uri",
            format!("redirect_uri inválido: {detalhe}"),
        )
        .with_field(
            "redirect_uris",
            "https://… ou http://127.0.0.1:porta/… (loopback)",
        )
    };
    let url = url::Url::parse(raw).map_err(|_| bad("não é um URL absoluto"))?;
    if url.fragment().is_some() {
        return Err(bad("não pode ter fragmento"));
    }
    if raw.contains('*') {
        return Err(bad("não pode ter curinga"));
    }
    match url.scheme() {
        "https" => {
            if url.host_str().is_none_or(str::is_empty) {
                return Err(bad("falta o host"));
            }
        }
        "http" => {
            let host = url.host_str().unwrap_or_default();
            if !matches!(host, "127.0.0.1" | "::1" | "[::1]") {
                return Err(bad(
                    "em claro só é aceite loopback por endereço (127.0.0.1 ou [::1])",
                ));
            }
        }
        outro => return Err(bad(&format!("esquema «{outro}» não é aceite"))),
    }
    Ok(url.to_string())
}

pub fn validate_redirect_uris(raw: &[String]) -> Result<Vec<String>, DomainError> {
    if raw.is_empty() {
        return Err(DomainError::invalid(
            "oauth.invalid_redirect_uri",
            "o cliente tem de registar pelo menos um redirect_uri",
        )
        .with_field("redirect_uris", "1 a 5 endereços"));
    }
    if raw.len() > MAX_REDIRECT_URIS {
        return Err(DomainError::invalid(
            "oauth.invalid_redirect_uri",
            format!("no máximo {MAX_REDIRECT_URIS} endereços de retorno"),
        )
        .with_field("redirect_uris", "1 a 5 endereços"));
    }
    raw.iter().map(|u| validate_redirect_uri(u)).collect()
}

/// O `redirect_uri` do pedido tem de ser **exactamente** um dos registados.
/// Comparação literal, não por prefixo: «começa por» é como um redirector
/// aberto nasce.
pub fn ensure_redirect_registered(registered: &[String], asked: &str) -> Result<(), DomainError> {
    if registered.iter().any(|r| r == asked) {
        return Ok(());
    }
    Err(DomainError::invalid(
        "oauth.invalid_redirect_uri",
        "o redirect_uri não está registado neste cliente",
    )
    .with_field("redirect_uri", "um dos endereços registados, literalmente"))
}

// ---------- Pedido de autorização ----------

pub fn ensure_response_type(raw: &str) -> Result<(), DomainError> {
    if raw == "code" {
        return Ok(());
    }
    Err(
        DomainError::invalid("oauth.unsupported_response_type", "só `code` é suportado")
            .with_field("response_type", "code"),
    )
}

/// Só `S256`. O `plain` do RFC 7636 fica fora: o OAuth 2.1 não o permite num
/// cliente público, e com ele o verifier viaja em claro no primeiro pedido.
pub fn ensure_code_challenge_method(raw: &str) -> Result<(), DomainError> {
    if raw == "S256" {
        return Ok(());
    }
    Err(
        DomainError::invalid("oauth.invalid_request", "só o método PKCE `S256` é aceite")
            .with_field("code_challenge_method", "S256"),
    )
}

/// O `code_challenge` tem a forma de base64url sem padding de 32 bytes: 43
/// caracteres do alfabeto. Validar a forma aqui poupa um código guardado que
/// nunca poderia casar com nada.
pub fn validate_code_challenge(raw: &str) -> Result<(), DomainError> {
    let ok = raw.len() == 43
        && raw
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_');
    if ok {
        return Ok(());
    }
    Err(DomainError::invalid(
        "oauth.invalid_request",
        "code_challenge não é base64url de 32 bytes",
    )
    .with_field("code_challenge", "43 caracteres base64url sem padding"))
}

/// O `resource` (RFC 8707) tem de ser o identificador canónico deste recurso.
/// É isto que faz o token ter público: sem esta verificação, um token emitido
/// para o MCP valia para o que mais aparecesse.
pub fn ensure_resource(asked: &str, canonical: &str) -> Result<(), DomainError> {
    // Comparação tolerante à barra final — a mesma URL com e sem `/` é o mesmo
    // recurso, e discutir isso com um cliente é tempo perdido.
    if asked.trim_end_matches('/') == canonical.trim_end_matches('/') {
        return Ok(());
    }
    Err(DomainError::new(
        ErrorKind::InvalidArgument,
        "oauth.invalid_target",
        format!("este servidor só autoriza o recurso {canonical}"),
    )
    .with_field("resource", canonical.to_string()))
}

// ---------- Troca do código ----------

pub fn ensure_grant_type(raw: &str) -> Result<(), DomainError> {
    if matches!(raw, "authorization_code" | "refresh_token") {
        return Ok(());
    }
    Err(DomainError::invalid(
        "oauth.unsupported_grant_type",
        "só `authorization_code` e `refresh_token` são suportados",
    )
    .with_field("grant_type", "authorization_code | refresh_token"))
}

/// Estado em que um código de autorização foi encontrado.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodeState {
    Usable,
    Expired,
    AlreadyUsed,
}

pub fn code_state(
    expires_at: DateTime<Utc>,
    consumed_at: Option<DateTime<Utc>>,
    now: DateTime<Utc>,
) -> CodeState {
    if consumed_at.is_some() {
        return CodeState::AlreadyUsed;
    }
    if expires_at <= now {
        return CodeState::Expired;
    }
    CodeState::Usable
}

/// A recusa é sempre a mesma, aconteça o que for: código desconhecido,
/// expirado, já gasto, de outro cliente ou com outro `redirect_uri`.
///
/// Distinguir ajudaria quem está a adivinhar — e a quem integra de boa fé não
/// faz falta, porque o erro é sempre o mesmo problema: trocar o código outra
/// vez não vai funcionar.
pub fn invalid_grant() -> DomainError {
    DomainError::new(
        ErrorKind::InvalidArgument,
        "oauth.invalid_grant",
        "código ou refresh token inválido, expirado ou já usado",
    )
}

/// Um refresh token **já usado** apresentado outra vez não é um engano: é a
/// assinatura de um token roubado, porque o cliente legítimo recebeu o
/// substituto. A resposta é revogar a cadeia inteira (RFC 9700 §4.14.2).
pub fn refresh_reuse_is_theft(used_at: Option<DateTime<Utc>>) -> bool {
    used_at.is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalogo_fechado_sem_curinga() {
        assert_eq!(McpScope::ALL.len(), 9);
        assert!(McpScope::parse("*").is_none());
        assert!(
            McpScope::parse("rooms:read").is_none(),
            "não é o catálogo dlx_"
        );
        for s in McpScope::ALL {
            assert_eq!(McpScope::parse(s.as_str()), Some(s));
            assert!(
                !s.describes().is_empty(),
                "{} sem texto de consentimento",
                s.as_str()
            );
        }
    }

    #[test]
    fn escopos_lidos_sao_canonicos_e_o_desconhecido_recusa() {
        let s = parse_scopes("recordings.read meet.read recordings.read").unwrap();
        assert_eq!(s, vec![McpScope::MeetRead, McpScope::RecordingsRead]);
        assert_eq!(
            parse_scopes("meet.read inventado").unwrap_err().code,
            "oauth.invalid_scope"
        );
        assert_eq!(parse_scopes("   ").unwrap_err().code, "oauth.invalid_scope");
    }

    #[test]
    fn concessao_cobre_igual_ou_menos_nunca_mais() {
        let dado = vec![McpScope::MeetRead, McpScope::RecordingsRead];
        assert!(grant_covers(&dado, &[McpScope::MeetRead]));
        assert!(grant_covers(&dado, &dado));
        assert!(!grant_covers(&dado, &[McpScope::ClipsWrite]));
    }

    #[test]
    fn redirect_aceita_https_e_loopback_e_recusa_o_resto() {
        assert!(validate_redirect_uri("https://claude.ai/api/mcp/callback").is_ok());
        assert!(validate_redirect_uri("http://127.0.0.1:33418/callback").is_ok());
        assert!(validate_redirect_uri("http://[::1]:7777/cb").is_ok());
        // Em claro fora de loopback entrega o código na rede.
        assert!(validate_redirect_uri("http://exemplo.ao/cb").is_err());
        // `localhost` por nome resolve-se onde o cliente estiver.
        assert!(validate_redirect_uri("http://localhost:7777/cb").is_err());
        assert!(validate_redirect_uri("app://retorno").is_err());
        assert!(validate_redirect_uri("https://exemplo.ao/cb#frag").is_err());
        assert!(validate_redirect_uri("https://*.exemplo.ao/cb").is_err());
        assert!(validate_redirect_uri("/relativo").is_err());
    }

    #[test]
    fn redirect_compara_literalmente_nao_por_prefixo() {
        let reg = vec!["https://claude.ai/cb".to_string()];
        assert!(ensure_redirect_registered(&reg, "https://claude.ai/cb").is_ok());
        // «começa por» é como um redirector aberto nasce.
        assert!(ensure_redirect_registered(&reg, "https://claude.ai/cb/../x").is_err());
        assert!(ensure_redirect_registered(&reg, "https://claude.ai/cb?x=1").is_err());
    }

    #[test]
    fn so_s256_e_a_forma_do_challenge() {
        assert!(ensure_code_challenge_method("S256").is_ok());
        assert!(ensure_code_challenge_method("plain").is_err());
        assert!(validate_code_challenge("E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM").is_ok());
        assert!(validate_code_challenge("curto").is_err());
        assert!(validate_code_challenge(&"A".repeat(43)).is_ok());
        assert!(validate_code_challenge(&"A=".repeat(22)).is_err());
    }

    #[test]
    fn recurso_tem_de_ser_o_canonico() {
        let canon = "https://meet.ngolacloud.com/api/mcp";
        assert!(ensure_resource(canon, canon).is_ok());
        assert!(ensure_resource("https://meet.ngolacloud.com/api/mcp/", canon).is_ok());
        assert_eq!(
            ensure_resource("https://outro.ao/api/mcp", canon)
                .unwrap_err()
                .code,
            "oauth.invalid_target"
        );
    }

    #[test]
    fn estado_do_codigo() {
        let now = Utc::now();
        assert_eq!(
            code_state(now + Duration::seconds(30), None, now),
            CodeState::Usable
        );
        assert_eq!(
            code_state(now - Duration::seconds(1), None, now),
            CodeState::Expired
        );
        assert_eq!(
            code_state(now + Duration::seconds(30), Some(now), now),
            CodeState::AlreadyUsed
        );
        assert!(refresh_reuse_is_theft(Some(now)));
        assert!(!refresh_reuse_is_theft(None));
    }

    #[test]
    fn nome_de_cliente_sem_controlo() {
        assert_eq!(
            validate_client_name("  Claude Desktop ").unwrap(),
            "Claude Desktop"
        );
        assert!(validate_client_name("").is_err());
        assert!(validate_client_name("a\nb").is_err());
        assert!(validate_client_name(&"x".repeat(121)).is_err());
    }
}
