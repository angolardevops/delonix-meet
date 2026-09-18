//! Contexto **identity**: contas, credenciais, sessões, chaves de API.
//!
//! Primeira fatia: chaves `dlx_` (`org_api_keys`). O resto do contexto
//! (`users`, `auth`, `mfa`) continua no monólito — sai depois, na mesma
//! ordem do ADR-0006 §6 linha D.

pub mod api_key;
