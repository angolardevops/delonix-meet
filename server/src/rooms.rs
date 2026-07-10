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
    /// 'normal' (por defeito) ou 'training' — só treino permite salas de grupo.
    pub format: String,
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
    /// 'normal' (por defeito) ou 'training' (ativa salas de grupo).
    #[serde(default)]
    pub format: Option<String>,
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
    format: &str,
) -> Result<Room, ApiError> {
    for _ in 0..5 {
        let code = generate_room_code();
        let res: Result<Room, sqlx::Error> = sqlx::query_as(
            "INSERT INTO rooms (code, name, owner_id, topology, waiting_room, e2ee, format) VALUES ($1, $2, $3, $4, $5, $6, $7)
             RETURNING id, code, name, owner_id, topology, waiting_room, e2ee, format, created_at",
        )
        .bind(&code)
        .bind(name)
        .bind(owner_id)
        .bind(topology)
        .bind(waiting_room)
        .bind(e2ee)
        .bind(format)
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
        return Err(ApiError::BadRequest(
            "topology must be 'mesh' or 'sfu'".into(),
        ));
    }
    let format = req.format.as_deref().unwrap_or("normal");
    if !matches!(format, "normal" | "training") {
        return Err(ApiError::BadRequest(
            "format must be 'normal' or 'training'".into(),
        ));
    }
    let room = insert_room(
        &state.db,
        auth.user_id,
        name,
        topology,
        req.waiting_room,
        req.e2ee,
        format,
    )
    .await?;
    Ok(Json(room))
}

pub async fn get_room(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(code): Path<String>,
) -> Result<Json<Room>, ApiError> {
    let room: Room = sqlx::query_as(
        "SELECT id, code, name, owner_id, topology, waiting_room, e2ee, format, created_at FROM rooms WHERE code = $1",
    )
    .bind(code.to_lowercase())
    .fetch_one(&state.db)
    .await?;
    // O código da sala é a credencial (capability, estilo Meet): quem o conhece
    // pode ver os metadados e pedir para entrar. O controlo de acesso à REUNIÃO
    // ao vivo faz-se no join_room (não-membros vão para a sala de espera).
    let _ = auth;
    Ok(Json(room))
}

/// Resultado da verificação de acesso a uma sala.
pub struct RoomAccess {
    /// Pode aceder à sala (metadados, participação, gravações) — dono, colega
    /// de organização do dono, ou convidado explícito de uma reunião.
    pub authorized: bool,
    /// Entra DIRETO, sem sala de espera — o dono, quem está na agenda da
    /// reunião (meeting_invitees) para este código, ou um co-anfitrião de
    /// admissões persistido. Um colega de org que só recebeu o link (sem estar
    /// na agenda) é `authorized` mas NÃO `direct`: aguarda admissão.
    pub direct: bool,
    /// Pode admitir convidados: o dono ou um co-anfitrião persistido em
    /// `room_admitters`. Vai no token (`adm`) e habilita a sala de espera.
    pub admitter: bool,
}

/// Autorização de acesso a uma sala com distinção entre entrada direta (agenda/
/// co-anfitrião) e entrada por link (aguarda aprovação). Fecha o buraco de
/// qualquer utilizador entrar em qualquer sala.
pub async fn room_access(
    state: &AppState,
    user_id: Uuid,
    room: &Room,
) -> Result<RoomAccess, ApiError> {
    if room.owner_id == user_id {
        return Ok(RoomAccess {
            authorized: true,
            direct: true,
            admitter: true,
        });
    }
    // Uma única consulta devolve os sinais: colega de org (→ authorized),
    // convidado na agenda desta sala (→ direct) e co-anfitrião persistido
    // (→ direct + admitter, entra sem esperar e pode admitir outros).
    let (org_mate, invitee, admitter): (bool, bool, bool) = sqlx::query_as(
        r#"SELECT
             EXISTS(SELECT 1 FROM org_members a JOIN org_members b ON a.org_id = b.org_id
                    WHERE a.user_id = $1 AND b.user_id = $2),
             EXISTS(SELECT 1 FROM meeting_invitees mi JOIN meetings m ON m.id = mi.meeting_id
                    WHERE m.room_code = $3 AND mi.user_id = $1),
             EXISTS(SELECT 1 FROM room_admitters WHERE room_id = $4 AND user_id = $1)"#,
    )
    .bind(user_id)
    .bind(room.owner_id)
    .bind(&room.code)
    .bind(room.id)
    .fetch_one(&state.db)
    .await?;
    Ok(RoomAccess {
        authorized: org_mate || invitee || admitter,
        direct: invitee || admitter,
        admitter,
    })
}

