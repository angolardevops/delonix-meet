//! Gravação: estado derivado, tipo de sessão, textos editáveis, publicação,
//! regras de acesso e pesquisa. Puro — o adaptador Postgres
//! (`server/src/recordings.rs`) lê os factos e pergunta aqui o que eles dão.
//!
//! **O contrato de dados é o da UI** (R183): milissegundos, `kind`
//! (`meeting|training|broadcast|hybrid`), `status`/`state`/`transcript_status`,
//! descrição, etiquetas e `visibility` (`private|org`).
//!
//! **Uma regra de acesso, um sítio.** O download, a biblioteca, os capítulos,
//! os comentários, as legendas e a publicação decidem todos por
//! [`AccessFacts`]. Um membro ARQUIVADO (auditoria S3) deixa de ser «quem pede»
//! válido em todas de uma vez.

use delonix_meet_core::DomainError;

pub const MAX_FILENAME: usize = 200;
pub const MAX_DESCRIPTION: usize = 8000;
pub const MAX_TAGS: usize = 20;
pub const MAX_TAG_CHARS: usize = 40;
pub const MAX_CHAPTER_TITLE: usize = 200;
pub const MAX_COMMENT: usize = 2000;
/// Capítulos por gravação. A listagem devolve o índice inteiro (≤ este tecto).
pub const MAX_CHAPTERS: i64 = 100;
/// Marca temporal máxima quando a duração não é conhecida (48 h). Uma gravação
/// do servidor é limitada muito antes disso pelo `FFMPEG_TIMEOUT_SECS`.
pub const MAX_T_MS_UNKNOWN_DURATION: i64 = 48 * 3600 * 1000;
/// Termos numa pesquisa. Mais do que isto é colar um parágrafo, não pesquisar.
pub const MAX_SEARCH_TERMS: usize = 8;
const MAX_TERM_CHARS: usize = 64;

// ---------------------------------------------------------------------------
//  Estado derivado
// ---------------------------------------------------------------------------

/// Os factos de onde os estados derivam, tal como a base os tem.
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

impl ProcessingFacts<'_> {
    /// Há ficheiro. Um `status` desconhecido falha fechado.
    pub fn has_file(&self) -> bool {
        self.status == "ready"
    }
}

/// Estado do FICHEIRO que a UI mostra (`RecordingFileStatus`).
///
/// A UI conhece também `processing`, e esta linha nunca o emite: o `recorder`
/// só insere a linha DEPOIS de o ffmpeg acabar de compor. Um estado que nunca
/// pode ser observado não se inventa.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileStatus {
    Transcribing,
    Ready,
    Failed,
}

impl FileStatus {
    pub const ALL: [&'static str; 3] = ["transcribing", "ready", "failed"];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Transcribing => "transcribing",
            Self::Ready => "ready",
            Self::Failed => "failed",
        }
    }
}

/// Precedência: sem ficheiro nada mais importa; um resultado final da
/// transcrição (feita, ou desistiu-se) ganha a uma reserva que ficou por limpar.
pub fn file_status(f: ProcessingFacts<'_>) -> FileStatus {
    if !f.has_file() {
        return FileStatus::Failed;
    }
    if f.lease_active && !f.transcribed && !f.transcription_failed {
        return FileStatus::Transcribing;
    }
    FileStatus::Ready
}

/// `state` da UI: o `status`, com `published` quando está pronta e publicada.
pub fn display_state(file: FileStatus, published: bool) -> &'static str {
    match file {
        FileStatus::Ready if published => "published",
        other => other.as_str(),
    }
}

/// Estado da transcrição (`TranscriptStatus`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TranscriptStatus {
    None,
    Transcribing,
    Ready,
    Failed,
}

impl TranscriptStatus {
    pub const ALL: [&'static str; 4] = ["none", "transcribing", "ready", "failed"];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Transcribing => "transcribing",
            Self::Ready => "ready",
            Self::Failed => "failed",
        }
    }
}

pub fn transcript_status(f: ProcessingFacts<'_>) -> TranscriptStatus {
    if !f.has_file() {
        return TranscriptStatus::None;
    }
    if f.transcribed {
        return TranscriptStatus::Ready;
    }
    if f.transcription_failed {
        return TranscriptStatus::Failed;
    }
    if f.lease_active {
        return TranscriptStatus::Transcribing;
    }
    TranscriptStatus::None
}

// ---------------------------------------------------------------------------
//  Tipo de sessão e visibilidade
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Meeting,
    Training,
    Broadcast,
    Hybrid,
}

