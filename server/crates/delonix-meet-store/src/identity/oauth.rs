//! Repositório do OAuth 2.1 (ADR-0028): clientes, códigos, concessões e a
//! cadeia rotativa de refresh tokens.
//!
//! As regras (escopos, PKCE, forma do `redirect_uri`, prazos, o que conta como
//! roubo) ficam em `delonix_meet_domain::identity::oauth`; aqui só está a
//! travessia das tabelas. Nada decide: quem lê é que decide.
//!
//! Duas escritas são **atómicas de propósito** — gastar um código e gastar um
//! refresh token. Com dois nós a servir o mesmo cliente (ADR-0001), um
//! `SELECT` seguido de `UPDATE` deixava dois tokens nascer do mesmo código.

use chrono::{DateTime, Utc};
use sqlx::PgPool;
use uuid::Uuid;

// ---------- Clientes ----------

#[derive(Clone, Debug, sqlx::FromRow)]
pub struct Client {
    pub client_id: String,
    pub client_name: String,
    pub redirect_uris: Vec<String>,
    pub registered_dynamically: bool,
    pub disabled_at: Option<DateTime<Utc>>,
}

impl Client {
    pub fn is_usable(&self) -> bool {
        self.disabled_at.is_none()
    }
}

pub async fn find_client(db: &PgPool, client_id: &str) -> Result<Option<Client>, sqlx::Error> {
    sqlx::query_as(
        "SELECT client_id, client_name, redirect_uris, registered_dynamically, disabled_at
         FROM oauth_clients WHERE client_id = $1",
    )
    .bind(client_id)
    .fetch_optional(db)
    .await
}

pub async fn insert_client(
    db: &PgPool,
    client_id: &str,
    client_name: &str,
    redirect_uris: &[String],
    registered_dynamically: bool,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO oauth_clients (client_id, client_name, redirect_uris, registered_dynamically)
         VALUES ($1, $2, $3, $4)",
    )
    .bind(client_id)
    .bind(client_name)
    .bind(redirect_uris)
    .bind(registered_dynamically)
    .execute(db)
    .await?;
    Ok(())
}

// ---------- Códigos de autorização ----------

#[derive(Clone, Debug, sqlx::FromRow)]
pub struct AuthCode {
    pub client_id: String,
    pub user_id: Uuid,
    pub org_id: Uuid,
    pub scopes: Vec<String>,
    pub redirect_uri: String,
    pub code_challenge: String,
    pub resource: String,
}

#[allow(clippy::too_many_arguments)]
pub async fn insert_code(
    db: &PgPool,
    code_hash: &str,
    client_id: &str,
    user_id: Uuid,
    org_id: Uuid,
    scopes: &[String],
    redirect_uri: &str,
    code_challenge: &str,
    resource: &str,
    expires_at: DateTime<Utc>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO oauth_authorization_codes
             (code_hash, client_id, user_id, org_id, scopes, redirect_uri,
              code_challenge, resource, expires_at)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)",
    )
    .bind(code_hash)
    .bind(client_id)
    .bind(user_id)
    .bind(org_id)
    .bind(scopes)
    .bind(redirect_uri)
    .bind(code_challenge)
    .bind(resource)
    .bind(expires_at)
    .execute(db)
    .await?;
    Ok(())
}

/// Gasta o código e devolve o que ele autorizava. **Uma só escrita**: o
/// `consumed_at IS NULL AND expires_at > now()` está no `WHERE`, por isso dois
/// pedidos simultâneos com o mesmo código dão um sucesso e um `None`.
///
/// `None` cobre tudo o que falhou — desconhecido, expirado, já gasto. Quem
/// chama devolve sempre `oauth.invalid_grant`, sem dizer qual foi.
pub async fn consume_code(db: &PgPool, code_hash: &str) -> Result<Option<AuthCode>, sqlx::Error> {
    sqlx::query_as(
        "UPDATE oauth_authorization_codes
            SET consumed_at = now()
          WHERE code_hash = $1 AND consumed_at IS NULL AND expires_at > now()
      RETURNING client_id, user_id, org_id, scopes, redirect_uri, code_challenge, resource",
    )
    .bind(code_hash)
    .fetch_optional(db)
    .await
}

