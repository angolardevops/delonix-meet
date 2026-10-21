//! A superfície MCP (ADR-0027) — a quinta superfície deste servidor, para
//! agentes.
//!
//! **Uma tool é um pedido do BFF com a mesma autorização.** Não há caminho
//! próprio para a base de dados nem para o LLM: cada tool monta as entradas do
//! handler do BFF e chama-o. É por isso que herda, sem repetir, a visibilidade
//! de cada recurso, o DLP, os limites por organização e a auditoria. Um
//! ficheiro deste módulo com um `SELECT` ou um `prompt` está errado.
//!
//! **Não-invasão em três camadas**, nenhuma delas no cliente:
//! 1. o **escopo** do token (ADR-0028 §6) — sem ele a tool não é anunciada nem
//!    aceita;
//! 2. a **capacidade** da pessoa (ADR-0008) — decidida por `org::decide`;
//! 3. o **alcance** do recurso — o mesmo `404` que o browser recebe, e um id de
//!    outra organização é indistinguível de um inexistente.
//!
//! E a regra que fecha a porta mais fácil: **nenhuma tool aceita `org_id` nem
//! `user_id`**. A organização vem do token, que a trouxe do consentimento.
//!
//! **Sem estado.** Não emitimos `Mcp-Session-Id`: o token já diz quem é, em
//! que organização e com que escopos, e uma sessão MCP não acrescentava nada —
//! acrescentaria um estado partilhado entre três réplicas (ADR-0001) para
//! guardar o que já vem em cada pedido. O protocolo permite-o: um servidor que
//! não devolve o cabeçalho não o recebe de volta.

use std::sync::Arc;

