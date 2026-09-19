//! Inventário de nós de media (G10) — batimento por pod e a superfície de
//! OPERADOR que o lê (`/api/operator/v1/nodes`, ADR-0004 §4).
//!
//! Só o administrador da PLATAFORMA (`PLATFORM_ADMIN_USER_IDS`) lê: o
//! inventário diz quantas salas e pessoas há em cada nó, de todos os
//! inquilinos juntos — não é informação de uma organização.

use std::sync::{atomic::Ordering, Arc};

use axum::{extract::State, Json};
use chrono::{DateTime, Utc};
use delonix_meet_core::DomainError;
use delonix_meet_domain::operations::media_node as rules;
use serde::Serialize;
use uuid::Uuid;

use crate::{auth::AuthUser, error::ApiError, AppState};

#[derive(Serialize, sqlx::FromRow)]
struct NodeRow {
    node_id: Uuid,
    hostname: String,
    version: String,
    edition: String,
    started_at: DateTime<Utc>,
    last_seen_at: DateTime<Utc>,
    draining: bool,
    rooms: i32,
    peers: i32,
    ws_connections: i32,
    live_broadcasts: i32,
    peer_capacity: Option<i32>,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct MediaNode {
    pub node_id: Uuid,
    pub hostname: String,
    pub version: String,
    pub edition: String,
    pub started_at: DateTime<Utc>,
    pub last_seen_at: DateTime<Utc>,
    /// `serving` | `draining` | `unreachable` — derivado do último batimento.
    #[schema(value_type = String)]
    pub status: rules::NodeStatus,
    pub rooms: i32,
    pub peers: i32,
    pub ws_connections: i32,
    pub live_broadcasts: i32,
    /// Capacidade declarada (`NODE_PEER_CAPACITY`), se houver.
    pub peer_capacity: Option<i32>,
    /// Ocupação 0–1 face à capacidade declarada; ausente sem capacidade.
    pub load: Option<f64>,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct MediaNodeList {
    pub items: Vec<MediaNode>,
    pub serving: usize,
    pub draining: usize,
    pub unreachable: usize,
    /// Participantes em nós que respondem.
    pub peers: i64,
}

#[derive(utoipa::OpenApi)]
#[openapi(paths(list), components(schemas(MediaNode, MediaNodeList)))]
pub struct ApiDoc;

fn hostname() -> String {
    std::env::var("HOSTNAME")
        .ok()
        .filter(|h| !h.is_empty())
        .or_else(|| {
            std::fs::read_to_string("/etc/hostname")
                .ok()
                .map(|h| h.trim().to_string())
        })
        .unwrap_or_else(|| "desconhecido".into())
}

/// Escreve o batimento DESTE nó. Chamado pelo ciclo em `run()`.
pub async fn heartbeat(state: &AppState, started_at: DateTime<Utc>) -> Result<(), sqlx::Error> {
    let rooms = state.hub.rooms.len() as i32;
    let peers = state.hub.peers_ligados() as i32;
    let ws = (state.metrics.ws_signaling.load(Ordering::Relaxed)
        + state.metrics.ws_presence.load(Ordering::Relaxed))
    .max(0) as i32;
    let live = state.directos.quantas().await as i32;
    sqlx::query(
        "INSERT INTO media_nodes (node_id, hostname, version, edition, started_at, last_seen_at,
                                  draining, rooms, peers, ws_connections, live_broadcasts, peer_capacity)
         VALUES ($1, $2, $3, $4, $5, now(), $6, $7, $8, $9, $10, $11)
         ON CONFLICT (node_id) DO UPDATE SET
            last_seen_at = now(), draining = EXCLUDED.draining, rooms = EXCLUDED.rooms,
            peers = EXCLUDED.peers, ws_connections = EXCLUDED.ws_connections,
            live_broadcasts = EXCLUDED.live_broadcasts, peer_capacity = EXCLUDED.peer_capacity",
    )
    .bind(*crate::pubsub::NODE_ID)
    .bind(hostname())
    .bind(env!("CARGO_PKG_VERSION"))
    .bind(format!("{:?}", state.config.edition).to_lowercase())
    .bind(started_at)
    .bind(state.draining.load(Ordering::Relaxed))
    .bind(rooms)
    .bind(peers)
    .bind(ws)
    .bind(live)
    .bind(state.config.node_peer_capacity.map(|c| c as i32))
    .execute(&state.db)
    .await?;
    Ok(())
}

/// Esquece nós sem sinal há mais de um dia (pods substituídos por rollouts).
pub async fn forget_old(db: &sqlx::PgPool) -> Result<u64, sqlx::Error> {
    Ok(sqlx::query(
        "DELETE FROM media_nodes WHERE last_seen_at < now() - make_interval(hours => $1)",
    )
    .bind(rules::FORGET_AFTER_HOURS as i32)
    .execute(db)
    .await?
    .rows_affected())
}

/// Nós de media da plataforma e o seu estado.
#[utoipa::path(
    get, path = "/api/operator/v1/nodes", tag = "operator",
    security(("session" = [])),
    responses(
        (status = 200, body = MediaNodeList),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 403, description = "Não é administrador da plataforma.", body = crate::openapi::ErrorBody),
        (status = 404, description = "Edição sem superfície de operador (pessoal).", body = crate::openapi::ErrorBody),
    )
)]
pub async fn list(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
) -> Result<Json<MediaNodeList>, ApiError> {
    if !state.config.edition.operator_surface() {
        return Err(DomainError::not_found("operator.surface_disabled").into());
    }
    crate::storage::require_platform_admin(&state, auth.user_id)?;
    let rows: Vec<NodeRow> = sqlx::query_as(
        "SELECT node_id, hostname, version, edition, started_at, last_seen_at, draining,
                rooms, peers, ws_connections, live_broadcasts, peer_capacity
           FROM media_nodes ORDER BY hostname, started_at",
    )
    .fetch_all(&state.db)
    .await?;
    let now = Utc::now();
    let items: Vec<MediaNode> = rows
        .into_iter()
        .map(|r| {
            let status = rules::status(r.last_seen_at, r.draining, now);
            MediaNode {
                load: rules::load_ratio(r.peers as i64, r.peer_capacity.map(i64::from)),
                node_id: r.node_id,
                hostname: r.hostname,
                version: r.version,
                edition: r.edition,
                started_at: r.started_at,
                last_seen_at: r.last_seen_at,
                status,
                rooms: r.rooms,
                peers: r.peers,
                ws_connections: r.ws_connections,
                live_broadcasts: r.live_broadcasts,
                peer_capacity: r.peer_capacity,
            }
        })
        .collect();
    let count = |s| items.iter().filter(|n| n.status == s).count();
    Ok(Json(MediaNodeList {
        serving: count(rules::NodeStatus::Serving),
        draining: count(rules::NodeStatus::Draining),
        unreachable: count(rules::NodeStatus::Unreachable),
        peers: items
            .iter()
            .filter(|n| n.status != rules::NodeStatus::Unreachable)
            .map(|n| n.peers as i64)
            .sum(),
        items,
    }))
}
