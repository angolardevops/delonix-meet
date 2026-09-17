//! Chaves de API por organização + superfície REST pública `/api/v1`.
//!
//! Gestão (admin da org, autenticada por sessão): criar / listar / revogar.
//! A chave completa (`dlx_<32 hex>`) só é mostrada UMA vez na criação; guarda-se
//! o SHA-256. As integrações externas autenticam com
//! `Authorization: Bearer dlx_...` (ou header `X-API-Key`).

use axum::{
    extract::{FromRequestParts, Path, State},
    http::{request::Parts, HeaderMap},
    Json,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use uuid::Uuid;

use crate::{auth::AuthUser, error::ApiError, AppState};

use delonix_meet_core::crypto::{ct_eq, sha256_hex};

// ---------- Autenticação por chave de API (extractor) ----------

use delonix_meet_domain::identity::api_key as policy;
pub use delonix_meet_domain::identity::api_key::Scope;

/// A linha de uma chave `dlx_` tal como está na base de dados. Procurada UMA
/// vez por pedido: o `v1_rate_limit` precisa dela para escolher o balde, e o
/// extractor reutiliza-a (vai nas extensões do pedido).
#[derive(Clone, Debug, sqlx::FromRow)]
pub struct KeyRecord {
    pub id: Uuid,
    pub org_id: Uuid,
    pub created_by: Uuid,
    pub scopes: Vec<String>,
    pub expires_at: Option<chrono::DateTime<chrono::Utc>>,
    pub last_used_at: Option<chrono::DateTime<chrono::Utc>>,
}

impl KeyRecord {
    /// A chave ainda não expirou.
    pub fn is_usable(&self, now: chrono::DateTime<chrono::Utc>) -> bool {
        policy::ensure_not_expired(self.expires_at, now).is_ok()
    }
}

/// Resultado da procura da chave, guardado nas extensões do pedido. `None`
/// quando não havia chave `dlx_` ou ela não existe.
#[derive(Clone)]
pub struct KeyLookup(pub Option<KeyRecord>);

/// Lê a chave dos cabeçalhos (`X-API-Key` ou `Authorization: Bearer dlx_…`).
fn raw_key(headers: &HeaderMap) -> Option<String> {
    headers
        .get("x-api-key")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string())
        .or_else(|| crate::auth::bearer_token(headers).map(str::to_string))
        .filter(|k| k.starts_with("dlx_"))
}

/// Procura a chave apresentada. Não decide nada: expiração e escopos são do
/// extractor e de `ApiKeyAuth::require`.
pub async fn lookup_key(state: &AppState, headers: &HeaderMap) -> Result<KeyLookup, ApiError> {
    let Some(raw) = raw_key(headers) else {
        return Ok(KeyLookup(None));
    };
    let row: Option<KeyRecord> = sqlx::query_as(
        "SELECT id, org_id, created_by, scopes, expires_at, last_used_at
         FROM org_api_keys WHERE key_hash = $1",
    )
    .bind(sha256_hex(&raw))
    .fetch_optional(&state.db)
    .await?;
    Ok(KeyLookup(row))
}

/// Pedido autenticado por chave de API: transporta a organização, o
/// utilizador dono da chave (usado como owner das salas criadas via API) e os
/// escopos concedidos.
///
/// **Todo o handler que o recebe chama `key.require(Scope::…)?` na primeira
/// linha.** O teste `api_key_scopes::cada_rota_v1_exige_o_seu_escopo` percorre
/// as rotas e falha se alguma servir sem o seu escopo.
pub struct ApiKeyAuth {
    pub org_id: Uuid,
    pub owner_id: Uuid,
    scopes: Vec<Scope>,
}

impl ApiKeyAuth {
    /// `403 api_key.scope_missing`, com o escopo em `details`, se a chave
    /// não o tiver. O único sítio onde a v1 decide escopos.
    pub fn require(&self, scope: Scope) -> Result<(), ApiError> {
        policy::require_scope(&self.scopes, scope).map_err(ApiError::from)
    }
}

impl FromRequestParts<Arc<AppState>> for ApiKeyAuth {
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &Arc<AppState>,
    ) -> Result<Self, ApiError> {
        // O `v1_rate_limit` já procurou a chave; sem ele, procura-se aqui.
        let lookup = match parts.extensions.get::<KeyLookup>() {
            Some(l) => l.clone(),
            None => lookup_key(state, &parts.headers).await?,
        };
        // Revogada (apagada) ou desconhecida: 401 como sempre.
        let key = lookup.0.ok_or(ApiError::Unauthorized)?;
        let now = chrono::Utc::now();
        policy::ensure_not_expired(key.expires_at, now)?;

        // Registo de uso: no máximo uma escrita por minuto por chave. A guarda
        // repete-se no SQL para que dois nós com a mesma leitura antiga não
        // escrevam os dois.
        if policy::should_record_use(key.last_used_at, now) {
            if let Err(e) = sqlx::query(
                "UPDATE org_api_keys SET last_used_at = now()
                 WHERE id = $1
                   AND (last_used_at IS NULL
                        OR last_used_at <= now() - make_interval(secs => $2))",
            )
            .bind(key.id)
            .bind(policy::LAST_USED_THROTTLE_SECS as f64)
            .execute(&state.db)
            .await
            {
                // Não falha o pedido: o uso é informação, não autorização.
                tracing::warn!(error = %e, key_id = %key.id, "last_used_at não registado");
            }
        }
        Ok(ApiKeyAuth {
            org_id: key.org_id,
            owner_id: key.created_by,
            scopes: policy::granted_from_stored(&key.scopes),
        })
    }
}

// ---------- OpenAPI ----------

/// Rotas de gestão de chaves (BFF, sessão de admin da org).
#[derive(utoipa::OpenApi)]
#[openapi(
    paths(list, create, revoke),
    components(schemas(ApiKeyInfo, CreateKeyReq, CreatedKey))
)]
pub struct ApiDoc;

/// Rotas da superfície pública v1 servidas por este módulo (chave `dlx_`,
/// excepto a provisão, que usa o segredo de plataforma).
#[derive(utoipa::OpenApi)]
#[openapi(
    paths(
        v1_org,
        v1_provision_org,
        v1_create_room,
        v1_get_room,
        v1_join_bot_room,
        v1_recordings,
        v1_meetings,
        v1_meeting_notes,
    ),
    components(schemas(
        V1Org,
        ApiCreateRoomReq,
        V1RoomInfo,
        JoinBotReq,
        V1BotJoin,
        V1Recording,
        V1RecordingList,
        V1MeetingSummary,
        V1MeetingList,
        V1MeetingNotes,
        ProvisionOrgReq,
        ProvisionSso,
        ProvisionedOrg,
    ))
)]
pub struct V1ApiDoc;

// ---------- Gestão das chaves (admin, sessão) ----------