use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use delonix_meet_domain::{
    identity::{
        authorization::{Capability, Decision, ResourceScope},
        oauth::McpScope,
    },
    integration::mcp::{Tool, PROTOCOL_VERSION},
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use uuid::Uuid;

use crate::{auth::AuthUser, error::ApiError, oauth, AppState};

/// Tamanho máximo do corpo que se lê de uma resposta do BFF ao serializá-la
/// para JSON-RPC. Um VTT grande cabe aqui; uma gravação não passa por aqui.
const MAX_BFF_BODY: usize = 4 * 1024 * 1024;

// ---------------------------------------------------------------------------
//  1. Autenticação: o token do agente
// ---------------------------------------------------------------------------

/// Pedido autenticado por um token de agente.
///
/// Verifica o que o extractor do BFF **não** verificaria: o `typ` é `mcp` (e
/// não `access`), e o `aud` é o recurso canónico deste servidor (RFC 8707).
/// Depois pergunta à sessão, como o `AuthUser` pergunta — terminar a sessão
/// mata o agente no mesmo pedido.
pub(crate) struct McpAuth {
    pub user_id: Uuid,
    pub session_id: Uuid,
    pub org_id: Uuid,
    pub client_id: String,
    pub scopes: Vec<McpScope>,
}

impl McpAuth {
    /// A pessoa, como o resto do servidor a conhece. É isto que se passa aos
    /// handlers do BFF — e é sólido porque a sessão foi confirmada acima.
    fn as_user(&self) -> AuthUser {
        AuthUser {
            user_id: self.user_id,
            session_id: Some(self.session_id),
            reauthenticated_at: None,
        }
    }

    fn has(&self, scope: McpScope) -> bool {
        self.scopes.contains(&scope)
    }
}

/// A recusa desta superfície.
///
/// É um tipo próprio por uma razão da especificação MCP: o `401` tem de levar
/// `WWW-Authenticate` com o endereço do documento de metadata do recurso, para
/// o cliente descobrir sozinho onde autorizar. Um `ApiError::Unauthorized` nu
/// não o escreve — e sem ele o cliente não sabe o que fazer com o `401`.
pub(crate) struct McpRejection {
    /// O endereço do documento de metadata; `None` sem `PUBLIC_URL`.
    metadata: Option<String>,
    /// Quando o OAuth está desligado, é isso que se diz (e não «não
    /// autenticado», que mandaria o cliente tentar outra vez para sempre).
    desligado: bool,
}

impl IntoResponse for McpRejection {
    fn into_response(self) -> Response {
        if self.desligado {
            return ApiError::ServiceUnavailable(
                "MCP desligado: sem PUBLIC_URL não há identificador de recurso para o token".into(),
            )
            .into_response();
        }
        let mut res = ApiError::Unauthorized.into_response();
        if let Some(url) = self.metadata {
            if let Ok(v) =
                axum::http::HeaderValue::from_str(&format!("Bearer resource_metadata=\"{url}\""))
            {
                res.headers_mut()
                    .insert(axum::http::header::WWW_AUTHENTICATE, v);
            }
        }
        res
    }
}

fn recusa(state: &AppState) -> McpRejection {
    McpRejection {
        metadata: state
            .config
            .public_url
            .as_deref()
            .map(|b| format!("{b}/.well-known/oauth-protected-resource")),
        desligado: false,
    }
}

impl axum::extract::FromRequestParts<Arc<AppState>> for McpAuth {
    type Rejection = McpRejection;

    async fn from_request_parts(
        parts: &mut axum::http::request::Parts,
        state: &Arc<AppState>,
    ) -> Result<Self, Self::Rejection> {
        let token = crate::auth::bearer_token(&parts.headers).ok_or_else(|| recusa(state))?;
        let claims: oauth::McpClaims = crate::auth::verify_claims(&state.config.jwt_secret, token)
            .map_err(|_| recusa(state))?;
        if claims.typ != "mcp" {
            return Err(recusa(state));
        }
        // O público do token. Sem isto, um token emitido para outro recurso
        // servia aqui — e é esse o ponto do RFC 8707.
        let canonical = oauth::canonical_resource(state).map_err(|_| McpRejection {
            metadata: None,
            desligado: true,
        })?;
        if claims.aud != canonical {
            return Err(recusa(state));
        }
        crate::sessions::ensure_active(state, claims.sub, claims.sid)
            .await
            .map_err(|_| recusa(state))?;
        Ok(McpAuth {
            user_id: claims.sub,
            session_id: claims.sid,
            org_id: claims.org,
            client_id: claims.client_id,
            scopes: claims
                .scope
                .split_whitespace()
                .filter_map(McpScope::parse)
                .collect(),
        })
    }
}

// ---------------------------------------------------------------------------
//  2. JSON-RPC 2.0
// ---------------------------------------------------------------------------

/// Uma mensagem JSON-RPC 2.0. Documentada para o contrato ser legível, mas o
/// handler lê o corpo como texto: a mensagem pode ser uma notificação (sem
/// `id`) e um extractor tipado responderia `422` a algo que é válido.
#[derive(Deserialize, utoipa::ToSchema)]
pub struct RpcRequest {
    #[allow(dead_code)]
    jsonrpc: String,
    /// Ausente numa **notificação** — e uma notificação não leva resposta.
    #[serde(default)]
    id: Option<Value>,
    method: String,
    #[serde(default)]
    params: Value,
}

#[derive(Serialize)]
struct RpcError {
    code: i32,
    message: String,
}

/// Códigos do JSON-RPC 2.0 que usamos, com o que cada um significa aqui.
mod codigo {
    /// O corpo não é JSON-RPC.
    pub const PEDIDO_INVALIDO: i32 = -32600;
    /// Método ou tool que não existe — ou que o token não abre, que para quem
    /// chama é o mesmo: não está lá.
    pub const NAO_EXISTE: i32 = -32601;
    /// Os argumentos não casam com o esquema.
    pub const ARGS_INVALIDOS: i32 = -32602;
}

fn resposta(id: Option<Value>, resultado: Result<Value, RpcError>) -> Response {
    // Uma notificação não leva corpo (JSON-RPC 2.0 §4.1). `202` é o que o
    // transporte Streamable HTTP manda devolver.
    let Some(id) = id else {
        return StatusCode::ACCEPTED.into_response();
    };
    let corpo = match resultado {
        Ok(v) => json!({"jsonrpc": "2.0", "id": id, "result": v}),
        Err(e) => {
            json!({"jsonrpc": "2.0", "id": id, "error": {"code": e.code, "message": e.message}})
        }
    };
    // O estado HTTP é 200 mesmo com erro JSON-RPC: o erro é da chamada, não do
    // transporte, e um cliente que olhe para o estado HTTP tem de ver que a
    // mensagem chegou.
    Json(corpo).into_response()
}

/// `POST /api/mcp` — uma mensagem JSON-RPC por pedido.
#[utoipa::path(
    post, path = "/api/mcp", tag = "mcp",
    security(("mcp" = [])),
    request_body = RpcRequest,
    responses(
        (status = 200, description = "A resposta JSON-RPC. Um erro da CHAMADA vem aqui com `error` (ou, numa tool, com `isError: true`): o estado HTTP diz que a mensagem chegou."),
        (status = 202, description = "Era uma notificação (sem `id`): não leva corpo."),
        (status = 401, body = crate::openapi::ErrorBody, description = "Sem token de agente, com outro `typ`/`aud`, ou com a sessão terminada. Leva `WWW-Authenticate` com o endereço da metadata do recurso."),
        (status = 429, body = crate::openapi::ErrorBody),
        (status = 503, body = crate::openapi::ErrorBody, description = "Sem `PUBLIC_URL` o MCP está fechado."),
    )
)]
pub async fn post(State(state): State<Arc<AppState>>, auth: McpAuth, body: String) -> Response {
    let req: RpcRequest = match serde_json::from_str(&body) {
        Ok(r) => r,
        Err(e) => {
            return resposta(
                Some(Value::Null),
                Err(RpcError {
                    code: codigo::PEDIDO_INVALIDO,
                    message: format!("não é uma mensagem JSON-RPC: {e}"),
                }),
            )
        }
    };
    let id = req.id.clone();
    let resultado = dispatch(&state, &auth, &req).await;
    resposta(id, resultado)
}

