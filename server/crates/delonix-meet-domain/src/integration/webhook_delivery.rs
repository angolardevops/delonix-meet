//! Registo de entregas de webhooks e reenvio (G7).
//!
//! As regras PURAS de uma entrega vivem aqui, sem IO: que estados existem e
//! que transições são válidas, como um resultado HTTP vira estado, como uma
//! mensagem de erro é encurtada e limpa antes de ir para a base (e daí para a
//! consola do admin), e quantos reenvios por webhook se aceitam por minuto.
//! O adaptador (`server/src/webhooks.rs`) só as chama.

use delonix_meet_core::{DomainError, ErrorKind};

/// Tamanho máximo (em caracteres) do erro guardado numa entrega. É para uma
/// pessoa perceber o que falhou, não um registo de diagnóstico completo.
pub const MAX_ERROR_CHARS: usize = 300;

/// Janela e tecto do reenvio manual, por webhook. Um reenvio é uma acção de
/// pessoa («tentar outra vez»); dez por minuto chegam para isso e impedem que
/// a consola seja usada para martelar o destino de alguém.
pub const REDELIVERY_WINDOW_SECS: i64 = 60;
pub const MAX_REDELIVERIES_PER_WINDOW: i64 = 10;

/// Dias que uma entrega fica no registo. O registo serve para diagnosticar a
/// integração de agora; o payload inclui dados de reuniões, e guardá-lo para
/// sempre seria reter dados pessoais sem finalidade.
pub const RETENTION_DAYS: i64 = 30;

/// Uma entrega `pending` mais velha do que isto já não está em curso: o
/// cliente HTTP tem um tempo-limite de segundos, por isso o processo que a
/// enviava morreu (reinício, rollout) antes de escrever o resultado.
pub const STALE_PENDING_SECS: i64 = 300;

/// Erro gravado numa entrega abandonada pelo varredor.
pub const ABANDONED_ERROR: &str =
    "entrega interrompida (o servidor reiniciou antes de registar o resultado)";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeliveryStatus {
    Pending,
    Succeeded,
    Failed,
}

impl DeliveryStatus {
    pub const ALL: [&'static str; 3] = ["pending", "succeeded", "failed"];

    pub fn parse(s: &str) -> Result<Self, DomainError> {
        Ok(match s {
            "pending" => Self::Pending,
            "succeeded" => Self::Succeeded,
            "failed" => Self::Failed,
            other => {
                return Err(DomainError::invalid(
                    "webhook_delivery.invalid_status",
                    format!(
                        "estado de entrega inválido «{other}» — válidos: {}",
                        Self::ALL.join(", ")
                    ),
                )
                .with_field("status", Self::ALL.join(" | ")))
            }
        })
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
        }
    }

    /// Uma entrega só sai de `pending`, e sai uma vez. `succeeded` e `failed`
    /// são finais: uma nova tentativa é uma LINHA nova (`redelivery_of`), para
    /// o registo contar a história e não a reescrever.
    pub fn can_transition_to(self, next: DeliveryStatus) -> bool {
        matches!(
            (self, next),
            (Self::Pending, Self::Succeeded) | (Self::Pending, Self::Failed)
        )
    }

    pub fn is_final(self) -> bool {
        !matches!(self, Self::Pending)
    }
}

/// O estado final a partir do código HTTP devolvido pelo destino: só `2xx` é
/// sucesso. Um `3xx` é falha — o cliente não segue redirecções (anti-SSRF), por
/// isso o payload não chegou a lado nenhum.
pub fn status_for_http(code: u16) -> DeliveryStatus {
    if (200..300).contains(&code) {
        DeliveryStatus::Succeeded
    } else {
        DeliveryStatus::Failed
    }
}

