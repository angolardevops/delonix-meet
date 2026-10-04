//! O PIN de um ramal (plano de produção, item 3.8, lote 1 — R276).
//!
//! Três coisas separadas (decisão do dono, 2026-10-04): o NÚMERO do ramal é a
//! identidade e não é secreto; a PASSWORD SIP é a credencial do aparelho
//! (`ramais.rs`); o PIN é um código secreto de seis dígitos.
//!
//! O que este módulo garante:
//!
//! - o PIN é sorteado pelo servidor (aleatoriedade do SO, sem enviesamento) ou
//!   escolhido por quem tem direito a isso, e nos dois casos passa pelas mesmas
//!   recusas (`delonix_meet_domain::telephony::extension_pin::pin_refusal`);
//! - só o hash (Argon2, `auth::hash_password`) fica na base; o valor em claro
//!   existe UMA vez, na resposta que o gera, e nenhuma leitura o devolve — as
//!   leituras só dizem o estado (`unset` / `set` / `locked`);
//! - **quem vê o PIN de quem.** O PIN de um ramal de PESSOA é dela: só ela o
//!   gera ou escolhe, na sua área (`/my-extension`). O administrador não o vê
//!   nem o define — «forçar a regeneração» é LIMPAR o PIN
//!   (`DELETE …/extensions/{id}/pin`): fica «por definir» e a pessoa gera um
//!   novo. O PIN de um ramal da EMPRESA (sem pessoa) é do administrador: é ele
//!   que o gera ou escolhe, e o vê uma vez;
//! - cinco falhas numa janela de quinze minutos bloqueiam o PIN — quinze
//!   minutos no primeiro bloqueio, o dobro em cada reincidência —, e cada
//!   falha e cada bloqueio ficam na auditoria imutável (`audit::log`), com o
//!   actor de SISTEMA e o ramal no alvo — nunca em nome do dono do ramal, que
//!   é a vítima de quem anda a adivinhar.
//!
//! **A verificação (R277)** passa primeiro por um travão por ORIGEM da
//! chamada (número e rede, segundo o FreeSWITCH), que trava à terceira falha
//! — antes de a mesma origem poder juntar as cinco que bloqueiam um ramal. O
//! contador do ramal tem janela, e o bloqueio dobra a cada reincidência.

use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    Json,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use uuid::Uuid;

use crate::{auth::AuthUser, error::ApiError, voice::check_media_secret, AppState};
use delonix_meet_core::DomainError;
use delonix_meet_domain::telephony::extension_pin as rules;

// ---------- Gerar, validar e guardar ----------

/// Um PIN novo: seis dígitos da aleatoriedade do SO (`crate::crypto`, regra 4
/// do ADR-0004 §5), sorteados de novo enquanto caírem numa das recusas.
fn generate_pin(extension: &str) -> String {
    loop {
        let bytes = crate::crypto::random_bytes(4);
        let random = u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
        if let Some(pin) = rules::pin_candidate(random) {
            if rules::pin_refusal(&pin, extension).is_none() {
                return pin;
            }
        }
    }
}

/// As recusas, para um PIN escolhido por uma pessoa.
fn check_chosen_pin(pin: &str, extension: &str) -> Result<(), ApiError> {
    match rules::pin_refusal(pin, extension) {
        None => Ok(()),
        Some(r) => Err(DomainError::invalid(r.code(), r.message()).into()),
    }
}

/// Guarda o hash do PIN e levanta qualquer bloqueio: um PIN novo começa do zero.
async fn store_pin(state: &AppState, id: Uuid, pin: &str) -> Result<(), ApiError> {
    let hash = crate::auth::hash_password(pin)?;
    sqlx::query(
        "UPDATE voice_extensions
            SET pin_hash = $2, pin_set_at = now(), pin_failed_attempts = 0, pin_locked_until = NULL
          WHERE id = $1",
    )
    .bind(id)
    .bind(hash)
    .execute(&state.db)
    .await?;
    Ok(())
}

// ---------- Tipos ----------

/// O ramal de quem pede, visto pela própria pessoa.
#[derive(Debug, Serialize, sqlx::FromRow, utoipa::ToSchema)]
pub struct MyExtension {
    pub id: Uuid,
    #[schema(example = "1004")]
    pub extension: String,
    pub label: String,
    pub active: bool,
    /// `unset` (por definir), `set` (definido) ou `locked` (bloqueado).
    #[schema(example = "set")]
    pub pin_state: String,
    /// Número curto que o ramal marca para entrar numa reunião.
    #[sqlx(default)]
    pub meeting_access_number: String,
}

