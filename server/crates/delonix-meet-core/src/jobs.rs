//! Fila de trabalho durável — **a parte PURA, sem SQL** (trabalho nº2 do
//! `docs/levantamento-2026-10-07-trabalho-assincrono.md`; desenho em
//! `docs/desenho-2026-10-07-uma-fila-so.md`).
//!
//! PORQUE EXISTE: a 2026-10-07 havia **sete** reivindicações de trabalho no
//! servidor, todas com a mesma forma de SQL
//! (`… FOR UPDATE SKIP LOCKED` dentro de um `UPDATE`) e **zero linhas
//! partilhadas**. Cada coluna do que uma fila precisa tinha um dono que a fez
//! bem e seis que não a tinham: o backoff com espalhamento existia uma vez (nos
//! webhooks), a justiça entre organizações existia uma vez (nos mesmos), o
//! tecto de tentativas em três das sete. A catraca da arquitectura não apanha
//! isto porque conta padrões sintácticos, e sete SQL diferentes com a mesma
//! semântica não batem em nenhum.
//!
//! O MOLDE é o do [`crate::query`] (ADR-0007): aqui ficam a forma e a
//! aritmética, com **cada nome de tabela e de coluna a ser um `&'static str`**
//! escrito no código que declara a fila; a tradução para SQL vive no adaptador
//! de Postgres (`delonix-meet-store::jobs`), que só conhece esses nomes e liga
//! todos os VALORES por *bind*. Esta camada não pode fazer IO — nem `sqlx`, nem
//! `tokio` (`scripts/check-crate-deps.sh`).
//!
//! O que isto **não** é: um broker. A migração `0090` fixou a postura da casa —
//! «o livro de entregas já é a fila: não há broker, e um reinício não perde o
//! que está agendado». Isto junta sete implementações do padrão; não o troca.

use std::time::Duration;

/// Como se reivindica o trabalho de uma fila.
///
/// Tudo aqui é `&'static str` escrito no módulo que declara a fila: nada nesta
/// estrutura vem de um pedido, de um cliente ou da base. É o mesmo contrato do
/// [`crate::query::SearchSchema`] — e é o que permite ao adaptador interpolar
/// estes nomes com segurança enquanto liga os valores por *bind*.
#[derive(Clone, Copy, Debug)]
pub struct Queue {
    /// Nome da fila nos registos e nas métricas (`"webhook_delivery"`).
    pub name: &'static str,
    /// Tabela onde o trabalho vive.
    pub table: &'static str,
    /// Coluna da chave primária.
    pub id_column: &'static str,
    /// Quando é que uma linha está PRONTA a ser levada. Fragmento SQL fixo,
    /// escrito no módulo da fila; os valores que precise vêm por *bind* nos
    /// parâmetros que o adaptador reserva (ver `store::jobs`).
    ///
    /// Exemplo: `"status = 'failed' AND retry_at IS NOT NULL AND retry_at <= now()"`.
    pub ready_when: &'static str,
    /// O que a reivindicação ESCREVE (sem `SET`): a marca de posse.
    ///
    /// Exemplo: `"status = 'running', started_at = now()"`.
    pub claim_set: &'static str,
    /// Colunas a devolver a quem levou o trabalho (sem `RETURNING`).
    pub returning: &'static str,
    /// Ordem da fila. FIFO em todas as sete que existiam.
    pub order_by: &'static str,
    /// Coluna do inquilino, para a justiça. `None` numa fila que não é por
    /// organização (as exportações são por pessoa e a ordem de chegada basta).
    ///
    /// Com ela, o lote é repartido entre inquilinos em vez de ser dado por
    /// ordem de chegada: é o que impede uma organização com mil itens falhados
    /// de empurrar as outras para trás da fila. A ideia é dos webhooks
    /// (`row_number() OVER (PARTITION BY org_id …)`) e era a única da casa.
    pub tenant_column: Option<&'static str>,
    /// Quantas linhas por volta.
    pub batch: i64,
}