/// `GET /api/mcp` — o fluxo de eventos do servidor para o cliente.
#[utoipa::path(
    get, path = "/api/mcp", tag = "mcp",
    security(("mcp" = [])),
    responses(
        (status = 405, description = "Não há fluxo: nenhuma tool emite notificações por iniciativa do servidor."),
        (status = 401, body = crate::openapi::ErrorBody),
    )
)]
pub async fn get(_auth: McpAuth) -> Response {
    StatusCode::METHOD_NOT_ALLOWED.into_response()
}

/// `DELETE /api/mcp` — terminar a sessão MCP.
#[utoipa::path(
    delete, path = "/api/mcp", tag = "mcp",
    security(("mcp" = [])),
    responses(
        (status = 405, description = "Não há sessão MCP para terminar: quem corta o agente termina a SESSÃO em «A minha conta», ou retira o consentimento."),
        (status = 401, body = crate::openapi::ErrorBody),
    )
)]
pub async fn delete(_auth: McpAuth) -> Response {
    StatusCode::METHOD_NOT_ALLOWED.into_response()
}

async fn dispatch(
    state: &Arc<AppState>,
    auth: &McpAuth,
    req: &RpcRequest,
) -> Result<Value, RpcError> {
    match req.method.as_str() {
        "initialize" => Ok(json!({
            "protocolVersion": PROTOCOL_VERSION,
            // Só tools. `sampling` e `elicitation` ficam declaradas ausentes de
            // propósito (ADR-0027 §9): deixar um agente externo fazer o nosso
            // servidor chamar o LLM é gastar o modelo partilhado de outra
            // organização.
            "capabilities": {"tools": {"listChanged": false}},
            "serverInfo": {"name": "delonix-meet", "version": env!("CARGO_PKG_VERSION")},
            "instructions": "As tools agem como a pessoa que autorizou este agente: vêem o que ela vê, na organização dela, e nada mais. O que escreve fica na trilha de auditoria com o nome dela.",
        })),
        "notifications/initialized" | "ping" => Ok(json!({})),
        "tools/list" => Ok(json!({"tools": listar(state, auth).await})),
        "tools/call" => chamar(state, auth, &req.params).await,
        outro => Err(RpcError {
            code: codigo::NAO_EXISTE,
            message: format!("método desconhecido: {outro}"),
        }),
    }
}

// ---------------------------------------------------------------------------
//  3. tools/list — só o que ESTA pessoa pode fazer
// ---------------------------------------------------------------------------

/// A pessoa tem esta capacidade nesta organização?
///
/// `requires_approval` conta como **sim**: a tool é anunciada, e a chamada
/// devolve `authz.approval_required` com o pedido criado — que é uma resposta
/// accionável, ao contrário de uma tool que não aparece.
async fn pode(state: &Arc<AppState>, auth: &McpAuth, cap: Capability) -> bool {
    matches!(
        crate::org::decide(
            state,
            auth.org_id,
            auth.user_id,
            cap,
            ResourceScope::Organization,
        )
        .await,
        Ok(Some((_, Decision::Allow))) | Ok(Some((_, Decision::RequiresApproval)))
    )
}

