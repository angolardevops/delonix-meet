//! Diagramas no servidor — o modelo EDITÁVEL, não a imagem (ADR-0020).
//!
//! **Porque existe.** O `web/src/pages/diagrams/store.ts` guardava o diagrama
//! só no IndexedDB deste browser, e o próprio comentário dele dizia o preço:
//! «um diagrama feito aqui não aparece noutro computador». A única forma de o
//! levar era exportar o JSON à mão. A biblioteca de `whiteboards` guarda o
//! **PNG** — a imagem achatada —, que serve para mostrar e nunca para continuar
//! a editar.
//!
//! **O que isto NÃO é.** Não é edição em simultâneo. Não há CRDT, não há
//! merge: quem grava por último fica com o documento, e é por isso que o `PUT`
//! leva o `updated_at` que o cliente viu (`If-Unmodified-Since` em espírito,
//! `expected_updated_at` em prática) e responde `409` quando entretanto mudou.
//! Duas abas do mesmo diagrama são o caso real, e a resposta certa é dizer
//! «mudou por baixo de ti» em vez de escolher um vencedor em silêncio.
//!
//! **Isolamento.** Um diagrama é da PESSOA, dentro de uma organização. Não é da
//! organização: nada o lista a um colega. A chave da tabela é
//! `(owner_id, id)` — o `id` vem do cliente (`uid('d')`), para o diagrama ter
//! nome antes de haver rede, e dois utilizadores podem escolher o mesmo. Com a
//! chave composta, a linha de um nunca toca na do outro; toda a leitura e toda
//! a escrita trazem o `owner_id` da sessão no `WHERE`, nunca o do pedido.

use axum::{
    extract::{Path, State},
    Json,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::{auth::AuthUser, error::ApiError, org::orgs_of_user, AppState};

/// Documento máximo aceite. Um diagrama de centenas de nós anda nas dezenas de
/// KB; 2 MiB é folga de uma ordem de grandeza e ainda assim um tecto — sem
/// tecto, um cliente com um `for` enche a tabela e o disco é partilhado com
/// todos os inquilinos.
const MAX_DOC_BYTES: usize = 2 * 1024 * 1024;

/// Diagramas por pessoa. O IndexedDB não tinha limite porque o disco era dela;
/// aqui é nosso.
const MAX_POR_PESSOA: i64 = 500;

/// Documentação OpenAPI das rotas deste módulo (`openapi.rs` junta-as).
#[derive(utoipa::OpenApi)]
#[openapi(
    paths(list, get_one, save, delete),
    components(schemas(DiagramSummary, DiagramDetail, SaveReq))
)]
pub struct ApiDoc;

