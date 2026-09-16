use axum::{
    extract::{Query, State},
    Json,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use std::sync::Arc;
use uuid::Uuid;

use crate::{auth::AuthUser, error::ApiError, AppState};

#[derive(Debug, Serialize, sqlx::FromRow, utoipa::ToSchema)]
pub struct UserPublic {
    pub id: Uuid,
    pub email: String,
    pub username: String,
    pub created_at: DateTime<Utc>,
    #[serde(default)]
    pub locale: String,
}

/// Lista de colunas que cobre todos os campos de `UserPublic` — usar sempre
/// que se hidrata `UserPublic`. Estava copiada à mão em quatro sítios (aqui e
/// três em `auth.rs`) — mesmo padrão de risco de `meetings::MEETING_COLUMNS`
/// (ver ADR-0004).
pub const USER_PUBLIC_COLUMNS: &str =
    "id, email, username, created_at, COALESCE(locale, 'pt') AS locale";

pub async fn fetch_public(db: &PgPool, user_id: Uuid) -> Result<UserPublic, ApiError> {
    Ok(sqlx::query_as::<_, UserPublic>(&format!(
        "SELECT {USER_PUBLIC_COLUMNS} FROM users WHERE id = $1"
    ))
    .bind(user_id)
    .fetch_one(db)
    .await?)
}

/// Documentação OpenAPI das rotas deste módulo (`openapi.rs` junta-as).
#[derive(utoipa::OpenApi)]
#[openapi(
    paths(me, update_me, search, my_room, update_my_room, rotate_my_room_code),
    components(schemas(UserPublic, UpdateMeReq, PersonalRoom, UpdatePersonalRoomReq))
)]
pub struct ApiDoc;

/// O perfil de quem está autenticado.
#[utoipa::path(
    get, path = "/api/users/me", tag = "users",
    security(("session" = [])),
    responses(
        (status = 200, body = UserPublic),
        (status = 401, body = crate::openapi::ErrorBody),
    )
)]
pub async fn me(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
) -> Result<Json<UserPublic>, ApiError> {
    Ok(Json(fetch_public(&state.db, auth.user_id).await?))
}

#[derive(Deserialize, utoipa::ToSchema)]
pub struct UpdateMeReq {
    pub username: Option<String>,
    pub password: Option<String>,
    pub locale: Option<String>,
}

/// Atualiza os próprios dados: username e/ou password (cada campo é opcional).
#[utoipa::path(
    patch, path = "/api/users/me", tag = "users",
    security(("session" = [])),
    request_body = UpdateMeReq,
    responses(
        (status = 200, body = UserPublic),
        (status = 400, body = crate::openapi::ErrorBody),
        (status = 401, body = crate::openapi::ErrorBody),
    )
)]
pub async fn update_me(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Json(req): Json<UpdateMeReq>,
) -> Result<Json<UserPublic>, ApiError> {
    if let Some(raw) = req.username.as_deref() {
        let username = raw.trim();
        if username.is_empty() || username.len() > 40 {
            return Err(ApiError::BadRequest(
                "username deve ter 1-40 caracteres".into(),
            ));
        }
        sqlx::query("UPDATE users SET username = $1 WHERE id = $2")
            .bind(username)
            .bind(auth.user_id)
            .execute(&state.db)
            .await?;
    }
    if let Some(password) = req.password.as_deref() {
        // Antes desta chamada faltava aqui o tecto de 128 que auth::register
        // já impunha — a mesma política de password, agora num só sítio
        // (ADR-0004, Fase 2).
        delonix_meet_domain::identity::validation::validate_password(password)
            .map_err(ApiError::BadRequest)?;
        let hash = crate::auth::hash_password(password)?;
        sqlx::query("UPDATE users SET password_hash = $1 WHERE id = $2")
            .bind(hash)
            .bind(auth.user_id)
            .execute(&state.db)
            .await?;
    }
    if let Some(locale) = req.locale.as_deref() {
        let locale = locale.trim();
        if matches!(locale, "pt" | "en" | "fr") {
            sqlx::query("UPDATE users SET locale = $1 WHERE id = $2")
                .bind(locale)
                .bind(auth.user_id)
                .execute(&state.db)
                .await?;
        }
    }
    Ok(Json(fetch_public(&state.db, auth.user_id).await?))
}

