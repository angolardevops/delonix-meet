//! Biblioteca de quadros brancos: guardados como PNG por organização.
//!
//! Isolamento multi-tenant: um quadro pertence a uma organização e só é
//! listado/acedido por membros dessa org (via [`crate::org::orgs_of_user`]).
//! Partilha só-leitura por link público com token (`share_token`).
//!
//! **URL assinado (G11).** `POST /api/whiteboards/{id}/signed-url` devolve um
//! `/png?exp=…&sig=…` de no máximo 15 minutos, para um `<img>` o carregar sem
//! sessão. Só o emite quem já pode ver o quadro; as regras estão em
//! `delonix_meet_domain::content::whiteboard`.

use axum::{
    extract::{Path, Query, State},
    http::header,
    response::IntoResponse,
    Json,
};
use base64::{engine::general_purpose, Engine};
use chrono::{DateTime, Utc};
use delonix_meet_core::crypto;
use delonix_meet_domain::content::whiteboard as rules;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use uuid::Uuid;

use crate::{auth::AuthUser, error::ApiError, org::orgs_of_user, AppState};

/// PNG máximo aceite (evita abusos): 8 MB.
const MAX_PNG_BYTES: usize = 8 * 1024 * 1024;

/// Imagem PNG em bruto (só para o spec).
#[derive(utoipa::ToSchema)]
#[schema(value_type = String, format = Binary)]
#[allow(dead_code)]
pub struct PngBytes(Vec<u8>);

/// Documentação OpenAPI das rotas deste módulo (`openapi.rs` junta-as).
#[derive(utoipa::OpenApi)]
#[openapi(
    paths(list, get_one, save, delete, png, signed_url, set_share, shared_png),
    components(schemas(WhiteboardMeta, SaveReq, ShareReq, SignedUrl))
)]
pub struct ApiDoc;

#[derive(Serialize, sqlx::FromRow, utoipa::ToSchema)]
pub struct WhiteboardMeta {
    pub id: Uuid,
    pub title: String,
    pub room_code: String,
    pub is_public: bool,
    /// Token do link público; string vazia enquanto `is_public = false`.
    pub share_token: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Deserialize, utoipa::ToSchema)]
#[schema(as = WhiteboardSaveReq)]
pub struct SaveReq {
    /// Truncado a 120 caracteres; vazio = «Quadro sem título».
    #[serde(default)]
    pub title: String,
    /// Truncado a 64 caracteres. Não é verificado.
    #[serde(default)]
    pub room_code: String,
    /// PNG em base64 (com ou sem prefixo `data:image/png;base64,`).
    pub png_base64: String,
}

const PNG_MAGIC: &[u8] = &[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];

fn decode_png(s: &str) -> Result<Vec<u8>, ApiError> {
    let raw = s.split(',').next_back().unwrap_or(s).trim();
    // Rejeita ANTES de descodificar (evita alocar ~500MB só para rejeitar).
    if raw.len() > MAX_PNG_BYTES * 4 / 3 + 4 {
        return Err(ApiError::BadRequest(
            "tamanho do quadro fora do limite".into(),
        ));
    }
    let bytes = general_purpose::STANDARD
        .decode(raw)
        .map_err(|_| ApiError::BadRequest("PNG inválido".into()))?;
    if bytes.len() > MAX_PNG_BYTES {
        return Err(ApiError::BadRequest(
            "tamanho do quadro fora do limite".into(),
        ));
    }
    // Valida a assinatura PNG (impede guardar bytes arbitrários servidos como image/png).
    if !bytes.starts_with(PNG_MAGIC) {
        return Err(ApiError::BadRequest(
            "o conteúdo não é um PNG válido".into(),
        ));
    }
    Ok(bytes)
}

/// Não expõe o token de partilha enquanto o quadro não for público.
fn mask_token(mut m: WhiteboardMeta) -> WhiteboardMeta {
    if !m.is_public {
        m.share_token = String::new();
    }
    m
}

