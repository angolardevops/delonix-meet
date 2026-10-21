//! SCIM 2.0 — o directório da empresa escreve os membros (RFC 7643/7644).
//!
//! **PORQUE EXISTE.** É a lacuna nº1 do levantamento de 2026-10-13 e a primeira
//! pergunta de qualquer organização com mais de 200 pessoas: «as entradas e as
//! saídas vêm do nosso directório?». Sem isto, cada saída é manual — e uma
//! conta que não sai é um achado de auditoria.
//!
//! **O CICLO JÁ EXISTIA, para um só ecossistema.** O `odoo.rs::provision` cria,
//! actualiza, muda papéis e suspende quem saiu. O SCIM é o mesmo ciclo falado
//! pelo protocolo que o Entra ID e o Okta falam — e por isso este módulo
//! **orquestra os helpers que já existem** (`org::ensure_user_tx`,
//! `insert_member_tx`, `archive_member_tx`, `reactivate_member_tx`) em vez de
//! escrever outra vez as regras de pertença. É a regra 1 do ADR-0004: a
//! pertença decide-se em `org.rs`, e a catraca conta quem a contorna.
//!
//! **O TOKEN É PRÓPRIO (`dlxs_`), e isso é a lição da R142:** um token emitido
//! para uma superfície não pode abrir outra. O extractor recusa o `dlxo_` do
//! Odoo e a chave `dlx_` do inquilino, e exige `scim_enabled`.
//!
//! **O QUE ESTA FATIA FAZ:** os `Users` (criar, ler, listar por `userName`,
//! `PATCH`/`PUT` de `active` e de nome, e `DELETE`) mais a descoberta
//! (`ServiceProviderConfig`, `ResourceTypes`, `Schemas`) que os provedores lêem
//! antes de falar. A listagem pagina por `startIndex`/`count` (RFC 7644
//! §3.4.2.4) e o `totalResults` é o total da organização — ver `list_users`.
//!
//! **Os `Groups` ficam de fora, por decisão escrita a 2026-10-21** (fecho do
//! `B-10`): um grupo do directório só vale alguma coisa quando decidir algo
//! aqui dentro — um papel, uma sala, um departamento — e o mapeamento
//! atributo→papel é decisão de produto em aberto (ver «não mexe em papéis»
//! abaixo). Anunciar `/Groups` sem esse efeito era aceitar escritas que não
//! mudam nada. Até lá a descoberta continua a dizer a verdade: quem lê o
//! `ResourceTypes` vê só `User`, em vez de descobrir a ausência à terceira
//! chamada.
//!
//! **O QUE NÃO FAZ, dito aqui:**
//! - `DELETE` **suspende**, não apaga. Uma pessoa que sai leva consigo actas,
//!   gravações e trilha de auditoria; apagar a conta apagava o rasto de quem
//!   fez o quê. É o mesmo que o `odoo_exit` faz.
//! - não muda o email de uma conta existente: o `userName` serve para
//!   encontrar, o `externalId` para reencontrar. Mudar o email de alguém é
//!   tomar-lhe a identidade e passa pelas regras da casa (R25), não por aqui.
//! - não mexe em papéis. O directório diz quem pertence; **quem manda na
//!   organização decide-se aqui** — e dar papéis por atributo de directório é
//!   decisão de produto, não de protocolo.

use std::sync::Arc;

