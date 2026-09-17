//! Resiliência de um destino de emissão em directo (ADR-0013 §3).
//!
//! A máquina de estados de UM destino, sem IO e sem relógio: recebe o que o
//! supervisor observou (bytes a sair, atraso na fila, processo que morreu,
//! backoff cumprido, pedido da pessoa) e devolve o estado seguinte e os
//! efeitos a executar (lançar, matar, agendar, registar). O jitter entra como
//! argumento — é o que deixa a tabela de transições ser testada linha a linha.
//!
//! O que esta máquina NÃO faz: envio em diferido. Um destino reposto retoma no
//! presente; os segundos perdidos ficam na gravação do servidor (ADR-0013 §4).

use std::time::Duration;

use delonix_meet_core::DomainError;

/// O estado de um destino, como vai no fio (`as_str`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputState {
    /// Processo lançado; ainda não saiu nenhum byte.
    Connecting,
    /// O ffmpeg reporta bytes a sair.
    Live,
    /// No ar, mas a fila acumula atraso acima do limiar.
    Degraded,
    /// Caiu; espera o backoff antes da tentativa `next_attempt`.
    Interrupted { next_attempt: u32 },
    /// Tentativa `attempt` em curso.
    Retrying { attempt: u32 },
    /// Esgotou as tentativas; só volta com «reconectar à mão».
    Lost,
    /// Parado pela pessoa ou porque a emissão terminou.
    Stopped,
}

impl OutputState {
    pub const ALL: [&'static str; 7] = [
        "connecting",
        "live",
        "degraded",
        "interrupted",
        "retrying",
        "lost",
        "stopped",
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Connecting => "connecting",
            Self::Live => "live",
            Self::Degraded => "degraded",
            Self::Interrupted { .. } => "interrupted",
            Self::Retrying { .. } => "retrying",
            Self::Lost => "lost",
            Self::Stopped => "stopped",
        }
    }

    /// Há media a sair para o destino.
    pub fn on_air(self) -> bool {
        matches!(self, Self::Live | Self::Degraded)
    }

    /// O número da tentativa em curso ou da próxima (para «tentativa 3 de 8»).
    pub fn attempt(self) -> Option<u32> {
        match self {
            Self::Interrupted { next_attempt } => Some(next_attempt),
            Self::Retrying { attempt } => Some(attempt),
            _ => None,
        }
    }
}

/// Porque é que um processo de saída deixou de servir.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailureCause {
    /// O processo terminou.
    Exited,
    /// Não pôs bytes a sair dentro do prazo de arranque.
    Stalled,
    /// A fila do destino encheu: a saída não acompanha.
    Overflow,
    /// O processo nem arrancou.
    SpawnFailed,
}

/// O que o supervisor observou, ou o que a pessoa pediu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Input {
    BytesFlowing,
    /// Atraso acumulado na fila do destino, em milissegundos de media.
    Backlog {
        backlog_ms: u64,
    },
    Failed(FailureCause),
    BackoffElapsed,
    /// Esteve no ar sem cair durante `stable_after`: as quedas voltam a zero.
    Stable,
    /// «Repor agora» (num destino a aguardar) ou «reconectar à mão» (perdido).
    Reconnect,
    /// O perfil mudou (ex.: 1080p): reinicia sem contar como queda.
    ProfileChanged,
    Stop,
}

/// Um efeito que o supervisor executa. A ordem conta.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Effect {
    Spawn,
    Kill,
    ScheduleRetry(Duration),
    CancelRetry,
    Log(EventCode),
}

/// Códigos estáveis dos eventos que um destino produz no registo.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventCode {
    DestinationLive,
    DestinationRecovered { attempt: u32 },
    DestinationDegraded,
    DestinationBacklogCleared,
    DestinationInterrupted,
    RetryStarted { attempt: u32, max: u32 },
    RetryFailed { attempt: u32, retry_in: Duration },
    DestinationLost { attempts: u32 },
    ManualReconnect,
    ProfileChanged,
    DestinationStopped,
}

