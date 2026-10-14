//! Inventário de captura de TV por HDMI (RFC-0002, Fase 1; ADR-0031; migração
//! 0121) — adaptador HTTP + Postgres. As regras de forma, e a identidade de
//! uma entrada, estão em `delonix_meet_domain::content::tv_capture`.
//!
//! Duas superfícies, dois públicos (ADR-0004 §4):
//!
//! - **Consola** (sessão, `broadcast.manage_channels`): criar a caixa, ver o
//!   que ela reportou, etiquetar uma entrada e ligá-la a um canal.
//!   `GET|POST /api/orgs/{org_id}/tv/capture-agents`,
//!   `GET|PATCH|DELETE …/{agent_id}`, `POST …/{agent_id}/revoke`,
//!   `GET …/{agent_id}/inputs`,
//!   `GET|PATCH /api/orgs/{org_id}/tv/capture-inputs/{input_id}`.
//! - **Agente** (token `dlxtv_`): `PUT /api/integrations/tv-agent/v1/inputs`,
//!   que SUBSTITUI o inventário da caixa.
//!
//! O que este módulo **não** faz: media. Não há publicação de sinal nem fila de
//! sintonia — ambas esperam a Fase 0 da RFC (que hardware responde, e como),
//! e um canal só fica no ar pelo plano de controlo de `tv_broadcasts.rs`.
//!
//! **O servidor não confia no agente para emparelhar o áudio.** O agente
//! reporta o que viu — as entradas de vídeo e, em separado, os nós de áudio com
//! o caminho físico de cada um — e é aqui que o par se decide, pela PORTA
//! (`tv_capture::pair_audio`). Um agente que apontasse o áudio da porta errada
//! daria um canal com a voz de outro descodificador, e isso não é detectável a
//! olho.