impl Kind {
    pub const ALL: [&'static str; 4] = ["meeting", "training", "broadcast", "hybrid"];

    pub fn parse(s: &str) -> Result<Self, DomainError> {
        Ok(match s {
            "meeting" => Self::Meeting,
            "training" => Self::Training,
            "broadcast" => Self::Broadcast,
            "hybrid" => Self::Hybrid,
            other => {
                return Err(DomainError::invalid(
                    "recording.invalid_kind",
                    format!(
                        "tipo de sessão inválido «{other}» — válidos: {}",
                        Self::ALL.join(", ")
                    ),
                )
                .with_field("kind", Self::ALL.join(" | ")))
            }
        })
    }

    /// Formato da sala (`rooms.format`) → tipo de sessão. `normal` e qualquer
    /// formato desconhecido são uma reunião.
    pub fn from_room_format(format: &str) -> Self {
        match format {
            "training" => Self::Training,
            "broadcast" => Self::Broadcast,
            "hybrid" => Self::Hybrid,
            _ => Self::Meeting,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Meeting => "meeting",
            Self::Training => "training",
            Self::Broadcast => "broadcast",
            Self::Hybrid => "hybrid",
        }
    }
}

/// A visibilidade que um `PUT …/publication` aceita. Só `org`: voltar a
/// privada é `DELETE …/publication`, e não há visibilidade pública por aqui
/// (o link público é outro recurso, com token e prazo).
pub fn parse_publication_visibility(s: &str) -> Result<&'static str, DomainError> {
    match s {
        "org" => Ok("org"),
        other => Err(DomainError::invalid(
            "recording.invalid_visibility",
            format!("visibilidade inválida «{other}» — só «org» se publica"),
        )
        .with_field("visibility", "org")),
    }
}

// ---------------------------------------------------------------------------
//  Textos editáveis
// ---------------------------------------------------------------------------

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

fn one_line(
    raw: &str,
    max: usize,
    code: &'static str,
    field: &'static str,
    what: &str,
) -> Result<String, DomainError> {
    let t = bounded_text(raw, max, code, field, what)?;
    if t.contains('\n') || t.contains('\r') {
        return Err(DomainError::invalid(code, format!("{what} é uma só linha"))
            .with_field(field, format!("1-{max} caracteres, uma linha")));
    }
    Ok(t)
}

/// Nome da gravação — o que a UI mostra e com que se descarrega.
pub fn validate_filename(raw: &str) -> Result<String, DomainError> {
    one_line(
        raw,
        MAX_FILENAME,
        "recording.invalid_filename",
        "filename",
        "o nome",
    )
}

/// Descrição: pode ficar vazia (apaga), até 8000 caracteres, com quebras de linha.
pub fn validate_description(raw: &str) -> Result<String, DomainError> {
    let t = raw.trim();
    let bad_control = t
        .chars()
        .any(|c| c.is_control() && c != '\n' && c != '\r' && c != '\t');
    if t.chars().count() > MAX_DESCRIPTION || bad_control {
        return Err(DomainError::invalid(
            "recording.invalid_description",
            format!(
                "a descrição tem no máximo {MAX_DESCRIPTION} caracteres, sem caracteres de controlo"
            ),
        )
        .with_field("description", format!("0-{MAX_DESCRIPTION} caracteres")));
    }
    Ok(t.to_string())
}

/// Etiquetas: sem `#`, minúsculas, aparadas, sem repetidas nem vazias. Recusa
/// em vez de cortar em silêncio.
pub fn normalize_tags(raw: &[String]) -> Result<Vec<String>, DomainError> {
    let invalid = |msg: String| {
        DomainError::invalid("recording.invalid_tags", msg).with_field(
            "tags",
            format!("até {MAX_TAGS} etiquetas de 1-{MAX_TAG_CHARS} caracteres, sem vírgulas"),
        )
    };
    let mut out: Vec<String> = Vec::new();
    for t in raw {
        let t = t.trim().trim_start_matches('#').trim().to_lowercase();
        if t.is_empty() {
            continue;
        }
        if t.chars().count() > MAX_TAG_CHARS {
            return Err(invalid(format!(
                "cada etiqueta tem no máximo {MAX_TAG_CHARS} caracteres"
            )));
        }
        if t.chars().any(|c| c.is_control() || c == ',') {
            return Err(invalid("etiqueta com caracteres inválidos".into()));
        }
        if !out.contains(&t) {
            out.push(t);
        }
    }
    if out.len() > MAX_TAGS {
        return Err(invalid(format!("no máximo {MAX_TAGS} etiquetas")));
    }
    Ok(out)
}