#[derive(Deserialize, utoipa::ToSchema)]
pub struct SetPinReq {
    /// Seis dígitos. Recusados: todos iguais, sequências, e o que contém o
    /// número do ramal.
    #[schema(example = "482913")]
    pub pin: String,
}

/// Um PIN acabado de gerar. Sai UMA vez: só o hash fica guardado.
#[derive(Serialize, utoipa::ToSchema)]
pub struct GeneratedPin {
    #[schema(example = "482913")]
    pub pin: String,
    /// O ramal a que o PIN pertence.
    #[schema(example = "1004")]
    pub extension: String,
}

// ============================================================
//  A própria pessoa — o seu ramal e o seu PIN
// ============================================================

async fn own_extension(
    state: &AppState,
    org_id: Uuid,
    user_id: Uuid,
) -> Result<MyExtension, ApiError> {
    crate::org::require_member_pub(state, org_id, user_id).await?;
    let mut ext: MyExtension = sqlx::query_as(
        "SELECT e.id, e.extension, e.label, e.active,
                CASE WHEN e.pin_hash IS NULL THEN 'unset'
                     WHEN e.pin_locked_until > now() THEN 'locked'
                     ELSE 'set' END AS pin_state
           FROM voice_extensions e
          WHERE e.org_id = $1 AND e.member_id = $2",
    )
    .bind(org_id)
    .bind(user_id)
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(|| DomainError::not_found("ramais.no_extension"))?;
    ext.meeting_access_number = state.config.voice_meeting_access_number.clone();
    Ok(ext)
}

/// O ramal de quem pede nesta organização (qualquer membro activo).
#[utoipa::path(
    get, path = "/api/orgs/{org_id}/my-extension", tag = "voice",
    security(("session" = [])),
    params(("org_id" = Uuid, Path, description = "Organização.")),
    responses(
        (status = 200, body = MyExtension, description = "O número e o ESTADO do PIN — nunca o valor."),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 404, body = crate::openapi::ErrorBody, description = "Não é membro activo, ou ainda não tem ramal (`ramais.no_extension`)."),
    )
)]
pub async fn my_extension(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(org_id): Path<Uuid>,
) -> Result<Json<MyExtension>, ApiError> {
    own_extension(&state, org_id, auth.user_id).await.map(Json)
}

/// A pessoa escolhe o PIN do seu ramal. Substitui o anterior e levanta o
/// bloqueio: quem chegou aqui autenticou-se com a sessão.
#[utoipa::path(
    put, path = "/api/orgs/{org_id}/my-extension/pin", tag = "voice",
    security(("session" = [])),
    params(("org_id" = Uuid, Path, description = "Organização.")),
    request_body = SetPinReq,
    responses(
        (status = 204, description = "PIN guardado (só o hash)."),
        (status = 400, body = crate::openapi::ErrorBody, description = "`ramais.pin_format`, `ramais.pin_repeated`, `ramais.pin_sequence` ou `ramais.pin_contains_extension`."),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 404, body = crate::openapi::ErrorBody),
    )
)]
pub async fn set_my_pin(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(org_id): Path<Uuid>,
    Json(req): Json<SetPinReq>,
) -> Result<StatusCode, ApiError> {
    let ext = own_extension(&state, org_id, auth.user_id).await?;
    check_chosen_pin(&req.pin, &ext.extension)?;
    store_pin(&state, ext.id, &req.pin).await?;
    crate::audit::log(
        &state.db,
        Some(org_id),
        auth.user_id,
        "ramal.pin_alterado",
        &ext.extension,
    )
    .await;
    Ok(StatusCode::NO_CONTENT)
}

/// Gera um PIN novo para o ramal de quem pede e mostra-o UMA vez (*custom
/// method*). É também como a pessoa obtém o primeiro PIN, e o caminho depois
/// de o administrador o ter limpo.
#[utoipa::path(
    post, path = "/api/orgs/{org_id}/my-extension/regenerate-pin", tag = "voice",
    security(("session" = [])),
    params(("org_id" = Uuid, Path, description = "Organização.")),
    responses(
        (status = 200, body = GeneratedPin, description = "O PIN sai UMA vez; o anterior deixa de servir."),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 404, body = crate::openapi::ErrorBody),
    )
)]
pub async fn regenerate_my_pin(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(org_id): Path<Uuid>,
) -> Result<Json<GeneratedPin>, ApiError> {
    let ext = own_extension(&state, org_id, auth.user_id).await?;
    let pin = generate_pin(&ext.extension);
    store_pin(&state, ext.id, &pin).await?;
    crate::audit::log(
        &state.db,
        Some(org_id),
        auth.user_id,
        "ramal.pin_gerado",
        &ext.extension,
    )
    .await;
    Ok(Json(GeneratedPin {
        pin,
        extension: ext.extension,
    }))
}

