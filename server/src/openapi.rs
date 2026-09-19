//! Especificação OpenAPI 3.1 gerada a partir do código (ADR-0006 §3).
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
#[openapi(paths(crate::status), components(schemas(crate::StatusResp)))]
struct PlatformDoc;

macro_rules! surface_doc {
    ($name:ident, $title:literal, $desc:literal) => {
        #[derive(OpenApi)]
        #[openapi(
                                                    info(title = $title, description = $desc),
                                                    components(schemas(ErrorBody, FieldViolation)),
                                                    modifiers(&Security)
                                                )]
        struct $name;
    };
}

surface_doc!(
    BffDoc,
    "Delonix Meet — BFF",
    "API do web Delonix (sessão). **Instável**: muda com o frontend. Erros no envelope `ErrorBody`."
);
surface_doc!(
    V1Doc,
    "Delonix Meet — API pública v1",
    "Superfície estável do inquilino para SDK, mobile e integrações. Chave `dlx_` com escopos. Muda só com v2."
);
surface_doc!(
    OperatorDoc,
    "Delonix Meet — Operador",
    "Superfície de quem opera a plataforma (ADR-0004 §4): provisionamento de organizações, armazenamento e nós. Fora do SDK do inquilino."
);
surface_doc!(
    IntegrationsDoc,
    "Delonix Meet — Integrações",
    "Módulo Odoo `nk_delonix_meet` (token `dlxo_`) e agente de SMS por USB (token `dlxg_`)."
);

/// Todos os `ApiDoc` dos módulos. Um módulo novo entra aqui; a superfície a
/// que cada rota pertence decide-se pelo prefixo do caminho, não por esta lista.
fn parts() -> Vec<utoipa::openapi::OpenApi> {
    vec![
        PlatformDoc::openapi(),
        crate::nodes::ApiDoc::openapi(),
        crate::users::ApiDoc::openapi(),
        crate::webhooks::ApiDoc::openapi(),
        crate::stream_destinations::ApiDoc::openapi(),
        crate::broadcast::ApiDoc::openapi(),
        crate::usage::ApiDoc::openapi(),
        crate::sms::ApiDoc::openapi(),
        crate::notifications::ApiDoc::openapi(),
        crate::meetings::ApiDoc::openapi(),
        crate::actions::ApiDoc::openapi(),
        crate::presence::ApiDoc::openapi(),
        crate::ai::ApiDoc::openapi(),
        crate::rooms::ApiDoc::openapi(),
        crate::recordings::ApiDoc::openapi(),
        crate::whiteboards::ApiDoc::openapi(),
        crate::voice::ApiDoc::openapi(),
        crate::odoo::ApiDoc::openapi(),
        crate::auth::ApiDoc::openapi(),
        crate::mfa::ApiDoc::openapi(),
        crate::org::ApiDoc::openapi(),
        crate::roles::ApiDoc::openapi(),
        crate::approvals::ApiDoc::openapi(),
        crate::directory::ApiDoc::openapi(),
        crate::audit::ApiDoc::openapi(),
        crate::apikeys::ApiDoc::openapi(),
        crate::meetings_v1::ApiDoc::openapi(),
        crate::odoo::V1ApiDoc::openapi(),
        crate::apikeys::V1ApiDoc::openapi(),
        crate::storage::ApiDoc::openapi(),
        crate::account::ApiDoc::openapi(),
        crate::ai_studio::ApiDoc::openapi(),
        crate::net_probe::ApiDoc::openapi(),
        crate::ramais::ApiDoc::openapi(),
        crate::recording_meta::ApiDoc::openapi(),
        crate::recording_captions::ApiDoc::openapi(),
        crate::recording_chapters::ApiDoc::openapi(),
    ]
}

/// A superfície de um caminho (ADR-0004 §4).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Surface {
    Bff,
    V1,
    Operator,
    Integrations,
}

pub fn surface_of(path: &str) -> Surface {
    if path.starts_with("/api/v1/") {
        Surface::V1
    } else if path.starts_with("/api/operator/") {
        Surface::Operator
    } else if path.starts_with("/api/integrations/") {
        Surface::Integrations
    } else {
        Surface::Bff
    }
}

fn build(mut doc: utoipa::openapi::OpenApi, surface: Surface) -> utoipa::openapi::OpenApi {
    doc.info.version = env!("CARGO_PKG_VERSION").to_string();
    let mut all = utoipa::openapi::OpenApi::default();
    for part in parts() {
        all.merge(part);
    }
    for (path, item) in all.paths.paths {
        if surface_of(&path) == surface {
            doc.paths.paths.insert(path, item);
        }
    }
    if let (Some(to), Some(from)) = (doc.components.as_mut(), all.components) {
        for (k, v) in from.schemas {
            to.schemas.entry(k).or_insert(v);
        }
    }
    doc
}

pub fn bff() -> utoipa::openapi::OpenApi {
    build(BffDoc::openapi(), Surface::Bff)
}

pub fn v1() -> utoipa::openapi::OpenApi {
    build(V1Doc::openapi(), Surface::V1)
}

pub fn operator() -> utoipa::openapi::OpenApi {
    build(OperatorDoc::openapi(), Surface::Operator)
}

pub fn integrations() -> utoipa::openapi::OpenApi {
    build(IntegrationsDoc::openapi(), Surface::Integrations)
}

pub async fn bff_json() -> Json<utoipa::openapi::OpenApi> {
    Json(bff())
}

pub async fn v1_json() -> Json<utoipa::openapi::OpenApi> {
    Json(v1())
}

pub async fn operator_json() -> Json<utoipa::openapi::OpenApi> {
    Json(operator())
}

pub async fn integrations_json() -> Json<utoipa::openapi::OpenApi> {
    Json(integrations())
}

/// JSON estável (chaves ordenadas pelo serde_json/preserve_order desligado) para
/// o spec commitado.
pub fn to_pretty(doc: &utoipa::openapi::OpenApi) -> String {
    let value = serde_json::to_value(doc).expect("OpenAPI serializável");
    serde_json::to_string_pretty(&value).expect("JSON") + "\n"
}
