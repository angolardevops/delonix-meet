//! Servidor de autorização OAuth 2.1 para clientes MCP (ADR-0028) — adaptador
//! HTTP + Postgres.
//!
//! As regras estão em `delonix_meet_domain::identity::oauth`; a travessia das
//! tabelas em `delonix_meet_store::identity::oauth`. Aqui só se lê o pedido,
//! se chamam as duas, e se escreve a resposta.
//!
//! **A decisão que explica a forma deste módulo** (ADR-0028 §4): a autorização
//! de um agente abre uma linha em `user_sessions`, com `auth_method = 'oauth'`.
//! Não há segunda vida de credenciais — o agente aparece em «A minha conta →
//! Sessões activas», e `sessions::ensure_active` mata-o no mesmo pedido em que
//! a pessoa termina a sessão.
//!
//! **Quem é público e porquê** (ver `scripts/rotas-publicas.txt`):
//! - os dois documentos de metadata descrevem endpoints, não dados;
//! - `register` é RFC 7591: o cliente ainda não existe;
//! - `token` e `revoke` autenticam-se pelo código mais o PKCE, não por uma
//!   sessão nossa;
//! - `authorize` é anterior a tudo: valida e manda o browser para o ecrã de
//!   consentimento, que **exige sessão**.
//!
//! O que NÃO está aqui: a decisão da pessoa. Essa é
//! `POST /api/oauth/authorizations`, com `AuthUser`, e é a única rota deste
//! módulo que emite um código.

use std::sync::Arc;

use axum::{
    extract::{Query, State},
    response::{IntoResponse, Redirect, Response},
    Json,
};
use chrono::Utc;
use delonix_meet_core::{
    crypto::{pkce_s256_matches, prefixed_token, sha256_hex},
    DomainError,
};
use delonix_meet_domain::identity::oauth as rules;
use delonix_meet_store::identity::oauth as store;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{auth::AuthUser, error::ApiError, AppState};

/// O identificador canónico do recurso que este servidor protege (RFC 8707).
/// Sai do `PUBLIC_URL`: sem ele não há nome para pôr no `aud`, e por isso o
/// OAuth fica **fechado** em vez de emitir tokens sem público — a mesma
/// decisão que o correio toma com os links (`mail.public_url_missing`).
pub(crate) fn canonical_resource(state: &AppState) -> Result<String, ApiError> {
    let base = state.config.public_url.as_deref().ok_or_else(|| {
        ApiError::ServiceUnavailable(
            "OAuth desligado: sem PUBLIC_URL não há identificador de recurso para o token".into(),
        )
    })?;
    Ok(format!("{base}/api/mcp"))
}

fn issuer(state: &AppState) -> Result<String, ApiError> {
    state
        .config
        .public_url
        .clone()
        .ok_or_else(|| ApiError::ServiceUnavailable("OAuth desligado: sem PUBLIC_URL".into()))
}

// ---------- 1. Metadata ----------

#[derive(Serialize, utoipa::ToSchema)]
pub struct AsMetadata {
    issuer: String,
    authorization_endpoint: String,
    token_endpoint: String,
    registration_endpoint: Option<String>,
    revocation_endpoint: String,
    scopes_supported: Vec<String>,
    response_types_supported: Vec<String>,
    grant_types_supported: Vec<String>,
    code_challenge_methods_supported: Vec<String>,
    token_endpoint_auth_methods_supported: Vec<String>,
}

