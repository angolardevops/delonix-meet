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
use rand::{rngs::OsRng, RngCore};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::sync::Arc;
use uuid::Uuid;

use crate::{auth::AuthUser, error::ApiError, AppState};

fn sha256_hex(s: &str) -> String {
    hex::encode(Sha256::digest(s.as_bytes()))
}

// ---------- Autenticação por chave de API (extractor) ----------

/// Pedido autenticado por chave de API: transporta a organização e o
/// utilizador dono da chave (usado como owner das salas criadas via API).
pub struct ApiKeyAuth {
    pub org_id: Uuid,
    pub owner_id: Uuid,
}

impl FromRequestParts<Arc<AppState>> for ApiKeyAuth {
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &Arc<AppState>,
    ) -> Result<Self, ApiError> {
        let raw = parts
            .headers
            .get("x-api-key")
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string())
            .or_else(|| {
                parts
                    .headers
                    .get(axum::http::header::AUTHORIZATION)
                    .and_then(|v| v.to_str().ok())
                    .and_then(|h| h.strip_prefix("Bearer "))
                    .map(|s| s.to_string())
            })
            .ok_or(ApiError::Unauthorized)?;
        if !raw.starts_with("dlx_") {
            return Err(ApiError::Unauthorized);
        }
        let hash = sha256_hex(&raw);
        let row: Option<(Uuid, Uuid)> =
            sqlx::query_as("SELECT org_id, created_by FROM org_api_keys WHERE key_hash = $1")
                .bind(&hash)
                .fetch_optional(&state.db)
                .await?;
        let (org_id, owner_id) = row.ok_or(ApiError::Unauthorized)?;
        // Regista uso (best-effort) sem falhar o pedido.
        let _ = sqlx::query("UPDATE org_api_keys SET last_used_at = now() WHERE key_hash = $1")
            .bind(&hash)
            .execute(&state.db)
            .await;
        Ok(ApiKeyAuth { org_id, owner_id })
    }
}

// ---------- Gestão das chaves (admin, sessão) ----------

#[derive(Serialize, sqlx::FromRow)]
pub struct ApiKeyInfo {
    pub id: Uuid,
    pub name: String,
    pub prefix: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub last_used_at: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Deserialize)]
pub struct CreateKeyReq {
    #[serde(default)]
    pub name: String,
}

#[derive(Serialize)]
pub struct CreatedKey {
    pub id: Uuid,
    pub name: String,
    pub prefix: String,
    /// A chave completa — só devolvida AGORA, não fica guardada em claro.
    pub key: String,
}

pub async fn list(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(org_id): Path<Uuid>,
) -> Result<Json<Vec<ApiKeyInfo>>, ApiError> {
    crate::org::require_admin_pub(&state, org_id, auth.user_id).await?;
    let keys: Vec<ApiKeyInfo> = sqlx::query_as(
        "SELECT id, name, prefix, created_at, last_used_at FROM org_api_keys
         WHERE org_id = $1 ORDER BY created_at DESC",
    )
    .bind(org_id)
    .fetch_all(&state.db)
    .await?;
    Ok(Json(keys))
}

pub async fn create(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(org_id): Path<Uuid>,
    Json(req): Json<CreateKeyReq>,
) -> Result<Json<CreatedKey>, ApiError> {
    crate::org::require_admin_pub(&state, org_id, auth.user_id).await?;
    let mut bytes = [0u8; 32]; // 256 bits de entropia
    OsRng.fill_bytes(&mut bytes);
    let key = format!("dlx_{}", hex::encode(bytes));
    let prefix = key.chars().take(12).collect::<String>();
    let hash = sha256_hex(&key);
    let name = req.name.trim().chars().take(60).collect::<String>();
    let (id,): (Uuid,) = sqlx::query_as(
        "INSERT INTO org_api_keys (org_id, name, prefix, key_hash, created_by)
         VALUES ($1, $2, $3, $4, $5) RETURNING id",
    )
    .bind(org_id)
    .bind(&name)
    .bind(&prefix)
    .bind(&hash)
    .bind(auth.user_id)
    .fetch_one(&state.db)
    .await?;
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
    }))
}

