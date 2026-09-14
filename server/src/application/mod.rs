//! Camada de aplicação (ADR-0004): orquestra repositório + portas +
//! regras/auditoria por caso de uso. Os handlers HTTP (`interface`, hoje
//! ainda os módulos por feature em `server/src/*.rs`) chamam estas funções e
//! nunca tocam em SQL nem em ficheiros directamente.

pub mod recording_service;