/// RFC 8414. O cliente MCP lê isto para saber onde autorizar.
#[utoipa::path(
    get, path = "/.well-known/oauth-authorization-server", tag = "oauth",
    responses(
        (status = 200, body = AsMetadata, description = "Metadata do servidor de autorização (RFC 8414)."),
        (status = 503, body = crate::openapi::ErrorBody, description = "Sem PUBLIC_URL o OAuth está fechado."),
    )
)]
pub async fn as_metadata(State(state): State<Arc<AppState>>) -> Result<Response, ApiError> {
    let iss = issuer(&state)?;
    Ok(Json(AsMetadata {
        authorization_endpoint: format!("{iss}/api/oauth/authorize"),
        token_endpoint: format!("{iss}/api/oauth/token"),
        registration_endpoint: state
            .config
            .oauth_dynamic_registration
            .then(|| format!("{iss}/api/oauth/register")),
        revocation_endpoint: format!("{iss}/api/oauth/revoke"),
        issuer: iss,
        scopes_supported: rules::McpScope::ALL
            .iter()
            .map(|s| s.as_str().to_string())
            .collect(),
        response_types_supported: vec!["code".into()],
        grant_types_supported: vec!["authorization_code".into(), "refresh_token".into()],
        code_challenge_methods_supported: vec!["S256".into()],
        // Cliente público: quem prova é o PKCE (ADR-0028 §3).
        token_endpoint_auth_methods_supported: vec!["none".into()],
    })
    .into_response())
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct ResourceMetadata {
    resource: String,
    authorization_servers: Vec<String>,
    scopes_supported: Vec<String>,
    bearer_methods_supported: Vec<String>,
}

/// RFC 9728, exigido pela especificação MCP: diz ao cliente QUEM autoriza
/// este recurso. É o primeiro pedido de um cliente que recebeu `401`.
#[utoipa::path(
    get, path = "/.well-known/oauth-protected-resource", tag = "oauth",
    responses(
        (status = 200, body = ResourceMetadata, description = "Metadata do recurso protegido (RFC 9728)."),
        (status = 503, body = crate::openapi::ErrorBody),
    )
)]
pub async fn resource_metadata(State(state): State<Arc<AppState>>) -> Result<Response, ApiError> {
    let iss = issuer(&state)?;
    Ok(Json(ResourceMetadata {
        resource: canonical_resource(&state)?,
        authorization_servers: vec![iss],
        scopes_supported: rules::McpScope::ALL
            .iter()
            .map(|s| s.as_str().to_string())
            .collect(),
        bearer_methods_supported: vec!["header".into()],
    })
    .into_response())
}

// ---------- 2. Registo dinâmico (RFC 7591) ----------

#[derive(Deserialize, utoipa::ToSchema)]
pub struct RegisterReq {
    pub client_name: String,
    pub redirect_uris: Vec<String>,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct RegisterResp {
    pub client_id: String,
    pub client_name: String,
    pub redirect_uris: Vec<String>,
    /// Fixo em `none`: não emitimos segredo, e um cliente que espere um
    /// recebe-o explicitamente ausente em vez de o procurar.
    pub token_endpoint_auth_method: String,
    pub grant_types: Vec<String>,
    pub response_types: Vec<String>,
}

#[utoipa::path(
    post, path = "/api/oauth/register", tag = "oauth",
    request_body = RegisterReq,
    responses(
        (status = 201, body = RegisterResp, description = "Cliente registado (RFC 7591)."),
        (status = 400, body = crate::openapi::ErrorBody, description = "redirect_uri ou nome inválidos."),
        (status = 403, body = crate::openapi::ErrorBody, description = "Registo dinâmico desligado."),
    )
)]
pub async fn register(
    State(state): State<Arc<AppState>>,
    Json(req): Json<RegisterReq>,
) -> Result<(axum::http::StatusCode, Json<RegisterResp>), ApiError> {
    if !state.config.oauth_dynamic_registration {
        return Err(DomainError::forbidden("oauth.registration_disabled")
            .with_message("o registo dinâmico de clientes está desligado nesta instalação")
            .into());
    }
    let name = rules::validate_client_name(&req.client_name)?;
    let uris = rules::validate_redirect_uris(&req.redirect_uris)?;
    let client_id = prefixed_token("dlxc_");
    store::insert_client(&state.db, &client_id, &name, &uris, true).await?;
    crate::audit::log(
        &state.db,
        None,
        Uuid::nil(),
        "oauth.client_registered",
        &client_id,
    )
    .await;
    Ok((
        axum::http::StatusCode::CREATED,
        Json(RegisterResp {
            client_id,
            client_name: name,
            redirect_uris: uris,
            token_endpoint_auth_method: "none".into(),
            grant_types: vec!["authorization_code".into(), "refresh_token".into()],
            response_types: vec!["code".into()],
        }),
    ))
}

// ---------- 3. Autorizar: validar e mandar ao ecrã de consentimento ----------

#[derive(Deserialize, Clone, Debug)]
pub struct AuthorizeQuery {
    pub response_type: String,
    pub client_id: String,
    pub redirect_uri: String,
    pub scope: String,
    pub code_challenge: String,
    pub code_challenge_method: String,
    pub resource: String,
    #[serde(default)]
    pub state: Option<String>,
}