// ---------- Concessões ----------

#[derive(Clone, Debug, sqlx::FromRow)]
pub struct Grant {
    pub id: Uuid,
    pub client_id: String,
    pub client_name: String,
    pub org_id: Uuid,
    pub scopes: Vec<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// A concessão viva desta pessoa para este cliente nesta organização.
pub async fn find_grant(
    db: &PgPool,
    user_id: Uuid,
    client_id: &str,
    org_id: Uuid,
) -> Result<Option<Grant>, sqlx::Error> {
    sqlx::query_as(
        "SELECT g.id, g.client_id, c.client_name, g.org_id, g.scopes, g.created_at, g.updated_at
           FROM oauth_grants g JOIN oauth_clients c ON c.client_id = g.client_id
          WHERE g.user_id = $1 AND g.client_id = $2 AND g.org_id = $3
            AND g.revoked_at IS NULL",
    )
    .bind(user_id)
    .bind(client_id)
    .bind(org_id)
    .fetch_optional(db)
    .await
}

/// Grava o consentimento. Consentir outra vez **substitui** os escopos: o que
/// a pessoa viu no ecrã é o que fica, nunca a união com o que havia antes.
pub async fn upsert_grant(
    db: &PgPool,
    user_id: Uuid,
    client_id: &str,
    org_id: Uuid,
    scopes: &[String],
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO oauth_grants (user_id, client_id, org_id, scopes)
         VALUES ($1, $2, $3, $4)
         ON CONFLICT (user_id, client_id, org_id) WHERE revoked_at IS NULL
         DO UPDATE SET scopes = EXCLUDED.scopes, updated_at = now()",
    )
    .bind(user_id)
    .bind(client_id)
    .bind(org_id)
    .bind(scopes)
    .execute(db)
    .await?;
    Ok(())
}

/// Concessões vivas desta pessoa, para ela as ver e revogar.
pub async fn list_grants_for_user(db: &PgPool, user_id: Uuid) -> Result<Vec<Grant>, sqlx::Error> {
    sqlx::query_as(
        "SELECT g.id, g.client_id, c.client_name, g.org_id, g.scopes, g.created_at, g.updated_at
           FROM oauth_grants g JOIN oauth_clients c ON c.client_id = g.client_id
          WHERE g.user_id = $1 AND g.revoked_at IS NULL
          ORDER BY g.updated_at DESC",
    )
    .bind(user_id)
    .fetch_all(db)
    .await
}

/// Revoga a concessão desta pessoa (e só desta). Devolve se algo mudou — quem
/// chama decide o `404`.
pub async fn revoke_grant(db: &PgPool, user_id: Uuid, grant_id: Uuid) -> Result<bool, sqlx::Error> {
    let n = sqlx::query(
        "UPDATE oauth_grants SET revoked_at = now()
          WHERE id = $1 AND user_id = $2 AND revoked_at IS NULL",
    )
    .bind(grant_id)
    .bind(user_id)
    .execute(db)
    .await?
    .rows_affected();
    Ok(n > 0)
}

/// As sessões abertas por esta concessão — é o que se termina quando ela é
/// revogada, para o agente morrer no mesmo instante.
pub async fn sessions_of_grant(
    db: &PgPool,
    user_id: Uuid,
    grant_id: Uuid,
) -> Result<Vec<Uuid>, sqlx::Error> {
    let rows: Vec<(Uuid,)> = sqlx::query_as(
        "SELECT DISTINCT t.session_id
           FROM oauth_refresh_tokens t
           JOIN oauth_grants g
             ON g.client_id = t.client_id AND g.user_id = t.user_id AND g.org_id = t.org_id
          WHERE g.id = $1 AND g.user_id = $2 AND t.revoked_at IS NULL",
    )
    .bind(grant_id)
    .bind(user_id)
    .fetch_all(db)
    .await?;
    Ok(rows.into_iter().map(|(id,)| id).collect())
}

// ---------- Refresh tokens ----------

