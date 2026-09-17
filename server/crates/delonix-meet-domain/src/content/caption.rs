//! Legendas de uma gravação (R183): língua, estado e WebVTT. Puro.
//!
//! O VTT que entra por `PUT` é validado aqui (a mensagem diz a linha), e o que
//! sai de uma transcrição é construído aqui — e o próprio parser aceita-o.

use delonix_meet_core::DomainError;

use super::transcription::Segment;

/// Tecto de um ficheiro VTT (bytes). A base tem o mesmo tecto na coluna.
pub const MAX_VTT_BYTES: usize = 2 * 1024 * 1024;
pub const MAX_CUES: usize = 20_000;

#[derive(Debug, Clone, PartialEq)]
pub struct Cue {
    pub start_ms: i64,
    pub end_ms: i64,
    pub text: String,
}

/// BCP 47 curto: `pt`, `pt-AO`, `en`, `zh-Hans`. Nada de `_`, maiúsculas no
/// primeiro subtag, nem mais de dois subtags (o `lang` vai para o caminho).
pub fn valid_lang(lang: &str) -> bool {
    let mut parts = lang.split('-');
    let p = parts.next().unwrap_or("");
    let ok_primary = (2..=3).contains(&p.len()) && p.bytes().all(|b| b.is_ascii_lowercase());
    let ok_rest = match (parts.next(), parts.next()) {
        (None, _) => true,
        (Some(r), None) => {
            (2..=8).contains(&r.len()) && r.bytes().all(|b| b.is_ascii_alphanumeric())
        }
        _ => false,
    };
    ok_primary && ok_rest
}

pub fn check_lang(lang: &str) -> Result<(), DomainError> {
    if valid_lang(lang) {
        Ok(())
    } else {
        Err(DomainError::invalid(
            "recording.invalid_caption_lang",
            "a língua tem de ser uma etiqueta BCP 47 curta (pt, pt-AO, en)",
        )
        .with_field("lang", "BCP 47 curto"))
    }
}

/// O estado que um `PATCH` aceita: publicar ou voltar a rascunho.
pub fn parse_patch_status(s: &str) -> Result<&'static str, DomainError> {
    match s {
        "draft" => Ok("draft"),
        "published" => Ok("published"),
        other => Err(DomainError::invalid(
            "recording.invalid_caption_status",
            format!("estado inválido «{other}» — válidos: draft, published"),
        )
        .with_field("status", "draft | published")),
    }
}

/// Só se publica (ou despublica) uma legenda que tem conteúdo pronto.
pub fn publishable(current_status: &str) -> Result<(), DomainError> {
    if matches!(current_status, "draft" | "published") {
        Ok(())
    } else {
        Err(DomainError::conflict(
            "recording.caption_not_ready",
            format!("a legenda está em «{current_status}» e não tem conteúdo pronto"),
        ))
    }
}

/// `hh:mm:ss.ttt` ou `mm:ss.ttt` → milissegundos (a forma que o WebVTT exige).
fn parse_timestamp(s: &str) -> Option<i64> {
    let (hms, frac) = s.split_once('.')?;
    if frac.len() != 3 || !frac.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let ms: i64 = frac.parse().ok()?;
    let nums: Vec<i64> = hms
        .split(':')
        .map(|p| {
            (p.len() >= 2 && p.len() <= 9 && p.bytes().all(|b| b.is_ascii_digit()))
                .then(|| p.parse().ok())
                .flatten()
        })
        .collect::<Option<_>>()?;
    let (h, m, sec) = match nums.as_slice() {
        [m, s] => (0, *m, *s),
        [h, m, s] => (*h, *m, *s),
        _ => return None,
    };
    if m > 59 || sec > 59 {
        return None;
    }
    Some(((h * 60 + m) * 60 + sec) * 1000 + ms)
}

