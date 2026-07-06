use axum::{
    extract::{Path, State},
    Json,
};
use base64::Engine;
use chrono::{DateTime, Utc};
use hmac::{Hmac, Mac};
use rand::Rng;
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha1::Sha1;
use std::sync::Arc;
use uuid::Uuid;

use crate::{
    auth::{sign_jwt, AuthUser, Claims},
    error::ApiError,
    AppState,
};

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct Room {
    pub id: Uuid,
    pub code: String,
    pub name: String,
    pub owner_id: Uuid,
    pub topology: String,
    pub waiting_room: bool,
    pub e2ee: bool,
    pub created_at: DateTime<Utc>,
}

/// Meet-style room code: `abc-defg-hij`, unambiguous lowercase letters.
pub fn generate_room_code() -> String {
    const ALPHABET: &[u8] = b"abcdefghijkmnpqrstuvwxyz";
    let mut rng = rand::thread_rng();
    let mut part = |len: usize| -> String {
        (0..len)
            .map(|_| ALPHABET[rng.gen_range(0..ALPHABET.len())] as char)
            .collect()
    };
    format!("{}-{}-{}", part(3), part(4), part(3))
}

#[derive(Deserialize)]
pub struct CreateRoomReq {
    pub name: String,
    #[serde(default)]
    pub topology: Option<String>,
    #[serde(default)]
    pub waiting_room: bool,
    /// Encriptação ponta-a-ponta do media (a chave nunca passa pelo servidor).
    #[serde(default)]
    pub e2ee: bool,
}

/// Cria uma sala (com retry em colisão de código). Reutilizado pelo endpoint
/// e pelo arranque de reuniões agendadas.
pub async fn insert_room(
    db: &sqlx::PgPool,
    owner_id: Uuid,
    name: &str,
    topology: &str,
    waiting_room: bool,
    e2ee: bool,
) -> Result<Room, ApiError> {
    for _ in 0..5 {
        let code = generate_room_code();
        let res: Result<Room, sqlx::Error> = sqlx::query_as(
            "INSERT INTO rooms (code, name, owner_id, topology, waiting_room, e2ee) VALUES ($1, $2, $3, $4, $5, $6)
             RETURNING id, code, name, owner_id, topology, waiting_room, e2ee, created_at",
        )
        .bind(&code)
        .bind(name)
        .bind(owner_id)
        .bind(topology)
        .bind(waiting_room)
        .bind(e2ee)
        .fetch_one(db)
        .await;
        match res {
            Ok(room) => return Ok(room),
            Err(sqlx::Error::Database(dbe)) if dbe.is_unique_violation() => continue,
            Err(e) => return Err(e.into()),
        }
    }
    Err(ApiError::internal("could not allocate room code"))
}

pub async fn create_room(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Json(req): Json<CreateRoomReq>,
) -> Result<Json<Room>, ApiError> {
    let name = req.name.trim();
    if name.is_empty() || name.len() > 100 {
        return Err(ApiError::BadRequest("room name must be 1-100 chars".into()));
    }
    let topology = req.topology.as_deref().unwrap_or("sfu");
    if !matches!(topology, "mesh" | "sfu") {
        return Err(ApiError::BadRequest("topology must be 'mesh' or 'sfu'".into()));
    }
    let room = insert_room(&state.db, auth.user_id, name, topology, req.waiting_room, req.e2ee).await?;
    Ok(Json(room))
}

pub async fn get_room(
    State(state): State<Arc<AppState>>,
    _auth: AuthUser,
    Path(code): Path<String>,
) -> Result<Json<Room>, ApiError> {
    let room: Room = sqlx::query_as(
        "SELECT id, code, name, owner_id, topology, waiting_room, e2ee, created_at FROM rooms WHERE code = $1",
    )
    .bind(code.to_lowercase())
    .fetch_one(&state.db)
    .await?;
    Ok(Json(room))
}

/// Exchange an access token for a short-lived, signed **room token** — the
/// only credential the signaling WebSocket accepts. Scoped to one room and
/// expiring in minutes, it prevents room hijacking with stolen/old URLs.
pub async fn join_room(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(code): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let room: Room = sqlx::query_as(
        "SELECT id, code, name, owner_id, topology, waiting_room, e2ee, created_at FROM rooms WHERE code = $1",
    )
    .bind(code.to_lowercase())
    .fetch_one(&state.db)
    .await?;
    let user = crate::users::fetch_public(&state.db, auth.user_id).await?;

    // Registar participação (para a biblioteca "gravações onde participei").
    sqlx::query(
        "INSERT INTO room_participants (room_id, user_id) VALUES ($1, $2)
         ON CONFLICT (room_id, user_id) DO NOTHING",
    )
    .bind(room.id)
    .bind(auth.user_id)
    .execute(&state.db)
    .await?;

    let now = Utc::now().timestamp();
    let room_token = sign_jwt(
        &state.config.jwt_secret,
        &Claims {
            sub: auth.user_id,
            typ: "room".into(),
            iat: now,
            exp: now + state.config.room_token_ttl_secs,
            room: Some(room.id),
            name: Some(user.username),
            topo: Some(room.topology.clone()),
            owner: room.owner_id == auth.user_id,
            wait: room.waiting_room,
        },
    )?;

    Ok(Json(json!({
        "room": room,
        "room_token": room_token,
        "ws_path": format!("/ws?token={room_token}"),
    })))
}

/// Time-limited TURN credentials (coturn `use-auth-secret` / REST API spec):
/// username = expiry unix ts, password = base64(HMAC-SHA1(secret, username)).
pub async fn ice_servers(
    State(state): State<Arc<AppState>>,
    _auth: AuthUser,
) -> Result<Json<serde_json::Value>, ApiError> {
    let expiry = Utc::now().timestamp() + 3600;
    let username = expiry.to_string();
    let mut mac = Hmac::<Sha1>::new_from_slice(state.config.turn_secret.as_bytes())
        .map_err(ApiError::internal)?;
    mac.update(username.as_bytes());
    let credential = base64::engine::general_purpose::STANDARD.encode(mac.finalize().into_bytes());

    Ok(Json(json!({
        "iceServers": [
            { "urls": [format!("stun:{}", state.config.turn_host)] },
            {
                "urls": [format!("turn:{}", state.config.turn_host)],
                "username": username,
                "credential": credential,
            }
        ]
    })))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn room_code_shape() {
        for _ in 0..100 {
            let code = generate_room_code();
            let parts: Vec<&str> = code.split('-').collect();
            assert_eq!(parts.len(), 3);
            assert_eq!(parts[0].len(), 3);
            assert_eq!(parts[1].len(), 4);
            assert_eq!(parts[2].len(), 3);
            assert!(code.chars().all(|c| c.is_ascii_lowercase() || c == '-'));
            // No ambiguous letters.
            assert!(!code.contains('l') && !code.contains('o'));
        }
    }

    #[test]
    fn room_codes_are_random() {
        let a = generate_room_code();
        let b = generate_room_code();
        assert_ne!(a, b);
    }
}
