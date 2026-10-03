//! Dial-in PSTN — CONTROL PLANE (Fase 1, sub-fase 1).
//!
//! Vive no backend Rust (sem serviço novo). Gere salas de voz, PINs, inventário
//! de DIDs, participantes e CDRs, agnóstico à camada de media. A camada de media
//! (`freeswitch` self-hosted ou `provider`) é escolhida por organização e será
//! ligada nas sub-fases 2/3 através da API interna de IVR aqui exposta.
//!
//! Isolamento multi-tenant: uma sala de voz pertence a uma org; o par (DID, PIN)
//! é a fronteira lógica ao nível da chamada — um PIN nunca abre a sala de outro
//! tenant (índice único `(did_id, pin)` enquanto ativa + validação escopada).

use axum::{
    extract::{Path, State},
    http::HeaderMap,
    Json,
};
use chrono::{DateTime, Utc};
use rand::Rng;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use uuid::Uuid;

use delonix_meet_core::DomainError;

use crate::{auth::AuthUser, error::ApiError, org::role_in_org, AppState};

// ---------- Enums (persistidos como TEXT) ----------

/// Backend de media escolhido pela org. Impls reais nas sub-fases 2/3.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MediaBackend {
    Freeswitch,
    Provider,
}
impl MediaBackend {
    pub fn as_str(&self) -> &'static str {
        match self {
            MediaBackend::Freeswitch => "freeswitch",
            MediaBackend::Provider => "provider",
        }
    }
    pub fn parse(s: &str) -> MediaBackend {
        match s {
            "provider" => MediaBackend::Provider,
            _ => MediaBackend::Freeswitch, // default seguro (residência)
        }
    }
}

/// Estima o custo (na moeda da tarifa) de uma chamada, arredondando ao minuto.
pub fn estimate_cost(duration_secs: i64, tariff_per_min: f64) -> f64 {
    let minutes = ((duration_secs.max(0) + 59) / 60) as f64; // ceil
    (minutes * tariff_per_min * 10_000.0).round() / 10_000.0
}

// ---------- Tipos de saída ----------

/// Documentação OpenAPI do control plane de voz (`openapi.rs` junta-a). A API
/// interna de IVR (`/internal/v1/voice/ivr/*`) fica de fora: o contrato dela é o
/// `.proto`.
#[derive(utoipa::OpenApi)]
#[openapi(
    paths(
        create_room,
        get_room,
        list_participants,
        close_room,
        list_dids,
        create_did,
        list_cdr,
        billing_summary
    ),
    components(schemas(
        VoiceRoom,
        VoiceRoomResp,
        CreateVoiceRoomReq,
        VoiceParticipant,
        VoiceDid,
        CreateDidReq,
        VoiceCdr,
        BillingSummary
    ))
)]
pub struct ApiDoc;

#[derive(Serialize, sqlx::FromRow, utoipa::ToSchema)]
pub struct VoiceRoom {
    pub id: Uuid,
    pub org_id: Uuid,
    pub room_code: String,
    /// PIN de 6 dígitos para o dial-in.
    pub pin: String,
    pub did_id: Option<Uuid>,
    /// `freeswitch` | `provider`.
    pub media_backend: String,
    /// `active` | `closed`.
    pub status: String,
    pub created_at: DateTime<Utc>,
}

/// Estava copiada à mão em `create_room` e `get_room` (ADR-0004, mesmo
/// padrão de `meetings::MEETING_COLUMNS`).
const VOICE_ROOM_COLUMNS: &str =
    "id, org_id, room_code, pin, did_id, media_backend, status, created_at";

#[derive(Serialize, sqlx::FromRow, utoipa::ToSchema)]
pub struct VoiceParticipant {
    pub id: Uuid,
    pub channel: String,
    pub caller_number: String,
    pub joined_at: DateTime<Utc>,
    pub left_at: Option<DateTime<Utc>>,
}

#[derive(Serialize, sqlx::FromRow, utoipa::ToSchema)]
pub struct VoiceDid {
    pub id: Uuid,
    /// `null` = pool partilhado entre organizações.
    pub org_id: Option<Uuid>,
    /// Número em +E.164.
    pub e164: String,
    pub market: String,
    /// `shared` | `dedicated`.
    pub model: String,
    pub provider: String,
    pub active: bool,
    pub created_at: DateTime<Utc>,
    /// Ramal a que este número está permanentemente atribuído (Fase 2, ver
    /// `ramais.rs::assign_extension_did`) — `None` enquanto o número está
    /// livre para dial-in por PIN ou por atribuir. Não é uma FK gerida por
    /// `voice.rs`; só exposta aqui para a consola saber que números já não
    /// estão disponíveis para uma sala de voz efémera.
    pub extension_id: Option<Uuid>,
}

/// Estava copiada à mão em `create_did` e `list_dids` (ADR-0004).
const VOICE_DID_COLUMNS: &str =
    "id, org_id, e164, market, model, provider, active, created_at, extension_id";

#[derive(Serialize, sqlx::FromRow, utoipa::ToSchema)]
pub struct VoiceCdr {
    pub id: Uuid,
    pub direction: String,
    pub caller_number: String,
    pub did_e164: String,
    pub duration_secs: i32,
    pub cost_estimate: f64,
    pub started_at: DateTime<Utc>,
    pub ended_at: Option<DateTime<Utc>>,
}

// ---------- Helpers ----------

/// Como ligar para uma sala de conferência por telefone.
#[derive(Debug, Serialize, sqlx::FromRow, utoipa::ToSchema)]
pub struct DialIn {
    /// Número em +E.164.
    pub number: String,
    /// PIN de 6 dígitos.
    pub pin: String,
}

/// O dial-in ACTIVO ligado à sala `room_code` por uma das organizações
/// `org_ids` — só leitura: não cria sala de voz nem escolhe DID. `None` quando
/// não há sala de voz activa com DID activo. Com várias, a mais recente.
pub(crate) async fn dial_in_for_room(
    state: &AppState,
    org_ids: &[Uuid],
    room_code: &str,
) -> Result<Option<DialIn>, ApiError> {
    if org_ids.is_empty() {
        return Ok(None);
    }
    Ok(sqlx::query_as(
        "SELECT d.e164 AS number, vr.pin
           FROM voice_room vr JOIN voice_did d ON d.id = vr.did_id
          WHERE vr.room_code = $1 AND vr.org_id = ANY($2)
            AND vr.status = 'active' AND d.active
          ORDER BY vr.created_at DESC LIMIT 1",
    )
    .bind(room_code)
    .bind(org_ids)
    .fetch_optional(&state.db)
    .await?)
}

fn gen_pin() -> String {
    let mut rng = rand::thread_rng();
    format!("{:06}", rng.gen_range(0..1_000_000))
}