// ============================================================
//  O administrador
// ============================================================

/// `(número, é da empresa)` de um ramal desta organização.
async fn admin_target(
    state: &AppState,
    org_id: Uuid,
    admin: Uuid,
    id: Uuid,
) -> Result<(String, bool), ApiError> {
    crate::org::require_admin_pub(state, org_id, admin).await?;
    sqlx::query_as(
        "SELECT extension, member_id IS NULL FROM voice_extensions WHERE id = $1 AND org_id = $2",
    )
    .bind(id)
    .bind(org_id)
    .fetch_optional(&state.db)
    .await?
    .ok_or(ApiError::NotFound)
}

/// O PIN de um ramal de pessoa não se define nem se vê por terceiros.
fn company_only(is_company: bool) -> Result<(), ApiError> {
    if is_company {
        return Ok(());
    }
    Err(DomainError::conflict(
        "ramais.pin_belongs_to_member",
        "o PIN de um ramal de pessoa é dela: limpa-o, e ela gera um novo na sua área",
    )
    .into())
}

/// O administrador escolhe o PIN de um ramal da EMPRESA.
#[utoipa::path(
    put, path = "/api/orgs/{org_id}/extensions/{id}/pin", tag = "voice",
    security(("session" = [])),
    params(("org_id" = Uuid, Path, description = "Organização."), ("id" = Uuid, Path, description = "Ramal.")),
    request_body = SetPinReq,
    responses(
        (status = 204, description = "PIN guardado (só o hash)."),
        (status = 400, body = crate::openapi::ErrorBody, description = "`ramais.pin_format`, `ramais.pin_repeated`, `ramais.pin_sequence` ou `ramais.pin_contains_extension`."),
        (status = 409, body = crate::openapi::ErrorBody, description = "O ramal é de uma pessoa (`ramais.pin_belongs_to_member`)."),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 403, body = crate::openapi::ErrorBody),
        (status = 404, body = crate::openapi::ErrorBody),
    )
)]
pub async fn set_extension_pin(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path((org_id, id)): Path<(Uuid, Uuid)>,
    Json(req): Json<SetPinReq>,
) -> Result<StatusCode, ApiError> {
    let (extension, is_company) = admin_target(&state, org_id, auth.user_id, id).await?;
    company_only(is_company)?;
    check_chosen_pin(&req.pin, &extension)?;
    store_pin(&state, id, &req.pin).await?;
    crate::audit::log(
        &state.db,
        Some(org_id),
        auth.user_id,
        "ramal.pin_alterado",
        &extension,
    )
    .await;
    Ok(StatusCode::NO_CONTENT)
}

/// Gera o PIN de um ramal da EMPRESA e mostra-o UMA vez ao administrador
/// (*custom method*).
#[utoipa::path(
    post, path = "/api/orgs/{org_id}/extensions/{id}/regenerate-pin", tag = "voice",
    security(("session" = [])),
    params(("org_id" = Uuid, Path, description = "Organização."), ("id" = Uuid, Path, description = "Ramal.")),
    responses(
        (status = 200, body = GeneratedPin, description = "O PIN sai UMA vez; o anterior deixa de servir."),
        (status = 409, body = crate::openapi::ErrorBody, description = "O ramal é de uma pessoa (`ramais.pin_belongs_to_member`): o administrador não vê o PIN dela."),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 403, body = crate::openapi::ErrorBody),
        (status = 404, body = crate::openapi::ErrorBody),
    )
)]
pub async fn regenerate_extension_pin(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path((org_id, id)): Path<(Uuid, Uuid)>,
) -> Result<Json<GeneratedPin>, ApiError> {
    let (extension, is_company) = admin_target(&state, org_id, auth.user_id, id).await?;
    company_only(is_company)?;
    let pin = generate_pin(&extension);
    store_pin(&state, id, &pin).await?;
    crate::audit::log(
        &state.db,
        Some(org_id),
        auth.user_id,
        "ramal.pin_gerado",
        &extension,
    )
    .await;
    Ok(Json(GeneratedPin { pin, extension }))
}

