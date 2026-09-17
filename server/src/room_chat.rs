//! Chat da sala persistido FORA do caminho quente.
//!
//! O handler do WebSocket corre com o lock da sala à mão (R16) e não pode
//! esperar pela base de dados. Por isso a escrita vai para uma fila LIMITADA
//! (`try_send`, nunca `send().await`) consumida por uma tarefa própria, por
//! ordem — uma reacção nunca chega à base antes da mensagem a que se refere.
//! Fila cheia: descarta-se e conta-se (`chat_persist_dropped_total`). A sala
//! recebeu a mensagem na mesma; perde-se só o histórico.
//!
//! Retenção: a migração 0018 promete o chat «até ao fim do dia (UTC) após a
//! última mensagem». Até aqui nada escrevia na tabela; agora que escreve, a
//! varredura (`retention_sweep`) cumpre essa promessa.

use std::sync::Arc;

use sqlx::PgPool;
use tokio::sync::mpsc;
use uuid::Uuid;

use crate::metrics::Metrics;

/// Capacidade da fila de persistência (mensagens e reacções).
const CHAT_WRITE_QUEUE_CAP: usize = 4096;

#[derive(Debug)]
pub enum ChatWrite {
    Message {
        id: Uuid,
        room_id: Uuid,
        user_id: Uuid,
        username: String,
        text: String,
        parent_id: Option<Uuid>,
        /// Epoch ms.
        at: i64,
        /// Conversa directa: a conta que a recebe (e o nome, para o histórico).
        to_user_id: Option<Uuid>,
        to_username: Option<String>,
    },
    Reaction {
        message_id: Uuid,
        user_id: Uuid,
        emoji: String,
        /// `true` = acrescenta, `false` = retira.
        on: bool,
    },
}

#[derive(Clone)]
pub struct ChatStore {
    tx: mpsc::Sender<ChatWrite>,
    metrics: Arc<Metrics>,
}

impl ChatStore {
    /// Enfileira sem esperar. Devolve `false` se a fila estava cheia/fechada.
    pub fn write(&self, w: ChatWrite) -> bool {
        match self.tx.try_send(w) {
            Ok(()) => true,
            Err(_) => {
                Metrics::bump(&self.metrics.chat_persist_dropped_total);
                false
            }
        }
    }

    /// Para testes: uma loja cujo lado de leitura fica com quem a criou.
    #[cfg(test)]
    pub fn for_test(cap: usize) -> (Self, mpsc::Receiver<ChatWrite>, Arc<Metrics>) {
        let (tx, rx) = mpsc::channel(cap);
        let metrics = Arc::new(Metrics::default());
        (
            Self {
                tx,
                metrics: metrics.clone(),
            },
            rx,
            metrics,
        )
    }
}

/// Cria a fila e a tarefa que a escreve. A tarefa termina quando o último
/// `ChatStore` é largado (o hub, no fim do processo).
pub fn spawn_writer(db: PgPool, metrics: Arc<Metrics>) -> ChatStore {
    let (tx, mut rx) = mpsc::channel::<ChatWrite>(CHAT_WRITE_QUEUE_CAP);
    let m = metrics.clone();
    tokio::spawn(async move {
        while let Some(w) = rx.recv().await {
            if let Err(e) = apply(&db, &w).await {
                Metrics::bump(&m.chat_persist_failed_total);
                tracing::warn!(error = %e, "chat: escrita na base de dados falhou");
            }
        }
    });
    ChatStore { tx, metrics }
}

pub async fn apply(db: &PgPool, w: &ChatWrite) -> Result<(), sqlx::Error> {
    match w {
        ChatWrite::Message {
            id,
            room_id,
            user_id,
            username,
            text,
            parent_id,
            at,
            to_user_id,
            to_username,
        } => {
            // O `parent_id` só é aceite se for da MESMA sala: a validação em
            // memória já o garante, e a subconsulta garante-o também aqui, para
            // um fio nunca atravessar salas nem que a memória esteja errada.
            sqlx::query(
                "INSERT INTO room_chat_messages (id, room_id, user_id, username, message, parent_id, created_at, to_user_id, to_username)
                 VALUES ($1, $2, $3, $4, $5,
                         (SELECT p.id FROM room_chat_messages p WHERE p.id = $6 AND p.room_id = $2),
                         to_timestamp($7::double precision / 1000.0), $8, $9)
                 ON CONFLICT (id) DO NOTHING",
            )
            .bind(id)
            .bind(room_id)
            .bind(user_id)
            .bind(username)
            .bind(text)
            .bind(parent_id)
            .bind(*at)
            .bind(to_user_id)
            .bind(to_username)
            .execute(db)
            .await?;
        }
        ChatWrite::Reaction {
            message_id,
            user_id,
            emoji,
            on,
        } => {
            if *on {
                sqlx::query(
                    "INSERT INTO room_chat_reactions (message_id, user_id, emoji)
                     VALUES ($1, $2, $3) ON CONFLICT DO NOTHING",
                )
                .bind(message_id)
                .bind(user_id)
                .bind(emoji)
                .execute(db)
                .await?;
            } else {
                sqlx::query(
                    "DELETE FROM room_chat_reactions
                     WHERE message_id = $1 AND user_id = $2 AND emoji = $3",
                )
                .bind(message_id)
                .bind(user_id)
                .bind(emoji)
                .execute(db)
                .await?;
            }
        }
    }
    Ok(())
}

/// Apaga o chat das salas cuja última mensagem é de um dia (UTC) já acabado —
/// a retenção prometida na migração 0018. Devolve quantas mensagens saíram.
pub async fn retention_sweep(db: &PgPool) -> Result<u64, sqlx::Error> {
    let r = sqlx::query(
        "DELETE FROM room_chat_messages m
         USING (SELECT room_id FROM room_chat_messages
                GROUP BY room_id
                HAVING date_trunc('day', max(created_at) AT TIME ZONE 'UTC') + interval '1 day'
                       <= (now() AT TIME ZONE 'UTC')) velhas
         WHERE m.room_id = velhas.room_id",
    )
    .execute(db)
    .await?;
    Ok(r.rows_affected())
}