/// Guarda um quadro na biblioteca da organização do utilizador (a primeira,
/// se tiver várias). PNG até 8 MiB, validado pela assinatura.
#[utoipa::path(
    post, path = "/api/whiteboards", tag = "whiteboards",
    security(("session" = [])),
    request_body = SaveReq,
    responses(
        (status = 200, body = WhiteboardMeta),
        (status = 400, description = "Utilizador sem organização, base64 inválido, acima de 8 MiB, ou não é PNG.", body = crate::openapi::ErrorBody),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 413, description = "Corpo acima do limite do router (texto simples)."),
    )
)]
pub async fn save(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Json(req): Json<SaveReq>,
) -> Result<Json<WhiteboardMeta>, ApiError> {
    let org_id = *orgs_of_user(&state, auth.user_id)
        .await
        .first()
        .ok_or(ApiError::BadRequest("utilizador sem organização".into()))?;
    let png = decode_png(&req.png_base64)?;
    let title = if req.title.trim().is_empty() {
        "Quadro sem título".to_string()
    } else {
        req.title.trim().chars().take(120).collect()
    };
    let meta: WhiteboardMeta = sqlx::query_as(
        "INSERT INTO whiteboards (org_id, owner_id, title, room_code, png)
         VALUES ($1, $2, $3, $4, $5)
         RETURNING id, title, room_code, is_public, share_token, created_at",
    )
    .bind(org_id)
    .bind(auth.user_id)
    .bind(title)
    .bind(req.room_code.chars().take(64).collect::<String>())
    .bind(png)
    .fetch_one(&state.db)
    .await?;
    Ok(Json(mask_token(meta)))
}

/// Lista os quadros das organizações a que o utilizador pertence (máx. 200,
/// mais recentes primeiro).
#[utoipa::path(
    get, path = "/api/whiteboards", tag = "whiteboards",
    security(("session" = [])),
    responses(
        (status = 200, body = Vec<WhiteboardMeta>),
        (status = 401, body = crate::openapi::ErrorBody),
    )
)]
pub async fn list(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
) -> Result<Json<Vec<WhiteboardMeta>>, ApiError> {
    let orgs = orgs_of_user(&state, auth.user_id).await;
    if orgs.is_empty() {
        return Ok(Json(vec![]));
    }
    let items: Vec<WhiteboardMeta> = sqlx::query_as(
        "SELECT id, title, room_code, is_public, share_token, created_at
         FROM whiteboards WHERE org_id = ANY($1) ORDER BY created_at DESC LIMIT 200",
    )
    .bind(&orgs)
    .fetch_all(&state.db)
    .await?;
    Ok(Json(items.into_iter().map(mask_token).collect()))
}