/// Chave de API como a lista a mostra. Nunca leva a chave nem o hash.
#[derive(Serialize, sqlx::FromRow, utoipa::ToSchema)]
pub struct ApiKeyInfo {
    pub id: Uuid,
    pub name: String,
    pub prefix: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// Actualizado no máximo uma vez por minuto por chave.
    pub last_used_at: Option<chrono::DateTime<chrono::Utc>>,
    /// Escopos concedidos (`org:read`, `rooms:read`, `rooms:write`, `bots:join`,
    /// `meetings:read`, `meetings:write`, `recordings:read`).
    pub scopes: Vec<String>,
    /// Ausente ⇒ não expira.
    pub expires_at: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Deserialize, utoipa::ToSchema)]
pub struct CreateKeyReq {
    /// Rótulo (cortado a 60 caracteres).
    #[serde(default)]
    pub name: String,
    /// Escopos do catálogo (`org:read`, `rooms:read`, `rooms:write`,
    /// `bots:join`, `meetings:read`, `meetings:write`, `recordings:read`).
    /// **Omisso ⇒ o catálogo inteiro** (compatibilidade com os clientes que
    /// já criam chaves sem escopos); `[]` é recusado.
    #[serde(default)]
    pub scopes: Option<Vec<String>>,
    /// Expiração opcional: no futuro e a no máximo dois anos. Omissa ⇒ não
    /// expira.
    #[serde(default)]
    pub expires_at: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct CreatedKey {
    pub id: Uuid,
    pub name: String,
    pub prefix: String,
    /// A chave completa — só devolvida AGORA, não fica guardada em claro.
    pub key: String,
    pub scopes: Vec<String>,
    pub expires_at: Option<chrono::DateTime<chrono::Utc>>,
}

/// Grava uma chave nova e devolve `(id, chave completa, prefixo)`. Um só
/// sítio para a geração: a BFF e o provisionamento chamam isto.
async fn insert_key(
    state: &AppState,
    org_id: Uuid,
    name: &str,
    created_by: Uuid,
    scopes: &[Scope],
    expires_at: Option<chrono::DateTime<chrono::Utc>>,
) -> Result<(Uuid, String, String), ApiError> {
    let key = delonix_meet_core::crypto::prefixed_token("dlx_"); // 256 bits de entropia
    let prefix = key.chars().take(12).collect::<String>();
    let scopes: Vec<&str> = scopes.iter().map(|s| s.as_str()).collect();
    let (id,): (Uuid,) = sqlx::query_as(
        "INSERT INTO org_api_keys (org_id, name, prefix, key_hash, created_by, scopes, expires_at)
         VALUES ($1, $2, $3, $4, $5, $6, $7) RETURNING id",
    )
    .bind(org_id)
    .bind(name)
    .bind(&prefix)
    .bind(sha256_hex(&key))
    .bind(created_by)
    .bind(&scopes)
    .bind(expires_at)
    .fetch_one(&state.db)
    .await?;
    Ok((id, key, prefix))
}

/// Chaves de API da organização (só admin). Nunca devolve a chave nem o hash,
/// só o prefixo, os escopos, a expiração e o último uso.
#[utoipa::path(
    get, path = "/api/orgs/{org_id}/api-keys", tag = "api-keys",
    security(("session" = [])),
    params(("org_id" = Uuid, Path, description = "Organização.")),
    responses(
        (status = 200, body = Vec<ApiKeyInfo>),
        (status = 401, description = "Sem sessão.", body = crate::openapi::ErrorBody),
        (status = 403, description = "Membro sem papel de admin.", body = crate::openapi::ErrorBody),
        (status = 404, description = "A organização não existe ou quem pede não é membro activo.", body = crate::openapi::ErrorBody),
    )
)]
pub async fn list(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(org_id): Path<Uuid>,
) -> Result<Json<Vec<ApiKeyInfo>>, ApiError> {
    crate::org::require_admin_pub(&state, org_id, auth.user_id).await?;
    let keys: Vec<ApiKeyInfo> = sqlx::query_as(
        "SELECT id, name, prefix, created_at, last_used_at, scopes, expires_at
         FROM org_api_keys WHERE org_id = $1 ORDER BY created_at DESC",
    )
    .bind(org_id)
    .fetch_all(&state.db)
    .await?;
    Ok(Json(keys))
}

/// Emite uma chave `dlx_` nova (só admin). A chave completa só aparece nesta
/// resposta; guarda-se o SHA-256.
#[utoipa::path(
    post, path = "/api/orgs/{org_id}/api-keys", tag = "api-keys",
    security(("session" = [])),
    params(("org_id" = Uuid, Path, description = "Organização.")),
    request_body = CreateKeyReq,
    responses(
        (status = 200, body = CreatedKey),
        (status = 400, description = "`api_key.scopes_empty`, `api_key.unknown_scope`, `api_key.expiry_in_past` ou `api_key.expiry_too_far`.", body = crate::openapi::ErrorBody),
        (status = 401, description = "Sem sessão.", body = crate::openapi::ErrorBody),
        (status = 403, description = "Membro sem papel de admin.", body = crate::openapi::ErrorBody),
        (status = 404, description = "A organização não existe ou quem pede não é membro activo.", body = crate::openapi::ErrorBody),
    )
)]
pub async fn create(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(org_id): Path<Uuid>,
    Json(req): Json<CreateKeyReq>,
) -> Result<Json<CreatedKey>, ApiError> {
    crate::org::require_admin_pub(&state, org_id, auth.user_id).await?;
    let scopes = policy::scopes_for_new_key(req.scopes.as_deref())?;
    policy::validate_expiry(req.expires_at, chrono::Utc::now())?;
    let name = req.name.trim().chars().take(60).collect::<String>();
    let (id, key, prefix) =
        insert_key(&state, org_id, &name, auth.user_id, &scopes, req.expires_at).await?;
    crate::audit::log(
        &state.db,
        Some(org_id),
        auth.user_id,
        "apikey.created",
        &name,
    )
    .await;
    Ok(Json(CreatedKey {
        id,
        name,
        prefix,
        key,
        scopes: scopes.iter().map(|s| s.as_str().to_string()).collect(),
        expires_at: req.expires_at,
    }))
}