impl EventCode {
    pub fn code(self) -> &'static str {
        match self {
            Self::DestinationLive => "destination.live",
            Self::DestinationRecovered { .. } => "destination.recovered",
            Self::DestinationDegraded => "destination.degraded",
            Self::DestinationBacklogCleared => "destination.backlog_cleared",
            Self::DestinationInterrupted => "destination.interrupted",
            Self::RetryStarted { .. } => "destination.retry_started",
            Self::RetryFailed { .. } => "destination.retry_failed",
            Self::DestinationLost { .. } => "destination.lost",
            Self::ManualReconnect => "destination.manual_reconnect",
            Self::ProfileChanged => "destination.profile_changed",
            Self::DestinationStopped => "destination.stopped",
        }
    }

    pub fn kind(self) -> EventKind {
        match self {
            Self::DestinationLive
            | Self::DestinationRecovered { .. }
            | Self::DestinationBacklogCleared
            | Self::ProfileChanged
            | Self::DestinationStopped => EventKind::Normal,
            Self::DestinationDegraded => EventKind::Warning,
            Self::DestinationInterrupted | Self::DestinationLost { .. } => EventKind::Failure,
            Self::RetryStarted { .. } | Self::RetryFailed { .. } | Self::ManualReconnect => {
                EventKind::Recovering
            }
        }
    }
}

/// A família de um evento no registo cronológico (ADR-0013 §9).
///
/// `now` não é uma família gravada: atribui-se na leitura ao último evento
/// `recovering` de um destino que ainda está a tentar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventKind {
    Normal,
    Warning,
    Failure,
    Safe,
    Recovering,
}

impl EventKind {
    pub const ALL: [&'static str; 5] = ["normal", "warning", "failure", "safe", "recovering"];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Normal => "normal",
            Self::Warning => "warning",
            Self::Failure => "failure",
            Self::Safe => "safe",
            Self::Recovering => "recovering",
        }
    }
}

/// Perfil de saída de um destino.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Profile {
    /// O vídeo do browser, copiado (ADR-0003).
    Source,
    /// Transcodificado para 1080p (ADR-0013 §7): custa mais de um core.
    P1080,
}

impl Profile {
    pub const ALL: [&'static str; 2] = ["source", "1080p"];

    pub fn parse(s: &str) -> Result<Self, DomainError> {
        match s {
            "source" => Ok(Self::Source),
            "1080p" => Ok(Self::P1080),
            other => Err(DomainError::invalid(
                "broadcast.invalid_profile",
                format!(
                    "perfil de saída inválido «{other}» — válidos: {}",
                    Self::ALL.join(", ")
                ),
            )
            .with_field("profile", Self::ALL.join(" | "))),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Source => "source",
            Self::P1080 => "1080p",
        }
    }
}

/// Tentativas e limiares. Os valores por omissão são os do ADR-0013 §3.
#[derive(Debug, Clone)]
pub struct RetryPolicy {
    pub max_attempts: u32,
    pub initial_backoff: Duration,
    pub max_backoff: Duration,
    /// Fracção máxima de variação aleatória do backoff (0,2 = ±20 %).
    pub jitter: f64,
    /// Atraso na fila a partir do qual um destino no ar fica `degraded`.
    pub degraded_backlog_ms: u64,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_attempts: 8,
            initial_backoff: Duration::from_secs(2),
            max_backoff: Duration::from_secs(15),
            jitter: 0.2,
            degraded_backlog_ms: 2_000,
        }
    }
}

impl RetryPolicy {
    /// Espera antes da tentativa `attempt` (1-based):
    /// `min(inicial × 2^(n-1), tecto) × (1 + jitter × amostra)`, com a amostra
    /// em `[-1, 1]`. O jitter nunca leva a espera acima do tecto × (1 + jitter)
    /// nem abaixo de zero.
    pub fn backoff(&self, attempt: u32, sample: f64) -> Duration {
        let exp = attempt.saturating_sub(1).min(20);
        let base = self
            .initial_backoff
            .saturating_mul(1u32 << exp)
            .min(self.max_backoff);
        let jitter = self.jitter.clamp(0.0, 0.5);
        let factor = 1.0 + jitter * sample.clamp(-1.0, 1.0);
        base.mul_f64(factor.max(0.0))
    }
}

/// A máquina de UM destino.
#[derive(Debug, Clone)]
pub struct OutputMachine {
    pub state: OutputState,
    /// Quedas desde a última vez que esteve estável no ar.
    pub failures: u32,
}

/// O resultado de um passo.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Transition {
    pub from: OutputState,
    pub to: OutputState,
    pub effects: Vec<Effect>,
}

impl Default for OutputMachine {
    fn default() -> Self {
        Self::new()
    }
}

