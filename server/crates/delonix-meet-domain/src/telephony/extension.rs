//! Números curtos dos ramais internos — a forma, e o número RESERVADO pelo
//! qual um ramal entra numa reunião (R273).
//!
//! Sem IO: quem lê a configuração e a base é `server/src/ramais.rs`.

/// Número de acesso às reuniões quando `VOICE_MEETING_ACCESS_NUMBER` não está
/// definido.
pub const DEFAULT_MEETING_ACCESS_NUMBER: usize = 8000;

/// Limites do número de acesso lido do ambiente: três a cinco dígitos, sem
/// zero à esquerda (é lido como inteiro).
pub const MEETING_ACCESS_NUMBER_MIN: usize = 100;
pub const MEETING_ACCESS_NUMBER_MAX: usize = 99_999;

/// Um número curto: 3 a 5 dígitos ASCII, nada mais. É a mesma forma que o
/// dialplan `delonix_ramais` aceita (`^\d{3,5}$`).
pub fn is_short_number(s: &str) -> bool {
    (3..=5).contains(&s.len()) && s.bytes().all(|b| b.is_ascii_digit())
}

/// O número marcado é o de acesso às reuniões — não pode ser de um ramal, e
/// quem o marca vai para o IVR da sala em vez de tocar em alguém.
pub fn is_meeting_access_number(dialed: &str, meeting_access_number: &str) -> bool {
    !meeting_access_number.is_empty() && dialed == meeting_access_number
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numero_curto_tem_tres_a_cinco_digitos() {
        for ok in ["101", "8000", "12345", "007"] {
            assert!(is_short_number(ok), "{ok}");
        }
        for bad in ["", "12", "123456", "10a", "+101", " 101", "１０１"] {
            assert!(!is_short_number(bad), "{bad}");
        }
    }

    #[test]
    fn so_o_numero_exacto_e_reservado() {
        assert!(is_meeting_access_number("8000", "8000"));
        assert!(!is_meeting_access_number("800", "8000"));
        assert!(!is_meeting_access_number("80000", "8000"));
        assert!(!is_meeting_access_number("8001", "8000"));
        // Sem número configurado nada é reservado — e nada entra por engano.
        assert!(!is_meeting_access_number("", ""));
    }

    #[test]
    fn a_omissao_e_um_numero_curto_dentro_dos_limites() {
        let n = DEFAULT_MEETING_ACCESS_NUMBER;
        assert!((MEETING_ACCESS_NUMBER_MIN..=MEETING_ACCESS_NUMBER_MAX).contains(&n));
        assert!(is_short_number(&n.to_string()));
        assert!(is_short_number(&MEETING_ACCESS_NUMBER_MIN.to_string()));
        assert!(is_short_number(&MEETING_ACCESS_NUMBER_MAX.to_string()));
    }
}