/// Revoga (apaga) uma chave (só admin). `204`; uma chave que não existe — ou
/// que é de outra organização — dá `404 api_key.not_found`, sem confirmar que
/// existe noutro sítio.
#[utoipa::path(
    delete, path = "/api/orgs/{org_id}/api-keys/{key_id}", tag = "api-keys",
    security(("session" = [])),
    params(("org_id" = Uuid, Path, description = "Organização."), ("key_id" = Uuid, Path, description = "Chave a revogar.")),
    responses(
        (status = 204, description = "Revogada: a chave deixa de servir no pedido seguinte."),
        (status = 401, description = "Sem sessão.", body = crate::openapi::ErrorBody),
        (status = 403, description = "Membro sem papel de admin.", body = crate::openapi::ErrorBody),
        (status = 404, description = "A organização não existe, quem pede não é membro activo, ou a chave não existe nesta organização (`api_key.not_found`).", body = crate::openapi::ErrorBody),
    )
)]
pub async fn revoke(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path((org_id, key_id)): Path<(Uuid, Uuid)>,
) -> Result<axum::http::StatusCode, ApiError> {
    crate::org::require_admin_pub(&state, org_id, auth.user_id).await?;
    let deleted = sqlx::query("DELETE FROM org_api_keys WHERE id = $1 AND org_id = $2")
        .bind(key_id)
        .bind(org_id)
        .execute(&state.db)
        .await?
        .rows_affected();
    if deleted == 0 {
        return Err(delonix_meet_core::DomainError::not_found("api_key.not_found").into());
    }
    crate::audit::log(
        &state.db,
        Some(org_id),
        auth.user_id,
        "apikey.revoked",
        &key_id.to_string(),
    )
    .await;
    Ok(axum::http::StatusCode::NO_CONTENT)
}

// ---------- API pública v1 (autenticada por chave) ----------

/// Constrói o link partilhável de uma sala com o domínio de produção da org.
pub async fn room_link(state: &AppState, org_id: Uuid, code: &str) -> String {
    let domain: Option<(String,)> =
        sqlx::query_as("SELECT domain FROM organizations WHERE id = $1 AND domain <> ''")
            .bind(org_id)
            .fetch_optional(&state.db)
            .await
            .ok()
            .flatten();
    match domain {
        Some((d,)) => format!("https://{d}/#/r/{code}"),
        None => format!("/#/r/{code}"),
    }
}

#[derive(Deserialize, utoipa::ToSchema)]
pub struct ApiCreateRoomReq {
    /// Omisso ⇒ «Reunião (API)»; cortado a 120 caracteres.
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub e2ee: bool,
    #[serde(default)]
    pub waiting_room: bool,
}

/// Sala vista pela API v1.
#[derive(Serialize, utoipa::ToSchema)]
pub struct V1RoomInfo {
    pub code: String,
    pub name: String,
    pub e2ee: bool,
    pub waiting_room: bool,
    /// Link de entrada: absoluto com o domínio de produção da org, ou relativo
    /// (`/#/r/{code}`) se não houver domínio.
    pub join_url: String,
}

/// `POST /api/v1/rooms` — cria uma sala e devolve o código + link de entrada.
/// O dono da sala é o utilizador que criou a chave.
#[utoipa::path(
    post, path = "/api/v1/rooms", tag = "v1",
    security(("api_key" = ["rooms:write"])),
    request_body = ApiCreateRoomReq,
    responses(
        (status = 200, body = V1RoomInfo),
        (status = 401, description = "Chave ausente, sem prefixo `dlx_`, desconhecida ou revogada (`auth.unauthenticated`), ou expirada (`api_key.expired`).", body = crate::openapi::ErrorBody),
        (status = 403, description = "A chave não tem o escopo `rooms:write` (`api_key.scope_missing`, escopo em `details`).", body = crate::openapi::ErrorBody),
        (status = 429, description = "Limite de pedidos da v1 por chave (`Retry-After` com o que falta da janela).", body = crate::openapi::ErrorBody),
    )
)]
pub async fn v1_create_room(
    State(state): State<Arc<AppState>>,
    key: ApiKeyAuth,
    Json(req): Json<ApiCreateRoomReq>,
) -> Result<Json<V1RoomInfo>, ApiError> {
    key.require(Scope::RoomsWrite)?;
    let name: String = req
        .name
        .unwrap_or_else(|| "Reunião (API)".into())
        .trim()
        .chars()
        .take(120)
        .collect();
    let room = crate::rooms::insert_room(
        &state.db,
        key.owner_id,
        &name,
        "sfu",
        req.waiting_room,
        req.e2ee,
        "normal",
    )
    .await?;
    let link = room_link(&state, key.org_id, &room.code).await;
    Ok(Json(V1RoomInfo {
        code: room.code,
        name: room.name,
        e2ee: room.e2ee,
        waiting_room: room.waiting_room,
        join_url: link,
    }))
}

/// `GET /api/v1/rooms/{code}` — metadados de uma sala da organização da chave.
#[utoipa::path(
    get, path = "/api/v1/rooms/{room_code}", tag = "v1",
    security(("api_key" = ["rooms:read"])),
    params(("room_code" = String, Path, description = "Código da sala (sensível a maiúsculas aqui).")),
    responses(
        (status = 200, body = V1RoomInfo),
        (status = 401, description = "Chave ausente, sem prefixo `dlx_`, desconhecida ou revogada (`auth.unauthenticated`), ou expirada (`api_key.expired`).", body = crate::openapi::ErrorBody),
        (status = 403, description = "A chave não tem o escopo `rooms:read` (`api_key.scope_missing`, escopo em `details`).", body = crate::openapi::ErrorBody),
        (status = 429, description = "Limite de pedidos da v1 por chave (`Retry-After` com o que falta da janela).", body = crate::openapi::ErrorBody),
        (status = 404, description = "A sala não existe ou o dono não é membro activo da organização da chave.", body = crate::openapi::ErrorBody),
    )
)]
pub async fn v1_get_room(
    State(state): State<Arc<AppState>>,
    key: ApiKeyAuth,
    Path(code): Path<String>,
) -> Result<Json<V1RoomInfo>, ApiError> {
    key.require(Scope::RoomsRead)?;
    let row: Option<(String, String, bool, bool, Uuid)> = sqlx::query_as(
        "SELECT code, name, e2ee, waiting_room, owner_id FROM rooms WHERE code = $1",
    )
    .bind(&code)
    .fetch_optional(&state.db)
    .await?;
    let (code, name, e2ee, waiting, owner_id) = row.ok_or(ApiError::NotFound)?;

    // Assegura que a sala pertence à organização da chave API.
    crate::org::require_member_pub(&state, key.org_id, owner_id).await?;

    let link = room_link(&state, key.org_id, &code).await;
    Ok(Json(V1RoomInfo {
        code,
        name,
        e2ee,
        waiting_room: waiting,
        join_url: link,
    }))
}

#[derive(Deserialize, utoipa::ToSchema)]
pub struct JoinBotReq {
    /// Nome mostrado na sala; vazio ⇒ «AI Assistant»; cortado a 40 caracteres.
    pub bot_name: String,
}

