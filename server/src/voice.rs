//! Dial-in PSTN — CONTROL PLANE (Fase 1, sub-fase 1).
//!
//! Vive no backend Rust (sem serviço novo). Gere salas de voz, PINs, inventário
//! de DIDs, participantes e CDRs, agnóstico à camada de media. A camada de media
//! (`freeswitch` self-hosted ou `provider`) é escolhida por organização e será
//! ligada nas sub-fases 2/3 através da API interna de IVR aqui exposta.
//!
//! Isolamento multi-tenant: uma sala de voz pertence a uma org; o par (DID, PIN)
//! é a fronteira lógica ao nível da chamada — um PIN nunca abre a sala de outro
//! tenant (índice único `(did_id, pin)` enquanto ativa + validação escopada).

use axum::{
    extract::{Path, State},
    http::HeaderMap,
    Json,
};
use chrono::{DateTime, Utc};
use rand::Rng;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use uuid::Uuid;

use crate::{auth::AuthUser, error::ApiError, org::orgs_of_user, AppState};

// ---------- Enums (persistidos como TEXT) ----------

/// Backend de media escolhido pela org. Impls reais nas sub-fases 2/3.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MediaBackend {
    Freeswitch,
    Provider,
}
impl MediaBackend {
    pub fn as_str(&self) -> &'static str {
        match self {
            MediaBackend::Freeswitch => "freeswitch",
            MediaBackend::Provider => "provider",
        }
    }
    pub fn parse(s: &str) -> MediaBackend {
        match s {
            "provider" => MediaBackend::Provider,
            _ => MediaBackend::Freeswitch, // default seguro (residência)
        }
    }
}

/// Estima o custo (na moeda da tarifa) de uma chamada, arredondando ao minuto.
pub fn estimate_cost(duration_secs: i64, tariff_per_min: f64) -> f64 {
    let minutes = ((duration_secs.max(0) + 59) / 60) as f64; // ceil
    (minutes * tariff_per_min * 10_000.0).round() / 10_000.0
}

// ---------- Tipos de saída ----------

#[derive(Serialize, sqlx::FromRow)]
pub struct VoiceRoom {
    pub id: Uuid,
    pub org_id: Uuid,
    pub room_code: String,
    pub pin: String,
    pub did_id: Option<Uuid>,
    pub media_backend: String,
    pub status: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Serialize, sqlx::FromRow)]
pub struct VoiceParticipant {
    pub id: Uuid,
    pub channel: String,
    pub caller_number: String,
    pub joined_at: DateTime<Utc>,
    pub left_at: Option<DateTime<Utc>>,
}

#[derive(Serialize, sqlx::FromRow)]
pub struct VoiceDid {
    pub id: Uuid,
    pub org_id: Option<Uuid>,
    pub e164: String,
    pub market: String,
    pub model: String,
    pub provider: String,
    pub active: bool,
    pub created_at: DateTime<Utc>,
}

#[derive(Serialize, sqlx::FromRow)]
pub struct VoiceCdr {
    pub id: Uuid,
    pub direction: String,
    pub caller_number: String,
    pub did_e164: String,
    pub duration_secs: i32,
    pub cost_estimate: f64,
    pub started_at: DateTime<Utc>,
    pub ended_at: Option<DateTime<Utc>>,
}

// ---------- Helpers ----------

fn gen_pin() -> String {
    let mut rng = rand::thread_rng();
    format!("{:06}", rng.gen_range(0..1_000_000))
}

/// Org "principal" do utilizador (a sala de voz pertence a esta org).
async fn caller_org(state: &AppState, user_id: Uuid) -> Result<Uuid, ApiError> {
    orgs_of_user(state, user_id)
        .await
        .first()
        .copied()
        .ok_or_else(|| ApiError::BadRequest("utilizador sem organização".into()))
}

// ============================================================
//  API do utilizador (autenticada por sessão, escopada à org)
// ============================================================

#[derive(Deserialize)]
pub struct CreateVoiceRoomReq {
    /// Código da sala de conferência existente (rooms.code) a ligar ao dial-in.
    pub room_code: String,
    /// DID a usar; se omitido, o control plane escolhe segundo o modelo da org.
    #[serde(default)]
    pub did_id: Option<Uuid>,
}