#[derive(Deserialize, utoipa::IntoParams)]
#[into_params(parameter_in = Query)]
pub struct SearchQuery {
    /// Termo (email ou username), 2+ caracteres.
    pub q: String,
}

/// Pesquisa utilizadores por email/username (para convidar/partilhar).
/// Devolve no máximo 10; exclui o próprio.
#[utoipa::path(
    get, path = "/api/users", tag = "users",
    security(("session" = [])),
    params(SearchQuery),
    responses(
        (status = 200, body = Vec<UserPublic>),
        (status = 401, body = crate::openapi::ErrorBody),
    )
)]
pub async fn search(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Query(q): Query<SearchQuery>,
) -> Result<Json<Vec<UserPublic>>, ApiError> {
    let term = q.q.trim();
    if term.len() < 2 {
        return Ok(Json(vec![]));
    }
    let pattern = format!("%{}%", term.to_lowercase());
    // Isolamento multi-tenant: só encontra utilizadores que partilham uma
    // organização com quem pesquisa (não vaza o diretório de outras empresas).
    // Membros ACTIVOS dos dois lados, como em `org::org_co_members` (S3).
    let users = sqlx::query_as::<_, UserPublic>(
        // `locale` é campo de `UserPublic`: sem ele esta rota devolvia SEMPRE 500
        // («no column found for name: locale») — o SQL de runtime não o apanha
        // na compilação. Encontrado pelo controlo positivo do teste S3.
        "SELECT u.id, u.email, u.username, u.created_at, COALESCE(u.locale, 'pt') AS locale
         FROM users u
         WHERE u.id <> $1 AND (lower(u.username) LIKE $2 OR lower(u.email) LIKE $2)
           AND EXISTS (SELECT 1 FROM org_members a JOIN org_members b ON a.org_id = b.org_id
                       WHERE a.user_id = $1 AND b.user_id = u.id
                         AND a.archived_at IS NULL AND b.archived_at IS NULL)
         ORDER BY u.username LIMIT 10",
    )
    .bind(auth.user_id)
    .bind(&pattern)
    .fetch_all(&state.db)
    .await?;
    Ok(Json(users))
}

// ---------- «A minha sala» (G2) ----------
//
// - `GET   /api/users/me/room`              a sala pessoal; criada na primeira chamada
// - `PATCH /api/users/me/room`              nome e/ou sala de espera
// - `POST  /api/users/me/room/rotate-code`  método personalizado: código novo, o antigo deixa de abrir
//
// Só o próprio: não há caminho com o id de outra pessoa, por isso não há como
// tocar na sala de outro. Entrar na sala segue `rooms::room_access`, como
// qualquer outra.

/// A sala pessoal, o link para a partilhar e o dial-in, se houver.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct PersonalRoom {
    #[serde(flatten)]
    pub room: crate::rooms::Room,
    /// Link partilhável (`https://<domínio da org>/#/r/<código>`, ou relativo
    /// se a organização não tem domínio).
    pub join_url: String,
    /// Número e PIN, se uma organização do dono tiver uma sala de voz ACTIVA
    /// ligada a este código. Só leitura: esta rota não cria DIDs.
    pub dial_in: Option<crate::voice::DialIn>,
}

/// Campos alteráveis. Um campo desconhecido é recusado (`422`): um campo que o
/// cliente escreve e o servidor ignora é pior do que não existir.
#[derive(Debug, Deserialize, Default, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdatePersonalRoomReq {
    /// 1–100 caracteres (depois de `trim`).
    pub name: Option<String>,
    pub waiting_room: Option<bool>,
}