/// Credenciais de entrada de um bot numa sala.
#[derive(Serialize, utoipa::ToSchema)]
pub struct V1BotJoin {
    /// A sala completa (`id`, `code`, `name`, `owner_id`, `topology`,
    /// `waiting_room`, `e2ee`, `format`, `created_at`).
    #[schema(value_type = Object)]
    pub room: crate::rooms::Room,
    /// JWT `typ: "room"` com `is_bot: true`, atribuído ao criador da chave.
    pub room_token: String,
    /// `/ws?token=<room_token>`.
    pub ws_path: String,
}

/// `POST /api/v1/rooms/{code}/bots` — gera um room_token para um bot headless.
/// O token contorna a sala de espera.
#[utoipa::path(
    post, path = "/api/v1/rooms/{room_code}/bots", tag = "v1",
    security(("api_key" = ["bots:join"])),
    params(("room_code" = String, Path, description = "Código da sala (normalizado para minúsculas).")),
    request_body = JoinBotReq,
    responses(
        (status = 200, body = V1BotJoin),
        (status = 401, description = "Chave ausente, sem prefixo `dlx_`, desconhecida ou revogada (`auth.unauthenticated`), ou expirada (`api_key.expired`).", body = crate::openapi::ErrorBody),
        (status = 403, description = "A chave não tem o escopo `bots:join` (`api_key.scope_missing`, escopo em `details`).", body = crate::openapi::ErrorBody),
        (status = 429, description = "Limite de pedidos da v1 por chave (`Retry-After` com o que falta da janela).", body = crate::openapi::ErrorBody),
        (status = 404, description = "A sala não existe ou o dono não é membro activo da organização da chave.", body = crate::openapi::ErrorBody),
    )
)]
pub async fn v1_join_bot_room(
    State(state): State<Arc<AppState>>,
    key: ApiKeyAuth,
    Path(code): Path<String>,
    Json(req): Json<JoinBotReq>,
) -> Result<Json<V1BotJoin>, ApiError> {
    key.require(Scope::BotsJoin)?;
    let room: crate::rooms::Room = sqlx::query_as(&format!(
        "SELECT {} FROM rooms WHERE code = $1",
        crate::rooms::ROOM_COLUMNS
    ))
    .bind(code.to_lowercase())
    .fetch_one(&state.db)
    .await?;

    // O bot só pode entrar em salas que pertençam à sua organização.
    crate::org::require_member_pub(&state, key.org_id, room.owner_id).await?;

    // Para bots autorizados pela org (via API key), podemos forçar entrada direta
    // mesmo que a sala tenha waiting_room, dado o consentimento via plano implementado.
    let now = chrono::Utc::now().timestamp();
    let bot_name = if req.bot_name.trim().is_empty() {
        "AI Assistant".into()
    } else {
        req.bot_name.trim().chars().take(40).collect::<String>()
    };

    let room_token = crate::auth::sign_jwt(
        &state.config.jwt_secret,
        &crate::auth::Claims {
            sub: key.owner_id, // Atribui as ações do bot ao criador da chave API
            typ: "room".into(),
            iat: now,
            exp: now + state.config.room_token_ttl_secs,
            room: Some(room.id),
            name: Some(bot_name),
            topo: Some(room.topology.clone()),
            owner: false, // bots não podem gerir a sala
            wait: false,  // Bypass da sala de espera (acordado com o utilizador)
            adm: false,
            is_bot: true,
        },
    )?;

    Ok(Json(V1BotJoin {
        ws_path: format!("/ws?token={room_token}"),
        room,
        room_token,
    }))
}

/// Gravação vista pela API v1.
#[derive(Serialize, utoipa::ToSchema)]
pub struct V1Recording {
    pub id: Uuid,
    pub filename: String,
    pub size_bytes: i64,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub room_code: String,
    /// `/api/recordings/{id}` (rota da BFF, autenticada por sessão).
    pub download_url: String,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct V1RecordingList {
    pub recordings: Vec<V1Recording>,
}

/// `GET /api/v1/recordings` — gravações da organização (membros), as 200 mais
/// recentes.
#[utoipa::path(
    get, path = "/api/v1/recordings", tag = "v1",
    security(("api_key" = ["recordings:read"])),
    responses(
        (status = 200, body = V1RecordingList),
        (status = 401, description = "Chave ausente, sem prefixo `dlx_`, desconhecida ou revogada (`auth.unauthenticated`), ou expirada (`api_key.expired`).", body = crate::openapi::ErrorBody),
        (status = 403, description = "A chave não tem o escopo `recordings:read` (`api_key.scope_missing`, escopo em `details`).", body = crate::openapi::ErrorBody),
        (status = 429, description = "Limite de pedidos da v1 por chave (`Retry-After` com o que falta da janela).", body = crate::openapi::ErrorBody),
    )
)]
pub async fn v1_recordings(
    State(state): State<Arc<AppState>>,
    key: ApiKeyAuth,
) -> Result<Json<V1RecordingList>, ApiError> {
    key.require(Scope::RecordingsRead)?;
    let rows: Vec<(Uuid, String, i64, chrono::DateTime<chrono::Utc>, String)> = sqlx::query_as(
        "SELECT r.id, r.filename, r.size_bytes, r.created_at, rm.code
         FROM recordings r
         JOIN rooms rm ON rm.id = r.room_id
         JOIN org_members m ON m.user_id = r.uploader_id AND m.org_id = $1
         ORDER BY r.created_at DESC LIMIT 200",
    )
    .bind(key.org_id)
    .fetch_all(&state.db)
    .await?;
    let recordings: Vec<V1Recording> = rows
        .into_iter()
        .map(|(id, filename, size, created, code)| V1Recording {
            id,
            filename,
            size_bytes: size,
            created_at: created,
            room_code: code,
            download_url: format!("/api/recordings/{id}/content"),
        })
        .collect();
    Ok(Json(V1RecordingList { recordings }))
}

// ---------- Reuniões (sync de calendário e MoM — nk_delonix_meet) ----------

#[derive(Deserialize, utoipa::IntoParams)]
#[into_params(parameter_in = Query)]
pub struct MeetingsQuery {
    /// Cursor incremental: devolve reuniões criadas, com ata AI atualizada ou
    /// com início desde este instante. Omisso => tudo (limitado a 500).
    #[serde(default)]
    pub since: Option<chrono::DateTime<chrono::Utc>>,
}