/// Metadados de um quadro (sem a imagem).
#[utoipa::path(
    get, path = "/api/whiteboards/{whiteboard_id}", tag = "whiteboards",
    security(("session" = [])),
    params(("whiteboard_id" = Uuid, Path)),
    responses(
        (status = 200, body = WhiteboardMeta),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 404, body = crate::openapi::ErrorBody, description = "não existe, ou é de uma organização de que não és membro"),
    )
)]
pub async fn get_one(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> Result<Json<WhiteboardMeta>, ApiError> {
    let orgs = orgs_of_user(&state, auth.user_id).await;
    let item: Option<WhiteboardMeta> = sqlx::query_as(
        "SELECT id, title, room_code, is_public, share_token, created_at
         FROM whiteboards WHERE id = $1 AND org_id = ANY($2)",
    )
    .bind(id)
    .bind(&orgs)
    .fetch_optional(&state.db)
    .await?;
    Ok(Json(mask_token(item.ok_or(ApiError::NotFound)?)))
}

#[derive(Deserialize, utoipa::IntoParams)]
#[into_params(parameter_in = Query)]
pub struct PngQuery {
    /// Prazo (segundos Unix) de um URL assinado. Só com `sig`.
    pub exp: Option<String>,
    /// Assinatura de um URL assinado (ver `POST /api/whiteboards/{id}/signed-url`).
    pub sig: Option<String>,
}

/// A subchave dos URLs assinados, derivada do segredo do servidor.
fn signing_key(state: &AppState) -> [u8; 32] {
    crypto::derive_key(&state.config.jwt_secret, rules::KEY_PURPOSE)
}

/// O PNG do quadro, se quem pede o puder ver (membro activo da org
/// dona). Inexistente e alheio dão o mesmo `404`.
async fn viewable_png(state: &AppState, id: Uuid, user_id: Uuid) -> Result<Vec<u8>, ApiError> {
    let orgs = orgs_of_user(state, user_id).await;
    let row: Option<(Vec<u8>, Uuid)> =
        sqlx::query_as("SELECT png, org_id FROM whiteboards WHERE id = $1")
            .bind(id)
            .fetch_optional(&state.db)
            .await?;
    match row {
        Some((png, org)) if orgs.contains(&org) => Ok(png),
        // Quadro de outra organização: não se confirma que existe.
        _ => Err(ApiError::NotFound),
    }
}

/// Imagem PNG de um quadro.
///
/// - **Com sessão** (sem `sig`): apenas membros da org dona.
/// - **Com `exp` e `sig`** (URL assinado): sem sessão. Assinatura errada, prazo
///   expirado ou quadro inexistente dão todos `404`.
#[utoipa::path(
    get, path = "/api/whiteboards/{whiteboard_id}/image", tag = "whiteboards",
    security((), ("session" = [])),
    params(("whiteboard_id" = Uuid, Path), PngQuery),
    responses(
        (status = 200, body = inline(PngBytes), content_type = "image/png"),
        (status = 400, description = "Com sessão: `id` que não é UUID.", body = crate::openapi::ErrorBody),
        (status = 401, description = "Sem `sig` e sem sessão válida.", body = crate::openapi::ErrorBody),
        (status = 404, description = "Não existe, é de uma organização de que não és membro, ou o URL assinado é inválido ou expirou.", body = crate::openapi::ErrorBody),
    )
)]
pub async fn png(
    State(state): State<Arc<AppState>>,
    // A sessão só se exige sem URL assinado: a rejeição do extractor fica
    // guardada e devolve-se tal e qual nesse caso (o comportamento de sempre).
    auth: Result<AuthUser, ApiError>,
    Path(id): Path<String>,
    Query(q): Query<PngQuery>,
) -> Result<impl IntoResponse, ApiError> {
    if q.sig.is_some() || q.exp.is_some() {
        let id = Uuid::parse_str(&id).map_err(|_| ApiError::NotFound)?;
        let valid = match (q.exp.as_deref(), q.sig.as_deref()) {
            (Some(exp), Some(sig)) => {
                rules::verify(&signing_key(&state), id, exp, sig, Utc::now().timestamp())
            }
            _ => false,
        };
        if !valid {
            return Err(ApiError::NotFound);
        }
        let png: Option<Vec<u8>> = sqlx::query_scalar("SELECT png FROM whiteboards WHERE id = $1")
            .bind(id)
            .fetch_optional(&state.db)
            .await?;
        let png = png.ok_or(ApiError::NotFound)?;
        return Ok((
            [
                (header::CONTENT_TYPE, "image/png"),
                // O URL é uma credencial de curta duração: nenhuma cache
                // partilhada o guarda, e o browser não o reenvia como Referer.
                (header::CACHE_CONTROL, "private, no-store"),
                (header::REFERRER_POLICY, "no-referrer"),
            ],
            png,
        )
            .into_response());
    }
    let auth = auth?;
    let id =
        Uuid::parse_str(&id).map_err(|_| ApiError::BadRequest("id de quadro inválido".into()))?;
    let png = viewable_png(&state, id, auth.user_id).await?;
    Ok(([(header::CONTENT_TYPE, "image/png")], png).into_response())
}

/// URL assinado do PNG.
#[derive(Serialize, utoipa::ToSchema)]
#[schema(as = WhiteboardSignedUrl)]
pub struct SignedUrl {
    /// Caminho relativo (`/api/whiteboards/{id}/image?exp=…&sig=…`), carregável sem sessão.
    pub url: String,
    pub expires_at: DateTime<Utc>,
}

