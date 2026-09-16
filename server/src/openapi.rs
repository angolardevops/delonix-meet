//! Especificação OpenAPI 3.1 gerada a partir do código (ADR-0005 §3).
//!
//! Dois documentos, um por superfície e por regime de compatibilidade:
//! - **BFF** (`/api/openapi.json`): o contrato do web Delonix. Instável — muda
//!   com o web — mas descrito, para o cliente TypeScript ser GERADO em vez de
//!   escrito à mão.
//! - **v1** (`/api/v1/openapi.json`): a superfície pública estável.
//!
//! Cada módulo declara o seu `ApiDoc` (`#[derive(OpenApi)]` com os `paths` dos
//! seus handlers); aqui só se juntam. O spec commitado em
//! `docs/reference/openapi/` tem de ser igual ao gerado
//! (`scripts/check-openapi.sh`), e uma rota montada sem documentação conta na
//! catraca desse portão.

use axum::Json;
use serde::Serialize;
use utoipa::{
    openapi::security::{HttpAuthScheme, HttpBuilder, SecurityScheme},
    Modify, OpenApi, ToSchema,
};

/// Envelope de erro de todas as superfícies (ver `error.rs`).
#[derive(Serialize, ToSchema)]
pub struct ErrorBody {
    /// Mensagem para pessoas. Pode mudar.
    pub error: String,
    /// Código estável (contrato), p.ex. `registration.domain_taken`.
    pub code: String,
    pub details: Vec<FieldViolation>,
    /// Igual ao cabeçalho `X-Request-Id`.
    pub request_id: Option<String>,
}

#[derive(Serialize, ToSchema)]
pub struct FieldViolation {
    pub field: String,
    pub description: String,
}

struct Security;

impl Modify for Security {
    fn modify(&self, openapi: &mut utoipa::openapi::OpenApi) {
        let components = openapi.components.get_or_insert_with(Default::default);
        components.add_security_scheme(
            "session",
            SecurityScheme::Http(
                HttpBuilder::new()
                    .scheme(HttpAuthScheme::Bearer)
                    .bearer_format("JWT")
                    .description(Some(
                        "Access token da sessão (15 min), renovado por /api/auth/refresh.",
                    ))
                    .build(),
            ),
        );
        components.add_security_scheme(
            "api_key",
            SecurityScheme::Http(
                HttpBuilder::new()
                    .scheme(HttpAuthScheme::Bearer)
                    .description(Some("Chave de API da organização (`dlx_…`)."))
                    .build(),
            ),
        );
    }
}

#[derive(OpenApi)]
#[openapi(
    info(
        title = "Delonix Meet — BFF",
        description = "API interna do web Delonix. **Instável**: muda com o frontend. Erros no envelope `ErrorBody`.",
    ),
    components(schemas(ErrorBody, FieldViolation)),
    modifiers(&Security)
)]
struct BffDoc;

#[derive(OpenApi)]
#[openapi(
    info(
        title = "Delonix Meet — API pública v1",
        description = "Superfície estável para SDK, mobile e integrações. Autentica por chave `dlx_`. Muda só com v2.",
    ),
    components(schemas(ErrorBody, FieldViolation)),
    modifiers(&Security)
)]
struct V1Doc;

/// O documento da BFF, com os `ApiDoc` de cada módulo.
pub fn bff() -> utoipa::openapi::OpenApi {
    let mut doc = BffDoc::openapi();
    doc.info.version = env!("CARGO_PKG_VERSION").to_string();
    for part in bff_parts() {
        doc.merge(part);
    }
    doc
}

/// O documento da v1.
pub fn v1() -> utoipa::openapi::OpenApi {
    let mut doc = V1Doc::openapi();
    doc.info.version = env!("CARGO_PKG_VERSION").to_string();
    for part in v1_parts() {
        doc.merge(part);
    }
    doc
}

/// Os módulos que já documentam as suas rotas da BFF. Um módulo novo entra aqui.
fn bff_parts() -> Vec<utoipa::openapi::OpenApi> {
    vec![
        crate::users::ApiDoc::openapi(),
        crate::webhooks::ApiDoc::openapi(),
        crate::meetings::ApiDoc::openapi(),
        crate::actions::ApiDoc::openapi(),
        crate::presence::ApiDoc::openapi(),
        crate::ai::ApiDoc::openapi(),
    ]
}

fn v1_parts() -> Vec<utoipa::openapi::OpenApi> {
    vec![crate::meetings_v1::ApiDoc::openapi()]
}

pub async fn bff_json() -> Json<utoipa::openapi::OpenApi> {
    Json(bff())
}

pub async fn v1_json() -> Json<utoipa::openapi::OpenApi> {
    Json(v1())
}

/// JSON estável (chaves ordenadas pelo serde_json/preserve_order desligado) para
/// o spec commitado.
pub fn to_pretty(doc: &utoipa::openapi::OpenApi) -> String {
    let value = serde_json::to_value(doc).expect("OpenAPI serializável");
    serde_json::to_string_pretty(&value).expect("JSON") + "\n"
}
