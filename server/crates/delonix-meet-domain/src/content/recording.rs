//! Gravação (G4–G6): estado de processamento, categoria, regras de acesso,
//! capítulos, comentários e pesquisa. Puro — o adaptador Postgres
//! (`server/src/recordings.rs`) lê os factos e pergunta aqui o que eles dão.
//!
//! **Uma regra de acesso, um sítio.** O download, a biblioteca, os capítulos e
//! os comentários decidem todos por [`AccessFacts`]. Antes, a reprodução e o
//! download tinham cada um a sua função, e nenhuma das duas sabia que um membro
//! ARQUIVADO (auditoria S3) deixa de ser «quem pede» válido.

use delonix_meet_core::DomainError;

pub const MAX_TITLE: usize = 120;
pub const MAX_COMMENT: usize = 2000;
/// Capítulos por gravação. O tecto é o tamanho máximo de página: uma página
/// devolve sempre o índice inteiro.
pub const MAX_CHAPTERS: i64 = 100;
/// Marca temporal máxima quando a duração não é conhecida (48 h). Uma gravação
/// do servidor é limitada muito antes disso pelo `FFMPEG_TIMEOUT_SECS`.
pub const MAX_AT_SECS_UNKNOWN_DURATION: i32 = 48 * 3600;
/// Termos numa pesquisa. Mais do que isto é colar um parágrafo, não pesquisar.
pub const MAX_SEARCH_TERMS: usize = 8;
const MAX_TERM_CHARS: usize = 64;

// ---------------------------------------------------------------------------
//  Estado de processamento
// ---------------------------------------------------------------------------

/// Estado derivado que a UI mostra. Não é uma coluna: deriva de `status`
/// (0036) e das marcas da fila de transcrição (0016, 0040).
///
/// Não há `processing` (ffmpeg a compor): o `recorder` só insere a linha
/// DEPOIS de o ffmpeg acabar — ou a falha, com `status = failed`. Um estado
/// que nunca pode ser observado não entra no contrato.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessingState {
    Ready,
    Failed,
    Transcribing,
    Transcribed,
    TranscriptionFailed,
}

impl ProcessingState {
    pub const ALL: [&'static str; 5] = [
        "ready",
        "failed",
        "transcribing",
        "transcribed",
        "transcription_failed",
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ready => "ready",
            Self::Failed => "failed",
            Self::Transcribing => "transcribing",
            Self::Transcribed => "transcribed",
            Self::TranscriptionFailed => "transcription_failed",
        }
    }
}

/// Os factos de onde o estado deriva, tal como a base os tem.
#[derive(Debug, Clone, Copy, Default)]
pub struct ProcessingFacts<'a> {
    /// `recordings.status` (`ready` | `failed`).
    pub status: &'a str,
    /// `transcribed_at IS NOT NULL`.
    pub transcribed: bool,
    /// `transcription_failed_at IS NOT NULL`.
    pub transcription_failed: bool,
    /// Há reserva e o prazo ainda não passou. Uma reserva expirada é trabalho
    /// devolvido à fila, não «a transcrever».
    pub lease_active: bool,
}

/// Precedência: sem ficheiro nada mais importa; um resultado final
/// (transcrita, ou desistiu-se) ganha a uma reserva que ficou por limpar.
pub fn processing_state(f: ProcessingFacts<'_>) -> ProcessingState {
    // Um `status` desconhecido falha fechado: não se promete um ficheiro.
    if f.status != "ready" {
        return ProcessingState::Failed;
    }
    if f.transcribed {
        return ProcessingState::Transcribed;
    }
    if f.transcription_failed {
        return ProcessingState::TranscriptionFailed;
    }
    if f.lease_active {
        return ProcessingState::Transcribing;
    }
    ProcessingState::Ready
}

// ---------------------------------------------------------------------------
//  Categoria e título
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Category {
    Meeting,
    Lecture,
    Broadcast,
    Other,
}

