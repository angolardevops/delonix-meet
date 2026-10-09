//! Os aparelhos de um ramal móvel e o *wake* por push (ADR-0023, S-01).
//!
//! Um telemóvel com a app morta não tem registo SIP, e a chamada para ele morria em 10 ms. Aqui o
//! servidor sabe QUE aparelhos tem um ramal e COMO acordá-los:
//!
//! 1. **Registo** (sessão): a pessoa regista o aparelho do SEU ramal (`PUT …/my-extension/devices/{id}`),
//!    o administrador lista e revoga os de qualquer ramal da organização. O token de push vive cifrado
//!    em repouso e nunca volta numa resposta. Um aparelho está ligado à sessão que o registou: terminar a
//!    sessão (ADR-0011) desliga-o, sem tocar em `sessions.rs` — o *wake* só olha para aparelhos cuja
//!    sessão não foi revogada.
//! 2. **Wake** (máquina, segredo de voz): o `ramais_dial.lua` pergunta, quando o destino não está
//!    registado, se há aparelhos a acordar (`POST /internal/v1/voice/push/wake`). O servidor resolve o
//!    ramal DENTRO da organização do domínio, manda um push por aparelho e responde `awaiting` — é isso
//!    que decide se o FreeSWITCH segura a chamada ou falha já, como antes.
//!
//! **Fornecedores.** Só o `lab` está ligado: entrega o pedido a um URL do operador (`PUSH_LAB_URL`), por
//! trás da guarda de saída, e serve o laboratório e os testes. `fcm` e `apns_voip` aceitam-se no registo
//! mas **não enviam nada ainda**: não há contas Firebase nem Apple, e um fornecedor que finge enviar dá
//! luz verde sem acordar ninguém. Por isso contam como «não configurados» e não como `awaiting`.
//!
//! **Não provado:** nenhum push chegou a um telemóvel real; só o `lab`, contra um receptor de papel.