/// O que a lista mostra. Sem o `doc`: a lista de vinte diagramas não puxa vinte
/// documentos inteiros para contar os elementos.
#[derive(Serialize, sqlx::FromRow, utoipa::ToSchema)]
pub struct DiagramSummary {
    pub id: String,
    pub title: String,
    /// `uml` | `bpmn` | `arch` | `flow` | `free` — como o cliente a escreveu.
    pub notation: String,
    pub room_code: String,
    /// Número de nós mais traços, como o cliente o contou ao gravar.
    pub elements: i32,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Um diagrama com o documento. O `doc` é opaco para o servidor **de
/// propósito**: a forma dele é do cliente (`DiagramDoc`, 1142 linhas de modelo
/// em TypeScript), e tipá-la aqui obrigaria a mudar o Rust a cada forma nova de
/// nó. O que o servidor garante é que é JSON, que cabe no tecto e de quem é.
#[derive(Serialize, utoipa::ToSchema)]
pub struct DiagramDetail {
    #[serde(flatten)]
    pub meta: DiagramSummary,
    #[schema(value_type = Object)]
    pub doc: serde_json::Value,
}

#[derive(Deserialize, utoipa::ToSchema)]
#[schema(as = DiagramSaveReq)]
pub struct SaveReq {
    /// Truncado a 200 caracteres; vazio fica vazio (o cliente já tem um nome
    /// por omissão traduzido, e inventar um aqui dava dois nomes diferentes).
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub notation: String,
    #[serde(default)]
    pub room_code: String,
    /// Nós mais traços. Só para a lista; não é verificado contra o `doc`.
    #[serde(default)]
    pub elements: i32,
    /// O documento. Objecto JSON; um array ou um número são recusados.
    #[schema(value_type = Object)]
    pub doc: serde_json::Value,
    /// O `updated_at` que o cliente viu. Presente e diferente do que está
    /// gravado → `409`. Ausente → escreve (é o primeiro envio, ou um cliente
    /// que não participa no controlo).
    #[serde(default)]
    pub expected_updated_at: Option<DateTime<Utc>>,
}

/// Notações que o cliente conhece. Uma string livre aqui deixava a lista do
/// cliente a receber um valor que ela não sabe desenhar.
const NOTACOES: [&str; 5] = ["uml", "bpmn", "arch", "flow", "free"];

fn validar(req: &SaveReq) -> Result<(String, String), ApiError> {
    if !req.doc.is_object() {
        return Err(ApiError::BadRequest(
            "o documento tem de ser um objecto JSON".into(),
        ));
    }
    // Mede-se o JSON SERIALIZADO e não o corpo do pedido: é isto que vai para
    // a coluna, e é por isto que se paga.
    let tamanho = serde_json::to_vec(&req.doc).map(|v| v.len()).unwrap_or(0);
    if tamanho > MAX_DOC_BYTES {
        return Err(ApiError::BadRequest(format!(
            "documento acima do limite ({tamanho} bytes; máximo {MAX_DOC_BYTES})"
        )));
    }
    let notation = if NOTACOES.contains(&req.notation.as_str()) {
        req.notation.clone()
    } else if req.notation.trim().is_empty() {
        "free".to_string()
    } else {
        return Err(ApiError::BadRequest("notação desconhecida".into()));
    };
    Ok((
        req.title.trim().chars().take(200).collect::<String>(),
        notation,
    ))
}

/// `GET /api/diagrams` — os diagramas de quem pede, mais recentes primeiro.
#[utoipa::path(
    get, path = "/api/diagrams", tag = "diagrams",
    security(("session" = [])),
    responses(
        (status = 200, body = Vec<DiagramSummary>, description = "Até 500, do mais recente para o mais antigo. Nunca os de outra pessoa, nem com a mesma organização."),
        (status = 401, body = crate::openapi::ErrorBody),
    )
)]
pub async fn list(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
) -> Result<Json<Vec<DiagramSummary>>, ApiError> {
    let items: Vec<DiagramSummary> = sqlx::query_as(
        "SELECT id, title, notation, room_code, elements, created_at, updated_at
         FROM diagrams WHERE owner_id = $1 ORDER BY updated_at DESC LIMIT $2",
    )
    .bind(auth.user_id)
    .bind(MAX_POR_PESSOA)
    .fetch_all(&state.db)
    .await?;
    Ok(Json(items))
}

/// `GET /api/diagrams/{diagram_id}` — um diagrama com o documento.
#[utoipa::path(
    get, path = "/api/diagrams/{diagram_id}", tag = "diagrams",
    security(("session" = [])),
    params(("diagram_id" = String, Path, description = "O id que o cliente escolheu (`uid('d')`), não um UUID.")),
    responses(
        (status = 200, body = DiagramDetail),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 404, description = "Não existe, ou é de outra pessoa — a MESMA resposta, de propósito: um `403` confirmava a existência.", body = crate::openapi::ErrorBody),
    )
)]
pub async fn get_one(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(id): Path<String>,
) -> Result<Json<DiagramDetail>, ApiError> {
    // Uma struct e não uma tupla de oito: o clippy chama-lhe «very complex
    // type» com razão — oito posições sem nome trocam-se com um erro que
    // compila (o `title` no lugar da `notation` são os dois `String`).
    #[derive(sqlx::FromRow)]
    struct Row {
        id: String,
        title: String,
        notation: String,
        room_code: String,
        elements: i32,
        created_at: DateTime<Utc>,
        updated_at: DateTime<Utc>,
        doc: serde_json::Value,
    }
    let row: Option<Row> = sqlx::query_as(
        "SELECT id, title, notation, room_code, elements, created_at, updated_at, doc
         FROM diagrams WHERE owner_id = $1 AND id = $2",
    )
    .bind(auth.user_id)
    .bind(&id)
    .fetch_optional(&state.db)
    .await?;
    let r = row.ok_or(ApiError::NotFound)?;
    Ok(Json(DiagramDetail {
        meta: DiagramSummary {
            id: r.id,
            title: r.title,
            notation: r.notation,
            room_code: r.room_code,
            elements: r.elements,
            created_at: r.created_at,
            updated_at: r.updated_at,
        },
        doc: r.doc,
    }))
}