use axum::extract::FromRequestParts;
use axum::{
    extract::{Path, Query, State},
    http::{request::Parts, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use delonix_meet_core::crypto::{prefixed_token, sha256_hex};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use uuid::Uuid;

use crate::{auth::AuthUser, error::ApiError, AppState};

/// Prefixo do token desta superfície. Ver a R142 no cabeçalho.
const TOKEN_PREFIX: &str = "dlxs_";
const SCHEMA_USER: &str = "urn:ietf:params:scim:schemas:core:2.0:User";
const SCHEMA_LIST: &str = "urn:ietf:params:scim:api:messages:2.0:ListResponse";
const SCHEMA_PATCH: &str = "urn:ietf:params:scim:api:messages:2.0:PatchOp";
const SCHEMA_ERROR: &str = "urn:ietf:params:scim:api:messages:2.0:Error";
/// Quantos recursos uma listagem devolve, no máximo. Os provedores paginam.
const MAX_RESULTS: i64 = 200;

// ---------- erro no envelope do SCIM ----------

/// O SCIM tem o SEU formato de erro (RFC 7644 §3.12) e os provedores lêem-no:
/// o Entra ID mostra o `detail` no painel de provisionamento. Devolver o nosso
/// `{"error": …}` aqui dava um erro sem explicação do lado deles.
#[derive(Debug)]
pub struct ScimError {
    status: StatusCode,
    /// `scimType` — o vocabulário fechado da norma (`invalidFilter`,
    /// `uniqueness`, `invalidValue`, `mutability`).
    kind: Option<&'static str>,
    detail: String,
}

impl ScimError {
    fn new(status: StatusCode, kind: Option<&'static str>, detail: impl Into<String>) -> Self {
        ScimError {
            status,
            kind,
            detail: detail.into(),
        }
    }
    fn not_found(id: &str) -> Self {
        Self::new(
            StatusCode::NOT_FOUND,
            None,
            format!("recurso {id} não existe"),
        )
    }
    fn bad(kind: &'static str, detail: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, Some(kind), detail)
    }
}

impl IntoResponse for ScimError {
    fn into_response(self) -> Response {
        let mut body = json!({
            "schemas": [SCHEMA_ERROR],
            "status": self.status.as_u16().to_string(),
            "detail": self.detail,
        });
        if let Some(kind) = self.kind {
            body["scimType"] = json!(kind);
        }
        (self.status, Json(body)).into_response()
    }
}

impl From<ApiError> for ScimError {
    /// Um erro nosso que escape para esta superfície sai no envelope dela — e
    /// sem detalhe nenhum do que se passou por dentro.
    fn from(e: ApiError) -> Self {
        let status = e.into_response().status();
        ScimError::new(status, None, "o pedido não pôde ser atendido")
    }
}

impl From<sqlx::Error> for ScimError {
    fn from(e: sqlx::Error) -> Self {
        tracing::error!(error = ?e, "scim: base de dados");
        ScimError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            None,
            "o pedido não pôde ser atendido",
        )
    }
}

// ---------- extractor ----------

/// Quem fala nesta superfície: o directório de UMA organização.
pub struct ScimAuth {
    pub org_id: Uuid,
}

impl FromRequestParts<Arc<AppState>> for ScimAuth {
    type Rejection = ScimError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &Arc<AppState>,
    ) -> Result<Self, ScimError> {
        let raw = crate::auth::bearer_token(&parts.headers)
            .ok_or_else(|| ScimError::new(StatusCode::UNAUTHORIZED, None, "falta o token"))?;
        // SÓ o token desta superfície (R142): o `dlxo_` provisiona pelo Odoo e
        // o `dlx_` é a chave da org para a `/api/v1`. Nenhum dos dois declarou
        // o poder de reescrever membros por SCIM.
        if !raw.starts_with(TOKEN_PREFIX) {
            return Err(ScimError::new(
                StatusCode::UNAUTHORIZED,
                None,
                "token inválido",
            ));
        }
        let hash = sha256_hex(raw);
        let org_id: Option<Uuid> = sqlx::query_scalar(
            "SELECT id FROM organizations WHERE scim_token_hash = $1 AND scim_enabled",
        )
        .bind(&hash)
        .fetch_optional(&state.db)
        .await?;
        let org_id = org_id
            .ok_or_else(|| ScimError::new(StatusCode::UNAUTHORIZED, None, "token inválido"))?;
        // «O SCIM parou» é a queixa, e esta coluna é a resposta: saber se o
        // directório ainda fala connosco, e quando falou pela última vez.
        let _ = sqlx::query("UPDATE organizations SET scim_last_seen_at = now() WHERE id = $1")
            .bind(org_id)
            .execute(&state.db)
            .await;
        Ok(ScimAuth { org_id })
    }
}

// ---------- o recurso User ----------

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScimName {
    #[serde(default)]
    pub formatted: Option<String>,
    #[serde(default)]
    pub given_name: Option<String>,
    #[serde(default)]
    pub family_name: Option<String>,
}

