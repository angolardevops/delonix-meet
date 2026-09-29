//! Ponte de media telefone↔sala (ADR-0010).
//!
//! Quem entra por telefone, sala SIP ou WhatsApp chega ao servidor como RTP
//! G.711 vindo do FreeSWITCH. A ponte faz dele um participante da sala:
//! publica o áudio dele no SFU como Opus e devolve-lhe a mistura da sala em
//! G.711. Nada disto passa por um browser nem por um segundo servidor de media.
//!
//! É a ÚNICA ponte do repo. O `pstn_bridge.rs` (Abordagem B) foi absorvido aqui
//! na integração de 2026-09-24: o transporte e o caminho de áudio são os deste
//! módulo (UA SIP + G.711↔Opus com mix-minus medido), a segurança é a dele
//! (`srtp.rs`). Razão escrita em `docs/pstn-sfu-bridge-design.md` §Superseded.

pub mod audio;
pub mod g711;
pub mod leg;
pub mod quality;
pub mod sip;
pub mod srtp;