/// O catálogo filtrado pelo cruzamento dos escopos do token com as capacidades
/// da pessoa (ADR-0027 §5).
///
/// Anunciar uma tool que vai responder `403` é vender o que não se tem — e é a
/// `tools/list` que o agente lê como contrato.
async fn listar(state: &Arc<AppState>, auth: &McpAuth) -> Vec<Value> {
    let mut out = Vec::new();
    for t in Tool::ALL {
        if !auth.has(t.scope) {
            continue;
        }
        if let Some(cap) = t.capability {
            if !pode(state, auth, cap).await {
                continue;
            }
        }
        let schema: Value = serde_json::from_str(t.input_schema)
            .unwrap_or_else(|_| json!({"type": "object", "properties": {}}));
        out.push(json!({
            "name": t.name,
            "description": t.summary,
            "inputSchema": schema,
            "annotations": {"readOnlyHint": t.read_only},
        }));
    }
    out
}

// ---------------------------------------------------------------------------
//  4. tools/call
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct CallParams {
    name: String,
    #[serde(default)]
    arguments: Value,
}

/// Um erro de EXECUÇÃO devolve-se dentro do resultado (`isError: true`), não
/// como erro de JSON-RPC: é informação para o modelo trabalhar, e o protocolo
/// distingue as duas coisas. Um `404` de um recurso de outra organização chega
/// aqui como texto — e continua a não dizer se existe.
fn erro_de_tool(msg: String) -> Value {
    json!({"content": [{"type": "text", "text": msg}], "isError": true})
}

fn ok_de_tool(valor: Value) -> Value {
    json!({
        // O texto é o que um modelo sem suporte a conteúdo estruturado lê.
        "content": [{"type": "text", "text": serde_json::to_string_pretty(&valor).unwrap_or_default()}],
        "structuredContent": valor,
        "isError": false,
    })
}

async fn chamar(state: &Arc<AppState>, auth: &McpAuth, params: &Value) -> Result<Value, RpcError> {
    let p: CallParams = serde_json::from_value(params.clone()).map_err(|e| RpcError {
        code: codigo::ARGS_INVALIDOS,
        message: format!("params inválidos: {e}"),
    })?;
    let tool = Tool::by_name(&p.name).ok_or_else(|| RpcError {
        code: codigo::NAO_EXISTE,
        message: format!("tool desconhecida: {}", p.name),
    })?;
    // Sem o escopo, a tool não existe para este token — a mesma resposta que
    // uma tool inventada, porque é o que a `tools/list` dele diz.
    if !auth.has(tool.scope) {
        return Err(RpcError {
            code: codigo::NAO_EXISTE,
            message: format!("tool desconhecida: {}", p.name),
        });
    }
    // A capacidade impõe-se ANTES de tocar no recurso, pelo motor do ADR-0008.
    if let Some(cap) = tool.capability {
        if let Err(e) = crate::org::require_capability(
            state,
            auth.org_id,
            auth.user_id,
            cap,
            ResourceScope::Organization,
        )
        .await
        {
            return Ok(erro_de_tool(texto_de_erro(e)));
        }
    }
    let resultado = executar(state, auth, tool, &p.arguments).await;
    // ADR-0027 §8: um agente é tão rastreável como a pessoa. Só o que ESCREVE
    // entra na trilha — uma leitura por minuto encheria a auditoria de ruído e
    // afogava o que importa. Grava-se o nome da tool, o cliente MCP e como
    // acabou, para que «foi o agente» seja uma afirmação verificável.
    if !tool.read_only {
        let como = match &resultado {
            Ok(_) => "ok",
            Err(Executar::Args(_)) => "argumentos_invalidos",
            Err(Executar::Api(_)) => "recusado",
        };
        crate::audit::log(
            &state.db,
            Some(auth.org_id),
            auth.user_id,
            "mcp.tool_call",
            &format!("{} via {} ({como})", tool.name, auth.client_id),
        )
        .await;
    }
    match resultado {
        Ok(v) => Ok(ok_de_tool(v)),
        Err(Executar::Args(msg)) => Err(RpcError {
            code: codigo::ARGS_INVALIDOS,
            message: msg,
        }),
        Err(Executar::Api(e)) => Ok(erro_de_tool(texto_de_erro(e))),
    }
}