/// `PUT /api/diagrams/{diagram_id}` — grava (cria ou substitui).
#[utoipa::path(
    put, path = "/api/diagrams/{diagram_id}", tag = "diagrams",
    security(("session" = [])),
    params(("diagram_id" = String, Path)),
    request_body = SaveReq,
    responses(
        (status = 200, body = DiagramSummary, description = "O diagrama como ficou gravado. O `updated_at` daqui é o que o próximo `PUT` manda em `expected_updated_at`."),
        (status = 400, description = "Sem organização, documento que não é objecto, acima de 2 MiB, notação desconhecida, ou id fora do formato.", body = crate::openapi::ErrorBody),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 409, description = "`diagram.conflict` — mudou no servidor desde o `expected_updated_at`. Não há merge: o cliente mostra as duas datas e decide.", body = crate::openapi::ErrorBody),
        (status = 422, description = "`diagram.too_many` — a pessoa chegou ao tecto de 500 diagramas.", body = crate::openapi::ErrorBody),
    )
)]
pub async fn save(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(id): Path<String>,
    Json(req): Json<SaveReq>,
) -> Result<Json<DiagramSummary>, ApiError> {
    if id.is_empty() || id.len() > 64 {
        return Err(ApiError::BadRequest(
            "id do diagrama entre 1 e 64 caracteres".into(),
        ));
    }
    let (title, notation) = validar(&req)?;
    let org_id = *orgs_of_user(&state, auth.user_id)
        .await
        .first()
        .ok_or(ApiError::BadRequest("utilizador sem organização".into()))?;

    // Tudo na MESMA transacção: o tecto, o controlo de concorrência e a
    // escrita. Contar fora dela deixava dois pedidos a passar o tecto ao mesmo
    // tempo, e ler o `updated_at` fora dela deixava a janela que o `409`
    // existe para fechar.
    let mut tx = state.db.begin().await?;

    let actual: Option<(DateTime<Utc>,)> = sqlx::query_as(
        "SELECT updated_at FROM diagrams WHERE owner_id = $1 AND id = $2 FOR UPDATE",
    )
    .bind(auth.user_id)
    .bind(&id)
    .fetch_optional(&mut *tx)
    .await?;

    if let (Some((gravado,)), Some(esperado)) = (actual.as_ref(), req.expected_updated_at) {
        // Comparação ao MILISSEGUNDO: o `TIMESTAMPTZ` guarda microssegundos e o
        // `Date` do browser só tem milissegundos, pelo que a igualdade exacta
        // dava `409` em todas as gravações seguidas.
        if gravado.timestamp_millis() != esperado.timestamp_millis() {
            return Err(ApiError::Conflict("diagram.conflict".into()));
        }
    }

    if actual.is_none() {
        let (quantos,): (i64,) =
            sqlx::query_as("SELECT count(*) FROM diagrams WHERE owner_id = $1")
                .bind(auth.user_id)
                .fetch_one(&mut *tx)
                .await?;
        if quantos >= MAX_POR_PESSOA {
            return Err(ApiError::Unprocessable("diagram.too_many".into()));
        }
    }

    let meta: DiagramSummary = sqlx::query_as(
        "INSERT INTO diagrams (id, owner_id, org_id, title, notation, room_code, doc, elements)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
         ON CONFLICT (owner_id, id) DO UPDATE
         SET title = EXCLUDED.title,
             notation = EXCLUDED.notation,
             room_code = EXCLUDED.room_code,
             doc = EXCLUDED.doc,
             elements = EXCLUDED.elements,
             -- O `org_id` NÃO se actualiza: um diagrama fica na organização em
             -- que nasceu. Mudá-lo ao gravar movia trabalho entre inquilinos
             -- a quem pertence a mais de um.
             updated_at = now()
         RETURNING id, title, notation, room_code, elements, created_at, updated_at",
    )
    .bind(&id)
    .bind(auth.user_id)
    .bind(org_id)
    .bind(&title)
    .bind(&notation)
    .bind(req.room_code.chars().take(64).collect::<String>())
    .bind(&req.doc)
    .bind(req.elements.max(0))
    .fetch_one(&mut *tx)
    .await?;

    tx.commit().await?;
    Ok(Json(meta))
}