use axum::{
    extract::{Path, State},
    http::{header, HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use chrono::{DateTime, Utc};
use delonix_meet_core::DomainError;
use delonix_meet_domain::telephony::extension_device as rules;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;
use uuid::Uuid;

use crate::{auth::AuthUser, error::ApiError, AppState};

/// Quanto tempo o servidor dá aos fornecedores para responder. O `mod_curl` do Lua espera 6 s: o
/// *wake* tem de voltar antes disso, e um fornecedor lento não pode prender a chamada.
const WAKE_BUDGET: Duration = Duration::from_secs(4);

// ---------- Tipos ----------

/// Um aparelho de um ramal. Nunca leva o token.
#[derive(Debug, Serialize, sqlx::FromRow, utoipa::ToSchema)]
pub struct DeviceInfo {
    pub id: Uuid,
    #[schema(example = "android")]
    pub platform: String,
    #[schema(example = "fcm")]
    pub provider: String,
    pub app_version: String,
    pub created_at: DateTime<Utc>,
    pub last_seen_at: DateTime<Utc>,
}

/// A resposta a um aparelho NOVO. Com o fornecedor `delonix` e o serviço configurado, traz o que a app precisa
/// para se ligar ao delonix-push; **o segredo só aparece aqui, uma vez**.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct DeviceCreated {
    #[serde(flatten)]
    pub device: DeviceInfo,
    pub delonix_push: Option<DelonixPushGrant>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct DelonixPushGrant {
    /// URL base do serviço delonix-push.
    pub url: String,
    pub device_id: Uuid,
    /// Credencial do aparelho para `GET {url}/v1/connect`. Não volta a ser mostrada.
    pub device_secret: String,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct PutDeviceReq {
    /// `android` ou `ios`.
    #[schema(example = "android")]
    pub platform: String,
    /// `fcm` (Android), `apns_voip` (iOS), `delonix` (o serviço delonix-push, ambos) ou `lab` (só laboratório).
    #[schema(example = "fcm")]
    pub provider: String,
    /// O token que o fornecedor deu à app. Guarda-se cifrado e não volta em nenhuma resposta. Com o fornecedor
    /// `delonix` e o serviço configurado, o servidor cunha o aparelho no delonix-push e **ignora** este valor.
    pub push_token: String,
    /// Versão da app, só para diagnóstico.
    #[serde(default)]
    pub app_version: String,
}

// ---------- Registo (sessão) ----------

struct OwnExtension {
    id: Uuid,
    active: bool,
}

async fn own_extension(
    state: &AppState,
    org_id: Uuid,
    user_id: Uuid,
) -> Result<OwnExtension, ApiError> {
    crate::org::require_member_pub(state, org_id, user_id).await?;
    let (id, active): (Uuid, bool) = sqlx::query_as(
        "SELECT id, active FROM voice_extensions WHERE org_id = $1 AND member_id = $2",
    )
    .bind(org_id)
    .bind(user_id)
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(|| DomainError::not_found("ramais.no_extension"))?;
    Ok(OwnExtension { id, active })
}

fn validate(req: &PutDeviceReq) -> Result<(rules::Platform, rules::Provider), ApiError> {
    let platform = rules::Platform::parse(&req.platform).ok_or_else(|| {
        DomainError::invalid(
            "devices.platform_invalid",
            "a plataforma é «android» ou «ios»",
        )
    })?;
    let provider = rules::Provider::parse(&req.provider).ok_or_else(|| {
        DomainError::invalid(
            "devices.provider_invalid",
            "o fornecedor é «fcm», «apns_voip», «delonix» ou «lab»",
        )
    })?;
    if !provider.serves(platform) {
        return Err(DomainError::invalid(
            "devices.provider_platform_mismatch",
            "este fornecedor de push não serve esta plataforma",
        )
        .into());
    }
    if !rules::is_valid_token(&req.push_token) {
        return Err(DomainError::invalid(
            "devices.token_invalid",
            "o token de push tem de ser texto visível, sem espaços, até 4096 caracteres",
        )
        .into());
    }
    if !rules::is_valid_app_version(&req.app_version) {
        return Err(DomainError::invalid(
            "devices.app_version_invalid",
            "a versão da app é curta e sem caracteres de controlo",
        )
        .into());
    }
    Ok((platform, provider))
}

/// A pessoa regista (ou renova) o aparelho do SEU ramal. O `id` é da app: repetir o pedido com o mesmo
/// `id` renova o token em vez de criar outro aparelho. Um aparelho revogado não ressuscita: a app pede
/// com um `id` novo.
#[utoipa::path(
    put, path = "/api/orgs/{org_id}/my-extension/devices/{device_id}", tag = "voice",
    security(("session" = [])),
    params(
        ("org_id" = Uuid, Path, description = "Organização."),
        ("device_id" = Uuid, Path, description = "Identificador do aparelho, escolhido pela app (UUID).")
    ),
    request_body = PutDeviceReq,
    responses(
        (status = 201, body = DeviceCreated, description = "Aparelho criado (com `delonix_push` se o fornecedor é `delonix`)."),
        (status = 200, body = DeviceInfo, description = "Aparelho já existia: token renovado."),
        (status = 400, body = crate::openapi::ErrorBody, description = "`devices.platform_invalid`, `devices.provider_invalid`, `devices.provider_platform_mismatch`, `devices.token_invalid` ou `devices.app_version_invalid`."),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 404, body = crate::openapi::ErrorBody, description = "Não é membro activo, não tem ramal (`ramais.no_extension`), ou o `id` é de outro aparelho."),
        (status = 409, body = crate::openapi::ErrorBody, description = "`devices.revoked` (usar um `id` novo) ou `devices.too_many`."),
        (status = 422, body = crate::openapi::ErrorBody, description = "`devices.needs_session` (token anterior às sessões) ou `ramais.extension_inactive`."),
    )
)]
pub async fn put_my_device(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path((org_id, device_id)): Path<(Uuid, Uuid)>,
    Json(req): Json<PutDeviceReq>,
) -> Result<Response, ApiError> {
    let ext = own_extension(&state, org_id, auth.user_id).await?;
    if !ext.active {
        return Err(DomainError::precondition(
            "ramais.extension_inactive",
            "o ramal está inactivo",
        )
        .into());
    }
    let session_id = auth.session_id.ok_or_else(|| {
        DomainError::precondition(
            "devices.needs_session",
            "inicie sessão de novo: este token é anterior às sessões e não liga o aparelho a nenhuma",
        )
    })?;
    let (platform, provider) = validate(&req)?;
    // Fornecedor `delonix` com o serviço configurado: um aparelho NOVO é cunhado lá e o token passa a ser o
    // `device_id` dele. Uma renovação não volta a cunhar (o segredo já está com a app).
    let mut grant: Option<DelonixPushGrant> = None;
    let mut req = req;
    if provider == rules::Provider::Delonix && delonix_configured(&state) {
        let known: Option<String> =
            sqlx::query_scalar("SELECT provider FROM voice_devices WHERE id = $1")
                .bind(device_id)
                .fetch_optional(&state.db)
                .await?;
        if known.is_none() {
            let g = mint_delonix(&state, platform.as_str()).await?;
            req.push_token = g.device_id.to_string();
            grant = Some(g);
        }
    }
    let sealed = crate::secrets_at_rest::seal(
        &state.config,
        &req.push_token,
        &format!("voice_devices.push_token:{device_id}"),
    )?;
    let token_hash = crate::crypto::sha256_hex(&req.push_token);

    let mut tx = state.db.begin().await?;
    let existing: Option<(Uuid, Uuid, Option<DateTime<Utc>>)> = sqlx::query_as(
        "SELECT extension_id, user_id, revoked_at FROM voice_devices WHERE id = $1 FOR UPDATE",
    )
    .bind(device_id)
    .fetch_optional(&mut *tx)
    .await?;
    let created = match existing {
        // O `id` é de outro aparelho (de outra pessoa ou de outra organização): não se confirma.
        Some((ext_id, user_id, _)) if ext_id != ext.id || user_id != auth.user_id => {
            return Err(ApiError::NotFound)
        }
        Some((_, _, Some(_))) => {
            return Err(DomainError::conflict(
                "devices.revoked",
                "este aparelho foi revogado: registe-o com um identificador novo",
            )
            .into())
        }
        Some(_) => false,
        None => {
            let active: i64 = sqlx::query_scalar(
                "SELECT count(*) FROM voice_devices WHERE extension_id = $1 AND revoked_at IS NULL",
            )
            .bind(ext.id)
            .fetch_one(&mut *tx)
            .await?;
            if active >= rules::MAX_DEVICES_PER_EXTENSION {
                return Err(DomainError::conflict(
                    "devices.too_many",
                    "o ramal já tem o máximo de aparelhos: revogue um",
                )
                .into());
            }
            true
        }
    };
    // O mesmo token no mesmo ramal é o mesmo telemóvel: um registo novo revoga o anterior.
    sqlx::query(
        "UPDATE voice_devices SET revoked_at = now()
          WHERE extension_id = $1 AND provider = $2 AND push_token_hash = $3
            AND id <> $4 AND revoked_at IS NULL",
    )
    .bind(ext.id)
    .bind(provider.as_str())
    .bind(&token_hash)
    .bind(device_id)
    .execute(&mut *tx)
    .await?;
    let info: DeviceInfo = if created {
        sqlx::query_as(
            "INSERT INTO voice_devices
                 (id, org_id, extension_id, user_id, session_id, platform, provider,
                  push_token, push_token_hash, app_version)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
             RETURNING id, platform, provider, app_version, created_at, last_seen_at",
        )
        .bind(device_id)
        .bind(org_id)
        .bind(ext.id)
        .bind(auth.user_id)
        .bind(session_id)
        .bind(platform.as_str())
        .bind(provider.as_str())
        .bind(&sealed)
        .bind(&token_hash)
        .bind(&req.app_version)
        .fetch_one(&mut *tx)
        .await?
    } else {
        sqlx::query_as(
            "UPDATE voice_devices
                SET platform = $2, provider = $3,
                    push_token = CASE WHEN $3 = 'delonix' AND provider = 'delonix' AND $8 THEN push_token ELSE $4 END,
                    push_token_hash = CASE WHEN $3 = 'delonix' AND provider = 'delonix' AND $8 THEN push_token_hash ELSE $5 END,
                    app_version = $6, session_id = $7, last_seen_at = now()
              WHERE id = $1
              RETURNING id, platform, provider, app_version, created_at, last_seen_at",
        )
        .bind(device_id)
        .bind(platform.as_str())
        .bind(provider.as_str())
        .bind(&sealed)
        .bind(&token_hash)
        .bind(&req.app_version)
        .bind(session_id)
        .bind(delonix_configured(&state))
        .fetch_one(&mut *tx)
        .await?
    };
    tx.commit().await?;
    if created {
        crate::audit::log(
            &state.db,
            Some(org_id),
            auth.user_id,
            "ramal.aparelho_registado",
            platform.as_str(),
        )
        .await;
        let mut resp = (
            StatusCode::CREATED,
            Json(DeviceCreated {
                device: info,
                delonix_push: grant,
            }),
        )
            .into_response();
        resp.headers_mut().insert(
            header::LOCATION,
            HeaderValue::from_str(&format!(
                "/api/orgs/{org_id}/my-extension/devices/{device_id}"
            ))
            .map_err(DomainError::internal)?,
        );
        Ok(resp)
    } else {
        Ok(Json(info).into_response())
    }
}