pub async fn revoke(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path((org_id, key_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<serde_json::Value>, ApiError> {
    crate::org::require_admin_pub(&state, org_id, auth.user_id).await?;
    sqlx::query("DELETE FROM org_api_keys WHERE id = $1 AND org_id = $2")
        .bind(key_id)
        .bind(org_id)
        .execute(&state.db)
        .await?;
    crate::audit::log(
        &state.db,
        Some(org_id),
        auth.user_id,
        "apikey.revoked",
        &key_id.to_string(),
    )
    .await;
    Ok(Json(serde_json::json!({ "ok": true })))
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

#[derive(Deserialize)]
pub struct ApiCreateRoomReq {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub e2ee: bool,
    #[serde(default)]
    pub waiting_room: bool,
}

/// `POST /api/v1/rooms` — cria uma sala e devolve o código + link de entrada.
pub async fn v1_create_room(
    State(state): State<Arc<AppState>>,
    key: ApiKeyAuth,
    Json(req): Json<ApiCreateRoomReq>,
) -> Result<Json<serde_json::Value>, ApiError> {
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
    Ok(Json(serde_json::json!({
        "code": room.code,
        "name": room.name,
        "e2ee": room.e2ee,
        "waiting_room": room.waiting_room,
        "join_url": link,
    })))
}

/// `GET /api/v1/rooms/{code}` — metadados de uma sala.
pub async fn v1_get_room(
    State(state): State<Arc<AppState>>,
    key: ApiKeyAuth,
    Path(code): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
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
    Ok(Json(serde_json::json!({
        "code": code, "name": name, "e2ee": e2ee, "waiting_room": waiting, "join_url": link,
    })))
}

#[derive(Deserialize)]
pub struct JoinBotReq {
    pub bot_name: String,
}

/// `POST /api/v1/rooms/{code}/join-bot` — gera um room_token para um bot headless.
pub async fn v1_join_bot_room(
    State(state): State<Arc<AppState>>,
    key: ApiKeyAuth,
    Path(code): Path<String>,
    Json(req): Json<JoinBotReq>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let room: crate::rooms::Room = sqlx::query_as(
        "SELECT id, code, name, owner_id, topology, waiting_room, e2ee, format, created_at FROM rooms WHERE code = $1",
    )
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

    Ok(Json(serde_json::json!({
        "room": room,
        "room_token": room_token,
        "ws_path": format!("/ws?token={room_token}"),
    })))
}

/// `GET /api/v1/recordings` — gravações da organização (membros).
pub async fn v1_recordings(
    State(state): State<Arc<AppState>>,
    key: ApiKeyAuth,
) -> Result<Json<serde_json::Value>, ApiError> {
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
    let items: Vec<serde_json::Value> = rows
        .into_iter()
        .map(|(id, filename, size, created, code)| {
            serde_json::json!({
                "id": id, "filename": filename, "size_bytes": size,
                "created_at": created, "room_code": code,
                "download_url": format!("/api/recordings/{id}"),
            })
        })
        .collect();
    Ok(Json(serde_json::json!({ "recordings": items })))
}

// ---------- Reuniões (sync de calendário e MoM — nk_delonix_meet) ----------

#[derive(Deserialize)]
pub struct MeetingsQuery {
    /// Cursor incremental: devolve reuniões criadas, com ata AI atualizada ou
    /// com início desde este instante. Omisso => tudo (limitado a 500).
    #[serde(default)]
    pub since: Option<chrono::DateTime<chrono::Utc>>,
}

/// `GET /api/v1/meetings?since=<rfc3339>` — reuniões da organização (dono é
/// membro), incremental para o cron de sync do Odoo (idempotente no cliente).
pub async fn v1_meetings(
    State(state): State<Arc<AppState>>,
    key: ApiKeyAuth,
    axum::extract::Query(q): axum::extract::Query<MeetingsQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
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
    let items: Vec<serde_json::Value> = rows
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
            )| {
                serde_json::json!({
                    "id": id, "title": title, "description": description, "kind": kind,
                    "starts_at": starts_at, "duration_min": duration_min,
                    "room_code": room_code, "created_at": created_at,
                    "minutes_ai_at": minutes_ai_at,
                })
            },
        )
        .collect();
    Ok(Json(serde_json::json!({ "meetings": items })))
}

/// `GET /api/v1/meetings/{id}/notes` — ata (MoM) + transcrição (ata bruta).
/// `minutes_ai_at` presente => o MoM já é a versão final do LLM local.
pub async fn v1_meeting_notes(
    State(state): State<Arc<AppState>>,
    key: ApiKeyAuth,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
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
    Ok(Json(serde_json::json!({
        "id": id, "title": title, "minutes": minutes,
        "transcript": transcript, "minutes_ai_at": minutes_ai_at,
    })))
}

/// `GET /api/v1/org` — dados da organização da chave.
pub async fn v1_org(
    State(state): State<Arc<AppState>>,
    key: ApiKeyAuth,
) -> Result<Json<serde_json::Value>, ApiError> {
    let row: (String, String, String) =
        sqlx::query_as("SELECT name, email_domain, domain FROM organizations WHERE id = $1")
            .bind(key.org_id)
            .fetch_one(&state.db)
            .await?;
    let (members,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM org_members WHERE org_id = $1")
        .bind(key.org_id)
        .fetch_one(&state.db)
        .await?;
    Ok(Json(serde_json::json!({
        "id": key.org_id, "name": row.0, "email_domain": row.1,
        "domain": row.2, "members": members,
    })))
}

// ---------- Provisão de organização (segredo de plataforma) ----------

/// Config OIDC opcional a aplicar à org acabada de criar — deixa o provisionador
/// (Odoo) apontar a org ao seu próprio IdP num só passo, sem um segundo pedido
/// autenticado por sessão de admin (que a org recém-criada ainda não tem).
#[derive(Deserialize)]
pub struct ProvisionSso {
    pub issuer_url: String,
    pub client_id: String,
    pub client_secret: String,
    #[serde(default)]
    pub enforce_sso: bool,
}

#[derive(Deserialize)]
pub struct ProvisionOrgReq {
    /// Nome da organização (== nome da empresa no sistema chamador).
    pub name: String,
    /// Rótulo da chave de API emitida (default "Integration").
    #[serde(default)]
    pub key_name: Option<String>,
    /// Domínio de email da org — necessário para o ``/api/auth/sso/login?domain=``
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

#[derive(Serialize)]
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

/// Comparação em tempo constante para não abrir um oráculo de temporização
/// sobre o segredo de provisão.
fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
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
    let mut pw = [0u8; 24];
    OsRng.fill_bytes(&mut pw);
    let hash = crate::auth::hash_password(&hex::encode(pw))?;
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

/// `POST /api/v1/admin/orgs` — provisiona uma organização + emite a sua chave
/// de API. Autenticado pelo **segredo de plataforma** (`X-Provisioning-Secret`),
/// não por chave de org (que ainda não existe). Pensado para o Odoo criar a org
/// de cada empresa e receber a chave para depois criar salas via `/api/v1/rooms`.
///
/// Fail-closed: com `PROVISIONING_SECRET` vazio o endpoint recusa sempre.
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

    let service_user_id = ensure_provisioning_user(&state).await?;

    // A empresa Odoo já tem organização? (criada por um login SSO anterior,
    // ou por um provisionamento repetido). Reutiliza-se em vez de duplicar —
    // ver o comentário em `odoo_company_id`.
    let existing_org: Option<(Uuid, String)> = match (&req.odoo_db, req.odoo_company_id) {
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
    let base = crate::org::slugify_pub(name);
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
        .bind(req.odoo_db.as_deref().map(str::trim).filter(|d| !d.is_empty()))
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

    // Chave de API da org (mesma geração que apikeys::create).
    let mut bytes = [0u8; 32];
    OsRng.fill_bytes(&mut bytes);
    let key = format!("dlx_{}", hex::encode(bytes));
    let prefix = key.chars().take(12).collect::<String>();
    let hash = sha256_hex(&key);
    let key_name: String = req
        .key_name
        .unwrap_or_else(|| "Integration".into())
        .trim()
        .chars()
        .take(60)
        .collect();
    sqlx::query(
        "INSERT INTO org_api_keys (org_id, name, prefix, key_hash, created_by)
         VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(org_id)
    .bind(&key_name)
    .bind(&prefix)
    .bind(&hash)
    .bind(service_user_id)
    .execute(&state.db)
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

    // Config OIDC (fecha o SSO no mesmo passo — sem SQL manual). Mesmo
    // armazenamento que upsert_sso_config (client_secret em claro).
    let mut sso_configured = false;
    if let Some(sso) = &req.sso {
        let issuer = sso.issuer_url.trim();
        if !issuer.is_empty() && !sso.client_id.trim().is_empty() {
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
            .bind(sso.client_secret.trim())
            .bind(sso.enforce_sso)
            .execute(&state.db)
            .await?;
            sso_configured = true;
        }
    }

    // Integração Odoo — activar e gerar dlxo_ token num só passo.
    let odoo_token = if req.setup_odoo {
        let raw = crate::odoo::gen_token_pub();
        let hash = crate::odoo::sha256_hex_pub(&raw);
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
    use super::ct_eq;

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
