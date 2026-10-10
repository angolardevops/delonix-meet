//! Correio: a espinha do envio, e só ela.
//!
//! **PORQUE EXISTE.** Até aqui o servidor não enviava correio nenhum — a única
//! ocorrência de «smtp» em todo o `server/` era um comentário a dizer que não
//! enviava. Isso travava três itens do plano de lacunas: os convites (E2), a
//! reposição de password pela própria pessoa (E3) e o calendário (E7).
//!
//! **O D7 está decidido** (2026-10-09, ADR-0025): **relay do operador
//! primeiro**, SMTP por organização depois. Por isso aqui não há fornecedor por
//! inquilino, e isso é uma decisão de segurança, não de pressa: o `net_guard`
//! guarda **URLs HTTP** e não cobre uma ligação SMTP, que é `host:porta` em TCP
//! puro. Um host de SMTP escolhido pelo inquilino seria uma ligação de saída
//! arbitrária — varrer a rede interna, chegar ao `169.254.169.254`, servir de
//! sonda. O trabalho caro do «SMTP do cliente» é esse guarda, e ele ainda não
//! existe.
//!
//! **Este módulo não sabe nada sobre convites nem sobre passwords.** Quem quer
//! mandar uma mensagem chama `enqueue`; os casos de uso vivem nos seus módulos,
//! como o `sms_notify` vive ao lado do `sms`.
//!
//! **A fila é a peça comum** (`delonix_meet_core::jobs` +
//! `delonix_meet_store::jobs::claim_in`), como manda o
//! `check-filas-reivindicacao.sh`: não se escreve outro `FOR UPDATE SKIP
//! LOCKED` à mão. O livro de entregas É a fila — não há broker, e um reinício
//! não perde o que está agendado.

use std::sync::Arc;
use std::time::Duration;

use lettre::transport::smtp::authentication::Credentials;
use lettre::{AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor};
use uuid::Uuid;

use delonix_meet_core::DomainError;

use crate::{error::ApiError, AppState};

/// Quantas mensagens um passo do worker reclama de uma vez. Um relay que volta
/// depois de uma queda recebe-as em lotes, não numa rajada só.
const BATCH: i64 = 50;

/// A política de repetição dos webhooks, que é a completa da casa: cinco
/// tentativas, 30 s a 1 h, ±20 % de espalhamento. Um relay que esteve em baixo
/// uma hora não recebe tudo o que falhou no mesmo segundo.
const RETRY: delonix_meet_core::jobs::Retry = delonix_meet_core::jobs::RETRY_WEBHOOK;

/// Falha que repetir não cura: endereço inválido, remetente mal escrito,
/// configuração errada.
const PERMANENTE: delonix_meet_core::jobs::Failure = delonix_meet_core::jobs::Failure::Permanent;
/// Falha que pode passar sozinha: o relay em baixo, DNS, tempo-limite.
const TRANSITORIA: delonix_meet_core::jobs::Failure = delonix_meet_core::jobs::Failure::Transient;

/// Para que serve uma mensagem. Fica em texto na base (uma mensagem nova não
/// exige migração), mas quem enfileira passa por aqui — assim a lista das
/// mensagens que existem está num só sítio e não espalhada por literais.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Purpose {
    /// Reposição de password pedida pela própria pessoa (E3, por fazer).
    PasswordReset,
    /// Convite para uma organização (E2, por fazer).
    Invitation,
    /// Confirmação de endereço de email (pré-requisito do E3).
    EmailVerification,
}

impl Purpose {
    pub fn as_str(self) -> &'static str {
        match self {
            Purpose::PasswordReset => "password_reset",
            Purpose::Invitation => "invitation",
            Purpose::EmailVerification => "email_verification",
        }
    }
}

/// Uma mensagem a enfileirar.
pub struct Outgoing<'a> {
    /// A organização em cujo nome se envia, quando há uma. `None` para correio
    /// da plataforma ou de uma conta sem org.
    pub org_id: Option<Uuid>,
    pub purpose: Purpose,
    pub to: &'a str,
    pub subject: &'a str,
    pub body: &'a str,
}

/// O correio está configurado? Quem enfileira deve perguntar ANTES de prometer
/// à pessoa que a mensagem vai sair.
pub fn enabled(state: &AppState) -> bool {
    state.config.smtp_host.is_some() && state.config.smtp_from.is_some()
}