use axum::{
    extract::{FromRequestParts, Path, Query, State},
    http::{header::LOCATION, request::Parts, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use chrono::{DateTime, Utc};
use delonix_meet_core::{
    page::{Page, PageRequest},
    DomainError,
};
use delonix_meet_domain::content::tv_capture as rules;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use uuid::Uuid;

use crate::{auth::AuthUser, error::ApiError, AppState};

const AGENT_COLUMNS: &str = "id, org_id, name, location, token_prefix, agent_version, \
                             created_by, created_at, last_seen_at, revoked_at";
const INPUT_COLUMNS: &str = "id, agent_id, org_id, device_path, label, vendor_id, product_id, \
                             serial, video_node, audio_node, capabilities, signal, tuner_kind, \
                             tuner_config, channel_id, version, created_at, updated_at, \
                             last_seen_at, last_signal_at";

/// Uma caixa de captura. **O token não vem aqui** — só na resposta que a cria.
#[derive(Debug, Clone, Serialize, sqlx::FromRow, utoipa::ToSchema)]
pub struct TvCaptureAgent {
    pub id: Uuid,
    pub org_id: Uuid,
    pub name: String,
    pub location: String,
    /// O início do token, para a pessoa reconhecer qual é sem o ver todo.
    pub token_prefix: String,
    /// O que o agente diz ser; nulo até ao primeiro relatório.
    pub agent_version: Option<String>,
    pub created_by: Uuid,
    pub created_at: DateTime<Utc>,
    pub last_seen_at: Option<DateTime<Utc>>,
    /// Preenchido = o token já não autentica.
    pub revoked_at: Option<DateTime<Utc>>,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct TvCaptureAgentPage {
    pub items: Vec<TvCaptureAgent>,
    pub next_page_token: Option<String>,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct CreatedTvCaptureAgent {
    #[serde(flatten)]
    pub agent: TvCaptureAgent,
    /// O token `dlxtv_` do agente. **Sai uma vez e não volta**: guarda-se o
    /// SHA-256. Perdido, revoga-se este e cria-se outro.
    pub token: String,
}

#[derive(Deserialize, utoipa::ToSchema)]
pub struct CreateAgentReq {
    pub name: String,
    #[serde(default)]
    pub location: Option<String>,
}

#[derive(Deserialize, utoipa::ToSchema)]
pub struct UpdateAgentReq {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub location: Option<String>,
}

/// Uma entrada física: a porta onde um descodificador está ligado.
#[derive(Debug, Clone, Serialize, sqlx::FromRow, utoipa::ToSchema)]
pub struct TvCaptureInput {
    pub id: Uuid,
    pub agent_id: Uuid,
    pub org_id: Uuid,
    /// O caminho físico do barramento: a chave estável (ADR-0031 §2).
    pub device_path: String,
    pub label: String,
    pub vendor_id: String,
    pub product_id: String,
    pub serial: Option<String>,
    /// O nó do SO agora. Informativo: muda com a ordem de ligação.
    pub video_node: Option<String>,
    /// O áudio do MESMO capturador, decidido aqui pela porta.
    pub audio_node: Option<String>,
    #[schema(value_type = Object)]
    pub capabilities: serde_json::Value,
    /// `live | no_signal | protected | error` — `protected` é o caso HDCP.
    pub signal: String,
    pub tuner_kind: String,
    #[schema(value_type = Object)]
    pub tuner_config: serde_json::Value,
    pub channel_id: Option<Uuid>,
    pub version: i32,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub last_seen_at: DateTime<Utc>,
    pub last_signal_at: Option<DateTime<Utc>>,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct TvCaptureInputPage {
    pub items: Vec<TvCaptureInput>,
    pub next_page_token: Option<String>,
}

#[derive(Deserialize, utoipa::ToSchema)]
pub struct UpdateInputReq {
    /// A versão que o cliente viu. Velha = `409` e nada se escreve.
    pub version: i32,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub tuner_kind: Option<String>,
    #[serde(default)]
    #[schema(value_type = Option<Object>)]
    pub tuner_config: Option<serde_json::Value>,
    /// Liga esta entrada a um canal da organização. `null` desliga.
    #[serde(default, deserialize_with = "crate::tv_capture::some_nullable")]
    #[schema(value_type = Option<String>)]
    pub channel_id: Option<Option<Uuid>>,
}

/// Distingue «campo ausente» de «campo a `null`» — o primeiro não mexe, o
/// segundo limpa.
pub fn some_nullable<'de, D, T>(d: D) -> Result<Option<Option<T>>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(d).map(Some)
}

#[derive(Deserialize, utoipa::IntoParams)]
#[into_params(parameter_in = Query)]
pub struct ListQuery {
    /// 1-100, omissão 50.
    pub page_size: Option<u32>,
    pub page_token: Option<String>,
}

#[derive(Serialize, Deserialize)]
struct Cursor {
    at: DateTime<Utc>,
    id: Uuid,
}

/// `broadcast.manage_channels` (ADR-0008 §4): gerir caixas de captura é
/// PREPARAR um canal, e por isso não ganha capacidade nova.
async fn require_manage(state: &AppState, org_id: Uuid, user: Uuid) -> Result<(), ApiError> {
    crate::org::require_capability(
        state,
        org_id,
        user,
        delonix_meet_domain::identity::authorization::Capability::BroadcastManageChannels,
        delonix_meet_domain::identity::authorization::ResourceScope::Organization,
    )
    .await
    .map(|_| ())
}

#[derive(utoipa::OpenApi)]
#[openapi(
    paths(
        list_agents,
        create_agent,
        get_agent,
        update_agent,
        delete_agent,
        revoke_agent,
        list_inputs,
        get_input,
        update_input,
        agent_put_inputs
    ),
    components(schemas(
        TvCaptureAgent,
        TvCaptureAgentPage,
        CreatedTvCaptureAgent,
        CreateAgentReq,
        UpdateAgentReq,
        TvCaptureInput,
        TvCaptureInputPage,
        UpdateInputReq,
        InventoryReq,
        InputReport,
        AudioReport,
        InventoryResp,
        AgentInputView
    ))
)]
pub struct ApiDoc;

// ============================================================
//  Consola
// ============================================================

/// As caixas de captura da organização, paginadas.
#[utoipa::path(
    get, path = "/api/orgs/{org_id}/tv/capture-agents", tag = "tv-capture",
    security(("session" = [])),
    params(("org_id" = Uuid, Path), ListQuery),
    responses(
        (status = 200, body = TvCaptureAgentPage),
        (status = 400, body = crate::openapi::ErrorBody, description = "page_token inválido"),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 404, body = crate::openapi::ErrorBody),
    )
)]
pub async fn list_agents(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(org_id): Path<Uuid>,
    Query(q): Query<ListQuery>,
) -> Result<Json<TvCaptureAgentPage>, ApiError> {
    require_manage(&state, org_id, auth.user_id).await?;
    let page = PageRequest {
        page_size: q.page_size,
        page_token: q.page_token,
    };
    let size = page.size();
    let cursor: Option<Cursor> = page.cursor()?;
    let rows: Vec<TvCaptureAgent> = sqlx::query_as(&format!(
        "SELECT {AGENT_COLUMNS} FROM tv_capture_agents
          WHERE org_id = $1
            AND ($2::timestamptz IS NULL OR (created_at, id) > ($2, $3))
          ORDER BY created_at, id
          LIMIT $4"
    ))
    .bind(org_id)
    .bind(cursor.as_ref().map(|c| c.at))
    .bind(cursor.as_ref().map(|c| c.id).unwrap_or_default())
    .bind(size as i64 + 1)
    .fetch_all(&state.db)
    .await?;
    let p = Page::from_overfetch(rows, size, |a| Cursor {
        at: a.created_at,
        id: a.id,
    });
    Ok(Json(TvCaptureAgentPage {
        items: p.items,
        next_page_token: p.next_page_token,
    }))
}

