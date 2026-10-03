//! Estúdio de TV (ADR-0014) — adaptador HTTP + Postgres: estúdios, códigos de
//! emparelhamento, fontes (a app Delonix Câmara) e o destino das gravações.
//! As regras estão em `delonix_meet_domain::studio`; o tempo real em
//! `studio_realtime.rs`. Contrato: `docs/reference/estudio-tv.md`.
//!
//! Quem pode:
//! - ver     — membro activo da org;                          senão `404`;
//! - operar  — `org.administer` **ou** quem criou o estúdio;   senão `403 studio.not_operator`;
//! - gerir   — `org.administer`;                              senão `403 studio.not_manager`.
//!
//! A decisão vem do motor de capacidades do ADR-0008 (`org::decide`), não de
//! comparar o TEXTO do papel: `role == "admin"` espalhado pelos módulos foi
//! exactamente o que o ADR-0008 veio arrumar, e a catraca da arquitectura
//! (`verificacoes_papel_por_string_fora_de_org_rs`) recusa-o.
//!
//! Porque `org.administer` e não `studio.manage`: o contrato
//! (`docs/reference/estudio-tv.md` §1) PROPÔS `studio.view/operate/manage` para
//! o catálogo, e essas três não existem no `Capability` do domínio. Inventá-las
//! aqui obrigaria a mexer no enum, no `CapabilityInfo`, na semeadura das
//! matrizes de papel e na consola que as mostra — o catálogo é de outra frente.
//! A `org.administer` é, pela sua própria descrição, «todas as rotas de
//! administração sem capacidade fina», que é hoje o caso destas. Quando as três
//! entrarem no catálogo, muda-se ESTE bloco e mais nada.

use std::{net::SocketAddr, sync::Arc};

