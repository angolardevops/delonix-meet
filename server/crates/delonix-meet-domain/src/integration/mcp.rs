//! O catálogo de tools MCP (ADR-0027) — fechado, puro, e a única fonte da
//! verdade sobre o que um agente pode pedir.
//!
//! Porque vive no domínio e não no adaptador: o que cada tool exige
//! (**escopo** do token e **capacidade** da pessoa) é uma regra de
//! autorização, e o portão `scripts/check-mcp-tools.sh` lê esta tabela para
//! verificar que nenhuma tool entra sem as duas declaradas, sem esquema, ou
//! com `org_id`/`user_id` à entrada.
//!
//! **Nenhuma tool aceita `org_id` nem `user_id`** (ADR-0027 §4). A organização
//! vem do token, que a traz do consentimento; aceitá-la do cliente
//! transformava o isolamento numa escolha de quem chama.
//!
//! O catálogo é fechado como o dos escopos: não há tool dinâmica, e uma tool
//! nova é uma linha aqui mais a ligação no adaptador.

use crate::identity::{authorization::Capability, oauth::McpScope};

/// Uma tool do catálogo.
#[derive(Debug, Clone, Copy)]
pub struct Tool {
    /// O nome que o agente usa em `tools/call`. Contrato.
    pub name: &'static str,
    /// Uma linha para a pessoa (e para o agente) saber o que isto faz.
    pub summary: &'static str,
    /// O escopo do token que a abre.
    pub scope: McpScope,
    /// A capacidade que a PESSOA precisa de ter (ADR-0008). `None` quando
    /// basta a pertença e a visibilidade do recurso — que o handler do BFF já
    /// impõe, e é a maioria dos casos de leitura.
    pub capability: Option<Capability>,
    /// A tool não muda nada. Vai no `readOnlyHint` que o MCP anuncia, e é o
    /// que permite a um cliente distinguir olhar de mexer.
    pub read_only: bool,
    /// Os parâmetros, em JSON Schema (`type: object`). Texto literal para que
    /// o portão o possa ler sem compilar nada.
    pub input_schema: &'static str,
}

/// Sem parâmetros.
const SEM_ARGS: &str = r#"{"type":"object","properties":{},"additionalProperties":false}"#;

/// Só o id de uma reunião.
const ARGS_REUNIAO: &str = r#"{"type":"object","properties":{"meeting_id":{"type":"string","format":"uuid","description":"A reunião."}},"required":["meeting_id"],"additionalProperties":false}"#;

/// Só o id de uma gravação.
const ARGS_GRAVACAO: &str = r#"{"type":"object","properties":{"recording_id":{"type":"string","format":"uuid","description":"A gravação."}},"required":["recording_id"],"additionalProperties":false}"#;

/// Uma gravação e uma língua.
const ARGS_LEGENDA: &str = r#"{"type":"object","properties":{"recording_id":{"type":"string","format":"uuid","description":"A gravação."},"lang":{"type":"string","description":"Língua BCP-47 (ex.: pt, en, fr, zh)."}},"required":["recording_id","lang"],"additionalProperties":false}"#;

/// Pedir uma tradução de legendas para uma língua.
const ARGS_TRADUZIR: &str = r#"{"type":"object","properties":{"recording_id":{"type":"string","format":"uuid","description":"A gravação."},"lang":{"type":"string","description":"Língua de DESTINO, BCP-47."},"source":{"type":"string","enum":["transcript","translation"],"description":"`transcript` constrói da transcrição na língua dela; `translation` traduz segmento a segmento com o LLM local."}},"required":["recording_id","lang","source"],"additionalProperties":false}"#;

/// Pedir um recorte (ou um *short*) de uma gravação.
///
/// `captions_lang` produz um WebVTT **ao lado** do ficheiro; não queima nada na
/// imagem — o ffmpeg desta imagem não tem libass
/// (`delonix_meet_domain::content::clip`).
const ARGS_RECORTE: &str = r#"{"type":"object","properties":{"recording_id":{"type":"string","format":"uuid","description":"A gravação de origem."},"kind":{"type":"string","enum":["clip","short"],"description":"`clip` até 15 min; `short` vertical 9:16 até 3 min."},"label":{"type":"string","description":"Nome do recorte, 1 a 120 caracteres."},"start_ms":{"type":"integer","description":"Início, em milissegundos da gravação."},"end_ms":{"type":"integer","description":"Fim, em milissegundos. end_ms − start_ms ≥ 1000."},"focus":{"type":"string","enum":["center","left","right"],"description":"Só no short: que parte da largura fica. Omissão: center."},"captions_lang":{"type":"string","description":"Produz um .vtt ao lado, nesta língua (BCP-47). NÃO queima legendas na imagem."}},"required":["recording_id","kind","label","start_ms","end_ms"],"additionalProperties":false}"#;

