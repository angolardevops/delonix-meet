use argon2::{
    password_hash::{rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2,
};
use axum::{
    extract::{FromRequestParts, State},
    http::request::Parts,
    Json,
};
use chrono::Utc;
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::sync::Arc;
use uuid::Uuid;

use crate::{error::ApiError, AppState};

// ---------- JWT ----------

#[derive(Debug, Serialize, Deserialize)]
pub struct Claims {
    pub sub: Uuid,
    pub typ: String, // "access" | "room"
    pub exp: i64,
    pub iat: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub room: Option<Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub topo: Option<String>,
    /// Room token: este utilizador é o dono da sala (anfitrião).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub owner: bool,
    /// Room token: a sala tem sala de espera ativa.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub wait: bool,
}

pub fn sign_jwt(secret: &str, claims: &Claims) -> Result<String, ApiError> {
    encode(
        &Header::default(),
        claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    )
    .map_err(ApiError::internal)
}

pub fn verify_jwt(secret: &str, token: &str, expected_typ: &str) -> Result<Claims, ApiError> {
    let data = decode::<Claims>(
        token,
        &DecodingKey::from_secret(secret.as_bytes()),
        &Validation::default(),
    )
    .map_err(|_| ApiError::Unauthorized)?;
    if data.claims.typ != expected_typ {
        return Err(ApiError::Unauthorized);
    }
    Ok(data.claims)
}

pub fn access_token(state: &AppState, user_id: Uuid) -> Result<String, ApiError> {
    let now = Utc::now().timestamp();
    sign_jwt(
        &state.config.jwt_secret,
        &Claims {
            sub: user_id,
            typ: "access".into(),
            iat: now,
            exp: now + state.config.access_ttl_secs,
            room: None,
            name: None,
            topo: None,
            owner: false,
            wait: false,
        },
    )
}

// ---------- Passwords ----------

pub fn hash_password(password: &str) -> Result<String, ApiError> {
    let salt = SaltString::generate(&mut OsRng);
    Ok(Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map_err(ApiError::internal)?
        .to_string())
}

pub fn verify_password(password: &str, hash: &str) -> bool {
    PasswordHash::new(hash)
        .map(|parsed| {
            Argon2::default()
                .verify_password(password.as_bytes(), &parsed)
                .is_ok()
        })
        .unwrap_or(false)
}

// ---------- Refresh tokens ----------

pub fn new_refresh_token() -> (String, String) {
    let mut bytes = [0u8; 32];
    OsRng.fill_bytes(&mut bytes);
    let token = hex::encode(bytes);
    (token.clone(), hash_refresh_token(&token))
}

pub fn hash_refresh_token(token: &str) -> String {
    hex::encode(Sha256::digest(token.as_bytes()))
}

// ---------- Extractor ----------

/// Authenticated user, extracted from `Authorization: Bearer <access token>`.
pub struct AuthUser {
    pub user_id: Uuid,
}

impl FromRequestParts<Arc<AppState>> for AuthUser {
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &Arc<AppState>,
    ) -> Result<Self, Self::Rejection> {
        let header = parts
            .headers
            .get(axum::http::header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .ok_or(ApiError::Unauthorized)?;
        let token = header.strip_prefix("Bearer ").ok_or(ApiError::Unauthorized)?;
        let claims = verify_jwt(&state.config.jwt_secret, token, "access")?;
        Ok(AuthUser { user_id: claims.sub })
    }
}

// ---------- Handlers ----------

#[derive(Deserialize)]
pub struct RegisterReq {
    pub email: String,
    pub username: String,
    pub password: String,
}

#[derive(Deserialize)]
pub struct LoginReq {
    pub email: String,
    pub password: String,
}

#[derive(Deserialize)]
pub struct RefreshReq {
    pub refresh_token: String,
}

#[derive(Serialize)]
pub struct TokenPair {
    pub access_token: String,
    pub refresh_token: String,
    pub user: crate::users::UserPublic,
}

async fn issue_tokens(state: &AppState, user: crate::users::UserPublic) -> Result<TokenPair, ApiError> {
    let access = access_token(state, user.id)?;
    let (refresh, refresh_hash) = new_refresh_token();
    let expires = Utc::now() + chrono::Duration::seconds(state.config.refresh_ttl_secs);
    sqlx::query("INSERT INTO refresh_tokens (token_hash, user_id, expires_at) VALUES ($1, $2, $3)")
        .bind(&refresh_hash)
        .bind(user.id)
        .bind(expires)
        .execute(&state.db)
        .await?;
    Ok(TokenPair { access_token: access, refresh_token: refresh, user })
}

pub async fn register(
    State(state): State<Arc<AppState>>,
    Json(req): Json<RegisterReq>,
) -> Result<Json<TokenPair>, ApiError> {
    let email = req.email.trim().to_lowercase();
    let username = req.username.trim().to_string();
    if !email.contains('@') || email.len() > 254 {
        return Err(ApiError::BadRequest("invalid email".into()));
    }
    if username.len() < 2 || username.len() > 32 {
        return Err(ApiError::BadRequest("username must be 2-32 chars".into()));
    }
    if req.password.len() < 8 {
        return Err(ApiError::BadRequest("password must be at least 8 chars".into()));
    }

    let password_hash = hash_password(&req.password)?;
    let row: Result<crate::users::UserPublic, sqlx::Error> = sqlx::query_as(
        "INSERT INTO users (email, username, password_hash) VALUES ($1, $2, $3)
         RETURNING id, email, username, created_at",
    )
    .bind(&email)
    .bind(&username)
    .bind(&password_hash)
    .fetch_one(&state.db)
    .await;

    let user = row.map_err(|e| match &e {
        sqlx::Error::Database(db) if db.is_unique_violation() => {
            ApiError::Conflict("email or username already taken".into())
        }
        _ => e.into(),
    })?;

    Ok(Json(issue_tokens(&state, user).await?))
}

pub async fn login(
    State(state): State<Arc<AppState>>,
    Json(req): Json<LoginReq>,
) -> Result<Json<TokenPair>, ApiError> {
    let email = req.email.trim().to_lowercase();
    let row: Option<(Uuid, String, String, String, chrono::DateTime<Utc>)> = sqlx::query_as(
        "SELECT id, email, username, password_hash, created_at FROM users WHERE email = $1",
    )
    .bind(&email)
    .fetch_optional(&state.db)
    .await?;

    // Verify against a dummy hash when the user doesn't exist so timing
    // doesn't leak account existence.
    const DUMMY: &str = "$argon2id$v=19$m=19456,t=2,p=1$YWFhYWFhYWFhYWFhYWFhYQ$m6vRnxkbG10eB0QdjqfLd8Y6M3holKAAvfeFXTiXBdU";
    match row {
        Some((id, email, username, password_hash, created_at))
            if verify_password(&req.password, &password_hash) =>
        {
            let user = crate::users::UserPublic { id, email, username, created_at };
            Ok(Json(issue_tokens(&state, user).await?))
        }
        _ => {
            let _ = verify_password(&req.password, DUMMY);
            Err(ApiError::Unauthorized)
        }
    }
}

pub async fn refresh(
    State(state): State<Arc<AppState>>,
    Json(req): Json<RefreshReq>,
) -> Result<Json<TokenPair>, ApiError> {
    let hash = hash_refresh_token(&req.refresh_token);
    let row: Option<(Uuid,)> = sqlx::query_as(
        "SELECT user_id FROM refresh_tokens
         WHERE token_hash = $1 AND NOT revoked AND expires_at > now()",
    )
    .bind(&hash)
    .fetch_optional(&state.db)
    .await?;
    let (user_id,) = row.ok_or(ApiError::Unauthorized)?;

    // Rotate: revoke the used token, issue a fresh pair.
    sqlx::query("UPDATE refresh_tokens SET revoked = TRUE WHERE token_hash = $1")
        .bind(&hash)
        .execute(&state.db)
        .await?;

    let user = crate::users::fetch_public(&state.db, user_id).await?;
    Ok(Json(issue_tokens(&state, user).await?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn password_hash_roundtrip() {
        let hash = hash_password("s3cret-password").unwrap();
        assert!(verify_password("s3cret-password", &hash));
        assert!(!verify_password("wrong", &hash));
    }

    #[test]
    fn jwt_roundtrip_and_type_check() {
        let secret = "test-secret";
        let now = Utc::now().timestamp();
        let user = Uuid::new_v4();
        let token = sign_jwt(
            secret,
            &Claims {
                sub: user,
                typ: "access".into(),
                iat: now,
                exp: now + 60,
                room: None,
                name: None,
                topo: None,
                owner: false,
                wait: false,
            },
        )
        .unwrap();
        let claims = verify_jwt(secret, &token, "access").unwrap();
        assert_eq!(claims.sub, user);
        // Wrong expected type must fail (an access token is not a room token).
        assert!(verify_jwt(secret, &token, "room").is_err());
        // Tampered secret must fail.
        assert!(verify_jwt("other-secret", &token, "access").is_err());
    }

    #[test]
    fn expired_jwt_rejected() {
        let secret = "test-secret";
        let now = Utc::now().timestamp();
        let token = sign_jwt(
            secret,
            &Claims {
                sub: Uuid::new_v4(),
                typ: "access".into(),
                iat: now - 3600,
                exp: now - 120,
                room: None,
                name: None,
                topo: None,
                owner: false,
                wait: false,
            },
        )
        .unwrap();
        assert!(verify_jwt(secret, &token, "access").is_err());
    }

    #[test]
    fn refresh_token_hash_is_stable_and_opaque() {
        let (token, hash) = new_refresh_token();
        assert_eq!(hash, hash_refresh_token(&token));
        assert_ne!(token, hash);
        assert_eq!(token.len(), 64);
    }
}
