//! Registo SIP da organização (ADR-0009 §5).
//!
//! - `GET  /api/orgs/{org_id}/telephony/sip-settings`                      domínio, SBC, transporte, codecs (sem password)
//! - `PUT  /api/orgs/{org_id}/telephony/sip-settings`                      substitui (password write-only)
//! - `POST /api/orgs/{org_id}/telephony/sip-settings/reveal-credentials`   «Ver credenciais»: reautenticação + auditoria
//! - `GET  /api/orgs/{org_id}/telephony/sip-registration`                  estado MEDIDO (SBC, media, troncos, qualidade)
//! - `POST /api/orgs/{org_id}/telephony/sip-registration/restart`          «Reiniciar registo» (`202`)

use std::sync::Arc;

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use chrono::{DateTime, Utc};
use delonix_meet_core::DomainError;
use delonix_meet_domain::telephony::{
    ports::{gateway_name, MediaServerStatus, SbcStatus},
    trunk as rules,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{auth::AuthUser, error::ApiError, telephony_service::port_error, AppState};

/// Codecs que a plataforma sabe negociar. Lista fechada: um nome livre
/// acabava num `codec-prefs` que o FreeSWITCH ignora sem dizer nada.
pub const KNOWN_CODECS: [&str; 8] = [
    "OPUS", "G722", "PCMA", "PCMU", "G729", "GSM", "iLBC", "SPEEX",
];

fn aad(org_id: &Uuid) -> String {
    format!("telephony_sip_settings.password:{org_id}")
}

#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
pub struct SipSettings {
    /// `null` antes de configurado.
    pub configured: bool,
    pub domain: Option<String>,
    pub sbc_host: Option<String>,
    pub transport: Option<String>,
    pub srtp: Option<String>,
    pub codecs: Vec<String>,
    pub username: Option<String>,
    pub password_configured: bool,
    pub updated_at: Option<DateTime<Utc>>,
}

#[derive(Deserialize, utoipa::ToSchema)]
pub struct PutSipSettingsReq {
    /// `sip.delonix.co.ao`.
    pub domain: String,
    #[serde(default)]
    pub sbc_host: String,
    /// `tls` | `tcp` | `udp`.
    pub transport: String,
    /// `mandatory` | `optional` | `off` (diferente de `off` exige `tls`).
    pub srtp: String,
    /// Ordem de preferência, dos conhecidos: OPUS, G722, PCMA, PCMU, G729, GSM, iLBC, SPEEX.
    #[serde(default)]
    pub codecs: Vec<String>,
    #[serde(default)]
    pub username: String,
    /// Write-only. Ausente mantém; `""` apaga.
    #[serde(default)]
    pub password: Option<String>,
}

#[derive(Deserialize, utoipa::ToSchema)]
pub struct RevealReq {
    /// Password da conta de quem pede.
    #[serde(default)]
    pub password: Option<String>,
    /// Ou um código MFA (TOTP ou de recuperação), para contas sem password
    /// local (SSO).
    #[serde(default)]
    pub mfa_code: Option<String>,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct RevealedCredentials {
    pub domain: String,
    pub username: String,
    pub password: String,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct ComponentStatus {
    pub software: String,
    pub version: Option<String>,
    pub uptime_secs: Option<u64>,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct TrunkCounts {
    pub total: usize,
    pub up: usize,
    pub degraded: usize,
    pub down: usize,
    pub unknown: usize,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct Channels {
    /// Soma dos canais em uso MEDIDOS; `null` se nenhum tronco foi medido.
    pub in_use: Option<u64>,
    /// Soma dos máximos declarados dos troncos activos.
    pub max: i64,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct Quality {
    /// Média, das chamadas com medida na janela; `null` sem chamadas (`reason`).
    pub jitter_ms: Option<f64>,
    pub loss_pct: Option<f64>,
    pub mos: Option<f64>,
    pub calls: i64,
    pub window_hours: i64,
    /// `no_calls_in_window`.
    pub reason: Option<String>,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct SipRegistration {
    /// `healthy` | `degraded` | `down` | `not_configured`.
    pub state: String,
    /// `sip_not_configured`, `media_server_unreachable`, `sbc_unreachable`,
    /// `sbc_not_configured`, `trunk_down`, `trunk_degraded`, `settings_missing`.
    pub reasons: Vec<String>,
    /// Configurado pela org (não medido).
    pub domain: Option<String>,
    pub sbc_host: Option<String>,
    pub transport: Option<String>,
    pub srtp: Option<String>,
    pub codecs_configured: Vec<String>,
    /// Medido no SBC (Kamailio); `null` sem medida (`sbc_error`).
    pub sbc: Option<ComponentStatus>,
    pub sbc_error: Option<String>,
    /// Medido no media server (FreeSWITCH); `null` sem medida (`media_error`).
    pub media: Option<ComponentStatus>,
    pub media_error: Option<String>,
    /// Codecs de saída que o FreeSWITCH diz oferecer.
    pub codecs_offered: Vec<String>,
    pub sessions_active: Option<u32>,
    pub channels: Channels,
    pub trunks: TrunkCounts,
    pub quality: Quality,
    pub measured_at: DateTime<Utc>,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct RestartResp {
    /// Gateways a que se pediu novo registo.
    pub gateways: usize,
}

#[derive(utoipa::OpenApi)]
#[openapi(
    paths(
        get_settings,
        put_settings,
        reveal_credentials,
        registration,
        restart_registration
    ),
    components(schemas(
        SipSettings,
        PutSipSettingsReq,
        RevealReq,
        RevealedCredentials,
        SipRegistration,
        ComponentStatus,
        TrunkCounts,
        Channels,
        Quality,
        RestartResp
    ))
)]
pub struct ApiDoc;

#[derive(sqlx::FromRow)]
struct Row {
    domain: String,
    sbc_host: String,
    transport: String,
    srtp: String,
    codecs: Vec<String>,
    username: String,
    password_sealed: String,
    updated_at: DateTime<Utc>,
}

async fn read_row(state: &AppState, org_id: Uuid) -> Result<Option<Row>, ApiError> {
    Ok(sqlx::query_as(
        "SELECT domain, sbc_host, transport, srtp, codecs, username, password_sealed, updated_at
           FROM telephony_sip_settings WHERE org_id = $1",
    )
    .bind(org_id)
    .fetch_optional(&state.db)
    .await?)
}

fn to_dto(r: Option<Row>) -> SipSettings {
    match r {
        None => SipSettings {
            configured: false,
            domain: None,
            sbc_host: None,
            transport: None,
            srtp: None,
            codecs: Vec::new(),
            username: None,
            password_configured: false,
            updated_at: None,
        },
        Some(r) => SipSettings {
            configured: true,
            domain: Some(r.domain),
            sbc_host: Some(r.sbc_host).filter(|s| !s.is_empty()),
            transport: Some(r.transport),
            srtp: Some(r.srtp),
            codecs: r.codecs,
            username: Some(r.username).filter(|s| !s.is_empty()),
            password_configured: !r.password_sealed.is_empty(),
            updated_at: Some(r.updated_at),
        },
    }
}

/// Configuração SIP da org (sem a password).
#[utoipa::path(
    get, path = "/api/orgs/{org_id}/telephony/sip-settings", tag = "telephony",
    security(("session" = [])),
    params(("org_id" = Uuid, Path)),
    responses(
        (status = 200, body = SipSettings),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 403, body = crate::openapi::ErrorBody),
        (status = 404, body = crate::openapi::ErrorBody),
    )
)]
pub async fn get_settings(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(org_id): Path<Uuid>,
) -> Result<Json<SipSettings>, ApiError> {
    crate::org::require_admin_pub(&state, org_id, auth.user_id).await?;
    Ok(Json(to_dto(read_row(&state, org_id).await?)))
}

/// Substitui a configuração SIP (singleton).
#[utoipa::path(
    put, path = "/api/orgs/{org_id}/telephony/sip-settings", tag = "telephony",
    security(("session" = [])),
    params(("org_id" = Uuid, Path)),
    request_body = PutSipSettingsReq,
    responses(
        (status = 200, body = SipSettings),
        (status = 400, body = crate::openapi::ErrorBody, description = "`telephony.invalid_domain`, `telephony.invalid_codec`, `telephony.srtp_requires_tls`, …"),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 403, body = crate::openapi::ErrorBody),
        (status = 404, body = crate::openapi::ErrorBody),
        (status = 409, body = crate::openapi::ErrorBody, description = "`telephony.sip_domain_taken`: o domínio decide a org das chamadas que entram, e é único."),
        (status = 422, body = crate::openapi::ErrorBody, description = "`secrets.encryption_unconfigured`"),
    )
)]
pub async fn put_settings(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(org_id): Path<Uuid>,
    Json(req): Json<PutSipSettingsReq>,
) -> Result<Json<SipSettings>, ApiError> {
    crate::org::require_admin_pub(&state, org_id, auth.user_id).await?;
    let domain = rules::validate_host(&req.domain).map_err(|_| {
        DomainError::invalid(
            "telephony.invalid_domain",
            "domínio SIP inválido (sip.empresa.ao)",
        )
        .with_field("domain", "nome de host")
    })?;
    let sbc_host = if req.sbc_host.trim().is_empty() {
        String::new()
    } else {
        rules::validate_host(&req.sbc_host)?
    };
    let transport = rules::validate_transport(&req.transport)?;
    let srtp = rules::validate_srtp(&req.srtp)?;
    rules::validate_security(transport, srtp)?;
    let mut codecs: Vec<String> = Vec::new();
    for c in &req.codecs {
        let known = KNOWN_CODECS
            .iter()
            .find(|k| k.eq_ignore_ascii_case(c.trim()))
            .ok_or_else(|| {
                DomainError::invalid(
                    "telephony.invalid_codec",
                    format!(
                        "codec «{c}» desconhecido — válidos: {}",
                        KNOWN_CODECS.join(", ")
                    ),
                )
                .with_field("codecs", KNOWN_CODECS.join(" | "))
            })?;
        if !codecs.iter().any(|x| x == known) {
            codecs.push(known.to_string());
        }
    }
    rules::validate_credentials(&req.username, req.password.as_deref())?;
    let sealed = match req.password.as_deref() {
        None => None,
        Some("") => Some(String::new()),
        Some(p) => Some(crate::secrets_at_rest::seal(
            &state.config,
            p,
            &aad(&org_id),
        )?),
    };
    let before = to_dto(read_row(&state, org_id).await?);
    sqlx::query(
        "INSERT INTO telephony_sip_settings
            (org_id, domain, sbc_host, transport, srtp, codecs, username, password_sealed, updated_by, updated_at)
         VALUES ($1,$2,$3,$4,$5,$6,$7,COALESCE($8, ''),$9, now())
         ON CONFLICT (org_id) DO UPDATE SET
            domain = $2, sbc_host = $3, transport = $4, srtp = $5, codecs = $6, username = $7,
            password_sealed = COALESCE($8, telephony_sip_settings.password_sealed),
            updated_by = $9, updated_at = now()",
    )
    .bind(org_id)
    .bind(&domain)
    .bind(&sbc_host)
    .bind(transport)
    .bind(srtp)
    .bind(&codecs)
    .bind(req.username.trim())
    .bind(&sealed)
    .bind(auth.user_id)
    .execute(&state.db)
    .await
    .map_err(|e| match e {
        sqlx::Error::Database(d) if d.is_unique_violation() => ApiError::from(DomainError::conflict(
            "telephony.sip_domain_taken",
            "esse domínio SIP já está atribuído a outra organização",
        )),
        other => other.into(),
    })?;
    let after = to_dto(read_row(&state, org_id).await?);
    crate::audit::log(
        &state.db,
        Some(org_id),
        auth.user_id,
        "telephony.sip_settings.updated",
        &format!(
            "domínio {:?}→{:?}; sbc {:?}→{:?}; transporte {:?}→{:?}; srtp {:?}→{:?}; codecs {:?}→{:?}; utilizador {:?}→{:?}{}",
            before.domain, after.domain, before.sbc_host, after.sbc_host,
            before.transport, after.transport, before.srtp, after.srtp,
            before.codecs, after.codecs, before.username, after.username,
            if req.password.is_some() { "; password alterada" } else { "" }
        ),
    )
    .await;
    Ok(Json(after))
}