/// Limpa o PIN de um ramal (admin): fica «por definir» e o bloqueio é
/// levantado. Num ramal de pessoa é assim que o administrador FORÇA a
/// regeneração sem ver o PIN — a pessoa gera um novo na sua área. Idempotente.
#[utoipa::path(
    delete, path = "/api/orgs/{org_id}/extensions/{id}/pin", tag = "voice",
    security(("session" = [])),
    params(("org_id" = Uuid, Path, description = "Organização."), ("id" = Uuid, Path, description = "Ramal.")),
    responses(
        (status = 204, description = "PIN limpo; a resposta não traz PIN nenhum."),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 403, body = crate::openapi::ErrorBody),
        (status = 404, body = crate::openapi::ErrorBody),
    )
)]
pub async fn clear_extension_pin(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path((org_id, id)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, ApiError> {
    let (extension, _) = admin_target(&state, org_id, auth.user_id, id).await?;
    sqlx::query(
        "UPDATE voice_extensions
            SET pin_hash = NULL, pin_set_at = NULL, pin_failed_attempts = 0, pin_locked_until = NULL
          WHERE id = $1 AND org_id = $2",
    )
    .bind(id)
    .bind(org_id)
    .execute(&state.db)
    .await?;
    crate::audit::log(
        &state.db,
        Some(org_id),
        auth.user_id,
        "ramal.pin_limpo",
        &extension,
    )
    .await;
    Ok(StatusCode::NO_CONTENT)
}

// ============================================================
//  Verificação — para o IVR (R276, endurecida na R277)
// ============================================================

/// O que a verificação de um PIN conclui.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum PinCheck {
    Valid {
        extension_id: Uuid,
        member_id: Option<Uuid>,
        display_name: String,
    },
    /// PIN errado — e também ramal inexistente, inactivo ou de pessoa
    /// arquivada: quem liga não distingue os casos.
    Invalid,
    /// O RAMAL está bloqueado por falhas. Não se verifica nada enquanto durar.
    Locked { retry_after_secs: i64 },
    /// O ramal existe mas não tem PIN.
    NotSet,
    /// A ORIGEM da chamada está travada: nada foi verificado, e o ramal não
    /// foi tocado (R277).
    OriginLocked { retry_after_secs: i64 },
}

/// De onde vem a chamada, segundo o FreeSWITCH: o número de quem liga
/// (`caller_id_number`) e o endereço do par SIP que a entregou
/// (`sip_network_ip` — atrás do Kamailio, o do Kamailio). Nenhum dos dois é
/// uma prova de identidade: o número de quem liga pode ser forjado na rede
/// telefónica. Servem para TRAVAR, não para autenticar.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct CallOrigin {
    #[serde(default)]
    pub caller_number: String,
    #[serde(default)]
    pub network_ip: String,
}

impl CallOrigin {
    fn clean(raw: &str, max: usize) -> String {
        raw.trim()
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '.' | ':' | '-' | '_'))
            .take(max)
            .collect()
    }

    /// A chave do travão: `<rede>|<número>`. Sem número (chamada anónima),
    /// todas as anónimas da mesma rede partilham UMA chave — é o travão mais
    /// apertado, de propósito.
    pub(crate) fn key(&self) -> String {
        format!(
            "{}|{}",
            Self::clean(&self.network_ip, 64),
            Self::clean(&self.caller_number, 32)
        )
    }

    /// Para a auditoria: o administrador vê de onde vieram as tentativas.
    pub(crate) fn describe(&self) -> String {
        let number = Self::clean(&self.caller_number, 32);
        let net = Self::clean(&self.network_ip, 64);
        format!(
            "origem {} via {}",
            if number.is_empty() {
                "sem número"
            } else {
                &number
            },
            if net.is_empty() {
                "rede desconhecida"
            } else {
                &net
            },
        )
    }
}

/// Um contador, com as idades calculadas pela base (um só relógio).
#[derive(sqlx::FromRow)]
struct CounterRow {
    failures: i32,
    window_age: Option<i64>,
    lock_level: i32,
    lock_ended_ago: Option<i64>,
}

impl From<CounterRow> for rules::Counter {
    fn from(r: CounterRow) -> Self {
        rules::Counter {
            failures: r.failures,
            window_age_secs: r.window_age,
            lock_level: r.lock_level,
            lock_ended_secs_ago: r.lock_ended_ago,
        }
    }
}

/// O que cobrar à origem devolveu.
enum OriginCharge {
    /// Travada: não se verifica nada.
    Locked(i64),
    /// Cobrada uma falha; `lock_now` se foi esta que a travou.
    Charged { lock_now: bool },
}