/// Enfileira uma mensagem. **Não envia**: o worker envia.
///
/// Separar as duas é o que torna a rota que chama isto rápida e honesta — uma
/// rota que esperasse pelo SMTP ficaria presa num relay lento, e um relay em
/// baixo passaria a ser um erro 500 na cara de quem pede a reposição.
///
/// Com o correio desligado devolve `mail.disabled` em vez de enfileirar
/// mensagens que ninguém vai enviar.
pub async fn enqueue(state: &AppState, msg: Outgoing<'_>) -> Result<Uuid, ApiError> {
    if !enabled(state) {
        return Err(DomainError::precondition(
            "mail.disabled",
            "o correio não está configurado neste servidor",
        )
        .into());
    }
    let id: Uuid = sqlx::query_scalar(
        "INSERT INTO mail_messages (org_id, purpose, to_address, subject, body_text)
         VALUES ($1, $2, $3, $4, $5) RETURNING id",
    )
    .bind(msg.org_id)
    .bind(msg.purpose.as_str())
    .bind(msg.to)
    .bind(msg.subject)
    .bind(msg.body)
    .fetch_one(&state.db)
    .await?;
    Ok(id)
}

/// O transporte, construído a partir da configuração do operador.
///
/// STARTTLS por omissão. `SMTP_STARTTLS=0` cai para uma ligação em claro e só
/// serve um relay em `localhost`: sem TLS, a password da conta viaja à vista.
fn transport(state: &AppState) -> Result<AsyncSmtpTransport<Tokio1Executor>, String> {
    let host = state.config.smtp_host.as_deref().ok_or("sem SMTP_HOST")?;
    let mut b = if state.config.smtp_starttls {
        AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(host).map_err(|e| e.to_string())?
    } else {
        AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous(host)
    }
    .port(state.config.smtp_port)
    .timeout(Some(Duration::from_secs(20)));
    if let (Some(u), Some(p)) = (
        state.config.smtp_username.as_deref(),
        state.config.smtp_password.as_deref(),
    ) {
        b = b.credentials(Credentials::new(u.to_string(), p.to_string()));
    }
    Ok(b.build())
}

/// A fila, declarada uma vez.
///
/// `ready_when` apanha os dois casos no mesmo passo: as que nunca foram
/// enviadas (`pending`) e as que falharam e têm repetição vencida.
///
/// O `claim_set` põe `status = 'sending'`, que é a **marca de posse**. Tem de
/// ser um estado DIFERENTE do `pending`: com a marca a deixar a linha em
/// `pending`, ela continuava a bater no `ready_when` e era reclamada outra vez
/// a cada volta do worker — a mensagem sairia tantas vezes quantas as voltas.
/// Por isso existe também o varredor (`sweep`): se o processo morrer entre a
/// reivindicação e o resultado, a linha fica `sending` para sempre sem ele.
const FILA: delonix_meet_core::jobs::Queue = delonix_meet_core::jobs::Queue {
    name: "mail_send",
    table: "mail_messages",
    id_column: "id",
    ready_when: "status = 'pending' \
                 OR (status = 'failed' AND retry_at IS NOT NULL AND retry_at <= now())",
    claim_set: "status = 'sending', claimed_at = now(), retry_at = NULL",
    returning: "id, to_address, subject, body_text, attempt",
    order_by: "created_at",
    tenant_column: Some("org_id"),
    batch: BATCH,
};

/// Uma mensagem reivindicada para enviar.
#[derive(sqlx::FromRow)]
struct PorEnviar {
    id: Uuid,
    to_address: String,
    subject: String,
    body_text: String,
    attempt: i32,
}