impl Category {
    pub const ALL: [&'static str; 4] = ["meeting", "lecture", "broadcast", "other"];

    pub fn parse(s: &str) -> Result<Self, DomainError> {
        Ok(match s {
            "meeting" => Self::Meeting,
            "lecture" => Self::Lecture,
            "broadcast" => Self::Broadcast,
            "other" => Self::Other,
            other => {
                return Err(DomainError::invalid(
                    "recording.invalid_category",
                    format!(
                        "categoria inválida «{other}» — válidas: {}",
                        Self::ALL.join(", ")
                    ),
                )
                .with_field("category", Self::ALL.join(" | ")))
            }
        })
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Meeting => "meeting",
            Self::Lecture => "lecture",
            Self::Broadcast => "broadcast",
            Self::Other => "other",
        }
    }
}

fn bounded_text(
    raw: &str,
    max: usize,
    code: &'static str,
    field: &'static str,
    what: &str,
) -> Result<String, DomainError> {
    let t = raw.trim();
    // Controlo proibido, excepto quebras de linha (um comentário pode tê-las).
    let bad_control = t.chars().any(|c| c.is_control() && c != '\n' && c != '\r');
    if t.is_empty() || t.chars().count() > max || bad_control {
        return Err(DomainError::invalid(
            code,
            format!("{what} tem de ter 1-{max} caracteres, sem caracteres de controlo"),
        )
        .with_field(field, format!("1-{max} caracteres")));
    }
    Ok(t.to_string())
}

/// Título de apresentação. Vazio (depois de aparar) = sem título: a UI volta
/// a mostrar o nome do ficheiro. Devolve `None` nesse caso.
pub fn validate_title(raw: &str) -> Result<Option<String>, DomainError> {
    if raw.trim().is_empty() {
        return Ok(None);
    }
    let t = bounded_text(
        raw,
        MAX_TITLE,
        "recording.invalid_title",
        "title",
        "o título",
    )?;
    if t.contains('\n') || t.contains('\r') {
        return Err(
            DomainError::invalid("recording.invalid_title", "o título é uma só linha")
                .with_field("title", format!("1-{MAX_TITLE} caracteres, uma linha")),
        );
    }
    Ok(Some(t))
}

pub fn validate_chapter_title(raw: &str) -> Result<String, DomainError> {
    let t = bounded_text(
        raw,
        MAX_TITLE,
        "recording.invalid_chapter_title",
        "title",
        "o título do capítulo",
    )?;
    if t.contains('\n') || t.contains('\r') {
        return Err(DomainError::invalid(
            "recording.invalid_chapter_title",
            "o título do capítulo é uma só linha",
        )
        .with_field("title", format!("1-{MAX_TITLE} caracteres, uma linha")));
    }
    Ok(t)
}

pub fn validate_comment_body(raw: &str) -> Result<String, DomainError> {
    bounded_text(
        raw,
        MAX_COMMENT,
        "recording.invalid_comment",
        "body",
        "o comentário",
    )
}

/// Uma marca temporal cabe na gravação: `0..=duração` quando a duração é
/// conhecida, senão `0..=48 h`.
pub fn validate_at_secs(at_secs: i32, duration_secs: Option<i32>) -> Result<i32, DomainError> {
    let max = duration_secs
        .filter(|d| *d >= 0)
        .unwrap_or(MAX_AT_SECS_UNKNOWN_DURATION);
    if at_secs < 0 || at_secs > max {
        return Err(DomainError::invalid(
            "recording.invalid_timestamp",
            format!("a marca temporal tem de estar entre 0 e {max} segundos"),
        )
        .with_field("at_secs", format!("0..={max}")));
    }
    Ok(at_secs)
}

// ---------------------------------------------------------------------------
//  Acesso
// ---------------------------------------------------------------------------

/// O que a base sabe sobre QUEM PEDE e ESTA gravação.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AccessFacts {
    /// Quem pede fez o upload / iniciou a gravação no servidor.
    pub is_uploader: bool,
    /// Quem pede está em `room_participants` da sala.
    pub participant: bool,
    /// A gravação foi partilhada com quem pede (`recording_shares`).
    pub shared: bool,
    /// Quem pede é admin ACTIVO de uma organização do dono.
    pub org_admin: bool,
    /// Quem pede é membro ACTIVO de uma organização do dono.
    pub active_member: bool,
    /// Quem pede tem pertença ARQUIVADA numa organização do dono.
    pub archived_member: bool,
}

