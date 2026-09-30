//! Qualidade medida do RTP que chega do telefone (RFC 3550 §6.4.1 e A.8).
//!
//! É daqui que sai o crachá «Ligação fraca»: jitter entre chegadas e perda por
//! buracos na numeração. Não é uma opinião do cliente nem um valor que o
//! FreeSWITCH nos diga — é o que a ponte vê chegar.

use std::time::Instant;

/// Janela de medição da perda (pacotes esperados). A perda é da janela, não
/// desde o início: uma chamada de uma hora que teve um minuto mau não fica
/// «fraca» para sempre.
const WINDOW: u32 = 250; // 5 s a 20 ms

#[derive(Debug)]
pub struct RtpQuality {
    clock_rate: f64,
    /// Jitter estimado em unidades de timestamp (RFC 3550 A.8).
    jitter: f64,
    last_transit: Option<f64>,
    epoch: Instant,
    highest_seq: Option<u16>,
    expected_in_window: u32,
    received_in_window: u32,
    last_loss: f64,
    pub received_total: u64,
    pub lost_total: u64,
}

impl RtpQuality {
    pub fn new(clock_rate: u32) -> Self {
        Self {
            clock_rate: clock_rate as f64,
            jitter: 0.0,
            last_transit: None,
            epoch: Instant::now(),
            highest_seq: None,
            expected_in_window: 0,
            received_in_window: 0,
            last_loss: 0.0,
            received_total: 0,
            lost_total: 0,
        }
    }

    pub fn observe_at(&mut self, seq: u16, rtp_ts: u32, arrival: Instant) {
        self.received_total += 1;
        let arrival_units = arrival.duration_since(self.epoch).as_secs_f64() * self.clock_rate;
        let transit = arrival_units - rtp_ts as f64;
        if let Some(prev) = self.last_transit {
            let d = (transit - prev).abs();
            // Um salto enorme é uma mudança de SSRC ou de relógio, não jitter.
            if d < self.clock_rate {
                self.jitter += (d - self.jitter) / 16.0;
            }
        }
        self.last_transit = Some(transit);

        match self.highest_seq {
            None => {
                self.highest_seq = Some(seq);
                self.expected_in_window = 1;
                self.received_in_window = 1;
            }
            Some(high) => {
                let delta = seq.wrapping_sub(high);
                if delta == 0 || delta > 0x8000 {
                    // Duplicado ou atrasado: chegou, mas não abre buraco novo.
                    self.received_in_window += 1;
                } else {
                    self.highest_seq = Some(seq);
                    self.expected_in_window += delta as u32;
                    self.received_in_window += 1;
                    let lost = delta as u64 - 1;
                    self.lost_total += lost;
                }
            }
        }
        if self.expected_in_window >= WINDOW {
            let lost = self
                .expected_in_window
                .saturating_sub(self.received_in_window);
            self.last_loss = lost as f64 / self.expected_in_window as f64;
            self.expected_in_window = 0;
            self.received_in_window = 0;
        }
    }

    /// Jitter em milissegundos.
    pub fn jitter_ms(&self) -> f64 {
        self.jitter / self.clock_rate * 1000.0
    }

    /// Perda (fracção) na última janela completa, ou na janela em curso se
    /// ainda não houve nenhuma completa.
    pub fn loss(&self) -> f64 {
        if self.last_loss > 0.0 || self.expected_in_window == 0 {
            return self.last_loss;
        }
        let lost = self
            .expected_in_window
            .saturating_sub(self.received_in_window);
        lost as f64 / self.expected_in_window.max(1) as f64
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn chegadas_regulares_dao_jitter_zero_e_sem_perda() {
        let t0 = Instant::now();
        let mut q = RtpQuality::new(8000);
        for i in 0..300u32 {
            q.observe_at(i as u16, i * 160, t0 + Duration::from_millis(20 * i as u64));
        }
        assert!(q.jitter_ms() < 0.5, "{}", q.jitter_ms());
        assert_eq!(q.loss(), 0.0);
    }

    #[test]
    fn chegadas_irregulares_medem_jitter() {
        let t0 = Instant::now();
        let mut q = RtpQuality::new(8000);
        for i in 0..300u32 {
            // ±40 ms alternados.
            let wobble = if i % 2 == 0 { 0 } else { 40 };
            q.observe_at(
                i as u16,
                i * 160,
                t0 + Duration::from_millis(20 * i as u64 + wobble),
            );
        }
        assert!(q.jitter_ms() > 30.0, "{}", q.jitter_ms());
    }

    #[test]
    fn buracos_na_numeracao_sao_perda() {
        let t0 = Instant::now();
        let mut q = RtpQuality::new(8000);
        let mut seq = 0u16;
        for i in 0..260u32 {
            // Perde 1 em cada 10.
            if i % 10 == 9 {
                seq = seq.wrapping_add(1);
                continue;
            }
            q.observe_at(seq, i * 160, t0 + Duration::from_millis(20 * i as u64));
            seq = seq.wrapping_add(1);
        }
        let loss = q.loss();
        assert!((0.08..=0.12).contains(&loss), "{loss}");
        // O último buraco (i = 259) só se saberia com o pacote seguinte.
        assert_eq!(q.lost_total, 25);
    }

    #[test]
    fn numeracao_da_volta_nao_e_perda() {
        let t0 = Instant::now();
        let mut q = RtpQuality::new(8000);
        for i in 0..20u32 {
            let seq = 65530u16.wrapping_add(i as u16);
            q.observe_at(seq, i * 160, t0 + Duration::from_millis(20 * i as u64));
        }
        assert_eq!(q.lost_total, 0);
    }
}