/// Reunião vista pelo sync de calendário v1.
#[derive(Serialize, utoipa::ToSchema)]
pub struct V1MeetingSummary {
    pub id: Uuid,
    pub title: String,
    pub description: String,
    /// `video` | `voice`.
    pub kind: String,
    pub starts_at: chrono::DateTime<chrono::Utc>,
    pub duration_min: i32,
    pub room_code: Option<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// Presente ⇒ a ata AI já foi gerada.
    pub minutes_ai_at: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct V1MeetingList {
    pub meetings: Vec<V1MeetingSummary>,
}

/// `GET /api/v1/meetings?since=<rfc3339>` — reuniões da organização (dono é
/// membro), incremental para o cron de sync do Odoo (idempotente no cliente).
/// No máximo 500, por `starts_at`.
#[utoipa::path(
    get, path = "/api/v1/meetings", tag = "v1",
    security(("api_key" = ["meetings:read"])),
    params(MeetingsQuery),
    responses(
        (status = 200, body = V1MeetingList),
        (status = 400, description = "`since` não é RFC 3339.", body = crate::openapi::ErrorBody),
        (status = 401, description = "Chave ausente, sem prefixo `dlx_`, desconhecida ou revogada (`auth.unauthenticated`), ou expirada (`api_key.expired`).", body = crate::openapi::ErrorBody),
        (status = 403, description = "A chave não tem o escopo `meetings:read` (`api_key.scope_missing`, escopo em `details`).", body = crate::openapi::ErrorBody),
        (status = 429, description = "Limite de pedidos da v1 por chave (`Retry-After` com o que falta da janela).", body = crate::openapi::ErrorBody),
    )
)]
pub async fn v1_meetings(
    State(state): State<Arc<AppState>>,
    key: ApiKeyAuth,
    axum::extract::Query(q): axum::extract::Query<MeetingsQuery>,
) -> Result<Json<V1MeetingList>, ApiError> {
    key.require(Scope::MeetingsRead)?;
    #[allow(clippy::type_complexity)]
    let rows: Vec<(
        Uuid,
        String,
        String,
        String,
        chrono::DateTime<chrono::Utc>,
        i32,
        Option<String>,
        chrono::DateTime<chrono::Utc>,
        Option<chrono::DateTime<chrono::Utc>>,
    )> = sqlx::query_as(
        "SELECT m.id, m.title, m.description, m.kind, m.starts_at, m.duration_min,
                m.room_code, m.created_at, m.minutes_ai_at
         FROM meetings m
         WHERE EXISTS (SELECT 1 FROM org_members om
                       WHERE om.org_id = $1 AND om.user_id = m.owner_id)
           AND ($2::timestamptz IS NULL
                OR m.created_at >= $2 OR m.starts_at >= $2 OR m.minutes_ai_at >= $2)
         ORDER BY m.starts_at LIMIT 500",
    )
    .bind(key.org_id)
    .bind(q.since)
    .fetch_all(&state.db)
    .await?;
    let meetings: Vec<V1MeetingSummary> = rows
        .into_iter()
        .map(
            |(
                id,
                title,
                description,
                kind,
                starts_at,
                duration_min,
                room_code,
                created_at,
                minutes_ai_at,
            )| V1MeetingSummary {
                id,
                title,
                description,
                kind,
                starts_at,
                duration_min,
                room_code,
                created_at,
                minutes_ai_at,
            },
        )
        .collect();
    Ok(Json(V1MeetingList { meetings }))
}

/// Ata e transcrição de uma reunião.
#[derive(Serialize, utoipa::ToSchema)]
pub struct V1MeetingNotes {
    pub id: Uuid,
    pub title: String,
    /// Ata (MoM).
    pub minutes: String,
    /// Transcrição bruta.
    pub transcript: String,
    pub minutes_ai_at: Option<chrono::DateTime<chrono::Utc>>,
}

/// `GET /api/v1/meetings/{id}/minutes` — ata (MoM) + transcrição (ata bruta).
/// `minutes_ai_at` presente => o MoM já é a versão final do LLM local.
#[utoipa::path(
    get, path = "/api/v1/meetings/{meeting_id}/minutes", tag = "v1",
    security(("api_key" = ["meetings:read"])),
    params(("meeting_id" = Uuid, Path, description = "Reunião.")),
    responses(
        (status = 200, body = V1MeetingNotes),
        (status = 401, description = "Chave ausente, sem prefixo `dlx_`, desconhecida ou revogada (`auth.unauthenticated`), ou expirada (`api_key.expired`).", body = crate::openapi::ErrorBody),
        (status = 403, description = "A chave não tem o escopo `meetings:read` (`api_key.scope_missing`, escopo em `details`).", body = crate::openapi::ErrorBody),
        (status = 429, description = "Limite de pedidos da v1 por chave (`Retry-After` com o que falta da janela).", body = crate::openapi::ErrorBody),
        (status = 404, description = "A reunião não existe ou o dono não é membro da organização da chave.", body = crate::openapi::ErrorBody),
    )
)]
pub async fn v1_meeting_notes(
    State(state): State<Arc<AppState>>,
    key: ApiKeyAuth,
    Path(id): Path<Uuid>,
) -> Result<Json<V1MeetingNotes>, ApiError> {
    key.require(Scope::MeetingsRead)?;
    let row: Option<(
        String,
        String,
        String,
        Option<chrono::DateTime<chrono::Utc>>,
    )> = sqlx::query_as(
        "SELECT m.title, m.minutes, m.transcript, m.minutes_ai_at
             FROM meetings m
             WHERE m.id = $1
               AND EXISTS (SELECT 1 FROM org_members om
                           WHERE om.org_id = $2 AND om.user_id = m.owner_id)",
    )
    .bind(id)
    .bind(key.org_id)
    .fetch_optional(&state.db)
    .await?;
    let Some((title, minutes, transcript, minutes_ai_at)) = row else {
        return Err(ApiError::NotFound);
    };
    Ok(Json(V1MeetingNotes {
        id,
        title,
        minutes,
        transcript,
        minutes_ai_at,
    }))
}

/// Organização vista pela API v1.
#[derive(Serialize, utoipa::ToSchema)]
pub struct V1Org {
    pub id: Uuid,
    pub name: String,
    /// Domínio de email da organização (vazio se não definido).
    pub email_domain: String,
    /// Domínio de produção dos links (vazio se não definido).
    pub domain: String,
    /// Número de linhas de membro da organização.
    pub members: i64,
}