/// Cria uma caixa e devolve o token do agente — **uma vez**.
#[utoipa::path(
    post, path = "/api/orgs/{org_id}/tv/capture-agents", tag = "tv-capture",
    security(("session" = [])),
    params(("org_id" = Uuid, Path)),
    request_body = CreateAgentReq,
    responses(
        (status = 201, body = CreatedTvCaptureAgent, headers(("Location" = String)),
         description = "O token `dlxtv_` sai UMA vez."),
        (status = 400, body = crate::openapi::ErrorBody),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 404, body = crate::openapi::ErrorBody),
    )
)]
pub async fn create_agent(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(org_id): Path<Uuid>,
    Json(req): Json<CreateAgentReq>,
) -> Result<Response, ApiError> {
    require_manage(&state, org_id, auth.user_id).await?;
    let name = rules::validate_name(&req.name)?;
    let location = rules::validate_location(req.location.as_deref().unwrap_or(""))?;

    let token = crate::crypto::random_token("dlxtv_");
    let prefix: String = token.chars().take(14).collect();
    let id = Uuid::new_v4();
    let agent: TvCaptureAgent = sqlx::query_as(&format!(
        "INSERT INTO tv_capture_agents (id, org_id, name, location, token_prefix, token_hash, created_by)
         VALUES ($1, $2, $3, $4, $5, $6, $7) RETURNING {AGENT_COLUMNS}"
    ))
    .bind(id)
    .bind(org_id)
    .bind(&name)
    .bind(&location)
    .bind(&prefix)
    .bind(crate::crypto::sha256_hex(&token))
    .bind(auth.user_id)
    .fetch_one(&state.db)
    .await?;
    crate::audit::log(
        &state.db,
        Some(org_id),
        auth.user_id,
        "tv.capture_agent.created",
        &format!("{id} ({name})"),
    )
    .await;
    let location_header = format!("/api/orgs/{org_id}/tv/capture-agents/{id}");
    Ok((
        StatusCode::CREATED,
        [(LOCATION, location_header)],
        Json(CreatedTvCaptureAgent { agent, token }),
    )
        .into_response())
}

async fn fetch_agent(state: &AppState, org_id: Uuid, id: Uuid) -> Result<TvCaptureAgent, ApiError> {
    sqlx::query_as(&format!(
        "SELECT {AGENT_COLUMNS} FROM tv_capture_agents WHERE id = $1 AND org_id = $2"
    ))
    .bind(id)
    .bind(org_id)
    .fetch_optional(&state.db)
    .await?
    .ok_or(ApiError::NotFound)
}

