//! Segredos de integração EM REPOUSO (auditoria 2026-09-16, S5).
//!
//! Três colunas guardavam credenciais de terceiros em claro:
//!
//! | Tabela.coluna | Quem a lê de volta |
//! |---|---|
//! | `org_webhooks.secret` | a assinatura HMAC de cada entrega (`webhooks::attempt`) |
//! | `org_sso_configs.client_secret` | o fluxo OIDC (`auth::sso_login`/`sso_callback`) |
//! | `platform_storage.webdav_password` | o teste de ligação WebDAV (`storage::test_storage`) |
//!
//! Não podem ser hash (o servidor tem de as voltar a usar): cifram-se com a
//! `core::secret_box`, com o `aad` `<tabela>.<coluna>:<id da linha>` — copiar o
//! valor cifrado para outra linha falha a autenticação.
//!
//! **Compatibilidade.** Uma instalação existente tem linhas em claro e pode não
//! ter `DATA_ENCRYPTION_KEYS`. Por isso:
//! - ESCREVER um segredo sem chaves é recusado (`422
//!   secrets.encryption_unconfigured`) — nunca se grava um segredo NOVO em claro;
//! - LER um valor herdado em claro continua a funcionar, com ou sem chaves;
//! - ler um valor CIFRADO sem chaves é `500` com log de erro — a chave foi
//!   retirada da configuração, e nenhum segredo alheio sai por isso;
//! - com chaves, `reseal_legacy` (no arranque e de hora a hora) cifra as linhas
//!   herdadas.

use std::fmt::Display;

use delonix_meet_core::{secret_box::SecretBox, DomainError};
use sqlx::PgPool;

use crate::{config::Config, error::ApiError};

/// `aad` de um segredo: `<tabela>.<coluna>:<id>`.
pub(crate) fn aad(table: &str, column: &str, id: impl Display) -> String {
    format!("{table}.{column}:{id}")
}

/// A caixa de cifra, ou `422 secrets.encryption_unconfigured` — para ESCRITAS.
pub(crate) fn require_box(config: &Config) -> Result<&SecretBox, ApiError> {
    config.secret_box.as_deref().ok_or_else(|| {
        DomainError::precondition(
            "secrets.encryption_unconfigured",
            "esta instalação não tem DATA_ENCRYPTION_KEYS — não se guardam segredos de terceiros em claro",
        )
        .into()
    })
}

/// Cifra `plain` para gravar em `<tabela>.<coluna>` da linha `id`.
pub(crate) fn seal(config: &Config, plain: &str, aad: &str) -> Result<String, ApiError> {
    Ok(require_box(config)?.seal(plain, aad))
}

/// Lê um segredo guardado. Vazio fica vazio; texto herdado em claro passa como
/// está (com ou sem chaves); cifrado sem chaves, ou com o contexto errado, é
/// erro interno — nunca o valor de outra linha.
pub(crate) fn open(config: &Config, stored: &str, aad: &str) -> Result<String, ApiError> {
    if stored.is_empty() {
        return Ok(String::new());
    }
    match config.secret_box.as_deref() {
        Some(sb) => sb.open(stored, aad).map_err(|e| {
            tracing::error!(context = %aad, "segredo cifrado não abre (chave retirada ou valor copiado de outra linha)");
            e.into()
        }),
        None if SecretBox::is_sealed(stored) => {
            tracing::error!(
                context = %aad,
                "segredo cifrado em repouso mas DATA_ENCRYPTION_KEYS não está configurado"
            );
            Err(DomainError::internal("segredo cifrado sem DATA_ENCRYPTION_KEYS").into())
        }
        None => Ok(stored.to_string()),
    }
}

/// Linhas cifradas por `reseal_legacy`, por coluna.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct ResealReport {
    pub webhook_secrets: u64,
    pub sso_client_secrets: u64,
    pub webdav_passwords: u64,
}

impl ResealReport {
    pub fn total(&self) -> u64 {
        self.webhook_secrets + self.sso_client_secrets + self.webdav_passwords
    }
}

/// Tamanho de cada lote do `reseal_legacy`.
const BATCH: i64 = 200;

/// Uma coluna com segredos: a query dos herdados e o `UPDATE` condicional.
struct Column {
    table: &'static str,
    column: &'static str,
    /// `SELECT <id>::text, <coluna>` das linhas herdadas (não vazias, sem `enc:v1:`).
    select: &'static str,
    /// `UPDATE … SET <coluna> = $1 WHERE <id> = $2::<tipo> AND <coluna> = $3`.
    update: &'static str,
}

