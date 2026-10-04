//! Sessões de emissão de um canal de TV (RFC-0001, Fase 1) — adaptador HTTP +
//! Postgres. Estados e transições em `delonix_meet_domain::content::tv_broadcast`.
//!
//! Contrato:
//! - `POST /api/orgs/{org_id}/tv/channels/{channel_id}/broadcasts`                      pedir «no ar» (`201`)
//! - `GET  /api/orgs/{org_id}/tv/channels/{channel_id}/broadcasts`                      histórico, mais recente primeiro
//! - `GET  /api/orgs/{org_id}/tv/channels/{channel_id}/broadcasts/{broadcast_id}`       uma sessão
//! - `POST /api/orgs/{org_id}/tv/channels/{channel_id}/broadcasts/{broadcast_id}/stop`  pedir paragem (idempotente)
//!
//! **Pedir não é estar no ar.** Estas rotas só escrevem a INTENÇÃO
//! (`desired_state`). O estado observado (`state`) é do executor, que ainda não
//! existe: até lá, uma sessão fica em `requested` e a interface não pode
//! apresentá-la como emissão (RNF-14). Pôr no ar é `broadcast.go_live`, distinta
//! de preparar o canal (`broadcast.manage_channels`, RF-19).
//!
//! Abaixo ficam, sem rota, as primitivas do **lease com fencing** que o
//! executor usará ([`claim`], [`renew`], [`report`]): provadas aqui contra
//! Postgres, para que a próxima fatia as encontre prontas.

use axum::{
    extract::{Path, Query, State},
    http::{header::LOCATION, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use chrono::{DateTime, Utc};
use delonix_meet_core::{
    page::{Page, PageRequest},
    DomainError,
};
use delonix_meet_domain::content::tv_broadcast::State as BState;
use delonix_meet_domain::identity::authorization::{Capability, ResourceScope};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use uuid::Uuid;

use crate::{auth::AuthUser, error::ApiError, AppState};

/// Quanto dura um lease sem ser renovado. O executor renova a meio.
#[allow(dead_code)] // usado pelo executor (próxima fatia) e pelos testes
pub(crate) const LEASE_SECS: f64 = 30.0;

#[derive(Debug, Clone, Serialize, sqlx::FromRow, utoipa::ToSchema)]
pub struct TvBroadcast {
    pub id: Uuid,
    pub org_id: Uuid,
    pub channel_id: Uuid,
    /// A intenção de quem produz: `live` | `stopped`.
    pub desired_state: String,
    /// O que o executor observou: `requested` | `starting` | `live` | `ending` | `ended` | `failed`.
    /// Só `live` quer dizer que o sinal está a sair.
    pub state: String,
    pub failure_reason: Option<String>,
    pub requested_by: Uuid,
    pub requested_at: DateTime<Utc>,
    pub started_at: Option<DateTime<Utc>>,
    pub ended_at: Option<DateTime<Utc>>,
}

const COLUMNS: &str = "id, org_id, channel_id, desired_state, state, failure_reason, \
                       requested_by, requested_at, started_at, ended_at";

#[derive(Serialize, utoipa::ToSchema)]
pub struct TvBroadcastPage {
    pub items: Vec<TvBroadcast>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_page_token: Option<String>,
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

async fn require_go_live(state: &AppState, org_id: Uuid, user: Uuid) -> Result<(), ApiError> {
    crate::org::require_capability(
        state,
        org_id,
        user,
        Capability::BroadcastGoLive,
        ResourceScope::Organization,
    )
    .await
    .map(|_| ())
}

/// Ver o histórico: quem prepara OU quem põe no ar.
async fn require_view(state: &AppState, org_id: Uuid, user: Uuid) -> Result<(), ApiError> {
    match crate::org::require_capability(
        state,
        org_id,
        user,
        Capability::BroadcastManageChannels,
        ResourceScope::Organization,
    )
    .await
    {
        Ok(_) => Ok(()),
        Err(_) => require_go_live(state, org_id, user).await,
    }
}

/// O canal existe nesta organização (senão `404`, também para o de outra).
async fn require_channel(state: &AppState, org_id: Uuid, channel_id: Uuid) -> Result<(), ApiError> {
    let found: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM tv_channels WHERE id = $1 AND org_id = $2)",
    )
    .bind(channel_id)
    .bind(org_id)
    .fetch_one(&state.db)
    .await?;
    if found {
        Ok(())
    } else {
        Err(ApiError::NotFound)
    }
}

#[derive(utoipa::OpenApi)]
#[openapi(
    paths(request_live, list, get_one, request_stop),
    components(schemas(TvBroadcast, TvBroadcastPage))
)]
pub struct ApiDoc;

