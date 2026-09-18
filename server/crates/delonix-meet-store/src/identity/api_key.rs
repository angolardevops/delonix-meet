//! Repositório de `org_api_keys`. As regras (escopos, expiração, catálogo)
//! ficam em `delonix_meet_domain::identity::api_key`; aqui só está a
//! travessia da tabela.

use chrono::{DateTime, Utc};
use sqlx::PgPool;
use uuid::Uuid;

/// Uma chave `dlx_` tal como está gravada. `is_usable` delega inteiramente
/// na regra do domínio — este tipo não decide, só transporta a linha.
#[derive(Clone, Debug, sqlx::FromRow)]
pub struct StoredKey {
    pub id: Uuid,
    pub org_id: Uuid,
    pub created_by: Uuid,
    pub scopes: Vec<String>,
    pub expires_at: Option<DateTime<Utc>>,
    pub last_used_at: Option<DateTime<Utc>>,
}

impl StoredKey {
    pub fn is_usable(&self, now: DateTime<Utc>) -> bool {
        delonix_meet_domain::identity::api_key::ensure_not_expired(self.expires_at, now).is_ok()
    }
}

/// Uma linha da lista de chaves da organização — nunca a chave nem o hash.
#[derive(Clone, Debug, sqlx::FromRow)]
pub struct KeySummary {
    pub id: Uuid,
    pub name: String,
    pub prefix: String,
    pub created_at: DateTime<Utc>,
    pub last_used_at: Option<DateTime<Utc>>,
    pub scopes: Vec<String>,
    pub expires_at: Option<DateTime<Utc>>,
}

/// Procura a chave pelo SHA-256 apresentado. `None` = chave desconhecida ou
/// revogada — quem chama não distingue os dois casos (não há o que dizer a
/// mais a quem apresenta uma chave errada).
pub async fn find_by_hash(db: &PgPool, key_hash: &str) -> Result<Option<StoredKey>, sqlx::Error> {
    sqlx::query_as(
        "SELECT id, org_id, created_by, scopes, expires_at, last_used_at
         FROM org_api_keys WHERE key_hash = $1",
    )
    .bind(key_hash)
    .fetch_optional(db)
    .await
}

/// Regista o uso, no máximo uma vez por `throttle_secs` por chave. A guarda
/// repete-se no `WHERE` para que dois nós com a mesma leitura antiga não
/// escrevam os dois — a mesma razão por trás do padrão em `AppState`.
pub async fn touch_last_used(
    db: &PgPool,
    id: Uuid,
    throttle_secs: f64,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE org_api_keys SET last_used_at = now()
         WHERE id = $1
           AND (last_used_at IS NULL
                OR last_used_at <= now() - make_interval(secs => $2))",
    )
    .bind(id)
    .bind(throttle_secs)
    .execute(db)
    .await?;
    Ok(())
}

/// Chaves da organização, mais recente primeiro.
pub async fn list_for_org(db: &PgPool, org_id: Uuid) -> Result<Vec<KeySummary>, sqlx::Error> {
    sqlx::query_as(
        "SELECT id, name, prefix, created_at, last_used_at, scopes, expires_at
         FROM org_api_keys WHERE org_id = $1 ORDER BY created_at DESC",
    )
    .bind(org_id)
    .fetch_all(db)
    .await
}

/// Grava uma chave nova. Quem chama já gerou o segredo e o SHA-256 — aqui só
/// se persiste a linha.
#[allow(clippy::too_many_arguments)]
pub async fn insert(
    db: &PgPool,
    org_id: Uuid,
    name: &str,
    prefix: &str,
    key_hash: &str,
    created_by: Uuid,
    scopes: &[&str],
    expires_at: Option<DateTime<Utc>>,
) -> Result<Uuid, sqlx::Error> {
    let (id,): (Uuid,) = sqlx::query_as(
        "INSERT INTO org_api_keys (org_id, name, prefix, key_hash, created_by, scopes, expires_at)
         VALUES ($1, $2, $3, $4, $5, $6, $7) RETURNING id",
    )
    .bind(org_id)
    .bind(name)
    .bind(prefix)
    .bind(key_hash)
    .bind(created_by)
    .bind(scopes)
    .bind(expires_at)
    .fetch_one(db)
    .await?;
    Ok(id)
}

/// Apaga a chave se pertencer à organização. Devolve se algo saiu — quem
/// chama decide o 404, não este módulo.
pub async fn delete(db: &PgPool, key_id: Uuid, org_id: Uuid) -> Result<bool, sqlx::Error> {
    let deleted = sqlx::query("DELETE FROM org_api_keys WHERE id = $1 AND org_id = $2")
        .bind(key_id)
        .bind(org_id)
        .execute(db)
        .await?
        .rows_affected();
    Ok(deleted > 0)
}