impl AccessFacts {
    /// Saiu da organização do dono e não ficou noutra dele (S3). O SUJEITO
    /// (o dono) não se filtra — a gravação de quem saiu continua da empresa —
    /// mas QUEM PEDE tem de ser membro activo.
    pub fn departed(&self) -> bool {
        self.archived_member && !self.active_member
    }

    /// Reproduzir, ver na biblioteca, ler capítulos e comentários.
    pub fn can_view(&self) -> bool {
        !self.departed() && (self.is_uploader || self.participant || self.shared)
    }

    /// Descarregar o ficheiro (`?dl=1`): o dono ou um admin activo da org do dono.
    pub fn can_download(&self) -> bool {
        !self.departed() && (self.is_uploader || self.org_admin)
    }

    /// Alterar metadados e capítulos: os mesmos de quem descarrega — a regra
    /// mais restritiva das que já existiam, não uma terceira.
    pub fn can_manage(&self) -> bool {
        self.can_download()
    }

    /// Chega à gravação de alguma forma (reproduz OU descarrega): lê os
    /// metadados, os capítulos, e lê e escreve comentários.
    pub fn can_see(&self) -> bool {
        self.can_view() || self.can_download()
    }
}

// ---------------------------------------------------------------------------
//  Pesquisa
// ---------------------------------------------------------------------------