/// Uma caixa.
#[utoipa::path(
    get, path = "/api/orgs/{org_id}/tv/capture-agents/{agent_id}", tag = "tv-capture",
    security(("session" = [])),
    params(("org_id" = Uuid, Path), ("agent_id" = Uuid, Path)),
    responses(
        (status = 200, body = TvCaptureAgent),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 404, body = crate::openapi::ErrorBody),
    )
)]
pub async fn get_agent(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path((org_id, agent_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<TvCaptureAgent>, ApiError> {
    require_manage(&state, org_id, auth.user_id).await?;
    Ok(Json(fetch_agent(&state, org_id, agent_id).await?))
}

/// Altera o nome ou o sítio da caixa. O token não se altera: revoga-se.
#[utoipa::path(
    patch, path = "/api/orgs/{org_id}/tv/capture-agents/{agent_id}", tag = "tv-capture",
    security(("session" = [])),
    params(("org_id" = Uuid, Path), ("agent_id" = Uuid, Path)),
    request_body = UpdateAgentReq,
    responses(
        (status = 200, body = TvCaptureAgent),
        (status = 400, body = crate::openapi::ErrorBody),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 404, body = crate::openapi::ErrorBody),
    )
)]
pub async fn update_agent(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path((org_id, agent_id)): Path<(Uuid, Uuid)>,
    Json(req): Json<UpdateAgentReq>,
) -> Result<Json<TvCaptureAgent>, ApiError> {
    require_manage(&state, org_id, auth.user_id).await?;
    let atual = fetch_agent(&state, org_id, agent_id).await?;
    let name = match req.name.as_deref() {
        Some(n) => rules::validate_name(n)?,
        None => atual.name,
    };
    let location = match req.location.as_deref() {
        Some(l) => rules::validate_location(l)?,
        None => atual.location,
    };
    let agent: TvCaptureAgent = sqlx::query_as(&format!(
        "UPDATE tv_capture_agents SET name = $3, location = $4
          WHERE id = $1 AND org_id = $2 RETURNING {AGENT_COLUMNS}"
    ))
    .bind(agent_id)
    .bind(org_id)
    .bind(&name)
    .bind(&location)
    .fetch_optional(&state.db)
    .await?
    .ok_or(ApiError::NotFound)?;
    Ok(Json(agent))
}

