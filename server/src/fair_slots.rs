//! Vagas de trabalho pesado repartidas por inquilino.
//!
//! Um semáforo do tokio serve por ordem de chegada: uma organização com cem
//! composições na fila deixava a primeira de outra organização à espera das
//! cem. Aqui as vagas continuam a ter um tecto global, mas quando uma fica livre
//! vai para o inquilino que MENOS tem a correr e, a empate, o que foi servido há
//! mais tempo. Um inquilino com um pedido só espera, no pior caso, o fim de uma
//! vaga — não da fila dos outros.
//!
//! Não é preempção: uma vaga em curso nunca é retirada. E é por processo: com
//! vários pods cada um tem as suas vagas.

use std::{
    collections::{HashMap, VecDeque},
    sync::{Arc, Mutex},
};

use tokio::sync::oneshot;
use uuid::Uuid;

/// Inquilino de quem a origem não se conseguiu determinar (a base não
/// respondeu): partilham todos o mesmo balde, nunca o de alguém.
pub const UNKNOWN_TENANT: Uuid = Uuid::nil();

#[derive(Default)]
struct Inner {
    free: usize,
    /// A correr por inquilino.
    running: HashMap<Uuid, usize>,
    /// À espera, por inquilino, por ordem de chegada.
    waiting: HashMap<Uuid, VecDeque<oneshot::Sender<Slot>>>,
    /// Inquilinos com gente à espera, do servido há mais tempo para o mais
    /// recente. Desempata a escolha.
    ring: VecDeque<Uuid>,
}

#[derive(Clone)]
pub struct FairSlots(Arc<Mutex<Inner>>);

/// O conjunto de vagas deixou de existir com o pedido ainda à espera.
#[derive(Debug)]
pub struct Closed;

/// Uma vaga ocupada. Larga-se ao cair, mesmo que o futuro que a guardava seja
/// cancelado.
pub struct Slot {
    owner: Option<(FairSlots, Uuid)>,
}

impl FairSlots {
    pub fn new(capacity: usize) -> Self {
        Self(Arc::new(Mutex::new(Inner {
            free: capacity,
            ..Inner::default()
        })))
    }

    /// Vagas livres neste momento (para testes e métricas).
    pub fn available(&self) -> usize {
        self.0.lock().unwrap().free
    }

    /// Pede uma vaga para `tenant`. O lugar na fila fica marcado ao chamar, não
    /// ao primeiro poll do futuro devolvido.
    pub fn acquire(&self, tenant: Uuid) -> impl std::future::Future<Output = Result<Slot, Closed>> {
        let ready = {
            let mut g = self.0.lock().unwrap();
            if g.free > 0 && g.waiting.is_empty() {
                g.free -= 1;
                *g.running.entry(tenant).or_default() += 1;
                Ok(self.slot(tenant))
            } else {
                let (tx, rx) = oneshot::channel();
                if !g.waiting.contains_key(&tenant) {
                    g.ring.push_back(tenant);
                }
                g.waiting.entry(tenant).or_default().push_back(tx);
                // Pode haver vagas livres e só pedidos mortos à frente: sem isto
                // este ficava à espera de uma libertação que nunca vem.
                self.dispatch(&mut g);
                Err(rx)
            }
        };
        async move {
            match ready {
                Ok(slot) => Ok(slot),
                Err(rx) => rx.await.map_err(|_| Closed),
            }
        }
    }

    fn slot(&self, tenant: Uuid) -> Slot {
        Slot {
            owner: Some((self.clone(), tenant)),
        }
    }

    /// Entrega vagas livres a quem espera. Corre com o cadeado tomado.
    fn dispatch(&self, g: &mut Inner) {
        while g.free > 0 {
            // O inquilino com menos a correr; a empate, o que vem primeiro no anel.
            let Some(pick) = g
                .ring
                .iter()
                .enumerate()
                .min_by_key(|(i, t)| (g.running.get(*t).copied().unwrap_or(0), *i))
                .map(|(i, _)| i)
            else {
                return;
            };
            let tenant = g.ring.remove(pick).expect("índice do anel");
            let queue = g.waiting.get_mut(&tenant).expect("anel sem fila");
            let mut granted = false;
            while let Some(tx) = queue.pop_front() {
                match tx.send(self.slot(tenant)) {
                    Ok(()) => {
                        granted = true;
                        break;
                    }
                    // Quem esperava desistiu (future cancelado): a vaga volta e
                    // tenta-se o seguinte. Desarma-se para não se contar duas vezes.
                    Err(mut devolvida) => {
                        devolvida.owner.take();
                    }
                }
            }
            if queue.is_empty() {
                g.waiting.remove(&tenant);
            } else {
                g.ring.push_back(tenant);
            }
            if granted {
                g.free -= 1;
                *g.running.entry(tenant).or_default() += 1;
            }
        }
    }
}

