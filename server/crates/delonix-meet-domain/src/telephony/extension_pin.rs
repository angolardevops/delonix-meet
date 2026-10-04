//! O PIN de um ramal e o intervalo de onde saem os números automáticos
//! (plano de produção, item 3.8).
//!
//! Três coisas separadas (decisão do dono, 2026-10-04): o NÚMERO do ramal é a
//! identidade e não é secreto; a PASSWORD SIP é a credencial do aparelho; o
//! PIN é um código secreto de seis dígitos, da pessoa.
//!
//! Sem IO: quem gera os bytes aleatórios, guarda o hash e conta as falhas é
//! `server/src/extension_pin.rs`.

use std::collections::HashSet;

/// Um PIN tem exactamente seis dígitos.
pub const PIN_LEN: usize = 6;

/// Falhas seguidas que bloqueiam o PIN.
pub const MAX_FAILED_ATTEMPTS: i32 = 5;

/// Quanto tempo o PIN fica bloqueado depois da quinta falha.
pub const LOCK_SECS: i64 = 15 * 60;

/// Intervalo de numeração automática quando a organização não escolheu um.
pub const DEFAULT_RANGE_START: u32 = 1000;
pub const DEFAULT_RANGE_END: u32 = 1999;

/// Limites de um intervalo: a forma de um número curto (3 a 5 dígitos), sem
/// zero à esquerda, porque o intervalo é de inteiros.
pub const RANGE_MIN: u32 = 100;
pub const RANGE_MAX: u32 = 99_999;

/// Porque é que um PIN é recusado. Vale para o que o servidor gera e para o
/// que a pessoa escolhe.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PinRefusal {
    /// Não são seis dígitos ASCII.
    Format,
    /// Todos os dígitos iguais (`000000`, `777777`).
    Repeated,
    /// Sequência ascendente ou descendente (`123456`, `987654`, `890123`).
    Sequence,
    /// Contém o número do ramal (`001234` ou `123499` para o ramal `1234`).
    ContainsExtension,
}

impl PinRefusal {
    /// Código estável do envelope de erro.
    pub fn code(self) -> &'static str {
        match self {
            Self::Format => "ramais.pin_format",
            Self::Repeated => "ramais.pin_repeated",
            Self::Sequence => "ramais.pin_sequence",
            Self::ContainsExtension => "ramais.pin_contains_extension",
        }
    }

    pub fn message(self) -> &'static str {
        match self {
            Self::Format => "o PIN tem de ter exactamente 6 dígitos",
            Self::Repeated => "o PIN não pode ter os dígitos todos iguais",
            Self::Sequence => "o PIN não pode ser uma sequência",
            Self::ContainsExtension => "o PIN não pode conter o número do ramal",
        }
    }
}

/// `None` se o PIN serve; a razão se não serve.
///
/// «Igual ao número do ramal» não pode acontecer à letra — o ramal tem 3 a 5
/// dígitos e o PIN seis. O que se recusa é o PIN que CONTÉM o número do ramal:
/// é o que alguém faz para o tornar «igual» (`001234`, `123400`, `123412`).
pub fn pin_refusal(pin: &str, extension: &str) -> Option<PinRefusal> {
    let d = pin.as_bytes();
    if d.len() != PIN_LEN || !d.iter().all(u8::is_ascii_digit) {
        return Some(PinRefusal::Format);
    }
    if d.iter().all(|&b| b == d[0]) {
        return Some(PinRefusal::Repeated);
    }
    // No teclado o 0 vem depois do 9: `567890` e `210987` também são sequências.
    let step = |by: u8| d.windows(2).all(|w| (w[0] - b'0' + by) % 10 == w[1] - b'0');
    if step(1) || step(9) {
        return Some(PinRefusal::Sequence);
    }
    if !extension.is_empty() && pin.contains(extension) {
        return Some(PinRefusal::ContainsExtension);
    }
    None
}

/// Um candidato a PIN a partir de 32 bits aleatórios, SEM enviesamento: os
/// valores acima do maior múltiplo de 10⁶ rejeitam-se (`None`) em vez de se
/// reduzirem com o resto da divisão. Quem chama volta a sortear.
pub fn pin_candidate(random: u32) -> Option<String> {
    const SPACE: u32 = 1_000_000;
    const CEILING: u32 = (u32::MAX / SPACE) * SPACE;
    (random < CEILING).then(|| format!("{:06}", random % SPACE))
}

/// O intervalo cabe na forma de um número curto e não está invertido.
pub fn is_valid_range(start: u32, end: u32) -> bool {
    (RANGE_MIN..=RANGE_MAX).contains(&start)
        && (RANGE_MIN..=RANGE_MAX).contains(&end)
        && start <= end
}