/// «Ver credenciais»: devolve a password SIP depois de reautenticar quem pede
/// (password da conta, ou código MFA para contas SSO). Cada sucesso e cada
/// falha ficam na auditoria; falhas repetidas bloqueiam 5 minutos.
#[utoipa::path(
    post, path = "/api/orgs/{org_id}/telephony/sip-settings/reveal-credentials", tag = "telephony",
    security(("session" = [])),
    params(("org_id" = Uuid, Path)),
    request_body = RevealReq,
    responses(
        (status = 200, body = RevealedCredentials),
        (status = 401, body = crate::openapi::ErrorBody, description = "Sessão inválida."),
        (status = 403, body = crate::openapi::ErrorBody, description = "`telephony.reauth_failed` ou `telephony.reauth_unavailable` (conta sem password e sem MFA)."),
        (status = 404, body = crate::openapi::ErrorBody),
        (status = 422, body = crate::openapi::ErrorBody, description = "`telephony.credentials_not_configured`"),
        (status = 429, body = crate::openapi::ErrorBody, description = "`telephony.reauth_rate_limited`"),
    )
)]
pub async fn reveal_credentials(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(org_id): Path<Uuid>,
    Json(req): Json<RevealReq>,
) -> Result<Json<RevealedCredentials>, ApiError> {
    crate::org::require_admin_pub(&state, org_id, auth.user_id).await?;
    let key = auth.user_id.to_string();
    if state.telephony_reveal_limiter.is_blocked(&key) {
        return Err(DomainError::new(
            delonix_meet_core::ErrorKind::ResourceExhausted,
            "telephony.reauth_rate_limited",
            "demasiadas tentativas; volta a tentar daqui a alguns minutos",
        )
        .into());
    }
    let hash: Option<String> = sqlx::query_scalar("SELECT password_hash FROM users WHERE id = $1")
        .bind(auth.user_id)
        .fetch_optional(&state.db)
        .await?;
    let has_password = hash.as_deref().is_some_and(|h| !h.is_empty());
    let has_mfa = crate::mfa::factors(&state.db, auth.user_id)
        .await?
        .totp_enabled;
    let ok =
        match (req.password.as_deref(), req.mfa_code.as_deref()) {
            (Some(pw), _) if has_password => {
                crate::auth::verify_password(pw, hash.as_deref().unwrap_or(""))
            }
            (_, Some(code)) if has_mfa => {
                crate::mfa::consome_codigo(&state, auth.user_id, code).await?
            }
            _ if !has_password && !has_mfa => return Err(DomainError::new(
                delonix_meet_core::ErrorKind::PermissionDenied,
                "telephony.reauth_unavailable",
                "a tua conta não tem password local nem MFA — activa o MFA para ver credenciais",
            )
            .into()),
            _ => false,
        };
    if !ok {
        state.telephony_reveal_limiter.check(&key);
        crate::audit::log(
            &state.db,
            Some(org_id),
            auth.user_id,
            "telephony.sip_credentials.reveal_denied",
            "reautenticação falhada",
        )
        .await;
        return Err(DomainError::new(
            delonix_meet_core::ErrorKind::PermissionDenied,
            "telephony.reauth_failed",
            "a reautenticação falhou",
        )
        .into());
    }
    let row = read_row(&state, org_id)
        .await?
        .filter(|r| !r.password_sealed.is_empty())
        .ok_or_else(|| {
            ApiError::from(DomainError::precondition(
                "telephony.credentials_not_configured",
                "não há credenciais SIP guardadas nesta organização",
            ))
        })?;
    let password =
        crate::secrets_at_rest::open(&state.config, &row.password_sealed, &aad(&org_id))?;
    crate::audit::log(
        &state.db,
        Some(org_id),
        auth.user_id,
        "telephony.sip_credentials.revealed",
        &format!("utilizador SIP {}", row.username),
    )
    .await;
    Ok(Json(RevealedCredentials {
        domain: row.domain,
        username: row.username,
        password,
    }))
}