/// Limpa uma mensagem de erro antes de a guardar:
///
/// - **URLs saem.** O erro do cliente HTTP inclui o URL do pedido, e o URL de
///   um webhook do Slack/Teams É a credencial (o token vai no caminho). O admin
///   já sabe o URL do seu webhook; o registo não precisa de o repetir.
/// - Caracteres de controlo saem e o espaço colapsa (o texto vai para uma UI).
/// - Corta-se em [`MAX_ERROR_CHARS`], com reticências.
pub fn sanitize_error(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len().min(MAX_ERROR_CHARS * 4));
    let mut rest = raw;
    while !rest.is_empty() {
        let next_url = ["http://", "https://"]
            .iter()
            .filter_map(|p| find_ascii_ci(rest, p))
            .min();
        match next_url {
            Some(i) => {
                out.push_str(&rest[..i]);
                out.push_str("[url]");
                let tail = &rest[i..];
                let end = tail
                    .find(|c: char| c.is_whitespace() || matches!(c, ')' | '"' | '\'' | '>'))
                    .unwrap_or(tail.len());
                rest = &tail[end..];
            }
            None => {
                out.push_str(rest);
                break;
            }
        }
    }
    let mut clean = String::with_capacity(out.len());
    let mut last_space = true;
    for c in out.chars() {
        if c.is_control() || c.is_whitespace() {
            if !last_space {
                clean.push(' ');
                last_space = true;
            }
        } else {
            clean.push(c);
            last_space = false;
        }
    }
    let clean = clean.trim_end();
    if clean.chars().count() > MAX_ERROR_CHARS {
        let mut cut: String = clean.chars().take(MAX_ERROR_CHARS - 1).collect();
        cut.push('…');
        cut
    } else {
        clean.to_string()
    }
}

fn find_ascii_ci(hay: &str, needle: &str) -> Option<usize> {
    hay.as_bytes()
        .windows(needle.len())
        .position(|w| w.eq_ignore_ascii_case(needle.as_bytes()))
}

/// O erro de uma resposta não-`2xx`: só o código, nunca o corpo (o corpo é do
/// destino, pode ser grande e pode trazer o que não queremos guardar).
pub fn http_status_error(code: u16) -> String {
    format!("o destino respondeu HTTP {code}")
}

/// Aceita ou recusa um reenvio, dado quantos reenvios deste webhook houve na
/// janela de [`REDELIVERY_WINDOW_SECS`].
pub fn check_redelivery_rate(recent_in_window: i64) -> Result<(), DomainError> {
    if recent_in_window >= MAX_REDELIVERIES_PER_WINDOW {
        return Err(DomainError::new(
            ErrorKind::ResourceExhausted,
            "webhook_delivery.redelivery_rate_limited",
            format!(
                "demasiados reenvios para este webhook — máximo {MAX_REDELIVERIES_PER_WINDOW} por minuto"
            ),
        ));
    }
    Ok(())
}

/// O número da tentativa de um reenvio: a seguinte à entrega reenviada.
pub fn next_attempt(previous: i32) -> i32 {
    previous.saturating_add(1)
}

/// Tentativas automáticas por evento, a primeira incluída. Depois da última a
/// entrega fica `failed` para sempre, e só um reenvio manual a volta a tentar.
pub const MAX_AUTO_ATTEMPTS: i32 = 5;

/// Espera, em segundos, depois de a tentativa `n` falhar (índice `n - 1`).
/// Quatro esperas dão as cinco tentativas: 30 s, 2 min, 10 min, 1 h. Cobre um
/// reinício do destino e um rollout de minutos, e para a seguir: um destino
/// morto não deve receber martelo durante dias.
pub const RETRY_DELAYS_SECS: [i64; 4] = [30, 120, 600, 3600];

/// A falha merece nova tentativa? Só as que podem passar sozinhas: `408`,
/// `425`, `429` e `5xx`. Um `4xx` é o destino a dizer que o pedido está errado
/// (URL, autenticação, recurso removido) e repeti-lo só gasta tentativas; um
/// `3xx` também — o cliente não segue redirecções, ver [`status_for_http`].
pub fn is_retryable_http(code: u16) -> bool {
    matches!(code, 408 | 425 | 429) || (500..600).contains(&code)
}