/// Converte o texto do utilizador numa `tsquery` segura: cada termo só com
/// letras e dígitos, em prefixo (`termo:*`), todos obrigatórios (`&`).
///
/// Nenhum carácter de sintaxe da `tsquery` (`& | ! ( ) : * '`) sobrevive, por
/// isso o texto nunca é interpretado como operador. `None` = não sobrou termo.
pub fn search_query(raw: &str) -> Option<String> {
    let terms: Vec<String> = raw
        .split(|c: char| !c.is_alphanumeric())
        .filter(|t| !t.is_empty())
        .take(MAX_SEARCH_TERMS)
        .map(|t| {
            let t: String = t.chars().take(MAX_TERM_CHARS).collect();
            format!("{}:*", t.to_lowercase())
        })
        .collect();
    if terms.is_empty() {
        None
    } else {
        Some(terms.join(" & "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn facts(status: &str) -> ProcessingFacts<'_> {
        ProcessingFacts {
            status,
            ..Default::default()
        }
    }

    #[test]
    fn processing_state_each_state() {
        assert_eq!(processing_state(facts("ready")), ProcessingState::Ready);
        assert_eq!(processing_state(facts("failed")), ProcessingState::Failed);
        assert_eq!(
            processing_state(ProcessingFacts {
                lease_active: true,
                ..facts("ready")
            }),
            ProcessingState::Transcribing
        );
        assert_eq!(
            processing_state(ProcessingFacts {
                transcribed: true,
                ..facts("ready")
            }),
            ProcessingState::Transcribed
        );
        assert_eq!(
            processing_state(ProcessingFacts {
                transcription_failed: true,
                ..facts("ready")
            }),
            ProcessingState::TranscriptionFailed
        );
    }

    #[test]
    fn processing_state_precedence() {
        // Falhada ganha a tudo; um resultado final ganha a uma reserva por limpar.
        let all = ProcessingFacts {
            status: "failed",
            transcribed: true,
            transcription_failed: true,
            lease_active: true,
        };
        assert_eq!(processing_state(all), ProcessingState::Failed);
        assert_eq!(
            processing_state(ProcessingFacts {
                status: "ready",
                ..all
            }),
            ProcessingState::Transcribed
        );
        assert_eq!(
            processing_state(ProcessingFacts {
                status: "ready",
                transcribed: false,
                ..all
            }),
            ProcessingState::TranscriptionFailed
        );
        assert_eq!(processing_state(facts("zombie")), ProcessingState::Failed);
        for s in ProcessingState::ALL {
            assert!(!s.is_empty());
        }
    }

    #[test]
    fn category_roundtrip_and_refuse_unknown() {
        for c in Category::ALL {
            assert_eq!(Category::parse(c).unwrap().as_str(), c);
        }
        assert_eq!(
            Category::parse("podcast").unwrap_err().code,
            "recording.invalid_category"
        );
    }

    #[test]
    fn text_bounds() {
        assert_eq!(
            validate_title("  Aula 1 ").unwrap().as_deref(),
            Some("Aula 1")
        );
        assert_eq!(validate_title("   ").unwrap(), None);
        assert!(validate_title(&"x".repeat(121)).is_err());
        assert!(
            validate_title(&"é".repeat(120)).is_ok(),
            "conta caracteres, não bytes"
        );
        assert!(validate_title("a\nb").is_err());
        assert!(validate_chapter_title("").is_err());
        assert!(validate_chapter_title("Introdução").is_ok());
        assert!(validate_comment_body("linha 1\nlinha 2").is_ok());
        assert!(validate_comment_body(&"x".repeat(2001)).is_err());
        assert!(validate_comment_body("a\u{0007}b").is_err());
        assert!(validate_comment_body(" \n ").is_err());
    }

    #[test]
    fn at_secs_bounds() {
        assert_eq!(validate_at_secs(0, Some(60)).unwrap(), 0);
        assert_eq!(validate_at_secs(60, Some(60)).unwrap(), 60);
        assert!(validate_at_secs(61, Some(60)).is_err());
        assert!(validate_at_secs(-1, None).is_err());
        assert!(validate_at_secs(MAX_AT_SECS_UNKNOWN_DURATION, None).is_ok());
        assert!(validate_at_secs(MAX_AT_SECS_UNKNOWN_DURATION + 1, None).is_err());
    }

    #[test]
    fn access_rules() {
        let uploader = AccessFacts {
            is_uploader: true,
            active_member: true,
            ..Default::default()
        };
        assert!(uploader.can_view() && uploader.can_download() && uploader.can_manage());

        let participant = AccessFacts {
            participant: true,
            active_member: true,
            ..Default::default()
        };
        assert!(participant.can_view() && participant.can_see());
        assert!(!participant.can_download() && !participant.can_manage());

        // Admin que não participou: descarrega e gere, mas não «vê» na biblioteca
        // (comportamento herdado da reprodução inline) — e pode comentar.
        let admin = AccessFacts {
            org_admin: true,
            active_member: true,
            ..Default::default()
        };
        assert!(!admin.can_view() && admin.can_download() && admin.can_see());

        // Partilhada com alguém de fora (sem pertença nenhuma): vê.
        let outsider = AccessFacts {
            shared: true,
            ..Default::default()
        };
        assert!(outsider.can_view() && !outsider.can_download());

        // S3: arquivado perde tudo, incluindo o próprio dono.
        for f in [uploader, participant, admin] {
            let gone = AccessFacts {
                active_member: false,
                archived_member: true,
                org_admin: false,
                ..f
            };
            assert!(gone.departed());
            assert!(!gone.can_view() && !gone.can_download() && !gone.can_see());
        }
        // Arquivado numa org do dono mas activo noutra org dele: não saiu.
        let moved = AccessFacts {
            participant: true,
            active_member: true,
            archived_member: true,
            ..Default::default()
        };
        assert!(!moved.departed() && moved.can_view());

        assert!(!AccessFacts::default().can_see());
    }

    #[test]
    fn search_query_is_safe() {
        assert_eq!(search_query("Orçamento"), Some("orçamento:*".into()));
        assert_eq!(
            search_query("  orçamento & 2027 | !x "),
            Some("orçamento:* & 2027:* & x:*".into())
        );
        assert_eq!(search_query("'):* | !&"), None);
        assert_eq!(search_query(""), None);
        let many = "a b c d e f g h i j k";
        assert_eq!(
            search_query(many).unwrap().matches(":*").count(),
            MAX_SEARCH_TERMS
        );
        let long = "x".repeat(500);
        assert!(search_query(&long).unwrap().len() <= MAX_TERM_CHARS + 2);
    }
}