impl OutputMachine {
    /// Um destino acabado de lançar.
    pub fn new() -> Self {
        Self {
            state: OutputState::Connecting,
            failures: 0,
        }
    }

    /// Aplica uma entrada. `sample` ∈ `[-1, 1]` é a amostra de jitter.
    ///
    /// Uma entrada que não se aplica ao estado (um backoff que chega depois
    /// de a pessoa ter parado, um atraso num destino que não está no ar) é
    /// ignorada. Só o `Reconnect` recusa, porque vem de uma pessoa que tem de
    /// saber porquê.
    pub fn step(
        &mut self,
        input: Input,
        policy: &RetryPolicy,
        sample: f64,
    ) -> Result<Transition, DomainError> {
        use OutputState as S;
        let from = self.state;
        let mut effects = Vec::new();
        let to = match (from, input) {
            (S::Stopped, _) => {
                if input == Input::Reconnect {
                    return Err(DomainError::conflict(
                        "broadcast.destination_stopped",
                        "este destino foi parado; volte a emitir para o usar",
                    ));
                }
                from
            }
            (_, Input::Stop) => {
                effects.push(Effect::CancelRetry);
                effects.push(Effect::Kill);
                effects.push(Effect::Log(EventCode::DestinationStopped));
                S::Stopped
            }

            // ---- a sair ----
            (S::Connecting, Input::BytesFlowing) => {
                effects.push(Effect::Log(EventCode::DestinationLive));
                S::Live
            }
            (S::Retrying { attempt }, Input::BytesFlowing) => {
                effects.push(Effect::Log(EventCode::DestinationRecovered { attempt }));
                S::Live
            }
            (S::Live, Input::Backlog { backlog_ms })
                if backlog_ms >= policy.degraded_backlog_ms =>
            {
                effects.push(Effect::Log(EventCode::DestinationDegraded));
                S::Degraded
            }
            (S::Degraded, Input::Backlog { backlog_ms })
                if backlog_ms <= policy.degraded_backlog_ms / 2 =>
            {
                effects.push(Effect::Log(EventCode::DestinationBacklogCleared));
                S::Live
            }
            (S::Live | S::Degraded, Input::Stable) => {
                self.failures = 0;
                from
            }

            // ---- a cair ----
            (S::Connecting | S::Live | S::Degraded | S::Retrying { .. }, Input::Failed(_)) => {
                self.failures += 1;
                effects.push(Effect::Kill);
                if self.failures > policy.max_attempts {
                    effects.push(Effect::Log(EventCode::DestinationLost {
                        attempts: policy.max_attempts,
                    }));
                    S::Lost
                } else {
                    let wait = policy.backoff(self.failures, sample);
                    effects.push(Effect::Log(match from {
                        S::Retrying { attempt } => EventCode::RetryFailed {
                            attempt,
                            retry_in: wait,
                        },
                        _ => EventCode::DestinationInterrupted,
                    }));
                    effects.push(Effect::ScheduleRetry(wait));
                    S::Interrupted {
                        next_attempt: self.failures,
                    }
                }
            }
            (S::Interrupted { next_attempt }, Input::BackoffElapsed) => {
                effects.push(Effect::Spawn);
                effects.push(Effect::Log(EventCode::RetryStarted {
                    attempt: next_attempt,
                    max: policy.max_attempts,
                }));
                S::Retrying {
                    attempt: next_attempt,
                }
            }

            // ---- a pessoa ----
            (S::Interrupted { next_attempt }, Input::Reconnect) => {
                effects.push(Effect::CancelRetry);
                effects.push(Effect::Spawn);
                effects.push(Effect::Log(EventCode::RetryStarted {
                    attempt: next_attempt,
                    max: policy.max_attempts,
                }));
                S::Retrying {
                    attempt: next_attempt,
                }
            }
            (S::Lost, Input::Reconnect) => {
                self.failures = 1;
                effects.push(Effect::Spawn);
                effects.push(Effect::Log(EventCode::ManualReconnect));
                S::Retrying { attempt: 1 }
            }
            (S::Connecting | S::Retrying { .. }, Input::Reconnect) => {
                return Err(DomainError::conflict(
                    "broadcast.retry_in_progress",
                    "já há uma tentativa de ligação a decorrer neste destino",
                ))
            }
            (S::Live | S::Degraded, Input::Reconnect) => {
                return Err(DomainError::conflict(
                    "broadcast.destination_live",
                    "este destino está no ar; não há nada a repor",
                ))
            }
            (S::Live | S::Degraded, Input::ProfileChanged) => {
                effects.push(Effect::Kill);
                effects.push(Effect::Spawn);
                effects.push(Effect::Log(EventCode::ProfileChanged));
                S::Connecting
            }
            // Fora do ar, o perfil novo vale para o próximo lançamento.
            (_, Input::ProfileChanged) => {
                effects.push(Effect::Log(EventCode::ProfileChanged));
                from
            }

            // Tudo o resto não se aplica ao estado: ignora-se.
            _ => from,
        };
        self.state = to;
        Ok(Transition { from, to, effects })
    }
}

