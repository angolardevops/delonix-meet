use crate::signaling::{PollState, QaState, Role, WbStrokeData};
use redis::AsyncCommands;
use uuid::Uuid;

pub async fn wb_push(mut c: redis::aio::ConnectionManager, room_id: Uuid, stroke: &WbStrokeData) {
    let json = serde_json::to_string(stroke).unwrap();
    let _: redis::RedisResult<()> = c.rpush(format!("room:{room_id}:wb"), json).await;
}

pub async fn wb_get_all(mut c: redis::aio::ConnectionManager, room_id: Uuid) -> Vec<WbStrokeData> {
    let raw: Vec<String> = c
        .lrange(format!("room:{room_id}:wb"), 0, -1)
        .await
        .unwrap_or_default();
    raw.into_iter()
        .filter_map(|s| serde_json::from_str(&s).ok())
        .collect()
}

pub async fn wb_clear(mut c: redis::aio::ConnectionManager, room_id: Uuid) {
    let _: redis::RedisResult<()> = c.del(format!("room:{room_id}:wb")).await;
}

// Timer
pub async fn timer_set(mut c: redis::aio::ConnectionManager, room_id: Uuid, ends_at: i64) {
    let _: redis::RedisResult<()> = c.set(format!("room:{room_id}:timer"), ends_at).await;
}

pub async fn timer_get(mut c: redis::aio::ConnectionManager, room_id: Uuid) -> Option<i64> {
    c.get(format!("room:{room_id}:timer")).await.unwrap_or(None)
}

// Settings
pub async fn settings_set(
    mut c: redis::aio::ConnectionManager,
    room_id: Uuid,
    locked: bool,
    host_share: bool,
) {
    let _: redis::RedisResult<()> = c
        .hset_multiple(
            format!("room:{room_id}:settings"),
            &[("locked", locked), ("host_share", host_share)],
        )
        .await;
}

pub async fn settings_get(mut c: redis::aio::ConnectionManager, room_id: Uuid) -> (bool, bool) {
    let locked: bool = c
        .hget(format!("room:{room_id}:settings"), "locked")
        .await
        .unwrap_or(false);
    let host_share: bool = c
        .hget(format!("room:{room_id}:settings"), "host_share")
        .await
        .unwrap_or(false);
    (locked, host_share)
}

// Polls
pub async fn poll_set(mut c: redis::aio::ConnectionManager, room_id: Uuid, poll: &PollState) {
    let json = serde_json::to_string(poll).unwrap();
    let _: redis::RedisResult<()> = c
        .hset(format!("room:{room_id}:polls"), poll.id.to_string(), json)
        .await;
}

pub async fn poll_get_all(mut c: redis::aio::ConnectionManager, room_id: Uuid) -> Vec<PollState> {
    let raw: std::collections::HashMap<String, String> = c
        .hgetall(format!("room:{room_id}:polls"))
        .await
        .unwrap_or_default();
    raw.into_values()
        .filter_map(|s| serde_json::from_str(&s).ok())
        .collect()
}

pub async fn poll_vote(
    mut c: redis::aio::ConnectionManager,
    room_id: Uuid,
    poll_id: Uuid,
    voter_id: Uuid,
    option_idx: usize,
) -> Option<PollState> {
    let key = format!("room:{room_id}:polls");
    let raw: Option<String> = c.hget(&key, poll_id.to_string()).await.unwrap_or(None);
    if let Some(r) = raw {
        if let Ok(mut p) = serde_json::from_str::<PollState>(&r) {
            if p.open && option_idx < p.options.len() {
                p.votes.insert(voter_id, option_idx);
                let _: redis::RedisResult<()> = c
                    .hset(
                        &key,
                        poll_id.to_string(),
                        serde_json::to_string(&p).unwrap(),
                    )
                    .await;
                return Some(p);
            }
        }
    }
    None
}

pub async fn poll_close(
    mut c: redis::aio::ConnectionManager,
    room_id: Uuid,
    poll_id: Uuid,
) -> Option<PollState> {
    let key = format!("room:{room_id}:polls");
    let raw: Option<String> = c.hget(&key, poll_id.to_string()).await.unwrap_or(None);
    if let Some(r) = raw {
        if let Ok(mut p) = serde_json::from_str::<PollState>(&r) {
            p.open = false;
            let _: redis::RedisResult<()> = c
                .hset(
                    &key,
                    poll_id.to_string(),
                    serde_json::to_string(&p).unwrap(),
                )
                .await;
            return Some(p);
        }
    }
    None
}

// QA
pub async fn qa_set(mut c: redis::aio::ConnectionManager, room_id: Uuid, qa: &QaState) {
    let json = serde_json::to_string(qa).unwrap();
    let _: redis::RedisResult<()> = c
        .hset(format!("room:{room_id}:qa"), qa.id.to_string(), json)
        .await;
}

pub async fn qa_get_all(mut c: redis::aio::ConnectionManager, room_id: Uuid) -> Vec<QaState> {
    let raw: std::collections::HashMap<String, String> = c
        .hgetall(format!("room:{room_id}:qa"))
        .await
        .unwrap_or_default();
    raw.into_values()
        .filter_map(|s| serde_json::from_str(&s).ok())
        .collect()
}