#[derive(Serialize)]
pub struct VoiceRoomResp {
    pub id: Uuid,
    pub room_code: String,
    pub pin: String,
    pub dial_in_number: Option<String>,
    pub media_backend: String,
}

/// Cria uma sala de voz (dial-in) para uma sala de conferência existente.
pub async fn create_room(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Json(req): Json<CreateVoiceRoomReq>,
) -> Result<Json<VoiceRoomResp>, ApiError> {
    let org_id = caller_org(&state, auth.user_id).await?;
    let room_code = req.room_code.trim().to_lowercase();
    if room_code.is_empty() {
        return Err(ApiError::BadRequest("room_code em falta".into()));
    }

    // Backend de media e modelo de DID vêm da configuração da org.
    let (backend, did_model): (String, String) = sqlx::query_as(
        "SELECT voice_media_backend, voice_did_model FROM organizations WHERE id = $1",
    )
    .bind(org_id)
    .fetch_one(&state.db)
    .await?;
    let backend = MediaBackend::parse(&backend).as_str().to_string();

    // Resolver o DID: explícito (validado), dedicado da org, ou do pool partilhado.
    let did: Option<VoiceDid> = if let Some(id) = req.did_id {
        sqlx::query_as(
            "SELECT id, org_id, e164, market, model, provider, active, created_at
             FROM voice_did WHERE id = $1 AND active AND (org_id = $2 OR org_id IS NULL)",
        )
        .bind(id)
        .bind(org_id)
        .fetch_optional(&state.db)
        .await?
    } else if did_model == "dedicated" {
        sqlx::query_as(
            "SELECT id, org_id, e164, market, model, provider, active, created_at
             FROM voice_did WHERE org_id = $1 AND active ORDER BY created_at LIMIT 1",
        )
        .bind(org_id)
        .fetch_optional(&state.db)
        .await?
    } else {
        // Modelo partilhado: primeiro um dedicado da org, senão o pool partilhado.
        sqlx::query_as(
            "SELECT id, org_id, e164, market, model, provider, active, created_at
             FROM voice_did WHERE active AND (org_id = $1 OR org_id IS NULL)
             ORDER BY (org_id = $1) DESC, created_at LIMIT 1",
        )
        .bind(org_id)
        .fetch_optional(&state.db)
        .await?
    };
    let did = did.ok_or_else(|| {
        ApiError::Conflict("sem DID disponível para dial-in nesta organização".into())
    })?;

    // Gera um PIN único para (DID, sala ativa); retenta em colisão.
    let mut last_err = None;
    for _ in 0..8 {
        let pin = gen_pin();
        let res: Result<VoiceRoom, sqlx::Error> = sqlx::query_as(
            "INSERT INTO voice_room (org_id, room_code, pin, did_id, media_backend, created_by)
             VALUES ($1, $2, $3, $4, $5, $6)
             RETURNING id, org_id, room_code, pin, did_id, media_backend, status, created_at",
        )
        .bind(org_id)
        .bind(&room_code)
        .bind(&pin)
        .bind(did.id)
        .bind(&backend)
        .bind(auth.user_id)
        .fetch_one(&state.db)
        .await;
        match res {
            Ok(vr) => {
                return Ok(Json(VoiceRoomResp {
                    id: vr.id,
                    room_code: vr.room_code,
                    pin: vr.pin,
                    dial_in_number: Some(did.e164),
                    media_backend: vr.media_backend,
                }))
            }
            Err(sqlx::Error::Database(dbe)) if dbe.is_unique_violation() => continue,
            Err(e) => last_err = Some(e),
        }
    }
    Err(last_err
        .map(Into::into)
        .unwrap_or_else(|| ApiError::internal("não foi possível gerar PIN")))
}