/// Apaga a caixa. Leva o inventário dela (`CASCADE`), e com ele as ligações a
/// canais: um canal alimentado por esta caixa fica sem fonte.
#[utoipa::path(
    delete, path = "/api/orgs/{org_id}/tv/capture-agents/{agent_id}", tag = "tv-capture",
    security(("session" = [])),
    params(("org_id" = Uuid, Path), ("agent_id" = Uuid, Path)),
    responses(
        (status = 204, description = "Apagada."),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 404, body = crate::openapi::ErrorBody),
    )
)]
pub async fn delete_agent(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path((org_id, agent_id)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, ApiError> {
    require_manage(&state, org_id, auth.user_id).await?;
    let n = sqlx::query("DELETE FROM tv_capture_agents WHERE id = $1 AND org_id = $2")
        .bind(agent_id)
        .bind(org_id)
        .execute(&state.db)
        .await?
        .rows_affected();
    if n == 0 {
        return Err(ApiError::NotFound);
    }
    crate::audit::log(
        &state.db,
        Some(org_id),
        auth.user_id,
        "tv.capture_agent.deleted",
        &agent_id.to_string(),
    )
    .await;
    Ok(StatusCode::NO_CONTENT)
}

/// Revoga o token sem perder o inventário — é o que se faz a um token
/// comprometido, para a configuração dos canais ficar de pé. Repetir é
/// inofensivo: a data da primeira revogação mantém-se.
#[utoipa::path(
    post, path = "/api/orgs/{org_id}/tv/capture-agents/{agent_id}/revoke", tag = "tv-capture",
    security(("session" = [])),
    params(("org_id" = Uuid, Path), ("agent_id" = Uuid, Path)),
    responses(
        (status = 200, body = TvCaptureAgent),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 404, body = crate::openapi::ErrorBody),
    )
)]
pub async fn revoke_agent(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path((org_id, agent_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<TvCaptureAgent>, ApiError> {
    require_manage(&state, org_id, auth.user_id).await?;
    let agent: TvCaptureAgent = sqlx::query_as(&format!(
        "UPDATE tv_capture_agents SET revoked_at = COALESCE(revoked_at, now())
          WHERE id = $1 AND org_id = $2 RETURNING {AGENT_COLUMNS}"
    ))
    .bind(agent_id)
    .bind(org_id)
    .fetch_optional(&state.db)
    .await?
    .ok_or(ApiError::NotFound)?;
    crate::audit::log(
        &state.db,
        Some(org_id),
        auth.user_id,
        "tv.capture_agent.revoked",
        &agent_id.to_string(),
    )
    .await;
    Ok(Json(agent))
}

/// O inventário de uma caixa, paginado.
#[utoipa::path(
    get, path = "/api/orgs/{org_id}/tv/capture-agents/{agent_id}/inputs", tag = "tv-capture",
    security(("session" = [])),
    params(("org_id" = Uuid, Path), ("agent_id" = Uuid, Path), ListQuery),
    responses(
        (status = 200, body = TvCaptureInputPage),
        (status = 400, body = crate::openapi::ErrorBody),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 404, body = crate::openapi::ErrorBody),
    )
)]
pub async fn list_inputs(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path((org_id, agent_id)): Path<(Uuid, Uuid)>,
    Query(q): Query<ListQuery>,
) -> Result<Json<TvCaptureInputPage>, ApiError> {
    require_manage(&state, org_id, auth.user_id).await?;
    // Um agente de outra organização dá `404` igual a um que não existe.
    fetch_agent(&state, org_id, agent_id).await?;
    let page = PageRequest {
        page_size: q.page_size,
        page_token: q.page_token,
    };
    let size = page.size();
    let cursor: Option<Cursor> = page.cursor()?;
    let rows: Vec<TvCaptureInput> = sqlx::query_as(&format!(
        "SELECT {INPUT_COLUMNS} FROM tv_capture_inputs
          WHERE org_id = $1 AND agent_id = $2
            AND ($3::timestamptz IS NULL OR (created_at, id) > ($3, $4))
          ORDER BY created_at, id
          LIMIT $5"
    ))
    .bind(org_id)
    .bind(agent_id)
    .bind(cursor.as_ref().map(|c| c.at))
    .bind(cursor.as_ref().map(|c| c.id).unwrap_or_default())
    .bind(size as i64 + 1)
    .fetch_all(&state.db)
    .await?;
    let p = Page::from_overfetch(rows, size, |i| Cursor {
        at: i.created_at,
        id: i.id,
    });
    Ok(Json(TvCaptureInputPage {
        items: p.items,
        next_page_token: p.next_page_token,
    }))
}

async fn fetch_input(state: &AppState, org_id: Uuid, id: Uuid) -> Result<TvCaptureInput, ApiError> {
    sqlx::query_as(&format!(
        "SELECT {INPUT_COLUMNS} FROM tv_capture_inputs WHERE id = $1 AND org_id = $2"
    ))
    .bind(id)
    .bind(org_id)
    .fetch_optional(&state.db)
    .await?
    .ok_or(ApiError::NotFound)
}