/// O que se valida ANTES de mostrar qualquer coisa a quem quer que seja, e
/// que é igual no `GET /authorize` e no `POST /authorizations`: o cliente
/// existe e está activo, o `redirect_uri` é um dos registados (literalmente),
/// o PKCE é `S256` e bem formado, o recurso é o nosso, e os escopos são do
/// catálogo.
async fn validate_request(
    state: &AppState,
    q: &AuthorizeQuery,
) -> Result<(store::Client, Vec<rules::McpScope>), ApiError> {
    rules::ensure_response_type(&q.response_type)?;
    rules::ensure_code_challenge_method(&q.code_challenge_method)?;
    rules::validate_code_challenge(&q.code_challenge)?;
    rules::ensure_resource(&q.resource, &canonical_resource(state)?)?;
    let client = store::find_client(&state.db, &q.client_id)
        .await?
        .filter(store::Client::is_usable)
        .ok_or_else(|| DomainError::invalid("oauth.invalid_client", "cliente desconhecido"))?;
    rules::ensure_redirect_registered(&client.redirect_uris, &q.redirect_uri)?;
    let scopes = rules::parse_scopes(&q.scope)?;
    Ok((client, scopes))
}

/// `GET /api/oauth/authorize` — pública e anterior a qualquer sessão.
///
/// Valida e **redirecciona para a web**, que desenha o ecrã de consentimento
/// com sessão. Não devolve HTML: um servidor de API que começa a desenhar
/// páginas de login é um servidor com duas caras.
///
/// Em erro, a resposta depende de uma coisa só: se já se sabe que o
/// `redirect_uri` é **daquele** cliente. Se sim, devolve-se o erro ao cliente
/// pelo redirect, como o RFC manda; se não, devolve-se o erro a quem pediu —
/// mandar um código de erro para um endereço não verificado é um redirector
/// aberto com a nossa assinatura.
#[utoipa::path(
    get, path = "/api/oauth/authorize", tag = "oauth",
    params(
        ("response_type" = String, Query, description = "Só `code`."),
        ("client_id" = String, Query, description = "Cliente registado."),
        ("redirect_uri" = String, Query, description = "Um dos registados, literalmente."),
        ("scope" = String, Query, description = "Escopos do catálogo MCP, separados por espaços."),
        ("code_challenge" = String, Query, description = "PKCE S256, base64url de 32 bytes."),
        ("code_challenge_method" = String, Query, description = "Só `S256`."),
        ("resource" = String, Query, description = "RFC 8707: o identificador canónico do recurso."),
        ("state" = Option<String>, Query, description = "Devolvido ao cliente tal e qual."),
    ),
    responses(
        (status = 303, description = "Redirecciona para o ecrã de consentimento (ou devolve o erro ao cliente)."),
        (status = 400, body = crate::openapi::ErrorBody, description = "Pedido inválido e redirect_uri não provado."),
    )
)]
pub async fn authorize(
    State(state): State<Arc<AppState>>,
    Query(q): Query<AuthorizeQuery>,
) -> Result<Response, ApiError> {
    let base = issuer(&state)?;
    match validate_request(&state, &q).await {
        Ok(_) => {
            // A web encaminha por hash (`#/…`, ver `web/src/rota.ts`), por
            // isso o destino monta-se à mão: `Url::query_pairs_mut` punha a
            // query ANTES do fragmento e o ecrã abria sem parâmetros.
            let mut qs = url::form_urlencoded::Serializer::new(String::new());
            qs.append_pair("response_type", &q.response_type)
                .append_pair("client_id", &q.client_id)
                .append_pair("redirect_uri", &q.redirect_uri)
                .append_pair("scope", &q.scope)
                .append_pair("code_challenge", &q.code_challenge)
                .append_pair("code_challenge_method", &q.code_challenge_method)
                .append_pair("resource", &q.resource);
            if let Some(s) = &q.state {
                qs.append_pair("state", s);
            }
            Ok(Redirect::to(&format!("{base}/#/oauth/consent?{}", qs.finish())).into_response())
        }
        Err(e) => {
            // O redirect só se usa se o endereço estiver PROVADO como daquele
            // cliente. Qualquer outra falha responde aqui.
            let provado = match store::find_client(&state.db, &q.client_id).await? {
                Some(c) if c.is_usable() => {
                    rules::ensure_redirect_registered(&c.redirect_uris, &q.redirect_uri).is_ok()
                }
                _ => false,
            };
            if !provado {
                return Err(e);
            }
            let codigo = match &e {
                ApiError::Domain(d) => d.code.strip_prefix("oauth.").unwrap_or("invalid_request"),
                _ => "invalid_request",
            };
            let mut url = url::Url::parse(&q.redirect_uri)
                .map_err(|_| DomainError::invalid("oauth.invalid_redirect_uri", "ilegível"))?;
            {
                let mut qp = url.query_pairs_mut();
                qp.append_pair("error", codigo);
                if let Some(s) = &q.state {
                    qp.append_pair("state", s);
                }
            }
            Ok(Redirect::to(url.as_str()).into_response())
        }
    }
}

