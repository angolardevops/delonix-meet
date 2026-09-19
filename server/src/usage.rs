//! Uso de armazenamento (G3) — adaptador HTTP + Postgres. A regra da quota
//! está em `delonix_meet_domain::organization::storage_quota`.
//!
//! - `GET /api/orgs/{org_id}/storage-usage`  admin da org: gravações, quadros, quota
//! - `GET /api/users/me/storage-usage`       o próprio (edição pessoal): o que carregou
//!
//! **De quem é uma gravação.** A mesma atribuição das estatísticas da org
//! (`org::recording_uploader_in_org_sql`): conta para a organização de que quem a
//! carregou é (ou foi) membro. Quem saiu não leva a gravação consigo (S3), e
//! por isso ela continua a ocupar a quota da empresa. Os quadros têm `org_id`.

use axum::{
    extract::{Path, State},
    Json,
};
use delonix_meet_domain::organization::storage_quota::{self as rules, Usage};
use serde::Serialize;
use std::sync::Arc;
use uuid::Uuid;

use crate::{auth::AuthUser, error::ApiError, AppState};

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct UsageBucket {
    pub count: i64,
    pub bytes: i64,
}

/// Uso de armazenamento da organização e a sua quota.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct OrgStorageUsage {
    pub org_id: Uuid,
    /// Gravações (soma de `size_bytes`; as falhadas contam 0).
    pub recordings: UsageBucket,
    /// Quadros (bytes do PNG guardado).
    pub whiteboards: UsageBucket,
    /// `recordings.bytes + whiteboards.bytes`.
    pub used_bytes: i64,
    /// Tecto em bytes; `null` = ilimitado.
    pub max_storage_bytes: Option<i64>,
    /// Quanto ainda cabe (nunca negativo); `null` = ilimitado.
    pub remaining_bytes: Option<i64>,
}

/// Uso de armazenamento de quem está autenticado.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct UserStorageUsage {
    pub user_id: Uuid,
    /// Gravações que carregou.
    pub recordings: UsageBucket,
    /// Quadros de que é dono.
    pub whiteboards: UsageBucket,
    pub used_bytes: i64,
}

#[derive(utoipa::OpenApi)]
#[openapi(
    paths(org_storage_usage, my_storage_usage),
    components(schemas(UsageBucket, OrgStorageUsage, UserStorageUsage))
)]
pub struct ApiDoc;

type Row = (i64, i64, i64, i64);

fn buckets((rec_n, rec_b, wb_n, wb_b): Row) -> (UsageBucket, UsageBucket, Usage) {
    (
        UsageBucket {
            count: rec_n,
            bytes: rec_b,
        },
        UsageBucket {
            count: wb_n,
            bytes: wb_b,
        },
        Usage {
            recordings_bytes: rec_b,
            whiteboards_bytes: wb_b,
        },
    )
}

/// Armazenamento da organização. Só administradores; um membro recebe `403` e
/// quem não é da organização `404`.
#[utoipa::path(
    get, path = "/api/orgs/{org_id}/storage-usage", tag = "orgs",
    security(("session" = [])),
    params(("org_id" = Uuid, Path)),
    responses(
        (status = 200, body = OrgStorageUsage),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 403, description = "Membro sem papel de administrador.", body = crate::openapi::ErrorBody),
        (status = 404, description = "Não é membro da organização (ou não existe).", body = crate::openapi::ErrorBody),
    )
)]
pub async fn org_storage_usage(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(org_id): Path<Uuid>,
) -> Result<Json<OrgStorageUsage>, ApiError> {
    crate::org::require_admin_pub(&state, org_id, auth.user_id).await?;
    let in_org = crate::org::recording_uploader_in_org_sql("$1", "r.uploader_id");
    let (rec_n, rec_b, wb_n, wb_b, max): (i64, i64, i64, i64, Option<i64>) =
        sqlx::query_as(&format!(
            "SELECT
               (SELECT COUNT(*) FROM recordings r WHERE {in_org}),
               (SELECT COALESCE(SUM(r.size_bytes), 0)::bigint FROM recordings r WHERE {in_org}),
               (SELECT COUNT(*) FROM whiteboards w WHERE w.org_id = $1),
               (SELECT COALESCE(SUM(octet_length(w.png)), 0)::bigint FROM whiteboards w WHERE w.org_id = $1),
               (SELECT max_storage_bytes FROM organizations WHERE id = $1)"
        ))
        .bind(org_id)
        .fetch_one(&state.db)
        .await?;
    let row = (rec_n, rec_b, wb_n, wb_b);
    let (recordings, whiteboards, usage) = buckets(row);
    Ok(Json(OrgStorageUsage {
        org_id,
        recordings,
        whiteboards,
        used_bytes: usage.used_bytes(),
        max_storage_bytes: max,
        remaining_bytes: rules::remaining(usage, max),
    }))
}

