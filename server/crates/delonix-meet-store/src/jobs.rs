//! Reivindicação de trabalho de uma fila — **o único `FOR UPDATE SKIP LOCKED`**.
//!
//! A forma e a aritmética estão em [`delonix_meet_core::jobs`]; aqui está a
//! travessia da tabela. Não vai num contexto delimitado (como `identity`) pela
//! mesma razão que a parte pura não vai num contexto do domínio: as sete filas
//! que isto junta atravessam cinco dos oito contextos.
//!
//! ## Porque a reivindicação justa tem DUAS fases
//!
//! A versão simples — `UPDATE … WHERE id IN (SELECT … FOR UPDATE SKIP LOCKED)
//! RETURNING …` — é a que seis das sete filas usavam, e não serve quando o lote
//! tem de ser repartido entre inquilinos: **o Postgres não combina uma função
//! de janela com `FOR UPDATE`**. A implementação dos webhooks já o tinha
//! resolvido e é a estrutura que se copia aqui:
//!
//! 1. escolher os ids com a janela (`row_number() OVER (PARTITION BY …)`), que
//!    **não** bloqueia;
//! 2. bloquear esses ids com `FOR UPDATE SKIP LOCKED` **revalidando a condição**
//!    — dois workers que escolham as mesmas linhas não levam as mesmas;
//! 3. marcar a posse, na mesma transacção.
//!
//! Sem `tenant_column` o passo 1 degenera no `ORDER BY … LIMIT` de sempre, e a
//! fila comporta-se como as seis antigas.

use delonix_meet_core::jobs::{Queue, Retry};
use sqlx::{postgres::PgRow, FromRow, PgPool, Row};

/// Monta o SQL da escolha. Os nomes vêm todos de `&'static str` do módulo que
/// declara a fila (nunca de um pedido); o único valor interpolado é o tecto de
/// tentativas, que é um `i32` do nosso código. O lote vai por *bind*.
fn select_sql(q: &Queue, retry: &Retry) -> String {
    let ready = q.ready_when.replace("{max_attempts}", &retry.max_attempts.to_string());
    let escolha = match q.tenant_column {
        // Reparte o lote: a primeira de cada inquilino, depois a segunda de
        // cada um, e assim por diante. Com um só inquilino à espera, continua a
        // levar o lote inteiro.
        Some(tenant) => format!(
            "SELECT {id} FROM (
               SELECT {id}, {order}, row_number() OVER (PARTITION BY {tenant} ORDER BY {order}) AS rn
                 FROM {table} WHERE {ready}
             ) due ORDER BY rn, {order} LIMIT $1",
            id = q.id_column,
            order = q.order_by,
            table = q.table,
        ),
        None => format!(
            "SELECT {id} FROM {table} WHERE {ready} ORDER BY {order} LIMIT $1",
            id = q.id_column,
            order = q.order_by,
            table = q.table,
        ),
    };
    format!(
        "SELECT {returning} FROM {table}
          WHERE {id} = ANY({escolha}) AND {ready}
          ORDER BY {order}
            FOR UPDATE SKIP LOCKED",
        returning = q.returning,
        table = q.table,
        id = q.id_column,
        order = q.order_by,
    )
}

