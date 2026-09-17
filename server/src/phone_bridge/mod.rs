//! Ponte de media telefone↔sala (ADR-0010).
//!
//! Quem entra por telefone, sala SIP ou WhatsApp chega ao servidor como RTP
//! G.711 vindo do FreeSWITCH. A ponte faz dele um participante da sala:
//! publica o áudio dele no SFU como Opus e devolve-lhe a mistura da sala em
//! G.711. Nada disto passa por um browser nem por um segundo servidor de media.

pub mod audio;
pub mod g711;
pub mod leg;
pub mod quality;
pub mod sip;