fn fmt_timestamp(ms: i64) -> String {
    let ms = ms.max(0);
    format!(
        "{:02}:{:02}:{:02}.{:03}",
        ms / 3_600_000,
        (ms / 60_000) % 60,
        (ms / 1000) % 60,
        ms % 1000
    )
}

fn vtt_error(msg: String) -> DomainError {
    DomainError::invalid("recording.invalid_vtt", format!("VTT inválido: {msg}"))
        .with_field("vtt", "WebVTT")
}

/// Valida um ficheiro WebVTT (tamanho incluído) e devolve as cues.
pub fn parse_vtt(src: &str) -> Result<Vec<Cue>, DomainError> {
    if src.len() > MAX_VTT_BYTES {
        return Err(DomainError::invalid(
            "recording.caption_too_large",
            format!("o VTT tem no máximo {} MiB", MAX_VTT_BYTES / (1024 * 1024)),
        )
        .with_field("vtt", format!("até {MAX_VTT_BYTES} bytes")));
    }
    let src = src.trim_start_matches('\u{feff}').replace("\r\n", "\n");
    let mut lines = src.split('\n').enumerate().peekable();
    match lines.next() {
        Some((_, first))
            if first == "WEBVTT"
                || first.starts_with("WEBVTT ")
                || first.starts_with("WEBVTT\t") => {}
        _ => return Err(vtt_error("a primeira linha tem de ser WEBVTT".into())),
    }
    // Cabeçalho: até à primeira linha em branco.
    for (_, l) in lines.by_ref() {
        if l.trim().is_empty() {
            break;
        }
    }
    let mut cues = Vec::new();
    while let Some((n, line)) = lines.next() {
        if line.trim().is_empty() {
            continue;
        }
        // Blocos que não são cues: saltam até à linha em branco.
        if line.starts_with("NOTE") || line == "STYLE" || line == "REGION" {
            for (_, l) in lines.by_ref() {
                if l.trim().is_empty() {
                    break;
                }
            }
            continue;
        }
        // Identificador opcional antes da linha de tempos.
        let (n, timing) = if line.contains("-->") {
            (n, line)
        } else {
            match lines.next() {
                Some((m, l)) if l.contains("-->") => (m, l),
                _ => {
                    return Err(vtt_error(format!(
                        "linha {}: falta a linha de tempos",
                        n + 1
                    )))
                }
            }
        };
        let (a, rest) = timing
            .split_once("-->")
            .ok_or_else(|| vtt_error(format!("linha {}: tempos inválidos", n + 1)))?;
        let b = rest.split_whitespace().next().unwrap_or("");
        let start = parse_timestamp(a.trim())
            .ok_or_else(|| vtt_error(format!("linha {}: início inválido", n + 1)))?;
        let end = parse_timestamp(b)
            .ok_or_else(|| vtt_error(format!("linha {}: fim inválido", n + 1)))?;
        if end < start {
            return Err(vtt_error(format!(
                "linha {}: o fim é anterior ao início",
                n + 1
            )));
        }
        let mut text = Vec::new();
        while let Some((_, l)) = lines.peek() {
            if l.trim().is_empty() {
                break;
            }
            if l.contains("-->") {
                return Err(vtt_error(format!(
                    "linha {}: cue sem linha em branco antes",
                    n + 2
                )));
            }
            text.push(lines.next().map(|x| x.1).unwrap_or_default());
        }
        cues.push(Cue {
            start_ms: start,
            end_ms: end,
            text: text.join("\n"),
        });
        if cues.len() > MAX_CUES {
            return Err(vtt_error(format!(
                "no máximo {MAX_CUES} legendas por ficheiro"
            )));
        }
    }
    Ok(cues)
}

/// Texto de uma cue sem o que o WebVTT interpretaria como marcação ou fim de bloco.
fn cue_text(s: &str) -> String {
    s.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace("-->", "→")
}

pub fn cues_to_vtt(cues: &[Cue]) -> String {
    let mut out = String::from("WEBVTT\n");
    for c in cues {
        out.push_str(&format!(
            "\n{} --> {}\n{}\n",
            fmt_timestamp(c.start_ms),
            fmt_timestamp(c.end_ms),
            cue_text(&c.text)
        ));
    }
    out
}