use axum::{
    extract::{ConnectInfo, Path, Query, State},
    http::{header::LOCATION, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use chrono::{DateTime, Utc};
use delonix_meet_core::{
    page::{Page, PageRequest},
    DomainError,
};
use delonix_meet_domain::{
    identity::authorization::{Capability, Decision, ResourceScope},
    studio::{self as rules, pairing, tally::Tally},
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    auth::{sign_jwt, AuthUser, Claims},
    error::ApiError,
    studio_realtime::SourceSession,
    AppState,
};

// ------------------------------------------------------------------ acesso ---

/// O que quem pede é na organização do caminho.
pub(crate) struct Access {
    pub user_id: Uuid,
    /// Tem `org.administer` nesta organização.
    can_administer: bool,
}

/// Pertença **e** capacidade numa só ida à base: o `org::decide` devolve
/// `None` a quem não é membro activo (ou quando a org não existe), que é o
/// `404` de quem não chega ao recurso — o mesmo que outra organização recebe.
pub(crate) async fn member(
    state: &AppState,
    org_id: Uuid,
    user_id: Uuid,
) -> Result<Access, ApiError> {
    let (_, decision) = crate::org::decide(
        state,
        org_id,
        user_id,
        Capability::OrgAdminister,
        ResourceScope::Organization,
    )
    .await?
    .ok_or(ApiError::NotFound)?;
    Ok(Access {
        user_id,
        // `RequiresApproval` NÃO é permissão. Aqui não se abre um pedido de
        // aprovação: estas rotas são de configuração do estúdio, e um `403`
        // com código é a resposta honesta — ao contrário de `require_capability`,
        // que abriria um pedido para uma acção que ninguém vai aprovar a tempo
        // de uma emissão.
        can_administer: matches!(decision, Decision::Allow),
    })
}

impl Access {
    /// Cria e apaga estúdios e agentes de luz.
    pub(crate) fn manage(&self) -> Result<(), ApiError> {
        if self.can_administer {
            Ok(())
        } else {
            Err(DomainError::forbidden("studio.not_manager")
                .with_message("só um administrador da organização cria ou apaga estúdios e agentes")
                .with_field("capability", Capability::OrgAdminister.as_str())
                .into())
        }
    }
    /// Opera um estúdio concreto: quem administra a org, ou quem o criou.
    pub(crate) fn operate(&self, studio: &Studio) -> Result<(), ApiError> {
        if self.can_administer || studio.created_by == self.user_id {
            Ok(())
        } else {
            Err(DomainError::forbidden("studio.not_operator")
                .with_message("só um administrador ou quem criou o estúdio o opera")
                .into())
        }
    }
}

// ------------------------------------------------------------------- tipos ---

#[derive(Debug, Clone, Serialize, sqlx::FromRow, utoipa::ToSchema)]
pub struct Studio {
    pub id: Uuid,
    pub org_id: Uuid,
    pub name: String,
    /// A sala SFU do estúdio (onde entram operador e fontes).
    pub room_code: String,
    #[serde(skip)]
    pub room_id: Uuid,
    /// Grava cada fonte em ficheiro próprio (ADR-0014 §2.3).
    pub iso_recording: bool,
    pub created_by: Uuid,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

const STUDIO_SELECT: &str = "SELECT s.id, s.org_id, s.name, r.code AS room_code, s.room_id, \
     s.iso_recording, s.created_by, s.created_at, s.updated_at \
     FROM studios s JOIN rooms r ON r.id = s.room_id";

#[derive(Serialize, utoipa::ToSchema)]
pub struct StudioPage {
    pub items: Vec<Studio>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_page_token: Option<String>,
}

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateStudioReq {
    pub name: String,
    #[serde(default)]
    pub iso_recording: Option<bool>,
}

#[derive(Deserialize, Default, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdateStudioReq {
    pub name: Option<String>,
    pub iso_recording: Option<bool>,
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

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CreatePairingCodeReq {
    #[serde(default)]
    pub label: Option<String>,
    /// CAM n (1–16). Omisso: o menor livre no momento do emparelhamento.
    #[serde(default)]
    pub number: Option<i32>,
}

/// O código acabado de gerar — o `code` só aparece nesta resposta.
#[derive(Serialize, utoipa::ToSchema)]
pub struct NewPairingCode {
    pub id: Uuid,
    /// `XXXX-XXXX`.
    pub code: String,
    pub expires_at: DateTime<Utc>,
    pub number: Option<i32>,
    pub label: String,
    pub max_attempts: i32,
    pub room_code: String,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct PairingCode {
    pub id: Uuid,
    pub number: Option<i32>,
    pub label: String,
    pub expires_at: DateTime<Utc>,
    pub attempts: i32,
    pub max_attempts: i32,
    pub consumed_at: Option<DateTime<Utc>>,
    /// `active` | `consumed` | `expired` | `burned`.
    pub state: String,
    pub created_at: DateTime<Utc>,
}

#[derive(sqlx::FromRow)]
struct PairingRow {
    id: Uuid,
    number: Option<i32>,
    label: String,
    expires_at: DateTime<Utc>,
    attempts: i32,
    consumed_at: Option<DateTime<Utc>>,
    revoked_at: Option<DateTime<Utc>>,
    created_at: DateTime<Utc>,
}

impl From<PairingRow> for PairingCode {
    fn from(r: PairingRow) -> Self {
        let state = pairing::CodeState::of(
            r.consumed_at.is_some(),
            r.attempts,
            r.expires_at <= Utc::now(),
            r.revoked_at.is_some(),
        );
        Self {
            id: r.id,
            number: r.number,
            label: r.label,
            expires_at: r.expires_at,
            attempts: r.attempts,
            max_attempts: pairing::MAX_ATTEMPTS,
            consumed_at: r.consumed_at,
            state: state.as_str().into(),
            created_at: r.created_at,
        }
    }
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct PairingCodePage {
    pub items: Vec<PairingCode>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_page_token: Option<String>,
}

#[derive(Deserialize, Default, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct DeviceReq {
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub platform: String,
    #[serde(default)]
    pub app_version: String,
}

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct RedeemReq {
    pub code: String,
    #[serde(default)]
    pub device: DeviceReq,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct Pairing {
    pub source_id: Uuid,
    pub studio_id: Uuid,
    pub number: i32,
    /// `CAM 2 · telefone da Ana`.
    pub label: String,
    pub room_code: String,
    /// JWT `typ: "source"`: só abre o `/ws` desta sala, só publica media.
    pub source_token: String,
    pub expires_at: DateTime<Utc>,
    pub ws_path: String,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct Device {
    pub model: String,
    pub platform: String,
    pub app_version: String,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct Source {
    pub id: Uuid,
    pub studio_id: Uuid,
    pub number: i32,
    pub label: String,
    /// `phone_app`.
    pub kind: String,
    pub device: Device,
    pub paired_at: DateTime<Utc>,
    pub revoked_at: Option<DateTime<Utc>>,
    pub last_seen_at: Option<DateTime<Utc>>,
    /// Neste pod: a sala viva. Noutro pod: o último estado persistido.
    pub connected: bool,
    /// `program` | `preview` | `free`.
    pub tally: String,
    #[schema(value_type = Option<Object>)]
    pub status: Option<serde_json::Value>,
}

#[derive(sqlx::FromRow)]
struct SourceRow {
    id: Uuid,
    studio_id: Uuid,
    number: i32,
    label: String,
    kind: String,
    device_model: String,
    device_platform: String,
    app_version: String,
    paired_at: DateTime<Utc>,
    revoked_at: Option<DateTime<Utc>>,
    last_seen_at: Option<DateTime<Utc>>,
    last_status: Option<serde_json::Value>,
    last_tally: String,
    connected: bool,
}

const SOURCE_COLUMNS: &str = "id, studio_id, number, label, kind, device_model, device_platform, \
     app_version, paired_at, revoked_at, last_seen_at, last_status, last_tally, connected";

#[derive(Serialize, utoipa::ToSchema)]
pub struct SourcePage {
    pub items: Vec<Source>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_page_token: Option<String>,
}

#[derive(Deserialize, Default, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdateSourceReq {
    pub label: Option<String>,
    pub number: Option<i32>,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct ObjectStorage {
    /// `minio`.
    pub kind: String,
    /// `not_configured`: o servidor não tem armazenamento de objectos.
    pub state: String,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct RecordingTarget {
    /// `local`: o volume de `RECORDINGS_DIR`.
    pub kind: String,
    /// `statvfs` real; `null` se o volume não se deixou medir.
    pub free_bytes: Option<u64>,
    pub total_bytes: Option<u64>,
    pub iso_recording: bool,
    pub object_storage: ObjectStorage,
}

#[derive(utoipa::OpenApi)]
#[openapi(
    paths(
        list_studios,
        create_studio,
        get_studio,
        update_studio,
        delete_studio,
        create_pairing_code,
        list_pairing_codes,
        delete_pairing_code,
        redeem,
        list_sources,
        get_source,
        update_source,
        delete_source,
        recording_target
    ),
    components(schemas(
        Studio,
        StudioPage,
        CreateStudioReq,
        UpdateStudioReq,
        CreatePairingCodeReq,
        NewPairingCode,
        PairingCode,
        PairingCodePage,
        DeviceReq,
        RedeemReq,
        Pairing,
        Device,
        Source,
        SourcePage,
        UpdateSourceReq,
        RecordingTarget,
        ObjectStorage
    ))
)]
pub struct ApiDoc;

// --------------------------------------------------------------- estúdios ---

pub(crate) async fn load_studio(
    state: &AppState,
    org_id: Uuid,
    studio_id: Uuid,
) -> Result<Studio, ApiError> {
    sqlx::query_as(&format!(
        "{STUDIO_SELECT} WHERE s.id = $1 AND s.org_id = $2"
    ))
    .bind(studio_id)
    .bind(org_id)
    .fetch_optional(&state.db)
    .await?
    .ok_or(ApiError::NotFound)
}

/// Estúdios da organização.
#[utoipa::path(
    get, path = "/api/orgs/{org_id}/studios", tag = "studio",
    security(("session" = [])),
    params(("org_id" = Uuid, Path), ListQuery),
    responses(
        (status = 200, body = StudioPage),
        (status = 400, body = crate::openapi::ErrorBody, description = "page_token inválido"),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 404, body = crate::openapi::ErrorBody),
    )
)]
pub async fn list_studios(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(org_id): Path<Uuid>,
    Query(q): Query<ListQuery>,
) -> Result<Json<StudioPage>, ApiError> {
    member(&state, org_id, auth.user_id).await?;
    let page = PageRequest {
        page_size: q.page_size,
        page_token: q.page_token,
    };
    let size = page.size();
    let cursor: Option<Cursor> = page.cursor()?;
    let rows: Vec<Studio> = sqlx::query_as(&format!(
        "{STUDIO_SELECT} WHERE s.org_id = $1
            AND ($2::timestamptz IS NULL OR (s.created_at, s.id) > ($2, $3))
          ORDER BY s.created_at, s.id LIMIT $4"
    ))
    .bind(org_id)
    .bind(cursor.as_ref().map(|c| c.at))
    .bind(cursor.as_ref().map(|c| c.id).unwrap_or_default())
    .bind(size as i64 + 1)
    .fetch_all(&state.db)
    .await?;
    let p = Page::from_overfetch(rows, size, |s| Cursor {
        at: s.created_at,
        id: s.id,
    });
    Ok(Json(StudioPage {
        items: p.items,
        next_page_token: p.next_page_token,
    }))
}

/// Cria um estúdio e a sua sala SFU (sem E2EE: o directo recusa-o, ADR-0003).
#[utoipa::path(
    post, path = "/api/orgs/{org_id}/studios", tag = "studio",
    security(("session" = [])),
    params(("org_id" = Uuid, Path)),
    request_body = CreateStudioReq,
    responses(
        (status = 201, body = Studio, headers(("Location" = String))),
        (status = 400, body = crate::openapi::ErrorBody, description = "`studio.invalid_name`"),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 403, body = crate::openapi::ErrorBody, description = "`studio.not_manager`"),
        (status = 404, body = crate::openapi::ErrorBody),
    )
)]
pub async fn create_studio(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(org_id): Path<Uuid>,
    Json(req): Json<CreateStudioReq>,
) -> Result<Response, ApiError> {
    member(&state, org_id, auth.user_id).await?.manage()?;
    let name = rules::validate_name(&req.name)?;
    let room = crate::rooms::insert_room(
        &state.db,
        auth.user_id,
        &format!("Estúdio · {name}"),
        "sfu",
        false,
        false,
        "normal",
    )
    .await?;
    let id = Uuid::new_v4();
    let inserted = sqlx::query(
        "INSERT INTO studios (id, org_id, name, room_id, iso_recording, created_by)
         VALUES ($1, $2, $3, $4, $5, $6)",
    )
    .bind(id)
    .bind(org_id)
    .bind(&name)
    .bind(room.id)
    .bind(req.iso_recording.unwrap_or(true))
    .bind(auth.user_id)
    .execute(&state.db)
    .await;
    if let Err(e) = inserted {
        // Sem estúdio a sala não serve a ninguém: não fica órfã.
        let _ = sqlx::query("DELETE FROM rooms WHERE id = $1")
            .bind(room.id)
            .execute(&state.db)
            .await;
        return Err(e.into());
    }
    let studio = load_studio(&state, org_id, id).await?;
    crate::audit::log(
        &state.db,
        Some(org_id),
        auth.user_id,
        "studio.created",
        &name,
    )
    .await;
    Ok((
        StatusCode::CREATED,
        [(LOCATION, format!("/api/orgs/{org_id}/studios/{id}"))],
        Json(studio),
    )
        .into_response())
}