// ---------- 4. O ecrã de consentimento: o que mostrar, e a decisão ----------

#[derive(Serialize, utoipa::ToSchema)]
pub struct ScopeInfo {
    pub scope: String,
    /// O que este escopo abre, em linguagem de pessoa.
    pub describes: String,
    pub read_only: bool,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct ConsentInfo {
    pub client_name: String,
    /// `true` quando o cliente se registou a si mesmo (RFC 7591) — quem
    /// consente tem direito a saber que ninguém o pré-aprovou.
    pub registered_dynamically: bool,
    pub organization_id: Uuid,
    pub organization_name: String,
    pub scopes: Vec<ScopeInfo>,
    /// A pessoa já tinha consentido estes escopos (ou mais) a este cliente.
    pub already_granted: bool,
}

/// `GET /api/oauth/authorization-info` — o que o ecrã de consentimento mostra.
/// Exige sessão: é informação sobre a organização de quem está a decidir.
#[utoipa::path(
    get, path = "/api/oauth/authorization-info", tag = "oauth",
    security(("session" = [])),
    params(
        ("response_type" = String, Query, description = "Só `code`."),
        ("client_id" = String, Query, description = "Cliente registado."),
        ("redirect_uri" = String, Query, description = "Um dos registados."),
        ("scope" = String, Query, description = "Escopos pedidos."),
        ("code_challenge" = String, Query, description = "PKCE S256."),
        ("code_challenge_method" = String, Query, description = "Só `S256`."),
        ("resource" = String, Query, description = "Identificador canónico do recurso."),
        ("state" = Option<String>, Query, description = "Opaco para nós."),
    ),
    responses(
        (status = 200, body = ConsentInfo, description = "O que o ecrã de consentimento mostra."),
        (status = 400, body = crate::openapi::ErrorBody),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 403, body = crate::openapi::ErrorBody, description = "A conta não pertence a nenhuma organização activa."),
    )
)]
pub async fn authorization_info(
    State(state): State<Arc<AppState>>,
    user: AuthUser,
    Query(q): Query<AuthorizeQuery>,
) -> Result<Json<ConsentInfo>, ApiError> {
    let (client, scopes) = validate_request(&state, &q).await?;
    let (org_id, org_name) = acting_org(&state, user.user_id).await?;
    let granted = store::find_grant(&state.db, user.user_id, &client.client_id, org_id).await?;
    let already = granted
        .as_ref()
        .map(|g| {
            let dados: Vec<rules::McpScope> = g
                .scopes
                .iter()
                .filter_map(|s| rules::McpScope::parse(s))
                .collect();
            rules::grant_covers(&dados, &scopes)
        })
        .unwrap_or(false);
    Ok(Json(ConsentInfo {
        client_name: client.client_name,
        registered_dynamically: client.registered_dynamically,
        organization_id: org_id,
        organization_name: org_name,
        scopes: scopes
            .iter()
            .map(|s| ScopeInfo {
                scope: s.as_str().to_string(),
                describes: s.describes().to_string(),
                read_only: s.is_read_only(),
            })
            .collect(),
        already_granted: already,
    }))
}

/// A organização em que o agente vai agir.
///
/// **Não vem do cliente** (ADR-0027 §4): é a pertença PRINCIPAL de quem
/// consente, lida pelo helper canónico (`org::primary_membership`) e não por
/// uma consulta própria — a pertença decide-se em `org.rs`, e a catraca da
/// arquitectura conta quem a copia.
///
/// Quem não pertence a nenhuma organização activa não pode autorizar um
/// agente: não há âmbito onde ele agisse.
async fn acting_org(state: &AppState, user_id: Uuid) -> Result<(Uuid, String), ApiError> {
    let m = crate::org::primary_membership(state, user_id)
        .await?
        .ok_or_else(|| {
            DomainError::forbidden("oauth.no_organization")
                .with_message("esta conta não pertence a nenhuma organização activa")
        })?;
    Ok((m.org_id, m.org_name))
}