/// Detalhes de uma sala de voz (membro da org dona).
pub async fn get_room(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> Result<Json<VoiceRoom>, ApiError> {
    let vr: VoiceRoom = sqlx::query_as(
        "SELECT id, org_id, room_code, pin, did_id, media_backend, status, created_at
         FROM voice_room WHERE id = $1",
    )
    .bind(id)
    .fetch_one(&state.db)
    .await?;
    if !orgs_of_user(&state, auth.user_id)
        .await
        .contains(&vr.org_id)
    {
        return Err(ApiError::NotFound);
    }
    Ok(Json(vr))
}

/// Participantes de uma sala de voz.
pub async fn list_participants(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> Result<Json<Vec<VoiceParticipant>>, ApiError> {
    let owner_org: Uuid = sqlx::query_scalar("SELECT org_id FROM voice_room WHERE id = $1")
        .bind(id)
        .fetch_one(&state.db)
        .await?;
    if !orgs_of_user(&state, auth.user_id)
        .await
        .contains(&owner_org)
    {
        return Err(ApiError::NotFound);
    }
    let parts: Vec<VoiceParticipant> = sqlx::query_as(
        "SELECT id, channel, caller_number, joined_at, left_at
         FROM voice_participant WHERE voice_room_id = $1 ORDER BY joined_at",
    )
    .bind(id)
    .fetch_all(&state.db)
    .await?;
    Ok(Json(parts))
}

/// Encerra uma sala de voz (o PIN deixa de ser válido).
pub async fn close_room(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let owner_org: Uuid = sqlx::query_scalar("SELECT org_id FROM voice_room WHERE id = $1")
        .bind(id)
        .fetch_one(&state.db)
        .await?;
    if !orgs_of_user(&state, auth.user_id)
        .await
        .contains(&owner_org)
    {
        return Err(ApiError::NotFound);
    }
    sqlx::query("UPDATE voice_room SET status = 'closed', closed_at = now() WHERE id = $1 AND status = 'active'")
        .bind(id)
        .execute(&state.db)
        .await?;
    sqlx::query(
        "UPDATE voice_participant SET left_at = now() WHERE voice_room_id = $1 AND left_at IS NULL",
    )
    .bind(id)
    .execute(&state.db)
    .await?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

// ---------- Inventário de DIDs (admin) ----------

#[derive(Deserialize)]
pub struct CreateDidReq {
    pub e164: String,
    #[serde(default = "default_market")]
    pub market: String,
    #[serde(default = "default_model")]
    pub model: String,
    #[serde(default)]
    pub provider: String,
    /// Se `shared`, pode ficar sem org (pool). Se ausente, atribui à org do path.
    #[serde(default)]
    pub org_scoped: Option<bool>,
}
fn default_market() -> String {
    "AO".into()
}
fn default_model() -> String {
    "shared".into()
}

/// Adiciona um DID ao inventário de uma org (admin).
pub async fn create_did(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(org_id): Path<Uuid>,
    Json(req): Json<CreateDidReq>,
) -> Result<Json<VoiceDid>, ApiError> {
    crate::org::require_admin_pub(&state, org_id, auth.user_id).await?;
    let e164 = req.e164.trim();
    if !e164.starts_with('+') || e164.len() < 8 || e164.len() > 20 {
        return Err(ApiError::BadRequest(
            "número deve estar em formato +E.164".into(),
        ));
    }
    let model = if req.model == "dedicated" {
        "dedicated"
    } else {
        "shared"
    };
    // shared + org_scoped=false => pool partilhado (org_id NULL).
    let scoped = req.org_scoped.unwrap_or(model == "dedicated");
    let did: VoiceDid = sqlx::query_as(
        "INSERT INTO voice_did (org_id, e164, market, model, provider)
         VALUES ($1, $2, $3, $4, $5)
         RETURNING id, org_id, e164, market, model, provider, active, created_at",
    )
    .bind(if scoped { Some(org_id) } else { None })
    .bind(e164)
    .bind(req.market.trim())
    .bind(model)
    .bind(req.provider.trim())
    .fetch_one(&state.db)
    .await
    .map_err(|e| match e {
        sqlx::Error::Database(dbe) if dbe.is_unique_violation() => {
            ApiError::Conflict("esse número já existe no inventário".into())
        }
        other => other.into(),
    })?;
    Ok(Json(did))
}

/// Lista os DIDs visíveis a uma org (dedicados + pool partilhado).
pub async fn list_dids(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(org_id): Path<Uuid>,
) -> Result<Json<Vec<VoiceDid>>, ApiError> {
    crate::org::require_admin_pub(&state, org_id, auth.user_id).await?;
    let dids: Vec<VoiceDid> = sqlx::query_as(
        "SELECT id, org_id, e164, market, model, provider, active, created_at
         FROM voice_did WHERE org_id = $1 OR org_id IS NULL ORDER BY created_at DESC",
    )
    .bind(org_id)
    .fetch_all(&state.db)
    .await?;
    Ok(Json(dids))
}

/// CDRs da org para billing/auditoria (admin).
pub async fn list_cdr(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(org_id): Path<Uuid>,
) -> Result<Json<Vec<VoiceCdr>>, ApiError> {
    crate::org::require_admin_pub(&state, org_id, auth.user_id).await?;
    let rows: Vec<VoiceCdr> = sqlx::query_as(
        "SELECT id, direction, caller_number, did_e164, duration_secs, cost_estimate, started_at, ended_at
         FROM voice_cdr WHERE org_id = $1 ORDER BY started_at DESC LIMIT 500",
    )
    .bind(org_id)
    .fetch_all(&state.db)
    .await?;
    Ok(Json(rows))
}

#[derive(Deserialize)]
pub struct BillingQuery {
    #[serde(default = "default_period")]
    pub period: String,
}
fn default_period() -> String {
    "month".into()
}

#[derive(Serialize)]
pub struct BillingSummary {
    pub period: String,
    pub calls: i64,
    pub total_minutes: i64,
    pub total_cost: f64,
    pub currency_note: String,
}

/// Resumo de billing de voz do período (admin) — alimenta a faturação Delonix.
pub async fn billing_summary(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(org_id): Path<Uuid>,
    axum::extract::Query(q): axum::extract::Query<BillingQuery>,
) -> Result<Json<BillingSummary>, ApiError> {
    crate::org::require_admin_pub(&state, org_id, auth.user_id).await?;
    let days: i64 = match q.period.as_str() {
        "week" => 7,
        "quarter" => 90,
        "year" => 365,
        _ => 30,
    };
    // minutos = soma do arredondamento ao minuto de cada chamada (coerente com o CDR).
    let row: (Option<i64>, Option<i64>, Option<f64>) = sqlx::query_as(
        "SELECT COUNT(*),
                SUM((duration_secs + 59) / 60)::bigint,
                SUM(cost_estimate)
         FROM voice_cdr
         WHERE org_id = $1 AND started_at >= now() - make_interval(days => $2::int)",
    )
    .bind(org_id)
    .bind(days as i32)
    .fetch_one(&state.db)
    .await?;
    Ok(Json(BillingSummary {
        period: q.period,
        calls: row.0.unwrap_or(0),
        total_minutes: row.1.unwrap_or(0),
        total_cost: row.2.unwrap_or(0.0),
        currency_note: "custo estimado à tarifa VOICE_TARIFF_INBOUND; billing recalcula".into(),
    }))
}

// ============================================================
//  API interna de IVR (chamada pela camada de media)
//  Autenticada por segredo partilhado (X-Voice-Secret).
// ============================================================

fn check_media_secret(state: &AppState, headers: &HeaderMap) -> Result<(), ApiError> {
    let cfg = state.config.voice_internal_secret.as_bytes();
    if cfg.is_empty() {
        return Err(ApiError::NotFound); // feature desativada => não revela nada
    }
    let got = headers
        .get("x-voice-secret")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .as_bytes();
    // Comparação de comprimento-constante simples (segredo de alta entropia).
    if got.len() == cfg.len() && got.iter().zip(cfg).fold(0u8, |a, (x, y)| a | (x ^ y)) == 0 {
        Ok(())
    } else {
        Err(ApiError::Unauthorized)
    }
}

#[derive(Deserialize)]
pub struct ValidatePinReq {
    pub did_e164: String,
    pub pin: String,
}
#[derive(Serialize)]
pub struct ValidatePinResp {
    pub voice_room_id: Uuid,
    pub room_code: String,
    pub media_backend: String,
}

/// Valida (DID, PIN) → devolve a sala a que o chamador PSTN deve ser ligado.
/// Fronteira de isolamento: só encontra salas ATIVAS cujo DID corresponde.
pub async fn ivr_validate_pin(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(req): Json<ValidatePinReq>,
) -> Result<Json<ValidatePinResp>, ApiError> {
    check_media_secret(&state, &headers)?;
    let did = req.did_e164.trim();
    let row: Option<(Uuid, String, String)> = sqlx::query_as(
        "SELECT vr.id, vr.room_code, vr.media_backend
         FROM voice_room vr JOIN voice_did d ON d.id = vr.did_id
         WHERE d.e164 = $1 AND vr.pin = $2 AND vr.status = 'active'",
    )
    .bind(did)
    .bind(req.pin.trim())
    .fetch_optional(&state.db)
    .await?;
    match row {
        Some((id, room_code, backend)) => Ok(Json(ValidatePinResp {
            voice_room_id: id,
            room_code,
            media_backend: backend,
        })),
        None => {
            // Anti-toll-fraud / PIN-guessing: só as FALHAS contam para o limite;
            // chamadores legítimos com PIN certo nunca são penalizados.
            if !state.voice_pin_limiter.check(did) {
                tracing::warn!(did = %did, "possível brute-force de PIN no dial-in — a bloquear");
                return Err(ApiError::TooManyRequests);
            }
            Err(ApiError::NotFound)
        }
    }
}

#[derive(Deserialize)]
pub struct CdrReq {
    pub voice_room_id: Uuid,
    #[serde(default = "inbound")]
    pub direction: String,
    #[serde(default)]
    pub caller_number: String,
    #[serde(default)]
    pub did_e164: String,
    pub duration_secs: i64,
}
fn inbound() -> String {
    "inbound".into()
}

/// Regista um CDR no fim de uma chamada (chamado pela camada de media).
pub async fn ivr_record_cdr(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(req): Json<CdrReq>,
) -> Result<Json<serde_json::Value>, ApiError> {
    check_media_secret(&state, &headers)?;
    let org_id: Uuid = sqlx::query_scalar("SELECT org_id FROM voice_room WHERE id = $1")
        .bind(req.voice_room_id)
        .fetch_one(&state.db)
        .await?;
    let cost = estimate_cost(req.duration_secs, state.config.voice_tariff_inbound);
    let id: Uuid = sqlx::query_scalar(
        "INSERT INTO voice_cdr
             (org_id, voice_room_id, direction, caller_number, did_e164, duration_secs, cost_estimate, ended_at)
         VALUES ($1, $2, $3, $4, $5, $6, $7, now()) RETURNING id",
    )
    .bind(org_id)
    .bind(req.voice_room_id)
    .bind(&req.direction)
    .bind(req.caller_number.trim())
    .bind(req.did_e164.trim())
    .bind(req.duration_secs as i32)
    .bind(cost)
    .fetch_one(&state.db)
    .await?;
    Ok(Json(serde_json::json!({ "id": id, "cost_estimate": cost })))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pin_is_six_digits() {
        for _ in 0..100 {
            let p = gen_pin();
            assert_eq!(p.len(), 6);
            assert!(p.chars().all(|c| c.is_ascii_digit()));
        }
    }

    #[test]
    fn cost_rounds_up_to_the_minute() {
        assert_eq!(estimate_cost(0, 10.0), 0.0);
        assert_eq!(estimate_cost(1, 10.0), 10.0); // 1s → 1 min
        assert_eq!(estimate_cost(60, 10.0), 10.0);
        assert_eq!(estimate_cost(61, 10.0), 20.0); // 61s → 2 min
        assert_eq!(estimate_cost(600, 2.5), 25.0);
    }

    #[test]
    fn media_backend_defaults_to_freeswitch() {
        assert_eq!(MediaBackend::parse("provider").as_str(), "provider");
        assert_eq!(MediaBackend::parse("freeswitch").as_str(), "freeswitch");
        assert_eq!(MediaBackend::parse("qualquer").as_str(), "freeswitch"); // default residência
    }
}