/// Um estúdio.
#[utoipa::path(
    get, path = "/api/orgs/{org_id}/studios/{studio_id}", tag = "studio",
    security(("session" = [])),
    params(("org_id" = Uuid, Path), ("studio_id" = Uuid, Path)),
    responses(
        (status = 200, body = Studio),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 404, body = crate::openapi::ErrorBody),
    )
)]
pub async fn get_studio(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path((org_id, studio_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<Studio>, ApiError> {
    member(&state, org_id, auth.user_id).await?;
    Ok(Json(load_studio(&state, org_id, studio_id).await?))
}

/// Altera o nome ou a gravação ISO.
#[utoipa::path(
    patch, path = "/api/orgs/{org_id}/studios/{studio_id}", tag = "studio",
    security(("session" = [])),
    params(("org_id" = Uuid, Path), ("studio_id" = Uuid, Path)),
    request_body = UpdateStudioReq,
    responses(
        (status = 200, body = Studio),
        (status = 400, body = crate::openapi::ErrorBody),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 403, body = crate::openapi::ErrorBody, description = "`studio.not_operator`"),
        (status = 404, body = crate::openapi::ErrorBody),
    )
)]
pub async fn update_studio(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path((org_id, studio_id)): Path<(Uuid, Uuid)>,
    Json(req): Json<UpdateStudioReq>,
) -> Result<Json<Studio>, ApiError> {
    let access = member(&state, org_id, auth.user_id).await?;
    let studio = load_studio(&state, org_id, studio_id).await?;
    access.operate(&studio)?;
    let name = req.name.as_deref().map(rules::validate_name).transpose()?;
    sqlx::query(
        "UPDATE studios SET name = COALESCE($3, name),
                iso_recording = COALESCE($4, iso_recording), updated_at = now()
          WHERE id = $1 AND org_id = $2",
    )
    .bind(studio_id)
    .bind(org_id)
    .bind(name)
    .bind(req.iso_recording)
    .execute(&state.db)
    .await?;
    crate::audit::log(
        &state.db,
        Some(org_id),
        auth.user_id,
        "studio.updated",
        &studio_id.to_string(),
    )
    .await;
    Ok(Json(load_studio(&state, org_id, studio_id).await?))
}