fn delonix_configured(state: &AppState) -> bool {
    state.config.push_delonix_url.is_some() && state.config.push_delonix_key.is_some()
}

/// Cria o aparelho no delonix-push (`POST /v1/devices`) com a chave do projecto.
async fn mint_delonix(state: &AppState, platform: &str) -> Result<DelonixPushGrant, ApiError> {
    #[derive(Deserialize)]
    struct Minted {
        device_id: Uuid,
        device_secret: String,
    }
    let unavailable = || {
        DomainError::precondition(
            "devices.push_unavailable",
            "o serviço de push não respondeu: tente de novo daqui a pouco",
        )
    };
    let (Some(base), Some(key)) = (
        state.config.push_delonix_url.as_deref(),
        state.config.push_delonix_key.as_deref(),
    ) else {
        return Err(unavailable().into());
    };
    let base = base.trim_end_matches('/');
    let url = state
        .outbound
        .check_operator_url(&format!("{base}/v1/devices"))
        .await
        .map_err(|_| unavailable())?;
    let r = state
        .outbound
        .operator()
        .post(url)
        .bearer_auth(key)
        .json(&serde_json::json!({ "platform": platform }))
        .timeout(Duration::from_secs(5))
        .send()
        .await
        .map_err(|_| unavailable())?;
    if !r.status().is_success() {
        tracing::warn!(status = %r.status(), "delonix-push recusou cunhar o aparelho");
        return Err(unavailable().into());
    }
    let m: Minted = r.json().await.map_err(|_| unavailable())?;
    Ok(DelonixPushGrant {
        url: base.to_string(),
        device_id: m.device_id,
        device_secret: m.device_secret,
    })
}