/// Pede a emissão do canal. Cria uma sessão em `requested`: o estado «no ar»
/// só o executor o escreve. Só uma sessão por terminar por canal.
#[utoipa::path(
    post, path = "/api/orgs/{org_id}/tv/channels/{channel_id}/broadcasts", tag = "tv-broadcasts",
    security(("session" = [])),
    params(("org_id" = Uuid, Path), ("channel_id" = Uuid, Path)),
    responses(
        (status = 201, body = TvBroadcast, headers(("Location" = String))),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 404, body = crate::openapi::ErrorBody),
        (status = 409, body = crate::openapi::ErrorBody, description = "`tv.broadcast.already_active`"),
    )
)]
pub async fn request_live(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path((org_id, channel_id)): Path<(Uuid, Uuid)>,
) -> Result<Response, ApiError> {
    require_go_live(&state, org_id, auth.user_id).await?;
    require_channel(&state, org_id, channel_id).await?;
    let id = Uuid::new_v4();
    let inserted: Result<TvBroadcast, sqlx::Error> = sqlx::query_as(&format!(
        "INSERT INTO tv_broadcast_sessions (id, org_id, channel_id, requested_by)
         VALUES ($1, $2, $3, $4) RETURNING {COLUMNS}"
    ))
    .bind(id)
    .bind(org_id)
    .bind(channel_id)
    .bind(auth.user_id)
    .fetch_one(&state.db)
    .await;
    let session = match inserted {
        Ok(s) => s,
        Err(e)
            if e.as_database_error()
                .and_then(|d| d.constraint())
                .is_some_and(|c| c == "tv_broadcast_one_active_idx") =>
        {
            return Err(DomainError::conflict(
                "tv.broadcast.already_active",
                "este canal já tem uma emissão por terminar — pare-a antes de pedir outra",
            )
            .into())
        }
        Err(e) => return Err(e.into()),
    };
    crate::audit::log(
        &state.db,
        Some(org_id),
        auth.user_id,
        "tv.broadcast.requested",
        &format!("{id} (canal {channel_id})"),
    )
    .await;
    let location = format!("/api/orgs/{org_id}/tv/channels/{channel_id}/broadcasts/{id}");
    Ok((StatusCode::CREATED, [(LOCATION, location)], Json(session)).into_response())
}