pub fn validate_chapter_title(raw: &str) -> Result<String, DomainError> {
    one_line(
        raw,
        MAX_CHAPTER_TITLE,
        "recording.invalid_chapter_title",
        "title",
        "o título do capítulo",
    )
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
/// conhecida, senão `0..=48 h`. Em milissegundos.
pub fn validate_t_ms(t_ms: i64, duration_ms: Option<i64>) -> Result<i64, DomainError> {
    let max = duration_ms
        .filter(|d| *d >= 0)
        .unwrap_or(MAX_T_MS_UNKNOWN_DURATION);
    if t_ms < 0 || t_ms > max {
        return Err(DomainError::invalid(
            "recording.invalid_timestamp",
            format!("a marca temporal tem de estar entre 0 e {max} ms"),
        )
        .with_field("t_ms", format!("0..={max}")));
    }
    Ok(t_ms)
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
    /// A gravação está publicada para a organização (`visibility = org`) E
    /// quem pede é membro ACTIVO de uma organização do dono.
    pub published_to_my_org: bool,
}

/// Qual biblioteca se lista (`GET /api/recordings?scope=`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LibraryScope {
    /// As de sempre: carregou, participou, ou foram-lhe partilhadas.
    Mine,
    /// As publicadas que quem pede vê — incluindo as da organização em que
    /// não participou.
    Published,
}

impl LibraryScope {
    pub fn parse(s: Option<&str>) -> Result<Self, DomainError> {
        match s {
            None | Some("") | Some("mine") => Ok(Self::Mine),
            Some("published") => Ok(Self::Published),
            Some(other) => Err(DomainError::invalid(
                "recording.invalid_scope",
                format!("scope inválido «{other}» — válidos: mine, published"),
            )
            .with_field("scope", "mine | published")),
        }
    }
}

impl AccessFacts {
    /// Saiu da organização do dono e não ficou noutra dele (S3). O SUJEITO
    /// (o dono) não se filtra — a gravação de quem saiu continua da empresa —
    /// mas QUEM PEDE tem de ser membro activo.
    pub fn departed(&self) -> bool {
        self.archived_member && !self.active_member
    }

    /// Reproduzir, ver na biblioteca, ler capítulos, comentários e legendas
    /// publicadas.
    pub fn can_view(&self) -> bool {
        !self.departed()
            && (self.is_uploader || self.participant || self.shared || self.published_to_my_org)
    }

    /// Descarregar o ficheiro (`?dl=1`): o dono ou um admin activo da org do dono.
    /// Publicar NÃO dá download: dá reprodução.
    pub fn can_download(&self) -> bool {
        !self.departed() && (self.is_uploader || self.org_admin)
    }

    /// Alterar metadados, capítulos, legendas e publicação: os mesmos de quem
    /// descarrega — a regra mais restritiva das que já existiam, não uma terceira.
    pub fn can_manage(&self) -> bool {
        self.can_download()
    }

    /// Chega à gravação de alguma forma (reproduz OU descarrega): lê os
    /// metadados, os capítulos, e lê e escreve comentários.
    pub fn can_see(&self) -> bool {
        self.can_view() || self.can_download()
    }

    /// Partilhar com pessoas e gerir o link público: só o DONO, e activo. Um
    /// admin da organização gere metadados mas não decide a quem a gravação de
    /// outra pessoa é mostrada; e quem saiu da empresa já não a partilha (S3).
    pub fn can_share(&self) -> bool {
        !self.departed() && self.is_uploader
    }

    /// Apagar um comentário: o autor (enquanto chega à gravação) ou quem a
    /// gere (moderação). Alterar o texto continua a ser só do autor.
    pub fn can_delete_comment(&self, is_author: bool) -> bool {
        (is_author && self.can_see()) || self.can_manage()
    }

    /// Ler uma legenda: publicada para quem vê; qualquer estado para quem gere.
    pub fn can_read_caption(&self, caption_status: &str) -> bool {
        self.can_manage() || (self.can_see() && caption_status == "published")
    }

    /// A gravação aparece na biblioteca `scope`. É o que o SQL
    /// `LIBRARY_VISIBLE_*` de `server/src/recordings.rs` escreve; o teste
    /// `library_scopes_agree_with_access_facts` prova que concordam.
    pub fn listed_in(&self, scope: LibraryScope, published: bool) -> bool {
        match scope {
            LibraryScope::Mine => {
                !self.departed() && (self.is_uploader || self.participant || self.shared)
            }
            LibraryScope::Published => published && self.can_view(),
        }
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

    #[test]
    fn so_o_dono_activo_partilha() {
        let dono = AccessFacts {
            is_uploader: true,
            active_member: true,
            ..Default::default()
        };
        assert!(dono.can_share());
        let admin = AccessFacts {
            org_admin: true,
            active_member: true,
            ..Default::default()
        };
        assert!(admin.can_manage() && !admin.can_share());
        let saiu = AccessFacts {
            is_uploader: true,
            archived_member: true,
            ..Default::default()
        };
        assert!(!saiu.can_share());
        let partilhado = AccessFacts {
            shared: true,
            active_member: true,
            ..Default::default()
        };
        assert!(partilhado.can_see() && !partilhado.can_share());
    }

    fn facts(status: &str) -> ProcessingFacts<'_> {
        ProcessingFacts {
            status,
            ..Default::default()
        }
    }