/// Cobra uma falha à origem ANTES de verificar — e devolve-a se o PIN estiver
/// certo ([`refund_origin`]). Contar depois deixava dez pedidos em paralelo
/// da mesma origem passarem todos pelo «ainda não travada» e chegarem ao
/// ramal: cinco deles bloqueavam-no. Cobrando primeiro, com a linha da origem
/// em `FOR UPDATE`, só `ORIGIN_THROTTLE.max_failures` chegam a ver um ramal.
async fn charge_origin(state: &AppState, key: &str) -> Result<OriginCharge, ApiError> {
    let t = rules::ORIGIN_THROTTLE;
    let mut tx = state.db.begin().await?;
    sqlx::query(
        "INSERT INTO voice_pin_origins (origin) VALUES ($1) ON CONFLICT (origin) DO NOTHING",
    )
    .bind(key)
    .execute(&mut *tx)
    .await?;
    let c: rules::Counter = sqlx::query_as::<_, CounterRow>(
        "SELECT failures,
                FLOOR(EXTRACT(EPOCH FROM (now() - window_started_at)))::BIGINT AS window_age,
                lock_level,
                FLOOR(EXTRACT(EPOCH FROM (now() - locked_until)))::BIGINT AS lock_ended_ago
           FROM voice_pin_origins WHERE origin = $1 FOR UPDATE",
    )
    .bind(key)
    .fetch_one(&mut *tx)
    .await?
    .into();
    if let Some(secs) = t.locked_for(&c) {
        tx.commit().await?;
        return Ok(OriginCharge::Locked(secs));
    }
    let a = t.after_failure(&c);
    sqlx::query(
        "UPDATE voice_pin_origins
            SET failures = $2,
                window_started_at = CASE WHEN $5::float8 IS NOT NULL THEN NULL
                                         WHEN $3 THEN now() ELSE window_started_at END,
                lock_level = $4,
                locked_until = CASE WHEN $5::float8 IS NULL THEN locked_until
                                    ELSE now() + make_interval(secs => $5::float8) END,
                updated_at = now()
          WHERE origin = $1",
    )
    .bind(key)
    .bind(a.failures)
    .bind(a.restart_window)
    .bind(a.lock_level)
    .bind(a.lock_secs.map(|s| s as f64))
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(OriginCharge::Charged {
        lock_now: a.lock_secs.is_some(),
    })
}

/// Um acerto devolve à origem a falha que se lhe cobrou — e só essa: o
/// contador fica como estava antes deste pedido. Zerá-lo deixava quem tem um
/// PIN válido alternar «dois palpites, um acerto» sem nunca ser travado.
async fn refund_origin(state: &AppState, key: &str, lock_now: bool) -> Result<(), ApiError> {
    sqlx::query(
        "UPDATE voice_pin_origins
            SET failures = CASE WHEN $2 THEN $3 - 1 ELSE GREATEST(failures - 1, 0) END,
                window_started_at = CASE WHEN $2 THEN now() ELSE window_started_at END,
                lock_level = CASE WHEN $2 THEN GREATEST(lock_level - 1, 0) ELSE lock_level END,
                locked_until = CASE WHEN $2 THEN NULL ELSE locked_until END,
                updated_at = now()
          WHERE origin = $1",
    )
    .bind(key)
    .bind(lock_now)
    .bind(rules::ORIGIN_THROTTLE.max_failures)
    .execute(&state.db)
    .await?;
    Ok(())
}

/// Um Argon2 contra um hash que não é de ninguém: os caminhos que não têm PIN
/// para verificar (ramal inexistente, PIN por definir) custam o mesmo tempo
/// que um PIN errado. A resposta ao IVR distingue-os; o tempo deixa de o
/// fazer.
fn burn_like_a_verification(pin: &str) {
    static DUMMY: std::sync::LazyLock<String> =
        std::sync::LazyLock::new(|| crate::auth::hash_password("590317").unwrap_or_default());
    let _ = crate::auth::verify_password(pin, &DUMMY);
}

