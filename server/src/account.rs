//! «A minha conta» (Navegavel3, `DelonixProfile` e `DelonixTour`) — o perfil,
//! a fotografia, as preferências de entrada nas sessões, as preferências de
//! notificação e o progresso do guia. As regras estão no domínio
//! (`identity::{profile, join_preferences, tour}`, `notification::preferences`);
//! aqui só se lê, valida pela regra e grava.
//!
//! Contrato (BFF, sessão, SEMPRE a própria conta — não há `{user_id}` para
//! escrever na de outra pessoa):
//! - `GET/PATCH  /api/users/me/profile`
//! - `GET/PUT/DELETE /api/users/me/avatar`, e `GET /api/users/{user_id}/avatar`
//!   (só quem partilha uma organização activa; senão `404`)
//! - `GET/PUT    /api/users/me/join-preferences`
//! - `GET/PUT    /api/users/me/notification-preferences`
//! - `GET/PATCH  /api/users/me/tour`, `PUT /api/users/me/tour/steps/{step_id}`,
//!   `POST /api/users/me/tour/skip`, `POST /api/users/me/tour/restart`

use axum::{
    body::Bytes,
    extract::{Path, State},
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use chrono::{DateTime, Utc};
use delonix_meet_core::DomainError;
use delonix_meet_domain::identity::{join_preferences as jp, profile as rules, tour};
use delonix_meet_domain::notification::{preferences as np, Kind};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use uuid::Uuid;

use crate::{auth::AuthUser, error::ApiError, AppState};

// ---------------------------------------------------------------------------
//  Perfil
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct ProfileOrganization {
    pub id: Uuid,
    pub name: String,
    /// Papel na organização (`admin` | `member` | …).
    pub role: String,
    pub joined_at: DateTime<Utc>,
}

/// De onde vêm os campos só de leitura.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct ManagedBy {
    /// Sempre `odoo`.
    pub source: String,
    /// Campos que só mudam no Odoo: `legal_name`, `email`, `department`.
    pub fields: Vec<String>,
    /// «Alterações do Odoo chegam aqui em até N minutos».
    pub sync_interval_minutes: u32,
    /// Última sincronização do directório da organização.
    pub last_synced_at: Option<DateTime<Utc>>,
    /// Para o botão «Abrir no Odoo».
    pub odoo_url: Option<String>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct PasswordStatus {
    /// A password é a do Odoo: não se altera aqui.
    pub managed_by_odoo: bool,
    /// Última alteração conhecida. `null` = desconhecida (conta anterior a
    /// este registo, ou gerida pelo Odoo).
    pub changed_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct Profile {
    pub id: Uuid,
    pub email: String,
    pub username: String,
    /// O nome mostrado na sala e nas legendas (o `username` se não houver um).
    pub display_name: String,
    /// `true` se a pessoa escolheu um nome a mostrar.
    pub display_name_set: bool,
    /// Nome legal (do Odoo numa conta gerida; `null` se desconhecido).
    pub legal_name: Option<String>,
    /// Departamento. `null` até a frente de utilizadores e departamentos
    /// (`org_members.department_id`) estar integrada.
    pub department: Option<String>,
    /// Cargo na organização principal.
    pub job_title: String,
    /// Telefone em E.164 (`+244923447108`), da organização principal.
    pub phone: Option<String>,
    /// `manual` (escrito pela pessoa) | `odoo` (do directório) | `null`.
    pub phone_source: Option<String>,
    /// Nome IANA.
    pub timezone: String,
    /// Deslocamento actual do fuso, em minutos (60 = UTC+1).
    pub timezone_utc_offset_minutes: Option<i32>,
    /// `pt-AO` | `en` | `fr-FR` | `zh-CN` (e os antigos `pt` | `fr`).
    pub locale: String,
    /// Caminho da fotografia (`null` sem fotografia). Muda quando a foto muda.
    pub avatar_url: Option<String>,
    /// Organização principal (a pertença activa mais antiga).
    pub organization: Option<ProfileOrganization>,
    /// Reuniões agendadas por esta pessoa («sessões conduzidas»).
    pub meetings_hosted: i64,
    /// Presente numa conta gerida pelo Odoo.
    pub managed_by: Option<ManagedBy>,
    pub password: PasswordStatus,
    pub created_at: DateTime<Utc>,
    /// Última alteração do perfil («alterações guardadas há N min»).
    pub updated_at: Option<DateTime<Utc>>,
}

#[derive(sqlx::FromRow)]
struct ProfileRow {
    id: Uuid,
    email: String,
    username: String,
    display_name: Option<String>,
    legal_name: Option<String>,
    timezone: String,
    locale: String,
    password_changed_at: Option<DateTime<Utc>>,
    profile_updated_at: Option<DateTime<Utc>>,
    created_at: DateTime<Utc>,
    odoo_managed: bool,
    odoo_url: Option<String>,
    odoo_synced_at: Option<DateTime<Utc>>,
    avatar_at: Option<DateTime<Utc>>,
    meetings_hosted: i64,
}

async fn load_row(state: &AppState, user_id: Uuid) -> Result<ProfileRow, ApiError> {
    sqlx::query_as(
        "SELECT u.id, u.email, u.username, u.display_name, u.legal_name, u.timezone,
                COALESCE(u.locale, 'pt') AS locale, u.password_changed_at, u.profile_updated_at,
                u.created_at,
                (u.odoo_managed AND u.odoo_org_id IS NOT NULL AND COALESCE(o.odoo_enabled, FALSE)) AS odoo_managed,
                o.odoo_url, o.odoo_synced_at,
                a.updated_at AS avatar_at,
                (SELECT COUNT(*) FROM meetings mt WHERE mt.owner_id = u.id) AS meetings_hosted
           FROM users u
           LEFT JOIN organizations o ON o.id = u.odoo_org_id
           LEFT JOIN user_avatars a ON a.user_id = u.id
          WHERE u.id = $1",
    )
    .bind(user_id)
    .fetch_optional(&state.db)
    .await?
    .ok_or(ApiError::Unauthorized)
}

fn avatar_url(user_id: Uuid, at: Option<DateTime<Utc>>) -> Option<String> {
    at.map(|t| format!("/api/users/{user_id}/avatar?v={}", t.timestamp_millis()))
}

pub(crate) async fn build_profile(state: &AppState, user_id: Uuid) -> Result<Profile, ApiError> {
    let r = load_row(state, user_id).await?;
    let m = crate::org::primary_membership(state, user_id).await?;
    let managed_by = r.odoo_managed.then(|| ManagedBy {
        source: "odoo".into(),
        fields: rules::ManagedField::ALL
            .iter()
            .map(|f| f.as_str().to_string())
            .collect(),
        sync_interval_minutes: rules::ODOO_SYNC_INTERVAL_MINUTES,
        last_synced_at: r.odoo_synced_at,
        odoo_url: r.odoo_url.clone(),
    });
    Ok(Profile {
        id: r.id,
        display_name: r.display_name.clone().unwrap_or_else(|| r.username.clone()),
        display_name_set: r.display_name.is_some(),
        email: r.email,
        username: r.username,
        legal_name: r.legal_name,
        department: None,
        job_title: m.as_ref().map(|m| m.title.clone()).unwrap_or_default(),
        phone: m.as_ref().and_then(|m| m.phone_e164.clone()),
        phone_source: m.as_ref().and_then(|m| m.phone_source.clone()),
        timezone_utc_offset_minutes: rules::utc_offset_minutes(&r.timezone, Utc::now()),
        timezone: r.timezone,
        locale: r.locale,
        avatar_url: avatar_url(r.id, r.avatar_at),
        organization: m.map(|m| ProfileOrganization {
            id: m.org_id,
            name: m.org_name,
            role: m.role,
            joined_at: m.joined_at,
        }),
        meetings_hosted: r.meetings_hosted,
        managed_by,
        password: PasswordStatus {
            managed_by_odoo: r.odoo_managed,
            changed_at: if r.odoo_managed {
                None
            } else {
                r.password_changed_at
            },
        },
        created_at: r.created_at,
        updated_at: r.profile_updated_at,
    })
}

/// Uma conta é gerida por um Odoo activo? (a mesma leitura que o perfil mostra)
pub(crate) async fn is_odoo_managed(state: &AppState, user_id: Uuid) -> Result<bool, ApiError> {
    Ok(sqlx::query_scalar(
        "SELECT (u.odoo_managed AND u.odoo_org_id IS NOT NULL AND COALESCE(o.odoo_enabled, FALSE))
           FROM users u LEFT JOIN organizations o ON o.id = u.odoo_org_id WHERE u.id = $1",
    )
    .bind(user_id)
    .fetch_optional(&state.db)
    .await?
    .unwrap_or(false))
}

/// Alteração do perfil. Campo omisso = não muda. Um campo desconhecido é
/// `422`; um campo só de leitura (`legal_name`, `email`, `department`) é `409`.
#[derive(Debug, Deserialize, Default, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdateProfileReq {
    /// 1–80 caracteres; `""` volta a usar o `username`.
    pub display_name: Option<String>,
    /// 0–100 caracteres (`""` apaga). Precisa de organização.
    pub job_title: Option<String>,
    /// Móvel angolano, normalizado para E.164 como no gateway de SMS
    /// (`923 447 108`, `+244 923-447-108`); `""` apaga. Precisa de organização.
    pub phone: Option<String>,
    /// Nome IANA (`Africa/Luanda`).
    pub timezone: Option<String>,
    /// `pt-AO` | `en` | `fr-FR` | `zh-CN` (e os antigos `pt` | `fr`).
    pub locale: Option<String>,
    /// Só leitura: presente ⇒ `409`.
    #[schema(value_type = Option<String>)]
    pub legal_name: Option<serde_json::Value>,
    /// Só leitura: presente ⇒ `409`.
    #[schema(value_type = Option<String>)]
    pub email: Option<serde_json::Value>,
    /// Só leitura: presente ⇒ `409`.
    #[schema(value_type = Option<String>)]
    pub department: Option<serde_json::Value>,
}

#[derive(utoipa::OpenApi)]
#[openapi(
    paths(
        get_profile,
        update_profile,
        get_my_avatar,
        put_avatar,
        delete_avatar,
        get_avatar,
        get_join_preferences,
        put_join_preferences,
        get_notification_preferences,
        put_notification_preferences,
        get_tour,
        update_tour,
        put_tour_step,
        skip_tour,
        restart_tour
    ),
    components(schemas(
        Profile,
        ProfileOrganization,
        ManagedBy,
        PasswordStatus,
        UpdateProfileReq,
        JoinPreferencesBody,
        NotificationPreferences,
        NotificationChannelStatus,
        NotificationKindPreferences,
        NotificationPreferenceItem,
        PutNotificationPreferencesReq,
        TourState,
        TourStep,
        UpdateTourReq,
        TourStepReq
    ))
)]
pub struct ApiDoc;

