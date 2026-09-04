//! Redis pub/sub para entrega cross-nó de mensagens de presença.
//!
//! Arquitetura:
//! - Canal Redis `dlx:presence` → cada mensagem é um JSON `{user_id, msg}`.
//! - Conjunto Redis `dlx:online`  → todos os user_ids online em QUALQUER nó.
//! - Cada nó tem um subscriber a correr em background; ao receber uma mensagem
//!   tenta entregá-la ao WebSocket local (se o utilizador estiver neste nó).
//! - Cada nó publica quando o local delivery falha (utilizador está noutro nó).
//!
//! Se `REDIS_URL` não estiver definido, o `PubSubBus` não é criado e o sistema
//! opera em modo single-node sem qualquer alteração de comportamento.

use std::sync::Arc;

use futures_util::StreamExt;
use redis::AsyncCommands;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::presence::CallServerMsg;

const CHANNEL: &str = "dlx:presence";
const ONLINE_SET: &str = "dlx:online";
const SIGNALING_PREFIX: &str = "room:";

// ---------- Payload ----------

pub static NODE_ID: std::sync::LazyLock<Uuid> = std::sync::LazyLock::new(Uuid::new_v4);

#[derive(Serialize, Deserialize)]
pub struct PubPayload {
    pub user_id: Uuid,
    pub msg: CallServerMsg,
}

#[derive(Serialize, Deserialize)]
pub enum RedisRoomEvent {
    Broadcast {
        node_id: Uuid,
        from: Uuid,
        msg: crate::signaling::ServerMsg,
    },
    SendTo {
        node_id: Uuid,
        to: Uuid,
        msg: crate::signaling::ServerMsg,
    },
    BroadcastAll {
        node_id: Uuid,
        msg: crate::signaling::ServerMsg,
    },
    BroadcastHosts {
        node_id: Uuid,
        msg: crate::signaling::ServerMsg,
    },
}

// ---------- Bus ----------

pub struct PubSubBus {
    pub client: redis::Client,
    /// ConnectionManager faz auto-reconnect no caso de queda temporária do Redis.
    pub conn: redis::aio::ConnectionManager,
}

impl PubSubBus {
    pub async fn connect(url: &str) -> Result<Arc<Self>, redis::RedisError> {
        let client = redis::Client::open(url)?;
        let conn = redis::aio::ConnectionManager::new(client.clone()).await?;
        tracing::info!(%url, "Redis pub/sub conectado");
        Ok(Arc::new(Self { client, conn }))
    }

    /// Regista o utilizador como online em QUALQUER nó.
    pub async fn set_online(&self, user_id: Uuid) {
        let mut c = self.conn.clone();
        let _: redis::RedisResult<()> = c.sadd(ONLINE_SET, user_id.to_string()).await;
    }

    /// Remove o utilizador do conjunto global (chamado quando sai do último nó).
    pub async fn set_offline(&self, user_id: Uuid) {
        let mut c = self.conn.clone();
        let _: redis::RedisResult<()> = c.srem(ONLINE_SET, user_id.to_string()).await;
    }

    /// Verifica se o utilizador está online em QUALQUER nó.
    pub async fn is_online(&self, user_id: Uuid) -> bool {
        let mut c = self.conn.clone();
        c.sismember::<_, _, bool>(ONLINE_SET, user_id.to_string())
            .await
            .unwrap_or(false)
    }

    /// Publica uma mensagem no canal Redis para que outros nós a entreguem.
    pub async fn publish(&self, user_id: Uuid, msg: &CallServerMsg) {
        let payload = match serde_json::to_string(&PubPayload {
            user_id,
            msg: msg.clone(),
        }) {
            Ok(p) => p,
            Err(e) => {
                tracing::warn!("pubsub serialize error: {e}");
                return;
            }
        };
        let mut c = self.conn.clone();
        let _: redis::RedisResult<()> = c.publish(CHANNEL, payload).await;
    }