impl ScimName {
    /// O nome a mostrar, do que o directório mandou.
    fn display(&self) -> String {
        if let Some(f) = self
            .formatted
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            return f.to_string();
        }
        [self.given_name.as_deref(), self.family_name.as_deref()]
            .iter()
            .flatten()
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join(" ")
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScimUserReq {
    /// A norma manda o cliente declarar o esquema. Validamos **tolerante**:
    /// uma lista que não fale do `User` é recusada, uma lista VAZIA passa — há
    /// provedores que a omitem, e recusar por isso era fechar a porta a quem
    /// está só a ser desleixado com um campo que não muda nada.
    #[serde(default)]
    pub schemas: Vec<String>,
    /// O email. É a identidade da conta aqui.
    pub user_name: String,
    #[serde(default)]
    pub external_id: Option<String>,
    #[serde(default)]
    pub name: Option<ScimName>,
    /// Ausente = activo, como manda a norma.
    #[serde(default = "verdadeiro")]
    pub active: bool,
}

fn verdadeiro() -> bool {
    true
}

/// O recurso da norma, a partir da linha de membro que o `org.rs` devolve.
trait ParaRecurso {
    fn to_resource(&self) -> Value;
}

impl ParaRecurso for crate::org::ScimMemberRow {
    fn to_resource(&self) -> Value {
        json!({
            "schemas": [SCHEMA_USER],
            "id": self.id,
            "userName": self.email,
            "externalId": self.scim_external_id,
            "name": {"formatted": self.username},
            "emails": [{"value": self.email, "primary": true, "type": "work"}],
            "active": self.active,
            "meta": {
                "resourceType": "User",
                "location": format!("/scim/v2/Users/{}", self.id),
            },
        })
    }
}

async fn read_user(
    state: &AppState,
    org_id: Uuid,
    id: Uuid,
) -> Result<Option<crate::org::ScimMemberRow>, ScimError> {
    Ok(crate::org::scim_member(state, org_id, id).await?)
}

// ---------- descoberta ----------

/// `GET /scim/v2/ServiceProviderConfig` — o que esta instalação suporta.
///
/// Dizer a verdade aqui poupa uma investigação do outro lado: o Entra ID lê
/// isto e só tenta o que estiver declarado. `patch` sim, `filter` sim (só
/// `userName eq`), `bulk`, `sort` e `etag` não.
pub async fn service_provider_config(_scim: ScimAuth) -> Json<Value> {
    Json(json!({
        "schemas": ["urn:ietf:params:scim:schemas:core:2.0:ServiceProviderConfig"],
        "documentationUri": "https://github.com/angolardevops/delonix-meet",
        "patch": {"supported": true},
        "bulk": {"supported": false, "maxOperations": 0, "maxPayloadSize": 0},
        "filter": {"supported": true, "maxResults": MAX_RESULTS},
        "changePassword": {"supported": false},
        "sort": {"supported": false},
        "etag": {"supported": false},
        "authenticationSchemes": [{
            "type": "oauthbearertoken",
            "name": "OAuth Bearer Token",
            "description": "Token `dlxs_` emitido pelo administrador da organização",
        }],
        "meta": {"resourceType": "ServiceProviderConfig", "location": "/scim/v2/ServiceProviderConfig"},
    }))
}

/// `GET /scim/v2/ResourceTypes` — **só `User`**, porque os `Groups` são a fatia
/// seguinte. Quem lê isto não descobre a ausência à terceira chamada.
pub async fn resource_types(_scim: ScimAuth) -> Json<Value> {
    let user = json!({
        "schemas": ["urn:ietf:params:scim:schemas:core:2.0:ResourceType"],
        "id": "User",
        "name": "User",
        "endpoint": "/Users",
        "schema": SCHEMA_USER,
        "meta": {"resourceType": "ResourceType", "location": "/scim/v2/ResourceTypes/User"},
    });
    Json(json!({
        "schemas": [SCHEMA_LIST],
        "totalResults": 1,
        "itemsPerPage": 1,
        "startIndex": 1,
        "Resources": [user],
    }))
}

/// `GET /scim/v2/Schemas` — os atributos que esta superfície entende de facto.
pub async fn schemas(_scim: ScimAuth) -> Json<Value> {
    let atributo = |nome: &str, tipo: &str, obrigatorio: bool| {
        json!({"name": nome, "type": tipo, "multiValued": false, "required": obrigatorio,
               "caseExact": false, "mutability": "readWrite", "returned": "default",
               "uniqueness": if nome == "userName" { "server" } else { "none" }})
    };
    let user = json!({
        "schemas": ["urn:ietf:params:scim:schemas:core:2.0:Schema"],
        "id": SCHEMA_USER,
        "name": "User",
        "description": "Membro de uma organização do Delonix Meet",
        "attributes": [
            atributo("userName", "string", true),
            atributo("externalId", "string", false),
            atributo("active", "boolean", false),
            json!({"name": "name", "type": "complex", "multiValued": false, "required": false,
                   "mutability": "readWrite", "returned": "default",
                   "subAttributes": [atributo("formatted", "string", false),
                                     atributo("givenName", "string", false),
                                     atributo("familyName", "string", false)]}),
        ],
        "meta": {"resourceType": "Schema", "location": format!("/scim/v2/Schemas/{SCHEMA_USER}")},
    });
    Json(json!({
        "schemas": [SCHEMA_LIST],
        "totalResults": 1,
        "itemsPerPage": 1,
        "startIndex": 1,
        "Resources": [user],
    }))
}

// ---------- Users ----------

#[derive(Deserialize)]
pub struct ListParams {
    /// Só `userName eq "alguém@exemplo.ao"`, que é o que o Entra ID e o Okta
    /// mandam para reconciliar. Qualquer outro filtro é `invalidFilter`, e não
    /// uma lista silenciosamente errada.
    #[serde(default)]
    pub filter: Option<String>,
    /// Paginação do RFC 7644 §3.4.2.4, em texto de propósito: um valor que não
    /// seja um inteiro tem de sair no envelope do SCIM (`invalidValue`), e a
    /// recusa do extractor `Query` sairia no nosso, sem `detail` para o painel
    /// do provedor.
    #[serde(default, rename = "startIndex")]
    pub start_index: Option<String>,
    #[serde(default)]
    pub count: Option<String>,
}

/// Uma página pedida: `start` é 1-based, como a norma.
#[derive(Debug, PartialEq, Eq)]
struct Pagina {
    start: i64,
    count: i64,
}

/// A página pedida, nas regras da norma (RFC 7644 §3.4.2.4):
/// - `startIndex` menor que 1 vale 1; omisso vale 1;
/// - `count` negativo vale 0, e `0` é legítimo: o provedor só quer o
///   `totalResults`; omisso, ou maior que o tecto, vale [`MAX_RESULTS`], que é o
///   `filter.maxResults` que a descoberta declara.
///
/// O que NÃO é tolerado: um valor que não seja inteiro. Interpretá-lo como 1
/// ou como 200 dava ao provedor uma página que ele não pediu, sem erro.
fn pagina(start_index: Option<&str>, count: Option<&str>) -> Result<Pagina, ScimError> {
    let inteiro = |nome: &str, v: &str| {
        v.trim().parse::<i64>().map_err(|_| {
            ScimError::bad(
                "invalidValue",
                format!("`{nome}` tem de ser um inteiro, veio «{v}»"),
            )
        })
    };
    let start = match start_index {
        Some(v) => inteiro("startIndex", v)?.max(1),
        None => 1,
    };
    let count = match count {
        Some(v) => inteiro("count", v)?.clamp(0, MAX_RESULTS),
        None => MAX_RESULTS,
    };
    Ok(Pagina { start, count })
}

/// O email de um `filter` de `userName eq`. `None` quando não há filtro;
/// `Err` quando há um filtro que esta superfície não entende.
fn email_do_filtro(filtro: &str) -> Result<Option<String>, ScimError> {
    let f = filtro.trim();
    if f.is_empty() {
        return Ok(None);
    }
    let (campo, resto) = f.split_once(char::is_whitespace).unwrap_or((f, ""));
    if !campo.eq_ignore_ascii_case("userName") {
        return Err(ScimError::bad(
            "invalidFilter",
            "só se filtra por `userName eq \"…\"`",
        ));
    }
    let resto = resto.trim();
    let valor = resto
        .strip_prefix("eq")
        .or_else(|| resto.strip_prefix("Eq"))
        .or_else(|| resto.strip_prefix("EQ"))
        .ok_or_else(|| ScimError::bad("invalidFilter", "só o operador `eq` é suportado"))?
        .trim()
        .trim_matches('"');
    if valor.is_empty() {
        return Err(ScimError::bad(
            "invalidFilter",
            "o `eq` precisa de um valor",
        ));
    }
    Ok(Some(valor.to_lowercase()))
}

/// Recusa um corpo que declare esquemas e não inclua o do `User` (ver o campo
/// `schemas` em [`ScimUserReq`]).
fn esquema_de_user_ok(schemas: &[String]) -> Result<(), ScimError> {
    if schemas.is_empty() || schemas.iter().any(|s| s == SCHEMA_USER) {
        return Ok(());
    }
    Err(ScimError::bad(
        "invalidValue",
        "o corpo tem de declarar o esquema urn:ietf:params:scim:schemas:core:2.0:User",
    ))
}

/// `GET /scim/v2/Users` — a lista desta organização, ou um filtro por
/// `userName`, paginada por `startIndex` e `count`.
///
/// **O `totalResults` é o total da organização, não o tamanho da página.** Até
/// 2026-10-21 era `recursos.len()` com um tecto de 200 e o `startIndex`
/// ignorado: numa organização de 500, o Entra ID lia 200 e o campo
/// confirmava-lhe que eram 200 — 300 pessoas ficavam fora do provisionamento
/// sem erro de lado nenhum (lacuna `B-10` da fila de paridade). Os provedores
/// decidem quando parar pelo `totalResults`, por isso é ele que tem de ser
/// verdade.
pub async fn list_users(
    State(state): State<Arc<AppState>>,
    scim: ScimAuth,
    Query(params): Query<ListParams>,
) -> Result<Json<Value>, ScimError> {
    let email = match params.filter.as_deref() {
        Some(f) => email_do_filtro(f)?,
        None => None,
    };
    let p = pagina(params.start_index.as_deref(), params.count.as_deref())?;
    let total = crate::org::scim_members_total(&state, scim.org_id, email.as_deref()).await?;
    let rows =
        crate::org::scim_members(&state, scim.org_id, email.as_deref(), p.start - 1, p.count)
            .await?;
    let recursos: Vec<Value> = rows.iter().map(ParaRecurso::to_resource).collect();
    Ok(Json(json!({
        "schemas": [SCHEMA_LIST],
        "totalResults": total,
        "itemsPerPage": recursos.len(),
        "startIndex": p.start,
        "Resources": recursos,
    })))
}

/// `GET /scim/v2/Users/{id}`
pub async fn get_user(
    State(state): State<Arc<AppState>>,
    scim: ScimAuth,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ScimError> {
    match read_user(&state, scim.org_id, id).await? {
        Some(u) => Ok(Json(u.to_resource())),
        None => Err(ScimError::not_found(&id.to_string())),
    }
}

/// `POST /scim/v2/Users` — o directório manda alguém para dentro.
///
/// Cria a conta se não existir e torna-a membro desta organização. Uma conta
/// que já exista **noutra** organização não é roubada: 409 `uniqueness`, que é
/// o que a norma manda e o que o provedor sabe mostrar.
pub async fn create_user(
    State(state): State<Arc<AppState>>,
    scim: ScimAuth,
    Json(req): Json<ScimUserReq>,
) -> Result<Response, ScimError> {
    esquema_de_user_ok(&req.schemas)?;
    use delonix_meet_domain::identity::validation::{normalize_email, validate_email};
    let email = normalize_email(&req.user_name);
    validate_email(&email).map_err(|m| ScimError::bad("invalidValue", m))?;
    let nome = req.name.as_ref().map(ScimName::display).unwrap_or_default();

    let mut tx = state.db.begin().await?;
    // QUEM PODE SER RECLAMADO. Saber o endereço de alguém não o traz para
    // dentro: é a regra da R142, com a diferença que o SCIM gere pertença e não
    // autenticação (ver `org::claim_for_org_tx`). Sem isto, bastava listar o
    // email de alguém no directório para o capturar — o buraco que o Odoo já
    // teve, e que custou uma regressão a fechar.
    let user_id = match crate::org::claim_for_org_tx(&mut tx, scim.org_id, &email).await? {
        crate::org::EmailClaim::DestaOrg(id) => {
            // Já é desta organização: adopta-se (e o `active` abaixo decide se
            // fica ligada). É o caso de quem já estava aqui antes de o SCIM ser
            // ligado — sem isto, uma instalação com 200 pessoas não podia
            // adoptar ninguém.
            id
        }
        crate::org::EmailClaim::DeFora => {
            return Err(ScimError::new(
                StatusCode::CONFLICT,
                Some("uniqueness"),
                "existe uma conta com este endereço que não pertence a esta organização",
            ))
        }
        crate::org::EmailClaim::Nova => {
            let (id, _) = crate::org::ensure_user_tx(&mut tx, &email, &nome).await?;
            // A pertença passa pelos helpers de `org.rs` (regra 1 do ADR-0004):
            // é lá que `archived_at` e os papéis são decididos.
            // Sem título: o directório diz quem pertence, não que cargo tem.
            crate::org::insert_member_tx(&mut tx, scim.org_id, id, "member", "").await?;
            id
        }
    };
    sqlx::query(
        "UPDATE users SET scim_managed = TRUE, scim_org_id = $1, scim_external_id = $2
          WHERE id = $3",
    )
    .bind(scim.org_id)
    .bind(req.external_id.as_deref())
    .bind(user_id)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;

    // O `active` do corpo manda: um `false` à criação é alguém que saiu antes
    // da primeira volta do directório, e um `true` sobre quem estava arquivado
    // é uma readmissão.
    if req.active {
        reactivar(&state, scim.org_id, user_id).await?;
    } else {
        suspender(&state, scim.org_id, user_id).await?;
    }
    crate::audit::log(
        &state.db,
        Some(scim.org_id),
        Uuid::nil(),
        "member.scim_created",
        &user_id.to_string(),
    )
    .await;
    let u = read_user(&state, scim.org_id, user_id)
        .await?
        .ok_or_else(|| ScimError::not_found(&user_id.to_string()))?;
    Ok((StatusCode::CREATED, Json(u.to_resource())).into_response())
}

#[derive(Deserialize)]
pub struct PatchReq {
    #[serde(default)]
    pub schemas: Vec<String>,
    #[serde(rename = "Operations", default)]
    pub operations: Vec<PatchOp>,
}

#[derive(Deserialize)]
pub struct PatchOp {
    #[serde(default)]
    pub op: String,
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub value: Option<Value>,
}

/// `PATCH /scim/v2/Users/{id}` — na prática, ligar e desligar pessoas.
///
/// É esta operação que o Entra ID usa para a saída de alguém
/// (`replace active=false`), e por isso é a que mais importa de todo o módulo.
pub async fn patch_user(
    State(state): State<Arc<AppState>>,
    scim: ScimAuth,
    Path(id): Path<Uuid>,
    Json(req): Json<PatchReq>,
) -> Result<Json<Value>, ScimError> {
    if read_user(&state, scim.org_id, id).await?.is_none() {
        return Err(ScimError::not_found(&id.to_string()));
    }
    if !req.schemas.iter().any(|s| s == SCHEMA_PATCH) {
        return Err(ScimError::bad("invalidValue", "falta o schema do PatchOp"));
    }
    for op in &req.operations {
        let caminho = op.path.as_deref().unwrap_or("").trim().to_lowercase();
        let acao = op.op.trim().to_lowercase();
        if !matches!(acao.as_str(), "replace" | "add") {
            return Err(ScimError::bad(
                "invalidValue",
                format!("operação «{}» não suportada", op.op),
            ));
        }
        match caminho.as_str() {
            "active" => {
                // O valor vem como booleano ou como o texto "False" — os
                // provedores não concordam, e um deles mandava texto.
                let ligado = match op.value.as_ref() {
                    Some(Value::Bool(b)) => *b,
                    Some(Value::String(s)) => s.eq_ignore_ascii_case("true"),
                    _ => {
                        return Err(ScimError::bad(
                            "invalidValue",
                            "`active` tem de ser booleano",
                        ))
                    }
                };
                if ligado {
                    reactivar(&state, scim.org_id, id).await?;
                } else {
                    suspender(&state, scim.org_id, id).await?;
                }
            }
            // Sem `path`, o valor é um objecto com os atributos a substituir —
            // é a forma que o Okta usa.
            "" => {
                if let Some(Value::Object(mapa)) = op.value.as_ref() {
                    if let Some(ligado) = mapa.get("active").and_then(Value::as_bool) {
                        if ligado {
                            reactivar(&state, scim.org_id, id).await?;
                        } else {
                            suspender(&state, scim.org_id, id).await?;
                        }
                    }
                    if let Some(nome) = mapa.get("displayName").and_then(Value::as_str) {
                        renomear(&state, id, nome).await?;
                    }
                }
            }
            "displayname" | "name.formatted" => {
                if let Some(nome) = op.value.as_ref().and_then(Value::as_str) {
                    renomear(&state, id, nome).await?;
                }
            }
            // `userName` é a identidade da conta: mudá-la é tomá-la (R25).
            "username" | "emails" | "emails[type eq \"work\"].value" => {
                return Err(ScimError::new(
                    StatusCode::BAD_REQUEST,
                    Some("mutability"),
                    "o endereço de uma conta não se muda por SCIM",
                ))
            }
            outro => {
                return Err(ScimError::bad(
                    "invalidPath",
                    format!("atributo «{outro}» não suportado"),
                ))
            }
        }
    }
    let u = read_user(&state, scim.org_id, id)
        .await?
        .ok_or_else(|| ScimError::not_found(&id.to_string()))?;
    Ok(Json(u.to_resource()))
}

/// `PUT /scim/v2/Users/{id}` — substituição. Só o que esta superfície mexe:
/// `active` e o nome.
pub async fn replace_user(
    State(state): State<Arc<AppState>>,
    scim: ScimAuth,
    Path(id): Path<Uuid>,
    Json(req): Json<ScimUserReq>,
) -> Result<Json<Value>, ScimError> {
    esquema_de_user_ok(&req.schemas)?;
    let actual = read_user(&state, scim.org_id, id)
        .await?
        .ok_or_else(|| ScimError::not_found(&id.to_string()))?;
    // O `userName` do corpo tem de ser o da conta: um PUT com outro endereço é
    // uma tentativa de mudar a identidade, não uma substituição.
    let email = delonix_meet_domain::identity::validation::normalize_email(&req.user_name);
    if email != actual.email {
        return Err(ScimError::new(
            StatusCode::BAD_REQUEST,
            Some("mutability"),
            "o endereço de uma conta não se muda por SCIM",
        ));
    }
    if let Some(nome) = req.name.as_ref().map(ScimName::display) {
        if !nome.is_empty() {
            renomear(&state, id, &nome).await?;
        }
    }
    if req.active {
        reactivar(&state, scim.org_id, id).await?;
    } else {
        suspender(&state, scim.org_id, id).await?;
    }
    let u = read_user(&state, scim.org_id, id)
        .await?
        .ok_or_else(|| ScimError::not_found(&id.to_string()))?;
    Ok(Json(u.to_resource()))
}

/// `DELETE /scim/v2/Users/{id}` — **suspende**, não apaga (ver o cabeçalho).
pub async fn delete_user(
    State(state): State<Arc<AppState>>,
    scim: ScimAuth,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, ScimError> {
    if read_user(&state, scim.org_id, id).await?.is_none() {
        return Err(ScimError::not_found(&id.to_string()));
    }
    suspender(&state, scim.org_id, id).await?;
    Ok(StatusCode::NO_CONTENT)
}

// ---------- as três operações, pelos helpers da casa ----------

async fn suspender(state: &AppState, org_id: Uuid, user_id: Uuid) -> Result<(), ScimError> {
    let mut tx = state.db.begin().await?;
    let mudou = crate::org::archive_member_tx(&mut tx, org_id, user_id, MOTIVO_SAIDA, None).await?;
    tx.commit().await?;
    if mudou {
        // Uma pessoa suspensa não pode continuar a usar as sessões que tem
        // abertas — era a diferença entre «saiu» e «saiu mesmo».
        let _ = crate::sessions::revoke_all_except(state, user_id, None).await;
        crate::audit::log(
            &state.db,
            Some(org_id),
            Uuid::nil(),
            "member.scim_exit",
            &user_id.to_string(),
        )
        .await;
    }
    Ok(())
}

/// O motivo com que o SCIM arquiva. Entra no catálogo do `archived_reason`
/// (migração 0122) ao lado do `odoo_exit`, e é a proveniência que responde «quem
/// tirou esta pessoa?» na trilha de auditoria.
const MOTIVO_SAIDA: &str = "scim_exit";

async fn reactivar(state: &AppState, org_id: Uuid, user_id: Uuid) -> Result<(), ScimError> {
    let mut tx = state.db.begin().await?;
    // A readmissão passa pelo caminho DO DIRECTÓRIO, não pelo genérico: o
    // `reactivate_member_tx` recusa uma saída de directório de propósito
    // (ADR-0008 §7), e o caminho do directório recusa desfazer uma suspensão
    // feita por um administrador. Nenhum dos dois desfaz a decisão do outro.
    let mudou = match crate::org::reactivate_from_directory_tx(
        &mut tx,
        org_id,
        user_id,
        MOTIVO_SAIDA,
    )
    .await
    {
        Ok(v) => v,
        Err(e) => {
            // Uma recusa de regra (tecto de lugares, saída de outra
            // autoridade) é 409 com explicação: é o `detail` que o painel do
            // provedor mostra a quem está a configurar.
            let status = axum::response::IntoResponse::into_response(e).status();
            return Err(ScimError::new(
                if status.is_client_error() {
                    StatusCode::CONFLICT
                } else {
                    status
                },
                None,
                "não foi possível readmitir: a saída não foi feita por este directório, \
                     ou a organização está no limite de lugares",
            ));
        }
    };
    // SEM ISTO O UPDATE É DESFEITO: a transacção cai ao sair da função e a
    // resposta sai 200 com `active: false` — foi o que o teste apanhou.
    tx.commit().await?;
    if mudou {
        crate::audit::log(
            &state.db,
            Some(org_id),
            Uuid::nil(),
            "member.scim_reactivated",
            &user_id.to_string(),
        )
        .await;
    }
    Ok(())
}

async fn renomear(state: &AppState, user_id: Uuid, nome: &str) -> Result<(), ScimError> {
    let nome: String = nome.trim().chars().take(80).collect();
    if nome.is_empty() {
        return Ok(());
    }
    let _ = sqlx::query("UPDATE users SET username = $1 WHERE id = $2")
        .bind(&nome)
        .bind(user_id)
        .execute(&state.db)
        .await?;
    Ok(())
}

// ---------- o token, emitido pelo administrador ----------

#[derive(Serialize, utoipa::ToSchema)]
pub struct ScimTokenResp {
    /// Em claro, **uma vez só**. Depois disto só existe o hash.
    pub token: String,
    /// Os primeiros 12 caracteres, o que fica visível na configuração.
    pub prefix: String,
    /// O caminho base a dar ao provedor.
    pub base_url: String,
}

/// A rota de ADMINISTRAÇÃO do SCIM entra no OpenAPI como qualquer outra da BFF.
///
/// A superfície de dados (`/scim/v2/*`) **não** entra, e isso está escrito no
/// `check-openapi.sh`: o contrato dela é a RFC 7644 mais a descoberta que ela
/// própria serve. Esta é da consola, e por isso leva spec.
#[derive(utoipa::OpenApi)]
#[openapi(paths(rotate_token), components(schemas(ScimTokenResp)))]
pub struct ApiDoc;

/// `POST /api/orgs/{org_id}/scim/rotate-token` — gera ou roda o token do SCIM.
///
/// Invalida o anterior e LIGA a superfície no mesmo passo: rodar a chave numa
/// organização desligada dava um token que nunca autenticava (o defeito que o
/// Odoo já teve).
#[utoipa::path(
    post, path = "/api/orgs/{org_id}/scim/rotate-token", tag = "scim",
    security(("session" = [])),
    params(("org_id" = Uuid, Path)),
    responses(
        (status = 200, body = ScimTokenResp),
        (status = 401, body = crate::openapi::ErrorBody, description = "sessão inválida, ou membro sem papel de admin"),
        (status = 404, body = crate::openapi::ErrorBody, description = "não é membro da organização"),
    )
)]
pub async fn rotate_token(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(org_id): Path<Uuid>,
) -> Result<Json<ScimTokenResp>, ApiError> {
    crate::org::require_admin_pub(&state, org_id, auth.user_id).await?;
    let token = prefixed_token(TOKEN_PREFIX);
    let prefix = token[..12].to_string();
    let hash = sha256_hex(&token);
    sqlx::query(
        "UPDATE organizations
            SET scim_token_hash = $1, scim_token_prefix = $2, scim_enabled = TRUE
          WHERE id = $3",
    )
    .bind(&hash)
    .bind(&prefix)
    .bind(org_id)
    .execute(&state.db)
    .await?;
    crate::audit::log(
        &state.db,
        Some(org_id),
        auth.user_id,
        "org.scim_token_rotated",
        &prefix,
    )
    .await;
    let base = state
        .config
        .public_url
        .as_deref()
        .unwrap_or_default()
        .to_string();
    Ok(Json(ScimTokenResp {
        token,
        prefix,
        base_url: format!("{base}/scim/v2"),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn o_filtro_que_os_provedores_mandam() {
        assert_eq!(
            email_do_filtro("userName eq \"Maria@Exemplo.AO\"").unwrap(),
            Some("maria@exemplo.ao".to_string())
        );
        // O Okta manda sem maiúsculas no campo; o Entra ID manda `eq` colado.
        assert_eq!(
            email_do_filtro("username eq \"a@b.ao\"").unwrap(),
            Some("a@b.ao".to_string())
        );
        assert!(email_do_filtro("").unwrap().is_none());
    }

    /// Um filtro que não entendemos é um ERRO, não uma lista vazia: uma lista
    /// vazia faria o provedor concluir que a pessoa não existe — e criá-la
    /// outra vez, ou suspendê-la.
    #[test]
    fn filtro_desconhecido_e_erro_e_nao_lista_vazia() {
        for f in [
            "displayName eq \"Ana\"",
            "userName co \"ana\"",
            "userName eq \"\"",
        ] {
            let e = email_do_filtro(f).expect_err(f);
            assert_eq!(e.status, StatusCode::BAD_REQUEST, "{f}");
        }
    }

    #[test]
    fn o_nome_vem_do_que_o_directorio_mandar() {
        let n = ScimName {
            formatted: None,
            given_name: Some("Ana".into()),
            family_name: Some("Silva".into()),
        };
        assert_eq!(n.display(), "Ana Silva");
        let n = ScimName {
            formatted: Some("  Ana Maria Silva ".into()),
            given_name: Some("Ana".into()),
            family_name: None,
        };
        assert_eq!(n.display(), "Ana Maria Silva");
    }

    /// Tolerante de propósito: uma lista vazia passa (há provedores que a
    /// omitem), uma lista que fala de outra coisa é recusada.
    #[test]
    fn o_esquema_declarado_e_validado_sem_ser_rigido() {
        assert!(esquema_de_user_ok(&[]).is_ok());
        assert!(esquema_de_user_ok(&[SCHEMA_USER.to_string()]).is_ok());
        let e = esquema_de_user_ok(&["urn:ietf:params:scim:schemas:core:2.0:Group".into()])
            .expect_err("um Group não é um User");
        assert_eq!(e.status, StatusCode::BAD_REQUEST);
    }

    /// As regras da norma para a página (RFC 7644 §3.4.2.4).
    #[test]
    fn a_pagina_segue_a_norma() {
        let p = |s, c| pagina(s, c).unwrap();
        // Omisso: do princípio, até ao tecto declarado na descoberta.
        assert_eq!(
            p(None, None),
            Pagina {
                start: 1,
                count: MAX_RESULTS
            }
        );
        // O que o Entra ID manda na segunda volta.
        assert_eq!(
            p(Some("201"), Some("100")),
            Pagina {
                start: 201,
                count: 100
            }
        );
        // `startIndex` < 1 vale 1; `count` negativo vale 0; acima do tecto, o tecto.
        assert_eq!(p(Some("0"), Some("-5")), Pagina { start: 1, count: 0 });
        assert_eq!(
            p(Some("-3"), Some("100000")),
            Pagina {
                start: 1,
                count: MAX_RESULTS
            }
        );
        // `count=0` é um pedido legítimo: só o `totalResults`.
        assert_eq!(p(Some("1"), Some("0")), Pagina { start: 1, count: 0 });
    }

    /// Um valor que não é inteiro é um erro no envelope da norma — não uma
    /// página escolhida por nós que o provedor tomaria pela que pediu.
    #[test]
    fn pagina_com_lixo_e_erro() {
        for (s, c) in [(Some("um"), None), (None, Some("10.5")), (Some(""), None)] {
            let e = pagina(s, c).expect_err("lixo tem de ser recusado");
            assert_eq!(e.status, StatusCode::BAD_REQUEST);
            assert_eq!(e.kind, Some("invalidValue"));
        }
    }

    /// O envelope de erro é o da norma: é o `detail` que o painel do Entra ID
    /// mostra a quem está a configurar.
    #[test]
    fn o_erro_sai_no_envelope_do_scim() {
        let e = ScimError::bad("invalidFilter", "só `userName eq`");
        assert_eq!(e.status, StatusCode::BAD_REQUEST);
        assert_eq!(e.kind, Some("invalidFilter"));
    }
}