/// O meu perfil: dados editáveis, só de leitura (com a origem) e estado da
/// password.
#[utoipa::path(
    get, path = "/api/users/me/profile", tag = "account",
    security(("session" = [])),
    responses(
        (status = 200, body = Profile),
        (status = 401, body = crate::openapi::ErrorBody),
    )
)]
pub async fn get_profile(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
) -> Result<Json<Profile>, ApiError> {
    Ok(Json(build_profile(&state, auth.user_id).await?))
}

/// Altera o meu perfil. Valida TUDO antes de escrever: um campo inválido não
/// deixa os outros meio gravados.
#[utoipa::path(
    patch, path = "/api/users/me/profile", tag = "account",
    security(("session" = [])),
    request_body = UpdateProfileReq,
    responses(
        (status = 200, body = Profile),
        (status = 400, description = "`profile.invalid_display_name`, `profile.invalid_job_title`, `profile.invalid_timezone`, `profile.invalid_locale`.", body = crate::openapi::ErrorBody),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 409, description = "Campo só de leitura: `profile.field_managed_by_odoo` (conta gerida pelo Odoo — alterar lá) ou `profile.field_read_only`; ou `profile.organization_required` (cargo/telefone sem organização).", body = crate::openapi::ErrorBody),
        (status = 422, description = "Telefone que não é um móvel angolano (`profile.invalid_phone`), ou corpo com campo desconhecido.", body = crate::openapi::ErrorBody),
    )
)]
pub async fn update_profile(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Json(req): Json<UpdateProfileReq>,
) -> Result<Json<Profile>, ApiError> {
    use rules::ManagedField;
    let attempted: Vec<ManagedField> = [
        (req.legal_name.is_some(), ManagedField::LegalName),
        (req.email.is_some(), ManagedField::Email),
        (req.department.is_some(), ManagedField::Department),
    ]
    .into_iter()
    .filter_map(|(present, f)| present.then_some(f))
    .collect();
    if !attempted.is_empty() {
        rules::check_not_managed(is_odoo_managed(&state, auth.user_id).await?, &attempted)?;
    }

    // 1. validar tudo
    let display_name: Option<Option<String>> = match req.display_name.as_deref() {
        None => None,
        Some(s) if s.trim().is_empty() => Some(None),
        Some(s) => Some(Some(rules::validate_display_name(s)?)),
    };
    let job_title = req
        .job_title
        .as_deref()
        .map(rules::validate_job_title)
        .transpose()?;
    let phone: Option<Option<String>> = match req.phone.as_deref() {
        None => None,
        Some(s) if s.trim().is_empty() => Some(None),
        Some(s) => Some(Some(crate::sms::normalize_msisdn(s).map_err(|_| {
            DomainError::new(
                delonix_meet_core::ErrorKind::FailedPrecondition,
                "profile.invalid_phone",
                "nesta fase só números móveis angolanos: +244 9XX XXX XXX",
            )
            .with_field("phone", "+244 9XX XXX XXX")
        })?)),
    };
    let timezone = req
        .timezone
        .as_deref()
        .map(rules::validate_timezone)
        .transpose()?;
    let locale = req
        .locale
        .as_deref()
        .map(rules::canonical_locale)
        .transpose()?;
    let membership = if job_title.is_some() || phone.is_some() {
        Some(
            crate::org::primary_membership(&state, auth.user_id)
                .await?
                .ok_or_else(|| {
                    DomainError::conflict(
                        "profile.organization_required",
                        "o cargo e o telefone são da organização: esta conta não pertence a nenhuma",
                    )
                })?,
        )
    } else {
        None
    };

    // 2. escrever
    let mut changed: Vec<&str> = Vec::new();
    let mut tx = state.db.begin().await?;
    if let Some(v) = &display_name {
        sqlx::query("UPDATE users SET display_name = $2 WHERE id = $1")
            .bind(auth.user_id)
            .bind(v)
            .execute(&mut *tx)
            .await?;
        changed.push("display_name");
    }
    if let Some(v) = &timezone {
        sqlx::query("UPDATE users SET timezone = $2 WHERE id = $1")
            .bind(auth.user_id)
            .bind(v)
            .execute(&mut *tx)
            .await?;
        changed.push("timezone");
    }
    if let Some(v) = locale {
        sqlx::query("UPDATE users SET locale = $2 WHERE id = $1")
            .bind(auth.user_id)
            .bind(v)
            .execute(&mut *tx)
            .await?;
        changed.push("locale");
    }
    if let Some(m) = &membership {
        crate::org::set_own_contact(
            &mut tx,
            m.org_id,
            auth.user_id,
            job_title.as_deref(),
            phone.as_ref().map(|p| p.as_deref()),
        )
        .await?;
        if job_title.is_some() {
            changed.push("job_title");
        }
        if phone.is_some() {
            changed.push("phone");
        }
    }
    if !changed.is_empty() {
        sqlx::query("UPDATE users SET profile_updated_at = now() WHERE id = $1")
            .bind(auth.user_id)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    if !changed.is_empty() {
        // O alvo é a lista de campos, não os valores: o telefone é dado pessoal
        // e a trilha é lida por administradores.
        crate::audit::log(
            &state.db,
            membership.as_ref().map(|m| m.org_id),
            auth.user_id,
            "profile.updated",
            &changed.join(","),
        )
        .await;
    }
    Ok(Json(build_profile(&state, auth.user_id).await?))
}

// ---------------------------------------------------------------------------
//  Fotografia
// ---------------------------------------------------------------------------

/// Limite de corpo da rota da fotografia: acima do tecto do domínio, para o
/// domínio responder `422 profile.avatar_too_large` com código estável em vez
/// de o axum cortar com um 413 genérico.
pub const AVATAR_BODY_LIMIT: usize = 2 * rules::AVATAR_MAX_BYTES;

async fn avatar_response(state: &AppState, user_id: Uuid) -> Result<Response, ApiError> {
    let row: Option<(String, Vec<u8>)> =
        sqlx::query_as("SELECT content_type, bytes FROM user_avatars WHERE user_id = $1")
            .bind(user_id)
            .fetch_optional(&state.db)
            .await?;
    let (ct, bytes) = row.ok_or_else(|| DomainError::not_found("avatar.not_found"))?;
    Ok((
        [
            (header::CONTENT_TYPE, ct),
            // Privada: não fica em caches partilhadas. O `?v=` do URL muda
            // quando a foto muda, por isso uns minutos de cache chegam.
            (header::CACHE_CONTROL, "private, max-age=300".to_string()),
            (header::CONTENT_DISPOSITION, "inline".to_string()),
        ],
        bytes,
    )
        .into_response())
}

/// A minha fotografia.
#[utoipa::path(
    get, path = "/api/users/me/avatar", tag = "account",
    security(("session" = [])),
    responses(
        (status = 200, description = "A imagem (`image/png`, `image/jpeg` ou `image/webp`)."),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 404, description = "Sem fotografia (`avatar.not_found`).", body = crate::openapi::ErrorBody),
    )
)]
pub async fn get_my_avatar(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
) -> Result<Response, ApiError> {
    avatar_response(&state, auth.user_id).await
}