impl Drop for Slot {
    fn drop(&mut self) {
        let Some((slots, tenant)) = self.owner.take() else {
            return;
        };
        let mut g = slots.0.lock().unwrap();
        g.free += 1;
        if let Some(n) = g.running.get_mut(&tenant) {
            *n -= 1;
            if *n == 0 {
                g.running.remove(&tenant);
            }
        }
        slots.dispatch(&mut g);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn t(n: u128) -> Uuid {
        Uuid::from_u128(n)
    }

    /// Com uma vaga e a organização 1 com três pedidos à frente de um da 2, a 2
    /// é servida logo a seguir ao primeiro — numa fila por ordem de chegada
    /// seria a última.
    #[tokio::test]
    async fn a_second_tenant_is_not_queued_behind_the_first_ones_backlog() {
        let slots = FairSlots::new(1);
        let held = slots.acquire(t(1)).await.unwrap();

        let order = Arc::new(Mutex::new(Vec::<(u128, u8)>::new()));
        let mut tasks = Vec::new();
        for (tenant, n) in [(1u128, 1u8), (1, 2), (1, 3), (2, 1)] {
            let fut = slots.acquire(t(tenant));
            let order = order.clone();
            tasks.push(tokio::spawn(async move {
                let slot = fut.await.unwrap();
                order.lock().unwrap().push((tenant, n));
                tokio::time::sleep(Duration::from_millis(20)).await;
                drop(slot);
            }));
        }
        drop(held);
        for task in tasks {
            tokio::time::timeout(Duration::from_secs(5), task)
                .await
                .unwrap()
                .unwrap();
        }
        let order = order.lock().unwrap().clone();
        let pos = order.iter().position(|e| *e == (2, 1)).unwrap();
        assert!(
            pos <= 1,
            "o inquilino 2 tinha de ser servido logo a seguir ao primeiro do 1, foi o {pos}: {order:?}"
        );
        assert_eq!(order.len(), 4, "todos acabam por ser servidos: {order:?}");
    }

    /// O tecto global nunca é passado, e é usado por inteiro.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn the_global_cap_is_never_exceeded() {
        use std::sync::atomic::{AtomicUsize, Ordering::SeqCst};
        const CAP: usize = 2;
        let slots = FairSlots::new(CAP);
        let now = Arc::new(AtomicUsize::new(0));
        let peak = Arc::new(AtomicUsize::new(0));
        let mut tasks = Vec::new();
        for i in 0..12u128 {
            let fut = slots.acquire(t(i % 3));
            let (now, peak) = (now.clone(), peak.clone());
            tasks.push(tokio::spawn(async move {
                let _slot = fut.await.unwrap();
                let n = now.fetch_add(1, SeqCst) + 1;
                peak.fetch_max(n, SeqCst);
                tokio::time::sleep(Duration::from_millis(15)).await;
                now.fetch_sub(1, SeqCst);
            }));
        }
        for task in tasks {
            task.await.unwrap();
        }
        assert_eq!(peak.load(SeqCst), CAP);
        assert_eq!(slots.available(), CAP, "no fim todas as vagas voltaram");
    }

    /// Quem desiste da fila não leva a vaga consigo.
    #[tokio::test]
    async fn a_cancelled_waiter_does_not_leak_the_slot() {
        let slots = FairSlots::new(1);
        let held = slots.acquire(t(1)).await.unwrap();
        let desiste = slots.acquire(t(2));
        let fica = tokio::spawn(slots.acquire(t(3)));
        drop(desiste);
        drop(held);
        let slot = tokio::time::timeout(Duration::from_secs(5), fica)
            .await
            .expect("a vaga tinha de passar a quem ficou")
            .unwrap()
            .unwrap();
        assert_eq!(slots.available(), 0);
        drop(slot);
        assert_eq!(slots.available(), 1);
    }

    /// Com vagas livres e ninguém à espera, ninguém espera.
    #[tokio::test]
    async fn free_slots_are_taken_without_waiting() {
        let slots = FairSlots::new(2);
        let a = slots.acquire(t(1)).await.unwrap();
        let b = slots.acquire(t(2)).await.unwrap();
        assert_eq!(slots.available(), 0);
        drop((a, b));
        assert_eq!(slots.available(), 2);
    }
}