/// Método personalizado: emite um URL assinado do PNG, válido 15 minutos, para
/// quem JÁ pode ver o quadro. Quem não pode recebe `404`, como no PNG.
#[utoipa::path(
    post, path = "/api/whiteboards/{whiteboard_id}/signed-url", tag = "whiteboards",
    security(("session" = [])),
    params(("whiteboard_id" = Uuid, Path)),
    responses(
        (status = 200, body = SignedUrl),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 404, description = "Não existe, ou é de uma organização de que não és membro.", body = crate::openapi::ErrorBody),
    )
)]
pub async fn signed_url(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> Result<Json<SignedUrl>, ApiError> {
    let orgs = orgs_of_user(&state, auth.user_id).await;
    let org: Option<Uuid> = sqlx::query_scalar("SELECT org_id FROM whiteboards WHERE id = $1")
        .bind(id)
        .fetch_optional(&state.db)
        .await?;
    if !org.is_some_and(|o| orgs.contains(&o)) {
        return Err(ApiError::NotFound);
    }
    let exp = rules::expiry_from(Utc::now().timestamp());
    Ok(Json(SignedUrl {
        url: rules::signed_path(&signing_key(&state), id, exp),
        expires_at: DateTime::from_timestamp(exp, 0).ok_or_else(|| ApiError::internal("prazo"))?,
    }))
}

/// Apaga um quadro — dono ou admin da org.
#[utoipa::path(
    delete, path = "/api/whiteboards/{whiteboard_id}", tag = "whiteboards",
    security(("session" = [])),
    params(("whiteboard_id" = Uuid, Path)),
    responses(
        (status = 204, description = "Quadro apagado."),
        (status = 401, description = "Sessão inválida.", body = crate::openapi::ErrorBody),
        (status = 403, description = "`whiteboard.not_manager`: membro da organização, mas nem dono nem admin.", body = crate::openapi::ErrorBody),
        (status = 404, description = "Não existe, ou não é membro activo da organização do quadro.", body = crate::openapi::ErrorBody),
    )
)]
pub async fn delete(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> Result<axum::http::StatusCode, ApiError> {
    let row: (Uuid, Uuid) =
        sqlx::query_as("SELECT owner_id, org_id FROM whiteboards WHERE id = $1")
            .bind(id)
            .fetch_optional(&state.db)
            .await?
            .ok_or(ApiError::NotFound)?;
    // Fora da organização do quadro (ou arquivado nela) não existe: 404.
    let role = crate::org::role_in_org(&state, row.1, auth.user_id)
        .await?
        .ok_or(ApiError::NotFound)?;
    if row.0 != auth.user_id && role != "admin" {
        return Err(
            delonix_meet_core::DomainError::forbidden("whiteboard.not_manager")
                .with_message("só o dono do quadro ou um administrador da organização o apaga")
                .into(),
        );
    }
    sqlx::query("DELETE FROM whiteboards WHERE id = $1")
        .bind(id)
        .execute(&state.db)
        .await?;
    Ok(axum::http::StatusCode::NO_CONTENT)
}

#[derive(Deserialize, utoipa::ToSchema)]
#[schema(as = WhiteboardShareReq)]
pub struct ShareReq {
    pub public: bool,
}

/// Ativa/desativa a partilha por link público. Dono ou admin.
/// Desativar roda o token: o link antigo deixa de funcionar.
#[utoipa::path(
    put, path = "/api/whiteboards/{whiteboard_id}/public-link", tag = "whiteboards",
    security(("session" = [])),
    params(("whiteboard_id" = Uuid, Path)),
    request_body = ShareReq,
    responses(
        (status = 200, body = WhiteboardMeta),
        (status = 401, description = "Sessão inválida.", body = crate::openapi::ErrorBody),
        (status = 403, description = "Nem dono nem admin.", body = crate::openapi::ErrorBody),
        (status = 404, body = crate::openapi::ErrorBody),
    )
)]
pub async fn set_share(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
    Json(req): Json<ShareReq>,
) -> Result<Json<WhiteboardMeta>, ApiError> {
    let row: (Uuid, Uuid) =
        sqlx::query_as("SELECT owner_id, org_id FROM whiteboards WHERE id = $1")
            .bind(id)
            .fetch_one(&state.db)
            .await?;
    let is_owner = row.0 == auth.user_id;
    let is_admin = crate::org::require_admin_pub(&state, row.1, auth.user_id)
        .await
        .is_ok();
    if !is_owner && !is_admin {
        return Err(ApiError::Forbidden);
    }
    // Ao desativar a partilha, roda o token (o link antigo deixa de funcionar).
    let meta: WhiteboardMeta = sqlx::query_as(
        "UPDATE whiteboards SET is_public = $1,
             share_token = CASE WHEN $1 THEN share_token ELSE encode(gen_random_bytes(12), 'hex') END
         WHERE id = $2
         RETURNING id, title, room_code, is_public, share_token, created_at",
    )
    .bind(req.public)
    .bind(id)
    .fetch_one(&state.db)
    .await?;
    Ok(Json(mask_token(meta)))
}

/// Vista pública só-leitura por token — sem autenticação, se `is_public`.
#[utoipa::path(
    get, path = "/api/public/whiteboards/{token}/image", tag = "whiteboards",
    params(("token" = String, Path, description = "`share_token` do quadro.")),
    responses(
        (status = 200, body = inline(PngBytes), content_type = "image/png"),
        (status = 404, description = "Token inexistente ou quadro não público.", body = crate::openapi::ErrorBody),
    )
)]
pub async fn shared_png(
    State(state): State<Arc<AppState>>,
    Path(token): Path<String>,
) -> Result<impl IntoResponse, ApiError> {
    let row: (Vec<u8>, bool) =
        sqlx::query_as("SELECT png, is_public FROM whiteboards WHERE share_token = $1")
            .bind(token)
            .fetch_one(&state.db)
            .await?;
    if !row.1 {
        return Err(ApiError::NotFound);
    }
    Ok(([(header::CONTENT_TYPE, "image/png")], row.0))
}