#[derive(Deserialize, utoipa::ToSchema)]
pub struct ConsentDecision {
    #[serde(flatten)]
    pub request: AuthorizeQueryBody,
    /// `false` devolve `access_denied` ao cliente, como o RFC manda.
    pub approve: bool,
}

/// O mesmo que [`AuthorizeQuery`], em corpo JSON. Duplicado como tipo e não
/// como validação: a validação é uma só ([`validate_request`]).
#[derive(Deserialize, utoipa::ToSchema, Clone, Debug)]
pub struct AuthorizeQueryBody {
    pub response_type: String,
    pub client_id: String,
    pub redirect_uri: String,
    pub scope: String,
    pub code_challenge: String,
    pub code_challenge_method: String,
    pub resource: String,
    #[serde(default)]
    pub state: Option<String>,
}

impl From<AuthorizeQueryBody> for AuthorizeQuery {
    fn from(b: AuthorizeQueryBody) -> Self {
        AuthorizeQuery {
            response_type: b.response_type,
            client_id: b.client_id,
            redirect_uri: b.redirect_uri,
            scope: b.scope,
            code_challenge: b.code_challenge,
            code_challenge_method: b.code_challenge_method,
            resource: b.resource,
            state: b.state,
        }
    }
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct ConsentResp {
    /// Onde a web manda o browser a seguir. Leva o código (ou o erro) e o
    /// `state` do cliente.
    pub redirect_to: String,
}

/// `POST /api/oauth/authorizations` — a decisão da pessoa. **A única rota
/// deste módulo que emite um código**, e exige sessão.
#[utoipa::path(
    post, path = "/api/oauth/authorizations", tag = "oauth",
    security(("session" = [])),
    request_body = ConsentDecision,
    responses(
        (status = 200, body = ConsentResp, description = "Para onde a web manda o browser (com o código ou com o erro)."),
        (status = 400, body = crate::openapi::ErrorBody),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 403, body = crate::openapi::ErrorBody),
    )
)]
pub async fn decide(
    State(state): State<Arc<AppState>>,
    user: AuthUser,
    Json(body): Json<ConsentDecision>,
) -> Result<Json<ConsentResp>, ApiError> {
    let aprova = body.approve;
    let q: AuthorizeQuery = body.request.into();
    let (client, scopes) = validate_request(&state, &q).await?;
    let mut url = url::Url::parse(&q.redirect_uri)
        .map_err(|_| DomainError::invalid("oauth.invalid_redirect_uri", "ilegível"))?;

    if !aprova {
        {
            let mut qp = url.query_pairs_mut();
            qp.append_pair("error", "access_denied");
            if let Some(s) = &q.state {
                qp.append_pair("state", s);
            }
        }
        return Ok(Json(ConsentResp {
            redirect_to: url.to_string(),
        }));
    }

    let (org_id, _) = acting_org(&state, user.user_id).await?;
    let textos: Vec<String> = scopes.iter().map(|s| s.as_str().to_string()).collect();
    store::upsert_grant(&state.db, user.user_id, &client.client_id, org_id, &textos).await?;

    let code = prefixed_token("dlxac_");
    store::insert_code(
        &state.db,
        &sha256_hex(&code),
        &client.client_id,
        user.user_id,
        org_id,
        &textos,
        &q.redirect_uri,
        &q.code_challenge,
        &q.resource,
        rules::code_expires_at(Utc::now()),
    )
    .await?;
    crate::audit::log(
        &state.db,
        Some(org_id),
        user.user_id,
        "oauth.grant_approved",
        &format!(
            "{} [{}]",
            client.client_id,
            rules::scopes_to_string(&scopes)
        ),
    )
    .await;
    {
        let mut qp = url.query_pairs_mut();
        qp.append_pair("code", &code);
        if let Some(s) = &q.state {
            qp.append_pair("state", s);
        }
    }
    Ok(Json(ConsentResp {
        redirect_to: url.to_string(),
    }))
}

// ---------- 5. O token ----------

/// As claims do token do agente. **Outra forma** da do humano
/// (`crate::auth::Claims`), e é isso que impede um token de MCP de abrir as
/// rotas do BFF: o extractor de lá exige `typ: "access"`, o de cá exige
/// `typ: "mcp"` e o `aud` certo.
#[derive(Serialize, Deserialize, Debug)]
pub(crate) struct McpClaims {
    pub sub: Uuid,
    pub typ: String,
    /// RFC 8707: o recurso para que este token foi emitido.
    pub aud: String,
    pub org: Uuid,
    pub sid: Uuid,
    pub client_id: String,
    pub scope: String,
    pub iat: i64,
    pub exp: i64,
}