/// Armazenamento de quem está autenticado: as gravações que carregou e os
/// quadros de que é dono. Serve a edição pessoal, onde não há administração de
/// organização para consultar.
#[utoipa::path(
    get, path = "/api/users/me/storage-usage", tag = "users",
    security(("session" = [])),
    responses(
        (status = 200, body = UserStorageUsage),
        (status = 401, body = crate::openapi::ErrorBody),
    )
)]
pub async fn my_storage_usage(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
) -> Result<Json<UserStorageUsage>, ApiError> {
    let row: Row = sqlx::query_as(
        "SELECT
           (SELECT COUNT(*) FROM recordings WHERE uploader_id = $1),
           (SELECT COALESCE(SUM(size_bytes), 0)::bigint FROM recordings WHERE uploader_id = $1),
           (SELECT COUNT(*) FROM whiteboards WHERE owner_id = $1),
           (SELECT COALESCE(SUM(octet_length(png)), 0)::bigint FROM whiteboards WHERE owner_id = $1)",
    )
    .bind(auth.user_id)
    .fetch_one(&state.db)
    .await?;
    let (recordings, whiteboards, usage) = buckets(row);
    Ok(Json(UserStorageUsage {
        user_id: auth.user_id,
        recordings,
        whiteboards,
        used_bytes: usage.used_bytes(),
    }))
}

/// A quota perante uma gravação NOVA de `incoming` bytes carregada por
/// `uploader_id`. Verifica-se em cada organização a que a gravação vai contar
/// e que tenha tecto; a primeira que não a comporte recusa com
/// `storage.quota_exceeded` (422).
///
/// Limite honesto: não é transaccional com o `INSERT`. Dois carregamentos
/// simultâneos podem ambos caber na leitura e, juntos, passar o tecto — por no
/// máximo o tamanho de um deles.
pub(crate) async fn enforce_recording_quota(
    state: &AppState,
    uploader_id: Uuid,
    incoming: i64,
) -> Result<(), ApiError> {
    let in_org_row = crate::org::recording_uploader_in_org_sql("o.id", "r.uploader_id");
    let in_org_uploader = crate::org::recording_uploader_in_org_sql("o.id", "$1");
    let limits: Vec<(i64, i64, i64)> = sqlx::query_as(&format!(
        "SELECT o.max_storage_bytes,
                (SELECT COALESCE(SUM(r.size_bytes), 0)::bigint FROM recordings r WHERE {in_org_row}),
                (SELECT COALESCE(SUM(octet_length(w.png)), 0)::bigint FROM whiteboards w WHERE w.org_id = o.id)
           FROM organizations o
          WHERE o.max_storage_bytes IS NOT NULL AND {in_org_uploader}"
    ))
    .bind(uploader_id)
    .fetch_all(&state.db)
    .await?;
    for (max, recordings_bytes, whiteboards_bytes) in limits {
        rules::check_upload(
            Usage {
                recordings_bytes,
                whiteboards_bytes,
            },
            incoming,
            Some(max),
        )?;
    }
    Ok(())
}
