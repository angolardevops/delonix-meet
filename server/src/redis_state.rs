use crate::signaling::{PollState, QaState, WbStrokeData};
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