/// Apaga o estúdio. A sala e as gravações ficam.
#[utoipa::path(
    delete, path = "/api/orgs/{org_id}/studios/{studio_id}", tag = "studio",
    security(("session" = [])),
    params(("org_id" = Uuid, Path), ("studio_id" = Uuid, Path)),
    responses(
        (status = 204, description = "Apagado."),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 403, body = crate::openapi::ErrorBody, description = "`studio.not_manager`"),
        (status = 404, body = crate::openapi::ErrorBody),
    )
)]
pub async fn delete_studio(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path((org_id, studio_id)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, ApiError> {
    member(&state, org_id, auth.user_id).await?.manage()?;
    let studio = load_studio(&state, org_id, studio_id).await?;
    // As fontes vivas deste pod saem já; o token delas deixa de abrir o /ws
    // porque a linha desaparece.
    for s in state.studio.snapshot(studio.room_id) {
        state.studio.revoke(studio.room_id, s.source_id);
    }
    state.studio.forget_room(studio.room_id);
    sqlx::query("DELETE FROM studios WHERE id = $1 AND org_id = $2")
        .bind(studio_id)
        .bind(org_id)
        .execute(&state.db)
        .await?;
    crate::audit::log(
        &state.db,
        Some(org_id),
        auth.user_id,
        "studio.deleted",
        &studio_id.to_string(),
    )
    .await;
    Ok(StatusCode::NO_CONTENT)
}

// ---------------------------------------------------- códigos de emparelhar ---

async fn number_taken(state: &AppState, studio_id: Uuid, number: i32) -> Result<bool, ApiError> {
    Ok(sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM studio_sources
                        WHERE studio_id = $1 AND number = $2 AND revoked_at IS NULL)",
    )
    .bind(studio_id)
    .bind(number)
    .fetch_one(&state.db)
    .await?)
}

fn number_taken_error(number: i32) -> ApiError {
    DomainError::conflict(
        "studio.source_number_taken",
        format!("já há uma fonte activa como CAM {number} neste estúdio"),
    )
    .with_field("number", "livre no estúdio")
    .into()
}

/// Gera um código de uso único (10 min, 5 tentativas) para a app Delonix Câmara.
#[utoipa::path(
    post, path = "/api/orgs/{org_id}/studios/{studio_id}/pairing-codes", tag = "studio",
    security(("session" = [])),
    params(("org_id" = Uuid, Path), ("studio_id" = Uuid, Path)),
    request_body = CreatePairingCodeReq,
    responses(
        (status = 201, body = NewPairingCode, headers(("Location" = String))),
        (status = 400, body = crate::openapi::ErrorBody),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 403, body = crate::openapi::ErrorBody, description = "`studio.not_operator`"),
        (status = 404, body = crate::openapi::ErrorBody),
        (status = 409, body = crate::openapi::ErrorBody, description = "`studio.source_number_taken`"),
    )
)]
pub async fn create_pairing_code(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path((org_id, studio_id)): Path<(Uuid, Uuid)>,
    Json(req): Json<CreatePairingCodeReq>,
) -> Result<Response, ApiError> {
    let access = member(&state, org_id, auth.user_id).await?;
    let studio = load_studio(&state, org_id, studio_id).await?;
    access.operate(&studio)?;
    let label = rules::validate_label(req.label.as_deref().unwrap_or(""))?;
    let number = req.number.map(rules::validate_number).transpose()?;
    if let Some(n) = number {
        if number_taken(&state, studio_id, n).await? {
            return Err(number_taken_error(n));
        }
    }
    let id = Uuid::new_v4();
    // O localizador é único entre códigos vivos: uma colisão (2^20) repete.
    for _ in 0..8 {
        let code = pairing::generate();
        let r: Result<DateTime<Utc>, sqlx::Error> = sqlx::query_scalar(
            "INSERT INTO studio_pairing_codes
                (id, studio_id, org_id, locator, secret_hash, number, label, expires_at, created_by)
             VALUES ($1, $2, $3, $4, $5, $6, $7, now() + make_interval(secs => $8), $9)
             RETURNING expires_at",
        )
        .bind(id)
        .bind(studio_id)
        .bind(org_id)
        .bind(&code.locator)
        .bind(pairing::secret_hash(&code.locator, &code.secret))
        .bind(number)
        .bind(&label)
        .bind(pairing::TTL_SECS as f64)
        .bind(auth.user_id)
        .fetch_one(&state.db)
        .await;
        match r {
            Ok(expires_at) => {
                crate::audit::log(
                    &state.db,
                    Some(org_id),
                    auth.user_id,
                    "studio.pairing_code_created",
                    &format!("{studio_id}/{id}"),
                )
                .await;
                return Ok((
                    StatusCode::CREATED,
                    [(
                        LOCATION,
                        format!("/api/orgs/{org_id}/studios/{studio_id}/pairing-codes/{id}"),
                    )],
                    Json(NewPairingCode {
                        id,
                        code: code.display,
                        expires_at,
                        number,
                        label,
                        max_attempts: pairing::MAX_ATTEMPTS,
                        room_code: studio.room_code,
                    }),
                )
                    .into_response());
            }
            Err(sqlx::Error::Database(d)) if d.is_unique_violation() => continue,
            Err(e) => return Err(e.into()),
        }
    }
    Err(ApiError::internal(
        "não foi possível alocar um código de emparelhamento",
    ))
}