const COLUMNS: [Column; 3] = [
    Column {
        table: "org_webhooks",
        column: "secret",
        select: "SELECT id::text, secret FROM org_webhooks
                  WHERE secret <> '' AND secret NOT LIKE 'enc:v1:%' ORDER BY id LIMIT $1",
        update: "UPDATE org_webhooks SET secret = $1 WHERE id = $2::uuid AND secret = $3",
    },
    Column {
        table: "org_sso_configs",
        column: "client_secret",
        select: "SELECT org_id::text, client_secret FROM org_sso_configs
                  WHERE client_secret <> '' AND client_secret NOT LIKE 'enc:v1:%' ORDER BY org_id LIMIT $1",
        update: "UPDATE org_sso_configs SET client_secret = $1
                  WHERE org_id = $2::uuid AND client_secret = $3",
    },
    Column {
        table: "platform_storage",
        column: "webdav_password",
        select: "SELECT id::text, webdav_password FROM platform_storage
                  WHERE webdav_password <> '' AND webdav_password NOT LIKE 'enc:v1:%' ORDER BY id LIMIT $1",
        update: "UPDATE platform_storage SET webdav_password = $1
                  WHERE id = $2::int AND webdav_password = $3",
    },
];

/// Cifra os segredos herdados em claro das três colunas. Idempotente: uma
/// linha já cifrada não é seleccionada. Em lotes de `BATCH`. O `UPDATE` só
/// escreve se o valor ainda for o lido — uma escrita concorrente (já cifrada)
/// não é pisada.
pub async fn reseal_legacy(db: &PgPool, sb: &SecretBox) -> Result<ResealReport, sqlx::Error> {
    let mut counts = [0u64; 3];
    for (count, col) in counts.iter_mut().zip(&COLUMNS) {
        let mut sealed = 0u64;
        loop {
            let rows: Vec<(String, String)> =
                sqlx::query_as(col.select).bind(BATCH).fetch_all(db).await?;
            if rows.is_empty() {
                break;
            }
            let mut in_batch = 0u64;
            for (id, plain) in &rows {
                let value = sb.seal(plain, &aad(col.table, col.column, id));
                in_batch += sqlx::query(col.update)
                    .bind(&value)
                    .bind(id)
                    .bind(plain)
                    .execute(db)
                    .await?
                    .rows_affected();
            }
            sealed += in_batch;
            // Nenhuma linha mudou neste lote: só escritas concorrentes; parar
            // em vez de rodar sobre o mesmo lote.
            if in_batch == 0 || (rows.len() as i64) < BATCH {
                break;
            }
        }
        *count = sealed;
    }
    let [webhook_secrets, sso_client_secrets, webdav_passwords] = counts;
    Ok(ResealReport {
        webhook_secrets,
        sso_client_secrets,
        webdav_passwords,
    })
}

/// Quantos segredos herdados continuam em claro (as três colunas somadas).
pub async fn count_legacy(db: &PgPool) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT
           (SELECT COUNT(*) FROM org_webhooks
             WHERE secret <> '' AND secret NOT LIKE 'enc:v1:%')
         + (SELECT COUNT(*) FROM org_sso_configs
             WHERE client_secret <> '' AND client_secret NOT LIKE 'enc:v1:%')
         + (SELECT COUNT(*) FROM platform_storage
             WHERE webdav_password <> '' AND webdav_password NOT LIKE 'enc:v1:%')",
    )
    .fetch_one(db)
    .await
}

/// Uma passagem da tarefa de fundo: com chaves, cifra e regista quantas; sem
/// chaves, avisa quantas continuam em claro (não há com que cifrar).
pub(crate) async fn reseal_pass(db: &PgPool, config: &Config) {
    match config.secret_box.as_deref() {
        Some(sb) => match reseal_legacy(db, sb).await {
            Ok(r) if r.total() > 0 => tracing::info!(
                webhook_secrets = r.webhook_secrets,
                sso_client_secrets = r.sso_client_secrets,
                webdav_passwords = r.webdav_passwords,
                "segredos herdados cifrados em repouso"
            ),
            Ok(_) => {}
            Err(e) => tracing::warn!(error = %e, "cifra dos segredos herdados falhou"),
        },
        None => match count_legacy(db).await {
            Ok(n) if n > 0 => tracing::warn!(
                em_claro = n,
                "há segredos de integração em claro e DATA_ENCRYPTION_KEYS não está configurado"
            ),
            Ok(_) => {}
            Err(e) => tracing::warn!(error = %e, "contagem dos segredos herdados falhou"),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(with_box: bool) -> Config {
        let mut c = Config::from_map(&std::collections::HashMap::from([(
            "DELONIX_ALLOW_INSECURE",
            "1",
        )]));
        if !with_box {
            c.secret_box = None;
        }
        c
    }

    #[test]
    fn open_passes_legacy_plaintext_with_and_without_keys() {
        for with_box in [true, false] {
            assert_eq!(
                open(&config(with_box), "antigo", "t.c:1").unwrap(),
                "antigo"
            );
            assert_eq!(open(&config(with_box), "", "t.c:1").unwrap(), "");
        }
    }

    #[test]
    fn sealed_value_without_keys_or_in_another_row_does_not_open() {
        let c = config(true);
        let sealed = seal(&c, "s3gredo", &aad("t", "c", 1)).unwrap();
        assert!(sealed.starts_with("enc:v1:"));
        assert_eq!(open(&c, &sealed, "t.c:1").unwrap(), "s3gredo");
        assert!(open(&c, &sealed, "t.c:2").is_err());
        assert!(open(&config(false), &sealed, "t.c:1").is_err());
        assert!(seal(&config(false), "x", "t.c:1").is_err());
    }
}
