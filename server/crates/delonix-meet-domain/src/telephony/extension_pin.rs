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

/// Falhas, dentro da janela, que bloqueiam o PIN de um ramal.
pub const MAX_FAILED_ATTEMPTS: i32 = 5;

/// Quanto tempo o PIN fica bloqueado no PRIMEIRO bloqueio. Os seguintes
/// dobram (ver [`Throttle`]).
pub const LOCK_SECS: i64 = 15 * 60;

/// O travão de um ramal: cinco falhas em quinze minutos bloqueiam; o
/// bloqueio começa nos quinze minutos e dobra a cada reincidência, até um dia.
pub const EXTENSION_THROTTLE: Throttle = Throttle {
    max_failures: MAX_FAILED_ATTEMPTS,
    window_secs: 15 * 60,
    base_lock_secs: LOCK_SECS,
    max_lock_secs: 24 * 3600,
    level_decay_secs: 24 * 3600,
};

/// O travão de uma ORIGEM (quem liga: número e rede de onde a chamada vem).
/// Trava à TERCEIRA falha — antes de a mesma origem chegar às cinco de um
/// ramal: quem experimenta PIN de fora não consegue bloquear o ramal de um
/// colega, nem um, nem vários (R279). Conta todas as recusas da origem,
/// sejam em que ramal forem.
///
/// O primeiro bloqueio dura VINTE minutos, mais que a janela do ramal
/// (quinze): quando a origem volta a poder tentar, as falhas que deixou no
/// ramal já saíram da janela dele, com folga — ver as asserções abaixo.
pub const ORIGIN_THROTTLE: Throttle = Throttle {
    max_failures: 3,
    window_secs: 15 * 60,
    base_lock_secs: 20 * 60,
    max_lock_secs: 24 * 3600,
    level_decay_secs: 24 * 3600,
};

/// Uma política de travão: falhas contadas numa janela, e um bloqueio que
/// cresce com as reincidências.
///
/// - **Janela.** Uma falha com `window_secs` ou mais (contados desde a
///   primeira da janela) já não conta: quatro enganos em Janeiro e um em
///   Março não bloqueiam.
/// - **Duração crescente.** O bloqueio de nível `n` dura
///   `base_lock_secs · 2ⁿ⁻¹`, com tecto em `max_lock_secs`.
/// - **Esquecimento.** Passados `level_decay_secs` desde o fim do último
///   bloqueio sem nenhum outro, o nível volta a zero.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Throttle {
    pub max_failures: i32,
    pub window_secs: i64,
    pub base_lock_secs: i64,
    pub max_lock_secs: i64,
    pub level_decay_secs: i64,
}

/// O estado guardado de um contador, visto AGORA.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Counter {
    /// Falhas contadas na janela em curso.
    pub failures: i32,
    /// Há quantos segundos começou a janela em curso (`None`: nenhuma).
    pub window_age_secs: Option<i64>,
    /// Quantos bloqueios seguidos já houve (0: nenhum).
    pub lock_level: i32,
    /// Há quantos segundos ACABOU o último bloqueio (negativo: ainda dura;
    /// `None`: nunca houve).
    pub lock_ended_secs_ago: Option<i64>,
}

/// O que gravar depois de uma falha.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AfterFailure {
    /// O contador a gravar (0 quando esta falha bloqueou).
    pub failures: i32,
    /// A janela recomeça agora (esta é a primeira falha dela).
    pub restart_window: bool,
    /// O nível a gravar.
    pub lock_level: i32,
    /// `Some(segundos)`: esta falha bloqueia, durante tanto tempo.
    pub lock_secs: Option<i64>,
    /// A falha que esta é, na janela (para a auditoria: «tentativa 3 de 5»).
    pub attempt: i32,
}

// A regra que impede a negação de serviço a um colega, verificada AO COMPILAR:
// uma origem sozinha nunca junta, num ramal, as falhas que o bloqueiam — e o
// bloqueio da origem não é mais curto que a janela do ramal, por isso o
// segundo lote de falhas dela já não encontra o primeiro.
const _: () = assert!(ORIGIN_THROTTLE.max_failures < EXTENSION_THROTTLE.max_failures);
// COM FOLGA: as idades chegam da base em segundos inteiros, e «igual» deixava
// um segundo em que as duas contas se sobrepunham (3 falhas + 2 = o ramal de
// um colega bloqueado por uma só origem).
const _: () = assert!(
    ORIGIN_THROTTLE.base_lock_secs >= EXTENSION_THROTTLE.window_secs + ORIGIN_LOCK_MARGIN_SECS
);

/// Folga mínima entre o fim da janela do ramal e o fim do bloqueio da origem.
pub const ORIGIN_LOCK_MARGIN_SECS: i64 = 60;

impl Throttle {
    /// A duração do bloqueio de nível `level` (1 = o primeiro).
    pub fn lock_secs(&self, level: i32) -> i64 {
        let doublings = level.saturating_sub(1).clamp(0, 30) as u32;
        self.base_lock_secs
            .saturating_mul(1_i64 << doublings)
            .min(self.max_lock_secs)
    }

    /// Ainda bloqueado? Devolve os segundos que faltam.
    pub fn locked_for(&self, c: &Counter) -> Option<i64> {
        c.lock_ended_secs_ago.filter(|s| *s < 0).map(|s| -s)
    }