    /// Publica um evento de sinalização para uma sala específica.
    pub async fn publish_signaling(&self, room_id: Uuid, event: &RedisRoomEvent) {
        let payload = match serde_json::to_string(event) {
            Ok(p) => p,
            Err(e) => {
                tracing::warn!("signaling pubsub serialize error: {e}");
                return;
            }
        };
        let channel = format!("{SIGNALING_PREFIX}{room_id}:events");
        let mut c = self.conn.clone();
        let _: redis::RedisResult<()> = c.publish(channel, payload).await;
    }
}

/// Inicia o loop de subscriber em background.
/// Recebe mensagens do canal Redis e entrega-as ao `PresenceHub` local.
///
/// O `deliver_fn` é chamado com (user_id, msg) para cada mensagem recebida.
/// Em `main.rs` é um closure que chama `state.presence.send_to_user()`.
pub fn start_subscriber(
    bus: Arc<PubSubBus>,
    deliver_fn: impl Fn(Uuid, CallServerMsg) + Send + Sync + 'static,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        loop {
            match bus.client.get_async_pubsub().await {
                Err(e) => {
                    tracing::error!("Redis subscriber: falha ao ligar ({e}); retry em 5s");
                    tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
                }
                Ok(mut pubsub) => {
                    if let Err(e) = pubsub.subscribe(CHANNEL).await {
                        tracing::error!("Redis SUBSCRIBE falhou ({e}); retry em 5s");
                        tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
                        continue;
                    }
                    tracing::info!("Redis subscriber ativo no canal {CHANNEL}");
                    let mut stream = pubsub.on_message();
                    while let Some(msg) = stream.next().await {
                        let Ok(raw) = msg.get_payload::<String>() else {
                            continue;
                        };
                        let Ok(payload) = serde_json::from_str::<PubPayload>(&raw) else {
                            tracing::warn!("pubsub: payload inválido ignorado");
                            continue;
                        };
                        deliver_fn(payload.user_id, payload.msg);
                    }
                    // Stream fechou — Redis caiu; retry
                    tracing::warn!("Redis subscriber desconectado; a reconectar…");
                }
            }
        }
    })
}

/// Inicia o loop de subscriber para sinalização (PSUBSCRIBE room:*:events).
pub fn start_signaling_subscriber(
    bus: Arc<PubSubBus>,
    deliver_fn: impl Fn(Uuid, RedisRoomEvent) + Send + Sync + 'static,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        loop {
            match bus.client.get_async_pubsub().await {
                Err(e) => {
                    tracing::error!(
                        "Redis signaling subscriber: falha ao ligar ({e}); retry em 5s"
                    );
                    tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
                }
                Ok(mut pubsub) => {
                    let pattern = format!("{SIGNALING_PREFIX}*:events");
                    if let Err(e) = pubsub.psubscribe(&pattern).await {
                        tracing::error!("Redis PSUBSCRIBE falhou ({e}); retry em 5s");
                        tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
                        continue;
                    }
                    tracing::info!("Redis subscriber ativo no padrão {pattern}");
                    let mut stream = pubsub.on_message();
                    while let Some(msg) = stream.next().await {
                        let channel = msg.get_channel_name();
                        // Extrai o room_id do canal "room:<uuid>:events"
                        let parts: Vec<&str> = channel.split(':').collect();
                        if parts.len() != 3 {
                            continue;
                        }
                        let Ok(room_id) = Uuid::parse_str(parts[1]) else {
                            continue;
                        };

                        let Ok(raw) = msg.get_payload::<String>() else {
                            continue;
                        };
                        let Ok(payload) = serde_json::from_str::<RedisRoomEvent>(&raw) else {
                            tracing::warn!("pubsub: sinalização inválida ignorada");
                            continue;
                        };
                        deliver_fn(room_id, payload);
                    }
                    tracing::warn!("Redis signaling subscriber desconectado; a reconectar…");
                }
            }
        }
    })
}
