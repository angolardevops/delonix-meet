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

/// Sala de conferência. O `code` (`abc-defg-hij`) é a credencial de acesso
/// por link, estilo Meet.
#[derive(Debug, Serialize, sqlx::FromRow, utoipa::ToSchema)]
pub struct Room {
    pub id: Uuid,
    pub code: String,
    pub name: String,
    pub owner_id: Uuid,
    /// `mesh` | `sfu`.
    pub topology: String,
    pub waiting_room: bool,
    pub e2ee: bool,
    /// `normal` (por defeito), `training` (só este permite salas de grupo),
    /// `broadcast` ou `hybrid`. Passa a `recordings.kind` das gravações da sala.
    pub format: String,
    pub created_at: DateTime<Utc>,
}

/// Lista de colunas que cobre **todos** os campos de `Room` — usar sempre que
/// se hidrata `Room` (`SELECT`, `INSERT ... RETURNING`). O `FromRow` derivado
/// faz `try_get` por campo: uma coluna em falta é um erro de RUNTIME, não de
/// compilação. Esta lista estava copiada à mão em NOVE sítios (sete neste
/// ficheiro, mais um em `apikeys.rs` e um em `recordings.rs`) — o mesmo padrão
/// que partiu `meetings::start`/`ics` na migração 0022 (ver ADR-0004, Fase 3).
pub const ROOM_COLUMNS: &str =
    "id, code, name, owner_id, topology, waiting_room, e2ee, format, created_at";

/// Documentação OpenAPI das rotas deste módulo (`openapi.rs` junta-as).
#[derive(utoipa::OpenApi)]
#[openapi(
    paths(
        create_room,
        get_room,
        join_room,
        ice_servers,
        room_chat,
        invite_to_room,
        post_timings,
        post_qos
    ),
    components(schemas(
        Room,
        CreateRoomReq,
        JoinRoomResp,
        ChatMessage,
        InviteReq,
        InviteResp,
        TimingsReq,
        QosSample
    ))
)]
pub struct ApiDoc;

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

#[derive(Deserialize, utoipa::ToSchema)]
pub struct CreateRoomReq {
    /// 1–100 caracteres (depois de `trim`).
    pub name: String,
    /// `mesh` | `sfu` (omissão `sfu`).
    #[serde(default)]
    pub topology: Option<String>,
    #[serde(default)]
    pub waiting_room: bool,
    /// Encriptação ponta-a-ponta do media (a chave nunca passa pelo servidor).
    #[serde(default)]
    pub e2ee: bool,
    /// 'normal' (por defeito), 'training' (ativa salas de grupo), 'broadcast' ou 'hybrid'.
    #[serde(default)]
    pub format: Option<String>,
}

/// Formatos de sala aceites.
pub const ROOM_FORMATS: &[&str] = &["normal", "training", "broadcast", "hybrid"];

/// Formato de reunião agendada (`meetings.format`) → formato da sala.
pub(crate) fn room_format_for_meeting(format: &str) -> &'static str {
    match format {
        "training" => "training",
        "broadcast" => "broadcast",
        "hybrid" => "hybrid",
        _ => "normal",
    }
}