/// A adaptação automática reduz o débito? Só se o destino estiver degradado
/// há tempo suficiente, ainda no perfil de origem, e houver orçamento para
/// transcodificar (ADR-0013 §6).
pub fn should_adapt(
    state: OutputState,
    profile: Profile,
    degraded_for: Duration,
    adapt_after: Duration,
    transcode_available: bool,
) -> bool {
    state == OutputState::Degraded
        && profile == Profile::Source
        && degraded_for >= adapt_after
        && transcode_available
}

#[cfg(test)]
mod tests {
    use super::*;
    use Effect as E;
    use Input as I;
    use OutputState as S;

    fn policy() -> RetryPolicy {
        RetryPolicy {
            max_attempts: 3,
            initial_backoff: Duration::from_secs(2),
            max_backoff: Duration::from_secs(15),
            jitter: 0.2,
            degraded_backlog_ms: 2_000,
        }
    }

    fn at(state: S, failures: u32) -> OutputMachine {
        OutputMachine { state, failures }
    }

    /// A tabela do ADR-0013 §3, linha a linha: (estado, quedas, entrada) →
    /// (estado seguinte, efeitos). A amostra de jitter é 0 — o backoff é o base.
    #[test]
    fn tabela_de_transicoes() {
        let p = policy();
        let s2 = Duration::from_secs(2);
        let s4 = Duration::from_secs(4);
        #[rustfmt::skip]
        let casos: Vec<(S, u32, I, S, Vec<Effect>)> = vec![
            (S::Connecting, 0, I::BytesFlowing, S::Live, vec![E::Log(EventCode::DestinationLive)]),
            (S::Live, 0, I::Backlog { backlog_ms: 2_000 }, S::Degraded, vec![E::Log(EventCode::DestinationDegraded)]),
            (S::Live, 0, I::Backlog { backlog_ms: 1_999 }, S::Live, vec![]),
            (S::Degraded, 0, I::Backlog { backlog_ms: 1_500 }, S::Degraded, vec![]),
            (S::Degraded, 0, I::Backlog { backlog_ms: 1_000 }, S::Live, vec![E::Log(EventCode::DestinationBacklogCleared)]),
            (S::Live, 0, I::Failed(FailureCause::Exited), S::Interrupted { next_attempt: 1 },
                vec![E::Kill, E::Log(EventCode::DestinationInterrupted), E::ScheduleRetry(s2)]),
            (S::Degraded, 0, I::Failed(FailureCause::Overflow), S::Interrupted { next_attempt: 1 },
                vec![E::Kill, E::Log(EventCode::DestinationInterrupted), E::ScheduleRetry(s2)]),
            (S::Connecting, 0, I::Failed(FailureCause::Stalled), S::Interrupted { next_attempt: 1 },
                vec![E::Kill, E::Log(EventCode::DestinationInterrupted), E::ScheduleRetry(s2)]),
            (S::Interrupted { next_attempt: 1 }, 1, I::BackoffElapsed, S::Retrying { attempt: 1 },
                vec![E::Spawn, E::Log(EventCode::RetryStarted { attempt: 1, max: 3 })]),
            (S::Retrying { attempt: 1 }, 1, I::Failed(FailureCause::Exited), S::Interrupted { next_attempt: 2 },
                vec![E::Kill, E::Log(EventCode::RetryFailed { attempt: 1, retry_in: s4 }), E::ScheduleRetry(s4)]),
            (S::Retrying { attempt: 2 }, 2, I::BytesFlowing, S::Live,
                vec![E::Log(EventCode::DestinationRecovered { attempt: 2 })]),
            (S::Retrying { attempt: 3 }, 3, I::Failed(FailureCause::SpawnFailed), S::Lost,
                vec![E::Kill, E::Log(EventCode::DestinationLost { attempts: 3 })]),
            (S::Interrupted { next_attempt: 2 }, 2, I::Reconnect, S::Retrying { attempt: 2 },
                vec![E::CancelRetry, E::Spawn, E::Log(EventCode::RetryStarted { attempt: 2, max: 3 })]),
            (S::Lost, 4, I::Reconnect, S::Retrying { attempt: 1 },
                vec![E::Spawn, E::Log(EventCode::ManualReconnect)]),
            (S::Live, 0, I::ProfileChanged, S::Connecting,
                vec![E::Kill, E::Spawn, E::Log(EventCode::ProfileChanged)]),
            (S::Interrupted { next_attempt: 1 }, 1, I::ProfileChanged, S::Interrupted { next_attempt: 1 },
                vec![E::Log(EventCode::ProfileChanged)]),
            (S::Retrying { attempt: 1 }, 1, I::Stop, S::Stopped,
                vec![E::CancelRetry, E::Kill, E::Log(EventCode::DestinationStopped)]),
            (S::Live, 0, I::Stop, S::Stopped,
                vec![E::CancelRetry, E::Kill, E::Log(EventCode::DestinationStopped)]),
            // Entradas que já não se aplicam: ignoradas sem efeitos.
            (S::Stopped, 0, I::BackoffElapsed, S::Stopped, vec![]),
            (S::Stopped, 0, I::Failed(FailureCause::Exited), S::Stopped, vec![]),
            (S::Live, 0, I::BackoffElapsed, S::Live, vec![]),
            (S::Lost, 4, I::BackoffElapsed, S::Lost, vec![]),
            (S::Lost, 4, I::Failed(FailureCause::Exited), S::Lost, vec![]),
            (S::Interrupted { next_attempt: 1 }, 1, I::Failed(FailureCause::Exited), S::Interrupted { next_attempt: 1 }, vec![]),
        ];
        for (i, (de, quedas, entrada, para, efeitos)) in casos.into_iter().enumerate() {
            let mut m = at(de, quedas);
            let t = m
                .step(entrada, &p, 0.0)
                .unwrap_or_else(|e| panic!("linha {i}: {de:?} + {entrada:?} recusou: {e:?}"));
            assert_eq!(t.to, para, "linha {i}: {de:?} + {entrada:?}");
            assert_eq!(t.effects, efeitos, "linha {i}: {de:?} + {entrada:?}");
            assert_eq!(m.state, para, "linha {i}: o estado guardado");
        }
    }