/// `GET /api/v1/organization` — dados da organização da chave.
#[utoipa::path(
    get, path = "/api/v1/organization", tag = "v1",
    security(("api_key" = ["org:read"])),
    responses(
        (status = 200, body = V1Org),
        (status = 401, description = "Chave ausente, sem prefixo `dlx_`, desconhecida ou revogada (`auth.unauthenticated`), ou expirada (`api_key.expired`).", body = crate::openapi::ErrorBody),
        (status = 403, description = "A chave não tem o escopo `org:read` (`api_key.scope_missing`, escopo em `details`).", body = crate::openapi::ErrorBody),
        (status = 429, description = "Limite de pedidos da v1 por chave (`Retry-After` com o que falta da janela).", body = crate::openapi::ErrorBody),
    )
)]
pub async fn v1_org(
    State(state): State<Arc<AppState>>,
    key: ApiKeyAuth,
) -> Result<Json<V1Org>, ApiError> {
    key.require(Scope::OrgRead)?;
    let row: (String, String, String) =
        sqlx::query_as("SELECT name, email_domain, domain FROM organizations WHERE id = $1")
            .bind(key.org_id)
            .fetch_one(&state.db)
            .await?;
    let (members,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM org_members WHERE org_id = $1")
        .bind(key.org_id)
        .fetch_one(&state.db)
        .await?;
    Ok(Json(V1Org {
        id: key.org_id,
        name: row.0,
        email_domain: row.1,
        domain: row.2,
        members,
    }))
}

// ---------- Provisão de organização (segredo de plataforma) ----------

/// Config OIDC opcional a aplicar à org acabada de criar — deixa o provisionador
/// (Odoo) apontar a org ao seu próprio IdP num só passo, sem um segundo pedido
/// autenticado por sessão de admin (que a org recém-criada ainda não tem).
#[derive(Deserialize, utoipa::ToSchema)]
pub struct ProvisionSso {
    pub issuer_url: String,
    pub client_id: String,
    pub client_secret: String,
    #[serde(default)]
    pub enforce_sso: bool,
}

#[derive(Deserialize, utoipa::ToSchema)]
pub struct ProvisionOrgReq {
    /// Nome da organização (== nome da empresa no sistema chamador).
    pub name: String,
    /// Rótulo da chave de API emitida (default "Integration").
    #[serde(default)]
    pub key_name: Option<String>,
    /// Escopos da chave emitida (ver `CreateKeyReq::scopes`). **Omisso ⇒ o
    /// catálogo inteiro** — é o que o módulo Odoo recebe hoje.
    #[serde(default)]
    pub scopes: Option<Vec<String>>,
    /// Domínio de email da org — necessário para o ``/api/auth/sso/authorize?domain=``
    /// resolver esta org. Opcional.
    #[serde(default)]
    pub email_domain: Option<String>,
    /// Config OIDC a aplicar (opcional).
    #[serde(default)]
    pub sso: Option<ProvisionSso>,
    /// Se `true`, activa a integração Odoo e devolve também um `dlxo_...` token
    /// pronto a colar no módulo nk_delonix_meet. Requer `odoo_url` e `odoo_db`.
    #[serde(default)]
    pub setup_odoo: bool,
    /// URL base da instância Odoo (ex.: `http://localhost:8090`).
    #[serde(default)]
    pub odoo_url: Option<String>,
    /// Base de dados Odoo da empresa.
    #[serde(default)]
    pub odoo_db: Option<String>,
    /// Id da EMPRESA no Odoo (`res.company`). Com `odoo_db`, identifica a
    /// organização de forma estável — a mesma chave que o login por conta
    /// Odoo usa (`odoo_sso::ensure_org`).
    ///
    /// Sem isto, provisionar uma empresa que já tinha entrado por SSO criava
    /// uma SEGUNDA organização para a mesma empresa: os utilizadores ficavam
    /// numa e a chave de API na outra, e a criação de reuniões respondia
    /// «o anfitrião pertence a outra organização» (409).
    #[serde(default)]
    pub odoo_company_id: Option<i32>,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct ProvisionedOrg {
    pub org_id: Uuid,
    pub slug: String,
    pub name: String,
    /// A chave de API completa (`dlx_...`) — só devolvida AGORA, guarda-se o hash.
    pub api_key: String,
    /// True se uma config OIDC foi aplicada (fecha o SSO sem SQL manual).
    pub sso_configured: bool,
    /// Token de integração Odoo (`dlxo_...`) — presente só se `setup_odoo=true`.
    /// Mostrar uma vez ao utilizador: o servidor guarda apenas o hash.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub odoo_token: Option<String>,
}

/// Ver `ensure_provisioning_user`. Exposto para o login por conta Odoo, que
/// também precisa de um dono técnico para a org que acabou de nascer.
pub async fn ensure_provisioning_user_pub(state: &AppState) -> Result<Uuid, ApiError> {
    ensure_provisioning_user(state).await
}

/// Utilizador de serviço único que "possui" as organizações provisionadas.
/// Idempotente: criado à primeira, reutilizado depois. Nunca faz login (a
/// password é aleatória e descartada); serve só como ``created_by`` técnico.
async fn ensure_provisioning_user(state: &AppState) -> Result<Uuid, ApiError> {
    const EMAIL: &str = "provisioning@delonix.internal";
    const USERNAME: &str = "delonix-provisioning";
    if let Some((id,)) = sqlx::query_as::<_, (Uuid,)>("SELECT id FROM users WHERE email = $1")
        .bind(EMAIL)
        .fetch_optional(&state.db)
        .await?
    {
        return Ok(id);
    }
    let hash = crate::auth::hash_password(&delonix_meet_core::crypto::random_hex(24))?;
    let res: Result<(Uuid,), sqlx::Error> = sqlx::query_as(
        "INSERT INTO users (email, username, password_hash) VALUES ($1, $2, $3) RETURNING id",
    )
    .bind(EMAIL)
    .bind(USERNAME)
    .bind(&hash)
    .fetch_one(&state.db)
    .await;
    match res {
        Ok((id,)) => Ok(id),
        // Corrida: outro pedido criou-o entretanto → relê.
        Err(sqlx::Error::Database(db)) if db.is_unique_violation() => {
            let (id,): (Uuid,) = sqlx::query_as("SELECT id FROM users WHERE email = $1")
                .bind(EMAIL)
                .fetch_one(&state.db)
                .await?;
            Ok(id)
        }
        Err(e) => Err(e.into()),
    }
}

/// `POST /api/operator/v1/organizations` — provisiona uma organização + emite a sua chave
/// de API. Autenticado pelo **segredo de plataforma** (`X-Provisioning-Secret`),
/// não por chave de org (que ainda não existe). Pensado para o Odoo criar a org
/// de cada empresa e receber a chave para depois criar salas via `/api/v1/rooms`.
///
/// Fail-closed: com `PROVISIONING_SECRET` vazio o endpoint recusa sempre.
///
/// Idempotente por empresa Odoo (`odoo_db` + `odoo_company_id`): reprovisionar
/// reutiliza a organização, mas emite SEMPRE uma chave de API nova.
#[utoipa::path(
    post, path = "/api/operator/v1/organizations", tag = "v1",
    params(("X-Provisioning-Secret" = String, Header, description = "Segredo de plataforma (`PROVISIONING_SECRET`).")),
    request_body = ProvisionOrgReq,
    responses(
        (status = 200, body = ProvisionedOrg),
        (status = 400, description = "Nome vazio/longo, `odoo_db` sem `odoo_company_id`, ou `scopes` inválidos (`api_key.scopes_empty`, `api_key.unknown_scope`).", body = crate::openapi::ErrorBody),
        (status = 401, description = "Segredo ausente, errado, ou provisionamento desactivado.", body = crate::openapi::ErrorBody),
        (status = 429, description = "Limite de pedidos da superfície v1 por IP.", body = crate::openapi::ErrorBody),
    )
)]
pub async fn v1_provision_org(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(req): Json<ProvisionOrgReq>,
) -> Result<Json<ProvisionedOrg>, ApiError> {
    let configured = state.config.provisioning_secret.as_bytes();
    if configured.is_empty() {
        return Err(ApiError::Unauthorized);
    }
    let provided = headers
        .get("x-provisioning-secret")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    if !ct_eq(provided.as_bytes(), configured) {
        return Err(ApiError::Unauthorized);
    }

    let name = req.name.trim();
    if name.is_empty() || name.len() > 120 {
        return Err(ApiError::BadRequest("nome da organização inválido".into()));
    }
    // Antes de criar a organização: um escopo inválido não deixa uma org a meio.
    let key_scopes = policy::scopes_for_new_key(req.scopes.as_deref())?;

    let service_user_id = ensure_provisioning_user(&state).await?;

    // A empresa Odoo já tem organização? (criada por um login SSO anterior,
    // ou por um provisionamento repetido). Reutiliza-se em vez de duplicar —
    // ver o comentário em `odoo_company_id`.
    // `odoo_company_id` é `#[serde(default)]`, portanto um módulo Odoo ANTIGO
    // não o envia. Sem ele não há chave de deduplicação — e deixar passar em
    // silêncio criava exactamente a organização duplicada que este bloco existe
    // para evitar, para a população que ele mais precisa de servir.
    //
    // Não se pode desdobrar para "dedup só por `odoo_db`": uma base de dados
    // Odoo hospeda VÁRIAS empresas, e isso fundiria tenants distintos — pior que
    // duplicar. Fica fail-closed com a acção concreta: actualizar o módulo.
    if req
        .odoo_db
        .as_deref()
        .map(|d| !d.trim().is_empty())
        .unwrap_or(false)
        && req.odoo_company_id.is_none()
    {
        return Err(ApiError::BadRequest(
            "odoo_db enviado sem odoo_company_id: sem os dois não é possível              reconhecer a empresa e o provisionamento criaria uma organização              duplicada. Actualiza o módulo nk_delonix_meet para enviar              odoo_company_id."
                .into(),
        ));
    }

    let existing_org: Option<(Uuid, String)> =
        match (&req.odoo_db, req.odoo_company_id) {
            (Some(db), Some(cid)) if !db.trim().is_empty() => sqlx::query_as(
                "SELECT id, slug FROM organizations WHERE odoo_db = $1 AND odoo_company_id = $2",
            )
            .bind(db.trim())
            .bind(cid)
            .fetch_optional(&state.db)
            .await?,
            _ => None,
        };

    // Org com slug único (sufixo em colisão). SEM a quota anti-abuso de
    // create_org: aqui a autorização é o segredo de plataforma, não um user.
    let base = crate::org::slugify(name);
    let mut created: Option<(Uuid, String)> = existing_org;
    for i in 0..8 {
        if created.is_some() {
            break; // a empresa Odoo já tinha org — só falta emitir a chave
        }
        let slug = if i == 0 {
            base.clone()
        } else {
            format!("{base}-{i}")
        };
        let res: Result<(Uuid,), sqlx::Error> = sqlx::query_as(
            "INSERT INTO organizations (name, slug, created_by, odoo_db, odoo_company_id)
             VALUES ($1, $2, $3, $4, $5) RETURNING id",
        )
        .bind(name)
        .bind(&slug)
        .bind(service_user_id)
        .bind(
            req.odoo_db
                .as_deref()
                .map(str::trim)
                .filter(|d| !d.is_empty()),
        )
        .bind(req.odoo_company_id)
        .fetch_one(&state.db)
        .await;
        match res {
            Ok((id,)) => {
                created = Some((id, slug));
                break;
            }
            // Corrida na chave da empresa Odoo: outro pedido criou-a agora.
            Err(sqlx::Error::Database(db)) if db.is_unique_violation() => {
                if let (Some(dbn), Some(cid)) = (&req.odoo_db, req.odoo_company_id) {
                    if let Some(row) = sqlx::query_as::<_, (Uuid, String)>(
                        "SELECT id, slug FROM organizations
                         WHERE odoo_db = $1 AND odoo_company_id = $2",
                    )
                    .bind(dbn.trim())
                    .bind(cid)
                    .fetch_optional(&state.db)
                    .await?
                    {
                        created = Some(row);
                        break;
                    }
                }
                continue; // colisão só de slug: tenta o sufixo seguinte
            }
            Err(e) => return Err(e.into()),
        }
    }
    let (org_id, slug) =
        created.ok_or_else(|| ApiError::internal("could not allocate org slug"))?;

    // `DO NOTHING`: a org pode já existir (reprovisionamento, ou criada antes
    // por um login SSO) e o utilizador de serviço já ser membro dela.
    sqlx::query(
        "INSERT INTO org_members (org_id, user_id, role, title)
         VALUES ($1, $2, 'admin', 'Provisioning')
         ON CONFLICT (org_id, user_id) DO NOTHING",
    )
    .bind(org_id)
    .bind(service_user_id)
    .execute(&state.db)
    .await?;

    // Chave de API da org (mesma geração que apikeys::create). Sem `scopes`
    // ⇒ o catálogo inteiro: o módulo Odoo usa `meetings:read` (sync e atas) e
    // `meetings:write` (criar/alterar/cancelar/tocar), e o provisionamento é
    // a única forma de ele obter a chave — uma chave provisionada mais pobre
    // partia a integração sem ninguém mudar nada do lado de lá.
    let key_name: String = req
        .key_name
        .clone()
        .unwrap_or_else(|| "Integration".into())
        .trim()
        .chars()
        .take(60)
        .collect();
    let (_, key, _) = insert_key(
        &state,
        org_id,
        &key_name,
        service_user_id,
        &key_scopes,
        None,
    )
    .await?;

    // Domínio de email (best-effort — o índice único pode colidir com outra
    // org; nesse caso fica por definir e o admin resolve).
    if let Some(domain) = req
        .email_domain
        .as_ref()
        .map(|d| d.trim().to_lowercase())
        .filter(|d| !d.is_empty())
    {
        let _ = sqlx::query("UPDATE organizations SET email_domain = $1 WHERE id = $2")
            .bind(&domain)
            .bind(org_id)
            .execute(&state.db)
            .await;
    }

    // Config OIDC (fecha o SSO no mesmo passo — sem SQL manual). O
    // client_secret é cifrado como em `org::upsert_sso_config` (S5, R160): esta
    // era a última escrita em claro.
    let mut sso_configured = false;
    if let Some(sso) = &req.sso {
        let issuer = sso.issuer_url.trim();
        if !issuer.is_empty() && !sso.client_id.trim().is_empty() {
            let plain = sso.client_secret.trim();
            let sealed = if plain.is_empty() {
                String::new()
            } else {
                crate::org::seal_sso_client_secret(&state.config, org_id, plain)?
            };
            sqlx::query(
                "INSERT INTO org_sso_configs (org_id, issuer_url, client_id, client_secret, enforce_sso)
                 VALUES ($1, $2, $3, $4, $5)
                 ON CONFLICT (org_id) DO UPDATE
                 SET issuer_url = EXCLUDED.issuer_url, client_id = EXCLUDED.client_id,
                     client_secret = EXCLUDED.client_secret, enforce_sso = EXCLUDED.enforce_sso,
                     updated_at = now()",
            )
            .bind(org_id)
            .bind(issuer)
            .bind(sso.client_id.trim())
            .bind(&sealed)
            .bind(sso.enforce_sso)
            .execute(&state.db)
            .await?;
            sso_configured = true;
        }
    }

    // Integração Odoo — activar e gerar dlxo_ token num só passo.
    let odoo_token = if req.setup_odoo {
        let raw = delonix_meet_core::crypto::prefixed_token("dlxo_");
        let hash = sha256_hex(&raw);
        let prefix: String = raw.chars().take(12).collect();
        let url = req.odoo_url.as_deref().unwrap_or("").trim();
        let db = req.odoo_db.as_deref().unwrap_or("").trim();
        sqlx::query(
            "UPDATE organizations
             SET odoo_enabled = TRUE, odoo_token_hash = $1, odoo_token_prefix = $2,
                 odoo_url = NULLIF($3,''), odoo_db = NULLIF($4,'')
             WHERE id = $5",
        )
        .bind(&hash)
        .bind(&prefix)
        .bind(url)
        .bind(db)
        .bind(org_id)
        .execute(&state.db)
        .await?;
        Some(raw)
    } else {
        None
    };

    crate::audit::log(
        &state.db,
        Some(org_id),
        service_user_id,
        "org.provisioned",
        name,
    )
    .await;
    Ok(Json(ProvisionedOrg {
        org_id,
        slug,
        name: name.to_string(),
        api_key: key,
        sso_configured,
        odoo_token,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Os tipos que substituíram os `json!` da v1 (OpenAPI) serializam com os
    /// mesmos campos e valores — a v1 é contrato estável.
    #[test]
    fn respostas_v1_tipadas_serializam_como_antes() {
        let t0 = chrono::DateTime::from_timestamp(0, 0).unwrap();
        let id = Uuid::nil();
        assert_eq!(
            serde_json::to_value(V1RoomInfo {
                code: "c".into(),
                name: "n".into(),
                e2ee: true,
                waiting_room: false,
                join_url: "/#/r/c".into(),
            })
            .unwrap(),
            serde_json::json!({"code": "c", "name": "n", "e2ee": true, "waiting_room": false, "join_url": "/#/r/c"})
        );
        let room = crate::rooms::Room {
            id,
            code: "c".into(),
            name: "n".into(),
            owner_id: id,
            topology: "sfu".into(),
            waiting_room: false,
            e2ee: false,
            format: "normal".into(),
            created_at: t0,
        };
        let room_json = serde_json::to_value(&room).unwrap();
        assert_eq!(
            serde_json::to_value(V1BotJoin {
                room,
                room_token: "tok".into(),
                ws_path: "/ws?token=tok".into(),
            })
            .unwrap(),
            serde_json::json!({"room": room_json, "room_token": "tok", "ws_path": "/ws?token=tok"})
        );
        assert_eq!(
            serde_json::to_value(V1RecordingList {
                recordings: vec![V1Recording {
                    id,
                    filename: "f".into(),
                    size_bytes: 7,
                    created_at: t0,
                    room_code: "c".into(),
                    download_url: format!("/api/recordings/{id}/content"),
                }],
            })
            .unwrap(),
            serde_json::json!({"recordings": [{
                "id": id, "filename": "f", "size_bytes": 7, "created_at": t0,
                "room_code": "c", "download_url": format!("/api/recordings/{id}/content"),
            }]})
        );
        assert_eq!(
            serde_json::to_value(V1MeetingList {
                meetings: vec![V1MeetingSummary {
                    id,
                    title: "t".into(),
                    description: "d".into(),
                    kind: "video".into(),
                    starts_at: t0,
                    duration_min: 30,
                    room_code: None,
                    created_at: t0,
                    minutes_ai_at: Some(t0),
                }],
            })
            .unwrap(),
            serde_json::json!({"meetings": [{
                "id": id, "title": "t", "description": "d", "kind": "video",
                "starts_at": t0, "duration_min": 30, "room_code": null,
                "created_at": t0, "minutes_ai_at": t0,
            }]})
        );
        assert_eq!(
            serde_json::to_value(V1MeetingNotes {
                id,
                title: "t".into(),
                minutes: "m".into(),
                transcript: "x".into(),
                minutes_ai_at: None,
            })
            .unwrap(),
            serde_json::json!({"id": id, "title": "t", "minutes": "m", "transcript": "x", "minutes_ai_at": null})
        );
        assert_eq!(
            serde_json::to_value(V1Org {
                id,
                name: "n".into(),
                email_domain: "e".into(),
                domain: "".into(),
                members: 3,
            })
            .unwrap(),
            serde_json::json!({"id": id, "name": "n", "email_domain": "e", "domain": "", "members": 3})
        );
    }

    /// A migração 0046 dá às chaves antigas o catálogo de HOJE, escrito à mão
    /// no SQL. Se o catálogo crescer, este teste lembra que as chaves antigas
    /// NÃO recebem o escopo novo — e que isso é de propósito.
    #[test]
    fn migracao_0046_da_as_chaves_antigas_o_catalogo_inteiro() {
        let sql = include_str!("../migrations/0046_api_key_scopes.sql");
        let default = sql
            .split("ARRAY[")
            .nth(1)
            .and_then(|r| r.split(']').next())
            .unwrap();
        let listed: Vec<String> = default
            .split(',')
            .map(|s| s.trim().trim_matches('\'').to_string())
            .collect();
        assert_eq!(listed, Scope::all_strings());
    }

    #[test]
    fn ct_eq_matches_only_identical() {
        assert!(ct_eq(b"s3cr3t", b"s3cr3t"));
        assert!(!ct_eq(b"s3cr3t", b"s3cr3T"));
        assert!(!ct_eq(b"short", b"longer-secret"));
        assert!(!ct_eq(b"", b"x"));
        // Segredo vazio nunca deve validar (o handler já recusa antes, mas a
        // primitiva também não abre exceção para vazio-vs-vazio em uso real).
        assert!(ct_eq(b"", b""));
    }
}