#[derive(Deserialize, utoipa::ToSchema)]
pub struct TokenReq {
    pub grant_type: String,
    pub client_id: String,
    #[serde(default)]
    pub code: Option<String>,
    #[serde(default)]
    pub code_verifier: Option<String>,
    #[serde(default)]
    pub redirect_uri: Option<String>,
    #[serde(default)]
    pub refresh_token: Option<String>,
    #[serde(default)]
    pub resource: Option<String>,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct TokenResp {
    pub access_token: String,
    pub token_type: String,
    pub expires_in: i64,
    pub refresh_token: String,
    pub scope: String,
}

/// `POST /api/oauth/token` — pública: a prova é o código mais o PKCE.
///
/// Aceita JSON e `application/x-www-form-urlencoded`, que é o que o RFC manda
/// e o que os clientes MCP enviam.
#[utoipa::path(
    post, path = "/api/oauth/token", tag = "oauth",
    request_body = TokenReq,
    responses(
        (status = 200, body = TokenResp, description = "Par de tokens; o refresh é rotativo."),
        (status = 400, body = crate::openapi::ErrorBody, description = "`oauth.invalid_grant` cobre código desconhecido, expirado, já gasto, de outro cliente ou com PKCE errado."),
        (status = 503, body = crate::openapi::ErrorBody),
    )
)]
pub async fn token(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    body: String,
) -> Result<Json<TokenResp>, ApiError> {
    let req: TokenReq = parse_body(&headers, &body)?;
    rules::ensure_grant_type(&req.grant_type)?;
    let canonical = canonical_resource(&state)?;
    if let Some(r) = &req.resource {
        rules::ensure_resource(r, &canonical)?;
    }
    match req.grant_type.as_str() {
        "authorization_code" => code_to_token(&state, &req, &canonical).await,
        _ => refresh_to_token(&state, &req, &canonical).await,
    }
}

/// Lê um corpo que pode vir em JSON **ou** em `x-www-form-urlencoded`, que é
/// o que o RFC 6749 §4.1.3 manda e o que os clientes MCP enviam.
///
/// O formulário passa por `form_urlencoded` (do `url`, que já é dependência) e
/// não por uma crate de desserialização própria: o corpo do `token` são oito
/// campos de texto, e cada dependência nova numa rota pública é mais
/// superfície do que isto merece.
fn parse_body<T: serde::de::DeserializeOwned>(
    headers: &axum::http::HeaderMap,
    body: &str,
) -> Result<T, ApiError> {
    let json = headers
        .get(axum::http::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|c| c.starts_with("application/json"));
    let valor = if json {
        serde_json::from_str::<serde_json::Value>(body).map_err(|e| {
            ApiError::from(DomainError::invalid(
                "oauth.invalid_request",
                format!("JSON ilegível: {e}"),
            ))
        })?
    } else {
        let mut map = serde_json::Map::new();
        for (k, v) in url::form_urlencoded::parse(body.as_bytes()) {
            map.insert(k.into_owned(), serde_json::Value::String(v.into_owned()));
        }
        serde_json::Value::Object(map)
    };
    serde_json::from_value(valor).map_err(|e| {
        DomainError::invalid("oauth.invalid_request", format!("corpo ilegível: {e}")).into()
    })
}

async fn code_to_token(
    state: &AppState,
    req: &TokenReq,
    canonical: &str,
) -> Result<Json<TokenResp>, ApiError> {
    let code = req.code.as_deref().ok_or_else(rules::invalid_grant)?;
    let verifier = req.code_verifier.as_deref().ok_or_else(|| {
        DomainError::invalid("oauth.invalid_request", "falta o code_verifier (PKCE)")
    })?;
    // Gastar é uma só escrita: dois pedidos com o mesmo código dão um token e
    // um `invalid_grant`.
    let row = store::consume_code(&state.db, &sha256_hex(code))
        .await?
        .ok_or_else(rules::invalid_grant)?;
    // Tudo o que não bate é o MESMO erro. Distinguir só ajudaria a adivinhar.
    if row.client_id != req.client_id
        || req
            .redirect_uri
            .as_deref()
            .is_some_and(|u| u != row.redirect_uri)
        || row.resource != canonical
        || !pkce_s256_matches(&row.code_challenge, verifier)
    {
        return Err(rules::invalid_grant().into());
    }
    emit_pair(
        state,
        row.user_id,
        row.org_id,
        &row.client_id,
        &row.scopes,
        canonical,
        None,
        None,
    )
    .await
}