    #[test]
    fn file_and_transcript_status_each_state() {
        let cases: [(ProcessingFacts, &str, &str); 6] = [
            (facts("ready"), "ready", "none"),
            (facts("failed"), "failed", "none"),
            (
                ProcessingFacts {
                    lease_active: true,
                    ..facts("ready")
                },
                "transcribing",
                "transcribing",
            ),
            (
                ProcessingFacts {
                    transcribed: true,
                    ..facts("ready")
                },
                "ready",
                "ready",
            ),
            (
                ProcessingFacts {
                    transcription_failed: true,
                    ..facts("ready")
                },
                "ready",
                "failed",
            ),
            (facts("zombie"), "failed", "none"),
        ];
        for (f, file, transcript) in cases {
            assert_eq!(file_status(f).as_str(), file, "{f:?}");
            assert_eq!(transcript_status(f).as_str(), transcript, "{f:?}");
            assert!(FileStatus::ALL.contains(&file));
            assert!(TranscriptStatus::ALL.contains(&transcript));
        }
    }

    #[test]
    fn status_precedence() {
        // Falhada ganha a tudo; um resultado final ganha a uma reserva por limpar.
        let all = ProcessingFacts {
            status: "failed",
            transcribed: true,
            transcription_failed: true,
            lease_active: true,
        };
        assert_eq!(file_status(all), FileStatus::Failed);
        assert_eq!(transcript_status(all), TranscriptStatus::None);
        let ready = ProcessingFacts {
            status: "ready",
            ..all
        };
        assert_eq!(file_status(ready), FileStatus::Ready);
        assert_eq!(transcript_status(ready), TranscriptStatus::Ready);
        let no_result = ProcessingFacts {
            transcribed: false,
            ..ready
        };
        assert_eq!(transcript_status(no_result), TranscriptStatus::Failed);
    }

    #[test]
    fn published_state_only_when_ready() {
        assert_eq!(display_state(FileStatus::Ready, true), "published");
        assert_eq!(display_state(FileStatus::Ready, false), "ready");
        assert_eq!(
            display_state(FileStatus::Transcribing, true),
            "transcribing"
        );
        assert_eq!(display_state(FileStatus::Failed, true), "failed");
    }

    #[test]
    fn kind_roundtrip_room_format_and_refuse_unknown() {
        for k in Kind::ALL {
            assert_eq!(Kind::parse(k).unwrap().as_str(), k);
        }
        assert_eq!(
            Kind::parse("lecture").unwrap_err().code,
            "recording.invalid_kind"
        );
        assert_eq!(Kind::from_room_format("normal"), Kind::Meeting);
        assert_eq!(Kind::from_room_format("training"), Kind::Training);
        assert_eq!(Kind::from_room_format("broadcast"), Kind::Broadcast);
        assert_eq!(Kind::from_room_format("hybrid"), Kind::Hybrid);
        assert_eq!(Kind::from_room_format("???"), Kind::Meeting);
        assert_eq!(parse_publication_visibility("org").unwrap(), "org");
        for bad in ["private", "public", ""] {
            assert_eq!(
                parse_publication_visibility(bad).unwrap_err().code,
                "recording.invalid_visibility"
            );
        }
    }

    #[test]
    fn text_bounds() {
        assert_eq!(validate_filename("  Aula 1 ").unwrap(), "Aula 1");
        assert!(validate_filename("   ").is_err());
        assert!(validate_filename(&"x".repeat(201)).is_err());
        assert!(
            validate_filename(&"é".repeat(200)).is_ok(),
            "conta caracteres, não bytes"
        );
        assert!(validate_filename("a\nb").is_err());
        assert_eq!(validate_description("  ").unwrap(), "");
        assert!(validate_description("linha 1\nlinha 2\tcol").is_ok());
        assert!(validate_description(&"x".repeat(8001)).is_err());
        assert!(validate_description("a\u{0007}b").is_err());
        assert!(validate_chapter_title("").is_err());
        assert!(validate_chapter_title(&"x".repeat(200)).is_ok());
        assert!(validate_chapter_title(&"x".repeat(201)).is_err());
        assert!(validate_comment_body("linha 1\nlinha 2").is_ok());
        assert!(validate_comment_body(&"x".repeat(2001)).is_err());
        assert!(validate_comment_body("a\u{0007}b").is_err());
        assert!(validate_comment_body(" \n ").is_err());
    }

