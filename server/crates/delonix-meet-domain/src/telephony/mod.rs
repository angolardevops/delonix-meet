//! Contexto **telephony**: troncos SIP, plano de marcação, custo de chamadas e
//! as PORTAS para a infraestrutura de voz (ADR-0009).
//!
//! O que vive aqui não faz IO:
//!
//! - [`extension`] — a forma de um número curto de ramal e o número reservado
//!   de acesso às reuniões;
//! - [`number`] — normalizar o número marcado e mascará-lo para listas;
//! - [`dial_plan`] — padrões, validação do plano e resolução «a primeira regra
//!   que casar vale», com o invariante de EMERGÊNCIA (nunca gravada, nunca
//!   bloqueada);
//! - [`money`] — valores monetários em décimas-milésimas (sem `f64`);
//! - [`cost`] — preço em vigor num instante, custo de uma chamada, ASR e estado
//!   de um tronco derivado de medidas;
//! - [`ports`] — os traits que os adaptadores (FreeSWITCH ESL, `mod_json_cdr`,
//!   gateway de SMS) implementam: `SipControl`, `CallOriginator`, `CdrSource`,
//!   `SmsGateways`.
//!
//! O contrato para quem CONSOME estas portas (frente D, canais na sala) está em
//! `notas-ui-template/contrato-telefonia.md` e no ADR-0009.

pub mod cost;
pub mod dial_plan;
pub mod extension;
pub mod money;
pub mod number;
pub mod ports;
pub mod trunk;