async fn refresh_to_token(
    state: &AppState,
    req: &TokenReq,
    canonical: &str,
) -> Result<Json<TokenResp>, ApiError> {
    let raw = req
        .refresh_token
        .as_deref()
        .ok_or_else(rules::invalid_grant)?;
    let hash = sha256_hex(raw);
    let row = store::find_refresh(&state.db, &hash)
        .await?
        .ok_or_else(rules::invalid_grant)?;
    // Um token JÁ USADO apresentado outra vez é roubo, não engano: o cliente
    // legítimo recebeu o substituto. Revoga-se a família inteira.
    if rules::refresh_reuse_is_theft(row.used_at) {
        let revogados = store::revoke_family(&state.db, row.family_id).await?;
        crate::audit::log(
            &state.db,
            Some(row.org_id),
            row.user_id,
            "oauth.refresh_reuse_detected",
            &format!("{} ({revogados} tokens revogados)", row.client_id),
        )
        .await;
        return Err(rules::invalid_grant().into());
    }
    if row.client_id != req.client_id || row.resource != canonical {
        return Err(rules::invalid_grant().into());
    }
    // A sessão tem de continuar activa: terminar a sessão mata o agente, e é
    // aqui que isso se nota na renovação.
    crate::sessions::ensure_active(state, row.user_id, row.session_id).await?;
    if !store::mark_refresh_used(&state.db, &hash).await? {
        return Err(rules::invalid_grant().into());
    }
    emit_pair(
        state,
        row.user_id,
        row.org_id,
        &row.client_id,
        &row.scopes,
        canonical,
        Some(row.session_id),
        Some(row.family_id),
    )
    .await
}

/// Emite o par (access + refresh). Sem `session_id` abre uma sessão nova —
/// uma autorização é uma sessão, como um login (ADR-0028 §4).
#[allow(clippy::too_many_arguments)]
async fn emit_pair(
    state: &AppState,
    user_id: Uuid,
    org_id: Uuid,
    client_id: &str,
    scopes: &[String],
    canonical: &str,
    session_id: Option<Uuid>,
    family_id: Option<Uuid>,
) -> Result<Json<TokenResp>, ApiError> {
    let nome = store::find_client(&state.db, client_id)
        .await?
        .map(|c| c.client_name)
        .unwrap_or_else(|| client_id.to_string());
    let sid = match session_id {
        Some(s) => s,
        None => open_agent_session(state, user_id, &nome).await?,
    };
    let now = Utc::now();
    let scope = scopes.join(" ");
    let access = crate::auth::sign_claims(
        &state.config.jwt_secret,
        &McpClaims {
            sub: user_id,
            typ: "mcp".into(),
            aud: canonical.to_string(),
            org: org_id,
            sid,
            client_id: client_id.to_string(),
            scope: scope.clone(),
            iat: now.timestamp(),
            exp: now.timestamp() + state.config.access_ttl_secs,
        },
    )?;
    let refresh = prefixed_token("dlxrt_");
    store::insert_refresh(
        &state.db,
        &sha256_hex(&refresh),
        family_id.unwrap_or_else(Uuid::new_v4),
        sid,
        user_id,
        org_id,
        client_id,
        scopes,
        canonical,
        rules::refresh_expires_at(now),
    )
    .await?;
    Ok(Json(TokenResp {
        access_token: access,
        token_type: "Bearer".into(),
        expires_in: state.config.access_ttl_secs,
        refresh_token: refresh,
        scope,
    }))
}

/// Abre a sessão do agente.
///
/// Chama o helper canónico (`sessions::upsert`) e não um `INSERT` próprio: a
/// sessão é uma só coisa neste sistema, e uma segunda escrita na mesma tabela
/// é a cópia que a catraca da arquitectura persegue. O nome do cliente vai no
/// `user_agent` — é o que a pessoa lê na lista de sessões activas, e sem isso
/// «terminar» seria adivinhar qual é o agente.
async fn open_agent_session(
    state: &AppState,
    user_id: Uuid,
    client_name: &str,
) -> Result<Uuid, ApiError> {
    let id = Uuid::new_v4();
    crate::sessions::upsert(
        state,
        id,
        user_id,
        Some(crate::sessions::AuthMethod::Oauth),
        &format!("Agente MCP — {client_name}"),
        "",
        Utc::now(),
    )
    .await?;
    Ok(id)
}

// ---------- 6. Revogar ----------