/// A verificação vinda de uma chamada: o travão por ORIGEM primeiro, depois o
/// ramal (R277).
///
/// 1. Cobra-se uma falha à origem ([`charge_origin`]). Travada, a resposta é
///    `OriginLocked` e nem a organização nem o ramal são lidos: uma origem
///    abusiva não gasta tentativas de ninguém.
/// 2. A organização é a do domínio; um domínio que não é de ninguém é um PIN
///    errado (e a falha fica cobrada à origem).
/// 3. O ramal ([`verify_pin`]), com o seu próprio contador.
/// 4. Um acerto devolve à origem a falha cobrada.
///
/// Como a origem trava à terceira falha e o ramal à quinta, a mesma origem
/// não bloqueia o ramal de ninguém — nem falhando em muitos ramais.
pub(crate) async fn verify_from_call(
    state: &AppState,
    org_id: Option<Uuid>,
    extension: &str,
    pin: &str,
    origin: &CallOrigin,
) -> Result<PinCheck, ApiError> {
    let key = origin.key();
    let lock_now = match charge_origin(state, &key).await? {
        OriginCharge::Locked(secs) => {
            tracing::warn!(origem = %origin.describe(), "PIN de ramal: origem travada — nada verificado");
            return Ok(PinCheck::OriginLocked {
                retry_after_secs: secs,
            });
        }
        OriginCharge::Charged { lock_now } => lock_now,
    };
    let check = match org_id {
        Some(org_id) => verify_pin(state, org_id, extension, pin, origin).await?,
        None => {
            burn_like_a_verification(pin);
            PinCheck::Invalid
        }
    };
    if matches!(check, PinCheck::Valid { .. }) {
        refund_origin(state, &key, lock_now).await?;
    } else if lock_now {
        tracing::warn!(origem = %origin.describe(), "PIN de ramal: origem travada por falhas");
        crate::audit::log_com_metricas(
            &state.db,
            Some(&state.metrics),
            org_id,
            Uuid::nil(),
            "ramal.origem_travada",
            &format!(
                "{} — {} min — ramal {extension}",
                origin.describe(),
                rules::ORIGIN_THROTTLE.base_lock_secs / 60
            ),
        )
        .await;
    }
    Ok(check)
}