/// Revoga o aparelho no delonix-push (melhor esforço: o Meet já deixou de o acordar).
async fn revoke_delonix(state: &AppState, push_device: Uuid) {
    let (Some(base), Some(key)) = (
        state.config.push_delonix_url.as_deref(),
        state.config.push_delonix_key.as_deref(),
    ) else {
        return;
    };
    let Ok(url) = state
        .outbound
        .check_operator_url(&format!(
            "{}/v1/devices/{push_device}",
            base.trim_end_matches('/')
        ))
        .await
    else {
        return;
    };
    let _ = state
        .outbound
        .operator()
        .delete(url)
        .bearer_auth(key)
        .timeout(Duration::from_secs(5))
        .send()
        .await;
}

/// Os aparelhos activos do SEU ramal (no máximo oito: não há paginação). Nunca devolve o token.
#[utoipa::path(
    get, path = "/api/orgs/{org_id}/my-extension/devices", tag = "voice",
    security(("session" = [])),
    params(("org_id" = Uuid, Path, description = "Organização.")),
    responses(
        (status = 200, body = Vec<DeviceInfo>),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 404, body = crate::openapi::ErrorBody, description = "Não é membro activo, ou não tem ramal (`ramais.no_extension`)."),
    )
)]
pub async fn list_my_devices(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(org_id): Path<Uuid>,
) -> Result<Json<Vec<DeviceInfo>>, ApiError> {
    let ext = own_extension(&state, org_id, auth.user_id).await?;
    active_devices(&state, org_id, ext.id).await.map(Json)
}