/// A sala de conferência `room_code` é da organização `org_id`: o DONO da sala
/// é membro ACTIVO dela.
///
/// Sem isto, o admin de uma org ligava um DID+PIN seu ao código da sala de
/// OUTRA org, e o IVR (HTTP e gRPC partilham `validate_pin`) punha chamadores
/// PSTN dentro dessa reunião (R140). Regra escolhida por ser a mais restritiva
/// das que o `rooms::room_access` conhece: «colega do dono». O convite na
/// agenda e o co-anfitrião NÃO contam — dão acesso a uma pessoa, não fazem da
/// sala um recurso da org. Inexistente e alheia dão a mesma resposta.
async fn ensure_room_in_org(
    state: &AppState,
    org_id: Uuid,
    room_code: &str,
) -> Result<(), ApiError> {
    let not_found = || ApiError::Domain(DomainError::not_found("voice.room_not_found"));
    let owner: Option<Uuid> = sqlx::query_scalar("SELECT owner_id FROM rooms WHERE code = $1")
        .bind(room_code)
        .fetch_optional(&state.db)
        .await?;
    let owner = owner.ok_or_else(not_found)?;
    match role_in_org(state, org_id, owner).await? {
        Some(_) => Ok(()),
        None => Err(not_found()),
    }
}

// ============================================================
//  API do utilizador (autenticada por sessão, escopada à org)
// ============================================================

#[derive(Deserialize, utoipa::ToSchema)]
pub struct CreateVoiceRoomReq {
    /// Código da sala de conferência existente (rooms.code) a ligar ao dial-in.
    pub room_code: String,
    /// DID a usar; se omitido, o control plane escolhe segundo o modelo da org.
    #[serde(default)]
    pub did_id: Option<Uuid>,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct VoiceRoomResp {
    pub id: Uuid,
    pub room_code: String,
    pub pin: String,
    pub dial_in_number: Option<String>,
    pub media_backend: String,
}

/// Cria uma sala de voz (dial-in) para uma sala de conferência existente.
///
/// A sala de voz pertence à primeira organização do utilizador. O `room_code`
/// é normalizado para minúsculas e tem de ser de uma sala cujo DONO é membro
/// ACTIVO dessa organização; senão `404` (`voice.room_not_found`), a mesma
/// resposta de um código inexistente.
#[utoipa::path(
    post, path = "/api/orgs/{org_id}/voice/rooms", tag = "voice",
    security(("session" = [])),
    params(("org_id" = Uuid, Path)),
    request_body = CreateVoiceRoomReq,
    responses(
        (status = 200, body = VoiceRoomResp),
        (status = 400, description = "Utilizador sem organização ou `room_code` vazio.", body = crate::openapi::ErrorBody),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 404, description = "`voice.room_not_found`: a sala não existe, ou o dono não é membro activo da organização de quem pede.", body = crate::openapi::ErrorBody),
        (status = 409, description = "Sem DID disponível para dial-in nesta organização.", body = crate::openapi::ErrorBody),
    )
)]
pub async fn create_room(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(org_id): Path<Uuid>,
    Json(req): Json<CreateVoiceRoomReq>,
) -> Result<Json<VoiceRoomResp>, ApiError> {
    // A org vem do caminho e quem pede tem de ser membro activo dela (404 se
    // não). Antes era «a primeira org do utilizador», escolhida às cegas.
    if role_in_org(&state, org_id, auth.user_id).await?.is_none() {
        return Err(ApiError::NotFound);
    }
    let room_code = req.room_code.trim().to_lowercase();
    if room_code.is_empty() {
        return Err(ApiError::BadRequest("room_code em falta".into()));
    }
    ensure_room_in_org(&state, org_id, &room_code).await?;

    // Backend de media e modelo de DID vêm da configuração da org.
    let (backend, did_model): (String, String) = sqlx::query_as(
        "SELECT voice_media_backend, voice_did_model FROM organizations WHERE id = $1",
    )
    .bind(org_id)
    .fetch_one(&state.db)
    .await?;
    let backend = MediaBackend::parse(&backend).as_str().to_string();

    // Resolver o DID: explícito (validado), dedicado da org, ou do pool partilhado.
    // `extension_id IS NULL` nas três queries (migração 0065_ramais_did.sql):
    // um DID já atribuído a um ramal (server/src/ramais.rs::assign_extension_did)
    // é permanente e alcançado DIRECTAMENTE, sem PIN — não pode ser reaproveitado
    // aqui para uma sala de voz efémera, ou o mesmo número passaria a ambiguar
    // entre "toca o ramal" e "pede PIN".
    let did: Option<VoiceDid> = if let Some(id) = req.did_id {
        sqlx::query_as(&format!(
            "SELECT {VOICE_DID_COLUMNS}
             FROM voice_did
             WHERE id = $1 AND active AND extension_id IS NULL AND (org_id = $2 OR org_id IS NULL)"
        ))
        .bind(id)
        .bind(org_id)
        .fetch_optional(&state.db)
        .await?
    } else if did_model == "dedicated" {
        sqlx::query_as(&format!(
            "SELECT {VOICE_DID_COLUMNS}
             FROM voice_did
             WHERE org_id = $1 AND active AND extension_id IS NULL
             ORDER BY created_at LIMIT 1"
        ))
        .bind(org_id)
        .fetch_optional(&state.db)
        .await?
    } else {
        // Modelo partilhado: primeiro um dedicado da org, senão o pool partilhado.
        sqlx::query_as(&format!(
            "SELECT {VOICE_DID_COLUMNS}
             FROM voice_did
             WHERE active AND extension_id IS NULL AND (org_id = $1 OR org_id IS NULL)
             ORDER BY (org_id = $1) DESC, created_at LIMIT 1"
        ))
        .bind(org_id)
        .fetch_optional(&state.db)
        .await?
    };
    let did = did.ok_or_else(|| {
        ApiError::Conflict("sem DID disponível para dial-in nesta organização".into())
    })?;

    // Gera um PIN único para (DID, sala ativa); retenta em colisão.
    let mut last_err = None;
    for _ in 0..8 {
        let pin = gen_pin();
        let res: Result<VoiceRoom, sqlx::Error> = sqlx::query_as(&format!(
            "INSERT INTO voice_room (org_id, room_code, pin, did_id, media_backend, created_by)
             VALUES ($1, $2, $3, $4, $5, $6)
             RETURNING {VOICE_ROOM_COLUMNS}"
        ))
        .bind(org_id)
        .bind(&room_code)
        .bind(&pin)
        .bind(did.id)
        .bind(&backend)
        .bind(auth.user_id)
        .fetch_one(&state.db)
        .await;
        match res {
            Ok(vr) => {
                return Ok(Json(VoiceRoomResp {
                    id: vr.id,
                    room_code: vr.room_code,
                    pin: vr.pin,
                    dial_in_number: Some(did.e164),
                    media_backend: vr.media_backend,
                }))
            }
            Err(sqlx::Error::Database(dbe)) if dbe.is_unique_violation() => continue,
            Err(e) => last_err = Some(e),
        }
    }
    Err(last_err
        .map(Into::into)
        .unwrap_or_else(|| ApiError::internal("não foi possível gerar PIN")))
}