#[derive(Deserialize, utoipa::ToSchema)]
pub struct RevokeReq {
    pub token: String,
}

/// `POST /api/oauth/revoke` (RFC 7009). Responde `200` sempre, também a um
/// token que não conhece: dizer «esse não existe» é um oráculo de tokens.
///
/// Só revoga refresh tokens. Um access token de MCP vive 15 minutos e é
/// verificado contra a sessão em cada pedido — quem quer cortar JÁ termina a
/// sessão, que é a porta que existe para isso.
#[utoipa::path(
    post, path = "/api/oauth/revoke", tag = "oauth",
    request_body = RevokeReq,
    responses(
        (status = 200, description = "Revogado — ou nunca existiu; a resposta é a mesma (RFC 7009)."),
        (status = 400, body = crate::openapi::ErrorBody),
    )
)]
pub async fn revoke(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    body: String,
) -> Result<axum::http::StatusCode, ApiError> {
    let req: RevokeReq = parse_body(&headers, &body)?;
    if let Some(row) = store::find_refresh(&state.db, &sha256_hex(&req.token)).await? {
        store::revoke_family(&state.db, row.family_id).await?;
    }
    Ok(axum::http::StatusCode::OK)
}

// ---------- 7. As minhas concessões ----------

#[derive(Serialize, utoipa::ToSchema)]
pub struct GrantRow {
    pub id: Uuid,
    pub client_id: String,
    pub client_name: String,
    pub scopes: Vec<String>,
    pub created_at: chrono::DateTime<Utc>,
    pub updated_at: chrono::DateTime<Utc>,
}

/// `GET /api/users/me/oauth-grants` — a que agentes esta pessoa disse sim.
#[utoipa::path(
    get, path = "/api/users/me/oauth-grants", tag = "users",
    security(("session" = [])),
    responses(
        (status = 200, body = Vec<GrantRow>, description = "Agentes a que esta pessoa disse sim."),
        (status = 401, body = crate::openapi::ErrorBody),
    )
)]
pub async fn my_grants(
    State(state): State<Arc<AppState>>,
    user: AuthUser,
) -> Result<Json<Vec<GrantRow>>, ApiError> {
    let rows = store::list_grants_for_user(&state.db, user.user_id).await?;
    Ok(Json(
        rows.into_iter()
            .map(|g| GrantRow {
                id: g.id,
                client_id: g.client_id,
                client_name: g.client_name,
                scopes: g.scopes,
                created_at: g.created_at,
                updated_at: g.updated_at,
            })
            .collect(),
    ))
}

/// `DELETE /api/users/me/oauth-grants/{grant_id}` — retira o consentimento.
///
/// Revogar não é só apagar a linha: **termina as sessões** que a concessão
/// abriu, para o agente morrer no mesmo instante. Uma concessão de outra
/// pessoa dá a mesma resposta que uma inexistente (`404`).
#[utoipa::path(
    delete, path = "/api/users/me/oauth-grants/{grant_id}", tag = "users",
    security(("session" = [])),
    params(("grant_id" = Uuid, Path, description = "Concessão a revogar.")),
    responses(
        (status = 204, description = "Consentimento retirado e sessões do agente terminadas."),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 404, body = crate::openapi::ErrorBody, description = "Inexistente — ou de outra pessoa; a resposta é a mesma."),
    )
)]
pub async fn revoke_my_grant(
    State(state): State<Arc<AppState>>,
    user: AuthUser,
    axum::extract::Path(grant_id): axum::extract::Path<Uuid>,
) -> Result<axum::http::StatusCode, ApiError> {
    let sessoes = store::sessions_of_grant(&state.db, user.user_id, grant_id).await?;
    if !store::revoke_grant(&state.db, user.user_id, grant_id).await? {
        return Err(ApiError::NotFound);
    }
    for sid in sessoes {
        store::revoke_for_session(&state.db, sid).await?;
        crate::sessions::revoke(&state, user.user_id, sid, "user_revoked").await?;
    }
    crate::audit::log(
        &state.db,
        None,
        user.user_id,
        "oauth.grant_revoked",
        &grant_id.to_string(),
    )
    .await;
    Ok(axum::http::StatusCode::NO_CONTENT)
}

#[derive(utoipa::OpenApi)]
#[openapi(paths(
    as_metadata,
    resource_metadata,
    register,
    authorize,
    authorization_info,
    decide,
    token,
    revoke,
    my_grants,
    revoke_my_grant
))]
pub struct ApiDoc;