/// A pessoa desliga um aparelho do SEU ramal.
#[utoipa::path(
    delete, path = "/api/orgs/{org_id}/my-extension/devices/{device_id}", tag = "voice",
    security(("session" = [])),
    params(
        ("org_id" = Uuid, Path, description = "Organização."),
        ("device_id" = Uuid, Path, description = "Aparelho.")
    ),
    responses(
        (status = 204, description = "Aparelho desligado: deixa de ser acordado."),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 404, body = crate::openapi::ErrorBody, description = "Não é membro activo, não tem ramal, ou o aparelho não é seu (ou já está revogado)."),
    )
)]
pub async fn delete_my_device(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path((org_id, device_id)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, ApiError> {
    let ext = own_extension(&state, org_id, auth.user_id).await?;
    revoke(&state, org_id, ext.id, device_id, auth.user_id).await
}

// ---------- Gestão (administrador) ----------

/// O administrador lista os aparelhos activos de um ramal da organização.
#[utoipa::path(
    get, path = "/api/orgs/{org_id}/extensions/{id}/devices", tag = "voice",
    security(("session" = [])),
    params(
        ("org_id" = Uuid, Path, description = "Organização."),
        ("id" = Uuid, Path, description = "Ramal.")
    ),
    responses(
        (status = 200, body = Vec<DeviceInfo>),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 403, body = crate::openapi::ErrorBody),
        (status = 404, body = crate::openapi::ErrorBody, description = "O ramal não existe NESTA organização."),
    )
)]
pub async fn list_extension_devices(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path((org_id, id)): Path<(Uuid, Uuid)>,
) -> Result<Json<Vec<DeviceInfo>>, ApiError> {
    crate::org::require_admin_pub(&state, org_id, auth.user_id).await?;
    extension_in_org(&state, org_id, id).await?;
    active_devices(&state, org_id, id).await.map(Json)
}

/// O administrador desliga um aparelho de um ramal da organização («terminar o telemóvel»).
#[utoipa::path(
    delete, path = "/api/orgs/{org_id}/extensions/{id}/devices/{device_id}", tag = "voice",
    security(("session" = [])),
    params(
        ("org_id" = Uuid, Path, description = "Organização."),
        ("id" = Uuid, Path, description = "Ramal."),
        ("device_id" = Uuid, Path, description = "Aparelho.")
    ),
    responses(
        (status = 204, description = "Aparelho desligado."),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 403, body = crate::openapi::ErrorBody),
        (status = 404, body = crate::openapi::ErrorBody, description = "O ramal ou o aparelho não existem NESTA organização."),
    )
)]
pub async fn delete_extension_device(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path((org_id, id, device_id)): Path<(Uuid, Uuid, Uuid)>,
) -> Result<StatusCode, ApiError> {
    crate::org::require_admin_pub(&state, org_id, auth.user_id).await?;
    extension_in_org(&state, org_id, id).await?;
    revoke(&state, org_id, id, device_id, auth.user_id).await
}

async fn extension_in_org(state: &AppState, org_id: Uuid, id: Uuid) -> Result<(), ApiError> {
    let found: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM voice_extensions WHERE id = $1 AND org_id = $2)",
    )
    .bind(id)
    .bind(org_id)
    .fetch_one(&state.db)
    .await?;
    if found {
        Ok(())
    } else {
        Err(ApiError::NotFound)
    }
}

async fn active_devices(
    state: &AppState,
    org_id: Uuid,
    extension_id: Uuid,
) -> Result<Vec<DeviceInfo>, ApiError> {
    Ok(sqlx::query_as(
        "SELECT id, platform, provider, app_version, created_at, last_seen_at
           FROM voice_devices
          WHERE org_id = $1 AND extension_id = $2 AND revoked_at IS NULL
          ORDER BY created_at, id",
    )
    .bind(org_id)
    .bind(extension_id)
    .fetch_all(&state.db)
    .await?)
}

async fn revoke(
    state: &AppState,
    org_id: Uuid,
    extension_id: Uuid,
    device_id: Uuid,
    actor: Uuid,
) -> Result<StatusCode, ApiError> {
    let sealed: Option<(String, String)> = sqlx::query_as(
        "SELECT provider, push_token FROM voice_devices
          WHERE id = $1 AND org_id = $2 AND extension_id = $3 AND revoked_at IS NULL",
    )
    .bind(device_id)
    .bind(org_id)
    .bind(extension_id)
    .fetch_optional(&state.db)
    .await?;
    let n = sqlx::query(
        "UPDATE voice_devices SET revoked_at = now()
          WHERE id = $1 AND org_id = $2 AND extension_id = $3 AND revoked_at IS NULL",
    )
    .bind(device_id)
    .bind(org_id)
    .bind(extension_id)
    .execute(&state.db)
    .await?
    .rows_affected();
    if n == 0 {
        return Err(ApiError::NotFound);
    }
    if let Some((provider, token)) = sealed {
        if provider == "delonix" {
            if let Ok(open) = crate::secrets_at_rest::open(
                &state.config,
                &token,
                &format!("voice_devices.push_token:{device_id}"),
            ) {
                if let Ok(push_id) = open.parse::<Uuid>() {
                    revoke_delonix(state, push_id).await;
                }
            }
        }
    }
    crate::audit::log(
        &state.db,
        Some(org_id),
        actor,
        "ramal.aparelho_revogado",
        &device_id.to_string(),
    )
    .await;
    Ok(StatusCode::NO_CONTENT)
}