#[derive(Clone, Debug, sqlx::FromRow)]
pub struct StoredRefresh {
    pub family_id: Uuid,
    pub session_id: Uuid,
    pub user_id: Uuid,
    pub org_id: Uuid,
    pub client_id: String,
    pub scopes: Vec<String>,
    pub resource: String,
    pub expires_at: DateTime<Utc>,
    pub used_at: Option<DateTime<Utc>>,
    pub revoked_at: Option<DateTime<Utc>>,
}

#[allow(clippy::too_many_arguments)]
pub async fn insert_refresh(
    db: &PgPool,
    token_hash: &str,
    family_id: Uuid,
    session_id: Uuid,
    user_id: Uuid,
    org_id: Uuid,
    client_id: &str,
    scopes: &[String],
    resource: &str,
    expires_at: DateTime<Utc>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO oauth_refresh_tokens
             (token_hash, family_id, session_id, user_id, org_id, client_id,
              scopes, resource, expires_at)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)",
    )
    .bind(token_hash)
    .bind(family_id)
    .bind(session_id)
    .bind(user_id)
    .bind(org_id)
    .bind(client_id)
    .bind(scopes)
    .bind(resource)
    .bind(expires_at)
    .execute(db)
    .await?;
    Ok(())
}

/// Lê o token **sem o gastar**. É preciso ler primeiro porque um token já usado
/// obriga a revogar a família, e para isso há que saber qual é.
pub async fn find_refresh(
    db: &PgPool,
    token_hash: &str,
) -> Result<Option<StoredRefresh>, sqlx::Error> {
    sqlx::query_as(
        "SELECT family_id, session_id, user_id, org_id, client_id, scopes,
                resource, expires_at, used_at, revoked_at
           FROM oauth_refresh_tokens WHERE token_hash = $1",
    )
    .bind(token_hash)
    .fetch_optional(db)
    .await
}

/// Gasta o token. **Uma só escrita**, pela mesma razão do código: dois pedidos
/// com o mesmo refresh dão um sucesso e um `false` — e o `false` é que dispara
/// a revogação da família.
pub async fn mark_refresh_used(db: &PgPool, token_hash: &str) -> Result<bool, sqlx::Error> {
    let n = sqlx::query(
        "UPDATE oauth_refresh_tokens SET used_at = now()
          WHERE token_hash = $1 AND used_at IS NULL AND revoked_at IS NULL
            AND expires_at > now()",
    )
    .bind(token_hash)
    .execute(db)
    .await?
    .rows_affected();
    Ok(n > 0)
}

/// Revoga a família inteira: é a resposta a um refresh reapresentado.
pub async fn revoke_family(db: &PgPool, family_id: Uuid) -> Result<u64, sqlx::Error> {
    Ok(sqlx::query(
        "UPDATE oauth_refresh_tokens SET revoked_at = now()
          WHERE family_id = $1 AND revoked_at IS NULL",
    )
    .bind(family_id)
    .execute(db)
    .await?
    .rows_affected())
}

/// Revoga tudo o que uma sessão tinha. Chamado quando a sessão é terminada —
/// o agente não sobrevive à sessão que o abriu.
pub async fn revoke_for_session(db: &PgPool, session_id: Uuid) -> Result<u64, sqlx::Error> {
    Ok(sqlx::query(
        "UPDATE oauth_refresh_tokens SET revoked_at = now()
          WHERE session_id = $1 AND revoked_at IS NULL",
    )
    .bind(session_id)
    .execute(db)
    .await?
    .rows_affected())
}

/// Varre o que já não serve: códigos gastos ou expirados há mais de um dia, e
/// refresh tokens expirados. Sem isto, as duas tabelas só crescem.
pub async fn purge_expired(db: &PgPool) -> Result<u64, sqlx::Error> {
    let codes = sqlx::query(
        "DELETE FROM oauth_authorization_codes
          WHERE expires_at < now() - interval '1 day'",
    )
    .execute(db)
    .await?
    .rows_affected();
    let refreshes = sqlx::query(
        "DELETE FROM oauth_refresh_tokens
          WHERE expires_at < now() - interval '7 days'",
    )
    .execute(db)
    .await?
    .rows_affected();
    Ok(codes + refreshes)
}