/// Códigos do estúdio (sem o código em claro).
#[utoipa::path(
    get, path = "/api/orgs/{org_id}/studios/{studio_id}/pairing-codes", tag = "studio",
    security(("session" = [])),
    params(("org_id" = Uuid, Path), ("studio_id" = Uuid, Path), ListQuery),
    responses(
        (status = 200, body = PairingCodePage),
        (status = 400, body = crate::openapi::ErrorBody),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 404, body = crate::openapi::ErrorBody),
    )
)]
pub async fn list_pairing_codes(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path((org_id, studio_id)): Path<(Uuid, Uuid)>,
    Query(q): Query<ListQuery>,
) -> Result<Json<PairingCodePage>, ApiError> {
    member(&state, org_id, auth.user_id).await?;
    load_studio(&state, org_id, studio_id).await?;
    let page = PageRequest {
        page_size: q.page_size,
        page_token: q.page_token,
    };
    let size = page.size();
    let cursor: Option<Cursor> = page.cursor()?;
    let rows: Vec<PairingRow> = sqlx::query_as(
        "SELECT id, number, label, expires_at, attempts, consumed_at, revoked_at, created_at
           FROM studio_pairing_codes
          WHERE studio_id = $1 AND org_id = $2
            AND ($3::timestamptz IS NULL OR (created_at, id) > ($3, $4))
          ORDER BY created_at, id LIMIT $5",
    )
    .bind(studio_id)
    .bind(org_id)
    .bind(cursor.as_ref().map(|c| c.at))
    .bind(cursor.as_ref().map(|c| c.id).unwrap_or_default())
    .bind(size as i64 + 1)
    .fetch_all(&state.db)
    .await?;
    let p = Page::from_overfetch(rows, size, |r| Cursor {
        at: r.created_at,
        id: r.id,
    });
    Ok(Json(PairingCodePage {
        items: p.items.into_iter().map(PairingCode::from).collect(),
        next_page_token: p.next_page_token,
    }))
}

/// Revoga um código ainda não usado.
#[utoipa::path(
    delete, path = "/api/orgs/{org_id}/studios/{studio_id}/pairing-codes/{code_id}", tag = "studio",
    security(("session" = [])),
    params(("org_id" = Uuid, Path), ("studio_id" = Uuid, Path), ("code_id" = Uuid, Path)),
    responses(
        (status = 204, description = "Revogado."),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 403, body = crate::openapi::ErrorBody, description = "`studio.not_operator`"),
        (status = 404, body = crate::openapi::ErrorBody),
    )
)]
pub async fn delete_pairing_code(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path((org_id, studio_id, code_id)): Path<(Uuid, Uuid, Uuid)>,
) -> Result<StatusCode, ApiError> {
    let access = member(&state, org_id, auth.user_id).await?;
    let studio = load_studio(&state, org_id, studio_id).await?;
    access.operate(&studio)?;
    let r = sqlx::query(
        "UPDATE studio_pairing_codes SET revoked_at = now()
          WHERE id = $1 AND studio_id = $2 AND org_id = $3
            AND revoked_at IS NULL AND consumed_at IS NULL",
    )
    .bind(code_id)
    .bind(studio_id)
    .bind(org_id)
    .execute(&state.db)
    .await?;
    if r.rows_affected() == 0 {
        return Err(ApiError::NotFound);
    }
    crate::audit::log(
        &state.db,
        Some(org_id),
        auth.user_id,
        "studio.pairing_code_revoked",
        &format!("{studio_id}/{code_id}"),
    )
    .await;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(sqlx::FromRow)]
struct RedeemRow {
    id: Uuid,
    created_by: Uuid,
    studio_id: Uuid,
    org_id: Uuid,
    secret_hash: String,
    number: Option<i32>,
    label: String,
    attempts: i32,
    expired: bool,
}