/// Detalhes de uma sala de voz (membro da org dona). Inclui o PIN.
#[utoipa::path(
    get, path = "/api/orgs/{org_id}/voice/rooms/{voice_room_id}", tag = "voice",
    security(("session" = [])),
    params(("org_id" = Uuid, Path), ("voice_room_id" = Uuid, Path)),
    responses(
        (status = 200, body = VoiceRoom),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 404, description = "Inexistente ou de outra organização.", body = crate::openapi::ErrorBody),
    )
)]
pub async fn get_room(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path((org_id, id)): Path<(Uuid, Uuid)>,
) -> Result<Json<VoiceRoom>, ApiError> {
    if role_in_org(&state, org_id, auth.user_id).await?.is_none() {
        return Err(ApiError::NotFound);
    }
    let vr: VoiceRoom = sqlx::query_as(&format!(
        "SELECT {VOICE_ROOM_COLUMNS} FROM voice_room WHERE id = $1 AND org_id = $2"
    ))
    .bind(id)
    .bind(org_id)
    .fetch_optional(&state.db)
    .await?
    .ok_or(ApiError::NotFound)?;
    Ok(Json(vr))
}

/// Participantes de uma sala de voz (membro da org dona).
#[utoipa::path(
    get, path = "/api/orgs/{org_id}/voice/rooms/{voice_room_id}/participants", tag = "voice",
    security(("session" = [])),
    params(("org_id" = Uuid, Path), ("voice_room_id" = Uuid, Path)),
    responses(
        (status = 200, body = Vec<VoiceParticipant>),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 404, description = "Inexistente ou de outra organização.", body = crate::openapi::ErrorBody),
    )
)]
pub async fn list_participants(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path((org_id, id)): Path<(Uuid, Uuid)>,
) -> Result<Json<Vec<VoiceParticipant>>, ApiError> {
    if role_in_org(&state, org_id, auth.user_id).await?.is_none() {
        return Err(ApiError::NotFound);
    }
    let exists: Option<(i32,)> =
        sqlx::query_as("SELECT 1 FROM voice_room WHERE id = $1 AND org_id = $2")
            .bind(id)
            .bind(org_id)
            .fetch_optional(&state.db)
            .await?;
    exists.ok_or(ApiError::NotFound)?;
    let parts: Vec<VoiceParticipant> = sqlx::query_as(
        "SELECT id, channel, caller_number, joined_at, left_at
         FROM voice_participant WHERE voice_room_id = $1 ORDER BY joined_at",
    )
    .bind(id)
    .fetch_all(&state.db)
    .await?;
    Ok(Json(parts))
}

/// Encerra uma sala de voz (o PIN deixa de ser válido). Só quem a CRIOU ou um
/// admin da org dona; outro membro recebe `403` (`voice.room_close_forbidden`).
/// Idempotente.
#[utoipa::path(
    post, path = "/api/orgs/{org_id}/voice/rooms/{voice_room_id}/close", tag = "voice",
    security(("session" = [])),
    params(("org_id" = Uuid, Path), ("voice_room_id" = Uuid, Path)),
    responses(
        (status = 204, description = "Encerrada."),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 403, description = "`voice.room_close_forbidden`: membro da org, mas nem criador da sala de voz nem admin.", body = crate::openapi::ErrorBody),
        (status = 404, description = "Inexistente ou de outra organização.", body = crate::openapi::ErrorBody),
    )
)]
pub async fn close_room(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path((org_id, id)): Path<(Uuid, Uuid)>,
) -> Result<axum::http::StatusCode, ApiError> {
    let (owner_org, created_by): (Uuid, Uuid) =
        sqlx::query_as("SELECT org_id, created_by FROM voice_room WHERE id = $1 AND org_id = $2")
            .bind(id)
            .bind(org_id)
            .fetch_optional(&state.db)
            .await?
            .ok_or(ApiError::NotFound)?;
    // Quem não é membro activo da org dona não sabe que a sala existe (404).
    // Dentro da org, encerrar corta a chamada de TODOS os participantes PSTN:
    // é do criador ou de um admin, não de qualquer colega (R141).
    match role_in_org(&state, owner_org, auth.user_id).await? {
        None => return Err(ApiError::NotFound),
        Some(role) if role != "admin" && created_by != auth.user_id => {
            return Err(DomainError::forbidden("voice.room_close_forbidden").into());
        }
        Some(_) => {}
    }
    sqlx::query("UPDATE voice_room SET status = 'closed', closed_at = now() WHERE id = $1 AND status = 'active'")
        .bind(id)
        .execute(&state.db)
        .await?;
    sqlx::query(
        "UPDATE voice_participant SET left_at = now() WHERE voice_room_id = $1 AND left_at IS NULL",
    )
    .bind(id)
    .execute(&state.db)
    .await?;
    Ok(axum::http::StatusCode::NO_CONTENT)
}

// ---------- Inventário de DIDs (admin) ----------

#[derive(Deserialize, utoipa::ToSchema)]
pub struct CreateDidReq {
    /// `+` seguido do número; 8–20 caracteres.
    pub e164: String,
    /// Omissão `AO`.
    #[serde(default = "default_market")]
    pub market: String,
    /// `dedicated`; qualquer outro valor conta como `shared` (omissão).
    #[serde(default = "default_model")]
    pub model: String,
    #[serde(default)]
    pub provider: String,
    /// Se `shared`, pode ficar sem org (pool). Se ausente, atribui à org do path.
    #[serde(default)]
    pub org_scoped: Option<bool>,
}
fn default_market() -> String {
    "AO".into()
}
fn default_model() -> String {
    "shared".into()
}

