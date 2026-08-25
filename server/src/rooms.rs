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

    // Em K8s o SFU é relay-only (pod 10.244.x inalcançável) e o cliente
    // também: ambos só têm candidato relay `coturn_ip:porta`. O "peer" de cada
    // alocação é então o PRÓPRIO IP do coturn (hairpin relay-a-relay), que o
    // coturn nega por omissão (403 Forbidden IP) → `peer rp=0` → vídeo preto.
    // Resolvido no COTURN, não no cliente: `--allowed-peer-ip=<relay-ip>`
    // autoriza o hairpin (ver deploy/run-host-coturn.sh; provado com
    // turnutils: 0% perda p/ o próprio IP com o flag). Mantemos o cliente
    // relay-only — forçá-lo a `all` num host multi-homed gera dezenas de host
    // candidates (explosão de ICE) que inundam o WS e derrubam a ligação.
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

/// Amostra de qualidade reportada pelo CLIENTE.
///
/// Tudo aqui vem de fora e é tratado como tal: cada campo é limitado a um
/// intervalo plausível antes de ser gravado (ver `clamp_opt`/`clamp_pct`). Um
/// cliente alterado que mandasse `nack: 2_000_000_000` não pode envenenar as
/// médias do painel do administrador nem rebentar um `INT`.
///
/// Todos os campos novos são `Option` com `#[serde(default)]`: um cliente com a
/// app em cache antiga continua a reportar só os três originais, e a amostra
/// dele continua a contar. Exigi-los perderia exactamente as amostras das
/// sessões mais problemáticas.
#[derive(Deserialize)]
pub struct QosSample {
    pub rtt_ms: Option<i32>,
    pub loss_pct: f32,
    pub up_kbps: i32,
    #[serde(default)]
    pub down_kbps: Option<i32>,
    #[serde(default)]
    pub jitter_ms: Option<i32>,
    /// Delonix Call Quality Score, 0–100 (ver `web/src/callQuality.ts`).
    #[serde(default)]
    pub score: Option<i32>,
    #[serde(default)]
    pub freeze_ms: Option<i32>,
    #[serde(default)]
    pub concealment_pct: Option<f32>,
    #[serde(default)]
    pub frames_dropped: Option<i32>,
    #[serde(default)]
    pub nack: Option<i32>,
    #[serde(default)]
    pub pli: Option<i32>,
    #[serde(default)]
    pub fir: Option<i32>,
    #[serde(default)]
    pub turn_relay: Option<bool>,
    #[serde(default)]
    pub candidate_pair: Option<String>,
    #[serde(default)]
    pub limited_by: Option<String>,
}

/// Limita um inteiro opcional a `[0, max]`. `None` continua `None`.
fn clamp_opt(v: Option<i32>, max: i32) -> Option<i32> {
    v.map(|n| n.clamp(0, max))
}

/// Limita uma percentagem, tratando `NaN`/`inf` como 0 — um `f32` de fora pode
/// ser qualquer coisa, e `NaN` gravado numa coluna `REAL` contamina toda a
/// média que a leia depois.
fn clamp_pct(v: Option<f32>) -> Option<f32> {
    v.map(|n| {
        if n.is_finite() {
            n.clamp(0.0, 100.0)
        } else {
            0.0
        }
    })
}

/// Aceita apenas os rótulos que o próprio browser produz. Uma string livre de
/// um cliente iria direita para o painel do administrador.
fn clamp_label(v: Option<String>, allowed: &[&str]) -> Option<String> {
    v.filter(|x| allowed.contains(&x.as_str()))
}

/// O par de candidatos é `<tipo>/<tipo>`, e os tipos são um conjunto fechado.
fn clamp_candidate_pair(v: Option<String>) -> Option<String> {
    const TYPES: [&str; 5] = ["host", "srflx", "prflx", "relay", "?"];
    v.filter(|p| {
        let mut it = p.split('/');
        match (it.next(), it.next(), it.next()) {
            (Some(a), Some(b), None) => TYPES.contains(&a) && TYPES.contains(&b),
            _ => false,
        }
    })
}