/// Concede/revoga a um utilizador o estatuto de co-anfitrião de admissões,
/// persistido para reconexões. Chamado pelo anfitrião via sinalização.
pub async fn set_room_admitter(
    state: &AppState,
    room_id: Uuid,
    user_id: Uuid,
    granted_by: Uuid,
    allowed: bool,
) -> Result<(), ApiError> {
    if allowed {
        sqlx::query(
            "INSERT INTO room_admitters (room_id, user_id, granted_by) VALUES ($1, $2, $3)
             ON CONFLICT (room_id, user_id) DO NOTHING",
        )
        .bind(room_id)
        .bind(user_id)
        .bind(granted_by)
        .execute(&state.db)
        .await?;
    } else {
        sqlx::query("DELETE FROM room_admitters WHERE room_id = $1 AND user_id = $2")
            .bind(room_id)
            .bind(user_id)
            .execute(&state.db)
            .await?;
    }
    Ok(())
}

/// Compat: só o sinal de acesso (usado onde a distinção direto/espera não importa).
pub async fn can_access_room(
    state: &AppState,
    user_id: Uuid,
    room: &Room,
) -> Result<bool, ApiError> {
    Ok(room_access(state, user_id, room).await?.authorized)
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
        "SELECT id, code, name, owner_id, topology, waiting_room, e2ee, format, created_at FROM rooms WHERE code = $1",
    )
    .bind(code.to_lowercase())
    .fetch_one(&state.db)
    .await?;
    // Quem tem o código pode entrar (link-join estilo Meet), MAS só entra DIRETO
    // quem é dono ou está na AGENDA da reunião (convidado explícito). Um colega
    // de organização que apenas recebeu o link — ou um externo — vai para a SALA
    // DE ESPERA e é admitido pelo anfitrião (ou por um co-anfitrião promovido).
    // Isto reconcilia o isolamento multi-tenant com a partilha por link.
    let mut access = room_access(&state, auth.user_id, &room).await?;
    if state.presence.is_invited(&room.code, auth.user_id) {
        access.authorized = true;
        access.direct = true;
    }
    let authorized = access.authorized;
    let user = crate::users::fetch_public(&state.db, auth.user_id).await?;

    // Só regista participação (acesso a gravações/atas) para membros/convidados —
    // um convidado externo não ganha acesso ao histórico só por ter o link.
    if authorized {
        sqlx::query(
            "INSERT INTO room_participants (room_id, user_id) VALUES ($1, $2)
             ON CONFLICT (room_id, user_id) DO NOTHING",
        )
        .bind(room.id)
        .bind(auth.user_id)
        .execute(&state.db)
        .await?;
    }

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
            wait: room.waiting_room || !access.direct, // sem entrada direta → sala de espera
            adm: access.admitter, // anfitrião ou co-anfitrião persistido pode admitir
            is_bot: false,        // join normal de utilizador humano
        },
    )?;

    // `scheduled` = existe uma reunião agendada (agenda/calendário) para esta
    // sala. Chamadas instantâneas (sem agenda) são salas virtuais: o único
    // artefacto persistente é a gravação (a ata é no-op sem reunião associada).
    let scheduled: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM meetings WHERE room_code = $1)")
            .bind(&room.code)
            .fetch_one(&state.db)
            .await?;

    Ok(Json(json!({
        "room": room,
        "room_token": room_token,
        "ws_path": format!("/ws?token={room_token}"),
        "scheduled": scheduled,
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

    // `iceTransportPolicy: relay` força a media do CLIENTE a passar pelo TURN —
    // em K8s os host candidates do SFU não a transportam (ver Config).
    let mut cfg = json!({
        "iceServers": [
            { "urls": [format!("stun:{}", state.config.turn_host)] },
            {
                "urls": [format!("turn:{}", state.config.turn_host)],
                "username": username,
                "credential": credential,
            }
        ]
    });
    if state.config.force_turn_relay {
        cfg["iceTransportPolicy"] = json!("relay");
    }
    Ok(Json(cfg))
}

// ---------- Chat persistente ----------

#[derive(Serialize, sqlx::FromRow)]
pub struct ChatMessage {
    pub id: Uuid,
    pub user_id: Uuid,
    pub username: String,
    pub message: String,
    pub created_at: DateTime<Utc>,
}

/// Últimas 200 mensagens de chat de uma sala (requer autenticação + acesso).
pub async fn room_chat(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(code): Path<String>,
) -> Result<Json<Vec<ChatMessage>>, ApiError> {
    let room: Room = sqlx::query_as(
        "SELECT id, code, name, owner_id, topology, waiting_room, e2ee, format, created_at
         FROM rooms WHERE code = $1",
    )
    .bind(&code)
    .fetch_optional(&state.db)
    .await?
    .ok_or(ApiError::NotFound)?;

    if !can_access_room(&state, auth.user_id, &room).await? {
        return Err(ApiError::Unauthorized);
    }

    let msgs: Vec<ChatMessage> = sqlx::query_as(
        "SELECT id, user_id, username, message, created_at
         FROM room_chat_messages
         WHERE room_id = $1
         ORDER BY created_at ASC
         LIMIT 200",
    )
    .bind(room.id)
    .fetch_all(&state.db)
    .await?;

    Ok(Json(msgs))
}

// ---------- Convidar membros para sala em curso ----------

#[derive(Deserialize)]
pub struct InviteReq {
    pub targets: Vec<Uuid>,
    #[serde(default)]
    pub kind: Option<String>,
}

pub async fn invite_to_room(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(code): Path<String>,
    Json(req): Json<InviteReq>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let room: Room = sqlx::query_as(
        "SELECT id, code, name, owner_id, topology, waiting_room, e2ee, format, created_at
         FROM rooms WHERE code = $1",
    )
    .bind(&code)
    .fetch_optional(&state.db)
    .await?
    .ok_or(ApiError::NotFound)?;

    if !can_access_room(&state, auth.user_id, &room).await? {
        return Err(ApiError::Unauthorized);
    }

    let kind = req.kind.as_deref().unwrap_or("video");
    if !matches!(kind, "video" | "voice") {
        return Err(ApiError::BadRequest(
            "kind must be 'video' or 'voice'".into(),
        ));
    }
    if req.targets.is_empty() || req.targets.len() > 50 {
        return Err(ApiError::BadRequest("targets must be 1–50 users".into()));
    }

    let caller_user = crate::users::fetch_public(&state.db, auth.user_id).await?;

    // Isolamento multi-tenant: só colegas da mesma org.
    let co: std::collections::HashSet<Uuid> = crate::org::org_co_members(&state, auth.user_id)
        .await
        .into_iter()
        .collect();
    let targets: std::collections::HashSet<Uuid> = req
        .targets
        .into_iter()
        .filter(|u| *u != auth.user_id && co.contains(u))
        .collect();

    if targets.is_empty() {
        return Err(ApiError::BadRequest("sem destinatários válidos".into()));
    }

    // Registar a chamada no hub (para que accept/decline funcione e não vá para a sala de espera).
    state
        .presence
        .register_call(room.code.clone(), auth.user_id, targets.clone());

    let title = format!("Convite de {} para a reunião", caller_user.username);
    let (ringing, offline) = crate::presence::ring_users(
        &state,
        auth.user_id,
        &caller_user.username,
        targets,
        &room.code,
        kind,
        &title,
    )
    .await;

    Ok(Json(serde_json::json!({
        "ringing": ringing,
        "offline": offline,
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
