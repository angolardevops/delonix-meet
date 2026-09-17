//! Regras da fila de transcrição (ADR-0006 §3). Puras: o adaptador Postgres
//! aplica-as numa só instrução com `FOR UPDATE SKIP LOCKED`.

use std::time::Duration;

/// Uma gravação que falha este número de vezes sai da fila: um ficheiro que
/// rebenta o modelo não pode ocupar a GPU para sempre.
pub const MAX_ATTEMPTS: i32 = 5;

const MIN_LEASE: u64 = 60;
const MAX_LEASE: u64 = 2 * 3600;
const DEFAULT_LEASE: u64 = 30 * 60;

/// Prazo efectivo da reserva: o pedido do worker, preso a 1 min..2 h; zero ou
/// negativo dá 30 min.
pub fn lease_duration(requested_secs: i32) -> Duration {
    let secs = if requested_secs <= 0 {
        DEFAULT_LEASE
    } else {
        (requested_secs as u64).clamp(MIN_LEASE, MAX_LEASE)
    };
    Duration::from_secs(secs)
}

/// Depois de uma falha: a gravação volta à fila?
pub fn should_retry(retryable: bool, attempts_so_far: i32) -> bool {
    retryable && attempts_so_far < MAX_ATTEMPTS
}

/// Razão de falha guardada: curta e numa linha (vai para um painel, não para
/// um log).
pub fn sanitize_reason(reason: &str) -> String {
    reason
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(300)
        .collect()
}

/// Segmentos por gravação. Uma hora de fala dá ~1 000; o tecto trava uma
/// entrega absurda sem cortar uma reunião longa.
pub const MAX_SEGMENTS: usize = 50_000;
const MAX_SEGMENT_CHARS: usize = 2_000;

/// Um segmento da transcrição, na forma guardada em `transcript_segments`
/// (migração 0052) e servida em `GET /api/recordings/{id}/transcript`.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Segment {
    pub start_ms: i64,
    pub end_ms: i64,
    pub text: String,
    #[serde(default)]
    pub confidence: Option<f32>,
}

/// Limpa os segmentos entregues (ou lidos da base): sai o que não faz sentido
/// (texto vazio, fim antes do início, tempo negativo), a confiança fora de
/// 0..=1 passa a desconhecida, e ficam por ordem de início. Não falha: um
/// segmento estragado não deita fora a transcrição inteira.
pub fn sanitize_segments(raw: Vec<Segment>) -> Vec<Segment> {
    let mut out: Vec<Segment> = raw
        .into_iter()
        .filter(|s| s.start_ms >= 0 && s.end_ms >= s.start_ms && !s.text.trim().is_empty())
        .map(|mut s| {
            s.text = s.text.trim().chars().take(MAX_SEGMENT_CHARS).collect();
            s.confidence = s
                .confidence
                .filter(|c| c.is_finite() && (0.0..=1.0).contains(c));
            s
        })
        .take(MAX_SEGMENTS)
        .collect();
    out.sort_by_key(|s| s.start_ms);
    out
}

/// Confiança média dos segmentos que a têm; `None` se nenhum a tem.
pub fn mean_confidence(segments: &[Segment]) -> Option<f32> {
    let known: Vec<f32> = segments.iter().filter_map(|s| s.confidence).collect();
    if known.is_empty() {
        return None;
    }
    let mean = known.iter().sum::<f32>() / known.len() as f32;
    Some(mean.clamp(0.0, 1.0))
}

/// Língua detectada: uma etiqueta curta (`pt`, `en`, `pt-AO`) ou nada.
pub fn sanitize_language(raw: &str) -> Option<String> {
    let t = raw.trim();
    let ok =
        !t.is_empty() && t.len() <= 16 && t.chars().all(|c| c.is_ascii_alphanumeric() || c == '-');
    ok.then(|| t.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seg(start_ms: i64, end_ms: i64, text: &str, confidence: Option<f32>) -> Segment {
        Segment {
            start_ms,
            end_ms,
            text: text.into(),
            confidence,
        }
    }

    #[test]
    fn segments_are_sanitized_not_rejected() {
        let s = sanitize_segments(vec![
            seg(5000, 6000, " segundo ", Some(0.8)),
            seg(0, 4000, "primeiro", Some(7.0)),
            seg(7000, 6500, "fim antes do início", None),
            seg(8000, 9000, "   ", None),
            seg(-1, 10, "negativo", None),
        ]);
        assert_eq!(s.len(), 2);
        assert_eq!(s[0].text, "primeiro");
        assert_eq!(
            s[0].confidence, None,
            "confiança fora de 0..=1 fica desconhecida"
        );
        assert_eq!(s[1].text, "segundo");
        assert_eq!(mean_confidence(&s), Some(0.8));
        assert_eq!(mean_confidence(&[]), None);
    }

    #[test]
    fn language_is_a_short_tag_or_nothing() {
        assert_eq!(sanitize_language(" pt "), Some("pt".into()));
        assert_eq!(sanitize_language("pt-AO"), Some("pt-AO".into()));
        assert_eq!(sanitize_language(""), None);
        assert_eq!(sanitize_language("pt; DROP"), None);
    }

    #[test]
    fn lease_is_bounded() {
        assert_eq!(lease_duration(0), Duration::from_secs(1800));
        assert_eq!(lease_duration(5), Duration::from_secs(60));
        assert_eq!(lease_duration(999_999), Duration::from_secs(7200));
        assert_eq!(lease_duration(600), Duration::from_secs(600));
    }

    #[test]
    fn retry_stops_after_max_or_when_not_retryable() {
        assert!(should_retry(true, 1));
        assert!(!should_retry(true, MAX_ATTEMPTS));
        assert!(!should_retry(false, 1));
    }

    #[test]
    fn reason_is_one_short_line() {
        let r = sanitize_reason("CUDA\n  out of memory\t".repeat(50).as_str());
        assert!(!r.contains('\n'));
        assert!(r.chars().count() <= 300);
    }
}