// ---------- Wake (máquina) ----------

#[derive(Debug, Deserialize)]
pub struct WakeReq {
    /// O domínio SIP do destino (o realm): é dele que sai a organização.
    domain: String,
    /// O utilizador SIP do destino.
    sip_username: String,
    /// O UUID do canal do FreeSWITCH: dá idempotência ao pedido.
    call_uuid: String,
    /// O utilizador SIP de quem liga, como o FreeSWITCH o autenticou (vazio se a chamada vem de fora). É
    /// metade da credencial dele: **nunca** vai para o telemóvel de outra pessoa. O servidor traduz-o para o
    /// número curto do chamador DESTA organização (ver [`caller_label`]).
    #[serde(default)]
    caller_sip_username: String,
}

#[derive(Debug, Serialize)]
pub struct WakeResp {
    /// O FreeSWITCH segura a chamada só se for `true`.
    pub(crate) awaiting: bool,
    /// Quantos aparelhos foram acordados (ou já o tinham sido por este `call_uuid`).
    pub(crate) devices: i64,
}

impl WakeResp {
    const NOBODY: Self = Self {
        awaiting: false,
        devices: 0,
    };
}

#[derive(sqlx::FromRow)]
struct WakeDevice {
    id: Uuid,
    platform: String,
    provider: String,
    push_token: String,
}

/// O que se manda a um aparelho: só o que a app precisa para o ecrã de chamada e para casar o INVITE.
/// Nunca credenciais, nunca o token de outro aparelho.
#[derive(Debug, Serialize)]
struct WakePush<'a> {
    kind: &'static str,
    device_id: Uuid,
    platform: &'a str,
    call_uuid: &'a str,
    caller: &'a str,
}

enum Outcome {
    Sent,
    NotConfigured,
    Failed,
}

/// O FreeSWITCH pergunta se há aparelhos a acordar para um ramal sem registo (`ramais_dial.lua`, S-02).
/// Rota de máquina, no listener interno, autenticada pelo segredo de voz. **Nunca diz porquê** não há
/// ninguém a acordar (domínio desconhecido, ramal inexistente, sem aparelhos, limite): a resposta é a
/// mesma, e o chamador deixa de esperar.
pub async fn ivr_push_wake(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(req): Json<WakeReq>,
) -> Result<Json<WakeResp>, ApiError> {
    crate::voice::check_media_secret(&state, &headers)?;
    if req.call_uuid.is_empty()
        || req.call_uuid.len() > 64
        || !req
            .call_uuid
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-')
    {
        return Err(DomainError::invalid("push.call_uuid_invalid", "call_uuid inválido").into());
    }
    let Some(org_id) = crate::ramais::org_id_by_sip_domain(&state, &req.domain).await else {
        return Ok(Json(WakeResp::NOBODY));
    };
    // O ramal procura-se DENTRO da organização do domínio: nunca atravessa organizações.
    let Some((extension_id, number, active)): Option<(Uuid, String, bool)> = sqlx::query_as(
        "SELECT id, extension, active FROM voice_extensions WHERE org_id = $1 AND sip_username = $2",
    )
    .bind(org_id)
    .bind(&req.sip_username)
    .fetch_optional(&state.db)
    .await?
    else {
        return Ok(Json(WakeResp::NOBODY));
    };
    if !active {
        return Ok(Json(WakeResp::NOBODY));
    }

    let caller = caller_label(&state, org_id, &req.caller_sip_username).await?;
    acordar_ramal(
        &state,
        org_id,
        extension_id,
        &number,
        &req.call_uuid,
        &caller,
    )
    .await
    .map(Json)
}

/// O ramal tem registo no FreeSWITCH? `None` se não se consegue saber (sem Event Socket, ou um erro): quem
/// chama trata isso como «não sei» e segue como se esta funcionalidade não existisse.
pub(crate) async fn ramal_registado(
    state: &AppState,
    sip_username: &str,
    domain: &str,
) -> Option<bool> {
    let sip = state.telephony.sip.as_ref()?;
    sip.extension_registered(sip_username, domain).await.ok()
}