    /// Mais uma falha, num contador que NÃO está bloqueado.
    pub fn after_failure(&self, c: &Counter) -> AfterFailure {
        // Estrita: a idade vem em segundos inteiros (FLOOR), e `<=` fazia a
        // janela durar um segundo a mais do que diz.
        let in_window = c.window_age_secs.is_some_and(|age| age < self.window_secs);
        let attempt = if in_window { c.failures + 1 } else { 1 };
        let level = match c.lock_ended_secs_ago {
            Some(ago) if ago > self.level_decay_secs => 0,
            _ => c.lock_level.max(0),
        };
        if attempt >= self.max_failures {
            let next = level + 1;
            return AfterFailure {
                // Ao bloquear o contador volta a zero: passado o bloqueio, são
                // outra vez `max_failures` tentativas — não uma.
                failures: 0,
                restart_window: false,
                lock_level: next,
                lock_secs: Some(self.lock_secs(next)),
                attempt,
            };
        }
        AfterFailure {
            failures: attempt,
            restart_window: !in_window,
            lock_level: level,
            lock_secs: None,
            attempt,
        }
    }
}

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
    fn o_bloqueio_dobra_ate_ao_tecto() {
        let t = EXTENSION_THROTTLE;
        assert_eq!(t.lock_secs(1), 15 * 60);
        assert_eq!(t.lock_secs(2), 30 * 60);
        assert_eq!(t.lock_secs(3), 60 * 60);
        assert_eq!(t.lock_secs(7), 16 * 60 * 60);
        assert_eq!(t.lock_secs(8), 24 * 3600, "tecto de um dia");
        assert_eq!(t.lock_secs(1000), 24 * 3600, "sem transbordo");
    }

    #[test]
    fn a_quinta_falha_na_janela_bloqueia_e_o_nivel_sobe() {
        let t = EXTENSION_THROTTLE;
        let mut c = Counter::default();
        for n in 1..=4 {
            let a = t.after_failure(&c);
            assert_eq!(a.lock_secs, None, "falha {n}");
            assert_eq!(a.attempt, n);
            assert_eq!(a.restart_window, n == 1);
            c.failures = a.failures;
            c.window_age_secs = Some(60);
        }
        let quinta = t.after_failure(&c);
        assert_eq!(quinta.lock_secs, Some(15 * 60));
        assert_eq!(quinta.lock_level, 1);
        assert_eq!(quinta.failures, 0);
        // Reincidência logo a seguir ao fim do bloqueio: o nível sobe e o
        // bloqueio dobra.
        let c = Counter {
            failures: 4,
            window_age_secs: Some(60),
            lock_level: 1,
            lock_ended_secs_ago: Some(120),
        };
        let a = t.after_failure(&c);
        assert_eq!((a.lock_level, a.lock_secs), (2, Some(30 * 60)));
    }

    #[test]
    fn a_janela_expira_e_a_falha_velha_nao_conta() {
        let t = EXTENSION_THROTTLE;
        // Quatro falhas há mais de quinze minutos: a quinta não bloqueia —
        // abre uma janela nova e é a primeira dela.
        let c = Counter {
            failures: 4,
            window_age_secs: Some(15 * 60 + 1),
            ..Counter::default()
        };
        let a = t.after_failure(&c);
        assert_eq!(a.lock_secs, None);
        assert_eq!((a.failures, a.attempt, a.restart_window), (1, 1, true));
    }

    #[test]
    fn a_janela_e_estrita_no_segundo_exacto() {
        // As idades vêm da base em segundos inteiros (FLOOR): 900 quer dizer
        // «entre 900 e 901». Uma janela de 900 s que ainda contasse aos 900
        // durava 901 — o segundo que deixava uma origem acabada de sair do
        // bloqueio juntar as suas falhas novas às velhas do ramal.
        for t in [EXTENSION_THROTTLE, ORIGIN_THROTTLE] {
            let c = Counter {
                failures: t.max_failures - 1,
                window_age_secs: Some(t.window_secs),
                ..Counter::default()
            };
            let a = t.after_failure(&c);
            assert_eq!(a.lock_secs, None);
            assert_eq!((a.attempt, a.restart_window), (1, true));
            let dentro = Counter {
                window_age_secs: Some(t.window_secs - 1),
                ..c
            };
            assert!(t.after_failure(&dentro).lock_secs.is_some());
        }
    }

    #[test]
    fn o_nivel_esquece_se_um_dia_depois_do_ultimo_bloqueio() {
        let t = EXTENSION_THROTTLE;
        let c = Counter {
            failures: 4,
            window_age_secs: Some(10),
            lock_level: 5,
            lock_ended_secs_ago: Some(24 * 3600 + 1),
        };
        assert_eq!(t.after_failure(&c).lock_level, 1, "recomeça do primeiro");
        assert_eq!(t.after_failure(&c).lock_secs, Some(15 * 60));
    }

    #[test]
    fn bloqueado_diz_quanto_falta() {
        let t = ORIGIN_THROTTLE;
        let c = Counter {
            lock_ended_secs_ago: Some(-42),
            ..Counter::default()
        };
        assert_eq!(t.locked_for(&c), Some(42));
        assert_eq!(t.locked_for(&Counter::default()), None);
        let fim = Counter {
            lock_ended_secs_ago: Some(0),
            ..Counter::default()
        };
        assert_eq!(t.locked_for(&fim), None);
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