/// Uma entrada.
#[utoipa::path(
    get, path = "/api/orgs/{org_id}/tv/capture-inputs/{input_id}", tag = "tv-capture",
    security(("session" = [])),
    params(("org_id" = Uuid, Path), ("input_id" = Uuid, Path)),
    responses(
        (status = 200, body = TvCaptureInput),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 404, body = crate::openapi::ErrorBody),
    )
)]
pub async fn get_input(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path((org_id, input_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<TvCaptureInput>, ApiError> {
    require_manage(&state, org_id, auth.user_id).await?;
    Ok(Json(fetch_input(&state, org_id, input_id).await?))
}

/// Etiqueta a entrada, escolhe o caminho de sintonia e liga-a a um canal.
///
/// O `device_path`, os nós e o sinal **não** se alteram por aqui: são
/// observação do agente, não decisão da pessoa.
#[utoipa::path(
    patch, path = "/api/orgs/{org_id}/tv/capture-inputs/{input_id}", tag = "tv-capture",
    security(("session" = [])),
    params(("org_id" = Uuid, Path), ("input_id" = Uuid, Path)),
    request_body = UpdateInputReq,
    responses(
        (status = 200, body = TvCaptureInput),
        (status = 400, body = crate::openapi::ErrorBody),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 404, body = crate::openapi::ErrorBody),
        (status = 409, body = crate::openapi::ErrorBody,
         description = "`tv.capture.version_conflict` ou `tv.capture.channel_taken`"),
    )
)]
pub async fn update_input(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path((org_id, input_id)): Path<(Uuid, Uuid)>,
    Json(req): Json<UpdateInputReq>,
) -> Result<Json<TvCaptureInput>, ApiError> {
    require_manage(&state, org_id, auth.user_id).await?;
    let atual = fetch_input(&state, org_id, input_id).await?;

    let label = match req.label.as_deref() {
        Some(l) => rules::validate_label(l)?,
        None => atual.label,
    };
    let tuner_kind = match req.tuner_kind.as_deref() {
        Some(k) => rules::TunerKind::parse(k)?.as_str().to_string(),
        None => atual.tuner_kind,
    };
    let tuner_config = req.tuner_config.unwrap_or(atual.tuner_config);
    let channel_id = match req.channel_id {
        Some(Some(c)) => {
            // Um canal de outra organização é indistinguível de um inexistente.
            let existe: bool = sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM tv_channels WHERE id = $1 AND org_id = $2)",
            )
            .bind(c)
            .bind(org_id)
            .fetch_one(&state.db)
            .await?;
            if !existe {
                return Err(ApiError::NotFound);
            }
            Some(c)
        }
        Some(None) => None,
        None => atual.channel_id,
    };

    let updated: Option<TvCaptureInput> = sqlx::query_as(&format!(
        "UPDATE tv_capture_inputs
            SET label = $4, tuner_kind = $5, tuner_config = $6, channel_id = $7,
                version = version + 1, updated_at = now()
          WHERE id = $1 AND org_id = $2 AND version = $3
          RETURNING {INPUT_COLUMNS}"
    ))
    .bind(input_id)
    .bind(org_id)
    .bind(req.version)
    .bind(&label)
    .bind(&tuner_kind)
    .bind(&tuner_config)
    .bind(channel_id)
    .fetch_optional(&state.db)
    .await
    .map_err(|e| {
        if e.as_database_error()
            .and_then(|d| d.constraint())
            .is_some_and(|c| c == "tv_capture_inputs_channel_key")
        {
            DomainError::conflict(
                "tv.capture.channel_taken",
                "este canal já é alimentado por outra entrada",
            )
            .with_field("channel_id", "já em uso")
            .into()
        } else {
            ApiError::from(e)
        }
    })?;
    match updated {
        Some(i) => Ok(Json(i)),
        None => Err(DomainError::conflict(
            "tv.capture.version_conflict",
            "a entrada foi alterada entretanto; recarregue e tente outra vez",
        )
        .with_field("version", "desactualizada")
        .into()),
    }
}

// ============================================================
//  Agente — token `dlxtv_`
// ============================================================

/// Pedido autenticado por token de agente de captura
/// (`Authorization: Bearer dlxtv_…`). Marca a caixa como vista.
pub struct TvAgentAuth {
    pub agent_id: Uuid,
    pub org_id: Uuid,
}

impl FromRequestParts<Arc<AppState>> for TvAgentAuth {
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &Arc<AppState>,
    ) -> Result<Self, ApiError> {
        let token = crate::auth::bearer_token(&parts.headers)
            .filter(|t| t.starts_with("dlxtv_"))
            .ok_or(ApiError::Unauthorized)?;
        let row: Option<(Uuid, Uuid)> = sqlx::query_as(
            "UPDATE tv_capture_agents SET last_seen_at = now()
              WHERE token_hash = $1 AND revoked_at IS NULL
              RETURNING id, org_id",
        )
        .bind(crate::crypto::sha256_hex(token))
        .fetch_optional(&state.db)
        .await?;
        let (agent_id, org_id) = row.ok_or(ApiError::Unauthorized)?;
        Ok(TvAgentAuth { agent_id, org_id })
    }
}

