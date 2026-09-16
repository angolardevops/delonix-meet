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
}