/// Leva até `q.batch` trabalhos da fila e marca-lhes a posse.
///
/// O que volta é o `q.returning` de cada linha reivindicada, já com o
/// `q.claim_set` escrito e a transacção fechada: se isto devolve uma linha, o
/// trabalho é de quem chamou e de mais ninguém.
pub async fn claim<T>(pool: &PgPool, q: &Queue, retry: &Retry) -> Result<Vec<T>, sqlx::Error>
where
    T: for<'r> FromRow<'r, PgRow> + Send + Unpin,
{
    let mut tx = pool.begin().await?;
    let rows = sqlx::query(&select_sql(q, retry))
        .bind(q.batch)
        .fetch_all(&mut *tx)
        .await?;
    if rows.is_empty() {
        // Nada a fazer: fecha sem escrever. Um `commit` de uma transacção que
        // não mexeu em nada é barato, mas um `rollback` é mais honesto.
        tx.rollback().await?;
        return Ok(Vec::new());
    }
    let ids: Vec<sqlx::types::Uuid> = rows
        .iter()
        .map(|r| r.try_get(q.id_column))
        .collect::<Result<_, _>>()?;
    sqlx::query(&format!(
        "UPDATE {table} SET {set} WHERE {id} = ANY($1)",
        table = q.table,
        set = q.claim_set,
        id = q.id_column,
    ))
    .bind(&ids)
    .execute(&mut *tx)
    .await?;
    let levados = rows
        .iter()
        .map(T::from_row)
        .collect::<Result<Vec<T>, _>>()?;
    tx.commit().await?;
    Ok(levados)
}

#[cfg(test)]
mod tests {
    use super::*;
    use delonix_meet_core::jobs::RETRY_WEBHOOK;

    const SEM_INQUILINO: Queue = Queue {
        name: "export",
        table: "data_exports",
        id_column: "id",
        ready_when: "status = 'queued'",
        claim_set: "status = 'running', started_at = now()",
        returning: "id, user_id",
        order_by: "created_at",
        tenant_column: None,
        batch: 1,
    };

    const COM_INQUILINO: Queue = Queue {
        tenant_column: Some("org_id"),
        name: "webhook_delivery",
        table: "webhook_deliveries",
        ready_when: "status = 'failed' AND retry_at <= now() AND attempt < {max_attempts}",
        batch: 50,
        ..SEM_INQUILINO
    };

    #[test]
    fn sem_inquilino_e_a_forma_simples_de_sempre() {
        let sql = select_sql(&SEM_INQUILINO, &Retry::ONCE);
        assert!(sql.contains("FOR UPDATE SKIP LOCKED"));
        assert!(!sql.contains("row_number"), "pôs justiça onde não foi pedida");
        // A condição é revalidada DEPOIS da escolha: é o que impede dois
        // workers que escolham as mesmas linhas de levarem as mesmas.
        assert_eq!(sql.matches("status = 'queued'").count(), 2, "{sql}");
        // O lote vai por bind, nunca interpolado.
        assert!(sql.contains("LIMIT $1"));
        assert!(!sql.contains("LIMIT 1 "), "o lote foi interpolado");
    }

    #[test]
    fn com_inquilino_reparte_o_lote_pela_janela() {
        let sql = select_sql(&COM_INQUILINO, &RETRY_WEBHOOK);
        assert!(sql.contains("row_number() OVER (PARTITION BY org_id ORDER BY created_at)"));
        // A janela fica FORA do `FOR UPDATE` — o Postgres não as combina, e foi
        // assim que os webhooks o resolveram.
        let janela = sql.find("row_number").unwrap();
        let bloqueio = sql.find("FOR UPDATE").unwrap();
        assert!(janela < bloqueio, "a janela caiu dentro do FOR UPDATE: {sql}");
        assert!(sql.contains("ORDER BY rn, created_at"));
    }

    #[test]
    fn o_tecto_de_tentativas_vem_da_politica_e_nao_de_um_literal_repetido() {
        // Uma fila escreve `{max_attempts}` e o valor vem do `Retry`: sem isto,
        // o tecto ficava escrito duas vezes e divergia na primeira alteração.
        let sql = select_sql(&COM_INQUILINO, &RETRY_WEBHOOK);
        assert!(sql.contains("attempt < 5"), "{sql}");
        assert!(!sql.contains("{max_attempts}"));
        let outra = Retry {
            max_attempts: 3,
            ..RETRY_WEBHOOK
        };
        assert!(select_sql(&COM_INQUILINO, &outra).contains("attempt < 3"));
    }
}