/// Verifica o PIN de um ramal ACTIVO da organização. Conta as falhas numa
/// janela, bloqueia à quinta com duração crescente
/// (`rules::EXTENSION_THROTTLE`), e regista cada falha e cada bloqueio na
/// auditoria imutável, com a origem da chamada.
///
/// A linha do ramal é lida com `FOR UPDATE`: duas tentativas simultâneas não
/// contam como uma, e pedidos em paralelo não dão palpites a mais — dez
/// verificações erradas ao mesmo tempo contam cinco falhas e as outras cinco
/// já encontram o ramal bloqueado (`tests/ramal_pin.rs`,
/// `dez_palpites_em_paralelo_contam_cinco_e_bloqueiam`).
async fn verify_pin(
    state: &AppState,
    org_id: Uuid,
    extension: &str,
    pin: &str,
    origin: &CallOrigin,
) -> Result<PinCheck, ApiError> {
    let t = rules::EXTENSION_THROTTLE;
    #[derive(sqlx::FromRow)]
    struct Row {
        id: Uuid,
        member_id: Option<Uuid>,
        label: String,
        name: Option<String>,
        pin_hash: Option<String>,
        #[sqlx(flatten)]
        counter: CounterRow,
    }
    // O ramal de uma pessoa arquivada não identifica ninguém. A pertença
    // decide-se em org.rs (regra 1, ADR-0004 §5) — e ANTES de abrir a
    // transacção, para não segurar duas ligações da pool ao mesmo tempo.
    let owner: Option<Option<Uuid>> = sqlx::query_scalar(
        "SELECT member_id FROM voice_extensions
          WHERE org_id = $1 AND extension = $2 AND active",
    )
    .bind(org_id)
    .bind(extension)
    .fetch_optional(&state.db)
    .await?;
    match owner {
        None => {
            burn_like_a_verification(pin);
            return Ok(PinCheck::Invalid);
        }
        Some(Some(member_id)) => {
            if crate::org::role_in_org(state, org_id, member_id)
                .await?
                .is_none()
            {
                burn_like_a_verification(pin);
                return Ok(PinCheck::Invalid);
            }
        }
        Some(None) => {}
    }

    let mut tx = state.db.begin().await?;
    let row: Option<Row> = sqlx::query_as(
        "SELECT e.id, e.member_id, e.label, COALESCE(u.display_name, u.username) AS name,
                e.pin_hash, e.pin_failed_attempts AS failures,
                FLOOR(EXTRACT(EPOCH FROM (now() - e.pin_failure_window_at)))::BIGINT AS window_age,
                e.pin_lock_level AS lock_level,
                FLOOR(EXTRACT(EPOCH FROM (now() - e.pin_locked_until)))::BIGINT AS lock_ended_ago
           FROM voice_extensions e LEFT JOIN users u ON u.id = e.member_id
          WHERE e.org_id = $1 AND e.extension = $2 AND e.active
            FOR UPDATE OF e",
    )
    .bind(org_id)
    .bind(extension)
    .fetch_optional(&mut *tx)
    .await?;
    let Some(row) = row else {
        burn_like_a_verification(pin);
        return Ok(PinCheck::Invalid);
    };
    let counter: rules::Counter = row.counter.into();
    if let Some(secs) = t.locked_for(&counter) {
        return Ok(PinCheck::Locked {
            retry_after_secs: secs,
        });
    }
    let Some(hash) = row.pin_hash else {
        burn_like_a_verification(pin);
        return Ok(PinCheck::NotSet);
    };

    if crate::auth::verify_password(pin, &hash) {
        // Um acerto zera tudo, incluindo o nível: quem sabe o PIN é o dono.
        sqlx::query(
            "UPDATE voice_extensions
                SET pin_failed_attempts = 0, pin_failure_window_at = NULL,
                    pin_lock_level = 0, pin_locked_until = NULL
              WHERE id = $1",
        )
        .bind(row.id)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        return Ok(PinCheck::Valid {
            extension_id: row.id,
            member_id: row.member_id,
            display_name: row.name.unwrap_or(row.label),
        });
    }

    let a = t.after_failure(&counter);
    // Ao bloquear, a janela fecha: passado o bloqueio, são outra vez cinco
    // tentativas numa janela nova. O `pin_locked_until` de um bloqueio antigo
    // fica: é por ele que o nível se esquece (`level_decay_secs`).
    sqlx::query(
        "UPDATE voice_extensions
            SET pin_failed_attempts = $2,
                pin_failure_window_at = CASE WHEN $5::float8 IS NOT NULL THEN NULL
                                             WHEN $3 THEN now() ELSE pin_failure_window_at END,
                pin_lock_level = $4,
                pin_locked_until = CASE WHEN $5::float8 IS NULL THEN pin_locked_until
                                        ELSE now() + make_interval(secs => $5::float8) END
          WHERE id = $1",
    )
    .bind(row.id)
    .bind(a.failures)
    .bind(a.restart_window)
    .bind(a.lock_level)
    .bind(a.lock_secs.map(|s| s as f64))
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;

    // Quem falhou NÃO é o dono do ramal — é quem ligou, e esse não se conhece.
    // O actor é o de sistema (`Uuid::nil()`, como em `member.guest_expired` e
    // `odoo.provision`): atribuir a falha à pessoa do ramal punha a VÍTIMA de
    // uma tentativa de adivinhação como autora dela na trilha. O ramal vai no
    // `target`, com a origem da chamada (R277). O PIN tentado NUNCA entra.
    let actor = Uuid::nil();
    let from = origin.describe();
    crate::audit::log_com_metricas(
        &state.db,
        Some(&state.metrics),
        Some(org_id),
        actor,
        "ramal.pin_falhado",
        &format!(
            "ramal {extension} — tentativa {} de {} — {from}",
            a.attempt, t.max_failures
        ),
    )
    .await;
    if let Some(secs) = a.lock_secs {
        tracing::warn!(%org_id, ramal = %extension, nivel = a.lock_level, "PIN de ramal bloqueado por falhas");
        crate::audit::log_com_metricas(
            &state.db,
            Some(&state.metrics),
            Some(org_id),
            actor,
            "ramal.pin_bloqueado",
            &format!(
                "ramal {extension} — {} min (bloqueio n.º {}) — {from}",
                secs / 60,
                a.lock_level
            ),
        )
        .await;
        return Ok(PinCheck::Locked {
            retry_after_secs: secs,
        });
    }
    Ok(PinCheck::Invalid)
}

#[derive(Deserialize)]
pub struct VerifyExtensionPinReq {
    /// Domínio SIP da organização (`<slug>.<VOICE_RAMAIS_DOMAIN_SUFFIX>`).
    pub domain: String,
    pub extension: String,
    pub pin: String,
    /// De onde vem a chamada. Obrigatório: é a chave do travão por origem.
    pub origin: CallOrigin,
    /// A sala de voz em que quem liga vai entrar (a do `validate`). Com ela, um
    /// acerto traz em `channel_vars` o bilhete que identifica a pessoa na
    /// ponte. Não escolhe a organização — essa é a do `domain` —; uma sala
    /// que não é ACTIVA nessa organização responde como um PIN errado.
    #[serde(default)]
    pub voice_room_id: Option<Uuid>,
}

