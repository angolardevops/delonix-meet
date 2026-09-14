//! Camada de domínio (ADR-0004): tipos e contratos que não dependem de axum,
//! sqlx nem reqwest. Hoje só as portas (`ports`); entidades/regras puras
//! entram aqui à medida que as fases seguintes do redesenho as extraírem dos
//! módulos por feature.

pub mod ports;