/// O núcleo do *wake*, partilhado pelo FreeSWITCH (`ivr_push_wake`) e pelas chamadas que o próprio servidor
/// origina (ligar a partir da sala): limite por ramal e por minuto, aparelhos activos de sessões vivas, um push
/// por (chamada, aparelho). `caller` já vem traduzido para o número curto (ver [`caller_label`]).
pub(crate) async fn acordar_ramal(
    state: &Arc<AppState>,
    org_id: Uuid,
    extension_id: Uuid,
    number: &str,
    call_uuid: &str,
    caller: &str,
) -> Result<WakeResp, ApiError> {
    // O limite por ramal e por minuto, e a limpeza do que já não serve.
    sqlx::query(
        "DELETE FROM voice_push_wakes WHERE created_at < now() - make_interval(hours => $1::int)",
    )
    .bind(rules::WAKE_RETENTION_HOURS as i32)
    .execute(&state.db)
    .await?;
    let recent: i64 = sqlx::query_scalar(
        "SELECT count(DISTINCT call_uuid) FROM voice_push_wakes
          WHERE extension_id = $1 AND created_at > now() - interval '1 minute'
            AND call_uuid <> $2",
    )
    .bind(extension_id)
    .bind(call_uuid)
    .fetch_one(&state.db)
    .await?;
    if recent >= rules::MAX_WAKES_PER_MINUTE {
        tracing::warn!(%extension_id, "wake recusado: limite por minuto do ramal");
        return Ok(WakeResp::NOBODY);
    }

    // Só aparelhos activos CUJA SESSÃO continua viva: terminar a sessão desliga o telemóvel.
    let devices: Vec<WakeDevice> = sqlx::query_as(
        "SELECT d.id, d.platform, d.provider, d.push_token
           FROM voice_devices d
           JOIN user_sessions s ON s.id = d.session_id AND s.revoked_at IS NULL
          WHERE d.extension_id = $1 AND d.org_id = $2 AND d.revoked_at IS NULL
          ORDER BY d.created_at, d.id",
    )
    .bind(extension_id)
    .bind(org_id)
    .fetch_all(&state.db)
    .await?;

    let mut tasks = Vec::new();
    let mut already = 0i64;
    for d in devices {
        // Um pedido por (chamada, aparelho): o mesmo `call_uuid` não acorda o telemóvel duas vezes.
        let fresh = sqlx::query(
            "INSERT INTO voice_push_wakes (call_uuid, device_id, extension_id)
             VALUES ($1, $2, $3) ON CONFLICT DO NOTHING",
        )
        .bind(call_uuid)
        .bind(d.id)
        .bind(extension_id)
        .execute(&state.db)
        .await?
        .rows_affected()
            > 0;
        if !fresh {
            already += 1;
            continue;
        }
        let state = state.clone();
        let call_uuid = call_uuid.to_string();
        let caller = caller.to_string();
        tasks.push(async move { send(&state, &d, &call_uuid, &caller).await });
    }
    let outcomes = tokio::time::timeout(WAKE_BUDGET, futures_util::future::join_all(tasks))
        .await
        .unwrap_or_else(|_| {
            tracing::warn!(%extension_id, "wake: um fornecedor não respondeu a tempo");
            Vec::new()
        });
    let sent = outcomes
        .iter()
        .filter(|o| matches!(o, Outcome::Sent))
        .count() as i64;
    let not_configured = outcomes
        .iter()
        .filter(|o| matches!(o, Outcome::NotConfigured))
        .count();
    let failed = outcomes
        .iter()
        .filter(|o| matches!(o, Outcome::Failed))
        .count();
    let devices = sent + already;
    if not_configured > 0 || failed > 0 {
        tracing::info!(%extension_id, sent, not_configured, failed, "wake com aparelhos por acordar");
    }
    if sent > 0 {
        // O alvo é o número do ramal: nunca o token, nem o do aparelho.
        crate::audit::log(
            &state.db,
            Some(org_id),
            Uuid::nil(),
            "ramal.acordado_por_push",
            number,
        )
        .await;
    }
    Ok(WakeResp {
        awaiting: devices > 0,
        devices,
    })
}

/// O que o telemóvel mostra de quem liga: o NÚMERO CURTO do ramal chamador, procurado na organização do
/// destino. Um chamador que não é ramal desta organização (tronco, outra org, desconhecido) vai como vazio:
/// o push nunca leva o utilizador SIP de ninguém.
async fn caller_label(
    state: &AppState,
    org_id: Uuid,
    caller_sip_username: &str,
) -> Result<String, ApiError> {
    if caller_sip_username.is_empty() {
        return Ok(String::new());
    }
    let number: Option<String> = sqlx::query_scalar(
        "SELECT extension FROM voice_extensions WHERE org_id = $1 AND sip_username = $2",
    )
    .bind(org_id)
    .bind(caller_sip_username)
    .fetch_optional(&state.db)
    .await?;
    Ok(number.map(|n| rules::clean_caller(&n)).unwrap_or_default())
}

