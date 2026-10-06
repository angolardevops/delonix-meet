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
/// Nome de apresentação da gravação (é também o nome com que se descarrega).
pub const MAX_FILENAME: usize = 200;
/// Descrição da gravação. Pode ficar VAZIA — apagá-la é uma operação legítima,
/// ao contrário do título de um capítulo.
pub const MAX_DESCRIPTION: usize = 8000;
pub const MAX_TAGS: usize = 20;
pub const MAX_TAG_CHARS: usize = 40;
/// Título de um capítulo. Maior do que o da gravação ([`MAX_TITLE`]): um
/// capítulo descreve um trecho, não nomeia a sessão.
pub const MAX_CHAPTER_TITLE: usize = 200;
/// Capítulos por gravação. O tecto é o tamanho máximo de página: uma página
/// devolve sempre o índice inteiro.
pub const MAX_CHAPTERS: i64 = 100;
/// Marca temporal máxima quando a duração não é conhecida (48 h). Uma gravação
/// do servidor é limitada muito antes disso pelo `FFMPEG_TIMEOUT_SECS`.
pub const MAX_AT_SECS_UNKNOWN_DURATION: i32 = 48 * 3600;
/// O mesmo tecto de [`MAX_AT_SECS_UNKNOWN_DURATION`], em milissegundos — a
/// unidade do contrato (R183/R234).
pub const MAX_T_MS_UNKNOWN_DURATION: i64 = 48 * 3600 * 1000;
/// Termos numa pesquisa. Mais do que isto é colar um parágrafo, não pesquisar.
pub const MAX_SEARCH_TERMS: usize = 8;
const MAX_TERM_CHARS: usize = 64;

// ---------------------------------------------------------------------------
//  Estado de processamento
// ---------------------------------------------------------------------------

/// Estado derivado num só eixo. Não é uma coluna: deriva de `status` (0036) e
/// das marcas da fila de transcrição (0016, 0041).
///
/// **Já não é o que a API serve**: o contrato separou o ficheiro
/// ([`FileStatus`]) da transcrição ([`TranscriptStatus`]), e nenhum handler
/// chama [`processing_state`]. Não tem `processing` (ffmpeg a compor) — foi
/// escrito quando o `recorder` só inseria a linha DEPOIS de o ffmpeg acabar,
/// o que deixou de ser verdade (`recorder::insert_processing`). Quem o voltar
/// a ligar a uma resposta tem de lhe dar esse estado primeiro.
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
    /// `recordings.status` (`processing` | `transcribing` | `ready` | `failed`,
    /// migração 0057).
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
    /// Há ficheiro para ler. `transcribing` também tem: o ai-worker só começa
    /// depois de o ffmpeg acabar de compor.
    pub fn has_file(&self) -> bool {
        matches!(self.status, "ready" | "transcribing")
    }

    /// O ffmpeg ainda está a compor. Ainda não há ficheiro, e não é uma falha.
    pub fn composing(&self) -> bool {
        self.status == "processing"
    }
}

/// Estado do FICHEIRO que a UI mostra (`RecordingFileStatus`).
///
/// `processing` observa-se: o `recorder::insert_processing` cria a linha
/// quando a gravação pára, ANTES de o ffmpeg correr, com `progress_pct = 0`, e
/// só no fim a passa a `ready` (ou a `failed`, com a causa). Enquanto esta
/// regra não o conhecia, quem parava uma gravação via-a «falhada», sem causa,
/// até a composição acabar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileStatus {
    Processing,
    Transcribing,
    Ready,
    Failed,
}

impl FileStatus {
    pub const ALL: [&'static str; 4] = ["processing", "transcribing", "ready", "failed"];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Processing => "processing",
            Self::Transcribing => "transcribing",
            Self::Ready => "ready",
            Self::Failed => "failed",
        }
    }
}

/// Precedência: sem ficheiro nada mais importa; uma reserva que ficou por
/// limpar não faz a gravação voltar a «a transcrever» depois de um resultado.
///
/// Sem ficheiro há dois casos, e só um é falha: a compor (`processing`) ou
/// falhada. Um `status` desconhecido continua a falhar fechado.
pub fn file_status(f: ProcessingFacts<'_>) -> FileStatus {
    if f.composing() {
        return FileStatus::Processing;
    }
    if !f.has_file() {
        return FileStatus::Failed;
    }
    if f.lease_active && !f.transcribed && !f.transcription_failed {
        return FileStatus::Transcribing;
    }
    FileStatus::Ready
}