/// A fotografia de alguém com quem partilho uma organização activa (ou a
/// minha). De outra pessoa qualquer: `404`, como se não existisse.
#[utoipa::path(
    get, path = "/api/users/{user_id}/avatar", tag = "account",
    security(("session" = [])),
    params(("user_id" = Uuid, Path)),
    responses(
        (status = 200, description = "A imagem."),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 404, description = "Sem fotografia, ou pessoa fora das minhas organizações (`avatar.not_found`).", body = crate::openapi::ErrorBody),
    )
)]
pub async fn get_avatar(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(user_id): Path<Uuid>,
) -> Result<Response, ApiError> {
    if !crate::org::shares_active_org(&state, auth.user_id, user_id).await? {
        return Err(DomainError::not_found("avatar.not_found").into());
    }
    avatar_response(&state, user_id).await
}

/// Muda a fotografia. O corpo é a imagem crua (PNG, JPEG ou WebP, até 1 MiB);
/// o tipo decide-se pelos bytes, não pelo `Content-Type`.
#[utoipa::path(
    put, path = "/api/users/me/avatar", tag = "account",
    security(("session" = [])),
    request_body(content = Vec<u8>, content_type = "image/png", description = "PNG, JPEG ou WebP, até 1 MiB."),
    responses(
        (status = 200, body = Profile),
        (status = 400, description = "Corpo vazio (`profile.avatar_empty`).", body = crate::openapi::ErrorBody),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 413, description = "Acima de 2 MiB (corte do servidor).", body = crate::openapi::ErrorBody),
        (status = 422, description = "`profile.avatar_too_large` ou `profile.avatar_unsupported_type`.", body = crate::openapi::ErrorBody),
    )
)]
pub async fn put_avatar(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    body: Bytes,
) -> Result<Json<Profile>, ApiError> {
    let kind = rules::sniff_avatar(&body)?;
    sqlx::query(
        "INSERT INTO user_avatars (user_id, content_type, bytes, updated_at)
         VALUES ($1, $2, $3, now())
         ON CONFLICT (user_id) DO UPDATE
            SET content_type = EXCLUDED.content_type, bytes = EXCLUDED.bytes, updated_at = now()",
    )
    .bind(auth.user_id)
    .bind(kind.mime())
    .bind(body.as_ref())
    .execute(&state.db)
    .await?;
    sqlx::query("UPDATE users SET profile_updated_at = now() WHERE id = $1")
        .bind(auth.user_id)
        .execute(&state.db)
        .await?;
    crate::audit::log(
        &state.db,
        None,
        auth.user_id,
        "profile.avatar_updated",
        kind.mime(),
    )
    .await;
    Ok(Json(build_profile(&state, auth.user_id).await?))
}

