//! Fornecedor de IA local (Ollama in-cluster): as razões por que um modelo não
//! deu uma resposta aproveitável, com código estável, e as línguas que a
//! tradução conhece. Puro — o adaptador HTTP está em `server/src/ai.rs`.
//!
//! Existe para que quem chama possa dizer a VERDADE ao cliente: «o modelo não
//! está instalado» e «o serviço não responde» resolvem-se de maneiras
//! diferentes, e um `None` fazia de tudo «LLM sem resposta» (a linha antiga).

use delonix_meet_core::{DomainError, ErrorKind};

/// Porque é que o LLM local não deu uma resposta aproveitável.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LlmFailure {
    /// `OLLAMA_URL` vazio: a IA está desligada neste servidor.
    NotConfigured,
    /// O `OLLAMA_URL` do operador foi recusado pela guarda de saída (metadados
    /// da cloud, esquema errado, credenciais no URL).
    Rejected,
    /// Não se conseguiu ligar ao Ollama (recusado, DNS, rede).
    Unreachable,
    /// O Ollama não respondeu dentro do tecto (segundos).
    Timeout(u64),
    /// O modelo não está instalado no Ollama.
    ModelMissing(String),
    /// O Ollama respondeu com um estado de erro.
    Upstream(u16),
    /// A resposta chegou mas não se consegue usar: não é JSON, vem vazia, ou o
    /// texto do modelo não tem a forma pedida.
    BadResponse,
}

impl LlmFailure {
    /// Código estável (contrato).
    pub fn code(&self) -> &'static str {
        match self {
            LlmFailure::NotConfigured => "ai.not_configured",
            LlmFailure::Rejected => "ai.url_rejected",
            LlmFailure::Unreachable => "ai.unreachable",
            LlmFailure::Timeout(_) => "ai.timeout",
            LlmFailure::ModelMissing(_) => "ai.model_missing",
            LlmFailure::Upstream(_) => "ai.upstream_error",
            LlmFailure::BadResponse => "ai.bad_response",
        }
    }

    /// A razão curta que o `GET …/ai/status` publica em `reason`.
    pub fn reason(&self) -> &'static str {
        self.code().trim_start_matches("ai.")
    }

    /// Mensagem para pessoas, em português europeu. Nunca inclui o URL do
    /// Ollama: é um endereço interno do cluster.
    pub fn message(&self) -> String {
        match self {
            LlmFailure::NotConfigured => {
                "IA local não configurada neste servidor (OLLAMA_URL)".into()
            }
            LlmFailure::Rejected => "o endereço do Ollama foi recusado pela guarda de saída".into(),
            LlmFailure::Unreachable => "o serviço Ollama não responde".into(),
            LlmFailure::Timeout(s) => format!("o modelo não respondeu em {s} s"),
            LlmFailure::ModelMissing(m) => format!("o modelo «{m}» não está instalado no Ollama"),
            LlmFailure::Upstream(code) => {
                format!("o serviço Ollama respondeu com erro (HTTP {code})")
            }
            LlmFailure::BadResponse => {
                "o modelo devolveu uma resposta que não se consegue usar".into()
            }
        }
    }

    /// Como erro de domínio: `503` com o código. Nunca `500` — o problema é
    /// um serviço de que dependemos, não uma avaria nossa.
    pub fn into_domain(self) -> DomainError {
        DomainError::new(ErrorKind::Unavailable, self.code(), self.message())
    }
}

/// `model` está na lista do Ollama — igual, ou igual sem o sufixo `:latest`
/// (o Ollama lista `llama3:latest` para quem pediu `llama3`).
pub fn model_listed(installed: &[String], model: &str) -> bool {
    let bare = |n: &str| n.strip_suffix(":latest").unwrap_or(n).to_string();
    let want = bare(model.trim());
    installed.iter().any(|n| n == model || bare(n) == want)
}

/// Línguas de chegada que o prompt de tradução conhece (subtag primário).
///
/// Umbundu, Kimbundu e Kikongo NÃO estão: nenhum modelo local as traduz com
/// qualidade que se possa pôr numa legenda, e uma língua anunciada que devolve
/// texto inventado é pior do que uma que não está na lista.
pub const TRANSLATE_TARGETS: &[&str] = &["pt", "en", "fr", "es", "de", "zh"];

/// O nome da língua para o prompt, pelo subtag primário (`pt-AO` → `pt`).
pub fn target_name(lang: &str) -> Option<&'static str> {
    Some(match primary_subtag(lang) {
        "pt" => "European Portuguese",
        "en" => "English",
        "fr" => "French",
        "es" => "Spanish",
        "de" => "German",
        "zh" => "Simplified Chinese",
        _ => return None,
    })
}

/// `pt-AO` → `pt`.
pub fn primary_subtag(lang: &str) -> &str {
    lang.split('-').next().unwrap_or(lang)
}

/// Uma língua pedida que a tradução não conhece: `400`, com a lista.
pub fn unsupported_target(lang: &str) -> DomainError {
    DomainError::invalid(
        "ai.unsupported_language",
        format!(
            "tradução para «{lang}» não suportada; línguas: {}",
            TRANSLATE_TARGETS.join(", ")
        ),
    )
    .with_field("lang", TRANSLATE_TARGETS.join(" | "))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn messages_never_carry_the_url_and_codes_are_stable() {
        let all = [
            LlmFailure::NotConfigured,
            LlmFailure::Rejected,
            LlmFailure::Unreachable,
            LlmFailure::Timeout(120),
            LlmFailure::ModelMissing("m".into()),
            LlmFailure::Upstream(502),
            LlmFailure::BadResponse,
        ];
        for e in &all {
            assert!(!e.message().contains("http"), "{}", e.message());
            assert!(e.code().starts_with("ai."));
            assert_eq!(e.clone().into_domain().kind, ErrorKind::Unavailable);
        }
        assert_eq!(
            LlmFailure::Timeout(120).message(),
            "o modelo não respondeu em 120 s"
        );
        assert_eq!(
            LlmFailure::ModelMissing("x".into()).reason(),
            "model_missing"
        );
        assert_eq!(LlmFailure::NotConfigured.reason(), "not_configured");
    }

    #[test]
    fn model_listing_accepts_latest_suffix() {
        let m = vec!["qwen2.5:1.5b".to_string(), "llama3:latest".to_string()];
        assert!(model_listed(&m, "qwen2.5:1.5b"));
        assert!(model_listed(&m, "llama3"));
        assert!(model_listed(&m, "llama3:latest"));
        assert!(!model_listed(&m, "qwen2.5:7b"));
        assert!(!model_listed(&m, "qwen2.5"));
    }

    #[test]
    fn translation_targets_by_primary_subtag() {
        assert_eq!(target_name("pt-AO"), Some("European Portuguese"));
        assert_eq!(target_name("zh"), Some("Simplified Chinese"));
        assert!(target_name("umb").is_none());
        assert_eq!(unsupported_target("umb").code, "ai.unsupported_language");
    }
}