/// Os números livres do intervalo, por ordem crescente: fora os ocupados e o
/// número de acesso às reuniões. Um intervalo inválido não dá número nenhum.
pub fn free_numbers<'a>(
    start: u32,
    end: u32,
    taken: &'a HashSet<String>,
    meeting_access_number: &'a str,
) -> impl Iterator<Item = String> + 'a {
    // Inválido: nenhum número (e nunca uma volta por um intervalo gigante).
    let count = if is_valid_range(start, end) {
        (end - start + 1) as usize
    } else {
        0
    };
    (start..=RANGE_MAX)
        .take(count)
        .map(|n| n.to_string())
        .filter(move |n| !taken.contains(n) && n != meeting_access_number)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn um_pin_tem_seis_digitos() {
        for bad in [
            "",
            "12345",
            "1234567",
            "12a456",
            "12 456",
            "１２３４５６",
            "-12345",
        ] {
            assert_eq!(pin_refusal(bad, "1001"), Some(PinRefusal::Format), "{bad}");
        }
        assert_eq!(pin_refusal("482913", "1001"), None);
        assert_eq!(pin_refusal("004829", "1001"), None);
    }

    #[test]
    fn digitos_todos_iguais_sao_recusados() {
        for d in 0..=9 {
            let pin = d.to_string().repeat(6);
            assert_eq!(
                pin_refusal(&pin, "1001"),
                Some(PinRefusal::Repeated),
                "{pin}"
            );
        }
        assert_eq!(pin_refusal("111112", "9000"), None);
    }

    #[test]
    fn sequencias_nos_dois_sentidos_sao_recusadas() {
        for pin in [
            "012345", "123456", "234567", "345678", "456789", "567890", "890123", "987654",
            "654321", "543210", "210987", "098765",
        ] {
            assert_eq!(
                pin_refusal(pin, "7777"),
                Some(PinRefusal::Sequence),
                "{pin}"
            );
        }
        // Quase sequência não é sequência.
        for pin in ["123457", "124578", "135790", "987653"] {
            assert_eq!(pin_refusal(pin, "7777"), None, "{pin}");
        }
    }

    #[test]
    fn o_pin_nao_contem_o_numero_do_ramal() {
        for pin in ["001234", "123400", "912348", "123412"] {
            assert_eq!(
                pin_refusal(pin, "1234"),
                Some(PinRefusal::ContainsExtension),
                "{pin}"
            );
        }
        assert_eq!(pin_refusal("120034", "1234"), None);
        assert_eq!(
            pin_refusal("482101", "101"),
            Some(PinRefusal::ContainsExtension)
        );
        // Sem ramal conhecido não há o que comparar.
        assert_eq!(pin_refusal("001234", ""), None);
    }

    #[test]
    fn cada_recusa_tem_codigo_e_mensagem_proprios() {
        let all = [
            PinRefusal::Format,
            PinRefusal::Repeated,
            PinRefusal::Sequence,
            PinRefusal::ContainsExtension,
        ];
        let codes: HashSet<_> = all.iter().map(|r| r.code()).collect();
        assert_eq!(codes.len(), all.len());
        assert!(all
            .iter()
            .all(|r| r.code().starts_with("ramais.pin_") && !r.message().is_empty()));
    }

    #[test]
    fn o_candidato_tem_seis_digitos_e_rejeita_a_cauda_enviesada() {
        assert_eq!(pin_candidate(0).as_deref(), Some("000000"));
        assert_eq!(pin_candidate(42).as_deref(), Some("000042"));
        assert_eq!(pin_candidate(1_000_000).as_deref(), Some("000000"));
        assert_eq!(pin_candidate(4_293_999_999).as_deref(), Some("999999"));
        // 4 294 000 000 é o primeiro valor da cauda: aceitá-lo favorecia os
        // PIN baixos.
        assert_eq!(pin_candidate(4_294_000_000), None);
        assert_eq!(pin_candidate(u32::MAX), None);
    }

    #[test]
    fn o_intervalo_tem_a_forma_de_numeros_curtos() {
        assert!(is_valid_range(DEFAULT_RANGE_START, DEFAULT_RANGE_END));
        assert!(is_valid_range(100, 100));
        assert!(is_valid_range(100, 99_999));
        for (s, e) in [(99, 200), (0, 10), (2000, 1000), (100, 100_000)] {
            assert!(!is_valid_range(s, e), "{s}-{e}");
        }
    }

    #[test]
    fn os_livres_saltam_ocupados_e_o_numero_de_acesso() {
        let taken: HashSet<String> = ["7998", "8001"].iter().map(|s| s.to_string()).collect();
        let free: Vec<String> = free_numbers(7997, 8003, &taken, "8000").collect();
        assert_eq!(free, ["7997", "7999", "8002", "8003"]);
        assert_eq!(free_numbers(2000, 1000, &taken, "8000").count(), 0);
        assert_eq!(free_numbers(100, 102, &HashSet::new(), "").count(), 3);
    }
}