/// O que o gravador do servidor tem de cumprir nesta sala (migração 0059).
pub(crate) async fn set_recording_options(
    db: &sqlx::PgPool,
    room_id: Uuid,
    auto_record: bool,
    record_quality: Option<&str>,
) -> Result<(), ApiError> {
    sqlx::query("UPDATE rooms SET auto_record = $2, record_quality = $3 WHERE id = $1")
        .bind(room_id)
        .bind(auto_record)
        .bind(record_quality)
        .execute(db)
        .await?;
    Ok(())
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
        let res: Result<Room, sqlx::Error> = sqlx::query_as(&format!(
            "INSERT INTO rooms (code, name, owner_id, topology, waiting_room, e2ee, format) VALUES ($1, $2, $3, $4, $5, $6, $7)
             RETURNING {ROOM_COLUMNS}"
        ))
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

// ---------- Sala pessoal («a minha sala», G2) ----------
//
// Uma sala como as outras (as regras de acesso são as de `room_access`), com
// `is_personal = true`. As regras de forma estão em
// `delonix_meet_domain::conferencing::personal_room`.

/// Quantas vezes se tenta um código novo quando o sorteado já existe.
const CODE_ATTEMPTS: usize = 5;

async fn find_personal_room(db: &sqlx::PgPool, owner_id: Uuid) -> Result<Option<Room>, ApiError> {
    Ok(sqlx::query_as(&format!(
        "SELECT {ROOM_COLUMNS} FROM rooms WHERE owner_id = $1 AND is_personal"
    ))
    .bind(owner_id)
    .fetch_optional(db)
    .await?)
}

/// A sala pessoal de `owner_id`, criada na primeira chamada.
///
/// Idempotente sob concorrência. O árbitro é o índice único parcial
/// `rooms_personal_owner_uidx` (migração 0047): dois pedidos simultâneos tentam
/// ambos inserir, um ganha, e o outro não insere nada (`ON CONFLICT DO
/// NOTHING`) e lê a linha do vencedor. Uma colisão de CÓDIGO é outro índice, e
/// repete com um código novo — como `insert_room`.
pub(crate) async fn ensure_personal_room(
    db: &sqlx::PgPool,
    owner_id: Uuid,
    default_name: &str,
) -> Result<Room, ApiError> {
    use delonix_meet_domain::conferencing::personal_room as rules;
    if let Some(room) = find_personal_room(db, owner_id).await? {
        return Ok(room);
    }
    for _ in 0..CODE_ATTEMPTS {
        let res: Result<Option<Room>, sqlx::Error> = sqlx::query_as(&format!(
            "INSERT INTO rooms (code, name, owner_id, topology, waiting_room, e2ee, format, is_personal)
             VALUES ($1, $2, $3, $4, $5, false, 'normal', true)
             ON CONFLICT (owner_id) WHERE is_personal DO NOTHING
             RETURNING {ROOM_COLUMNS}"
        ))
        .bind(generate_room_code())
        .bind(default_name)
        .bind(owner_id)
        .bind(rules::DEFAULT_TOPOLOGY)
        .bind(rules::DEFAULT_WAITING_ROOM)
        .fetch_optional(db)
        .await;
        match res {
            Ok(Some(room)) => return Ok(room),
            // Outro pedido criou-a entre a leitura e a escrita.
            Ok(None) => {
                return find_personal_room(db, owner_id)
                    .await?
                    .ok_or_else(|| ApiError::internal("sala pessoal desapareceu a meio"))
            }
            Err(sqlx::Error::Database(dbe)) if dbe.is_unique_violation() => continue,
            Err(e) => return Err(e.into()),
        }
    }
    Err(ApiError::internal("could not allocate room code"))
}

/// Altera nome e/ou sala de espera da sala pessoal (que tem de existir).
pub(crate) async fn update_personal_room(
    db: &sqlx::PgPool,
    owner_id: Uuid,
    name: Option<&str>,
    waiting_room: Option<bool>,
) -> Result<Room, ApiError> {
    sqlx::query_as(&format!(
        "UPDATE rooms SET name = COALESCE($2, name), waiting_room = COALESCE($3, waiting_room)
          WHERE owner_id = $1 AND is_personal RETURNING {ROOM_COLUMNS}"
    ))
    .bind(owner_id)
    .bind(name)
    .bind(waiting_room)
    .fetch_optional(db)
    .await?
    .ok_or(ApiError::NotFound)
}

/// Dá um código novo à sala pessoal (que tem de existir). O antigo deixa de
/// existir no mesmo `UPDATE`: `GET /api/rooms/{antigo}` passa a `404`.
pub(crate) async fn rotate_personal_room_code(
    db: &sqlx::PgPool,
    owner_id: Uuid,
) -> Result<Room, ApiError> {
    for _ in 0..CODE_ATTEMPTS {
        let res: Result<Option<Room>, sqlx::Error> = sqlx::query_as(&format!(
            "UPDATE rooms SET code = $2 WHERE owner_id = $1 AND is_personal RETURNING {ROOM_COLUMNS}"
        ))
        .bind(owner_id)
        .bind(generate_room_code())
        .fetch_optional(db)
        .await;
        match res {
            Ok(Some(room)) => return Ok(room),
            Ok(None) => return Err(ApiError::NotFound),
            Err(sqlx::Error::Database(dbe)) if dbe.is_unique_violation() => continue,
            Err(e) => return Err(e.into()),
        }
    }
    Err(ApiError::internal("could not allocate room code"))
}

/// Cria uma sala; o autenticado fica dono.
#[utoipa::path(
    post, path = "/api/rooms", tag = "rooms",
    security(("session" = [])),
    request_body = CreateRoomReq,
    responses(
        (status = 200, body = Room),
        (status = 400, description = "Nome fora de 1–100, `topology` ou `format` inválidos.", body = crate::openapi::ErrorBody),
        (status = 401, body = crate::openapi::ErrorBody),
    )
)]
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
    if !ROOM_FORMATS.contains(&format) {
        return Err(ApiError::BadRequest(
            "format must be 'normal', 'training', 'broadcast' or 'hybrid'".into(),
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

/// Metadados de uma sala. O código é a credencial: qualquer sessão válida que
/// o conheça lê os metadados (o controlo de entrada faz-se no `join`). O código
/// é normalizado para minúsculas.
#[utoipa::path(
    get, path = "/api/rooms/{room_code}", tag = "rooms",
    security(("session" = [])),
    params(("room_code" = String, Path, description = "Código da sala (`abc-defg-hij`).")),
    responses(
        (status = 200, body = Room),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 404, body = crate::openapi::ErrorBody),
    )
)]
pub async fn get_room(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(code): Path<String>,
) -> Result<Json<Room>, ApiError> {
    let room: Room = sqlx::query_as(&format!("SELECT {ROOM_COLUMNS} FROM rooms WHERE code = $1"))
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
    //
    // «Colega» é membro ACTIVO dos dois lados — a mesma regra de
    // `org::org_co_members`. Sem o filtro, um funcionário arquivado continuava
    // a ler chat, notas e gravações das salas da ex-empresa (auditoria
    // 2026-09-16, S3, provado ao vivo antes desta correcção).
    let (org_mate, invitee, admitter): (bool, bool, bool) = sqlx::query_as(
        r#"SELECT
             EXISTS(SELECT 1 FROM org_members a JOIN org_members b ON a.org_id = b.org_id
                    WHERE a.user_id = $1 AND b.user_id = $2
                      AND a.archived_at IS NULL AND b.archived_at IS NULL),
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

/// Resposta do `join`: a sala e o token de sala para o WebSocket.
#[derive(Serialize, utoipa::ToSchema)]
pub struct JoinRoomResp {
    pub room: Room,
    /// JWT de sala (`typ = room`), de curta duração; a única credencial que o
    /// `/ws` aceita. Leva `wait` (vai para a sala de espera) e `adm` (pode admitir).
    pub room_token: String,
    /// `/ws?token=<room_token>`.
    pub ws_path: String,
    /// Existe uma reunião agendada para esta sala (senão é chamada instantânea).
    pub scheduled: bool,
}

/// Exchange an access token for a short-lived, signed **room token** — the
/// only credential the signaling WebSocket accepts. Scoped to one room and
/// expiring in minutes, it prevents room hijacking with stolen/old URLs.
///
/// Troca a sessão por um token de sala. Nunca recusa quem tem o código: quem
/// não é dono, convidado na agenda nem co-anfitrião recebe um token com
/// `wait = true` (sala de espera). O código é normalizado para minúsculas.
#[utoipa::path(
    post, path = "/api/rooms/{room_code}/join", tag = "rooms",
    security(("session" = [])),
    params(("room_code" = String, Path, description = "Código da sala.")),
    responses(
        (status = 200, body = JoinRoomResp),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 404, body = crate::openapi::ErrorBody),
    )
)]
pub async fn join_room(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(code): Path<String>,
) -> Result<Json<JoinRoomResp>, ApiError> {
    let room: Room = sqlx::query_as(&format!("SELECT {ROOM_COLUMNS} FROM rooms WHERE code = $1"))
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

    // Origem e cargo decidem-se AQUI, do lado do servidor, e viajam assinados
    // no token: o cliente não tem como se declarar «sso» nem inventar um cargo.
    let origin = if !authorized {
        "guest"
    } else if crate::users::is_sso_account(&state.db, auth.user_id).await {
        "sso"
    } else {
        "password"
    };
    let title = if authorized {
        crate::org::title_alongside(&state, room.owner_id, auth.user_id).await
    } else {
        None
    };

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
            origin: Some(origin.into()),
            title,
            lobby: Some(!access.direct),
            wr: Some(room.waiting_room),
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

    Ok(Json(JoinRoomResp {
        room,
        ws_path: format!("/ws?token={room_token}"),
        room_token,
        scheduled,
    }))
}