async fn send(state: &AppState, d: &WakeDevice, call_uuid: &str, caller: &str) -> Outcome {
    let Some(provider) = rules::Provider::parse(&d.provider) else {
        return Outcome::Failed;
    };
    match provider {
        rules::Provider::Lab => send_lab(state, d, call_uuid, caller).await,
        rules::Provider::Delonix => send_delonix(state, d, call_uuid, caller).await,
        // FCM e APNs ainda não existem (sem conta Firebase nem Apple): não fingem que enviaram.
        rules::Provider::Fcm | rules::Provider::ApnsVoip => Outcome::NotConfigured,
    }
}

/// O serviço `delonix-push`: `POST {PUSH_DELONIX_URL}/v1/messages` com a chave do projecto. O token do aparelho
/// é o `device_id` que o serviço devolveu. A mensagem leva só o que a app precisa (`call_uuid` e o número curto
/// de quem liga, nunca o utilizador SIP) e expira depressa: uma chamada que tocou há um minuto já não interessa.
async fn send_delonix(state: &AppState, d: &WakeDevice, call_uuid: &str, caller: &str) -> Outcome {
    let (Some(base), Some(key)) = (
        state.config.push_delonix_url.as_deref(),
        state.config.push_delonix_key.as_deref(),
    ) else {
        return Outcome::NotConfigured;
    };
    let Ok(token) = crate::secrets_at_rest::open(
        &state.config,
        &d.push_token,
        &format!("voice_devices.push_token:{}", d.id),
    ) else {
        return Outcome::Failed;
    };
    let Ok(target) = token.parse::<Uuid>() else {
        return Outcome::Failed;
    };
    let url = match state
        .outbound
        .check_operator_url(&format!("{}/v1/messages", base.trim_end_matches('/')))
        .await
    {
        Ok(u) => u,
        Err(_) => {
            tracing::warn!("PUSH_DELONIX_URL recusado pela guarda de saída");
            return Outcome::Failed;
        }
    };
    let body = serde_json::json!({
        "device_id": target,
        "priority": "high",
        "ttl_secs": 60,
        "collapse_key": format!("call:{call_uuid}"),
        "idempotency_key": format!("wake:{call_uuid}"),
        "payload": { "kind": "incoming_call", "call_uuid": call_uuid, "caller": caller },
    });
    match state
        .outbound
        .operator()
        .post(url)
        .bearer_auth(key)
        .json(&body)
        .send()
        .await
    {
        Ok(r) if r.status().is_success() => Outcome::Sent,
        Ok(r) => {
            tracing::warn!(status = %r.status(), "delonix-push recusou o wake");
            Outcome::Failed
        }
        Err(_) => Outcome::Failed,
    }
}

/// O fornecedor de laboratório: entrega o pedido ao URL do operador. O token abre-se só para provar
/// que é legível com a chave actual; **não vai no pedido** (o receptor identifica o aparelho pelo `id`).
async fn send_lab(state: &AppState, d: &WakeDevice, call_uuid: &str, caller: &str) -> Outcome {
    let Some(url) = state.config.push_lab_url.as_deref() else {
        return Outcome::NotConfigured;
    };
    if crate::secrets_at_rest::open(
        &state.config,
        &d.push_token,
        &format!("voice_devices.push_token:{}", d.id),
    )
    .is_err()
    {
        return Outcome::Failed;
    }
    let url = match state.outbound.check_operator_url(url).await {
        Ok(u) => u,
        Err(_) => {
            tracing::warn!("PUSH_LAB_URL recusado pela guarda de saída");
            return Outcome::Failed;
        }
    };
    let body = WakePush {
        kind: "incoming_call",
        device_id: d.id,
        platform: &d.platform,
        call_uuid,
        caller,
    };
    match state.outbound.operator().post(url).json(&body).send().await {
        Ok(r) if r.status().is_success() => Outcome::Sent,
        Ok(r) => {
            tracing::warn!(status = %r.status(), "fornecedor lab recusou o wake");
            Outcome::Failed
        }
        Err(_) => Outcome::Failed,
    }
}