/// Adiciona um DID ao inventário de uma org (admin).
///
/// Com `model = shared` e `org_scoped` falso/ausente o DID vai para o pool
/// PARTILHADO (`org_id = null`), visível a todas as organizações — e isso só o
/// administrador da PLATAFORMA (`PLATFORM_ADMIN_USER_IDS`) pode fazer; um admin
/// de org recebe `403` (`voice.shared_did_requires_platform_admin`) e cria DIDs
/// só da sua org (`org_scoped: true` ou `model: dedicated`).
#[utoipa::path(
    post, path = "/api/orgs/{org_id}/voice/dids", tag = "voice",
    security(("session" = [])),
    params(("org_id" = Uuid, Path)),
    request_body = CreateDidReq,
    responses(
        (status = 200, body = VoiceDid),
        (status = 400, description = "Número fora do formato +E.164.", body = crate::openapi::ErrorBody),
        (status = 401, description = "Sessão inválida OU membro sem papel de admin.", body = crate::openapi::ErrorBody),
        (status = 403, description = "`voice.shared_did_requires_platform_admin`: o pool partilhado é da plataforma.", body = crate::openapi::ErrorBody),
        (status = 404, description = "Não é membro da organização.", body = crate::openapi::ErrorBody),
        (status = 409, description = "Número já existe no inventário.", body = crate::openapi::ErrorBody),
    )
)]
pub async fn create_did(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(org_id): Path<Uuid>,
    Json(req): Json<CreateDidReq>,
) -> Result<Json<VoiceDid>, ApiError> {
    crate::org::require_admin_pub(&state, org_id, auth.user_id).await?;
    let e164 = req.e164.trim();
    if !e164.starts_with('+') || e164.len() < 8 || e164.len() > 20 {
        return Err(ApiError::BadRequest(
            "número deve estar em formato +E.164".into(),
        ));
    }
    let model = if req.model == "dedicated" {
        "dedicated"
    } else {
        "shared"
    };
    // shared + org_scoped=false => pool partilhado (org_id NULL).
    let scoped = req.org_scoped.unwrap_or(model == "dedicated");
    // O pool partilhado serve o dial-in de TODAS as organizações: um número
    // lá posto por um inquilino passava a atender chamadas de outros. Só a
    // plataforma o gere (R141). A recusa vem antes de escrever.
    if !scoped {
        crate::storage::require_platform_admin(&state, auth.user_id).map_err(|_| {
            ApiError::from(DomainError::forbidden(
                "voice.shared_did_requires_platform_admin",
            ))
        })?;
    }
    let did: VoiceDid = sqlx::query_as(&format!(
        "INSERT INTO voice_did (org_id, e164, market, model, provider)
         VALUES ($1, $2, $3, $4, $5)
         RETURNING {VOICE_DID_COLUMNS}"
    ))
    .bind(if scoped { Some(org_id) } else { None })
    .bind(e164)
    .bind(req.market.trim())
    .bind(model)
    .bind(req.provider.trim())
    .fetch_one(&state.db)
    .await
    .map_err(|e| match e {
        sqlx::Error::Database(dbe) if dbe.is_unique_violation() => {
            ApiError::Conflict("esse número já existe no inventário".into())
        }
        other => other.into(),
    })?;
    Ok(Json(did))
}

/// Lista os DIDs visíveis a uma org (dedicados + pool partilhado). Admin.
#[utoipa::path(
    get, path = "/api/orgs/{org_id}/voice/dids", tag = "voice",
    security(("session" = [])),
    params(("org_id" = Uuid, Path)),
    responses(
        (status = 200, body = Vec<VoiceDid>),
        (status = 401, description = "Sessão inválida OU membro sem papel de admin.", body = crate::openapi::ErrorBody),
        (status = 404, description = "Não é membro da organização.", body = crate::openapi::ErrorBody),
    )
)]
pub async fn list_dids(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(org_id): Path<Uuid>,
) -> Result<Json<Vec<VoiceDid>>, ApiError> {
    crate::org::require_admin_pub(&state, org_id, auth.user_id).await?;
    let dids: Vec<VoiceDid> = sqlx::query_as(&format!(
        "SELECT {VOICE_DID_COLUMNS} FROM voice_did WHERE org_id = $1 OR org_id IS NULL ORDER BY created_at DESC"
    ))
    .bind(org_id)
    .fetch_all(&state.db)
    .await?;
    Ok(Json(dids))
}

/// CDRs da org para billing/auditoria (admin). Os 500 mais recentes.
#[utoipa::path(
    get, path = "/api/orgs/{org_id}/voice/call-records", tag = "voice",
    security(("session" = [])),
    params(("org_id" = Uuid, Path)),
    responses(
        (status = 200, body = Vec<VoiceCdr>),
        (status = 401, description = "Sessão inválida OU membro sem papel de admin.", body = crate::openapi::ErrorBody),
        (status = 404, description = "Não é membro da organização.", body = crate::openapi::ErrorBody),
    )
)]
pub async fn list_cdr(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(org_id): Path<Uuid>,
) -> Result<Json<Vec<VoiceCdr>>, ApiError> {
    crate::org::require_admin_pub(&state, org_id, auth.user_id).await?;
    let rows: Vec<VoiceCdr> = sqlx::query_as(
        "SELECT id, direction, caller_number, did_e164, duration_secs, cost_estimate, started_at, ended_at
         FROM voice_cdr WHERE org_id = $1 ORDER BY started_at DESC LIMIT 500",
    )
    .bind(org_id)
    .fetch_all(&state.db)
    .await?;
    Ok(Json(rows))
}

#[derive(Deserialize, utoipa::IntoParams)]
#[into_params(parameter_in = Query)]
pub struct BillingQuery {
    /// `week` (7 dias) | `month` (30, omissão) | `quarter` (90) | `year` (365).
    /// Um valor desconhecido conta como 30 dias e é ecoado tal como veio.
    #[serde(default = "default_period")]
    pub period: String,
}
fn default_period() -> String {
    "month".into()
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct BillingSummary {
    pub period: String,
    pub calls: i64,
    pub total_minutes: i64,
    pub total_cost: f64,
    pub currency_note: String,
}

/// Resumo de billing de voz do período (admin) — alimenta a faturação Delonix.
#[utoipa::path(
    get, path = "/api/orgs/{org_id}/voice/billing", tag = "voice",
    security(("session" = [])),
    params(("org_id" = Uuid, Path), BillingQuery),
    responses(
        (status = 200, body = BillingSummary),
        (status = 401, description = "Sessão inválida OU membro sem papel de admin.", body = crate::openapi::ErrorBody),
        (status = 404, description = "Não é membro da organização.", body = crate::openapi::ErrorBody),
    )
)]
pub async fn billing_summary(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(org_id): Path<Uuid>,
    axum::extract::Query(q): axum::extract::Query<BillingQuery>,
) -> Result<Json<BillingSummary>, ApiError> {
    crate::org::require_admin_pub(&state, org_id, auth.user_id).await?;
    let days: i64 = match q.period.as_str() {
        "week" => 7,
        "quarter" => 90,
        "year" => 365,
        _ => 30,
    };
    // minutos = soma do arredondamento ao minuto de cada chamada (coerente com o CDR).
    let row: (Option<i64>, Option<i64>, Option<f64>) = sqlx::query_as(
        "SELECT COUNT(*),
                SUM((duration_secs + 59) / 60)::bigint,
                SUM(cost_estimate)
         FROM voice_cdr
         WHERE org_id = $1 AND started_at >= now() - make_interval(days => $2::int)",
    )
    .bind(org_id)
    .bind(days as i32)
    .fetch_one(&state.db)
    .await?;
    Ok(Json(BillingSummary {
        period: q.period,
        calls: row.0.unwrap_or(0),
        total_minutes: row.1.unwrap_or(0),
        total_cost: row.2.unwrap_or(0.0),
        currency_note: "custo estimado à tarifa VOICE_TARIFF_INBOUND; billing recalcula".into(),
    }))
}