/// Time-limited TURN credentials (coturn `use-auth-secret` / REST API spec):
/// username = expiry unix ts, password = base64(HMAC-SHA1(secret, username)).
///
/// Configuração ICE para o `RTCPeerConnection`: STUN + TURN com credenciais
/// válidas por 1 hora. Rate-limit por IP (partilha o limitador da v1).
#[utoipa::path(
    get, path = "/api/ice-servers", tag = "rooms",
    security(("session" = [])),
    responses(
        (status = 200, body = serde_json::Value,
         description = "`RTCConfiguration`: `{\"iceServers\": [{\"urls\": [\"stun:…\"]}, {\"urls\": [\"turn:…\"], \"username\": \"<expiry>\", \"credential\": \"<base64>\"}]}`, com `\"iceTransportPolicy\": \"relay\"` só quando o servidor força relay."),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 429, description = "Rate-limit por IP.", body = crate::openapi::ErrorBody),
    )
)]
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

#[derive(Serialize, sqlx::FromRow, utoipa::ToSchema)]
pub struct ChatMessage {
    pub id: Uuid,
    pub user_id: Uuid,
    pub username: String,
    pub message: String,
    pub created_at: DateTime<Utc>,
    /// Mensagem a que esta responde (fio), se houver.
    pub parent_id: Option<Uuid>,
    /// Contagem de reacções por emoji (`{}` sem reacções).
    pub reactions: serde_json::Value,
    /// Conversa directa: a conta que a recebe e o nome. `None` = pública.
    pub to_user_id: Option<Uuid>,
    pub to_username: Option<String>,
}