    #[test]
    fn tags_normalized_and_limits_refused() {
        let t =
            normalize_tags(&["#Aula".into(), " aula ".into(), "".into(), "Redes".into()]).unwrap();
        assert_eq!(t, vec!["aula".to_string(), "redes".to_string()]);
        assert!(normalize_tags(&["x".repeat(41)]).is_err());
        let many: Vec<String> = (0..21).map(|i| format!("t{i}")).collect();
        assert_eq!(
            normalize_tags(&many).unwrap_err().code,
            "recording.invalid_tags"
        );
        assert!(normalize_tags(&["a,b".into()]).is_err());
    }

    #[test]
    fn t_ms_bounds() {
        assert_eq!(validate_t_ms(0, Some(60_000)).unwrap(), 0);
        assert_eq!(validate_t_ms(60_000, Some(60_000)).unwrap(), 60_000);
        assert!(validate_t_ms(60_001, Some(60_000)).is_err());
        assert!(validate_t_ms(-1, None).is_err());
        assert!(validate_t_ms(MAX_T_MS_UNKNOWN_DURATION, None).is_ok());
        assert!(validate_t_ms(MAX_T_MS_UNKNOWN_DURATION + 1, None).is_err());
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

        // Publicada para a org: o colega activo vê e reproduz, não descarrega
        // nem gere.
        let colleague = AccessFacts {
            active_member: true,
            published_to_my_org: true,
            ..Default::default()
        };
        assert!(colleague.can_view() && colleague.can_see());
        assert!(!colleague.can_download() && !colleague.can_manage() && !colleague.can_share());

        // S3: arquivado perde tudo, incluindo o próprio dono e a publicação.
        for f in [uploader, participant, admin, colleague] {
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
    fn comment_delete_and_caption_read() {
        let participant = AccessFacts {
            participant: true,
            active_member: true,
            ..Default::default()
        };
        let owner = AccessFacts {
            is_uploader: true,
            active_member: true,
            ..Default::default()
        };
        assert!(participant.can_delete_comment(true));
        assert!(!participant.can_delete_comment(false));
        assert!(owner.can_delete_comment(false), "quem gere modera");
        assert!(!AccessFacts::default().can_delete_comment(true));

        for status in ["draft", "generating", "failed"] {
            assert!(!participant.can_read_caption(status), "{status}");
            assert!(owner.can_read_caption(status), "{status}");
        }
        assert!(participant.can_read_caption("published"));
        assert!(!AccessFacts::default().can_read_caption("published"));
    }

    /// A tabela de verdade da biblioteca: cada combinação de factos, nos dois
    /// âmbitos. É a mesma tabela que o SQL `LIBRARY_VISIBLE_*` escreve.
    #[test]
    fn library_scopes_truth_table() {
        assert_eq!(LibraryScope::parse(None).unwrap(), LibraryScope::Mine);
        assert_eq!(
            LibraryScope::parse(Some("mine")).unwrap(),
            LibraryScope::Mine
        );
        assert_eq!(
            LibraryScope::parse(Some("published")).unwrap(),
            LibraryScope::Published
        );
        assert_eq!(
            LibraryScope::parse(Some("all")).unwrap_err().code,
            "recording.invalid_scope"
        );
        for bits in 0u8..128 {
            let b = |i: u8| bits & (1 << i) != 0;
            let f = AccessFacts {
                is_uploader: b(0),
                participant: b(1),
                shared: b(2),
                org_admin: b(3),
                active_member: b(4),
                archived_member: b(5),
                published_to_my_org: b(6),
            };
            for published in [false, true] {
                let departed = f.archived_member && !f.active_member;
                let mine = !departed && (f.is_uploader || f.participant || f.shared);
                assert_eq!(f.listed_in(LibraryScope::Mine, published), mine, "{f:?}");
                let view = !departed
                    && (f.is_uploader || f.participant || f.shared || f.published_to_my_org);
                assert_eq!(
                    f.listed_in(LibraryScope::Published, published),
                    published && view,
                    "{f:?}"
                );
                // Listar nunca dá mais do que ver.
                if f.listed_in(LibraryScope::Mine, published)
                    || f.listed_in(LibraryScope::Published, published)
                {
                    assert!(f.can_view());
                }
            }
        }
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