/// A recusa de quem pede o ficheiro de uma gravação que o servidor ainda está
/// a compor. Não é uma falha: quem a recebe volta a pedir quando acabar.
pub fn processing_conflict() -> DomainError {
    DomainError::conflict(
        "recording.processing",
        "A gravação ainda está a ser composta. Fica disponível quando o processamento acabar.",
    )
}

/// Uma acção que mostra a gravação a OUTRA pessoa — partilhar com um
/// utilizador, criar o link público, publicar, contar uma visualização — pede
/// uma gravação com ficheiro. Sem ele, quem recebe fica com uma entrada sem
/// nada para abrir, e o link público com um ficheiro que não existe.
///
/// Os dois casos sem ficheiro têm códigos diferentes porque pedem coisas
/// diferentes a quem chama: `recording.processing` é esperar; em
/// `recording.no_file` não há o que esperar.
pub fn require_file(f: ProcessingFacts<'_>) -> Result<(), DomainError> {
    match file_status(f) {
        FileStatus::Ready | FileStatus::Transcribing => Ok(()),
        FileStatus::Processing => Err(processing_conflict()),
        FileStatus::Failed => Err(DomainError::conflict(
            "recording.no_file",
            "A gravação falhou e não tem ficheiro.",
        )),
    }
}

/// O `state` da UI: o `status`, com `published` quando está pronta E publicada.
/// Publicar uma gravação falhada, ou uma que ainda está a compor, não a
/// promove — não há o que ver.
pub fn display_state(file: FileStatus, published: bool) -> &'static str {
    match file {
        FileStatus::Ready if published => "published",
        other => other.as_str(),
    }
}

/// Estado da transcrição (`TranscriptStatus`), independente do do ficheiro.
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

/// Tipo de sessão gravada (`kind`, migração 0057). Distinto da [`Category`]
/// herdada: espelha o formato da SALA, não uma etiqueta à escolha.
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

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Meeting => "meeting",
            Self::Training => "training",
            Self::Broadcast => "broadcast",
            Self::Hybrid => "hybrid",
        }
    }
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
        MAX_CHAPTER_TITLE,
        "recording.invalid_chapter_title",
        "title",
        "o título do capítulo",
    )?;
    if t.contains('\n') || t.contains('\r') {
        return Err(DomainError::invalid(
            "recording.invalid_chapter_title",
            "o título do capítulo é uma só linha",
        )
        .with_field(
            "title",
            format!("1-{MAX_CHAPTER_TITLE} caracteres, uma linha"),
        ));
    }
    Ok(t)
}

/// Nome de apresentação da gravação (`filename`). Uma só linha, nunca vazio —
/// é o que a UI mostra e o nome com que o ficheiro se descarrega.
pub fn validate_filename(raw: &str) -> Result<String, DomainError> {
    let t = bounded_text(
        raw,
        MAX_FILENAME,
        "recording.invalid_filename",
        "filename",
        "o nome",
    )?;
    // Uma só linha, e sem separadores de caminho: o nome vai para o
    // `Content-Disposition` do download.
    if t.contains('\n') || t.contains('\r') || t.contains('/') || t.contains('\\') {
        return Err(DomainError::invalid(
            "recording.invalid_filename",
            "o nome é uma só linha, sem barras",
        )
        .with_field(
            "filename",
            format!("1-{MAX_FILENAME} caracteres, uma linha"),
        ));
    }
    Ok(t)
}

/// Descrição: pode ficar VAZIA (apaga), até [`MAX_DESCRIPTION`] caracteres,
/// com quebras de linha e tabulações.
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
/// uma etiqueta grande ou com vírgula em vez de a cortar em silêncio — só as
/// vazias se ignoram, porque um campo de texto deixa sempre separadores a mais.
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

/// Uma marca temporal cabe na gravação: `0..=duração` quando a duração é
/// conhecida, senão `0..=48 h`. Em MILISSEGUNDOS — a unidade do contrato de
/// capítulos e comentários (R234).
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
//  Visibilidade e biblioteca
// ---------------------------------------------------------------------------

/// Quem vê a gravação além de quem tem ligação directa com ela.
///
/// Nunca há visibilidade PÚBLICA por aqui: o link público continua a ser
/// `recording_share_links`, com token e prazo.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Visibility {
    /// Quem participou na sala, quem carregou, e com quem foi partilhada.
    Private,
    /// Além desses, os membros ACTIVOS de uma organização do autor.
    Org,
}