/// Últimas 200 mensagens de chat de uma sala, da mais antiga para a mais
/// recente (requer autenticação + acesso). As conversas directas só voltam a
/// quem as enviou e a quem as recebeu — o filtro é na consulta, não no cliente.
#[utoipa::path(
    get, path = "/api/rooms/{room_code}/messages", tag = "rooms",
    security(("session" = [])),
    params(("room_code" = String, Path, description = "Código da sala (sensível a maiúsculas).")),
    responses(
        (status = 200, body = Vec<ChatMessage>, description = "Ordem cronológica ascendente. As conversas directas só aparecem a quem as enviou e a quem as recebeu."),
        (status = 401, description = "Sessão inválida.", body = crate::openapi::ErrorBody),
        (status = 403, description = "Sem acesso à sala.", body = crate::openapi::ErrorBody),
        (status = 404, body = crate::openapi::ErrorBody),
    )
)]
pub async fn room_chat(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(code): Path<String>,
) -> Result<Json<Vec<ChatMessage>>, ApiError> {
    let room: Room = sqlx::query_as(&format!("SELECT {ROOM_COLUMNS} FROM rooms WHERE code = $1"))
        .bind(&code)
        .fetch_optional(&state.db)
        .await?
        .ok_or(ApiError::NotFound)?;

    if !can_access_room(&state, auth.user_id, &room).await? {
        return Err(ApiError::Forbidden);
    }

    // As ÚLTIMAS 200 (DESC + LIMIT) devolvidas por ordem cronológica. Antes era
    // `ASC LIMIT 200`, que numa conversa longa devolvia as primeiras 200 e
    // escondia exactamente as mais recentes.
    let msgs: Vec<ChatMessage> = sqlx::query_as(
        "SELECT * FROM (
             SELECT m.id, m.user_id, m.username, m.message, m.created_at, m.parent_id,
                    COALESCE((SELECT jsonb_object_agg(r.emoji, r.n)
                              FROM (SELECT emoji, count(*)::int AS n
                                    FROM room_chat_reactions
                                    WHERE message_id = m.id
                                    GROUP BY emoji) r), '{}'::jsonb) AS reactions,
                    m.to_user_id, m.to_username
             FROM room_chat_messages m
             WHERE m.room_id = $1
               AND (m.to_user_id IS NULL OR m.user_id = $2 OR m.to_user_id = $2)
             ORDER BY m.created_at DESC
             LIMIT 200
         ) ultimas ORDER BY created_at ASC",
    )
    .bind(room.id)
    .bind(auth.user_id)
    .fetch_all(&state.db)
    .await?;

    Ok(Json(msgs))
}