/// Quantas vezes se tenta, e com que espera entre tentativas.
///
/// `delays` é a espera DEPOIS da tentativa número *n* (1-based). Ter menos
/// atrasos do que `max_attempts - 1` não é erro: a última espera repete-se.
#[derive(Clone, Copy, Debug)]
pub struct Retry {
    pub max_attempts: i32,
    pub delays: &'static [u64],
    /// Espalhamento, em fracção do atraso (0.2 = ±20 %).
    ///
    /// Está aqui e não no chamador porque é o que impede um destino que volta
    /// de levar, no mesmo segundo, tudo o que falhou junto — e quem escreve uma
    /// fila nova não se lembra disso sozinho.
    pub jitter: f64,
}

/// A política dos webhooks (`domain::integration::webhook_delivery`), que era a
/// única completa do repositório: cinco tentativas, 30 s a 1 h, ±20 %.
pub const RETRY_WEBHOOK: Retry = Retry {
    max_attempts: 5,
    delays: &[30, 120, 600, 3600],
    jitter: 0.2,
};

impl Retry {
    /// Sem repetição nenhuma: uma tentativa e acabou.
    pub const ONCE: Retry = Retry {
        max_attempts: 1,
        delays: &[],
        jitter: 0.0,
    };

    /// A espera depois de a tentativa `attempt` falhar, ou `None` se já não há
    /// mais tentativas. **Sem espalhamento** — esse aplica-se no adaptador, que
    /// é quem tem a fonte de aleatoriedade.
    pub fn delay_after(&self, attempt: i32) -> Option<Duration> {
        if attempt < 1 || attempt >= self.max_attempts {
            return None;
        }
        if self.delays.is_empty() {
            return Some(Duration::ZERO);
        }
        let i = (attempt as usize - 1).min(self.delays.len() - 1);
        Some(Duration::from_secs(self.delays[i]))
    }

    /// Depois de uma falha: volta à fila?
    pub fn should_retry(&self, failure: Failure, attempts_so_far: i32) -> bool {
        failure == Failure::Transient && attempts_so_far < self.max_attempts
    }

    /// Os limites do espalhamento, para o adaptador multiplicar pelo atraso.
    /// Com `jitter` a zero devolve `(1.0, 1.0)` — sem aleatoriedade.
    pub fn jitter_range(&self) -> (f64, f64) {
        let j = self.jitter.clamp(0.0, 0.9);
        (1.0 - j, 1.0 + j)
    }
}

/// Que tipo de falha foi — a pergunta que decide se insistir faz sentido.
///
/// Os webhooks tinham isto e as outras seis filas não: uma falha ESTÁVEL
/// (destino bloqueado pela guarda de saída, quota da organização, «nada
/// gravado») dá o mesmo resultado à terceira vez e gasta as tentativas que
/// servem para o caso que importa.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Failure {
    /// Pode passar sozinha: ligação, DNS, tempo-limite, 5xx, o processo morreu.
    Transient,
    /// Estável. Repetir daria o mesmo.
    Permanent,
}

impl Failure {
    /// A regra dos webhooks, generalizada: só `408`, `425`, `429` e `5xx`
    /// voltam. Um `4xx` é o destino a dizer que o pedido está errado, e um
    /// `3xx` também — repeti-lo só gasta tentativas.
    pub fn from_http(code: u16) -> Self {
        if code == 408 || code == 425 || code == 429 || (500..600).contains(&code) {
            Failure::Transient
        } else {
            Failure::Permanent
        }
    }
}

/// Posse de um trabalho em curso: curta e renovada por quem o detém.
///
/// A conta que importa, e que já se errou uma vez: a reserva **não** se
/// dimensiona pelo pior caso da ferramenta. Uma reserva dimensionada pelo tecto
/// de um ffmpeg (setenta e cinco minutos) fazia o nó seguinte esperá-la inteira
/// depois de um rollout — pior do que o problema que resolvia. É curta, e quem a tem empurra-a para a frente
/// com [`Lease::renew_every`] enquanto vive; morto o processo, ninguém a renova.
#[derive(Clone, Copy, Debug)]
pub struct Lease {
    pub duration: Duration,
}

