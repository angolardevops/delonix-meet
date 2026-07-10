use regex::Regex;
use std::sync::LazyLock;

// Expressões Regulares de Prevenção de Fuga de Dados (DLP)
static CREDIT_CARD_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?:\d[ -]*?){13,16}").unwrap());

static NIF_RE: LazyLock<Regex> = LazyLock::new(|| {
    // Valida padrões parecidos com o NIF Português (9 dígitos)
    Regex::new(r"\b[1-9]\d{8}\b").unwrap()
});

static API_KEY_RE: LazyLock<Regex> = LazyLock::new(|| {
    // Simula uma chave de API (ex: sk-abcdef1234567890abcdef1234567890)
    Regex::new(r"sk-[a-zA-Z0-9]{32}").unwrap()
});

/// Redige informações sensíveis de um texto.
pub fn censor(text: &str) -> String {
    let mut safe = text.to_string();

    // Censura Cartão de Crédito
    safe = CREDIT_CARD_RE
        .replace_all(&safe, "[CARTÃO DE CRÉDITO BLOQUEADO PELO DLP]")
        .to_string();

    // Censura NIF
    safe = NIF_RE
        .replace_all(&safe, "[NIF BLOQUEADO PELO DLP]")
        .to_string();

    // Censura Chaves API
    safe = API_KEY_RE
        .replace_all(&safe, "[CHAVE API CENSURADA]")
        .to_string();

    safe
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_censor_credit_card() {
        let input = "O meu cartão é 1234 5678 1234 5678, usa-o bem.";
        let output = censor(input);
        assert_eq!(
            output,
            "O meu cartão é [CARTÃO DE CRÉDITO BLOQUEADO PELO DLP], usa-o bem."
        );
    }

    #[test]
    fn test_censor_nif() {
        let input = "Fatura para o NIF 234567890 por favor.";
        let output = censor(input);
        assert_eq!(
            output,
            "Fatura para o NIF [NIF BLOQUEADO PELO DLP] por favor."
        );
    }

    #[test]
    fn test_censor_api_key() {
        let input = "A chave secreta é sk-abcdef1234567890abcdef1234567890 não partilhes.";
        let output = censor(input);
        assert_eq!(
            output,
            "A chave secreta é [CHAVE API CENSURADA] não partilhes."
        );
    }

    #[test]
    fn test_no_censorship_needed() {
        let input = "Olá, tudo bem? 12345";
        let output = censor(input);
        assert_eq!(output, "Olá, tudo bem? 12345");
    }
}