/// Quanto esperar antes de repetir, dado o número da tentativa que acabou de
/// falhar. `None` = não repetir (esgotadas, ou número inválido). O espalhamento
/// aleatório não é daqui: é do adaptador, para que um destino que volta não
/// receba, no mesmo segundo, todos os eventos que falharam juntos.
pub fn retry_delay_secs(failed_attempt: i32) -> Option<i64> {
    if !(1..MAX_AUTO_ATTEMPTS).contains(&failed_attempt) {
        return None;
    }
    RETRY_DELAYS_SECS
        .get(usize::try_from(failed_attempt - 1).ok()?)
        .copied()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn statuses_roundtrip_and_refuse_unknown() {
        for s in DeliveryStatus::ALL {
            assert_eq!(DeliveryStatus::parse(s).unwrap().as_str(), s);
        }
        let e = DeliveryStatus::parse("delivered").unwrap_err();
        assert_eq!(e.code, "webhook_delivery.invalid_status");
        assert_eq!(e.details[0].field, "status");
    }

    #[test]
    fn only_pending_moves_and_only_to_a_final_state() {
        use DeliveryStatus::*;
        assert!(Pending.can_transition_to(Succeeded));
        assert!(Pending.can_transition_to(Failed));
        assert!(!Pending.can_transition_to(Pending));
        for fin in [Succeeded, Failed] {
            assert!(fin.is_final());
            for next in [Pending, Succeeded, Failed] {
                assert!(!fin.can_transition_to(next), "{fin:?} -> {next:?}");
            }
        }
        assert!(!Pending.is_final());
    }

    #[test]
    fn only_2xx_is_success() {
        assert_eq!(status_for_http(200), DeliveryStatus::Succeeded);
        assert_eq!(status_for_http(204), DeliveryStatus::Succeeded);
        assert_eq!(status_for_http(301), DeliveryStatus::Failed);
        assert_eq!(status_for_http(404), DeliveryStatus::Failed);
        assert_eq!(status_for_http(500), DeliveryStatus::Failed);
        assert_eq!(http_status_error(502), "o destino respondeu HTTP 502");
    }

    #[test]
    fn error_loses_urls_controls_and_length() {
        let raw =
            "error sending request for url (https://hooks.slack.com/services/T0/B0/SEGREDO?x=1): \
                   connection refused\n\tretry";
        let s = sanitize_error(raw);
        assert!(!s.contains("SEGREDO"), "{s}");
        assert!(!s.contains("hooks.slack.com"), "{s}");
        assert_eq!(
            s,
            "error sending request for url ([url]): connection refused retry"
        );
        assert_eq!(sanitize_error("HTTP://A.b/c d"), "[url] d");
        let long = "x".repeat(1000);
        let s = sanitize_error(&long);
        assert_eq!(s.chars().count(), MAX_ERROR_CHARS);
        assert!(s.ends_with('…'));
        // Multibyte não parte a meio de um carácter.
        let s = sanitize_error(&"ã".repeat(400));
        assert_eq!(s.chars().count(), MAX_ERROR_CHARS);
        assert_eq!(sanitize_error(""), "");
    }

    #[test]
    fn redelivery_rate_policy() {
        assert!(check_redelivery_rate(0).is_ok());
        assert!(check_redelivery_rate(MAX_REDELIVERIES_PER_WINDOW - 1).is_ok());
        let e = check_redelivery_rate(MAX_REDELIVERIES_PER_WINDOW).unwrap_err();
        assert_eq!(e.kind, ErrorKind::ResourceExhausted);
        assert_eq!(e.code, "webhook_delivery.redelivery_rate_limited");
        assert_eq!(next_attempt(1), 2);
        assert_eq!(next_attempt(i32::MAX), i32::MAX);
    }

    #[test]
    fn only_transient_http_failures_are_retried() {
        for code in [408, 425, 429, 500, 502, 503, 504, 599] {
            assert!(is_retryable_http(code), "{code}");
        }
        for code in [200, 204, 301, 302, 400, 401, 403, 404, 410, 422, 600] {
            assert!(!is_retryable_http(code), "{code}");
        }
    }

    #[test]
    fn the_schedule_gives_exactly_the_attempts_it_promises() {
        // As esperas e o tecto têm de andar juntos: uma espera a mais ou a
        // menos desalinha-os em silêncio.
        assert_eq!(RETRY_DELAYS_SECS.len() as i32, MAX_AUTO_ATTEMPTS - 1);
        assert_eq!(retry_delay_secs(1), Some(30));
        assert_eq!(retry_delay_secs(2), Some(120));
        assert_eq!(retry_delay_secs(3), Some(600));
        assert_eq!(retry_delay_secs(4), Some(3600));
        assert_eq!(retry_delay_secs(MAX_AUTO_ATTEMPTS), None);
        assert_eq!(retry_delay_secs(MAX_AUTO_ATTEMPTS + 7), None);
        assert_eq!(retry_delay_secs(0), None);
        assert_eq!(retry_delay_secs(-3), None);
        assert_eq!(retry_delay_secs(i32::MAX), None);
        assert!(
            RETRY_DELAYS_SECS.windows(2).all(|w| w[0] < w[1]),
            "o recuo tem de crescer"
        );
    }
}