// ============================================================
//  API interna de IVR (chamada pela camada de media)
//  Autenticada por segredo partilhado (X-Voice-Secret).
// ============================================================

pub(crate) fn check_media_secret(state: &AppState, headers: &HeaderMap) -> Result<(), ApiError> {
    authorize_media_secret(
        &state.config.voice_internal_secret,
        state.config.voice_secret_refusal,
        headers,
    )
}

/// A decisão de `check_media_secret`, sem `AppState`, para se poder testar.
///
/// Ordem deliberada (R154): primeiro o SEGREDO CONFIGURADO, depois o cabeçalho.
/// Um segredo ausente, curto ou publicado dá `503` com a razão, venha o
/// cabeçalho que vier — incluindo o valor certo, porque «certo» contra um
/// valor que está no GitHub não autentica ninguém. Antes, vazio dava `404` e
/// um valor publicado passava.
fn authorize_media_secret(
    configured: &str,
    refusal: Option<&'static str>,
    headers: &HeaderMap,
) -> Result<(), ApiError> {
    if let Some(reason) = refusal {
        return Err(ApiError::ServiceUnavailable(format!(
            "API interna de IVR desligada: {reason}"
        )));
    }
    if configured.is_empty() {
        // Defesa em profundidade: `voice_secret_refusal` já recusa o vazio.
        return Err(ApiError::ServiceUnavailable(
            "API interna de IVR desligada: VOICE_INTERNAL_SECRET não está definido".into(),
        ));
    }
    // Duas formas do MESMO segredo: o cabeçalho `X-Voice-Secret` (Lua do IVR)
    // e HTTP Basic com o segredo como password. O utilizador do Basic não conta.
    let header = headers
        .get("x-voice-secret")
        .and_then(|v| v.to_str().ok())
        .map(|v| v.as_bytes().to_vec());
    // O Basic é o que o `mod_json_cdr` (`cred`) e o `mod_xml_curl`
    // (`gateway-credentials`) do FreeSWITCH sabem enviar (ADR-0009). Chega aqui
    // DEPOIS das recusas de configuração acima (R154): um segredo ausente ou
    // publicado recusa por `503` venha o Basic que vier.
    let basic = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Basic "))
        .and_then(|b64| {
            use base64::Engine as _;
            base64::engine::general_purpose::STANDARD
                .decode(b64.trim())
                .ok()
        })
        .and_then(|raw| {
            let pos = raw.iter().position(|b| *b == b':')?;
            Some(raw[pos + 1..].to_vec())
        });
    match header.or(basic) {
        Some(got) if delonix_meet_core::crypto::ct_eq(&got, configured.as_bytes()) => Ok(()),
        _ => Err(ApiError::Unauthorized),
    }
}

#[derive(Deserialize)]
pub struct ValidatePinReq {
    pub did_e164: String,
    pub pin: String,
}

/// Ponte de media telefone↔sala (ADR-0010) — para onde o FreeSWITCH faz
/// `bridge` para que o chamador PSTN entre na sala WebRTC.
///
/// **Sucede à `PstnBridgeResp` da Abordagem B**, que devolvia um `host:porta`
/// de ingress SRTP e as chaves por fora. Esse mecanismo — o FreeSWITCH a
/// mandar SRTP cru para um endereço arbitrário — nunca se conseguiu verificar,
/// e a imagem oficial 1.11.3 não tem `mod_rtp` (medido: ver
/// `docs/pstn-sfu-bridge-design.md` §Superseded). O que o FreeSWITCH de stock
/// SABE fazer é uma segunda perna SIP, e é isso que estes campos descrevem.
///
/// Os nomes são o contrato com `voice/freeswitch/scripts/dialin_ivr.lua` —
/// mudar um lado sem o outro deixa a ponte silenciosamente inactiva (o IVR cai
/// na conferência local).
#[derive(Serialize)]
pub struct RoomBridgeResp {
    /// URI completo para `bridge`: `sofia/<perfil>/sip:room-<code>@<host:porta>`
    /// monta-se no Lua a partir daqui.
    pub sip_uri: String,
    /// Variáveis de canal a pôr ANTES do `bridge`. Traz
    /// `rtp_secure_media=mandatory:AES_CM_128_HMAC_SHA1_80`: a ponte recusa
    /// (`488`) uma oferta sem `a=crypto`, por isso o FreeSWITCH tem de oferecer
    /// SRTP. As chaves são negociadas NO SDP dessa perna, por chamada — não
    /// viajam neste JSON nem nas variáveis de canal (ver `phone_bridge::srtp`).
    pub channel_vars: std::collections::BTreeMap<String, String>,
    /// Nome do perfil SRTP exigido (hoje sempre "AES_CM_128_HMAC_SHA1_80").
    pub srtp_profile: String,
}

#[derive(Serialize)]
pub struct ValidatePinResp {
    pub voice_room_id: Uuid,
    pub room_code: String,
    pub media_backend: String,
    /// `Some` só quando o backend é `freeswitch` E a ponte está configurada
    /// (`PHONE_BRIDGE_SIP_BIND` e a allowlist — ver `config.rs`) E existe uma
    /// `rooms.code = room_code`. `None` mantém o comportamento de sempre: o
    /// IVR cai na conferência local do FreeSWITCH, SEM áudio WebRTC.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub room_bridge: Option<RoomBridgeResp>,
}

/// Valida (DID, PIN) → devolve a sala a que o chamador PSTN deve ser ligado.
/// Fronteira de isolamento: só encontra salas ATIVAS cujo DID corresponde.
pub async fn ivr_validate_pin(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(req): Json<ValidatePinReq>,
) -> Result<Json<ValidatePinResp>, ApiError> {
    check_media_secret(&state, &headers)?;
    validate_pin(&state, &req.did_e164, &req.pin)
        .await
        .map(Json)
}