/// Remove a fotografia.
#[utoipa::path(
    delete, path = "/api/users/me/avatar", tag = "account",
    security(("session" = [])),
    responses(
        (status = 204, description = "Removida."),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 404, description = "Não havia fotografia (`avatar.not_found`).", body = crate::openapi::ErrorBody),
    )
)]
pub async fn delete_avatar(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
) -> Result<StatusCode, ApiError> {
    let n = sqlx::query("DELETE FROM user_avatars WHERE user_id = $1")
        .bind(auth.user_id)
        .execute(&state.db)
        .await?
        .rows_affected();
    if n == 0 {
        return Err(DomainError::not_found("avatar.not_found").into());
    }
    sqlx::query("UPDATE users SET profile_updated_at = now() WHERE id = $1")
        .bind(auth.user_id)
        .execute(&state.db)
        .await?;
    crate::audit::log(&state.db, None, auth.user_id, "profile.avatar_removed", "").await;
    Ok(StatusCode::NO_CONTENT)
}

// ---------------------------------------------------------------------------
//  «Como entro nas sessões»
// ---------------------------------------------------------------------------

/// Preferências de entrada. No `PUT` é a representação COMPLETA: um campo
/// omisso volta à omissão (desligado).
#[derive(Debug, Serialize, Deserialize, Default, utoipa::ToSchema)]
#[serde(deny_unknown_fields, default)]
pub struct JoinPreferencesBody {
    /// Entrar com o som desligado.
    pub join_muted: bool,
    /// Entrar com a câmara desligada.
    pub join_camera_off: bool,
    /// Fundo desfocado por omissão.
    pub blur_background: bool,
    /// Supressão de ruído.
    pub noise_suppression: bool,
    /// Legendas sempre visíveis.
    pub captions_always_on: bool,
    /// Idioma das legendas (`pt`, `en`, `fr`, `zh`, `es`, `ln`, `kg`, `umb`);
    /// `null` = o da sessão.
    pub captions_language: Option<String>,
    /// Avisar antes de gravar: IMPOSTO pelo servidor — como anfitrião, o
    /// início de uma gravação exige `confirmed: true` no `server_record`.
    pub warn_before_recording: bool,
}