/// `DELETE /api/diagrams/{diagram_id}` — apaga.
#[utoipa::path(
    delete, path = "/api/diagrams/{diagram_id}", tag = "diagrams",
    security(("session" = [])),
    params(("diagram_id" = String, Path)),
    responses(
        (status = 204, description = "Apagado."),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 404, description = "Não existe, ou é de outra pessoa.", body = crate::openapi::ErrorBody),
    )
)]
pub async fn delete(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(id): Path<String>,
) -> Result<axum::http::StatusCode, ApiError> {
    let r = sqlx::query("DELETE FROM diagrams WHERE owner_id = $1 AND id = $2")
        .bind(auth.user_id)
        .bind(&id)
        .execute(&state.db)
        .await?;
    if r.rows_affected() == 0 {
        return Err(ApiError::NotFound);
    }
    Ok(axum::http::StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pedido(doc: serde_json::Value) -> SaveReq {
        SaveReq {
            title: "  Um diagrama  ".into(),
            notation: "bpmn".into(),
            room_code: String::new(),
            elements: 3,
            doc,
            expected_updated_at: None,
        }
    }

    #[test]
    fn o_documento_tem_de_ser_um_objecto() {
        // Um array passava pelo `is_object` de nada e chegava à coluna como
        // JSON válido que o cliente não sabe abrir.
        for nao in [
            serde_json::json!([1, 2, 3]),
            serde_json::json!("texto"),
            serde_json::json!(7),
            serde_json::Value::Null,
        ] {
            assert!(validar(&pedido(nao)).is_err());
        }
        assert!(validar(&pedido(serde_json::json!({"v": 1}))).is_ok());
    }

    #[test]
    fn o_titulo_apara_se_e_corta_se_aos_200() {
        let (t, _) = validar(&pedido(serde_json::json!({}))).unwrap();
        assert_eq!(t, "Um diagrama");

        let mut longo = pedido(serde_json::json!({}));
        longo.title = "á".repeat(500);
        let (t, _) = validar(&longo).unwrap();
        // CARACTERES e não bytes: cortar a 200 bytes partia um «á» em dois e
        // dava um título com um byte inválido no fim.
        assert_eq!(t.chars().count(), 200);
    }

    #[test]
    fn a_notacao_vazia_e_free_e_a_inventada_e_recusada() {
        let mut p = pedido(serde_json::json!({}));
        p.notation = String::new();
        assert_eq!(validar(&p).unwrap().1, "free");
        p.notation = "   ".into();
        assert_eq!(validar(&p).unwrap().1, "free");
        p.notation = "uml".into();
        assert_eq!(validar(&p).unwrap().1, "uml");
        // Uma notação que o cliente não sabe desenhar não entra na base: a
        // lista dele receberia um valor que não consegue mostrar.
        p.notation = "esquema-do-joao".into();
        assert!(validar(&p).is_err());
    }

    #[test]
    fn o_tecto_do_documento_mede_o_json_e_nao_o_pedido() {
        let grande = serde_json::json!({
            "nodes": (0..200_000).map(|i| serde_json::json!({"id": i})).collect::<Vec<_>>()
        });
        let erro = validar(&pedido(grande)).unwrap_err();
        assert!(format!("{erro:?}").contains("acima do limite"), "{erro:?}");
    }
}
