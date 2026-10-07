//! O ciclo que faz uma fila andar — e que PÁRA no shutdown.
//!
//! A terceira parte do trabalho nº2: a forma e a aritmética estão em
//! [`delonix_meet_core::jobs`], a reivindicação em
//! [`delonix_meet_store::jobs`], e aqui está quem as chama a horas.
//!
//! PORQUE EXISTE, além de juntar as sete filas: dos **catorze** ciclos de fundo
//! do `run()` medidos a 2026-10-07, **um** tem `CancellationToken`
//! (`run_quarantine_sweeper`). Os outros treze são abortados a meio da
//! iteração quando o processo sai — a forma normal de perder trabalho neste
//! sistema é fazer deploy. A `delonix-meet-backend` já o dizia pelo nome:
//! «*não*: mais um `tokio::spawn` solto em `run()` — já lá estão catorze».
//!
//! O molde é o do `meetings::run_quarantine_sweeper`, que é o que está certo.

use std::sync::Arc;
use std::time::Duration;

use delonix_meet_core::jobs::{Lease, Queue, Retry};
use tokio_util::sync::CancellationToken;

/// Uma fila montada: a declaração, a política e o ritmo.
pub struct Worker {
    pub queue: Queue,
    pub retry: Retry,
    /// A posse de um trabalho em curso. `None` numa fila cujo trabalho é
    /// instantâneo e não precisa de ser renovado (uma marca-e-esquece).
    pub lease: Option<Lease>,
    /// De quanto em quanto tempo se procura trabalho novo.
    pub every: Duration,
}

/// Corre `uma_volta` a cada `every` até o token ser cancelado.
///
/// O que isto garante e o `tokio::spawn` solto não garantia: **a volta em curso
/// acaba**. O `select!` espera pelo tique OU pelo cancelamento; cancelado, sai
/// no topo do ciclo, não a meio de uma reivindicação. Quem precisa de mais do
/// que isso — esperar que o trabalho despachado termine — tem de o contar num
/// gauge e esperá-lo no drain, como o `recorder` faz com as composições.
pub async fn run<F, Fut>(
    nome: &'static str,
    every: Duration,
    stop: CancellationToken,
    mut uma_volta: F,
) where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Result<usize, sqlx::Error>>,
{
    let mut ticker = tokio::time::interval(every);
    // `Skip` e não `Delay`: uma volta que demorou mais do que o período não
    // deve ser seguida por outra imediata — isso transformava um soluço numa
    // rajada contra a base.
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        tokio::select! {
            _ = stop.cancelled() => {
                tracing::info!(fila = nome, "fila parada (shutdown)");
                break;
            }
            _ = ticker.tick() => {}
        }
        match uma_volta().await {
            Ok(0) => {}
            Ok(n) => tracing::info!(fila = nome, levados = n, "trabalho reivindicado"),
            Err(e) => tracing::warn!(fila = nome, error = %e, "a fila falhou uma volta"),
        }
    }
}

/// As filas que o `run()` levanta, cada uma com o seu `JoinHandle`, para que o
/// shutdown as possa esperar em vez de as abortar.
///
/// Guarda um `CancellationToken` só: as filas param todas juntas, e uma fila
/// que precisasse de parar sozinha seria um caso novo a justificar.
pub struct Filas {
    stop: CancellationToken,
    tarefas: tokio::task::JoinSet<()>,
}

impl Filas {
    pub fn new() -> Self {
        Filas {
            stop: CancellationToken::new(),
            tarefas: tokio::task::JoinSet::new(),
        }
    }