/// Quem está na sala de espera — para o anfitrião ou co-anfitrião decidir
/// ANTES de entrar na reunião.
///
/// Quem pode: o dono da sala, um co-anfitrião persistido (`room_admitters`), ou
/// quem está AGORA na sala com papel de admitir. Quem tem acesso à sala mas não
/// admite leva `403`; quem nem acesso tem leva `404` (não se confirma nada).
///
/// A fila vive na memória do pod da sala: chama-se com `?room={code}` para o
/// balanceador (hash por `$arg_room`) mandar o pedido a esse pod.
pub async fn room_waiting(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(code): Path<String>,
) -> Result<Json<Vec<crate::signaling::WaitingView>>, ApiError> {
    let room: Room = sqlx::query_as(
        "SELECT id, code, name, owner_id, topology, waiting_room, e2ee, format, created_at
         FROM rooms WHERE code = $1",
    )
    .bind(code.to_lowercase())
    .fetch_optional(&state.db)
    .await?
    .ok_or(ApiError::NotFound)?;
    let access = room_access(&state, auth.user_id, &room).await?;
    let em_sala = state.hub.user_admits(room.id, auth.user_id);
    if !access.admitter && !em_sala {
        return Err(if access.authorized {
            ApiError::Forbidden
        } else {
            ApiError::NotFound
        });
    }
    Ok(Json(state.hub.waiting_list(room.id)))
}

// ---------- Convidar membros para sala em curso ----------

#[derive(Deserialize, utoipa::ToSchema)]
pub struct InviteReq {
    /// 1–50 utilizadores. Só contam colegas activos de organização (o próprio
    /// e estranhos são filtrados em silêncio).
    pub targets: Vec<Uuid>,
    /// `video` | `voice` (omissão `video`).
    #[serde(default)]
    pub kind: Option<String>,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct InviteResp {
    /// Destinatários com dispositivo ligado, a tocar.
    pub ringing: Vec<Uuid>,
    /// Destinatários sem ligação: ficam com chamada perdida.
    pub offline: Vec<Uuid>,
}

/// Faz tocar os dispositivos de colegas de organização para a sala em curso.
/// Sem acesso à sala devolve **403**. O código NÃO é normalizado.
#[utoipa::path(
    post, path = "/api/rooms/{room_code}/invitations", tag = "rooms",
    security(("session" = [])),
    params(("room_code" = String, Path, description = "Código da sala (sensível a maiúsculas).")),
    request_body = InviteReq,
    responses(
        (status = 200, body = InviteResp),
        (status = 400, description = "`kind` inválido, `targets` fora de 1–50, ou nenhum destinatário válido depois do filtro.", body = crate::openapi::ErrorBody),
        (status = 401, description = "Sessão inválida.", body = crate::openapi::ErrorBody),
        (status = 403, description = "Sem acesso à sala.", body = crate::openapi::ErrorBody),
        (status = 404, body = crate::openapi::ErrorBody),
    )
)]
pub async fn invite_to_room(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(code): Path<String>,
    Json(req): Json<InviteReq>,
) -> Result<Json<InviteResp>, ApiError> {
    let room: Room = sqlx::query_as(&format!("SELECT {ROOM_COLUMNS} FROM rooms WHERE code = $1"))
        .bind(&code)
        .fetch_optional(&state.db)
        .await?
        .ok_or(ApiError::NotFound)?;

    if !can_access_room(&state, auth.user_id, &room).await? {
        return Err(ApiError::Forbidden);
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

    Ok(Json(InviteResp { ringing, offline }))
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
#[derive(Deserialize, utoipa::ToSchema)]
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

/// Tempos de estabelecimento de UMA sessão, reportados quando a media aparece.
///
/// Todos os campos são `Option` e vêm do cliente: um marco que não aconteceu é
/// `null`, e `null` significa «não sei», que é diferente de zero. Zero seria
/// uma medição («foi instantâneo») e enviesava as médias para baixo.
#[derive(Deserialize, utoipa::ToSchema)]
pub struct TimingsReq {
    #[serde(default)]
    pub join_ms: Option<i32>,
    #[serde(default)]
    pub ws_ms: Option<i32>,
    #[serde(default)]
    pub ice_gathering_ms: Option<i32>,
    #[serde(default)]
    pub first_audio_ms: Option<i32>,
    #[serde(default)]
    pub first_video_ms: Option<i32>,
    #[serde(default)]
    pub ice_restarts: Option<i32>,
    #[serde(default)]
    pub reconnects: Option<i32>,
}

/// `POST /api/rooms/{code}/join-timings` — uma vez por sessão.
///
/// Valores limitados a 10 minutos (`ice_restarts`/`reconnects` a 1000) antes
/// de gravar. Sem acesso à sala devolve **401**, não 403.
#[utoipa::path(
    post, path = "/api/rooms/{room_code}/join-timings", tag = "rooms",
    security(("session" = [])),
    params(("room_code" = String, Path, description = "Código da sala (sensível a maiúsculas).")),
    request_body = TimingsReq,
    responses(
        (status = 204, description = "Amostra aceite."),
        (status = 401, description = "Sessão inválida.", body = crate::openapi::ErrorBody),
        (status = 403, description = "Sem acesso à sala.", body = crate::openapi::ErrorBody),
        (status = 404, body = crate::openapi::ErrorBody),
    )
)]
pub async fn post_timings(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(code): Path<String>,
    Json(t): Json<TimingsReq>,
) -> Result<axum::http::StatusCode, ApiError> {
    let room: Room = sqlx::query_as(&format!("SELECT {ROOM_COLUMNS} FROM rooms WHERE code = $1"))
        .bind(&code)
        .fetch_optional(&state.db)
        .await?
        .ok_or(ApiError::NotFound)?;
    if !can_access_room(&state, auth.user_id, &room).await? {
        return Err(ApiError::Forbidden);
    }

    // Tecto de 10 minutos: acima disto não é um tempo de entrada, é um cliente
    // a inventar — e um valor absurdo destrói a média que isto existe para dar.
    const MAX_MS: i32 = 600_000;
    let join = clamp_opt(t.join_ms, MAX_MS);
    sqlx::query(
        "INSERT INTO call_timings
           (room_id, user_id, join_ms, ws_ms, ice_gathering_ms,
            first_audio_ms, first_video_ms, ice_restarts, reconnects)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)",
    )
    .bind(room.id)
    .bind(auth.user_id)
    .bind(join)
    .bind(clamp_opt(t.ws_ms, MAX_MS))
    .bind(clamp_opt(t.ice_gathering_ms, MAX_MS))
    .bind(clamp_opt(t.first_audio_ms, MAX_MS))
    .bind(clamp_opt(t.first_video_ms, MAX_MS))
    .bind(clamp_opt(t.ice_restarts, 1_000).unwrap_or(0))
    .bind(clamp_opt(t.reconnects, 1_000).unwrap_or(0))
    .execute(&state.db)
    .await?;

    // Soma e contagem em vez de média: é a forma idiomática em Prometheus e
    // permite `rate()` por janela, em vez de uma média desde o arranque que
    // deixa de reagir ao fim de um dia.
    let m = &state.metrics;
    if let Some(j) = join {
        crate::metrics::Metrics::bump(&m.join_total);
        m.join_ms_sum
            .fetch_add(j as u64, std::sync::atomic::Ordering::Relaxed);
        if j > 5_000 {
            crate::metrics::Metrics::bump(&m.join_slow_total);
        }
    }
    Ok(axum::http::StatusCode::NO_CONTENT)
}

