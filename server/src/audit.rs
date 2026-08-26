//! Registos de auditoria — trilha IMUTÁVEL e VERIFICÁVEL de eventos de
//! segurança e administração.
//!
//! **O que torna isto uma auditoria e não um log.** Cada linha inclui o hash da
//! anterior, numa cadeia por organização (migração 0037). Editar ou apagar uma
//! linha parte a cadeia, e a quebra é detectável — mesmo por quem não confia em
//! quem administra a base de dados, que é exactamente o adversário que uma
//! auditoria tem de considerar. Gatilhos recusam UPDATE e DELETE; a cadeia é a
//! defesa que sobrevive a quem tenha poder para os remover.
//!
//! **A escrita continua a não falhar a operação principal** — recusar um login
//! porque a auditoria está em baixo seria pior do que o problema. Mas deixou de
//! ser silenciosa: falhar é ERRO e conta em
//! `delonix_audit_write_failures_total`, para uma trilha partida ser alertável
//! em vez de ficar num aviso que ninguém lê.
//!
//! Leitura: `GET /api/orgs/{org_id}/audit` (admins da org).
//! Verificação: `GET /api/orgs/{org_id}/audit/verify`.

use axum::{
    extract::{Path, Query, State},
    Json,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use std::sync::Arc;
use uuid::Uuid;

use crate::{auth::AuthUser, error::ApiError, org::require_admin_pub, AppState};

/// Escreve um evento de auditoria. Não propaga erro — mas falha ALTO.
pub async fn log(db: &PgPool, org_id: Option<Uuid>, actor_id: Uuid, action: &str, target: &str) {
    log_com_metricas(db, None, org_id, actor_id, action, target).await
}

/// Igual, com os contadores de observabilidade. É a que os caminhos com
/// `AppState` à mão devem usar.
pub async fn log_com_metricas(
    db: &PgPool,
    metrics: Option<&crate::metrics::Metrics>,
    org_id: Option<Uuid>,
    actor_id: Uuid,
    action: &str,
    target: &str,
) {
    // Sem org explícita, resolve-se a do actor. Sem isto, os eventos de LOGIN
    // — precisamente os que mais interessam numa auditoria — caíam numa cadeia
    // sem org, que a verificação por organização não cobre: um administrador
    // podia verificar a sua trilha e receber «intacta» sem que os logins lá
    // estivessem sequer. `ORDER BY` fixo para a escolha ser determinista.
    let org_id = match org_id {
        Some(o) => Some(o),
        None => sqlx::query_scalar::<_, Uuid>(
            "SELECT org_id FROM org_members WHERE user_id = $1 AND archived_at IS NULL
             ORDER BY created_at, org_id LIMIT 1",
        )
        .bind(actor_id)
        .fetch_optional(db)
        .await
        .ok()
        .flatten(),
    };

    // O nome do actor é gravado NO MOMENTO. É o que mantém a linha legível
    // depois de a conta desaparecer — e é o nome que ele tinha ENTÃO, que é o
    // que uma auditoria quer, não o actual.
    let nome: String = sqlx::query_scalar("SELECT username FROM users WHERE id = $1")
        .bind(actor_id)
        .fetch_optional(db)
        .await
        .ok()
        .flatten()
        .unwrap_or_else(|| "(desconhecido)".into());

    let res = sqlx::query(
        "INSERT INTO audit_logs (org_id, actor_id, actor_name, action, target)
         VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(org_id)
    .bind(actor_id)
    .bind(&nome)
    .bind(action)
    .bind(target)
    .execute(db)
    .await;
    if let Err(e) = res {
        // ERRO e não aviso: uma trilha de auditoria que não escreve é uma
        // falha de conformidade em curso, não um detalhe operacional.
        tracing::error!(error = %e, action, "ESCRITA DE AUDITORIA FALHOU — a trilha está incompleta");
        if let Some(m) = metrics {
            crate::metrics::Metrics::bump(&m.audit_write_failures_total);
        }
    }
}

/// Resultado da verificação da cadeia de uma organização.
#[derive(Debug, Serialize)]
pub struct VerificacaoCadeia {
    /// A cadeia está intacta?
    pub intact: bool,
    /// Linhas verificadas.
    pub entries: i64,
    /// `seq` da primeira linha que não bate. `None` se estiver intacta.
    pub broken_at_seq: Option<i64>,
    /// O que se detectou, em linguagem de quem vai ler isto num relatório.
    pub detail: String,
}

/// Percorre a cadeia da org e diz se alguém lhe mexeu.
///
/// Recalcula o hash de CADA linha a partir do conteúdo dela e do hash da
/// anterior. Uma linha editada dá hash diferente; uma linha apagada parte a
/// ligação da seguinte. Qualquer das duas aparece aqui com o `seq` onde
/// começou — que é o que permite dizer «a partir daqui não confio».
pub async fn verificar_cadeia(db: &PgPool, org_id: Uuid) -> Result<VerificacaoCadeia, ApiError> {
    #[derive(sqlx::FromRow)]
    struct Linha {
        seq: i64,
        prev_hash: String,
        hash: String,
        esperado: String,
    }
    // O hash esperado é recalculado PELA BASE DE DADOS com a mesma função que
    // o gatilho usa. Duas implementações do material divergem, e uma
    // verificação que discorda do escritor acusa falsas quebras.
    let linhas: Vec<Linha> = sqlx::query_as(
        "SELECT seq, prev_hash, hash,
                encode(sha256(convert_to(
                    audit_material(seq, org_id, actor_id, actor_name, action, target,
                                   created_at, prev_hash), 'UTF8')), 'hex') AS esperado
           FROM audit_logs
          WHERE audit_chain_key(org_id) = $1
          ORDER BY seq",
    )
    .bind(org_id)
    .fetch_all(db)
    .await?;

    let mut anterior = "0".repeat(64);
    // A sequência começa em 1 e não pode ter saltos — é isso que denuncia uma
    // linha apagada, mesmo que as restantes estejam todas correctas.
    for (esperada_seq, l) in (1i64..).zip(linhas.iter()) {
        if l.seq != esperada_seq {
            return Ok(VerificacaoCadeia {
                intact: false,
                entries: linhas.len() as i64,
                broken_at_seq: Some(esperada_seq),
                detail: format!(
                    "Falta o registo nº {esperada_seq}: a numeração salta para {}. Alguém apagou linhas.",
                    l.seq
                ),
            });
        }
        if l.prev_hash != anterior {
            return Ok(VerificacaoCadeia {
                intact: false,
                entries: linhas.len() as i64,
                broken_at_seq: Some(l.seq),
                detail: format!(
                    "O registo nº {} não liga ao anterior — a cadeia foi cortada.",
                    l.seq
                ),
            });
        }
        if l.hash != l.esperado {
            return Ok(VerificacaoCadeia {
                intact: false,
                entries: linhas.len() as i64,
                broken_at_seq: Some(l.seq),
                detail: format!("O registo nº {} foi ALTERADO depois de escrito.", l.seq),
            });
        }
        anterior = l.hash.clone();
    }
    Ok(VerificacaoCadeia {
        intact: true,
        entries: linhas.len() as i64,
        broken_at_seq: None,
        detail: if linhas.is_empty() {
            "Ainda não há registos nesta organização.".into()
        } else {
            format!("{} registos verificados, cadeia intacta.", linhas.len())
        },
    })
}

/// `GET /api/orgs/{org_id}/audit/verify` — só admins da org.
pub async fn verify(
    State(state): State<Arc<AppState>>,
    Path(org_id): Path<Uuid>,
    auth: AuthUser,
) -> Result<Json<VerificacaoCadeia>, ApiError> {
    require_admin_pub(&state, org_id, auth.user_id).await?;
    Ok(Json(verificar_cadeia(&state.db, org_id).await?))
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct AuditEntry {
    pub id: i64,
    pub actor: String,
    pub action: String,
    pub target: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Deserialize)]
pub struct AuditQuery {
    #[serde(default)]
    pub limit: Option<i64>,
}

/// Últimos eventos de auditoria da org (só admins).
pub async fn list(
    State(state): State<Arc<AppState>>,
    Path(org_id): Path<Uuid>,
    Query(q): Query<AuditQuery>,
    auth: AuthUser,
) -> Result<Json<Vec<AuditEntry>>, ApiError> {
    require_admin_pub(&state, org_id, auth.user_id).await?;
    let limit = q.limit.unwrap_or(100).clamp(1, 500);
    // LEFT JOIN e `actor_name` como recuo: com o INNER JOIN anterior, apagar
    // uma conta fazia os eventos DELA desaparecerem da vista do administrador —
    // exactamente os que mais interessam a seguir a uma saída.
    let rows: Vec<AuditEntry> = sqlx::query_as(
        "SELECT a.id, COALESCE(u.username, a.actor_name) AS actor, a.action, a.target, a.created_at
         FROM audit_logs a
         LEFT JOIN users u ON u.id = a.actor_id
         WHERE a.org_id = $1
            OR (a.org_id IS NULL AND EXISTS (
                  SELECT 1 FROM org_members om WHERE om.org_id = $1 AND om.user_id = a.actor_id))
         ORDER BY a.created_at DESC
         LIMIT $2",
    )
    .bind(org_id)
    .bind(limit)
    .fetch_all(&state.db)
    .await?;
    Ok(Json(rows))
}
