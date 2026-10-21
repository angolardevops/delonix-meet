//! Legal hold (A-08): uma organização suspende a retenção por ordem legal.
//!
//! **PORQUE EXISTE.** A retenção por dias apagava gravações e chat assim que
//! passavam do prazo. Uma organização sob ordem de um tribunal ou de um
//! regulador não tinha como dizer «isto não se apaga até ordem em contrário».
//!
//! **Contrato** (rotas da organização, atrás de `admin.change_retention` — pôr
//! dados sob hold É suspender a retenção, e quem a muda é quem a suspende):
//! - `GET  /api/orgs/{org_id}/legal-holds`                     os holds, activos e levantados
//! - `POST /api/orgs/{org_id}/legal-holds`                     abre um (`201`)
//! - `POST /api/orgs/{org_id}/legal-holds/{hold_id}/release`   levanta-o (método personalizado)
//!
//! **Onde vale:** na BASE, não aqui. As varreduras de retenção saltam o que está
//! retido (`recorder::retention_sweep`, `room_chat::retention_sweep`) e um
//! gatilho recusa o `DELETE` de uma gravação ou mensagem retida venha de onde
//! vier (migração 0129) — uma cascata, uma rota futura, um `psql`. Este módulo
//! só abre, lista e levanta.
//!
//! **Um hold nunca se apaga:** levanta-se, e fica o registo de quem o abriu,
//! quando, porquê, e quem o levantou. Cada um fica também na auditoria
//! encadeada.
//!
//! **O que NÃO faz (fatia seguinte):** a pesquisa de eDiscovery e a exportação
//! com manifesto — o critério 2 e 3 do A-08 —, que precisam de uma capacidade
//! própria: um investigador não é um administrador.

use std::sync::Arc;

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use chrono::{DateTime, Utc};
use delonix_meet_core::DomainError;
use delonix_meet_domain::identity::authorization::{Capability, ResourceScope};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{auth::AuthUser, error::ApiError, AppState};

#[derive(Serialize, sqlx::FromRow, utoipa::ToSchema)]
pub struct LegalHold {
    pub id: Uuid,
    pub name: String,
    /// A ordem legal: o processo, o ofício, quem a emitiu.
    pub reason: String,
    /// `org` | `user` | `room`.
    pub scope: String,
    pub user_id: Option<Uuid>,
    pub room_id: Option<Uuid>,
    pub created_by: Uuid,
    pub created_at: DateTime<Utc>,
    pub released_by: Option<Uuid>,
    pub released_at: Option<DateTime<Utc>>,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct LegalHoldList {
    pub items: Vec<LegalHold>,
}

#[derive(Deserialize, utoipa::ToSchema)]
pub struct CreateLegalHoldReq {
    pub name: String,
    pub reason: String,
    /// `org` (tudo da organização), `user` (o que uma pessoa gravou ou
    /// escreveu) ou `room` (o que aconteceu numa sala).
    pub scope: String,
    /// Obrigatório com `scope: user`.
    #[serde(default)]
    pub user_id: Option<Uuid>,
    /// Obrigatório com `scope: room` — o código da sala.
    #[serde(default)]
    pub room_code: Option<String>,
}

#[derive(utoipa::OpenApi)]
#[openapi(paths(list, create, release))]
pub struct ApiDoc;

const COLUNAS: &str =
    "id, name, reason, scope, user_id, room_id, created_by, created_at, released_by, released_at";

async fn exige(state: &AppState, org_id: Uuid, user_id: Uuid) -> Result<(), ApiError> {
    crate::org::require_capability(
        state,
        org_id,
        user_id,
        Capability::AdminChangeRetention,
        ResourceScope::Organization,
    )
    .await?;
    Ok(())
}

/// `GET /api/orgs/{org_id}/legal-holds`
#[utoipa::path(
    get, path = "/api/orgs/{org_id}/legal-holds", tag = "organizations",
    security(("session" = [])),
    params(("org_id" = Uuid, Path, description = "Organização.")),
    responses(
        (status = 200, body = LegalHoldList, description = "Os holds da organização, activos e levantados, mais recentes primeiro."),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 403, body = crate::openapi::ErrorBody, description = "Sem `admin.change_retention`."),
        (status = 404, body = crate::openapi::ErrorBody, description = "A organização não existe ou quem pede não é membro activo."),
    )
)]
pub async fn list(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(org_id): Path<Uuid>,
) -> Result<Json<LegalHoldList>, ApiError> {
    exige(&state, org_id, auth.user_id).await?;
    let items: Vec<LegalHold> = sqlx::query_as(&format!(
        "SELECT {COLUNAS} FROM legal_holds WHERE org_id = $1 ORDER BY created_at DESC"
    ))
    .bind(org_id)
    .fetch_all(&state.db)
    .await?;
    Ok(Json(LegalHoldList { items }))
}