/// A regra do IVR, partilhada pelo HTTP (`/internal/v1/voice/ivr/validate`) e pelo gRPC
/// (`IvrService.ValidatePin`). Fronteira de isolamento: só encontra salas
/// ATIVAS cujo DID corresponde.
pub(crate) async fn validate_pin(
    state: &AppState,
    did_e164: &str,
    pin: &str,
) -> Result<ValidatePinResp, ApiError> {
    let did = did_e164.trim();
    let row: Option<(Uuid, String, String)> = sqlx::query_as(
        "SELECT vr.id, vr.room_code, vr.media_backend
         FROM voice_room vr JOIN voice_did d ON d.id = vr.did_id
         WHERE d.e164 = $1 AND vr.pin = $2 AND vr.status = 'active'",
    )
    .bind(did)
    .bind(pin.trim())
    .fetch_optional(&state.db)
    .await?;
    match row {
        Some((id, room_code, backend)) => {
            let room_bridge = room_bridge_for(state, &room_code, &backend).await;
            Ok(ValidatePinResp {
                voice_room_id: id,
                room_code,
                media_backend: backend,
                room_bridge,
            })
        }
        None => {
            // Anti-toll-fraud / PIN-guessing: só as FALHAS contam para o limite;
            // chamadores legítimos com PIN certo nunca são penalizados.
            if !state.voice_pin_limiter.check(did) {
                tracing::warn!(did = %did, "possível brute-force de PIN no dial-in — a bloquear");
                return Err(ApiError::TooManyRequests);
            }
            Err(ApiError::NotFound)
        }
    }
}

/// Para onde o IVR deve fazer `bridge` para meter esta chamada na sala. Falha
/// SEMPRE em silêncio (log + `None`, nunca um erro que derrube a validação do
/// PIN): um chamador tem de conseguir entrar mesmo que a ponte não esteja
/// pronta — o IVR já sabe cair na conferência local quando este campo vem
/// ausente (ver `dialin_ivr.lua`). As razões para `None`, todas esperadas e
/// não-erros do ponto de vista do dial-in: backend não é `freeswitch` (o
/// `provider` faz media à parte), `PHONE_BRIDGE_SIP_BIND` não configurado,
/// allowlist vazia (fail-closed), ou `room_code` sem sala WebRTC em `rooms`.
async fn room_bridge_for(
    state: &AppState,
    room_code: &str,
    backend: &str,
) -> Option<RoomBridgeResp> {
    if MediaBackend::parse(backend) != MediaBackend::Freeswitch {
        return None;
    }
    let Some(bind) = state.config.phone_bridge_sip_bind else {
        tracing::warn!(
            "PHONE_BRIDGE_SIP_BIND não configurado — ponte telefone↔sala desligada, dial-in cai na conferência local"
        );
        return None;
    };
    if state.config.phone_bridge_freeswitch_ips.is_empty() {
        tracing::warn!(
            "PHONE_BRIDGE_FREESWITCH_IPS vazio — a ponte recusaria o INVITE (fail-closed); dial-in cai na conferência local"
        );
        return None;
    }
    let existe: Option<Uuid> = sqlx::query_scalar("SELECT id FROM rooms WHERE code = $1")
        .bind(room_code)
        .fetch_optional(&state.db)
        .await
        .ok()
        .flatten();
    if existe.is_none() {
        tracing::warn!(
            room_code,
            "ponte telefone↔sala: sala WebRTC ainda não existe para este room_code"
        );
        return None;
    }
    Some(RoomBridgeResp {
        sip_uri: format!("sip:room-{room_code}@{}", bridge_advertise(state, bind)),
        channel_vars: [
            (
                "rtp_secure_media".to_string(),
                format!("mandatory:{}", crate::phone_bridge::srtp::SRTP_PROFILE_NAME),
            ),
            // A ponte só transcodifica G.711: uma oferta sem PCMA/PCMU leva
            // `488`. É também o que a perna levava na prova contra o
            // FreeSWITCH real — o caminho do cliente não é uma variante por
            // medir do que foi medido.
            ("absolute_codec_string".to_string(), "PCMA".to_string()),
        ]
        .into_iter()
        .collect(),
        srtp_profile: crate::phone_bridge::srtp::SRTP_PROFILE_NAME.to_string(),
    })
}

/// `host:porta` que o FreeSWITCH usa para alcançar o UA SIP. O
/// `PHONE_BRIDGE_SIP_ADVERTISE` ganha (em K8s o Service e o bind não
/// coincidem); sem ele, o host do `PSTN_BRIDGE_HOST` com a porta do bind.
pub(crate) fn bridge_advertise(state: &AppState, bind: std::net::SocketAddr) -> String {
    match &state.config.phone_bridge_sip_advertise {
        Some(a) if !a.trim().is_empty() => a.trim().to_string(),
        _ => format!("{}:{}", state.config.pstn_bridge_host, bind.port()),
    }
}

/// Quem entra na ponte: `room-<code>` → a sala do SFU com esse `rooms.code`.
///
/// É a MESMA resolução que o `room_bridge_for` faz ao devolver o URI ao IVR —
/// aqui repete-se porque o `INVITE` chega bem depois, e a sala pode ter
/// desaparecido entretanto. Sem sala, `None`: o UA responde `404` e o
/// FreeSWITCH cai na conferência local (o IVR já sabe fazê-lo).
struct DialInAdmission {
    db: sqlx::PgPool,
}

#[async_trait::async_trait]
impl crate::phone_bridge::sip::BridgeAdmission for DialInAdmission {
    async fn admit(
        &self,
        room_code: &str,
        _call_id: Option<Uuid>,
    ) -> Option<crate::phone_bridge::sip::Admitted> {
        let room_id: Uuid = sqlx::query_scalar("SELECT id FROM rooms WHERE code = $1")
            .bind(room_code)
            .fetch_optional(&self.db)
            .await
            .ok()
            .flatten()?;
        Some(crate::phone_bridge::sip::Admitted {
            room_id,
            leg_id: Uuid::new_v4(),
        })
    }
}