/// Um nó de áudio que o agente viu na caixa, com o caminho físico dele. É o
/// servidor que decide a que vídeo pertence (pela porta).
#[derive(Deserialize, utoipa::ToSchema)]
pub struct AudioReport {
    pub device_path: String,
    pub node: String,
}

/// Uma entrada de vídeo que o agente viu.
#[derive(Deserialize, utoipa::ToSchema)]
pub struct InputReport {
    /// O caminho físico (`ID_PATH` do udev): a chave.
    pub device_path: String,
    #[serde(default)]
    pub video_node: Option<String>,
    #[serde(default)]
    pub vendor_id: Option<String>,
    #[serde(default)]
    pub product_id: Option<String>,
    #[serde(default)]
    pub serial: Option<String>,
    /// `live | no_signal | protected | error`.
    pub signal: String,
    /// O que o aparelho declara (formatos, resoluções, fps).
    #[serde(default)]
    #[schema(value_type = Option<Object>)]
    pub capabilities: Option<serde_json::Value>,
}

#[derive(Deserialize, utoipa::ToSchema)]
pub struct InventoryReq {
    #[serde(default)]
    pub agent_version: Option<String>,
    /// Os nós de áudio da caixa. O par vídeo↔áudio decide-se aqui, pela porta.
    #[serde(default)]
    pub audio: Vec<AudioReport>,
    pub inputs: Vec<InputReport>,
}