    #[test]
    fn o_reconnect_recusa_com_codigo_estavel_onde_nao_se_aplica() {
        let p = policy();
        for (estado, codigo) in [
            (S::Live, "broadcast.destination_live"),
            (S::Degraded, "broadcast.destination_live"),
            (S::Connecting, "broadcast.retry_in_progress"),
            (S::Retrying { attempt: 2 }, "broadcast.retry_in_progress"),
            (S::Stopped, "broadcast.destination_stopped"),
        ] {
            let mut m = at(estado, 0);
            let e = m.step(I::Reconnect, &p, 0.0).unwrap_err();
            assert_eq!(e.code, codigo, "{estado:?}");
            assert_eq!(m.state, estado, "uma recusa não muda o estado");
        }
    }

    #[test]
    fn o_limite_de_tentativas_leva_a_perdido_e_a_mao_repoe_o_contador() {
        let p = policy();
        let mut m = OutputMachine::new();
        m.step(I::BytesFlowing, &p, 0.0).unwrap();
        let mut visitados = vec![m.state.as_str()];
        // Cai, e cada tentativa falha até esgotar as 3.
        m.step(I::Failed(FailureCause::Exited), &p, 0.0).unwrap();
        for n in 1..=3 {
            assert_eq!(m.state, S::Interrupted { next_attempt: n });
            visitados.push(m.state.as_str());
            m.step(I::BackoffElapsed, &p, 0.0).unwrap();
            assert_eq!(m.state, S::Retrying { attempt: n });
            m.step(I::Failed(FailureCause::Exited), &p, 0.0).unwrap();
        }
        assert_eq!(m.state, S::Lost, "à quarta queda com N=3 é perdido");
        // Um backoff atrasado não o ressuscita.
        m.step(I::BackoffElapsed, &p, 0.0).unwrap();
        assert_eq!(m.state, S::Lost);
        // A mão: tentativa 1 de novo, e a próxima queda volta a ter N tentativas.
        m.step(I::Reconnect, &p, 0.0).unwrap();
        assert_eq!(m.state, S::Retrying { attempt: 1 });
        m.step(I::Failed(FailureCause::Exited), &p, 0.0).unwrap();
        assert_eq!(m.state, S::Interrupted { next_attempt: 2 });
        assert_eq!(visitados[0], "live");
    }