/// Recebe uma amostra de qualidade (QoS) do cliente durante a chamada (~1/30s).
/// Alimenta o cartão "Qualidade das chamadas" do admin (org_stats). Valores
/// clampados; autorização igual à do resto da sala (can_access_room).
pub async fn post_qos(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(code): Path<String>,
    Json(s): Json<QosSample>,
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

    let rtt = s.rtt_ms.map(|v| v.clamp(0, 10_000));
    let loss = if s.loss_pct.is_finite() {
        s.loss_pct.clamp(0.0, 100.0)
    } else {
        0.0
    };
    let up = s.up_kbps.clamp(0, 100_000);
    // Calculados ANTES do INSERT: os contadores do /metrics precisam dos mesmos
    // valores, e o `bind` consome as `String`.
    let score = clamp_opt(s.score, 100);
    let limited_by = clamp_label(s.limited_by, &["cpu", "bandwidth", "other", "none"]);
    let turn_relay = s.turn_relay;
    sqlx::query(
        "INSERT INTO call_quality_samples
           (room_id, user_id, rtt_ms, loss_pct, up_kbps,
            down_kbps, jitter_ms, score, freeze_ms, concealment_pct,
            frames_dropped, nack, pli, fir, turn_relay, candidate_pair, limited_by)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17)",
    )
    .bind(room.id)
    .bind(auth.user_id)
    .bind(rtt)
    .bind(loss)
    .bind(up)
    .bind(clamp_opt(s.down_kbps, 100_000))
    .bind(clamp_opt(s.jitter_ms, 60_000))
    .bind(score.map(|v| v as i16))
    // Tecto de 1 minuto: o intervalo de amostragem é de 30 s, por isso mais do
    // que isto só pode ser um cliente a inventar.
    .bind(clamp_opt(s.freeze_ms, 60_000))
    .bind(clamp_pct(s.concealment_pct))
    .bind(clamp_opt(s.frames_dropped, 1_000_000))
    .bind(clamp_opt(s.nack, 1_000_000))
    .bind(clamp_opt(s.pli, 1_000_000))
    .bind(clamp_opt(s.fir, 1_000_000))
    .bind(turn_relay)
    .bind(clamp_candidate_pair(s.candidate_pair))
    .bind(limited_by.clone())
    .execute(&state.db)
    .await?;

    // Contadores em processo para o `/metrics` — o painel de SRE não devia ter
    // de esperar por uma consulta ao Postgres para saber que a qualidade caiu.
    let m = &state.metrics;
    crate::metrics::Metrics::bump(&m.qos_samples_total);
    if let Some(sc) = score {
        crate::metrics::Metrics::bump(&m.qos_scored_total);
        m.qos_score_sum
            .fetch_add(sc as u64, std::sync::atomic::Ordering::Relaxed);
        if sc < 60 {
            crate::metrics::Metrics::bump(&m.qos_poor_total);
        }
    }
    if turn_relay == Some(true) {
        crate::metrics::Metrics::bump(&m.qos_turn_relay_total);
    }
    if limited_by.as_deref() == Some("cpu") {
        crate::metrics::Metrics::bump(&m.qos_cpu_limited_total);
    }

    Ok(Json(serde_json::json!({ "ok": true })))
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- Limitadores da amostra de QoS ----
    //
    // Isto é dado vindo de FORA. Um cliente alterado pode mandar o que quiser,
    // e estes valores vão para as médias do painel do administrador. Cada um
    // destes testes corresponde a uma forma concreta de envenenar esse painel.

    #[test]
    fn clamp_opt_prende_ao_intervalo_e_preserva_a_ausencia() {
        assert_eq!(clamp_opt(Some(50), 100), Some(50));
        assert_eq!(clamp_opt(Some(-7), 100), Some(0));
        assert_eq!(clamp_opt(Some(i32::MAX), 100), Some(100));
        // Ausente continua ausente: uma amostra de um cliente antigo não pode
        // passar a dizer "0" — zero é uma medição, ausente não é.
        assert_eq!(clamp_opt(None, 100), None);
    }

    #[test]
    fn clamp_pct_trata_nan_e_infinito() {
        assert_eq!(clamp_pct(Some(12.5)), Some(12.5));
        assert_eq!(clamp_pct(Some(-1.0)), Some(0.0));
        assert_eq!(clamp_pct(Some(1e9)), Some(100.0));
        // NaN gravado numa coluna REAL contamina TODA a média que a leia
        // depois — e a média fica NaN sem nada a apontar a origem.
        assert_eq!(clamp_pct(Some(f32::NAN)), Some(0.0));
        assert_eq!(clamp_pct(Some(f32::INFINITY)), Some(0.0));
        assert_eq!(clamp_pct(None), None);
    }

    #[test]
    fn clamp_label_so_aceita_o_que_o_browser_produz() {
        let ok = ["cpu", "bandwidth", "other", "none"];
        assert_eq!(clamp_label(Some("cpu".into()), &ok), Some("cpu".into()));
        // Uma string livre do cliente iria direita para o painel do admin.
        assert_eq!(
            clamp_label(Some("<script>alert(1)</script>".into()), &ok),
            None
        );
        assert_eq!(clamp_label(Some("".into()), &ok), None);
        assert_eq!(clamp_label(None, &ok), None);
    }

    #[test]
    fn clamp_candidate_pair_exige_a_forma_tipo_barra_tipo() {
        assert_eq!(
            clamp_candidate_pair(Some("relay/srflx".into())),
            Some("relay/srflx".into())
        );
        assert_eq!(
            clamp_candidate_pair(Some("host/host".into())),
            Some("host/host".into())
        );
        // Tipo inventado, forma errada, ou texto arbitrário: fora.
        assert_eq!(clamp_candidate_pair(Some("relay/quantum".into())), None);
        assert_eq!(clamp_candidate_pair(Some("relay".into())), None);
        assert_eq!(clamp_candidate_pair(Some("a/b/c".into())), None);
        assert_eq!(
            clamp_candidate_pair(Some("'; DROP TABLE rooms; --".into())),
            None
        );
    }

    #[test]
    fn a_pontuacao_cabe_num_smallint() {
        // A coluna é SMALLINT; sem o clamp, um cliente a mandar 40000 fazia o
        // INSERT falhar e perdia-se a amostra inteira, não só o campo.
        let v = clamp_opt(Some(40_000), 100).map(|v| v as i16);
        assert_eq!(v, Some(100));
        assert_eq!(clamp_opt(Some(-5), 100).map(|v| v as i16), Some(0));
    }

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