impl From<jp::JoinPreferences> for JoinPreferencesBody {
    fn from(p: jp::JoinPreferences) -> Self {
        Self {
            join_muted: p.join_muted,
            join_camera_off: p.join_camera_off,
            blur_background: p.blur_background,
            noise_suppression: p.noise_suppression,
            captions_always_on: p.captions_always_on,
            captions_language: p.captions_language,
            warn_before_recording: p.warn_before_recording,
        }
    }
}

/// Colunas de `user_join_preferences`, pela ordem do SELECT.
type JoinPreferencesRow = (bool, bool, bool, bool, bool, Option<String>, bool);

/// As preferências de entrada de `user_id` (as omissões se nunca as mudou).
pub(crate) async fn load_join_preferences(
    db: &sqlx::PgPool,
    user_id: Uuid,
) -> Result<jp::JoinPreferences, sqlx::Error> {
    let row: Option<JoinPreferencesRow> = sqlx::query_as(
        "SELECT join_muted, join_camera_off, blur_background, noise_suppression,
                captions_always_on, captions_language, warn_before_recording
           FROM user_join_preferences WHERE user_id = $1",
    )
    .bind(user_id)
    .fetch_optional(db)
    .await?;
    Ok(match row {
        None => jp::JoinPreferences::default(),
        Some((a, b, c, d, e, f, g)) => jp::JoinPreferences {
            join_muted: a,
            join_camera_off: b,
            blur_background: c,
            noise_suppression: d,
            captions_always_on: e,
            captions_language: f,
            warn_before_recording: g,
        },
    })
}