/// Contrato com o IVR. Sempre `200`: um PIN errado é uma resposta, não uma
/// falha HTTP. **As razões são para o IVR e para os testes, não para quem
/// liga:** o Lua dá UMA só recusa, igual para todas.
#[derive(Serialize)]
pub struct VerifyExtensionPinResp {
    pub valid: bool,
    /// `invalid`, `locked`, `not_set` ou `origin_locked`. Ausente quando `valid`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub retry_after_secs: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extension_id: Option<Uuid>,
    /// A pessoa identificada. Ausente num ramal da empresa.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub member_id: Option<Uuid>,
    /// Nome com que a pessoa (ou o ramal da empresa) entra.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    /// Variáveis de canal a juntar às do `room_bridge` antes do `bridge`:
    /// trazem o bilhete de identidade para a ponte. Só com `voice_room_id`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub channel_vars: Option<std::collections::BTreeMap<String, String>>,
}

impl VerifyExtensionPinResp {
    fn refused(reason: &'static str, retry_after_secs: Option<i64>) -> Self {
        Self {
            valid: false,
            reason: Some(reason),
            retry_after_secs,
            extension_id: None,
            member_id: None,
            display_name: None,
            channel_vars: None,
        }
    }

    fn from_check(check: PinCheck) -> Self {
        match check {
            PinCheck::Valid {
                extension_id,
                member_id,
                display_name,
            } => Self {
                valid: true,
                reason: None,
                retry_after_secs: None,
                extension_id: Some(extension_id),
                member_id,
                display_name: Some(display_name),
                channel_vars: None,
            },
            PinCheck::Invalid => Self::refused("invalid", None),
            PinCheck::NotSet => Self::refused("not_set", None),
            PinCheck::Locked { retry_after_secs } => {
                Self::refused("locked", Some(retry_after_secs))
            }
            PinCheck::OriginLocked { retry_after_secs } => {
                Self::refused("origin_locked", Some(retry_after_secs))
            }
        }
    }
}

/// `POST /internal/v1/voice/ivr/verify-extension-pin` — no listener interno,
/// com o segredo de voz (`X-Voice-Secret`), como as outras rotas do IVR.
pub async fn ivr_verify_extension_pin(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(req): Json<VerifyExtensionPinReq>,
) -> Result<Json<VerifyExtensionPinResp>, ApiError> {
    check_media_secret(&state, &headers)?;
    // Um domínio que não é de nenhuma organização responde como um PIN errado
    // — e uma sala que não é dessa organização também: sem organização, a
    // verificação cobra a falha à origem e não lê ramal nenhum.
    let mut org_id = crate::ramais::org_id_by_sip_domain(&state, req.domain.trim()).await;
    if let (Some(org), Some(room)) = (org_id, req.voice_room_id) {
        if crate::voice::voice_room_code_in_org(&state, org, room)
            .await
            .is_none()
        {
            org_id = None;
        }
    }
    let check = verify_from_call(
        &state,
        org_id,
        req.extension.trim(),
        req.pin.trim(),
        &req.origin,
    )
    .await?;
    // Identificado, e a caminho de uma sala: o bilhete para a ponte.
    let ticket_vars = match (&check, org_id, req.voice_room_id) {
        (
            PinCheck::Valid {
                extension_id,
                member_id,
                display_name,
            },
            Some(org),
            Some(room),
        ) => {
            let who = crate::voice_caller::CallerIdentity {
                display_name: display_name.clone(),
                member_id: *member_id,
            };
            crate::voice::caller_ticket_vars_for_voice_room(&state, org, room, *extension_id, &who)
                .await
        }
        _ => None,
    };
    let mut resp = VerifyExtensionPinResp::from_check(check);
    resp.channel_vars = ticket_vars;
    Ok(Json(resp))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn o_pin_gerado_tem_seis_digitos_e_passa_as_recusas() {
        let mut seen = std::collections::HashSet::new();
        for _ in 0..500 {
            let pin = generate_pin("1234");
            assert_eq!(pin.len(), 6);
            assert!(pin.bytes().all(|b| b.is_ascii_digit()));
            assert_eq!(rules::pin_refusal(&pin, "1234"), None, "{pin}");
            seen.insert(pin);
        }
        // 500 sorteios em 10⁶: mais de um punhado de repetições é um gerador
        // partido, não azar.
        assert!(seen.len() > 490, "só {} PIN distintos em 500", seen.len());
    }

    #[test]
    fn um_pin_escolhido_passa_pelas_mesmas_recusas() {
        assert!(check_chosen_pin("482913", "1234").is_ok());
        for bad in ["111111", "123456", "654321", "001234", "12345", "abcdef"] {
            assert!(check_chosen_pin(bad, "1234").is_err(), "{bad}");
        }
    }
}
