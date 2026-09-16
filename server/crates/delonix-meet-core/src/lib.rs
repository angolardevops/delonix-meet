//! # delonix-meet-core
//!
//! O núcleo partilhado (*shared kernel*) do Delonix Meet. Tudo o que aqui está
//! é usado por mais de um contexto de domínio e não faz IO:
//!
//! - [`error`] — o erro de domínio, com um código estável que é contrato de API;
//! - [`crypto`] — hashes, tokens aleatórios, comparação em tempo constante e
//!   argon2, com UM dono (antes havia cópias em seis módulos);
//! - [`page`] — paginação por cursor opaco, com limite obrigatório;
//! - [`query`] — pesquisa de lista estilo Odoo: domínio de filtro contra lista
//!   branca, ordenação, agrupamento e keyset (ADR-0007);
//! - [`edition`] — os perfis de instalação (SaaS, enterprise, pessoal).
//!
//! Ver `docs/adr/0006-backend-enterprise-contextos-edicoes-e-entrega.md`.

pub mod crypto;
pub mod edition;
pub mod error;
pub mod page;
pub mod query;
pub mod secret_box;

pub use error::{DomainError, ErrorKind};