/// As minhas preferências de entrada nas sessões (valem para todas as salas;
/// também vêm no `POST /api/rooms/{room_code}/join`).
#[utoipa::path(
    get, path = "/api/users/me/join-preferences", tag = "account",
    security(("session" = [])),
    responses(
        (status = 200, body = JoinPreferencesBody),
        (status = 401, body = crate::openapi::ErrorBody),
    )
)]
pub async fn get_join_preferences(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
) -> Result<Json<JoinPreferencesBody>, ApiError> {
    Ok(Json(
        load_join_preferences(&state.db, auth.user_id).await?.into(),
    ))
}

/// Substitui as preferências de entrada.
#[utoipa::path(
    put, path = "/api/users/me/join-preferences", tag = "account",
    security(("session" = [])),
    request_body = JoinPreferencesBody,
    responses(
        (status = 200, body = JoinPreferencesBody),
        (status = 400, description = "`join_preferences.invalid_captions_language`.", body = crate::openapi::ErrorBody),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 422, description = "Campo desconhecido.", body = crate::openapi::ErrorBody),
    )
)]
pub async fn put_join_preferences(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Json(req): Json<JoinPreferencesBody>,
) -> Result<Json<JoinPreferencesBody>, ApiError> {
    let captions_language = jp::validate_captions_language(req.captions_language.as_deref())?;
    sqlx::query(
        "INSERT INTO user_join_preferences (user_id, join_muted, join_camera_off, blur_background,
             noise_suppression, captions_always_on, captions_language, warn_before_recording, updated_at)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, now())
         ON CONFLICT (user_id) DO UPDATE SET
             join_muted = EXCLUDED.join_muted, join_camera_off = EXCLUDED.join_camera_off,
             blur_background = EXCLUDED.blur_background, noise_suppression = EXCLUDED.noise_suppression,
             captions_always_on = EXCLUDED.captions_always_on,
             captions_language = EXCLUDED.captions_language,
             warn_before_recording = EXCLUDED.warn_before_recording, updated_at = now()",
    )
    .bind(auth.user_id)
    .bind(req.join_muted)
    .bind(req.join_camera_off)
    .bind(req.blur_background)
    .bind(req.noise_suppression)
    .bind(req.captions_always_on)
    .bind(&captions_language)
    .bind(req.warn_before_recording)
    .execute(&state.db)
    .await?;
    Ok(Json(
        load_join_preferences(&state.db, auth.user_id).await?.into(),
    ))
}

// ---------------------------------------------------------------------------
//  Preferências de notificação
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct NotificationChannelStatus {
    /// `email` | `in_app` | `sms`.
    pub channel: String,
    /// `available` | `not_configured` — só o `in_app` entrega hoje.
    pub delivery: String,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct NotificationKindPreferences {
    /// Tipo do centro de notificações (`meeting.starting`, …).
    pub kind: String,
    pub email: bool,
    pub in_app: bool,
    pub sms: bool,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct NotificationPreferences {
    pub channels: Vec<NotificationChannelStatus>,
    pub items: Vec<NotificationKindPreferences>,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct NotificationPreferenceItem {
    pub kind: String,
    /// `email` | `in_app` | `sms`.
    pub channel: String,
    pub enabled: bool,
}

/// A matriz COMPLETA: os pares omissos voltam à omissão.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct PutNotificationPreferencesReq {
    pub preferences: Vec<NotificationPreferenceItem>,
}

pub(crate) async fn load_notification_preferences(
    state: &AppState,
    user_id: Uuid,
) -> Result<NotificationPreferences, ApiError> {
    let rows: Vec<(String, String, bool)> = sqlx::query_as(
        "SELECT kind, channel, enabled FROM user_notification_preferences WHERE user_id = $1",
    )
    .bind(user_id)
    .fetch_all(&state.db)
    .await?;
    let get = |k: Kind, c: np::Channel| {
        rows.iter()
            .find(|(rk, rc, _)| rk == k.as_str() && rc == c.as_str())
            .map(|(_, _, e)| *e)
            .unwrap_or_else(|| np::default_enabled(k, c))
    };
    Ok(NotificationPreferences {
        channels: np::Channel::ALL
            .iter()
            .map(|c| NotificationChannelStatus {
                channel: c.as_str().into(),
                delivery: np::delivery(*c).as_str().into(),
            })
            .collect(),
        items: Kind::ALL
            .iter()
            .map(|k| NotificationKindPreferences {
                kind: k.as_str().into(),
                email: get(*k, np::Channel::Email),
                in_app: get(*k, np::Channel::InApp),
                sms: get(*k, np::Channel::Sms),
            })
            .collect(),
    })
}

/// O canal `in_app` está ligado para este tipo? (o produtor pergunta antes de
/// criar a notificação)
pub(crate) async fn in_app_enabled(db: &sqlx::PgPool, user_id: Uuid, kind: Kind) -> bool {
    sqlx::query_scalar::<_, bool>(
        "SELECT enabled FROM user_notification_preferences
          WHERE user_id = $1 AND kind = $2 AND channel = 'in_app'",
    )
    .bind(user_id)
    .bind(kind.as_str())
    .fetch_optional(db)
    .await
    .ok()
    .flatten()
    .unwrap_or_else(|| np::default_enabled(kind, np::Channel::InApp))
}

/// As minhas preferências de notificação, por tipo e canal, e o estado de
/// entrega de cada canal nesta instalação.
#[utoipa::path(
    get, path = "/api/users/me/notification-preferences", tag = "account",
    security(("session" = [])),
    responses(
        (status = 200, body = NotificationPreferences),
        (status = 401, body = crate::openapi::ErrorBody),
    )
)]
pub async fn get_notification_preferences(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
) -> Result<Json<NotificationPreferences>, ApiError> {
    Ok(Json(
        load_notification_preferences(&state, auth.user_id).await?,
    ))
}