async fn personal_room_view(
    state: &AppState,
    user_id: Uuid,
    room: crate::rooms::Room,
) -> Result<PersonalRoom, ApiError> {
    let orgs = crate::org::orgs_of_user(state, user_id).await;
    let join_url = match orgs.first() {
        Some(org) => crate::apikeys::room_link(state, *org, &room.code).await,
        None => format!("/#/r/{}", room.code),
    };
    let dial_in = crate::voice::dial_in_for_room(state, &orgs, &room.code).await?;
    Ok(PersonalRoom {
        room,
        join_url,
        dial_in,
    })
}

async fn ensure_my_room(state: &AppState, user_id: Uuid) -> Result<crate::rooms::Room, ApiError> {
    let user = fetch_public(&state.db, user_id).await?;
    let default_name =
        delonix_meet_domain::conferencing::personal_room::default_name(&user.username);
    crate::rooms::ensure_personal_room(&state.db, user_id, &default_name).await
}

/// A sala pessoal de quem está autenticado. Na primeira chamada é criada (com
/// a sala de espera ligada); as seguintes devolvem sempre a mesma.
#[utoipa::path(
    get, path = "/api/users/me/room", tag = "users",
    security(("session" = [])),
    responses(
        (status = 200, body = PersonalRoom),
        (status = 401, body = crate::openapi::ErrorBody),
    )
)]
pub async fn my_room(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
) -> Result<Json<PersonalRoom>, ApiError> {
    let room = ensure_my_room(&state, auth.user_id).await?;
    Ok(Json(personal_room_view(&state, auth.user_id, room).await?))
}

/// Altera o nome e/ou a sala de espera. Valida tudo antes de escrever.
#[utoipa::path(
    patch, path = "/api/users/me/room", tag = "users",
    security(("session" = [])),
    request_body = UpdatePersonalRoomReq,
    responses(
        (status = 200, body = PersonalRoom),
        (status = 400, description = "`personal_room.invalid_name`.", body = crate::openapi::ErrorBody),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 422, description = "Corpo que não desserializa, incluindo um campo desconhecido.", body = crate::openapi::ErrorBody),
    )
)]
pub async fn update_my_room(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Json(req): Json<UpdatePersonalRoomReq>,
) -> Result<Json<PersonalRoom>, ApiError> {
    let name = req
        .name
        .as_deref()
        .map(delonix_meet_domain::conferencing::personal_room::validate_name)
        .transpose()?;
    ensure_my_room(&state, auth.user_id).await?;
    let room = crate::rooms::update_personal_room(
        &state.db,
        auth.user_id,
        name.as_deref(),
        req.waiting_room,
    )
    .await?;
    Ok(Json(personal_room_view(&state, auth.user_id, room).await?))
}

/// Método personalizado: dá à sala pessoal um código novo. O link antigo
/// deixa de abrir (`GET /api/rooms/{antigo}` → `404`). Um dial-in ligado ao
/// código antigo NÃO acompanha: fica no código antigo e deixa de aparecer.
#[utoipa::path(
    post, path = "/api/users/me/room/rotate-code", tag = "users",
    security(("session" = [])),
    responses(
        (status = 200, body = PersonalRoom),
        (status = 401, body = crate::openapi::ErrorBody),
    )
)]
pub async fn rotate_my_room_code(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
) -> Result<Json<PersonalRoom>, ApiError> {
    let old = ensure_my_room(&state, auth.user_id).await?;
    let room = crate::rooms::rotate_personal_room_code(&state.db, auth.user_id).await?;
    crate::audit::log(
        &state.db,
        None,
        auth.user_id,
        "personal_room.code_rotated",
        &old.id.to_string(),
    )
    .await;
    Ok(Json(personal_room_view(&state, auth.user_id, room).await?))
}