enum Executar {
    /// Os argumentos não casam com o esquema — erro de protocolo.
    Args(String),
    /// O servidor recusou (autorização, alcance, estado) — erro de execução.
    Api(ApiError),
}

impl From<ApiError> for Executar {
    fn from(e: ApiError) -> Self {
        Executar::Api(e)
    }
}

/// O que o modelo lê quando a chamada é recusada. Usa o CÓDIGO estável do
/// erro, que é contrato, mais a mensagem — sem inventar explicação nenhuma.
fn texto_de_erro(e: ApiError) -> String {
    match &e {
        ApiError::NotFound => {
            "não encontrado (`not_found`) — ou não existe, ou não tens acesso; a resposta é a mesma"
                .into()
        }
        ApiError::Domain(d) => format!("{} — {}", d.code, d.message),
        ApiError::Forbidden => "sem permissão (`permission_denied`)".into(),
        ApiError::Unauthorized => "sessão inválida (`unauthenticated`)".into(),
        outro => format!("recusado: {outro:?}"),
    }
}

/// Lê um `Uuid` obrigatório dos argumentos.
fn uuid_de(args: &Value, campo: &str) -> Result<Uuid, Executar> {
    args.get(campo)
        .and_then(Value::as_str)
        .and_then(|s| Uuid::parse_str(s).ok())
        .ok_or_else(|| Executar::Args(format!("falta `{campo}`, ou não é um uuid")))
}

fn texto_de(args: &Value, campo: &str) -> Result<String, Executar> {
    args.get(campo)
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| Executar::Args(format!("falta `{campo}`")))
}

/// Serializa a resposta de um handler do BFF que devolve `Response` em vez de
/// `Json<T>` (a lista de reuniões e a biblioteca devolvem-no, por causa da
/// paginação por cursor).
async fn json_de_resposta(res: Response) -> Result<Value, Executar> {
    let (partes, corpo) = res.into_parts();
    let bytes = axum::body::to_bytes(corpo, MAX_BFF_BODY)
        .await
        .map_err(|e| Executar::Api(ApiError::Internal(e.to_string())))?;
    if !partes.status.is_success() {
        let texto = String::from_utf8_lossy(&bytes).to_string();
        return Err(Executar::Api(ApiError::Internal(texto)));
    }
    serde_json::from_slice(&bytes).map_err(|e| Executar::Api(ApiError::Internal(e.to_string())))
}