/// Histórico de emissões do canal, da mais recente para a mais antiga.
#[utoipa::path(
    get, path = "/api/orgs/{org_id}/tv/channels/{channel_id}/broadcasts", tag = "tv-broadcasts",
    security(("session" = [])),
    params(("org_id" = Uuid, Path), ("channel_id" = Uuid, Path), ListQuery),
    responses(
        (status = 200, body = TvBroadcastPage),
        (status = 400, body = crate::openapi::ErrorBody, description = "page_token inválido"),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 404, body = crate::openapi::ErrorBody),
    )
)]
pub async fn list(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path((org_id, channel_id)): Path<(Uuid, Uuid)>,
    Query(q): Query<ListQuery>,
) -> Result<Json<TvBroadcastPage>, ApiError> {
    require_view(&state, org_id, auth.user_id).await?;
    require_channel(&state, org_id, channel_id).await?;
    let page = PageRequest {
        page_size: q.page_size,
        page_token: q.page_token,
    };
    let size = page.size();
    let cursor: Option<Cursor> = page.cursor()?;
    let rows: Vec<TvBroadcast> = sqlx::query_as(&format!(
        "SELECT {COLUMNS} FROM tv_broadcast_sessions
          WHERE org_id = $1 AND channel_id = $2
            AND ($3::timestamptz IS NULL OR (requested_at, id) < ($3, $4))
          ORDER BY requested_at DESC, id DESC
          LIMIT $5"
    ))
    .bind(org_id)
    .bind(channel_id)
    .bind(cursor.as_ref().map(|c| c.at))
    .bind(cursor.as_ref().map(|c| c.id).unwrap_or_default())
    .bind(size as i64 + 1)
    .fetch_all(&state.db)
    .await?;
    let p = Page::from_overfetch(rows, size, |s| Cursor {
        at: s.requested_at,
        id: s.id,
    });
    Ok(Json(TvBroadcastPage {
        items: p.items,
        next_page_token: p.next_page_token,
    }))
}

async fn fetch(
    state: &AppState,
    org_id: Uuid,
    channel_id: Uuid,
    id: Uuid,
) -> Result<TvBroadcast, ApiError> {
    sqlx::query_as(&format!(
        "SELECT {COLUMNS} FROM tv_broadcast_sessions
          WHERE id = $1 AND org_id = $2 AND channel_id = $3"
    ))
    .bind(id)
    .bind(org_id)
    .bind(channel_id)
    .fetch_optional(&state.db)
    .await?
    .ok_or(ApiError::NotFound)
}

/// Uma sessão.
#[utoipa::path(
    get, path = "/api/orgs/{org_id}/tv/channels/{channel_id}/broadcasts/{broadcast_id}", tag = "tv-broadcasts",
    security(("session" = [])),
    params(("org_id" = Uuid, Path), ("channel_id" = Uuid, Path), ("broadcast_id" = Uuid, Path)),
    responses(
        (status = 200, body = TvBroadcast),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 404, body = crate::openapi::ErrorBody),
    )
)]
pub async fn get_one(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path((org_id, channel_id, id)): Path<(Uuid, Uuid, Uuid)>,
) -> Result<Json<TvBroadcast>, ApiError> {
    require_view(&state, org_id, auth.user_id).await?;
    Ok(Json(fetch(&state, org_id, channel_id, id).await?))
}

/// Pede a paragem. Idempotente: parar uma sessão que já acabou devolve-a como
/// está. Uma sessão que ninguém chegou a arrancar (`requested`) fecha-se logo
/// (`ended`); as outras ficam com `desired_state = stopped` para o executor as
/// encerrar — o plano de controlo não decide o que o sinal está a fazer.
#[utoipa::path(
    post, path = "/api/orgs/{org_id}/tv/channels/{channel_id}/broadcasts/{broadcast_id}/stop", tag = "tv-broadcasts",
    security(("session" = [])),
    params(("org_id" = Uuid, Path), ("channel_id" = Uuid, Path), ("broadcast_id" = Uuid, Path)),
    responses(
        (status = 200, body = TvBroadcast),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 404, body = crate::openapi::ErrorBody),
    )
)]
pub async fn request_stop(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path((org_id, channel_id, id)): Path<(Uuid, Uuid, Uuid)>,
) -> Result<Json<TvBroadcast>, ApiError> {
    require_go_live(&state, org_id, auth.user_id).await?;
    let updated: Option<TvBroadcast> = sqlx::query_as(&format!(
        "UPDATE tv_broadcast_sessions SET
            desired_state = 'stopped',
            state = CASE WHEN state = 'requested' THEN 'ended' ELSE state END,
            ended_at = CASE WHEN state = 'requested' THEN now() ELSE ended_at END
          WHERE id = $1 AND org_id = $2 AND channel_id = $3 AND ended_at IS NULL
          RETURNING {COLUMNS}"
    ))
    .bind(id)
    .bind(org_id)
    .bind(channel_id)
    .fetch_optional(&state.db)
    .await?;
    let Some(session) = updated else {
        // Nenhuma linha por terminar: já acabou (devolve-se como está) ou não existe (404).
        return Ok(Json(fetch(&state, org_id, channel_id, id).await?));
    };
    crate::audit::log(
        &state.db,
        Some(org_id),
        auth.user_id,
        "tv.broadcast.stop_requested",
        &format!("{id} (canal {channel_id})"),
    )
    .await;
    Ok(Json(session))
}