/// Um recorte de uma gravação.
const ARGS_UM_RECORTE: &str = r#"{"type":"object","properties":{"recording_id":{"type":"string","format":"uuid","description":"A gravação."},"clip_id":{"type":"string","format":"uuid","description":"O recorte."}},"required":["recording_id","clip_id"],"additionalProperties":false}"#;

/// Sugestões de edição a partir de uma transcrição que o cliente já tem.
///
/// Os nomes dos campos são os do serviço (`ai_studio::StudioReq`), não nomes
/// novos: o esquema de uma tool descreve o que o handler aceita, e um nome
/// diferente aqui obrigaria a traduzi-lo — que é a primeira forma de a cópia
/// entrar.
const ARGS_SUGESTOES: &str = r#"{"type":"object","properties":{"task":{"type":"string","enum":["summary","publication","fillers"],"description":"`summary` dá resumo com capítulos; `publication` o texto de publicação; `fillers` os bordões a cortar."},"segments":{"type":"array","description":"Os segmentos da transcrição.","items":{"type":"object","properties":{"start_ms":{"type":"integer"},"end_ms":{"type":"integer"},"text":{"type":"string"}},"required":["start_ms","end_ms","text"]}},"language":{"type":"string","description":"Língua da resposta, BCP-47. Omitida, responde na da transcrição."},"title":{"type":"string","description":"Título do vídeo, se o houver — ajuda o modelo."}},"required":["task","segments"],"additionalProperties":false}"#;