/// Envia as mensagens por enviar. Devolve quantas saíram.
///
/// O envio vem **depois** do commit da reivindicação, para não segurar locks
/// durante uma conversa SMTP que pode levar 20 s. A entrega é «pelo menos uma
/// vez»: se o processo morrer entre o envio e a escrita do resultado, a
/// mensagem pode sair duas vezes — preferível a não sair.
pub async fn send_due(state: &Arc<AppState>) -> Result<usize, sqlx::Error> {
    if !enabled(state) {
        return Ok(0);
    }
    let mut tx = state.db.begin().await?;
    let due: Vec<PorEnviar> =
        delonix_meet_store::jobs::claim_in(&mut tx, &FILA, &RETRY, None).await?;
    tx.commit().await?;
    if due.is_empty() {
        return Ok(0);
    }

    let from = state
        .config
        .smtp_from
        .as_deref()
        .unwrap_or_default()
        .to_string();
    let tp = match transport(state) {
        Ok(tp) => tp,
        Err(e) => {
            // A configuração está errada, não o destino: marca todas como
            // falhadas sem gastar tentativas numa coisa que não melhora
            // sozinha.
            for m in &due {
                falhou(
                    state,
                    m.id,
                    m.attempt,
                    &format!("configuração: {e}"),
                    PERMANENTE,
                )
                .await;
            }
            return Ok(0);
        }
    };

    let mut enviadas = 0usize;
    for m in due {
        let built = Message::builder()
            .from(match from.parse() {
                Ok(f) => f,
                Err(e) => {
                    falhou(
                        state,
                        m.id,
                        m.attempt,
                        &format!("SMTP_FROM: {e}"),
                        PERMANENTE,
                    )
                    .await;
                    continue;
                }
            })
            .to(match m.to_address.parse() {
                Ok(t) => t,
                Err(e) => {
                    // Endereço inválido não melhora com repetição.
                    falhou(
                        state,
                        m.id,
                        m.attempt,
                        &format!("destinatário: {e}"),
                        PERMANENTE,
                    )
                    .await;
                    continue;
                }
            })
            .subject(&m.subject)
            .body(m.body_text.clone());
        let built = match built {
            Ok(b) => b,
            Err(e) => {
                falhou(
                    state,
                    m.id,
                    m.attempt,
                    &format!("mensagem: {e}"),
                    PERMANENTE,
                )
                .await;
                continue;
            }
        };
        match tp.send(built).await {
            Ok(_) => {
                let _ = sqlx::query(
                    "UPDATE mail_messages SET status = 'sent', sent_at = now(), error = NULL
                      WHERE id = $1",
                )
                .bind(m.id)
                .execute(&state.db)
                .await;
                enviadas += 1;
            }
            Err(e) => {
                // O relay pode estar em baixo: esta é a falha que a repetição
                // existe para cobrir.
                falhou(state, m.id, m.attempt, &e.to_string(), TRANSITORIA).await;
            }
        }
    }
    Ok(enviadas)
}

/// Marca uma falha.
///
/// `failure` decide se há outra tentativa, pelo vocabulário da casa
/// (`jobs::Failure`): um endereço inválido ou uma configuração errada são
/// `Permanent` — dão o mesmo à terceira vez e gastariam as tentativas que
/// servem para o caso que importa, que é o relay em baixo.
///
/// O espalhamento (±20 %) aplica-se no SQL, como nos webhooks: é o que impede
/// um relay que volta de receber, no mesmo segundo, tudo o que falhou junto.
///
/// `AND status = 'sending'` na condição: um resultado que chega tarde não
/// reescreve uma linha que o varredor já deu por abandonada.
async fn falhou(
    state: &Arc<AppState>,
    id: Uuid,
    attempt: i32,
    erro: &str,
    failure: delonix_meet_core::jobs::Failure,
) {
    let atraso: Option<f64> = RETRY
        .should_retry(failure, attempt)
        .then(|| RETRY.delay_after(attempt))
        .flatten()
        .map(|d| d.as_secs() as f64);
    let _ = sqlx::query(
        "UPDATE mail_messages
            SET status = 'failed', error = $2, attempt = attempt + 1, claimed_at = NULL,
                retry_at = CASE WHEN $3::float8 IS NULL THEN NULL
                                ELSE now() + make_interval(
                                       secs => $3::float8 * (0.8 + random() * 0.4)) END
          WHERE id = $1 AND status = 'sending'",
    )
    .bind(id)
    .bind(erro)
    .bind(atraso)
    .execute(&state.db)
    .await;
}

/// Retenção: quanto tempo o livro de entregas guarda uma mensagem.
const RETENTION_DAYS: i32 = 30;
/// Uma reivindicada que fica `pending` tempo demais foi abandonada por um
/// processo que morreu a meio do envio.
const STALE_SECS: i64 = 300;

/// Varre as abandonadas e apaga as velhas. Devolve `(reagendadas, apagadas)`.
///
/// Uma abandonada com tentativas por gastar é reagendada para já: o processo
/// que a enviava morreu sem o relay ter dito nada, e é exactamente o caso que
/// a repetição existe para cobrir. Pode, raramente, duplicar uma mensagem que
/// saiu mas cujo resultado não se chegou a escrever.
pub async fn sweep(db: &sqlx::PgPool) -> Result<(u64, u64), sqlx::Error> {
    let reagendadas = sqlx::query(
        "UPDATE mail_messages
            SET status = 'failed', error = 'abandonada por um processo que terminou',
                attempt = attempt + 1, claimed_at = NULL,
                retry_at = CASE WHEN attempt < $2 THEN now() END
          WHERE status = 'sending' AND claimed_at < now() - make_interval(secs => $1)",
    )
    .bind(STALE_SECS as f64)
    .bind(RETRY.max_attempts)
    .execute(db)
    .await?
    .rows_affected();
    let apagadas = sqlx::query(
        "DELETE FROM mail_messages WHERE created_at < now() - make_interval(days => $1)",
    )
    .bind(RETENTION_DAYS)
    .execute(db)
    .await?
    .rows_affected();
    Ok((reagendadas, apagadas))
}