// ---------------------------------------------------------------------------
//  Lease com fencing — as primitivas do executor (RNF-10)
// ---------------------------------------------------------------------------

/// Toma (ou renova) o lease da sessão para `executor`. `Some(token)` se o
/// lease é seu; `None` se outro o tem válido ou a sessão já acabou. Quem toma
/// o lease de OUTRO executor sobe o `fencing_token`: o antigo, ao acordar de
/// uma pausa, já não consegue escrever com o seu.
#[allow(dead_code)] // usado pelo executor (próxima fatia) e pelos testes
pub(crate) async fn claim(
    db: &sqlx::PgPool,
    session: Uuid,
    executor: &str,
) -> Result<Option<i64>, sqlx::Error> {
    sqlx::query_scalar(
        "UPDATE tv_broadcast_sessions SET
            executor_id = $2,
            lease_expires_at = now() + make_interval(secs => $3),
            fencing_token = CASE WHEN executor_id IS DISTINCT FROM $2
                                 THEN fencing_token + 1 ELSE fencing_token END
          WHERE id = $1 AND ended_at IS NULL
            AND (executor_id IS NULL OR executor_id = $2 OR lease_expires_at < now())
          RETURNING fencing_token",
    )
    .bind(session)
    .bind(executor)
    .bind(LEASE_SECS)
    .fetch_optional(db)
    .await
}

/// Renova o lease. `false` se já não é o dono — outro o tomou (token novo) ou
/// a sessão acabou. O executor que recebe `false` tem de parar de escrever.
#[allow(dead_code)] // usado pelo executor (próxima fatia) e pelos testes
pub(crate) async fn renew(
    db: &sqlx::PgPool,
    session: Uuid,
    executor: &str,
    token: i64,
) -> Result<bool, sqlx::Error> {
    let r = sqlx::query(
        "UPDATE tv_broadcast_sessions
            SET lease_expires_at = now() + make_interval(secs => $4)
          WHERE id = $1 AND executor_id = $2 AND fencing_token = $3 AND ended_at IS NULL",
    )
    .bind(session)
    .bind(executor)
    .bind(token)
    .bind(LEASE_SECS)
    .execute(db)
    .await?;
    Ok(r.rows_affected() == 1)
}