/// O telefone troca o código por um token de fonte. Rota PÚBLICA (sem conta),
/// com rate-limit por IP e 5 tentativas por código.
#[utoipa::path(
    post, path = "/api/studio-pairings", tag = "studio",
    request_body = RedeemReq,
    responses(
        (status = 201, body = Pairing),
        (status = 400, body = crate::openapi::ErrorBody, description = "`studio.pairing_malformed`"),
        (status = 404, body = crate::openapi::ErrorBody, description = "`studio.pairing_invalid` — não existe, expirou, já usado, queimado ou segredo errado (a mesma resposta para todos)"),
        (status = 409, body = crate::openapi::ErrorBody, description = "`studio.source_number_taken` / `studio.full`"),
        (status = 429, body = crate::openapi::ErrorBody),
    )
)]
pub async fn redeem(
    State(state): State<Arc<AppState>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(req): Json<RedeemReq>,
) -> Result<Response, ApiError> {
    let ip = crate::rate_limit::client_ip(&headers, addr.ip());
    if state.studio_pairing_limiter.acquire(&ip).is_err() {
        return Err(ApiError::TooManyRequests);
    }
    let (locator, secret) = pairing::parse(&req.code)?;
    let model = pairing::validate_device_field("device.model", &req.device.model)?;
    let platform = pairing::validate_device_field("device.platform", &req.device.platform)?;
    let app_version =
        pairing::validate_device_field("device.app_version", &req.device.app_version)?;

    let mut tx = state.db.begin().await?;
    let row: Option<RedeemRow> = sqlx::query_as(
        "SELECT id, created_by, studio_id, org_id, secret_hash, number, label, attempts,
                (expires_at <= now()) AS expired
           FROM studio_pairing_codes
          WHERE locator = $1 AND consumed_at IS NULL AND revoked_at IS NULL
          FOR UPDATE",
    )
    .bind(&locator)
    .fetch_optional(&mut *tx)
    .await?;
    let Some(code) = row else {
        return Err(pairing::invalid().into());
    };
    if code.expired || code.attempts >= pairing::MAX_ATTEMPTS {
        return Err(pairing::invalid().into());
    }
    if !pairing::verify(&code.secret_hash, &locator, &secret) {
        // A tentativa conta mesmo que o pedido morra a seguir: faz commit.
        sqlx::query("UPDATE studio_pairing_codes SET attempts = attempts + 1 WHERE id = $1")
            .bind(code.id)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        tracing::warn!(pairing_code = %code.id, %ip, "tentativa errada de emparelhamento");
        return Err(pairing::invalid().into());
    }
    // O número: o do código, ou o menor livre agora.
    let taken: Vec<i32> = sqlx::query_scalar(
        "SELECT number FROM studio_sources WHERE studio_id = $1 AND revoked_at IS NULL",
    )
    .bind(code.studio_id)
    .fetch_all(&mut *tx)
    .await?;
    let number = match code.number {
        Some(n) if taken.contains(&n) => return Err(number_taken_error(n)),
        Some(n) => n,
        None => rules::first_free_number(&taken).ok_or_else(|| {
            ApiError::from(DomainError::conflict(
                "studio.full",
                "o estúdio já tem 16 fontes activas — revogue uma antes de emparelhar outra",
            ))
        })?,
    };
    let source_id = Uuid::new_v4();
    let ins = sqlx::query(
        "INSERT INTO studio_sources
            (id, studio_id, org_id, number, label, device_model, device_platform, app_version, pairing_code_id)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)",
    )
    .bind(source_id)
    .bind(code.studio_id)
    .bind(code.org_id)
    .bind(number)
    .bind(&code.label)
    .bind(&model)
    .bind(&platform)
    .bind(&app_version)
    .bind(code.id)
    .execute(&mut *tx)
    .await;
    match ins {
        Ok(_) => {}
        Err(sqlx::Error::Database(d)) if d.is_unique_violation() => {
            return Err(number_taken_error(number))
        }
        Err(e) => return Err(e.into()),
    }
    sqlx::query("UPDATE studio_pairing_codes SET consumed_at = now() WHERE id = $1")
        .bind(code.id)
        .execute(&mut *tx)
        .await?;
    let (room_id, room_code): (Uuid, String) = sqlx::query_as(
        "SELECT r.id, r.code FROM studios s JOIN rooms r ON r.id = s.room_id WHERE s.id = $1",
    )
    .bind(code.studio_id)
    .fetch_one(&mut *tx)
    .await?;
    tx.commit().await?;

    let label = rules::display_label(number, &code.label);
    let now = Utc::now().timestamp();
    let exp = now + pairing::SOURCE_TOKEN_TTL_SECS;
    let token = sign_jwt(
        &state.config.jwt_secret,
        &Claims {
            sub: source_id,
            typ: "source".into(),
            iat: now,
            exp,
            room: Some(room_id),
            name: Some(label.clone()),
            topo: Some("sfu".into()),
            owner: false,
            wait: false,
            adm: false,
            is_bot: false,
            origin: None,
            title: None,
            lobby: Some(false),
            wr: Some(false),
            // Uma fonte de estúdio não é um convidado sem conta.
            guest: false,
        },
    )?;
    // O actor é quem emitiu o código: a fonte não é uma conta.
    crate::audit::log(
        &state.db,
        Some(code.org_id),
        code.created_by,
        "studio.source_paired",
        &format!("{}/{source_id} {label}", code.studio_id),
    )
    .await;
    Ok((
        StatusCode::CREATED,
        Json(Pairing {
            source_id,
            studio_id: code.studio_id,
            number,
            ws_path: format!("/ws?token={token}&room={room_code}"),
            label,
            room_code,
            source_token: token,
            expires_at: DateTime::from_timestamp(exp, 0).unwrap_or_else(Utc::now),
        }),
    )
        .into_response())
}

/// Para o `/ws`: a fonte do token existe, não está revogada e é desta sala.
pub(crate) async fn source_session_for_token(
    state: &AppState,
    claims: &Claims,
) -> Result<SourceSession, ApiError> {
    let row: Option<(Uuid, Uuid, Uuid, i32, String, Uuid)> = sqlx::query_as(
        "SELECT s.id, s.studio_id, s.org_id, s.number, s.label, st.room_id
           FROM studio_sources s JOIN studios st ON st.id = s.studio_id
          WHERE s.id = $1 AND s.revoked_at IS NULL",
    )
    .bind(claims.sub)
    .fetch_optional(&state.db)
    .await?;
    match row {
        Some((source_id, studio_id, org_id, number, label, room_id))
            if Some(room_id) == claims.room =>
        {
            Ok(SourceSession {
                source_id,
                studio_id,
                org_id,
                number,
                label: rules::display_label(number, &label),
            })
        }
        _ => Err(ApiError::Unauthorized),
    }
}