/// Recebe uma amostra de qualidade (QoS) do cliente durante a chamada (~1/30s).
/// Alimenta o cartão "Qualidade das chamadas" do admin (org_stats). Valores
/// clampados; autorização igual à do resto da sala (can_access_room).
/// Sem acesso à sala devolve **401**, não 403.
#[utoipa::path(
    post, path = "/api/rooms/{room_code}/quality-samples", tag = "rooms",
    security(("session" = [])),
    params(("room_code" = String, Path, description = "Código da sala (sensível a maiúsculas).")),
    request_body = QosSample,
    responses(
        (status = 204, description = "Amostra aceite."),
        (status = 401, description = "Sessão inválida.", body = crate::openapi::ErrorBody),
        (status = 403, description = "Sem acesso à sala.", body = crate::openapi::ErrorBody),
        (status = 404, body = crate::openapi::ErrorBody),
    )
)]
pub async fn post_qos(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(code): Path<String>,
    Json(s): Json<QosSample>,
) -> Result<axum::http::StatusCode, ApiError> {
    let room: Room = sqlx::query_as(&format!("SELECT {ROOM_COLUMNS} FROM rooms WHERE code = $1"))
        .bind(&code)
        .fetch_optional(&state.db)
        .await?
        .ok_or(ApiError::NotFound)?;

    if !can_access_room(&state, auth.user_id, &room).await? {
        return Err(ApiError::Forbidden);
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

    Ok(axum::http::StatusCode::NO_CONTENT)
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