/// O executor regista o estado observado. Atómico: só aplica se o lease e o
/// token ainda são seus E a transição é permitida a partir do estado actual.
/// `false` = recusada (executor obsoleto, ou transição inválida).
#[allow(dead_code)] // usado pelo executor (próxima fatia) e pelos testes
pub(crate) async fn report(
    db: &sqlx::PgPool,
    session: Uuid,
    executor: &str,
    token: i64,
    to: BState,
    failure_reason: Option<&str>,
) -> Result<bool, sqlx::Error> {
    let from = BState::sources_of(to);
    let r = sqlx::query(
        "UPDATE tv_broadcast_sessions SET
            state = $5,
            failure_reason = CASE WHEN $5 = 'failed' THEN $6 ELSE failure_reason END,
            started_at = CASE WHEN $5 = 'live' THEN COALESCE(started_at, now()) ELSE started_at END,
            ended_at = CASE WHEN $5 IN ('ended', 'failed') THEN now() ELSE ended_at END
          WHERE id = $1 AND executor_id = $2 AND fencing_token = $3
            AND ended_at IS NULL AND state = ANY($4)",
    )
    .bind(session)
    .bind(executor)
    .bind(token)
    .bind(&from)
    .bind(to.as_str())
    .bind(failure_reason)
    .execute(db)
    .await?;
    Ok(r.rows_affected() == 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Uma org, um utilizador, um canal e uma sessão `requested`.
    async fn seed(db: &sqlx::PgPool) -> Uuid {
        sqlx::query_scalar(
            "WITH u AS (INSERT INTO users (email, username, password_hash)
                        VALUES ('a@x.test', 'a', 'x') RETURNING id),
                  o AS (INSERT INTO organizations (name, slug, created_by)
                        SELECT 'X', 'x', id FROM u RETURNING id, created_by),
                  c AS (INSERT INTO tv_channels (id, org_id, slug, name, created_by)
                        SELECT gen_random_uuid(), id, 'canal', 'Canal', created_by FROM o
                        RETURNING id, org_id, created_by)
             INSERT INTO tv_broadcast_sessions (id, org_id, channel_id, requested_by)
             SELECT gen_random_uuid(), org_id, id, created_by FROM c RETURNING id",
        )
        .fetch_one(db)
        .await
        .unwrap()
    }

    async fn expire_lease(db: &sqlx::PgPool, id: Uuid) {
        sqlx::query(
            "UPDATE tv_broadcast_sessions SET lease_expires_at = now() - interval '1 second'
              WHERE id = $1",
        )
        .bind(id)
        .execute(db)
        .await
        .unwrap();
    }

    async fn state_of(db: &sqlx::PgPool, id: Uuid) -> String {
        sqlx::query_scalar("SELECT state FROM tv_broadcast_sessions WHERE id = $1")
            .bind(id)
            .fetch_one(db)
            .await
            .unwrap()
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn so_um_executor_tem_o_lease_de_cada_vez(db: sqlx::PgPool) {
        let s = seed(&db).await;
        let a = claim(&db, s, "worker-a").await.unwrap();
        assert_eq!(a, Some(1));
        // O lease de A é válido: B não o toma.
        assert_eq!(claim(&db, s, "worker-b").await.unwrap(), None);
        // A pode voltar a pedi-lo (renovação por claim) sem mudar de token.
        assert_eq!(claim(&db, s, "worker-a").await.unwrap(), Some(1));
        assert!(renew(&db, s, "worker-a", 1).await.unwrap());
        // Um nome certo com o token errado também não renova.
        assert!(!renew(&db, s, "worker-a", 99).await.unwrap());
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn o_executor_obsoleto_e_vedado_pelo_fencing(db: sqlx::PgPool) {
        let s = seed(&db).await;
        let ta = claim(&db, s, "worker-a").await.unwrap().unwrap();
        assert!(report(&db, s, "worker-a", ta, BState::Starting, None)
            .await
            .unwrap());

        // A fica parado (GC, rede) e o lease expira; B toma-o e o token sobe.
        expire_lease(&db, s).await;
        let tb = claim(&db, s, "worker-b").await.unwrap().unwrap();
        assert!(tb > ta, "quem toma o lease de outro sobe o token");

        // A acorda e tenta escrever: renovar e reportar são ambos recusados.
        assert!(!renew(&db, s, "worker-a", ta).await.unwrap());
        assert!(!report(&db, s, "worker-a", ta, BState::Live, None)
            .await
            .unwrap());
        assert_eq!(
            state_of(&db, s).await,
            "starting",
            "a escrita de A não passou"
        );
        // B, o dono actual, continua.
        assert!(report(&db, s, "worker-b", tb, BState::Live, None)
            .await
            .unwrap());
        assert_eq!(state_of(&db, s).await, "live");
    }

    /// O caso em que SÓ o token protege: A perde o lease para B e volta a
    /// tomá-lo. Uma escrita atrasada da primeira posse de A (token 1) leva o
    /// mesmo `executor_id` do dono actual — o nome não a trava, o token sim.
    #[sqlx::test(migrations = "./migrations")]
    async fn o_token_trava_uma_posse_anterior_do_mesmo_executor(db: sqlx::PgPool) {
        let s = seed(&db).await;
        let t1 = claim(&db, s, "worker-a").await.unwrap().unwrap();
        assert!(report(&db, s, "worker-a", t1, BState::Starting, None)
            .await
            .unwrap());
        expire_lease(&db, s).await;
        let t2 = claim(&db, s, "worker-b").await.unwrap().unwrap();
        expire_lease(&db, s).await;
        let t3 = claim(&db, s, "worker-a").await.unwrap().unwrap();
        assert!(
            t1 < t2 && t2 < t3,
            "cada troca de dono sobe o token: {t1} {t2} {t3}"
        );

        // A escrita atrasada da posse 1 chega agora, com o nome certo e o token velho.
        assert!(!report(&db, s, "worker-a", t1, BState::Live, None)
            .await
            .unwrap());
        assert!(!renew(&db, s, "worker-a", t1).await.unwrap());
        assert_eq!(state_of(&db, s).await, "starting");
        // A posse actual escreve.
        assert!(report(&db, s, "worker-a", t3, BState::Live, None)
            .await
            .unwrap());
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn as_transicoes_invalidas_sao_recusadas(db: sqlx::PgPool) {
        let s = seed(&db).await;
        let t = claim(&db, s, "w").await.unwrap().unwrap();
        // Não se salta de `requested` para `live`.
        assert!(!report(&db, s, "w", t, BState::Live, None).await.unwrap());
        assert_eq!(state_of(&db, s).await, "requested");
        assert!(report(&db, s, "w", t, BState::Starting, None)
            .await
            .unwrap());
        assert!(report(&db, s, "w", t, BState::Live, None).await.unwrap());
        let started: Option<DateTime<Utc>> =
            sqlx::query_scalar("SELECT started_at FROM tv_broadcast_sessions WHERE id = $1")
                .bind(s)
                .fetch_one(&db)
                .await
                .unwrap();
        assert!(started.is_some(), "`live` marca a hora de início");
        assert!(report(&db, s, "w", t, BState::Ending, None).await.unwrap());
        assert!(report(&db, s, "w", t, BState::Ended, None).await.unwrap());
        // Terminada: liberta o canal e já ninguém lhe escreve nem lhe toma o lease.
        assert!(!report(&db, s, "w", t, BState::Live, None).await.unwrap());
        assert_eq!(claim(&db, s, "outro").await.unwrap(), None);
        assert!(!renew(&db, s, "w", t).await.unwrap());
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn falhar_guarda_a_razao_e_fecha_a_sessao(db: sqlx::PgPool) {
        let s = seed(&db).await;
        let t = claim(&db, s, "w").await.unwrap().unwrap();
        assert!(report(&db, s, "w", t, BState::Failed, Some("sem fonte"))
            .await
            .unwrap());
        let (state, reason, ended): (String, Option<String>, Option<DateTime<Utc>>) =
            sqlx::query_as(
                "SELECT state, failure_reason, ended_at FROM tv_broadcast_sessions WHERE id = $1",
            )
            .bind(s)
            .fetch_one(&db)
            .await
            .unwrap();
        assert_eq!(state, "failed");
        assert_eq!(reason.as_deref(), Some("sem fonte"));
        assert!(ended.is_some());
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn a_base_recusa_um_terminal_sem_hora_de_fim(db: sqlx::PgPool) {
        let s = seed(&db).await;
        let r = sqlx::query("UPDATE tv_broadcast_sessions SET state = 'ended' WHERE id = $1")
            .bind(s)
            .execute(&db)
            .await;
        assert!(r.is_err(), "terminal sem ended_at tem de ser impossível");
    }
}
