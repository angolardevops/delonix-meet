//! Fila de transcrição das gravações — o adaptador Postgres do
//! `TranscriptionService` gRPC (ADR-0005 §3). As regras (prazo da reserva,
//! tentativas) estão em `delonix_meet_domain::content::transcription`.
//!
//! O servidor é o dono do estado: o ai-worker reserva, transcreve e entrega, e
//! o texto passa pelo DLP ANTES de entrar na base — o que o worker a escrever
//! directamente no Postgres não fazia.

use delonix_meet_core::DomainError;
use delonix_meet_domain::content::transcription as rules;
use uuid::Uuid;

use crate::{error::ApiError, AppState};

pub struct Claimed {
    pub recording_id: Uuid,
    pub lease_token: String,
    pub media_file: String,
    pub room_code: String,
    pub lease_expires_unix: i64,
    pub attempt: i32,
}

/// Reserva a gravação pronta mais antiga ainda por transcrever, ou uma cuja
/// reserva expirou. `SKIP LOCKED`: dois workers nunca levam a mesma.
pub async fn claim(
    state: &AppState,
    worker_id: &str,
    lease_seconds: i32,
) -> Result<Option<Claimed>, ApiError> {
    let lease = rules::lease_duration(lease_seconds);
    let token = delonix_meet_core::crypto::random_hex(24);
    let row: Option<(Uuid, String, chrono::DateTime<chrono::Utc>, i32)> = sqlx::query_as(
        "UPDATE recordings r
            SET transcription_lease_token = $1,
                transcription_lease_expires_at = now() + make_interval(secs => $2),
                transcription_attempts = r.transcription_attempts + 1
          WHERE r.id = (
                SELECT id FROM recordings
                 WHERE transcribed_at IS NULL
                   AND transcription_failed_at IS NULL
                   AND status = 'ready'
                   AND transcription_attempts < $3
                   AND (transcription_lease_expires_at IS NULL
                        OR transcription_lease_expires_at < now())
                 ORDER BY created_at
                 FOR UPDATE SKIP LOCKED
                 LIMIT 1)
         RETURNING r.id,
                   COALESCE((SELECT code FROM rooms WHERE id = r.room_id), ''),
                   r.transcription_lease_expires_at,
                   r.transcription_attempts",
    )
    .bind(&token)
    .bind(lease.as_secs() as f64)
    .bind(rules::MAX_ATTEMPTS)
    .fetch_optional(&state.db)
    .await?;
    Ok(row.map(|(recording_id, room_code, expires, attempt)| {
        tracing::info!(%recording_id, worker = worker_id, attempt, "transcrição reservada");
        Claimed {
            recording_id,
            lease_token: token,
            media_file: format!("{recording_id}.webm"),
            room_code,
            lease_expires_unix: expires.timestamp(),
            attempt,
        }
    }))
}

fn lease_lost() -> ApiError {
    DomainError::precondition(
        "transcription.lease_lost",
        "a reserva expirou ou pertence a outro worker",
    )
    .into()
}

/// Entrega a transcrição. Só quem tem a reserva em vigor entrega.
pub async fn complete(
    state: &AppState,
    recording_id: Uuid,
    lease_token: &str,
    transcript: &str,
    minutes: &str,
) -> Result<(), ApiError> {
    // DLP antes de qualquer byte chegar à base (e daí ao LLM da acta ou a um
    // webhook): cartões, NIF, chaves de API.
    let transcript = crate::dlp::censor(transcript);
    let minutes = crate::dlp::censor(minutes);
    let mut tx = state.db.begin().await?;
    let room_code: Option<(String,)> = sqlx::query_as(
        "UPDATE recordings r
            SET transcript = $3, minutes = $4, transcribed_at = now(),
                transcription_lease_token = NULL, transcription_lease_expires_at = NULL,
                transcription_error = NULL
          WHERE r.id = $1 AND r.transcription_lease_token = $2
            AND r.transcription_lease_expires_at >= now()
         RETURNING COALESCE((SELECT code FROM rooms WHERE id = r.room_id), '')",
    )
    .bind(recording_id)
    .bind(lease_token)
    .bind(&transcript)
    .bind(&minutes)
    .fetch_optional(&mut *tx)
    .await?;
    let Some((room_code,)) = room_code else {
        return Err(lease_lost());
    };
    // A acta da reunião ligada, se ainda não tem transcrição (o mesmo que o
    // worker fazia, agora do lado de cá).
    if !room_code.is_empty() {
        sqlx::query(
            "UPDATE meetings SET transcript = $1, minutes = $2
              WHERE room_code = $3 AND transcript = ''",
        )
        .bind(&transcript)
        .bind(&minutes)
        .bind(&room_code)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    tracing::info!(%recording_id, chars = transcript.len(), "transcrição entregue");
    crate::notifications::transcription_ready(state, recording_id).await;
    Ok(())
}

/// Desiste do trabalho: volta à fila, ou sai dela de vez.
pub async fn fail(
    state: &AppState,
    recording_id: Uuid,
    lease_token: &str,
    reason: &str,
    retryable: bool,
) -> Result<(), ApiError> {
    let attempts: Option<(i32,)> = sqlx::query_as(
        "SELECT transcription_attempts FROM recordings
          WHERE id = $1 AND transcription_lease_token = $2",
    )
    .bind(recording_id)
    .bind(lease_token)
    .fetch_optional(&state.db)
    .await?;
    let Some((attempts,)) = attempts else {
        return Err(lease_lost());
    };
    let retry = rules::should_retry(retryable, attempts);
    sqlx::query(
        "UPDATE recordings
            SET transcription_lease_token = NULL, transcription_lease_expires_at = NULL,
                transcription_error = $2,
                transcription_failed_at = CASE WHEN $3 THEN NULL ELSE now() END
          WHERE id = $1 AND transcription_lease_token = $4",
    )
    .bind(recording_id)
    .bind(rules::sanitize_reason(reason))
    .bind(retry)
    .bind(lease_token)
    .execute(&state.db)
    .await?;
    tracing::warn!(%recording_id, attempts, retry, "transcrição falhou");
    Ok(())
}