    /// Levanta uma fila. O `nome` é o que aparece nos registos.
    pub fn levanta<F, Fut>(&mut self, nome: &'static str, every: Duration, uma_volta: F)
    where
        F: FnMut() -> Fut + Send + 'static,
        Fut: std::future::Future<Output = Result<usize, sqlx::Error>> + Send + 'static,
    {
        let stop = self.stop.clone();
        self.tarefas.spawn(run(nome, every, stop, uma_volta));
    }

    /// Manda parar e **espera** que a volta em curso de cada fila acabe, com
    /// tecto. Sem o tecto, uma fila presa numa consulta lenta segurava o
    /// shutdown até ao SIGKILL — que é pior do que a abortar.
    pub async fn parar(mut self, limite: Duration) {
        self.stop.cancel();
        let quantas = self.tarefas.len();
        match tokio::time::timeout(limite, async {
            while self.tarefas.join_next().await.is_some() {}
        })
        .await
        {
            Ok(()) => tracing::info!(filas = quantas, "filas paradas"),
            Err(_) => tracing::warn!(
                filas = quantas,
                segundos = limite.as_secs(),
                "prazo esgotado a parar as filas — as voltas em curso foram abortadas"
            ),
        }
    }
}

impl Default for Filas {
    fn default() -> Self {
        Self::new()
    }
}

/// Reivindica trabalho desta fila. Atalho para o adaptador, para um módulo de
/// fila não ter de importar o `store` e o `core` só para uma chamada.
pub async fn claim<T>(state: &Arc<crate::AppState>, w: &Worker) -> Result<Vec<T>, sqlx::Error>
where
    T: for<'r> sqlx::FromRow<'r, sqlx::postgres::PgRow> + Send + Unpin,
{
    delonix_meet_store::jobs::claim(&state.db, &w.queue, &w.retry).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// **O defeito que isto guarda.** Treze dos catorze ciclos de fundo não
    /// param no shutdown: são abortados a meio da volta. Este sai no topo do
    /// ciclo e a volta em curso acaba.
    #[tokio::test]
    async fn uma_fila_para_no_cancelamento_e_a_volta_em_curso_acaba() {
        let voltas = Arc::new(AtomicUsize::new(0));
        let acabou = Arc::new(AtomicUsize::new(0));
        let stop = CancellationToken::new();
        let (v, a) = (voltas.clone(), acabou.clone());
        let tarefa = tokio::spawn(run(
            "prova",
            Duration::from_millis(10),
            stop.clone(),
            move || {
                let (v, a) = (v.clone(), a.clone());
                async move {
                    v.fetch_add(1, Ordering::SeqCst);
                    // Uma volta que demora: o cancelamento no meio dela não a
                    // pode cortar.
                    tokio::time::sleep(Duration::from_millis(60)).await;
                    a.fetch_add(1, Ordering::SeqCst);
                    Ok(0)
                }
            },
        ));
        tokio::time::sleep(Duration::from_millis(30)).await;
        stop.cancel();
        tokio::time::timeout(Duration::from_secs(2), tarefa)
            .await
            .expect("a fila não parou no cancelamento")
            .unwrap();
        let (v, a) = (voltas.load(Ordering::SeqCst), acabou.load(Ordering::SeqCst));
        assert!(v >= 1, "a fila nunca correu");
        assert_eq!(v, a, "uma volta foi cortada a meio pelo cancelamento");
    }

    #[tokio::test]
    async fn o_erro_de_uma_volta_nao_mata_a_fila() {
        let voltas = Arc::new(AtomicUsize::new(0));
        let stop = CancellationToken::new();
        let v = voltas.clone();
        let tarefa = tokio::spawn(run(
            "prova",
            Duration::from_millis(5),
            stop.clone(),
            move || {
                let v = v.clone();
                async move {
                    v.fetch_add(1, Ordering::SeqCst);
                    Err(sqlx::Error::PoolClosed)
                }
            },
        ));
        tokio::time::sleep(Duration::from_millis(40)).await;
        stop.cancel();
        let _ = tokio::time::timeout(Duration::from_secs(2), tarefa).await;
        assert!(
            voltas.load(Ordering::SeqCst) > 1,
            "a fila morreu ao primeiro erro"
        );
    }

    /// O `parar` espera, mas com tecto: uma fila presa não segura o shutdown
    /// até ao SIGKILL.
    #[tokio::test]
    async fn parar_tem_tecto_e_nao_segura_o_shutdown() {
        let mut filas = Filas::new();
        filas.levanta("presa", Duration::from_millis(5), || async {
            tokio::time::sleep(Duration::from_secs(3600)).await;
            Ok(0)
        });
        tokio::time::sleep(Duration::from_millis(20)).await;
        let inicio = std::time::Instant::now();
        filas.parar(Duration::from_millis(100)).await;
        assert!(
            inicio.elapsed() < Duration::from_secs(2),
            "o parar ficou pendurado numa fila presa"
        );
    }
}