/// A ligação de cada tool ao handler do BFF que faz o trabalho.
///
/// É aqui, e só aqui, que se vê o que cada tool é: um `State`, o `AuthUser`
/// desta pessoa, e os extractores montados a partir dos argumentos. Nenhuma
/// linha deste bloco decide autorização — isso é do handler, como no browser.
async fn executar(
    state: &Arc<AppState>,
    auth: &McpAuth,
    tool: &Tool,
    args: &Value,
) -> Result<Value, Executar> {
    use axum::extract::{Path, Query};
    let user = auth.as_user();
    let st = State(state.clone());

    match tool.name {
        "meetings_list" => {
            let res = crate::meetings::list(st, user, Query(Default::default())).await?;
            json_de_resposta(res).await
        }
        "meeting_get" => {
            let id = uuid_de(args, "meeting_id")?;
            Ok(serde_json::to_value(crate::meetings::get_one(st, user, Path(id)).await?.0)
                .unwrap_or(Value::Null))
        }
        "meeting_minutes_get" => {
            let id = uuid_de(args, "meeting_id")?;
            Ok(serde_json::to_value(crate::meetings::minutes(st, user, Path(id)).await?.0)
                .unwrap_or(Value::Null))
        }
        "meeting_minutes_summarize" => {
            let id = uuid_de(args, "meeting_id")?;
            let (_, Json(estado)) =
                crate::meetings::request_mom_summary(st, user, Path(id)).await?;
            Ok(serde_json::to_value(estado).unwrap_or(Value::Null))
        }
        "recordings_list" => {
            let res = crate::recordings::library(
                st,
                user,
                Query(Default::default()),
                Query(Default::default()),
            )
            .await?;
            json_de_resposta(res).await
        }
        "recording_get" => {
            let id = uuid_de(args, "recording_id")?;
            Ok(
                serde_json::to_value(crate::recordings::details(st, user, Path(id)).await?.0)
                    .unwrap_or(Value::Null),
            )
        }
        "recording_transcript_get" => {
            let id = uuid_de(args, "recording_id")?;
            Ok(serde_json::to_value(
                crate::recording_meta::transcript(st, user, Path(id)).await?.0,
            )
            .unwrap_or(Value::Null))
        }
        "recording_captions_list" => {
            let id = uuid_de(args, "recording_id")?;
            Ok(serde_json::to_value(
                crate::recording_captions::list(st, user, Path(id)).await?.0,
            )
            .unwrap_or(Value::Null))
        }
        "recording_caption_get" => {
            let id = uuid_de(args, "recording_id")?;
            let lang = texto_de(args, "lang")?;
            let res = crate::recording_captions::vtt(st, user, Path((id, lang)))
                .await?
                .into_response();
            let (partes, corpo) = res.into_parts();
            let bytes = axum::body::to_bytes(corpo, MAX_BFF_BODY)
                .await
                .map_err(|e| Executar::Api(ApiError::Internal(e.to_string())))?;
            if !partes.status.is_success() {
                return Err(Executar::Api(ApiError::NotFound));
            }
            // O VTT é texto, não JSON: entrega-se como texto.
            Ok(json!({"vtt": String::from_utf8_lossy(&bytes)}))
        }
        "recording_captions_translate" => {
            let id = uuid_de(args, "recording_id")?;
            let req = serde_json::from_value(json!({
                "lang": texto_de(args, "lang")?,
                "source": texto_de(args, "source")?,
            }))
            .map_err(|e| Executar::Args(format!("argumentos inválidos: {e}")))?;
            let res = crate::recording_captions::generate(st, user, Path(id), Json(req))
                .await?
                .into_response();
            json_de_resposta(res).await
        }
        "recording_chapters_list" => {
            let id = uuid_de(args, "recording_id")?;
            Ok(serde_json::to_value(
                crate::recordings::list_chapters(st, user, Path(id), Query(Default::default()))
                    .await?
                    .0,
            )
            .unwrap_or(Value::Null))
        }
        "recording_chapters_generate" => {
            let id = uuid_de(args, "recording_id")?;
            Ok(serde_json::to_value(
                crate::recording_chapters::generate(st, user, Path(id)).await?.0,
            )
            .unwrap_or(Value::Null))
        }
        "recording_chapters_status" => {
            let id = uuid_de(args, "recording_id")?;
            Ok(serde_json::to_value(
                crate::recording_chapters::generation(st, user, Path(id)).await?.0,
            )
            .unwrap_or(Value::Null))
        }
        "recording_clip_create" => {
            let id = uuid_de(args, "recording_id")?;
            let req = serde_json::from_value(args.clone())
                .map_err(|e| Executar::Args(format!("argumentos inválidos: {e}")))?;
            let (_, Json(clip)) =
                crate::recording_clips::create(st, user, Path(id), Json(req)).await?;
            Ok(serde_json::to_value(clip).unwrap_or(Value::Null))
        }
        "recording_clips_list" => {
            let id = uuid_de(args, "recording_id")?;
            Ok(serde_json::to_value(
                crate::recording_clips::list(st, user, Path(id), Query(Default::default()))
                    .await?
                    .0,
            )
            .unwrap_or(Value::Null))
        }
        "recording_clip_status" => {
            let rec = uuid_de(args, "recording_id")?;
            let clip = uuid_de(args, "clip_id")?;
            Ok(serde_json::to_value(
                crate::recording_clips::get_one(st, user, Path((rec, clip)))
                    .await?
                    .0,
            )
            .unwrap_or(Value::Null))
        }
        "edit_suggestions" => {
            let req = serde_json::from_value(args.clone())
                .map_err(|e| Executar::Args(format!("argumentos inválidos: {e}")))?;
            // A organização vem do TOKEN, nunca dos argumentos (ADR-0027 §4).
            let res =
                crate::ai_studio::suggestions(st, user, Path(auth.org_id), Json(req)).await?;
            json_de_resposta(res).await
        }
        outro => Err(Executar::Args(format!(
            "a tool `{outro}` está no catálogo e não está ligada — é um defeito, não um pedido inválido"
        ))),
    }
}

#[derive(utoipa::OpenApi)]
#[openapi(paths(post, get, delete), components(schemas(RpcRequest)))]
pub struct ApiDoc;
