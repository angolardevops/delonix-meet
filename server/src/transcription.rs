//! Fila de transcrição das gravações — o adaptador Postgres do
//! `TranscriptionService` gRPC (ADR-0006 §3). As regras (prazo da reserva,
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
/// A fila da transcrição, declarada uma vez.
///
/// Era o sexto `FOR UPDATE SKIP LOCKED` à mão do servidor, e já era um dos três
/// completos (reserva, token, tecto de tentativas). Passa pela peça comum
/// (`crate::jobs`) para que deixe de ser a sétima variante da mesma coisa.
///
/// **Duas mudanças de forma, e nenhuma de comportamento:**
///
/// 1. o token da reserva passa a nascer no SQL (`md5(random()…)`) em vez de em
///    Rust. Serve o mesmo: é um segredo opaco que só quem reservou conhece, e
///    volta no `returning` para o *worker* o guardar. Gerá-lo em Rust obrigava
///    a ligar um valor dentro do `claim_set`, que é um fragmento da fila;
///    gerá-lo aqui não muda o que o *worker* vê;
/// 2. o prazo vem por `{lease_secs}`, substituído pelo `Lease` do `Worker` —
///    porque este prazo é **pedido pelo worker**, não uma constante da fila.
///    Passa pelo prendedor do domínio (`rules::lease_duration`, 1 min..2 h)
///    antes de chegar aqui.
///
/// `tenant_column: None`: a ordem é por `created_at` e o lote é de um, como
/// antes. A justiça entre organizações exigiria a org da SALA por junção —
/// fica anotado para quando a GPU for o recurso disputado.
const FILA: delonix_meet_core::jobs::Queue = delonix_meet_core::jobs::Queue {
    name: "transcription",
    table: "recordings",
    id_column: "id",
    ready_when: "transcribed_at IS NULL AND transcription_failed_at IS NULL \
                 AND status = 'ready' \
                 AND transcription_attempts < {max_attempts} \
                 AND (transcription_lease_expires_at IS NULL \
                      OR transcription_lease_expires_at < now())",
    claim_set: "transcription_lease_token = md5(random()::text || clock_timestamp()::text), \
                transcription_lease_expires_at = now() + make_interval(secs => {lease_secs}), \
                transcription_attempts = transcription_attempts + 1",
    // O `returning` é lido ANTES da marca da posse (a escolha e a marca são
    // duas instruções na mesma transacção), por isso o token e o prazo não
    // podem vir daqui — leem-se depois, pelo id.
    returning: "id",
    order_by: "created_at",
    tenant_column: None,
    batch: 1,
};

#[derive(sqlx::FromRow)]
struct Reservada {
    id: Uuid,
}

pub async fn claim(
    state: &AppState,
    worker_id: &str,
    lease_seconds: i32,
) -> Result<Option<Claimed>, ApiError> {
    let lease = rules::lease_duration(lease_seconds);
    let levadas: Vec<Reservada> = delonix_meet_store::jobs::claim(
        &state.db,
        &FILA,
        &delonix_meet_core::jobs::Retry {
            max_attempts: rules::MAX_ATTEMPTS,
            delays: &[],
            jitter: 0.0,
        },
        Some(lease),
    )
    .await?;
    let Some(Reservada { id }) = levadas.into_iter().next() else {
        return Ok(None);
    };
    // O que o worker precisa, lido depois da marca: o token e o prazo que a
    // reivindicação acabou de escrever.
    let (token, expires, attempt): (String, chrono::DateTime<chrono::Utc>, i32) = sqlx::query_as(
        "SELECT transcription_lease_token, transcription_lease_expires_at,
                transcription_attempts
           FROM recordings WHERE id = $1",
    )
    .bind(id)
    .fetch_one(&state.db)
    .await?;
    let room_code: String =
        sqlx::query_scalar("SELECT COALESCE(code, '') FROM rooms WHERE id = (SELECT room_id FROM recordings WHERE id = $1)")
            .bind(id)
            .fetch_optional(&state.db)
            .await?
            .unwrap_or_default();
    tracing::info!(recording_id = %id, worker = worker_id, attempt, "transcrição reservada");
    Ok(Some(Claimed {
        recording_id: id,
        lease_token: token,
        media_file: format!("{id}.webm"),
        room_code,
        lease_expires_unix: expires.timestamp(),
        attempt,
    }))
}

fn lease_lost() -> ApiError {
    DomainError::precondition(
        "transcription.lease_lost",
        "a reserva expirou ou pertence a outro worker",
    )
    .into()
}

/// O que o worker entrega.
pub struct Delivery<'a> {
    pub transcript: &'a str,
    pub minutes: &'a str,
    /// Vazio num worker que só entrega o texto (versão anterior do contrato).
    pub segments: Vec<rules::Segment>,
    /// Língua detectada; vazia = desconhecida.
    pub language: &'a str,
}

/// Entrega a transcrição. Só quem tem a reserva em vigor entrega.
pub async fn complete(
    state: &AppState,
    recording_id: Uuid,
    lease_token: &str,
    delivery: Delivery<'_>,
) -> Result<(), ApiError> {
    // DLP antes de qualquer byte chegar à base (e daí ao LLM da acta ou a um
    // webhook): cartões, NIF, chaves de API. Nos segmentos também: são o mesmo
    // texto, e é deles que saem as legendas.
    let transcript = crate::dlp::censor(delivery.transcript);
    let minutes = crate::dlp::censor(delivery.minutes);
    // Censura ANTES de truncar: sanitize_segments corta a MAX_SEGMENT_CHARS,
    // e um padrão (chave, cartão) que atravesse esse corte deixa de bater
    // certo com a expressão regular depois de partido ao meio.
    let censored: Vec<rules::Segment> = delivery
        .segments
        .into_iter()
        .map(|mut s| {
            s.text = crate::dlp::censor(&s.text);
            s
        })
        .collect();
    let segments: Vec<rules::Segment> = rules::sanitize_segments(censored);
    let confidence = rules::mean_confidence(&segments);
    let language = rules::sanitize_language(delivery.language);
    let segments_json = serde_json::to_value(&segments).map_err(ApiError::internal)?;
    let mut tx = state.db.begin().await?;
    let room_code: Option<(String,)> = sqlx::query_as(
        "UPDATE recordings r
            SET transcript = $3, minutes = $4, transcribed_at = now(),
                transcript_segments = $5, transcript_language = $6,
                transcript_confidence = $7,
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
    .bind(&segments_json)
    .bind(&language)
    .bind(confidence)
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