/// Arranca o UA SIP da ponte telefone↔sala (ADR-0010), se estiver configurado.
///
/// **Fail-closed em duas frentes:** sem `PHONE_BRIDGE_SIP_BIND` não se abre
/// socket nenhum, e com a allowlist vazia não se abre também — um UA que
/// aceitasse `INVITE` de qualquer origem é uma porta para dentro das salas.
/// Em qualquer dos casos o dial-in continua a funcionar: o `room_bridge_for`
/// devolve `None` pelas mesmas razões e o IVR cai na conferência local.
///
/// Um erro a abrir o socket NÃO derruba o servidor — a videoconferência não
/// depende desta ponte.
pub(crate) async fn start_phone_bridge(state: &Arc<AppState>) {
    let Some(sip_bind) = state.config.phone_bridge_sip_bind else {
        return;
    };
    if state.config.phone_bridge_freeswitch_ips.is_empty() {
        tracing::warn!(
            "PHONE_BRIDGE_SIP_BIND definido mas PHONE_BRIDGE_FREESWITCH_IPS vazio — ponte telefone↔sala NÃO arranca (fail-closed)"
        );
        return;
    }
    let cfg = crate::phone_bridge::sip::SipBridgeConfig {
        sip_bind,
        rtp_ip: state.config.phone_bridge_rtp_ip.unwrap_or(sip_bind.ip()),
        rtp_ports: state.config.phone_bridge_rtp_ports,
        allowed_sources: state.config.phone_bridge_freeswitch_ips.clone(),
    };
    // A fila dos eventos é limitada: um pico de chamadas não pode crescer
    // memória sem tecto. O UA larga eventos quando ela enche — são
    // observabilidade, não o caminho da media.
    let (tx, mut rx) = tokio::sync::mpsc::channel(256);
    match crate::phone_bridge::sip::SipBridge::start(
        cfg,
        state.sfu.clone(),
        Arc::new(DialInAdmission {
            db: state.db.clone(),
        }),
        tx,
    )
    .await
    {
        Ok(b) => {
            tracing::info!(
                sip = %b.local_sip,
                anuncia = %bridge_advertise(state, sip_bind),
                origens = state.config.phone_bridge_freeswitch_ips.len(),
                "ponte telefone↔sala à escuta"
            );
            // A ponte é quem impõe o `ForceMute` a quem não tem cliente (R224).
            let _ = state
                .hub
                .phone
                .set(b.clone() as Arc<dyn crate::signaling::PhoneControl>);
            let st = state.clone();
            let metrics = state.metrics.clone();
            tokio::spawn(async move {
                while let Some(ev) = rx.recv().await {
                    use crate::phone_bridge::sip::BridgeEvent;
                    match &ev {
                        // A chamada passa a ser gente na sala: aparece no censo,
                        // com o crachá do telefone e o número mascarado. Sem
                        // isto, um anfitrião não a vê — logo não a modera.
                        BridgeEvent::Started {
                            leg_id, room_id, ..
                        } => {
                            // O telefone não tem WebSocket: o lado receptor é
                            // drenado e deitado fora. A fila existe só porque o
                            // censo a exige para toda a gente.
                            let (tx, mut rx_peer, _sd) =
                                crate::signaling::PeerTx::new(16, metrics.clone());
                            tokio::spawn(async move { while rx_peer.recv().await.is_some() {} });
                            st.hub.join_external(
                                *room_id,
                                *leg_id,
                                "Telefone".to_string(),
                                crate::signaling::Seat {
                                    channel:
                                        delonix_meet_domain::conferencing::channels::Channel::Phone,
                                    anonymous: true,
                                    video_unavailable: true,
                                    ..Default::default()
                                },
                                tx,
                            );
                        }
                        // A «ligação fraca» é MEDIDA no RTP da perna, não
                        // adivinhada: o crachá acende e apaga com ela.
                        BridgeEvent::Leg {
                            leg_id,
                            room_id,
                            event: crate::phone_bridge::leg::LegEvent::Quality { weak, .. },
                        } => {
                            let weak = *weak;
                            st.hub.update_external(*room_id, *leg_id, |seat, _, _| {
                                seat.weak_link = weak;
                            });
                        }
                        BridgeEvent::Ended { leg_id, room_id } => st.hub.leave(*room_id, *leg_id),
                        BridgeEvent::Leg { .. } => {}
                    }
                    tracing::info!(?ev, "ponte telefone↔sala");
                }
            });
        }
        Err(e) => tracing::error!(
            %sip_bind, error = %e,
            "ponte telefone↔sala não arrancou — o dial-in cai na conferência local"
        ),
    }
}

#[derive(Deserialize)]
pub struct CdrReq {
    pub voice_room_id: Uuid,
    #[serde(default = "inbound")]
    pub direction: String,
    #[serde(default)]
    pub caller_number: String,
    #[serde(default)]
    pub did_e164: String,
    pub duration_secs: i64,
}
pub(crate) fn inbound() -> String {
    "inbound".into()
}

/// Regista um CDR no fim de uma chamada (chamado pela camada de media).
pub async fn ivr_record_cdr(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(req): Json<CdrReq>,
) -> Result<Json<serde_json::Value>, ApiError> {
    check_media_secret(&state, &headers)?;
    let (id, cost) = record_cdr(&state, &req).await?;
    Ok(Json(serde_json::json!({ "id": id, "cost_estimate": cost })))
}