impl Tool {
    /// O catálogo inteiro. **Só entra aqui o que tem código por trás** — a
    /// segunda regra da casa, e o que `tools/list` promete é o que
    /// `tools/call` faz.
    pub const ALL: &'static [Tool] = &[
        // ---- reuniões e acta ----
        Tool {
            name: "meetings_list",
            summary: "As reuniões a que tens acesso, mais recentes primeiro.",
            scope: McpScope::MeetRead,
            capability: None,
            read_only: true,
            input_schema: SEM_ARGS,
        },
        Tool {
            name: "meeting_get",
            summary: "Os detalhes de uma reunião tua.",
            scope: McpScope::MeetRead,
            capability: None,
            read_only: true,
            input_schema: ARGS_REUNIAO,
        },
        Tool {
            name: "meeting_minutes_get",
            summary: "A acta (MoM) e a transcrição de uma reunião tua.",
            scope: McpScope::MinutesRead,
            capability: None,
            read_only: true,
            input_schema: ARGS_REUNIAO,
        },
        Tool {
            name: "meeting_minutes_summarize",
            summary: "Pede ao LLM local o resumo da acta. Devolve o estado da fila — não espera.",
            scope: McpScope::MinutesWrite,
            capability: None,
            read_only: false,
            input_schema: ARGS_REUNIAO,
        },
        // ---- gravações ----
        Tool {
            name: "recordings_list",
            summary: "A tua biblioteca de gravações.",
            scope: McpScope::RecordingsRead,
            capability: None,
            read_only: true,
            input_schema: SEM_ARGS,
        },
        Tool {
            name: "recording_get",
            summary: "Os detalhes de uma gravação que alcanças.",
            scope: McpScope::RecordingsRead,
            capability: None,
            read_only: true,
            input_schema: ARGS_GRAVACAO,
        },
        Tool {
            name: "recording_transcript_get",
            summary: "A transcrição de uma gravação, em segmentos com tempos.",
            scope: McpScope::MinutesRead,
            capability: None,
            read_only: true,
            input_schema: ARGS_GRAVACAO,
        },
        // ---- legendas e tradução ----
        Tool {
            name: "recording_captions_list",
            summary: "As legendas de uma gravação, por língua, com o estado de cada uma.",
            scope: McpScope::RecordingsRead,
            capability: None,
            read_only: true,
            input_schema: ARGS_GRAVACAO,
        },
        Tool {
            name: "recording_caption_get",
            summary: "O WebVTT de uma legenda, na língua pedida.",
            scope: McpScope::RecordingsRead,
            capability: None,
            read_only: true,
            input_schema: ARGS_LEGENDA,
        },
        Tool {
            name: "recording_captions_translate",
            summary: "Gera ou traduz as legendas de uma gravação para uma língua. Corre em segundo plano com progresso.",
            scope: McpScope::CaptionsWrite,
            capability: Some(Capability::StudioGenerateCaptions),
            read_only: false,
            input_schema: ARGS_TRADUZIR,
        },
        // ---- capítulos e edição ----
        Tool {
            name: "recording_chapters_list",
            summary: "Os capítulos de uma gravação, automáticos e manuais.",
            scope: McpScope::RecordingsRead,
            capability: None,
            read_only: true,
            input_schema: ARGS_GRAVACAO,
        },
        Tool {
            name: "recording_chapters_generate",
            summary: "Propõe capítulos com o LLM local. Substitui só os automáticos — um capítulo escrito à mão nunca é apagado por uma máquina.",
            scope: McpScope::ChaptersWrite,
            capability: Some(Capability::StudioEditTimeline),
            read_only: false,
            input_schema: ARGS_GRAVACAO,
        },
        Tool {
            name: "recording_chapters_status",
            summary: "O estado da última geração de capítulos: a correr, feita, ou falhada e porquê.",
            scope: McpScope::RecordingsRead,
            capability: None,
            read_only: true,
            input_schema: ARGS_GRAVACAO,
        },
        // ---- recortes e shorts ----
        Tool {
            name: "recording_clip_create",
            summary: "Pede um recorte de uma gravação. Devolve o recorte na fila — não espera pelo ffmpeg.",
            scope: McpScope::ClipsWrite,
            capability: Some(Capability::StudioEditTimeline),
            read_only: false,
            input_schema: ARGS_RECORTE,
        },
        Tool {
            name: "recording_clips_list",
            summary: "Os recortes e shorts de uma gravação, com o estado de cada um.",
            scope: McpScope::RecordingsRead,
            capability: None,
            read_only: true,
            input_schema: ARGS_GRAVACAO,
        },
        Tool {
            name: "recording_clip_status",
            summary: "O estado de um recorte: na fila, a compor (com progresso), pronto, ou falhado e porquê.",
            scope: McpScope::RecordingsRead,
            capability: None,
            read_only: true,
            input_schema: ARGS_UM_RECORTE,
        },
        Tool {
            name: "edit_suggestions",
            summary: "A partir de uma transcrição: resumo com capítulos, texto de publicação, ou os bordões a cortar.",
            scope: McpScope::RecordingsRead,
            capability: Some(Capability::StudioEditTimeline),
            read_only: true,
            input_schema: ARGS_SUGESTOES,
        },
    ];

    pub fn by_name(name: &str) -> Option<&'static Tool> {
        Tool::ALL.iter().find(|t| t.name == name)
    }
}

/// A versão do protocolo que este servidor fala.
pub const PROTOCOL_VERSION: &str = "2025-06-18";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nenhuma_tool_aceita_org_id_nem_user_id() {
        // ADR-0027 §4: a organização vem do token. Um parâmetro destes num
        // esquema transformava o isolamento numa escolha do cliente.
        for t in Tool::ALL {
            assert!(
                !t.input_schema.contains("org_id") && !t.input_schema.contains("user_id"),
                "{} declara org_id/user_id à entrada",
                t.name
            );
        }
    }

    #[test]
    fn o_catalogo_e_coerente() {
        let mut vistos = std::collections::HashSet::new();
        for t in Tool::ALL {
            assert!(vistos.insert(t.name), "tool repetida: {}", t.name);
            assert!(!t.summary.is_empty(), "{} sem resumo", t.name);
            assert!(
                t.input_schema.starts_with(r#"{"type":"object""#),
                "{} sem esquema de objecto",
                t.name
            );
            assert_eq!(Tool::by_name(t.name).map(|x| x.name), Some(t.name));
            // Uma tool que MEXE não pode pedir só um escopo de leitura.
            if !t.read_only {
                assert!(
                    !t.scope.is_read_only(),
                    "{} escreve mas pede um escopo de leitura ({})",
                    t.name,
                    t.scope.as_str()
                );
            }
        }
        assert!(Tool::by_name("inventada").is_none());
    }
}