/// Os segmentos da transcrição como cues. É o ponto de entrada da geração de
/// legendas na língua da transcrição (`captions/generate`, fora deste lote).
pub fn segments_to_cues(segments: &[Segment]) -> Vec<Cue> {
    segments
        .iter()
        .map(|s| Cue {
            start_ms: s.start_ms,
            end_ms: s.end_ms.max(s.start_ms + 1),
            text: s.text.clone(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_vtt_with_ids_notes_and_settings() {
        let src = "\u{feff}WEBVTT - legendas pt-AO\r\n\r\nNOTE isto é um comentário\r\nque continua\r\n\r\n1\r\n00:00:01.000 --> 00:00:04.500 line:90%\r\nOlá a todos\r\nsegunda linha\r\n\r\n01:05.250 --> 01:07.000\r\nBoa tarde\r\n";
        let c = parse_vtt(src).unwrap();
        assert_eq!(c.len(), 2);
        assert_eq!(c[0].start_ms, 1000);
        assert_eq!(c[0].end_ms, 4500);
        assert_eq!(c[0].text, "Olá a todos\nsegunda linha");
        assert_eq!(c[1].start_ms, 65_250);
    }

    #[test]
    fn invalid_vtt_names_the_line() {
        let e = parse_vtt("SRT\n\n1\n00:00:01,000 --> 00:00:02,000\nx").unwrap_err();
        assert_eq!(e.code, "recording.invalid_vtt");
        let e = parse_vtt("WEBVTT\n\n00:00:05.000 --> 00:00:01.000\nx\n").unwrap_err();
        assert!(e.message.contains("linha 3"), "{}", e.message);
        assert!(parse_vtt("WEBVTT\n\n00:00:01,000 --> 00:00:02.000\nx\n").is_err());
        assert!(parse_vtt("WEBVTT\n\n00:61.000 --> 00:62.000\nx\n").is_err());
        let huge = format!("WEBVTT\n\n{}", "x".repeat(MAX_VTT_BYTES));
        assert_eq!(
            parse_vtt(&huge).unwrap_err().code,
            "recording.caption_too_large"
        );
    }

    #[test]
    fn transcript_becomes_vtt_the_parser_accepts() {
        let segs = vec![
            Segment {
                start_ms: 0,
                end_ms: 3_725_004,
                text: "a <b>marcação</b> --> não passa & escapa".into(),
                confidence: Some(0.9),
            },
            Segment {
                start_ms: 5000,
                end_ms: 5000,
                text: "instantâneo".into(),
                confidence: None,
            },
        ];
        let vtt = cues_to_vtt(&segments_to_cues(&segs));
        assert!(vtt.starts_with("WEBVTT\n"));
        assert!(vtt.contains("00:00:00.000 --> 01:02:05.004"));
        assert!(!vtt.contains("<b>"), "{vtt}");
        let back = parse_vtt(&vtt).unwrap();
        assert_eq!(back.len(), 2);
        assert!(back[1].end_ms > back[1].start_ms);
    }

    #[test]
    fn accepted_languages_and_statuses() {
        for ok in ["pt", "pt-AO", "en", "zh", "kmb", "zh-Hans"] {
            assert!(valid_lang(ok), "{ok}");
        }
        for bad in ["", "PT", "portugues", "pt_AO", "pt-", "pt-AO-x", "../x"] {
            assert!(!valid_lang(bad), "{bad}");
            assert!(check_lang(bad).is_err());
        }
        assert_eq!(parse_patch_status("draft").unwrap(), "draft");
        assert!(parse_patch_status("generating").is_err());
        assert!(publishable("draft").is_ok() && publishable("published").is_ok());
        assert_eq!(
            publishable("generating").unwrap_err().code,
            "recording.caption_not_ready"
        );
    }
}