/// Estado do registo SIP, medido agora no SBC e no media server, com o resumo
/// dos troncos e a qualidade das últimas 24 h de CDRs.
#[utoipa::path(
    get, path = "/api/orgs/{org_id}/telephony/sip-registration", tag = "telephony",
    security(("session" = [])),
    params(("org_id" = Uuid, Path)),
    responses(
        (status = 200, body = SipRegistration),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 403, body = crate::openapi::ErrorBody),
        (status = 404, body = crate::openapi::ErrorBody),
    )
)]
pub async fn registration(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(org_id): Path<Uuid>,
) -> Result<Json<SipRegistration>, ApiError> {
    crate::org::require_admin_pub(&state, org_id, auth.user_id).await?;
    let settings = to_dto(read_row(&state, org_id).await?);
    let trunks = crate::telephony_trunks::all_with_status(&state, org_id, 24).await?;
    let ids: Vec<Uuid> = trunks.iter().map(|t| t.id).collect();
    // Uma segunda fotografia só para media/SBC; os gateways já vieram nos troncos.
    let (snap, _) = crate::telephony_trunks::snapshot_for(&state, &ids[..0]).await;
    let comp = |s: &SbcStatus| ComponentStatus {
        software: s.software.clone(),
        version: s.version.clone(),
        uptime_secs: s.uptime_secs,
    };
    let media_comp = |m: &MediaServerStatus| ComponentStatus {
        software: m.software.clone(),
        version: m.version.clone(),
        uptime_secs: m.uptime_secs,
    };
    let q: (Option<f64>, Option<f64>, Option<f64>, i64) = sqlx::query_as(
        "SELECT AVG(jitter_ms), AVG(loss_pct), AVG(mos), COUNT(*) FILTER (WHERE jitter_ms IS NOT NULL OR loss_pct IS NOT NULL OR mos IS NOT NULL)
           FROM telephony_call_records
          WHERE org_id = $1 AND started_at >= now() - interval '24 hours'",
    )
    .bind(org_id)
    .fetch_one(&state.db)
    .await?;

    let mut counts = TrunkCounts {
        total: trunks.len(),
        up: 0,
        degraded: 0,
        down: 0,
        unknown: 0,
    };
    let mut in_use: Option<u64> = None;
    let mut max = 0i64;
    for t in &trunks {
        match t.status.state.as_str() {
            "up" => counts.up += 1,
            "degraded" => counts.degraded += 1,
            "down" => counts.down += 1,
            _ => counts.unknown += 1,
        }
        if t.enabled {
            max += t.max_channels as i64;
        }
        if let Some(u) = t.status.channels_in_use {
            in_use = Some(in_use.unwrap_or(0) + u as u64);
        }
    }

    let mut reasons = Vec::new();
    let state_str = if state.telephony.sip.is_none() {
        reasons.push("sip_not_configured".to_string());
        "not_configured"
    } else {
        let media_ok = snap.as_ref().is_some_and(|s| s.media.is_some());
        let sbc_configured = state.config.telephony_kamailio_rpc_url.is_some();
        let sbc_ok = snap.as_ref().is_some_and(|s| s.sbc.is_some());
        if !settings.configured {
            reasons.push("settings_missing".into());
        }
        if !media_ok {
            reasons.push("media_server_unreachable".into());
        }
        if !sbc_configured {
            reasons.push("sbc_not_configured".into());
        } else if !sbc_ok {
            reasons.push("sbc_unreachable".into());
        }
        if counts.down > 0 {
            reasons.push("trunk_down".into());
        }
        if counts.degraded > 0 {
            reasons.push("trunk_degraded".into());
        }
        if !media_ok {
            "down"
        } else if (sbc_configured && !sbc_ok) || counts.down > 0 || counts.degraded > 0 {
            "degraded"
        } else {
            "healthy"
        }
    };
    Ok(Json(SipRegistration {
        state: state_str.into(),
        reasons,
        domain: settings.domain,
        sbc_host: settings.sbc_host,
        transport: settings.transport,
        srtp: settings.srtp,
        codecs_configured: settings.codecs,
        sbc: snap.as_ref().and_then(|s| s.sbc.as_ref()).map(comp),
        sbc_error: snap.as_ref().and_then(|s| s.sbc_error.clone()),
        media: snap.as_ref().and_then(|s| s.media.as_ref()).map(media_comp),
        media_error: snap
            .as_ref()
            .and_then(|s| s.media_error.clone())
            .or_else(|| {
                state
                    .telephony
                    .sip
                    .is_none()
                    .then(|| "not_configured".into())
            }),
        codecs_offered: snap
            .as_ref()
            .and_then(|s| s.media.as_ref())
            .map(|m| m.codecs.clone())
            .unwrap_or_default(),
        sessions_active: snap
            .as_ref()
            .and_then(|s| s.media.as_ref())
            .and_then(|m| m.sessions_active),
        channels: Channels { in_use, max },
        trunks: counts,
        quality: Quality {
            jitter_ms: q.0,
            loss_pct: q.1,
            mos: q.2,
            calls: q.3,
            window_hours: 24,
            reason: (q.3 == 0).then(|| "no_calls_in_window".into()),
        },
        measured_at: snap.map(|s| s.measured_at).unwrap_or_else(Utc::now),
    }))
}

