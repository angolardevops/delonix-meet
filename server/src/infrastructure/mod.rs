//! Camada de infraestrutura (ADR-0004): implementações concretas das portas
//! de `domain::ports`. Escolhidas e montadas em `main.rs` a partir da
//! configuração — nunca referenciadas por tipo concreto fora daqui.

pub mod storage;
