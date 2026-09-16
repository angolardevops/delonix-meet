//! # delonix-meet-domain
//!
//! As regras do Delonix Meet, organizadas por contexto delimitado
//! (ADR-0005 §1). Cada contexto tem a sua linguagem e as suas invariantes; os
//! adaptadores (HTTP, gRPC, Postgres) chamam estas funções e nunca as
//! reimplementam — é assim que a BFF e a v1 deixam de divergir.

pub mod content;
pub mod identity;
