//! Sessões de emissão de um canal de TV (RFC-0001, §8–9).
//!
//! Separa o que se PEDE do que está a ACONTECER. `desired` é a intenção de
//! quem produz («pôr no ar», «parar»); `state` é o que o EXECUTOR observou. O
//! servidor de controlo só escreve a intenção; só o executor, com o lease
//! válido, faz avançar o estado. Daí que, sem executor, uma sessão fique em
//! `requested` — e a interface nunca mostre «no ar» sem prova (RNF-14).
//!
//! Máquina de estados da sessão:
//!
//! ```text
//! requested → starting → live → ending → ended
//!     │           │        │       │
//!     └───────────┴────────┴───────┴──→ failed
//! requested → ended   (pedido de paragem antes de alguém a ter arrancado)
//! ```

use delonix_meet_core::DomainError;

/// A intenção de quem produz.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Desired {
    Live,
    Stopped,
}

impl Desired {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Live => "live",
            Self::Stopped => "stopped",
        }
    }
}

/// O que o executor observou.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    Requested,
    Starting,
    Live,
    Ending,
    Ended,
    Failed,
}

impl State {
    pub const ALL: [&'static str; 6] =
        ["requested", "starting", "live", "ending", "ended", "failed"];

    pub fn parse(s: &str) -> Result<Self, DomainError> {
        Ok(match s {
            "requested" => Self::Requested,
            "starting" => Self::Starting,
            "live" => Self::Live,
            "ending" => Self::Ending,
            "ended" => Self::Ended,
            "failed" => Self::Failed,
            other => {
                return Err(DomainError::invalid(
                    "tv.broadcast.invalid_state",
                    format!(
                        "estado inválido «{other}» — válidos: {}",
                        Self::ALL.join(", ")
                    ),
                ))
            }
        })
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Requested => "requested",
            Self::Starting => "starting",
            Self::Live => "live",
            Self::Ending => "ending",
            Self::Ended => "ended",
            Self::Failed => "failed",
        }
    }

    /// Terminal: a sessão acabou e liberta o canal.
    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Ended | Self::Failed)
    }

    /// A transição que o EXECUTOR pode fazer. Nunca se sai de um estado
    /// terminal; qualquer estado vivo pode falhar.
    pub fn can_become(self, to: State) -> bool {
        use State::*;
        match (self, to) {
            (Requested, Starting) | (Starting, Live) | (Live, Ending) | (Ending, Ended) => true,
            // Uma paragem pedida enquanto arrancava, ou uma sessão que o
            // executor decide encerrar sem ter chegado a estar no ar.
            (Starting, Ending) | (Requested, Ended) => true,
            (from, Failed) => !from.is_terminal(),
            _ => false,
        }
    }

    /// Os estados de que se pode partir para `to` — o que o SQL do adaptador
    /// usa para a transição ser atómica (`WHERE state = ANY(…)`).
    pub fn sources_of(to: State) -> Vec<&'static str> {
        [
            State::Requested,
            State::Starting,
            State::Live,
            State::Ending,
            State::Ended,
            State::Failed,
        ]
        .into_iter()
        .filter(|from| from.can_become(to))
        .map(State::as_str)
        .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn o_caminho_feliz_avanca_um_passo_de_cada_vez() {
        use State::*;
        for (a, b) in [
            (Requested, Starting),
            (Starting, Live),
            (Live, Ending),
            (Ending, Ended),
        ] {
            assert!(a.can_become(b), "{a:?} → {b:?}");
        }
        // Não se salta etapas: «no ar» exige ter arrancado.
        assert!(!Requested.can_become(Live));
        assert!(!Requested.can_become(Ending));
        assert!(!Live.can_become(Ended), "terminar passa por `ending`");
    }

    #[test]
    fn um_estado_terminal_nao_volta_atras() {
        use State::*;
        for t in [Ended, Failed] {
            assert!(t.is_terminal());
            for to in [Requested, Starting, Live, Ending, Ended, Failed] {
                assert!(!t.can_become(to), "{t:?} → {to:?} não devia ser possível");
            }
        }
    }

    #[test]
    fn qualquer_estado_vivo_pode_falhar() {
        use State::*;
        for from in [Requested, Starting, Live, Ending] {
            assert!(from.can_become(Failed), "{from:?}");
        }
    }

    #[test]
    fn as_origens_batem_com_as_transicoes() {
        assert_eq!(State::sources_of(State::Live), ["starting"]);
        assert_eq!(State::sources_of(State::Starting), ["requested"]);
        let mut f = State::sources_of(State::Failed);
        f.sort();
        assert_eq!(f, ["ending", "live", "requested", "starting"]);
        assert!(State::sources_of(State::Requested).is_empty());
    }

    #[test]
    fn o_estado_recusa_o_desconhecido_e_faz_round_trip() {
        for s in State::ALL {
            assert_eq!(State::parse(s).unwrap().as_str(), s);
        }
        assert_eq!(
            State::parse("on_air").unwrap_err().code,
            "tv.broadcast.invalid_state"
        );
        assert_eq!(Desired::Live.as_str(), "live");
        assert_eq!(Desired::Stopped.as_str(), "stopped");
    }
}