    #[test]
    fn um_destino_que_pisca_nao_ganha_tentativas_infinitas() {
        // Liga e cai logo a seguir, sem nunca ficar estável: as quedas somam.
        let p = policy();
        let mut m = OutputMachine::new();
        for _ in 0..3 {
            m.step(I::BytesFlowing, &p, 0.0).unwrap();
            m.step(I::Failed(FailureCause::Exited), &p, 0.0).unwrap();
            m.step(I::BackoffElapsed, &p, 0.0).unwrap();
        }
        m.step(I::BytesFlowing, &p, 0.0).unwrap();
        m.step(I::Failed(FailureCause::Exited), &p, 0.0).unwrap();
        assert_eq!(m.state, S::Lost);
    }

    #[test]
    fn estar_estavel_no_ar_devolve_as_tentativas() {
        let p = policy();
        let mut m = OutputMachine::new();
        for _ in 0..3 {
            m.step(I::BytesFlowing, &p, 0.0).unwrap();
            m.step(I::Stable, &p, 0.0).unwrap();
            m.step(I::Failed(FailureCause::Exited), &p, 0.0).unwrap();
            assert_eq!(m.state, S::Interrupted { next_attempt: 1 });
            m.step(I::BackoffElapsed, &p, 0.0).unwrap();
        }
    }

    #[test]
    fn backoff_exponencial_com_tecto_e_jitter_limitado() {
        let p = RetryPolicy::default();
        let secs = |n, s| p.backoff(n, s).as_secs_f64();
        assert_eq!(secs(1, 0.0), 2.0);
        assert_eq!(secs(2, 0.0), 4.0);
        assert_eq!(secs(3, 0.0), 8.0);
        assert_eq!(secs(4, 0.0), 15.0, "tecto");
        assert_eq!(secs(30, 0.0), 15.0, "sem overflow");
        assert!((secs(4, 1.0) - 18.0).abs() < 1e-9, "+20 %");
        assert!((secs(4, -1.0) - 12.0).abs() < 1e-9, "-20 %");
        assert!(
            (secs(4, 7.0) - 18.0).abs() < 1e-9,
            "amostra fora de [-1,1] limitada"
        );
        let louco = RetryPolicy {
            jitter: 3.0,
            ..RetryPolicy::default()
        };
        assert!(
            louco.backoff(1, -1.0) >= Duration::from_secs(1),
            "jitter limitado a 50 %"
        );
    }

    #[test]
    fn a_adaptacao_so_age_com_orcamento_e_degradacao_sustentada() {
        let dez = Duration::from_secs(10);
        assert!(should_adapt(S::Degraded, Profile::Source, dez, dez, true));
        assert!(!should_adapt(S::Degraded, Profile::Source, dez, dez, false));
        assert!(!should_adapt(S::Degraded, Profile::P1080, dez, dez, true));
        assert!(!should_adapt(S::Live, Profile::Source, dez, dez, true));
        assert!(!should_adapt(
            S::Degraded,
            Profile::Source,
            Duration::from_secs(9),
            dez,
            true
        ));
    }

    #[test]
    fn perfis_e_estados_no_fio() {
        assert_eq!(Profile::parse("1080p").unwrap(), Profile::P1080);
        assert_eq!(
            Profile::parse("720p").unwrap_err().code,
            "broadcast.invalid_profile"
        );
        for s in [
            S::Connecting,
            S::Live,
            S::Degraded,
            S::Interrupted { next_attempt: 1 },
            S::Retrying { attempt: 1 },
            S::Lost,
            S::Stopped,
        ] {
            assert!(S::ALL.contains(&s.as_str()));
        }
        assert_eq!(
            EventCode::DestinationLost { attempts: 8 }.kind(),
            EventKind::Failure
        );
        assert_eq!(
            EventCode::RetryStarted { attempt: 1, max: 8 }.kind(),
            EventKind::Recovering
        );
    }
}