// ----------------------------------------------------------------- fontes ---

fn source_view(state: &AppState, room_id: Uuid, r: SourceRow) -> Source {
    let (connected, tally, status) = match state.studio.live(room_id, r.id) {
        Some((c, t, st)) if r.revoked_at.is_none() => (
            c,
            t.as_str().to_string(),
            st.and_then(|s| serde_json::to_value(s).ok())
                .or(r.last_status),
        ),
        _ => (
            r.connected && r.revoked_at.is_none(),
            if r.revoked_at.is_none() {
                r.last_tally
            } else {
                Tally::Free.as_str().into()
            },
            r.last_status,
        ),
    };
    Source {
        id: r.id,
        studio_id: r.studio_id,
        number: r.number,
        label: rules::display_label(r.number, &r.label),
        kind: r.kind,
        device: Device {
            model: r.device_model,
            platform: r.device_platform,
            app_version: r.app_version,
        },
        paired_at: r.paired_at,
        revoked_at: r.revoked_at,
        last_seen_at: r.last_seen_at,
        connected,
        tally,
        status,
    }
}

/// Fontes activas do estúdio.
#[utoipa::path(
    get, path = "/api/orgs/{org_id}/studios/{studio_id}/sources", tag = "studio",
    security(("session" = [])),
    params(("org_id" = Uuid, Path), ("studio_id" = Uuid, Path), ListQuery),
    responses(
        (status = 200, body = SourcePage),
        (status = 400, body = crate::openapi::ErrorBody),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 404, body = crate::openapi::ErrorBody),
    )
)]
pub async fn list_sources(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path((org_id, studio_id)): Path<(Uuid, Uuid)>,
    Query(q): Query<ListQuery>,
) -> Result<Json<SourcePage>, ApiError> {
    member(&state, org_id, auth.user_id).await?;
    let studio = load_studio(&state, org_id, studio_id).await?;
    let page = PageRequest {
        page_size: q.page_size,
        page_token: q.page_token,
    };
    let size = page.size();
    let cursor: Option<Cursor> = page.cursor()?;
    let rows: Vec<SourceRow> = sqlx::query_as(&format!(
        "SELECT {SOURCE_COLUMNS} FROM studio_sources
          WHERE studio_id = $1 AND org_id = $2 AND revoked_at IS NULL
            AND ($3::timestamptz IS NULL OR (paired_at, id) > ($3, $4))
          ORDER BY paired_at, id LIMIT $5"
    ))
    .bind(studio_id)
    .bind(org_id)
    .bind(cursor.as_ref().map(|c| c.at))
    .bind(cursor.as_ref().map(|c| c.id).unwrap_or_default())
    .bind(size as i64 + 1)
    .fetch_all(&state.db)
    .await?;
    let p = Page::from_overfetch(rows, size, |r| Cursor {
        at: r.paired_at,
        id: r.id,
    });
    Ok(Json(SourcePage {
        items: p
            .items
            .into_iter()
            .map(|r| source_view(&state, studio.room_id, r))
            .collect(),
        next_page_token: p.next_page_token,
    }))
}

async fn fetch_source(
    state: &AppState,
    org_id: Uuid,
    studio_id: Uuid,
    source_id: Uuid,
) -> Result<SourceRow, ApiError> {
    sqlx::query_as(&format!(
        "SELECT {SOURCE_COLUMNS} FROM studio_sources WHERE id = $1 AND studio_id = $2 AND org_id = $3"
    ))
    .bind(source_id)
    .bind(studio_id)
    .bind(org_id)
    .fetch_optional(&state.db)
    .await?
    .ok_or(ApiError::NotFound)
}

/// Uma fonte (também revogada).
#[utoipa::path(
    get, path = "/api/orgs/{org_id}/studios/{studio_id}/sources/{source_id}", tag = "studio",
    security(("session" = [])),
    params(("org_id" = Uuid, Path), ("studio_id" = Uuid, Path), ("source_id" = Uuid, Path)),
    responses(
        (status = 200, body = Source),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 404, body = crate::openapi::ErrorBody),
    )
)]
pub async fn get_source(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path((org_id, studio_id, source_id)): Path<(Uuid, Uuid, Uuid)>,
) -> Result<Json<Source>, ApiError> {
    member(&state, org_id, auth.user_id).await?;
    let studio = load_studio(&state, org_id, studio_id).await?;
    let row = fetch_source(&state, org_id, studio_id, source_id).await?;
    Ok(Json(source_view(&state, studio.room_id, row)))
}