/// `POST /api/orgs/{org_id}/legal-holds`
#[utoipa::path(
    post, path = "/api/orgs/{org_id}/legal-holds", tag = "organizations",
    security(("session" = [])),
    params(("org_id" = Uuid, Path, description = "Organização.")),
    request_body = CreateLegalHoldReq,
    responses(
        (status = 201, body = LegalHold, description = "Hold aberto. A partir daqui, a retenção não apaga o que ele cobre."),
        (status = 400, body = crate::openapi::ErrorBody, description = "`legal_hold.invalid`: nome ou razão vazios ou longos, âmbito desconhecido, falta o `user_id`/`room_code` do âmbito."),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 403, body = crate::openapi::ErrorBody, description = "Sem `admin.change_retention`."),
        (status = 404, body = crate::openapi::ErrorBody, description = "A organização, a pessoa ou a sala não são desta organização."),
    )
)]
pub async fn create(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(org_id): Path<Uuid>,
    Json(req): Json<CreateLegalHoldReq>,
) -> Result<(StatusCode, Json<LegalHold>), ApiError> {
    exige(&state, org_id, auth.user_id).await?;
    let invalido =
        |m: &str| -> ApiError { DomainError::invalid("legal_hold.invalid", m.to_string()).into() };
    let name = req.name.trim();
    let reason = req.reason.trim();
    if name.is_empty() || name.chars().count() > 200 {
        return Err(invalido("o nome tem de ter entre 1 e 200 caracteres"));
    }
    if reason.is_empty() || reason.chars().count() > 2000 {
        return Err(invalido(
            "a razão (a ordem legal) tem de ter entre 1 e 2000 caracteres",
        ));
    }
    let (user_id, room_id) = match req.scope.as_str() {
        "org" => (None, None),
        "user" => {
            let u = req
                .user_id
                .ok_or_else(|| invalido("o âmbito `user` precisa do `user_id`"))?;
            // Uma pessoa de OUTRA organização não se põe sob hold daqui: 404,
            // sem dizer se a conta existe.
            if !crate::org::is_or_was_member(&state, org_id, u).await? {
                return Err(ApiError::NotFound);
            }
            (Some(u), None)
        }
        "room" => {
            let code = req
                .room_code
                .as_deref()
                .ok_or_else(|| invalido("o âmbito `room` precisa do `room_code`"))?;
            let r = crate::org::room_of_org(&state, org_id, code.trim())
                .await?
                .ok_or(ApiError::NotFound)?;
            (None, Some(r))
        }
        _ => return Err(invalido("o âmbito é `org`, `user` ou `room`")),
    };
    let hold: LegalHold = sqlx::query_as(&format!(
        "INSERT INTO legal_holds (org_id, name, reason, scope, user_id, room_id, created_by)
         VALUES ($1, $2, $3, $4, $5, $6, $7) RETURNING {COLUNAS}"
    ))
    .bind(org_id)
    .bind(name)
    .bind(reason)
    .bind(&req.scope)
    .bind(user_id)
    .bind(room_id)
    .bind(auth.user_id)
    .fetch_one(&state.db)
    .await?;
    crate::audit::log(
        &state.db,
        Some(org_id),
        auth.user_id,
        "legal_hold.created",
        &format!("{} ({})", hold.id, hold.scope),
    )
    .await;
    Ok((StatusCode::CREATED, Json(hold)))
}

/// `POST /api/orgs/{org_id}/legal-holds/{hold_id}/release`
#[utoipa::path(
    post, path = "/api/orgs/{org_id}/legal-holds/{hold_id}/release", tag = "organizations",
    security(("session" = [])),
    params(
        ("org_id" = Uuid, Path, description = "Organização."),
        ("hold_id" = Uuid, Path, description = "O hold. De outra organização dá 404."),
    ),
    responses(
        (status = 200, body = LegalHold, description = "Hold levantado. A retenção volta a valer para o que ele cobria — na próxima volta da varredura."),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 403, body = crate::openapi::ErrorBody, description = "Sem `admin.change_retention`."),
        (status = 404, body = crate::openapi::ErrorBody, description = "Não existe nesta organização."),
        (status = 409, body = crate::openapi::ErrorBody, description = "`legal_hold.already_released`."),
    )
)]
pub async fn release(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path((org_id, hold_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<LegalHold>, ApiError> {
    exige(&state, org_id, auth.user_id).await?;
    let hold: Option<LegalHold> = sqlx::query_as(&format!(
        "UPDATE legal_holds SET released_at = now(), released_by = $3
          WHERE id = $1 AND org_id = $2 AND released_at IS NULL RETURNING {COLUNAS}"
    ))
    .bind(hold_id)
    .bind(org_id)
    .bind(auth.user_id)
    .fetch_optional(&state.db)
    .await?;
    let Some(hold) = hold else {
        let existe: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM legal_holds WHERE id = $1 AND org_id = $2)",
        )
        .bind(hold_id)
        .bind(org_id)
        .fetch_one(&state.db)
        .await?;
        return Err(if existe {
            DomainError::conflict("legal_hold.already_released", "este hold já foi levantado")
                .into()
        } else {
            ApiError::NotFound
        });
    };
    crate::audit::log(
        &state.db,
        Some(org_id),
        auth.user_id,
        "legal_hold.released",
        &hold.id.to_string(),
    )
    .await;
    Ok(Json(hold))
}