/// Regista o CDR — partilhada pelo HTTP e pelo gRPC (`RecordCallDetail`).
pub(crate) async fn record_cdr(state: &AppState, req: &CdrReq) -> Result<(Uuid, f64), ApiError> {
    let org_id: Uuid = sqlx::query_scalar("SELECT org_id FROM voice_room WHERE id = $1")
        .bind(req.voice_room_id)
        .fetch_one(&state.db)
        .await?;
    let cost = estimate_cost(req.duration_secs, state.config.voice_tariff_inbound);
    let id: Uuid = sqlx::query_scalar(
        "INSERT INTO voice_cdr
             (org_id, voice_room_id, direction, caller_number, did_e164, duration_secs, cost_estimate, ended_at)
         VALUES ($1, $2, $3, $4, $5, $6, $7, now()) RETURNING id",
    )
    .bind(org_id)
    .bind(req.voice_room_id)
    .bind(&req.direction)
    .bind(req.caller_number.trim())
    .bind(req.did_e164.trim())
    .bind(req.duration_secs as i32)
    .bind(cost)
    .fetch_one(&state.db)
    .await?;
    Ok((id, cost))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pin_is_six_digits() {
        for _ in 0..100 {
            let p = gen_pin();
            assert_eq!(p.len(), 6);
            assert!(p.chars().all(|c| c.is_ascii_digit()));
        }
    }

    #[test]
    fn cost_rounds_up_to_the_minute() {
        assert_eq!(estimate_cost(0, 10.0), 0.0);
        assert_eq!(estimate_cost(1, 10.0), 10.0); // 1s → 1 min
        assert_eq!(estimate_cost(60, 10.0), 10.0);
        assert_eq!(estimate_cost(61, 10.0), 20.0); // 61s → 2 min
        assert_eq!(estimate_cost(600, 2.5), 25.0);
    }

    fn with_secret(v: &str) -> HeaderMap {
        let mut h = HeaderMap::new();
        h.insert("x-voice-secret", v.parse().unwrap());
        h
    }

    fn status(r: Result<(), ApiError>) -> u16 {
        match r {
            Ok(()) => 200,
            Err(e) => axum::response::IntoResponse::into_response(e)
                .status()
                .as_u16(),
        }
    }

    const STRONG: &str = "3f9c1a7e0b5d4c2a8e6f1b3d5a7c9e0f2b4d6a8c";

    fn with_auth(v: &str) -> HeaderMap {
        let mut h = HeaderMap::new();
        h.insert(axum::http::header::AUTHORIZATION, v.parse().unwrap());
        h
    }

    fn b64(raw: &str) -> String {
        use base64::Engine as _;
        base64::engine::general_purpose::STANDARD.encode(raw)
    }

    /// R227 — o FreeSWITCH (`mod_xml_curl`, `mod_json_cdr`) manda o segredo por
    /// HTTP Basic, como password. Só essa forma passa: nem o segredo como
    /// utilizador, nem outro esquema, nem um Basic certo atrás de um
    /// `X-Voice-Secret` errado, nem um Basic certo contra um segredo recusado.
    #[test]
    fn ivr_basic_takes_the_secret_only_as_the_password() {
        let ok = |h: &HeaderMap| status(authorize_media_secret(STRONG, None, h));

        assert_eq!(
            ok(&with_auth(&format!(
                "Basic {}",
                b64(&format!("freeswitch:{STRONG}"))
            ))),
            200
        );
        // O utilizador não conta, e a password pode ter `:` — corta-se no primeiro.
        assert_eq!(
            ok(&with_auth(&format!("Basic {}", b64(&format!(":{STRONG}"))))),
            200
        );
        let with_colon = "a:b-segredo-com-dois-pontos-0123456789";
        let h = with_auth(&format!("Basic {}", b64(&format!("fs:{with_colon}"))));
        assert_eq!(status(authorize_media_secret(with_colon, None, &h)), 200);

        for (what, value) in [
            (
                "password errada",
                format!("Basic {}", b64("freeswitch:errado")),
            ),
            ("sem dois pontos", format!("Basic {}", b64(STRONG))),
            (
                "segredo como utilizador",
                format!("Basic {}", b64(&format!("{STRONG}:"))),
            ),
            ("base64 inválido", "Basic ###".to_string()),
            (
                "esquema em minúsculas",
                format!("basic {}", b64(&format!("fs:{STRONG}"))),
            ),
            ("Bearer com o segredo", format!("Bearer {STRONG}")),
            ("o segredo cru", STRONG.to_string()),
        ] {
            assert_eq!(ok(&with_auth(&value)), 401, "{what}");
        }

        // Um `X-Voice-Secret` errado não é salvo por um Basic certo.
        let mut both = with_secret("errado");
        both.insert(
            axum::http::header::AUTHORIZATION,
            format!("Basic {}", b64(&format!("fs:{STRONG}")))
                .parse()
                .unwrap(),
        );
        assert_eq!(ok(&both), 401);

        // R154 ganha a tudo: um segredo publicado dá 503 mesmo com o Basic certo.
        let burned = "voice-internal-secret-for-pstn";
        let refusal = crate::config::voice_secret_refusal(burned, false);
        let h = with_auth(&format!("Basic {}", b64(&format!("fs:{burned}"))));
        assert_eq!(status(authorize_media_secret(burned, refusal, &h)), 503);
    }

    /// R154 — sem segredo, ou com o segredo publicado no repositório, as rotas
    /// de IVR dão 503 com razão. Mesmo quem manda o valor «certo».
    #[test]
    fn ivr_refuses_missing_short_or_burned_secret_with_503() {
        use crate::config::voice_secret_refusal;
        for configured in [
            "",
            "curto-demais",
            "voice-internal-secret-for-pstn",
            "dev-voice-secret-abc123",
        ] {
            let refusal = voice_secret_refusal(configured, false);
            assert!(refusal.is_some(), "«{configured}» devia ser recusado");
            let r = authorize_media_secret(
                configured,
                refusal,
                &with_secret(if configured.is_empty() {
                    "x"
                } else {
                    configured
                }),
            );
            assert_eq!(
                status(r),
                503,
                "«{configured}» com o próprio valor no cabeçalho"
            );
            assert_eq!(
                status(authorize_media_secret(
                    configured,
                    refusal,
                    &HeaderMap::new()
                )),
                503
            );
        }
        // Todos os valores queimados são recusados, e a razão é a lista e não
        // o comprimento (ver a razão devolvida): um valor publicado futuro com
        // 32+ caracteres também não pode passar.
        for burned in crate::config::BURNED_VOICE_SECRETS {
            let r = voice_secret_refusal(burned, false).unwrap_or_default();
            assert!(r.contains("publicado"), "«{burned}»: {r}");
        }
        // Defesa em profundidade: vazio sem razão calculada também é 503.
        assert_eq!(
            status(authorize_media_secret("", None, &with_secret(""))),
            503
        );
    }

    #[test]
    fn ivr_rejects_wrong_or_absent_header_with_401() {
        let refusal = crate::config::voice_secret_refusal(STRONG, false);
        assert_eq!(refusal, None);
        assert_eq!(
            status(authorize_media_secret(STRONG, refusal, &HeaderMap::new())),
            401
        );
        assert_eq!(
            status(authorize_media_secret(
                STRONG,
                refusal,
                &with_secret("errado")
            )),
            401
        );
        // Prefixo do certo: o comprimento conta.
        assert_eq!(
            status(authorize_media_secret(
                STRONG,
                refusal,
                &with_secret(&STRONG[..31])
            )),
            401
        );
        let mut quase = STRONG.to_string();
        quase.replace_range(39..40, "1");
        assert_eq!(
            status(authorize_media_secret(
                STRONG,
                refusal,
                &with_secret(&quase)
            )),
            401
        );
    }

    #[test]
    fn ivr_accepts_the_right_strong_secret() {
        let refusal = crate::config::voice_secret_refusal(STRONG, false);
        assert_eq!(
            status(authorize_media_secret(
                STRONG,
                refusal,
                &with_secret(STRONG)
            )),
            200
        );
    }

    /// `DELONIX_ALLOW_INSECURE=1` mantém o valor de dev do Makefile a funcionar,
    /// mas nunca um segredo vazio.
    #[test]
    fn insecure_dev_keeps_the_dev_value_but_not_empty() {
        use crate::config::voice_secret_refusal;
        let dev = "dev-voice-secret-abc123";
        let refusal = voice_secret_refusal(dev, true);
        assert_eq!(refusal, None);
        assert_eq!(
            status(authorize_media_secret(dev, refusal, &with_secret(dev))),
            200
        );
        assert_eq!(
            status(authorize_media_secret(dev, refusal, &with_secret("outro"))),
            401
        );
        assert!(voice_secret_refusal("", true).is_some());
    }

    #[test]
    fn media_backend_defaults_to_freeswitch() {
        assert_eq!(MediaBackend::parse("provider").as_str(), "provider");
        assert_eq!(MediaBackend::parse("freeswitch").as_str(), "freeswitch");
        assert_eq!(MediaBackend::parse("qualquer").as_str(), "freeswitch"); // default residência
    }
}
