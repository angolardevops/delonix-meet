//! Contexto **telephony**: troncos SIP, plano de marcação, custo de chamadas e
//! as PORTAS para a infraestrutura de voz (ADR-0009).
//!
//! O que vive aqui não faz IO:
//!
//! - [`extension`] — a forma de um número curto de ramal e o número reservado
//!   de acesso às reuniões;
//! - [`extension_device`] — os aparelhos de um ramal móvel (plataforma, fornecedor de push, limites do
//!   *wake*; ADR-0023);
//! - [`extension_pin`] — o PIN de seis dígitos de um ramal (o que se recusa,
//!   como se sorteia sem enviesamento) e o intervalo de numeração automática;
//! - [`extension_provisioning`] — o bilhete de uso único e a configuração que
//!   o Linphone descarrega por QR;
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
pub mod extension_device;
pub mod extension_pin;
pub mod extension_provisioning;
pub mod money;
pub mod number;
pub mod ports;
pub mod trunk;
