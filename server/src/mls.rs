//! Interface Message Layer Security (MLS, RFC 9420) — **DESENHO, NÃO SERVIÇO**.
//!
//! Descreve a camada que há-de substituir a chave AES-GCM partilhada e estática
//! por acordo de chaves de grupo com forward secrecy e autenticação por
//! remetente (ver `docs/reference/e2ee.md` para o que a chave partilhada NÃO
//! protege).
//!
//! **As rotas estão DESREGISTADAS no `main.rs`.** Estes handlers são stubs: não
//! guardam nada, não verificam nada, e não têm sequer extractor de
//! autenticação — devolviam `201`/`200`/`202` com `"status": "delivered"` a
//! qualquer pessoa, sem sessão e sem verificação de pertença à sala.
//!
//! Para voltarem a ser servidas falta-lhes, no mínimo: `AuthUser`, verificação
//! de acesso à sala (`can_access_room`), persistência dos KeyPackages, e a
//! máquina de estados MLS por trás. Enquanto isso não existir, a ausência é
//! mais honesta do que a resposta.
#![allow(dead_code)]

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use uuid::Uuid;

use crate::error::ApiError;
use crate::AppState;

#[derive(Debug, Serialize, Deserialize)]
pub struct KeyPackageReq {
    pub client_id: Uuid,
    pub key_package_bytes: String, // Base64 do KeyPackage MLS
}

#[derive(Debug, Serialize, Deserialize)]
pub struct WelcomeMsg {
    pub group_id: Uuid,
    pub welcome_bytes: String, // Base64 da Welcome Message MLS
}

/// Faz upload de um novo KeyPackage (usado para convidar este utilizador para o grupo MLS)
pub async fn upload_key_package(
    State(_state): State<Arc<AppState>>,
    Json(payload): Json<KeyPackageReq>,
) -> Result<impl IntoResponse, ApiError> {
    // Stub: Guarda o KeyPackage do utilizador na Base de Dados ou Redis.
    tracing::info!(
        "MLS: Recebido KeyPackage para o cliente {}",
        payload.client_id
    );
    Ok((
        StatusCode::CREATED,
        Json(serde_json::json!({"status": "ok"})),
    ))
}

/// Vai buscar os KeyPackages de um conjunto de utilizadores para os adicionar a uma sala
pub async fn fetch_key_packages(
    State(_state): State<Arc<AppState>>,
    Path(room_id): Path<Uuid>,
) -> Result<impl IntoResponse, ApiError> {
    // Stub: Retorna os pacotes dos utilizadores atualmente na sala para o criador do grupo
    tracing::info!(
        "MLS: A enviar KeyPackages dos utilizadores na sala {}",
        room_id
    );
    Ok((
        StatusCode::OK,
        Json(serde_json::json!({
            "room_id": room_id,
            "key_packages": [] // Lista vazia simulada
        })),
    ))
}

/// Distribui uma mensagem Welcome do anfitrião para os novos membros ingressarem
pub async fn distribute_welcome(
    State(_state): State<Arc<AppState>>,
    Json(payload): Json<WelcomeMsg>,
) -> Result<impl IntoResponse, ApiError> {
    // Stub: Recebe o WelcomeMessage do criador e reencaminha para o RedisPubSub
    // ou entrega assíncrona aos participantes alvo.
    tracing::info!(
        "MLS: Distribuída Welcome Message para a sala {}",
        payload.group_id
    );
    Ok((
        StatusCode::ACCEPTED,
        Json(serde_json::json!({"status": "delivered"})),
    ))
}

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/key-packages", post(upload_key_package))
        .route("/rooms/{room_id}/key-packages", get(fetch_key_packages))
        .route("/welcome", post(distribute_welcome))
}