/// «Reiniciar registo»: pede ao FreeSWITCH novo REGISTER de todos os troncos
/// da org. Não espera pelo resultado (lê-se em `sip-registration`).
#[utoipa::path(
    post, path = "/api/orgs/{org_id}/telephony/sip-registration/restart", tag = "telephony",
    security(("session" = [])),
    params(("org_id" = Uuid, Path)),
    responses(
        (status = 202, body = RestartResp),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 403, body = crate::openapi::ErrorBody),
        (status = 404, body = crate::openapi::ErrorBody),
        (status = 422, body = crate::openapi::ErrorBody, description = "`telephony.not_configured`"),
        (status = 503, body = crate::openapi::ErrorBody, description = "`telephony.media_server_unavailable`"),
    )
)]
pub async fn restart_registration(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(org_id): Path<Uuid>,
) -> Result<(StatusCode, Json<RestartResp>), ApiError> {
    crate::org::require_admin_pub(&state, org_id, auth.user_id).await?;
    let sip = state.telephony.sip.clone().ok_or_else(|| {
        port_error(
            delonix_meet_domain::telephony::ports::PortError::NotConfigured(
                "sem TELEPHONY_ESL_ADDR".into(),
            ),
        )
    })?;
    let names: Vec<String> = crate::telephony_service::load_trunks(&state, org_id)
        .await?
        .iter()
        .filter(|t| t.enabled)
        .map(|t| gateway_name(t.id))
        .collect();
    let result = sip.restart_registration(&names).await;
    crate::audit::log(
        &state.db,
        Some(org_id),
        auth.user_id,
        "telephony.sip_registration.restarted",
        &format!(
            "{} gateway(s): {}",
            names.len(),
            match &result {
                Ok(()) => "pedido aceite".to_string(),
                Err(e) => format!("falhou — {e}"),
            }
        ),
    )
    .await;
    result.map_err(port_error)?;
    Ok((
        StatusCode::ACCEPTED,
        Json(RestartResp {
            gateways: names.len(),
        }),
    ))
}