/// Substitui as preferências de notificação. Guardar para um canal sem entrega
/// (`not_configured`) é aceite — fica a vontade da pessoa — mas nada é enviado
/// por ele enquanto não houver adaptador.
#[utoipa::path(
    put, path = "/api/users/me/notification-preferences", tag = "account",
    security(("session" = [])),
    request_body = PutNotificationPreferencesReq,
    responses(
        (status = 200, body = NotificationPreferences),
        (status = 400, description = "`notification_preferences.unknown_kind`, `notification_preferences.unknown_channel` ou `notification_preferences.duplicate`.", body = crate::openapi::ErrorBody),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 422, description = "Campo desconhecido.", body = crate::openapi::ErrorBody),
    )
)]
pub async fn put_notification_preferences(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Json(req): Json<PutNotificationPreferencesReq>,
) -> Result<Json<NotificationPreferences>, ApiError> {
    let mut pairs = Vec::with_capacity(req.preferences.len());
    for item in &req.preferences {
        let (k, c) = np::parse_pair(&item.kind, &item.channel)?;
        if pairs.iter().any(|(pk, pc, _)| *pk == k && *pc == c) {
            return Err(DomainError::invalid(
                "notification_preferences.duplicate",
                format!("{} / {} aparece duas vezes", item.kind, item.channel),
            )
            .into());
        }
        pairs.push((k, c, item.enabled));
    }
    let mut tx = state.db.begin().await?;
    sqlx::query("DELETE FROM user_notification_preferences WHERE user_id = $1")
        .bind(auth.user_id)
        .execute(&mut *tx)
        .await?;
    for (k, c, enabled) in pairs {
        if enabled == np::default_enabled(k, c) {
            continue; // só se guarda o que difere da omissão
        }
        sqlx::query(
            "INSERT INTO user_notification_preferences (user_id, kind, channel, enabled)
             VALUES ($1, $2, $3, $4)",
        )
        .bind(auth.user_id)
        .bind(k.as_str())
        .bind(c.as_str())
        .bind(enabled)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    Ok(Json(
        load_notification_preferences(&state, auth.user_id).await?,
    ))
}

// ---------------------------------------------------------------------------
//  Guia (Tour)
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct TourStep {
    pub id: String,
    pub completed: bool,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct TourState {
    /// Versão da lista de passos (o conteúdo é do cliente).
    pub version: String,
    /// «Guia activo».
    pub enabled: bool,
    /// Os passos, por ordem.
    pub steps: Vec<TourStep>,
    /// «N de M passos concluídos».
    pub completed_count: usize,
    pub total: usize,
    /// O primeiro passo por concluir (`null` quando está completo).
    pub next_step: Option<String>,
    /// Todos concluídos.
    pub completed: bool,
    /// Quando o guia foi saltado (`null` se não foi).
    pub skipped_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdateTourReq {
    /// Liga/desliga o guia (também em Preferências).
    pub enabled: bool,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct TourStepReq {
    pub completed: bool,
}

pub(crate) async fn load_tour(db: &sqlx::PgPool, user_id: Uuid) -> Result<TourState, ApiError> {
    let row: Option<(bool, Vec<String>, Option<DateTime<Utc>>)> = sqlx::query_as(
        "SELECT enabled, completed_steps, skipped_at FROM user_tour_state WHERE user_id = $1",
    )
    .bind(user_id)
    .fetch_optional(db)
    .await?;
    let (enabled, stored, skipped_at) = row.unwrap_or((true, Vec::new(), None));
    let done = tour::completed_in_current(stored.iter().map(String::as_str));
    Ok(TourState {
        version: tour::VERSION.into(),
        enabled,
        steps: tour::STEPS
            .iter()
            .map(|s| TourStep {
                id: s.to_string(),
                completed: done.contains(s),
            })
            .collect(),
        completed_count: done.len(),
        total: tour::STEPS.len(),
        next_step: tour::next_step(&done).map(str::to_string),
        completed: done.len() == tour::STEPS.len(),
        skipped_at,
    })
}

/// Garante a linha do guia (com a versão actual) dentro da transacção.
const TOUR_UPSERT: &str = "INSERT INTO user_tour_state (user_id, version) VALUES ($1, $2)
                           ON CONFLICT (user_id) DO NOTHING";

/// O meu progresso no guia.
#[utoipa::path(
    get, path = "/api/users/me/tour", tag = "account",
    security(("session" = [])),
    responses(
        (status = 200, body = TourState),
        (status = 401, body = crate::openapi::ErrorBody),
    )
)]
pub async fn get_tour(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
) -> Result<Json<TourState>, ApiError> {
    Ok(Json(load_tour(&state.db, auth.user_id).await?))
}

/// Liga ou desliga o guia.
#[utoipa::path(
    patch, path = "/api/users/me/tour", tag = "account",
    security(("session" = [])),
    request_body = UpdateTourReq,
    responses(
        (status = 200, body = TourState),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 422, description = "Corpo inválido ou campo desconhecido.", body = crate::openapi::ErrorBody),
    )
)]
pub async fn update_tour(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Json(req): Json<UpdateTourReq>,
) -> Result<Json<TourState>, ApiError> {
    sqlx::query(
        "INSERT INTO user_tour_state (user_id, version, enabled) VALUES ($1, $2, $3)
         ON CONFLICT (user_id) DO UPDATE SET enabled = EXCLUDED.enabled, updated_at = now()",
    )
    .bind(auth.user_id)
    .bind(tour::VERSION)
    .bind(req.enabled)
    .execute(&state.db)
    .await?;
    Ok(Json(load_tour(&state.db, auth.user_id).await?))
}