impl Visibility {
    pub const ALL: [&'static str; 2] = ["private", "org"];

    pub fn parse(s: &str) -> Result<Self, DomainError> {
        Ok(match s {
            "private" => Self::Private,
            "org" => Self::Org,
            other => {
                return Err(DomainError::invalid(
                    "recording.invalid_visibility",
                    format!(
                        "visibilidade inválida «{other}» — válidas: {}",
                        Self::ALL.join(", ")
                    ),
                )
                .with_field("visibility", Self::ALL.join(" | ")))
            }
        })
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Private => "private",
            Self::Org => "org",
        }
    }
}

/// Qual biblioteca se lista (`GET /api/recordings?scope=`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LibraryScope {
    /// As de sempre: carregou, participou, ou foram-lhe partilhadas.
    #[default]
    Mine,
    /// As publicadas que quem pede vê — incluindo as da organização em que
    /// NÃO participou. É esta a biblioteca que dá sentido a publicar.
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
    /// A gravação está PUBLICADA para a organização (`visibility = 'org'` e
    /// `published_at` preenchido) E quem pede é membro ACTIVO de uma
    /// organização do dono. Sem isto, publicar não mostrava a gravação a
    /// ninguém: era uma coluna que se escrevia e mais nada a lia (R235).
    pub published_to_my_org: bool,
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
        !self.departed()
            && (self.is_uploader || self.participant || self.shared || self.published_to_my_org)
    }

    /// Ligação DIRECTA com a gravação — dono, admin activo da organização,
    /// participante da sala, ou partilha explícita. **Não** inclui
    /// `published_to_my_org`: publicar dá reprodução (`can_view`), não uma
    /// relação com quem esteve na reunião.
    fn has_direct_relation(&self) -> bool {
        !self.departed() && (self.is_uploader || self.org_admin || self.participant || self.shared)
    }

    /// Ler a transcrição (texto e segmentos). Mais estrito do que `can_view`:
    /// a transcrição pode conter nomes e decisões que a dona não revisou antes
    /// de publicar a gravação para ser VISTA.
    pub fn can_see_transcript(&self) -> bool {
        self.has_direct_relation()
    }

    /// Ler quem esteve na sala (`room_participants`). Mesma regra e mesma
    /// razão que [`Self::can_see_transcript`]: publicar a gravação não é
    /// convite para um colega nunca-presente saber quem esteve na reunião.
    pub fn can_see_participants(&self) -> bool {
        self.has_direct_relation()
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

    /// Partilhar com pessoas e gerir o link público: só o DONO, e activo. Um
    /// admin da organização gere metadados mas não decide a quem a gravação de
    /// outra pessoa é mostrada; e quem saiu da empresa já não a partilha (S3).
    pub fn can_share(&self) -> bool {
        !self.departed() && self.is_uploader
    }

    /// Ler uma legenda: publicada, para quem vê; em qualquer estado, para quem
    /// gere. Um rascunho de legenda ainda não é para ser lido.
    pub fn can_read_caption(&self, caption_status: &str) -> bool {
        self.can_manage() || (self.can_see() && caption_status == "published")
    }

    /// Apagar um comentário: o autor (enquanto chega à gravação) ou quem a
    /// gere (moderação). Alterar o TEXTO continua a ser só do autor.
    pub fn can_delete_comment(&self, is_author: bool) -> bool {
        (is_author && self.can_see()) || self.can_manage()
    }

    /// A gravação aparece na biblioteca `scope`. É o que o SQL
    /// `LIBRARY_VISIBLE_*` de `server/src/recordings.rs` escreve; o teste
    /// `library_scopes_agree_with_access_facts` prova que concordam.
    pub fn listed_in(&self, scope: LibraryScope, published: bool) -> bool {
        match scope {
            // A biblioteca de sempre não muda por a gravação estar publicada:
            // publicar não enche a biblioteca pessoal dos colegas.
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

    /// A linha nasce em `processing` antes de o ffmpeg correr
    /// (`recorder::insert_processing`): é uma gravação a compor, não uma falha.
    #[test]
    fn a_compor_nao_e_falhada() {
        let f = facts("processing");
        assert_eq!(file_status(f), FileStatus::Processing);
        assert_eq!(file_status(f).as_str(), "processing");
        assert!(f.composing());
        assert!(!f.has_file(), "a compor ainda não há ficheiro para ler");
        assert_eq!(transcript_status(f), TranscriptStatus::None);
        // Publicada ou não, o `state` é o do ficheiro enquanto ele não existe.
        for published in [false, true] {
            assert_eq!(display_state(file_status(f), published), "processing");
        }
        // Marcas de transcrição que tenham ficado na linha não a promovem.
        let com_marcas = ProcessingFacts {
            transcribed: true,
            lease_active: true,
            ..f
        };
        assert_eq!(file_status(com_marcas), FileStatus::Processing);
        assert_eq!(transcript_status(com_marcas), TranscriptStatus::None);
    }

    /// O que `failed` e um `status` desconhecido mostram não mudou: falham
    /// fechado, e publicar não os promove.
    #[test]
    fn falhada_e_desconhecida_continuam_falhadas() {
        for status in ["failed", "zombie", ""] {
            let f = facts(status);
            assert_eq!(file_status(f), FileStatus::Failed, "{status:?}");
            assert_eq!(display_state(file_status(f), true), "failed");
            assert!(!f.has_file() && !f.composing());
        }
        assert_eq!(file_status(facts("ready")), FileStatus::Ready);
        assert_eq!(file_status(facts("transcribing")), FileStatus::Ready);
        assert_eq!(display_state(FileStatus::Ready, true), "published");
    }

    /// Entregar a gravação a outra pessoa pede ficheiro, e a recusa diz qual
    /// dos dois casos é: esperar (`processing`) ou não há o que esperar.
    #[test]
    fn entregar_a_outrem_pede_ficheiro() {
        use delonix_meet_core::ErrorKind;
        assert!(require_file(facts("ready")).is_ok());
        assert!(require_file(facts("transcribing")).is_ok());
        // A ser transcrita (reserva activa) tem ficheiro.
        let a_transcrever = ProcessingFacts {
            lease_active: true,
            ..facts("ready")
        };
        assert!(require_file(a_transcrever).is_ok());

        let e = require_file(facts("processing")).unwrap_err();
        assert_eq!((e.kind, e.code), (ErrorKind::Conflict, "recording.processing"));
        assert_eq!(e.code, processing_conflict().code);
        assert!(!e.message.contains("falhou"), "a compor não é falha: {e}");
        // Falhada e um estado desconhecido falham fechado, com o mesmo código.
        for status in ["failed", "zombie", ""] {
            let e = require_file(facts(status)).unwrap_err();
            assert_eq!(
                (e.kind, e.code),
                (ErrorKind::Conflict, "recording.no_file"),
                "{status:?}"
            );
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

    // ---- o contrato em milissegundos e a publicação (R234/R235) ----

    #[test]
    fn t_ms_cabe_na_gravacao() {
        assert_eq!(validate_t_ms(0, Some(10_000)).unwrap(), 0);
        assert_eq!(validate_t_ms(10_000, Some(10_000)).unwrap(), 10_000);
        assert!(validate_t_ms(10_001, Some(10_000)).is_err());
        assert!(validate_t_ms(-1, Some(10_000)).is_err());
        // Sem duração conhecida, o tecto é 48 h — não «qualquer coisa».
        assert!(validate_t_ms(MAX_T_MS_UNKNOWN_DURATION, None).is_ok());
        assert!(validate_t_ms(MAX_T_MS_UNKNOWN_DURATION + 1, None).is_err());
        // Uma duração negativa na base não abre o tecto.
        assert!(validate_t_ms(MAX_T_MS_UNKNOWN_DURATION + 1, Some(-5)).is_err());
    }

    #[test]
    fn publicar_da_reproducao_mas_nao_download_nem_transcricao() {
        // Um colega de organização que NUNCA participou, e a quem a gravação
        // nunca foi partilhada: vê-a porque está publicada.
        let colega = AccessFacts {
            active_member: true,
            published_to_my_org: true,
            ..Default::default()
        };
        assert!(colega.can_view(), "publicar tem de dar reprodução");
        assert!(colega.can_see());
        assert!(!colega.can_download(), "publicar não dá o ficheiro");
        assert!(!colega.can_manage());
        assert!(!colega.can_share());
        // Publicar para VER não abre a transcrição nem a lista de presentes.
        assert!(!colega.can_see_transcript());
        assert!(!colega.can_see_participants());

        // Quem saiu da organização (S3) não vê a publicada.
        let saiu = AccessFacts {
            archived_member: true,
            published_to_my_org: true,
            ..Default::default()
        };
        assert!(!saiu.can_view());
    }

    #[test]
    fn scope_parse_recusa_o_desconhecido() {
        assert_eq!(LibraryScope::parse(None).unwrap(), LibraryScope::Mine);
        assert_eq!(LibraryScope::parse(Some("")).unwrap(), LibraryScope::Mine);
        assert_eq!(
            LibraryScope::parse(Some("mine")).unwrap(),
            LibraryScope::Mine
        );
        assert_eq!(
            LibraryScope::parse(Some("published")).unwrap(),
            LibraryScope::Published
        );
        let err = LibraryScope::parse(Some("todas")).unwrap_err();
        assert_eq!(err.code, "recording.invalid_scope");
    }

    #[test]
    fn listed_in_separa_as_duas_bibliotecas() {
        let participante = AccessFacts {
            participant: true,
            active_member: true,
            ..Default::default()
        };
        // Na biblioteca pessoal está, publicada ou não.
        assert!(participante.listed_in(LibraryScope::Mine, false));
        assert!(participante.listed_in(LibraryScope::Mine, true));
        // Na publicada só quando de facto está publicada.
        assert!(!participante.listed_in(LibraryScope::Published, false));
        assert!(participante.listed_in(LibraryScope::Published, true));

        // O colega que só a vê por publicação NÃO entope a biblioteca pessoal.
        let colega = AccessFacts {
            active_member: true,
            published_to_my_org: true,
            ..Default::default()
        };
        assert!(!colega.listed_in(LibraryScope::Mine, true));
        assert!(colega.listed_in(LibraryScope::Published, true));
    }

    #[test]
    fn legenda_rascunho_so_para_quem_gere() {
        let dono = AccessFacts {
            is_uploader: true,
            active_member: true,
            ..Default::default()
        };
        let colega = AccessFacts {
            active_member: true,
            published_to_my_org: true,
            ..Default::default()
        };
        assert!(dono.can_read_caption("draft"));
        assert!(dono.can_read_caption("published"));
        assert!(!colega.can_read_caption("draft"));
        assert!(colega.can_read_caption("published"));
    }

    #[test]
    fn apagar_comentario_e_do_autor_ou_de_quem_modera() {
        let autor = AccessFacts {
            participant: true,
            active_member: true,
            ..Default::default()
        };
        let admin = AccessFacts {
            org_admin: true,
            active_member: true,
            ..Default::default()
        };
        assert!(autor.can_delete_comment(true));
        assert!(!autor.can_delete_comment(false), "não modera os dos outros");
        assert!(admin.can_delete_comment(false), "quem gere modera");
    }

    #[test]
    fn etiquetas_normalizam_e_recusam() {
        assert_eq!(
            normalize_tags(&["#Orçamento".into(), " ".into(), "orçamento".into()]).unwrap(),
            vec!["orçamento".to_string()],
            "apara, tira o cardinal, baixa a caixa e não repete"
        );
        assert!(normalize_tags(&["a,b".into()]).is_err(), "vírgula recusada");
        assert!(normalize_tags(&["x".repeat(MAX_TAG_CHARS + 1)]).is_err());
        let muitas: Vec<String> = (0..=MAX_TAGS).map(|i| format!("t{i}")).collect();
        assert!(normalize_tags(&muitas).is_err());
    }

    #[test]
    fn descricao_pode_ficar_vazia_mas_nao_enorme() {
        assert_eq!(validate_description("  ").unwrap(), String::new());
        assert_eq!(validate_description(" olá\nmundo ").unwrap(), "olá\nmundo");
        assert!(validate_description(&"x".repeat(MAX_DESCRIPTION + 1)).is_err());
        assert!(validate_description("mau\u{0}").is_err());
    }

    #[test]
    fn nome_da_gravacao_e_uma_linha_sem_barras() {
        assert_eq!(validate_filename(" Reunião ").unwrap(), "Reunião");
        assert!(validate_filename("").is_err());
        assert!(
            validate_filename("a/b").is_err(),
            "sem separador de caminho"
        );
        assert!(validate_filename("a\nb").is_err());
        assert!(validate_filename(&"x".repeat(MAX_FILENAME + 1)).is_err());
    }

    #[test]
    fn visibilidade_so_tem_dois_valores() {
        assert_eq!(Visibility::parse("private").unwrap(), Visibility::Private);
        assert_eq!(Visibility::parse("org").unwrap(), Visibility::Org);
        // «public» não existe: o link público é outro mecanismo, com token.
        let err = Visibility::parse("public").unwrap_err();
        assert_eq!(err.code, "recording.invalid_visibility");
    }
}