/// Muda o rótulo ou o número (CAM n) de uma fonte activa.
#[utoipa::path(
    patch, path = "/api/orgs/{org_id}/studios/{studio_id}/sources/{source_id}", tag = "studio",
    security(("session" = [])),
    params(("org_id" = Uuid, Path), ("studio_id" = Uuid, Path), ("source_id" = Uuid, Path)),
    request_body = UpdateSourceReq,
    responses(
        (status = 200, body = Source),
        (status = 400, body = crate::openapi::ErrorBody),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 403, body = crate::openapi::ErrorBody, description = "`studio.not_operator`"),
        (status = 404, body = crate::openapi::ErrorBody),
        (status = 409, body = crate::openapi::ErrorBody, description = "`studio.source_number_taken`"),
    )
)]
pub async fn update_source(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path((org_id, studio_id, source_id)): Path<(Uuid, Uuid, Uuid)>,
    Json(req): Json<UpdateSourceReq>,
) -> Result<Json<Source>, ApiError> {
    let access = member(&state, org_id, auth.user_id).await?;
    let studio = load_studio(&state, org_id, studio_id).await?;
    access.operate(&studio)?;
    let label = req
        .label
        .as_deref()
        .map(rules::validate_label)
        .transpose()?;
    let number = req.number.map(rules::validate_number).transpose()?;
    let r = sqlx::query(
        "UPDATE studio_sources SET label = COALESCE($4, label), number = COALESCE($5, number)
          WHERE id = $1 AND studio_id = $2 AND org_id = $3 AND revoked_at IS NULL",
    )
    .bind(source_id)
    .bind(studio_id)
    .bind(org_id)
    .bind(label)
    .bind(number)
    .execute(&state.db)
    .await;
    match r {
        Ok(done) if done.rows_affected() == 0 => return Err(ApiError::NotFound),
        Ok(_) => {}
        Err(sqlx::Error::Database(d)) if d.is_unique_violation() => {
            return Err(number_taken_error(number.unwrap_or_default()))
        }
        Err(e) => return Err(e.into()),
    }
    crate::audit::log(
        &state.db,
        Some(org_id),
        auth.user_id,
        "studio.source_updated",
        &format!("{studio_id}/{source_id}"),
    )
    .await;
    let row = fetch_source(&state, org_id, studio_id, source_id).await?;
    Ok(Json(source_view(&state, studio.room_id, row)))
}

/// Revoga a fonte: o token deixa de abrir o `/ws` e o telefone é expulso.
#[utoipa::path(
    delete, path = "/api/orgs/{org_id}/studios/{studio_id}/sources/{source_id}", tag = "studio",
    security(("session" = [])),
    params(("org_id" = Uuid, Path), ("studio_id" = Uuid, Path), ("source_id" = Uuid, Path)),
    responses(
        (status = 204, description = "Revogada."),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 403, body = crate::openapi::ErrorBody, description = "`studio.not_operator`"),
        (status = 404, body = crate::openapi::ErrorBody),
    )
)]
pub async fn delete_source(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path((org_id, studio_id, source_id)): Path<(Uuid, Uuid, Uuid)>,
) -> Result<StatusCode, ApiError> {
    let access = member(&state, org_id, auth.user_id).await?;
    let studio = load_studio(&state, org_id, studio_id).await?;
    access.operate(&studio)?;
    let r = sqlx::query(
        "UPDATE studio_sources SET revoked_at = now(), connected = false
          WHERE id = $1 AND studio_id = $2 AND org_id = $3 AND revoked_at IS NULL",
    )
    .bind(source_id)
    .bind(studio_id)
    .bind(org_id)
    .execute(&state.db)
    .await?;
    if r.rows_affected() == 0 {
        return Err(ApiError::NotFound);
    }
    if state.studio.revoke(studio.room_id, source_id) {
        crate::studio_realtime::push_sources(&state, studio.room_id);
    }
    crate::audit::log(
        &state.db,
        Some(org_id),
        auth.user_id,
        "studio.source_revoked",
        &format!("{studio_id}/{source_id}"),
    )
    .await;
    Ok(StatusCode::NO_CONTENT)
}

// ------------------------------------------------------ destino da gravação ---

/// `(livres, total)` em bytes do volume onde `dir` está ou vai estar. O
/// directório das gravações cria-se na primeira gravação, por isso mede-se o
/// ascendente mais próximo que exista — é o volume onde ele vai nascer.
/// `None` se nada se deixou medir.
pub(crate) fn disk_space(dir: &std::path::Path) -> Option<(u64, u64)> {
    let dir = if dir.is_relative() {
        std::env::current_dir().ok()?.join(dir)
    } else {
        dir.to_path_buf()
    };
    let existing = dir.ancestors().find(|p| p.exists())?;
    let st = rustix::fs::statvfs(existing).ok()?;
    let frsize = if st.f_frsize > 0 {
        st.f_frsize
    } else {
        st.f_bsize
    };
    Some((
        st.f_bavail.saturating_mul(frsize),
        st.f_blocks.saturating_mul(frsize),
    ))
}

/// Para onde vão as gravações do estúdio, com o espaço livre real.
#[utoipa::path(
    get, path = "/api/orgs/{org_id}/studios/{studio_id}/recording-target", tag = "studio",
    security(("session" = [])),
    params(("org_id" = Uuid, Path), ("studio_id" = Uuid, Path)),
    responses(
        (status = 200, body = RecordingTarget),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 404, body = crate::openapi::ErrorBody),
    )
)]
pub async fn recording_target(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path((org_id, studio_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<RecordingTarget>, ApiError> {
    member(&state, org_id, auth.user_id).await?;
    let studio = load_studio(&state, org_id, studio_id).await?;
    let dir = state.config.recordings_dir.clone();
    let space = tokio::task::spawn_blocking(move || disk_space(&dir))
        .await
        .ok()
        .flatten();
    Ok(Json(RecordingTarget {
        kind: "local".into(),
        free_bytes: space.map(|s| s.0),
        total_bytes: space.map(|s| s.1),
        iso_recording: studio.iso_recording,
        object_storage: ObjectStorage {
            kind: "minio".into(),
            state: "not_configured".into(),
        },
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn o_espaco_do_disco_e_medido() {
        let (free, total) = disk_space(std::path::Path::new(".")).expect("statvfs");
        assert!(total > 0 && free <= total, "{free} / {total}");
        // Ainda por criar: mede o volume onde vai nascer.
        let (_, t2) = disk_space(std::path::Path::new("/nao/existe/mesmo")).expect("ascendente");
        assert!(t2 > 0);
    }
}
