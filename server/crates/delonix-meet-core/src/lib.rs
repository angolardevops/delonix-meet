//! # delonix-meet-core
//!
//! O núcleo partilhado (*shared kernel*) do Delonix Meet. Tudo o que aqui está
//! é usado por mais de um contexto de domínio e não faz IO:
//!
//! - [`error`] — o erro de domínio, com um código estável que é contrato de API;
//! - [`crypto`] — hashes, tokens aleatórios, comparação em tempo constante e
//!   argon2, com UM dono (antes havia cópias em seis módulos);
//! - [`page`] — paginação por cursor opaco, com limite obrigatório;
//! - [`edition`] — os perfis de instalação (SaaS, enterprise, pessoal).
//!
//! Ver `docs/adr/0005-backend-enterprise-contextos-edicoes-e-entrega.md`.

pub mod crypto;
pub mod edition;
pub mod error;
pub mod page;
pub mod secret_box;

pub use error::{DomainError, ErrorKind};