pub async fn qa_upvote(
    mut c: redis::aio::ConnectionManager,
    room_id: Uuid,
    qa_id: Uuid,
    voter_id: Uuid,
) -> Option<QaState> {
    let key = format!("room:{room_id}:qa");
    let raw: Option<String> = c.hget(&key, qa_id.to_string()).await.unwrap_or(None);
    if let Some(r) = raw {
        if let Ok(mut q) = serde_json::from_str::<QaState>(&r) {
            if q.upvotes.contains(&voter_id) {
                q.upvotes.remove(&voter_id);
            } else {
                q.upvotes.insert(voter_id);
            }
            let _: redis::RedisResult<()> = c
                .hset(&key, qa_id.to_string(), serde_json::to_string(&q).unwrap())
                .await;
            return Some(q);
        }
    }
    None
}

pub async fn qa_answered(
    mut c: redis::aio::ConnectionManager,
    room_id: Uuid,
    qa_id: Uuid,
) -> Option<QaState> {
    let key = format!("room:{room_id}:qa");
    let raw: Option<String> = c.hget(&key, qa_id.to_string()).await.unwrap_or(None);
    if let Some(r) = raw {
        if let Ok(mut q) = serde_json::from_str::<QaState>(&r) {
            q.answered = true;
            let _: redis::RedisResult<()> = c
                .hset(&key, qa_id.to_string(), serde_json::to_string(&q).unwrap())
                .await;
            return Some(q);
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Lugar reservado (R91) copiado para fora do pod.
//
// O lugar que `SignalingHub::reclaim` devolve vive na memória do pod. Se o pod
// morrer (OOM, queda do nó), o segredo que o cliente guardou deixa de bater em
// qualquer coisa e um convidado admitido volta à sala de espera — o sintoma que
// o R91 corrigiu para o F5. Esta cópia permite que OUTRO pod o reconheça.
//
// Só se guarda o que o lugar herda (identidade e papel), nunca media. A chave
// leva o SHA-256 do segredo, não o segredo: quem lê o Redis não o consegue
// usar. O registo é de uma só utilização (`GETDEL`).
//
// Limite conhecido: o uso único vale entre os pods que consultam o Redis. Um pod
// que continua VIVO e ainda guarda o lugar na memória (janela de graça de 45 s)
// honra o segredo mesmo depois de outro pod o ter gasto. Quem usa o segredo duas
// vezes nessa janela fica com o lugar em dois pods; o do pod antigo expira sozinho.
// ---------------------------------------------------------------------------

/// O que um lugar herda ao ser reclamado (igual a `signaling::ReclaimedSeat`).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SeatRecord {
    pub peer_id: Uuid,
    pub username: String,
    pub user_id: Uuid,
    pub is_host: bool,
    pub can_admit: bool,
    pub role: Role,
    pub is_guest: bool,
}

/// Quanto tempo um registo sobrevive sem ser apagado: o tecto de uma sessão. O
/// registo é gravado à entrada e não se sabe quando o pod morre, por isso o
/// prazo não pode ser a janela de graça (45 s) — acabaria antes de ser preciso.
/// Quem sai, ou cujo lugar expira, apaga-o (`seat_drop`); isto só limpa o que
/// ficou para trás de um pod que morreu.
pub const SEAT_TTL_SECS: u64 = 12 * 3600;

fn seat_key(room_id: Uuid, secret: &str) -> String {
    format!(
        "room:{room_id}:seat:{}",
        delonix_meet_core::crypto::sha256_hex(secret)
    )
}

pub async fn seat_put(
    mut c: redis::aio::ConnectionManager,
    room_id: Uuid,
    secret: &str,
    rec: &SeatRecord,
) {
    let Ok(json) = serde_json::to_string(rec) else {
        return;
    };
    let r: redis::RedisResult<()> = c
        .set_ex(seat_key(room_id, secret), json, SEAT_TTL_SECS)
        .await;
    if let Err(e) = r {
        tracing::warn!(%room_id, error = %e, "lugar não copiado para o Redis");
    }
}

/// Lê SEM consumir: quem decide se o lugar pode ser reclamado precisa de ver de
/// quem é antes de o gastar.
pub async fn seat_peek(
    mut c: redis::aio::ConnectionManager,
    room_id: Uuid,
    secret: &str,
) -> Option<SeatRecord> {
    let raw: Option<String> = c.get(seat_key(room_id, secret)).await.unwrap_or(None);
    serde_json::from_str(&raw?).ok()
}

/// Consome o registo (`GETDEL`): dois pods a reclamar o mesmo segredo ao mesmo
/// tempo não ficam ambos com o lugar.
pub async fn seat_take(
    mut c: redis::aio::ConnectionManager,
    room_id: Uuid,
    secret: &str,
) -> Option<SeatRecord> {
    let raw: Option<String> = redis::cmd("GETDEL")
        .arg(seat_key(room_id, secret))
        .query_async(&mut c)
        .await
        .unwrap_or(None);
    serde_json::from_str(&raw?).ok()
}

pub async fn seat_drop(mut c: redis::aio::ConnectionManager, room_id: Uuid, secret: &str) {
    let _: redis::RedisResult<()> = c.del(seat_key(room_id, secret)).await;
}