impl Lease {
    pub const fn minutes(m: u64) -> Self {
        Lease {
            duration: Duration::from_secs(m * 60),
        }
    }

    /// De quanto em quanto tempo renovar: um quarto da reserva, nunca menos de
    /// cinco segundos. Quatro oportunidades dentro da reserva — um soluço da
    /// base não custa a posse, e não se renova mais vezes do que é útil.
    pub fn renew_every(&self) -> Duration {
        Duration::from_secs((self.duration.as_secs() / 4).max(5))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_politica_dos_webhooks_atravessa_a_peca_igual() {
        // O caso que decide se esta peça pode substituir a referência: os
        // mesmos atrasos, a mesma contagem, o mesmo tecto.
        let r = RETRY_WEBHOOK;
        assert_eq!(r.delay_after(1), Some(Duration::from_secs(30)));
        assert_eq!(r.delay_after(2), Some(Duration::from_secs(120)));
        assert_eq!(r.delay_after(3), Some(Duration::from_secs(600)));
        assert_eq!(r.delay_after(4), Some(Duration::from_secs(3600)));
        // À quinta não há mais: é o `MAX_AUTO_ATTEMPTS` dos webhooks.
        assert_eq!(r.delay_after(5), None);
        assert_eq!(r.delay_after(i32::MAX), None);
        // E um número absurdo não entra pela porta de trás.
        assert_eq!(r.delay_after(0), None);
        assert_eq!(r.delay_after(-7), None);
    }

    #[test]
    fn menos_atrasos_do_que_tentativas_repete_o_ultimo() {
        let r = Retry {
            max_attempts: 5,
            delays: &[10],
            jitter: 0.0,
        };
        for n in 1..5 {
            assert_eq!(r.delay_after(n), Some(Duration::from_secs(10)), "{n}");
        }
        assert_eq!(r.delay_after(5), None);
    }

    #[test]
    fn uma_falha_estavel_nunca_volta_a_fila() {
        let r = RETRY_WEBHOOK;
        assert!(r.should_retry(Failure::Transient, 1));
        assert!(r.should_retry(Failure::Transient, 4));
        assert!(!r.should_retry(Failure::Transient, 5), "passou do tecto");
        // Estável não volta nem na primeira — é o ponto todo.
        assert!(!r.should_retry(Failure::Permanent, 0));
    }

    #[test]
    fn a_classificacao_http_e_a_dos_webhooks() {
        for t in [408, 425, 429, 500, 502, 503, 599] {
            assert_eq!(Failure::from_http(t), Failure::Transient, "{t}");
        }
        for p in [200, 301, 302, 400, 401, 403, 404, 410, 422, 600] {
            assert_eq!(Failure::from_http(p), Failure::Permanent, "{p}");
        }
    }

    #[test]
    fn o_espalhamento_tem_limites_e_zero_e_sem_aleatoriedade() {
        let (lo, hi) = RETRY_WEBHOOK.jitter_range();
        assert!((lo - 0.8).abs() < 1e-9 && (hi - 1.2).abs() < 1e-9);
        assert_eq!(Retry::ONCE.jitter_range(), (1.0, 1.0));
        // Um valor absurdo não faz a espera negativa.
        let louco = Retry {
            jitter: 9.0,
            ..RETRY_WEBHOOK
        };
        let (lo, _) = louco.jitter_range();
        assert!(lo > 0.0, "o espalhamento podia dar uma espera negativa");
    }

    #[test]
    fn a_reserva_renova_se_quatro_vezes_e_nunca_em_rajada() {
        // A lição: curta e renovada, nunca dimensionada pelo pior caso da
        // ferramenta que corre o trabalho.
        let l = Lease::minutes(3);
        assert_eq!(l.renew_every(), Duration::from_secs(45));
        assert!(l.duration.as_secs() / l.renew_every().as_secs() >= 4);
        // Uma reserva minúscula não dá uma renovação a cada milissegundo.
        assert_eq!(
            Lease {
                duration: Duration::from_secs(4)
            }
            .renew_every(),
            Duration::from_secs(5)
        );
    }
}
