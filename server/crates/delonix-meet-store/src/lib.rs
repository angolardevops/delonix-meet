//! # delonix-meet-store
//!
//! Repositórios sqlx do Delonix Meet, organizados pelos mesmos contextos
//! delimitados do `delonix-meet-domain` (ADR-0006 §1). Aqui mora o SQL que
//! hoje ainda está espalhado pelos handlers do monólito — sai fatia a fatia,
//! contexto a contexto, começando por `identity`.
//!
//! Uma função por consulta, não um trait de repositório genérico: um portão
//! (`scripts/check-crate-deps.sh`) impede este crate de depender de axum,
//! tonic ou webrtc, e outro (a catraca da arquitectura) impede o SQL de
//! reaparecer fora daqui uma vez que uma fatia sai do monólito.

pub mod identity;