/// O que o agente precisa de saber de volta sobre uma entrada sua.
#[derive(Serialize, utoipa::ToSchema)]
pub struct AgentInputView {
    pub id: Uuid,
    pub device_path: String,
    pub label: String,
    pub audio_node: Option<String>,
    /// Nulo = inventariada e sem uso: não há nada a publicar desta entrada.
    pub channel_id: Option<Uuid>,
    pub tuner_kind: String,
    #[schema(value_type = Object)]
    pub tuner_config: serde_json::Value,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct InventoryResp {
    /// Quantas entradas o relatório trouxe.
    pub accepted: usize,
    /// O estado conhecido de TODAS as entradas desta caixa, incluindo as que
    /// deixaram de aparecer (que ficam sem sinal, sem perder configuração).
    pub inputs: Vec<AgentInputView>,
}

/// Substitui o inventário desta caixa.
///
/// Uma entrada que deixa de aparecer **não** é apagada: fica `no_signal` e
/// mantém etiqueta, sintonia e canal — um cabo solto não apaga configuração.
#[utoipa::path(
    put, path = "/api/integrations/tv-agent/v1/inputs", tag = "tv-capture",
    security(("api_key" = [])),
    request_body = InventoryReq,
    responses(
        (status = 200, body = InventoryResp),
        (status = 400, body = crate::openapi::ErrorBody),
        (status = 401, description = "Token `dlxtv_` inválido ou revogado.", body = crate::openapi::ErrorBody),
    )
)]
pub async fn agent_put_inputs(
    State(state): State<Arc<AppState>>,
    agent: TvAgentAuth,
    Json(req): Json<InventoryReq>,
) -> Result<Json<InventoryResp>, ApiError> {
    if req.inputs.len() > rules::MAX_INPUTS_PER_REPORT {
        return Err(ApiError::BadRequest(format!(
            "no máximo {} entradas por relatório",
            rules::MAX_INPUTS_PER_REPORT
        )));
    }
    // Valida TUDO antes de escrever: um relatório meio aplicado seria pior do
    // que um recusado.
    let audio: Vec<(String, String)> = req
        .audio
        .iter()
        .map(|a| {
            Ok((
                rules::validate_device_path(&a.device_path)?,
                rules::validate_node(&a.node)?,
            ))
        })
        .collect::<Result<_, DomainError>>()?;
    struct Pronta {
        path: String,
        video_node: Option<String>,
        audio_node: Option<String>,
        vendor_id: String,
        product_id: String,
        serial: Option<String>,
        signal: rules::Signal,
        capabilities: serde_json::Value,
    }
    let mut prontas = Vec::with_capacity(req.inputs.len());
    for i in &req.inputs {
        let path = rules::validate_device_path(&i.device_path)?;
        prontas.push(Pronta {
            audio_node: rules::pair_audio(&path, &audio).map(str::to_string),
            video_node: i
                .video_node
                .as_deref()
                .map(rules::validate_node)
                .transpose()?
                .filter(|v| !v.is_empty()),
            vendor_id: rules::validate_ident(i.vendor_id.as_deref().unwrap_or(""))?,
            product_id: rules::validate_ident(i.product_id.as_deref().unwrap_or(""))?,
            serial: rules::validate_label(i.serial.as_deref().unwrap_or(""))
                .map(|s| if s.is_empty() { None } else { Some(s) })?,
            signal: rules::Signal::parse(&i.signal)?,
            capabilities: i
                .capabilities
                .clone()
                .unwrap_or(serde_json::Value::Object(Default::default())),
            path,
        });
    }

    let mut tx = state.db.begin().await?;
    let mut vistas: Vec<String> = Vec::with_capacity(prontas.len());
    for p in &prontas {
        sqlx::query(
            "INSERT INTO tv_capture_inputs
                 (id, agent_id, org_id, device_path, vendor_id, product_id, serial,
                  video_node, audio_node, capabilities, signal, last_seen_at, last_signal_at)
             VALUES (gen_random_uuid(), $1, $2, $3, $4, $5, $6, $7, $8, $9, $10, now(),
                     CASE WHEN $10 = 'live' THEN now() ELSE NULL END)
             ON CONFLICT (agent_id, device_path) DO UPDATE SET
                 vendor_id = EXCLUDED.vendor_id, product_id = EXCLUDED.product_id,
                 serial = EXCLUDED.serial, video_node = EXCLUDED.video_node,
                 audio_node = EXCLUDED.audio_node, capabilities = EXCLUDED.capabilities,
                 signal = EXCLUDED.signal, last_seen_at = now(), updated_at = now(),
                 last_signal_at = CASE WHEN EXCLUDED.signal = 'live' THEN now()
                                       ELSE tv_capture_inputs.last_signal_at END",
        )
        .bind(agent.agent_id)
        .bind(agent.org_id)
        .bind(&p.path)
        .bind(&p.vendor_id)
        .bind(&p.product_id)
        .bind(&p.serial)
        .bind(&p.video_node)
        .bind(&p.audio_node)
        .bind(&p.capabilities)
        .bind(p.signal.as_str())
        .execute(&mut *tx)
        .await?;
        vistas.push(p.path.clone());
    }
    // O que não veio no relatório perde o sinal, e só isso.
    sqlx::query(
        "UPDATE tv_capture_inputs SET signal = 'no_signal', updated_at = now()
          WHERE agent_id = $1 AND NOT (device_path = ANY($2)) AND signal <> 'no_signal'",
    )
    .bind(agent.agent_id)
    .bind(&vistas)
    .execute(&mut *tx)
    .await?;
    if let Some(v) = req.agent_version.as_deref() {
        let v = rules::validate_label(v)?;
        sqlx::query("UPDATE tv_capture_agents SET agent_version = $2 WHERE id = $1")
            .bind(agent.agent_id)
            .bind(&v)
            .execute(&mut *tx)
            .await?;
    }
    let inputs: Vec<AgentInputView> = sqlx::query_as::<_, (Uuid, String, String, Option<String>, Option<Uuid>, String, serde_json::Value)>(
        "SELECT id, device_path, label, audio_node, channel_id, tuner_kind, tuner_config
           FROM tv_capture_inputs WHERE agent_id = $1 ORDER BY device_path",
    )
    .bind(agent.agent_id)
    .fetch_all(&mut *tx)
    .await?
    .into_iter()
    .map(|(id, device_path, label, audio_node, channel_id, tuner_kind, tuner_config)| AgentInputView {
        id,
        device_path,
        label,
        audio_node,
        channel_id,
        tuner_kind,
        tuner_config,
    })
    .collect();
    tx.commit().await?;

    Ok(Json(InventoryResp {
        accepted: prontas.len(),
        inputs,
    }))
}