/// Marca (ou desmarca) um passo. O id é validado contra a lista versionada.
/// Idempotente: marcar duas vezes é o mesmo que uma.
#[utoipa::path(
    put, path = "/api/users/me/tour/steps/{step_id}", tag = "account",
    security(("session" = [])),
    params(("step_id" = String, Path, description = "Id do passo (`home.start-now`, …).")),
    request_body = TourStepReq,
    responses(
        (status = 200, body = TourState),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 404, description = "Passo fora da versão actual (`tour.unknown_step`).", body = crate::openapi::ErrorBody),
    )
)]
pub async fn put_tour_step(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(step_id): Path<String>,
    Json(req): Json<TourStepReq>,
) -> Result<Json<TourState>, ApiError> {
    let step = tour::validate_step(&step_id)?;
    let mut tx = state.db.begin().await?;
    sqlx::query(TOUR_UPSERT)
        .bind(auth.user_id)
        .bind(tour::VERSION)
        .execute(&mut *tx)
        .await?;
    let sql = if req.completed {
        "UPDATE user_tour_state
            SET completed_steps = CASE WHEN $2 = ANY(completed_steps) THEN completed_steps
                                       ELSE array_append(completed_steps, $2) END,
                version = $3, updated_at = now()
          WHERE user_id = $1"
    } else {
        "UPDATE user_tour_state SET completed_steps = array_remove(completed_steps, $2),
                version = $3, updated_at = now()
          WHERE user_id = $1"
    };
    sqlx::query(sql)
        .bind(auth.user_id)
        .bind(step)
        .bind(tour::VERSION)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(Json(load_tour(&state.db, auth.user_id).await?))
}

/// Método personalizado: salta o guia (fica desligado, com a data).
#[utoipa::path(
    post, path = "/api/users/me/tour/skip", tag = "account",
    security(("session" = [])),
    responses(
        (status = 200, body = TourState),
        (status = 401, body = crate::openapi::ErrorBody),
    )
)]
pub async fn skip_tour(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
) -> Result<Json<TourState>, ApiError> {
    sqlx::query(
        "INSERT INTO user_tour_state (user_id, version, enabled, skipped_at) VALUES ($1, $2, FALSE, now())
         ON CONFLICT (user_id) DO UPDATE SET enabled = FALSE, skipped_at = now(), updated_at = now()",
    )
    .bind(auth.user_id)
    .bind(tour::VERSION)
    .execute(&state.db)
    .await?;
    Ok(Json(load_tour(&state.db, auth.user_id).await?))
}

/// Método personalizado: recomeça o guia do início (liga-o, limpa os passos).
#[utoipa::path(
    post, path = "/api/users/me/tour/restart", tag = "account",
    security(("session" = [])),
    responses(
        (status = 200, body = TourState),
        (status = 401, body = crate::openapi::ErrorBody),
    )
)]
pub async fn restart_tour(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
) -> Result<Json<TourState>, ApiError> {
    sqlx::query(
        "INSERT INTO user_tour_state (user_id, version, enabled) VALUES ($1, $2, TRUE)
         ON CONFLICT (user_id) DO UPDATE
            SET enabled = TRUE, completed_steps = '{}', skipped_at = NULL,
                version = EXCLUDED.version, updated_at = now()",
    )
    .bind(auth.user_id)
    .bind(tour::VERSION)
    .execute(&state.db)
    .await?;
    Ok(Json(load_tour(&state.db, auth.user_id).await?))
}
