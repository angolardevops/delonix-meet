//! Multi-tenant enterprise: organizações, filiais, employees e grupos.
//!
//! Quem cria uma organização torna-se `admin`. Admins gerem filiais,
//! employees e grupos. Membros veem o diretório e podem iniciar chamadas.

use axum::{
    extract::{Path, State},
    Json,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use uuid::Uuid;

use crate::{auth::AuthUser, error::ApiError, AppState};

#[derive(Debug, Serialize, utoipa::ToSchema, sqlx::FromRow)]
pub struct Organization {
    pub id: Uuid,
    pub name: String,
    pub slug: String,
    pub created_by: Uuid,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, utoipa::ToSchema, sqlx::FromRow)]
pub struct OrgSummary {
    pub id: Uuid,
    pub name: String,
    pub slug: String,
    pub role: String,
    pub member_count: i64,
    #[serde(default)]
    pub domain: String,
    #[serde(default)]
    pub retention_days: i32,
    pub max_groups: Option<i32>,
    pub max_rooms: Option<i32>,
    pub max_meetings: Option<i32>,
}

#[derive(Deserialize, utoipa::ToSchema)]
pub struct OrgSettingsReq {
    #[serde(default)]
    pub domain: String,
    #[serde(default)]
    pub retention_days: i32,
    #[serde(default)]
    pub max_groups: Option<i32>,
    #[serde(default)]
    pub max_rooms: Option<i32>,
    #[serde(default)]
    pub max_meetings: Option<i32>,
    /// Dial-in PSTN: backend de media ('freeswitch' | 'provider').
    #[serde(default)]
    pub voice_media_backend: Option<String>,
    /// Dial-in PSTN: modelo de DID ('shared' | 'dedicated').
    #[serde(default)]
    pub voice_did_model: Option<String>,
}

/// Documentação OpenAPI das rotas deste módulo (`openapi.rs` junta-as).
#[derive(utoipa::OpenApi)]
#[openapi(
    paths(
        update_settings,
        create_org,
        my_orgs,
        create_branch,
        list_branches,
        add_employee,
        list_employees,
        update_employee,
        remove_employee,
        create_group,
        list_groups,
        create_meeting_room,
        list_meeting_rooms,
        org_stats,
        get_sso_config,
        upsert_sso_config,
        delete_sso_config,
    ),
    components(schemas(
        Organization,
        OrgSummary,
        OrgSettingsReq,
        AddEmployeeResp,
        OrgSettingsUpdated,
        Branch,
        Employee,
        Group,
        MeetingRoom,
        OrgStats,
        WeekBucket,
        Organizer,
        SsoConfigPublic,
        SsoConfigReq,
        CreateOrgReq,
        CreateBranchReq,
        AddEmployeeReq,
        UpdateEmployeeReq,
        CreateGroupReq,
        CreateMeetingRoomReq,
    ))
)]
pub struct ApiDoc;

/// Resposta de `POST /api/orgs/{org_id}/settings`: os valores como ficaram
/// gravados (domínio normalizado, retenção limitada a 0–3650).
#[derive(Serialize, utoipa::ToSchema)]
pub struct OrgSettingsUpdated {
    /// Sempre `true`.
    pub ok: bool,
    pub domain: String,
    pub retention_days: i32,
}

/// Definições da organização (só admin): domínio de produção + retenção.
/// O domínio (ex.: `meet.acme.com`) é usado nos links partilháveis; a
/// retenção (>0) apaga gravações mais antigas que N dias. Quotas negativas ou
/// omissas ficam ilimitadas; valores de voz fora do enum são ignorados.
#[utoipa::path(
    post, path = "/api/orgs/{org_id}/settings", tag = "orgs",
    security(("session" = [])),
    params(("org_id" = Uuid, Path, description = "Organização.")),
    request_body = OrgSettingsReq,
    responses(
        (status = 200, body = OrgSettingsUpdated),
        (status = 400, description = "Domínio inválido (>253 caracteres ou com espaços).", body = crate::openapi::ErrorBody),
        (status = 401, description = "Sem sessão.", body = crate::openapi::ErrorBody),
        (status = 403, description = "Membro sem papel de admin.", body = crate::openapi::ErrorBody),
        (status = 404, description = "A organização não existe ou quem pede não é membro activo.", body = crate::openapi::ErrorBody),
    )
)]
pub async fn update_settings(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(org_id): Path<Uuid>,
    Json(req): Json<OrgSettingsReq>,
) -> Result<Json<OrgSettingsUpdated>, ApiError> {
    require_admin(&state, org_id, auth.user_id).await?;
    // Normaliza o domínio: sem esquema, sem barra final, minúsculas.
    let domain = req
        .domain
        .trim()
        .trim_start_matches("https://")
        .trim_start_matches("http://")
        .trim_end_matches('/')
        .to_lowercase();
    if domain.len() > 253 || domain.contains(' ') {
        return Err(ApiError::BadRequest("domínio inválido".into()));
    }
    let retention = req.retention_days.clamp(0, 3650);
    // Quotas: None/negativo => ilimitado (NULL).
    let norm = |v: Option<i32>| v.filter(|n| *n >= 0);
    // Dial-in PSTN: valida os enums (None => mantém o atual via COALESCE).
    let backend = req
        .voice_media_backend
        .as_deref()
        .and_then(|b| matches!(b, "freeswitch" | "provider").then(|| b.to_string()));
    let did_model = req
        .voice_did_model
        .as_deref()
        .and_then(|m| matches!(m, "shared" | "dedicated").then(|| m.to_string()));
    sqlx::query(
        "UPDATE organizations SET domain = $1, retention_days = $2,
             max_groups = $3, max_rooms = $4, max_meetings = $5,
             voice_media_backend = COALESCE($7, voice_media_backend),
             voice_did_model = COALESCE($8, voice_did_model)
         WHERE id = $6",
    )
    .bind(&domain)
    .bind(retention)
    .bind(norm(req.max_groups))
    .bind(norm(req.max_rooms))
    .bind(norm(req.max_meetings))
    .bind(org_id)
    .bind(backend)
    .bind(did_model)
    .execute(&state.db)
    .await?;
    crate::audit::log(
        &state.db,
        Some(org_id),
        auth.user_id,
        "org.settings_updated",
        &domain,
    )
    .await;
    Ok(Json(OrgSettingsUpdated {
        ok: true,
        domain,
        retention_days: retention,
    }))
}

/// Faz cumprir uma quota da organização: se o limite (`limit_col`) estiver
/// definido e a contagem atual em `count_sql` já o atingir, devolve 409.
/// `count_sql` deve contar por `org_id = $1` (grupos/salas) — ver chamadas.
async fn enforce_quota(
    state: &AppState,
    org_id: Uuid,
    limit_col: &str,
    count_sql: &str,
) -> Result<(), ApiError> {
    let limit: Option<i32> = sqlx::query_scalar(&format!(
        "SELECT {limit_col} FROM organizations WHERE id = $1"
    ))
    .bind(org_id)
    .fetch_one(&state.db)
    .await?;
    if let Some(max) = limit {
        let count: i64 = sqlx::query_scalar(count_sql)
            .bind(org_id)
            .fetch_one(&state.db)
            .await?;
        if count >= max as i64 {
            return Err(ApiError::Conflict(format!(
                "limite da organização atingido ({max})"
            )));
        }
    }
    Ok(())
}

#[derive(Debug, Serialize, utoipa::ToSchema, sqlx::FromRow)]
pub struct Branch {
    pub id: Uuid,
    pub org_id: Uuid,
    pub name: String,
    pub location: String,
    pub created_at: DateTime<Utc>,
}

/// Ver ADR-0004 (mesmo padrão de `meetings::MEETING_COLUMNS`): estava copiada
/// à mão em `create_branch` e `list_branches`.
const BRANCH_COLUMNS: &str = "id, org_id, name, location, created_at";

#[derive(Debug, Serialize, utoipa::ToSchema, sqlx::FromRow)]
pub struct Employee {
    pub user_id: Uuid,
    pub username: String,
    pub email: String,
    pub role: String,
    pub title: String,
    pub branch_id: Option<Uuid>,
    pub branch_name: Option<String>,
    /// Último evento de auditoria do membro (login, etc.). Só preenchido na listagem.
    #[sqlx(default)]
    pub last_active: Option<DateTime<Utc>>,
}

/// Sem `last_active` (só a listagem o traz, via subquery à parte). Estava
/// copiada à mão em `add_employee` e `update_employee`; `list_employees`
/// agora compõe a partir daqui em vez de repetir a lista (ADR-0004).
const EMPLOYEE_COLUMNS: &str =
    "m.user_id, u.username, u.email, m.role, m.title, m.branch_id, b.name AS branch_name";

#[derive(Debug, Serialize, utoipa::ToSchema, sqlx::FromRow)]
pub struct Group {
    pub id: Uuid,
    pub org_id: Uuid,
    pub name: String,
    pub member_count: i64,
}

/// Estava copiada à mão em `create_group` e `list_groups` (ADR-0004).
const GROUP_COLUMNS: &str = "g.id, g.org_id, g.name, \
     (SELECT COUNT(*) FROM group_members gm WHERE gm.group_id = g.id) AS member_count";

// ---------- helpers ----------

pub async fn role_in_org(
    state: &AppState,
    org_id: Uuid,
    user_id: Uuid,
) -> Result<Option<String>, ApiError> {
    let row: Option<(String,)> = sqlx::query_as(
        "SELECT role FROM org_members WHERE org_id = $1 AND user_id = $2 AND archived_at IS NULL",
    )
    .bind(org_id)
    .bind(user_id)
    .fetch_optional(&state.db)
    .await?;
    Ok(row.map(|r| r.0))
}

async fn require_admin(state: &AppState, org_id: Uuid, user_id: Uuid) -> Result<(), ApiError> {
    match role_in_org(state, org_id, user_id).await? {
        Some(r) if r == "admin" => Ok(()),
        // Membro sem o papel: 403. O web lê 401 como «a sessão não serve» e
        // gastava um refresh antes de mostrar o erro (R153).
        Some(_) => Err(ApiError::Forbidden),
        None => Err(ApiError::NotFound),
    }
}

/// Igual a `require_admin`, exposto para outros módulos (webhooks, retenção).
pub async fn require_admin_pub(
    state: &AppState,
    org_id: Uuid,
    user_id: Uuid,
) -> Result<(), ApiError> {
    require_admin(state, org_id, user_id).await
}

async fn require_member(state: &AppState, org_id: Uuid, user_id: Uuid) -> Result<(), ApiError> {
    match role_in_org(state, org_id, user_id).await? {
        Some(_) => Ok(()),
        None => Err(ApiError::NotFound),
    }
}

pub async fn require_member_pub(
    state: &AppState,
    org_id: Uuid,
    user_id: Uuid,
) -> Result<(), ApiError> {
    require_member(state, org_id, user_id).await
}

/// Slugify exposto para o registo de organizações (auth.rs).
/// Junta uma conta a uma organização dentro de uma transação já aberta. É o
/// único `INSERT INTO org_members` fora dos handlers deste módulo — quem
/// precisa de o fazer (o registo) chama isto em vez de escrever o SQL.
pub(crate) async fn insert_member_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    org_id: Uuid,
    user_id: Uuid,
    role: &str,
    title: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query("INSERT INTO org_members (org_id, user_id, role, title) VALUES ($1, $2, $3, $4)")
        .bind(org_id)
        .bind(user_id)
        .bind(role)
        .bind(title)
        .execute(&mut **tx)
        .await?;
    Ok(())
}

pub(crate) fn slugify(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    for c in name.to_lowercase().chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c);
        } else if !out.ends_with('-') {
            out.push('-'); // colapsa runs de separadores num só hífen
        }
    }
    let trimmed = out.trim_matches('-').to_string();
    if trimmed.is_empty() {
        "org".into()
    } else {
        trimmed
    }
}

// ---------- organizations ----------

#[derive(Deserialize, utoipa::ToSchema)]
pub struct CreateOrgReq {
    pub name: String,
}

/// Cria uma organização; quem cria entra como `admin`. Máximo de 20
/// organizações criadas por utilizador.
#[utoipa::path(
    post, path = "/api/orgs", tag = "orgs",
    security(("session" = [])),
    request_body = CreateOrgReq,
    responses(
        (status = 200, body = Organization),
        (status = 400, description = "Nome vazio ou com mais de 120 caracteres.", body = crate::openapi::ErrorBody),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 409, description = "Limite de 20 organizações por utilizador atingido.", body = crate::openapi::ErrorBody),
        (status = 422, description = "Instalação em tenancy `single` (`organization.single_tenancy`).", body = crate::openapi::ErrorBody),
    )
)]
pub async fn create_org(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Json(req): Json<CreateOrgReq>,
) -> Result<Json<Organization>, ApiError> {
    // Tenancy `single` (ADR-0006 §2): a instalação tem UMA organização, criada
    // pelo primeiro registo. Criar outra partia a premissa de que toda a gente
    // se encontra no mesmo directório.
    if state.config.tenancy_mode == delonix_meet_core::edition::TenancyMode::Single {
        return Err(delonix_meet_core::DomainError::precondition(
            "organization.single_tenancy",
            "esta instalação tem uma só organização",
        )
        .into());
    }
    let name = req.name.trim();
    if name.is_empty() || name.len() > 120 {
        return Err(ApiError::BadRequest("nome da organização inválido".into()));
    }
    // Backstop anti-abuso: um utilizador não pode criar organizações sem limite.
    let owned: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM organizations WHERE created_by = $1")
        .bind(auth.user_id)
        .fetch_one(&state.db)
        .await?;
    if owned >= 20 {
        return Err(ApiError::Conflict(
            "limite de organizações por utilizador atingido".into(),
        ));
    }
    // slug único com sufixo em colisão.
    let base = slugify(name);
    let mut org: Option<Organization> = None;
    for i in 0..6 {
        let slug = if i == 0 {
            base.clone()
        } else {
            format!("{base}-{i}")
        };
        let res: Result<Organization, sqlx::Error> = sqlx::query_as(
            "INSERT INTO organizations (name, slug, created_by) VALUES ($1, $2, $3)
             RETURNING id, name, slug, created_by, created_at",
        )
        .bind(name)
        .bind(&slug)
        .bind(auth.user_id)
        .fetch_one(&state.db)
        .await;
        match res {
            Ok(o) => {
                org = Some(o);
                break;
            }
            Err(sqlx::Error::Database(db)) if db.is_unique_violation() => continue,
            Err(e) => return Err(e.into()),
        }
    }
    let org = org.ok_or_else(|| ApiError::internal("could not allocate org slug"))?;

    // O criador entra como admin.
    sqlx::query("INSERT INTO org_members (org_id, user_id, role, title) VALUES ($1, $2, 'admin', 'Administrador')")
        .bind(org.id)
        .bind(auth.user_id)
        .execute(&state.db)
        .await?;

    Ok(Json(org))
}

/// Organizações de quem está autenticado, com o seu papel em cada uma.
#[utoipa::path(
    get, path = "/api/orgs", tag = "orgs",
    security(("session" = [])),
    responses(
        (status = 200, body = Vec<OrgSummary>),
        (status = 401, body = crate::openapi::ErrorBody),
    )
)]
pub async fn my_orgs(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
) -> Result<Json<Vec<OrgSummary>>, ApiError> {
    let orgs: Vec<OrgSummary> = sqlx::query_as(
        r#"
        SELECT o.id, o.name, o.slug, m.role,
               (SELECT COUNT(*) FROM org_members mm
                 WHERE mm.org_id = o.id AND mm.archived_at IS NULL) AS member_count,
               o.domain, o.retention_days, o.max_groups, o.max_rooms, o.max_meetings
        FROM organizations o
        -- Só pertenças ACTIVAS: um membro arquivado deixava de alcançar as
        -- rotas da org (S3) mas continuava a vê-la aqui, com o papel antigo.
        JOIN org_members m ON m.org_id = o.id AND m.user_id = $1 AND m.archived_at IS NULL
        ORDER BY o.name
        "#,
    )
    .bind(auth.user_id)
    .fetch_all(&state.db)
    .await?;
    Ok(Json(orgs))
}

// ---------- branches ----------

#[derive(Deserialize, utoipa::ToSchema)]
pub struct CreateBranchReq {
    pub name: String,
    #[serde(default)]
    pub location: String,
}

/// Cria uma filial (só admin).
#[utoipa::path(
    post, path = "/api/orgs/{org_id}/branches", tag = "orgs",
    security(("session" = [])),
    params(("org_id" = Uuid, Path, description = "Organização.")),
    request_body = CreateBranchReq,
    responses(
        (status = 200, body = Branch),
        (status = 400, description = "Nome vazio ou com mais de 120 caracteres.", body = crate::openapi::ErrorBody),
        (status = 401, description = "Sem sessão.", body = crate::openapi::ErrorBody),
        (status = 403, description = "Membro sem papel de admin.", body = crate::openapi::ErrorBody),
        (status = 404, description = "A organização não existe ou quem pede não é membro activo.", body = crate::openapi::ErrorBody),
    )
)]
pub async fn create_branch(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(org_id): Path<Uuid>,
    Json(req): Json<CreateBranchReq>,
) -> Result<Json<Branch>, ApiError> {
    require_admin(&state, org_id, auth.user_id).await?;
    let name = req.name.trim();
    if name.is_empty() || name.len() > 120 {
        return Err(ApiError::BadRequest("nome da filial inválido".into()));
    }
    let branch: Branch = sqlx::query_as(&format!(
        "INSERT INTO branches (org_id, name, location) VALUES ($1, $2, $3)
         RETURNING {BRANCH_COLUMNS}"
    ))
    .bind(org_id)
    .bind(name)
    .bind(req.location.trim())
    .fetch_one(&state.db)
    .await?;
    Ok(Json(branch))
}

/// Filiais da organização (membros).
#[utoipa::path(
    get, path = "/api/orgs/{org_id}/branches", tag = "orgs",
    security(("session" = [])),
    params(("org_id" = Uuid, Path, description = "Organização.")),
    responses(
        (status = 200, body = Vec<Branch>),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 404, description = "A organização não existe ou quem pede não é membro activo.", body = crate::openapi::ErrorBody),
    )
)]
pub async fn list_branches(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(org_id): Path<Uuid>,
) -> Result<Json<Vec<Branch>>, ApiError> {
    require_member(&state, org_id, auth.user_id).await?;
    let branches: Vec<Branch> = sqlx::query_as(&format!(
        "SELECT {BRANCH_COLUMNS} FROM branches WHERE org_id = $1 ORDER BY name"
    ))
    .bind(org_id)
    .fetch_all(&state.db)
    .await?;
    Ok(Json(branches))
}

// ---------- employees ----------

#[derive(Deserialize, utoipa::ToSchema)]
pub struct AddEmployeeReq {
    /// Adicionar por email: se o utilizador existir liga-o; senão cria a conta.
    pub email: String,
    #[serde(default)]
    pub username: Option<String>,
    #[serde(default)]
    pub password: Option<String>,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub role: Option<String>,
    #[serde(default)]
    pub branch_id: Option<Uuid>,
}

/// Adiciona um colaborador por email (só admin). Se a conta não existir, é
/// criada (password por omissão quando omitida); se já for membro, actualiza
/// papel, cargo e filial.
#[utoipa::path(
    post, path = "/api/orgs/{org_id}/employees", tag = "orgs",
    security(("session" = [])),
    params(("org_id" = Uuid, Path, description = "Organização.")),
    request_body = AddEmployeeReq,
    responses(
        (status = 200, body = AddEmployeeResp, description = "Sem `password` no pedido e conta nova: `temporary_password` vem preenchida (uma só vez)."),
        (status = 400, description = "Email/password inválidos, email fora do domínio da organização, ou `role` diferente de `admin`/`member`.", body = crate::openapi::ErrorBody),
        (status = 401, description = "Sem sessão.", body = crate::openapi::ErrorBody),
        (status = 403, description = "Membro sem papel de admin.", body = crate::openapi::ErrorBody),
        (status = 404, description = "A organização não existe ou quem pede não é membro activo.", body = crate::openapi::ErrorBody),
        (status = 409, description = "A conta já pertence a outra organização, ou email/username já existe.", body = crate::openapi::ErrorBody),
    )
)]
pub async fn add_employee(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(org_id): Path<Uuid>,
    Json(req): Json<AddEmployeeReq>,
) -> Result<Json<AddEmployeeResp>, ApiError> {
    require_admin(&state, org_id, auth.user_id).await?;
    let email = delonix_meet_domain::identity::validation::normalize_email(&req.email);
    // Antes faltava aqui o limite de 254 caracteres que auth::register já
    // impunha — mesma política de email, agora num só sítio (ADR-0004, Fase 2).
    delonix_meet_domain::identity::validation::validate_email(&email)
        .map_err(ApiError::BadRequest)?;
    // Org-first: o email do colaborador tem de ser do domínio da organização.
    let org_domain: Option<(String,)> =
        sqlx::query_as("SELECT email_domain FROM organizations WHERE id = $1")
            .bind(org_id)
            .fetch_optional(&state.db)
            .await?;
    if let Some((dom,)) = org_domain {
        if !dom.is_empty() && email.split('@').nth(1) != Some(dom.as_str()) {
            return Err(ApiError::BadRequest(format!(
                "o email tem de ser do domínio da organização (@{dom})"
            )));
        }
    }
    let role = req.role.as_deref().unwrap_or("member");
    if !matches!(role, "admin" | "member") {
        return Err(ApiError::BadRequest("role inválido".into()));
    }

    // Utilizador existente ou criar novo.
    let existing: Option<(Uuid,)> = sqlx::query_as("SELECT id FROM users WHERE email = $1")
        .bind(&email)
        .fetch_optional(&state.db)
        .await?;
    // Uma conta que já pertença a OUTRA organização não é reclamada por email —
    // a mesma regra `ForeignOrg` de `meetings_v1::resolve_org_user` e da R25.
    // O domínio da org é a primeira barreira, mas não a única que interessa:
    // uma org LEGADA com `email_domain` vazio salta a verificação acima, e sem
    // isto o seu admin puxava qualquer conta existente (de qualquer domínio)
    // para dentro da org, como admin, tornando-se colega dela em `room_access`
    // — provado ao vivo a 2026-09-16. Re-adicionar alguém que já é membro DESTA
    // org continua a funcionar (mudar papel/filial): a guarda é só o outro org.
    if let Some((id,)) = existing {
        let noutra_org: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM org_members
                           WHERE user_id = $1 AND org_id <> $2 AND archived_at IS NULL)",
        )
        .bind(id)
        .bind(org_id)
        .fetch_one(&state.db)
        .await?;
        if noutra_org {
            return Err(ApiError::Conflict(format!(
                "a conta {email} já pertence a outra organização; \
                 a entrada numa segunda organização tem de ser feita pelo dono da conta"
            )));
        }
    }
    // Password temporária gerada quando o admin não indica nenhuma. Sai UMA vez,
    // nesta resposta, para o admin a entregar ao colaborador.
    let mut temporary_password: Option<String> = None;
    let user_id = match existing {
        Some((id,)) => id,
        None => {
            let username = req
                .username
                .as_deref()
                .map(|s| s.trim().to_string())
                .filter(|s| s.len() >= 2)
                .unwrap_or_else(|| email.split('@').next().unwrap_or("employee").to_string());
            // Nunca uma password FIXA: `changeme123` abria a conta de qualquer
            // colaborador recém-adicionado a quem soubesse o email (R150).
            let password = match req.password.as_deref() {
                Some(p) => {
                    delonix_meet_domain::identity::validation::validate_password(p)
                        .map_err(ApiError::BadRequest)?;
                    p.to_string()
                }
                None => {
                    let p = delonix_meet_core::crypto::random_hex(12);
                    temporary_password = Some(p.clone());
                    p
                }
            };
            let hash = crate::auth::hash_password(&password)?;
            let row: Result<(Uuid,), sqlx::Error> = sqlx::query_as(
                "INSERT INTO users (email, username, password_hash) VALUES ($1, $2, $3) RETURNING id",
            )
            .bind(&email)
            .bind(&username)
            .bind(&hash)
            .fetch_one(&state.db)
            .await;
            match row {
                Ok((id,)) => id,
                Err(sqlx::Error::Database(db)) if db.is_unique_violation() => {
                    return Err(ApiError::Conflict("email ou username já existe".into()))
                }
                Err(e) => return Err(e.into()),
            }
        }
    };

    sqlx::query(
        "INSERT INTO org_members (org_id, user_id, branch_id, role, title) VALUES ($1, $2, $3, $4, $5)
         ON CONFLICT (org_id, user_id) DO UPDATE SET branch_id = $3, role = $4, title = $5",
    )
    .bind(org_id)
    .bind(user_id)
    .bind(req.branch_id)
    .bind(role)
    .bind(req.title.trim())
    .execute(&state.db)
    .await?;

    let emp: Employee = sqlx::query_as(&format!(
        "SELECT {EMPLOYEE_COLUMNS}
           FROM org_members m JOIN users u ON u.id = m.user_id
           LEFT JOIN branches b ON b.id = m.branch_id
           WHERE m.org_id = $1 AND m.user_id = $2"
    ))
    .bind(org_id)
    .bind(user_id)
    .fetch_one(&state.db)
    .await?;
    crate::audit::log(
        &state.db,
        Some(org_id),
        auth.user_id,
        "member.added",
        &emp.email,
    )
    .await;
    Ok(Json(AddEmployeeResp {
        employee: emp,
        temporary_password,
    }))
}

/// O colaborador adicionado e, SÓ quando a conta nasceu sem password
/// indicada, a password temporária gerada — mostrada uma vez.
#[derive(Serialize, utoipa::ToSchema)]
pub struct AddEmployeeResp {
    #[serde(flatten)]
    pub employee: Employee,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temporary_password: Option<String>,
}

/// Colaboradores activos da organização (membros), com a última actividade.
#[utoipa::path(
    get, path = "/api/orgs/{org_id}/employees", tag = "orgs",
    security(("session" = [])),
    params(("org_id" = Uuid, Path, description = "Organização.")),
    responses(
        (status = 200, body = Vec<Employee>),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 404, description = "A organização não existe ou quem pede não é membro activo.", body = crate::openapi::ErrorBody),
    )
)]
pub async fn list_employees(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(org_id): Path<Uuid>,
) -> Result<Json<Vec<Employee>>, ApiError> {
    require_member(&state, org_id, auth.user_id).await?;
    let emps: Vec<Employee> = sqlx::query_as(&format!(
        "SELECT {EMPLOYEE_COLUMNS},
                  (SELECT MAX(a.created_at) FROM audit_logs a WHERE a.actor_id = m.user_id) AS last_active
           FROM org_members m JOIN users u ON u.id = m.user_id
           LEFT JOIN branches b ON b.id = m.branch_id
           WHERE m.org_id = $1 AND m.archived_at IS NULL ORDER BY u.username"
    ))
    .bind(org_id)
    .fetch_all(&state.db)
    .await?;
    Ok(Json(emps))
}

#[derive(Deserialize, utoipa::ToSchema)]
pub struct UpdateEmployeeReq {
    pub role: Option<String>,
    pub title: Option<String>,
    pub branch_id: Option<Uuid>,
}

/// Altera papel, cargo e/ou filial de um membro (só admin).
#[utoipa::path(
    patch, path = "/api/orgs/{org_id}/employees/{user_id}", tag = "orgs",
    security(("session" = [])),
    params(("org_id" = Uuid, Path, description = "Organização."), ("user_id" = Uuid, Path, description = "Utilizador membro.")),
    request_body = UpdateEmployeeReq,
    responses(
        (status = 200, body = Employee),
        (status = 400, description = "`role` diferente de `admin`/`member`.", body = crate::openapi::ErrorBody),
        (status = 401, description = "Sem sessão.", body = crate::openapi::ErrorBody),
        (status = 403, description = "Membro sem papel de admin.", body = crate::openapi::ErrorBody),
        (status = 404, description = "Quem pede não é membro activo, ou o utilizador não é membro da organização.", body = crate::openapi::ErrorBody),
    )
)]
pub async fn update_employee(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path((org_id, user_id)): Path<(Uuid, Uuid)>,
    Json(req): Json<UpdateEmployeeReq>,
) -> Result<Json<Employee>, ApiError> {
    require_admin(&state, org_id, auth.user_id).await?;
    if let Some(role) = &req.role {
        if !matches!(role.as_str(), "admin" | "member") {
            return Err(ApiError::BadRequest("role inválido".into()));
        }
    }
    if let Some(role) = &req.role {
        sqlx::query("UPDATE org_members SET role = $1 WHERE org_id = $2 AND user_id = $3")
            .bind(role)
            .bind(org_id)
            .bind(user_id)
            .execute(&state.db)
            .await?;
    }
    if let Some(title) = &req.title {
        let title = title.trim();
        sqlx::query("UPDATE org_members SET title = $1 WHERE org_id = $2 AND user_id = $3")
            .bind(title)
            .bind(org_id)
            .bind(user_id)
            .execute(&state.db)
            .await?;
    }
    if req.branch_id.is_some() || req.role.is_some() {
        if let Some(branch_id) = req.branch_id {
            sqlx::query("UPDATE org_members SET branch_id = $1 WHERE org_id = $2 AND user_id = $3")
                .bind(branch_id)
                .bind(org_id)
                .bind(user_id)
                .execute(&state.db)
                .await?;
        }
    }
    let emp: Employee = sqlx::query_as(&format!(
        "SELECT {EMPLOYEE_COLUMNS}
           FROM org_members m JOIN users u ON u.id = m.user_id
           LEFT JOIN branches b ON b.id = m.branch_id
           WHERE m.org_id = $1 AND m.user_id = $2"
    ))
    .bind(org_id)
    .bind(user_id)
    .fetch_one(&state.db)
    .await?;
    Ok(Json(emp))
}

/// Arquiva o acesso de um membro (soft delete, só admin). Idempotente: um
/// utilizador que não é membro também devolve `ok`.
#[utoipa::path(
    delete, path = "/api/orgs/{org_id}/employees/{user_id}", tag = "orgs",
    security(("session" = [])),
    params(("org_id" = Uuid, Path, description = "Organização."), ("user_id" = Uuid, Path, description = "Utilizador membro.")),
    responses(
        (status = 200, description = "{\"ok\": true} (forma herdada)", body = serde_json::Value),
        (status = 400, description = "Tentativa de arquivar o próprio acesso.", body = crate::openapi::ErrorBody),
        (status = 401, description = "Sem sessão.", body = crate::openapi::ErrorBody),
        (status = 403, description = "Membro sem papel de admin.", body = crate::openapi::ErrorBody),
        (status = 404, description = "A organização não existe ou quem pede não é membro activo.", body = crate::openapi::ErrorBody),
    )
)]
pub async fn remove_employee(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path((org_id, user_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_admin(&state, org_id, auth.user_id).await?;
    if user_id == auth.user_id {
        return Err(ApiError::BadRequest(
            "não podes arquivar o teu próprio acesso".into(),
        ));
    }
    // Soft delete: archived_at + archived_by para auditoria futura
    sqlx::query(
        "UPDATE org_members SET archived_at = NOW(), archived_by = $3
         WHERE org_id = $1 AND user_id = $2 AND archived_at IS NULL",
    )
    .bind(org_id)
    .bind(user_id)
    .bind(auth.user_id)
    .execute(&state.db)
    .await?;
    crate::audit::log(
        &state.db,
        Some(org_id),
        auth.user_id,
        "member.removed",
        &user_id.to_string(),
    )
    .await;
    Ok(Json(serde_json::json!({ "ok": true })))
}

// ---------- groups ----------

#[derive(Deserialize, utoipa::ToSchema)]
pub struct CreateGroupReq {
    pub name: String,
    #[serde(default)]
    pub member_ids: Vec<Uuid>,
}

/// Cria um grupo de colaboradores (qualquer membro). O criador entra sempre;
/// `member_ids` que não sejam membros da organização são ignorados.
#[utoipa::path(
    post, path = "/api/orgs/{org_id}/groups", tag = "orgs",
    security(("session" = [])),
    params(("org_id" = Uuid, Path, description = "Organização.")),
    request_body = CreateGroupReq,
    responses(
        (status = 200, body = Group),
        (status = 400, description = "Nome vazio ou com mais de 120 caracteres.", body = crate::openapi::ErrorBody),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 404, description = "A organização não existe ou quem pede não é membro activo.", body = crate::openapi::ErrorBody),
        (status = 409, description = "Quota `max_groups` da organização atingida.", body = crate::openapi::ErrorBody),
    )
)]
pub async fn create_group(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(org_id): Path<Uuid>,
    Json(req): Json<CreateGroupReq>,
) -> Result<Json<Group>, ApiError> {
    require_member(&state, org_id, auth.user_id).await?;
    let name = req.name.trim();
    if name.is_empty() || name.len() > 120 {
        return Err(ApiError::BadRequest("nome do grupo inválido".into()));
    }
    // RLS (migração 0024): employee_groups corre no contexto de tenant. O limite
    // de quota vive em `organizations` (sem RLS); o COUNT dos grupos existentes
    // corre na MESMA tx (sob RLS) para ser correto. Ver AppState::tenant_tx.
    // Os membros validam-se ANTES de abrir a transacção: `role_in_org` usa a
    // pool, e pedir uma segunda ligação com a tx aberta prendia duas ligações
    // por pedido — com a pool cheia, o pedido esperava por si próprio até ao
    // timeout e dava 500 (medido nos testes de integração, 2026-09-16).
    let mut ids = req.member_ids.clone();
    ids.push(auth.user_id);
    ids.sort();
    ids.dedup();
    let mut members = Vec::with_capacity(ids.len());
    for uid in ids {
        if role_in_org(&state, org_id, uid).await?.is_some() {
            members.push(uid);
        }
    }
    let mut tx = state.tenant_tx(auth.user_id).await?;
    let limit: Option<i32> =
        sqlx::query_scalar("SELECT max_groups FROM organizations WHERE id = $1")
            .bind(org_id)
            .fetch_one(&mut *tx)
            .await?;
    if let Some(max) = limit {
        let count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM employee_groups WHERE org_id = $1")
                .bind(org_id)
                .fetch_one(&mut *tx)
                .await?;
        if count >= max as i64 {
            return Err(ApiError::Conflict(format!(
                "limite da organização atingido ({max})"
            )));
        }
    }
    let (group_id,): (Uuid,) = sqlx::query_as(
        "INSERT INTO employee_groups (org_id, name, created_by) VALUES ($1, $2, $3) RETURNING id",
    )
    .bind(org_id)
    .bind(name)
    .bind(auth.user_id)
    .fetch_one(&mut *tx)
    .await?;

    // O criador entra sempre; membros já validados como pertencentes à org.
    for uid in members {
        sqlx::query(
            "INSERT INTO group_members (group_id, user_id) VALUES ($1, $2) ON CONFLICT DO NOTHING",
        )
        .bind(group_id)
        .bind(uid)
        .execute(&mut *tx)
        .await?;
    }

    let group: Group = sqlx::query_as(&format!(
        "SELECT {GROUP_COLUMNS} FROM employee_groups g WHERE g.id = $1"
    ))
    .bind(group_id)
    .fetch_one(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(Json(group))
}

/// Grupos da organização (membros).
#[utoipa::path(
    get, path = "/api/orgs/{org_id}/groups", tag = "orgs",
    security(("session" = [])),
    params(("org_id" = Uuid, Path, description = "Organização.")),
    responses(
        (status = 200, body = Vec<Group>),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 404, description = "A organização não existe ou quem pede não é membro activo.", body = crate::openapi::ErrorBody),
    )
)]
pub async fn list_groups(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(org_id): Path<Uuid>,
) -> Result<Json<Vec<Group>>, ApiError> {
    require_member(&state, org_id, auth.user_id).await?;
    // RLS: employee_groups tem Row-Level Security (migração 0024). A query corre
    // no contexto de tenant do utilizador — ver AppState::tenant_tx / ADR-0002.
    let mut tx = state.tenant_tx(auth.user_id).await?;
    let groups: Vec<Group> = sqlx::query_as(&format!(
        "SELECT {GROUP_COLUMNS} FROM employee_groups g WHERE g.org_id = $1 ORDER BY g.name"
    ))
    .bind(org_id)
    .fetch_all(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(Json(groups))
}

// ---------- salas presenciais (físicas) ----------

#[derive(Debug, Serialize, utoipa::ToSchema, sqlx::FromRow)]
pub struct MeetingRoom {
    pub id: Uuid,
    pub org_id: Uuid,
    pub name: String,
    pub location: String,
    pub capacity: i32,
}

/// Estava copiada à mão em `create_meeting_room` e `list_meeting_rooms` (ADR-0004).
const MEETING_ROOM_COLUMNS: &str = "id, org_id, name, location, capacity";

#[derive(Deserialize, utoipa::ToSchema)]
pub struct CreateMeetingRoomReq {
    pub name: String,
    #[serde(default)]
    pub location: String,
    #[serde(default)]
    pub capacity: i32,
}

/// Cria uma sala presencial (física) da organização (só admin).
#[utoipa::path(
    post, path = "/api/orgs/{org_id}/meeting-rooms", tag = "orgs",
    security(("session" = [])),
    params(("org_id" = Uuid, Path, description = "Organização.")),
    request_body = CreateMeetingRoomReq,
    responses(
        (status = 200, body = MeetingRoom),
        (status = 400, description = "Nome vazio ou com mais de 120 caracteres.", body = crate::openapi::ErrorBody),
        (status = 401, description = "Sem sessão.", body = crate::openapi::ErrorBody),
        (status = 403, description = "Membro sem papel de admin.", body = crate::openapi::ErrorBody),
        (status = 404, description = "A organização não existe ou quem pede não é membro activo.", body = crate::openapi::ErrorBody),
        (status = 409, description = "Quota `max_rooms` da organização atingida.", body = crate::openapi::ErrorBody),
    )
)]
pub async fn create_meeting_room(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(org_id): Path<Uuid>,
    Json(req): Json<CreateMeetingRoomReq>,
) -> Result<Json<MeetingRoom>, ApiError> {
    require_admin(&state, org_id, auth.user_id).await?;
    let name = req.name.trim();
    if name.is_empty() || name.len() > 120 {
        return Err(ApiError::BadRequest("nome da sala inválido".into()));
    }
    enforce_quota(
        &state,
        org_id,
        "max_rooms",
        "SELECT COUNT(*) FROM meeting_rooms WHERE org_id = $1",
    )
    .await?;
    let room: MeetingRoom = sqlx::query_as(&format!(
        "INSERT INTO meeting_rooms (org_id, name, location, capacity) VALUES ($1, $2, $3, $4)
         RETURNING {MEETING_ROOM_COLUMNS}"
    ))
    .bind(org_id)
    .bind(name)
    .bind(req.location.trim())
    .bind(req.capacity.max(0))
    .fetch_one(&state.db)
    .await?;
    Ok(Json(room))
}

/// Salas presenciais da organização (membros).
#[utoipa::path(
    get, path = "/api/orgs/{org_id}/meeting-rooms", tag = "orgs",
    security(("session" = [])),
    params(("org_id" = Uuid, Path, description = "Organização.")),
    responses(
        (status = 200, body = Vec<MeetingRoom>),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 404, description = "A organização não existe ou quem pede não é membro activo.", body = crate::openapi::ErrorBody),
    )
)]
pub async fn list_meeting_rooms(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(org_id): Path<Uuid>,
) -> Result<Json<Vec<MeetingRoom>>, ApiError> {
    require_member(&state, org_id, auth.user_id).await?;
    let rooms: Vec<MeetingRoom> = sqlx::query_as(&format!(
        "SELECT {MEETING_ROOM_COLUMNS} FROM meeting_rooms WHERE org_id = $1 ORDER BY name"
    ))
    .bind(org_id)
    .fetch_all(&state.db)
    .await?;
    Ok(Json(rooms))
}

// ---------- Estatísticas da organização (consola admin) ----------

#[derive(Debug, Serialize, utoipa::ToSchema, sqlx::FromRow)]
pub struct WeekBucket {
    pub week_start: DateTime<Utc>,
    pub count: i64,
    pub minutes: i64,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct OrgStats {
    pub meetings_30d: i64,
    pub meeting_minutes_30d: i64,
    pub active_users_30d: i64,
    pub members_total: i64,
    pub recordings_total: i64,
    pub recordings_bytes: i64,
    pub video_30d: i64,
    pub voice_30d: i64,
    pub avg_duration_min: i64,
    pub top_organizers: Vec<Organizer>,
    pub meetings_per_week: Vec<WeekBucket>,
    /// Qualidade das chamadas (amostras QoS dos clientes, últimos 30 dias).
    pub quality_samples_30d: i64,
    pub avg_rtt_ms: Option<i64>,
    pub avg_loss_pct: f64,
    /// % de amostras boas (perda < 2%) e fracas (perda > 5%).
    pub pct_good: i64,
    pub pct_poor: i64,
    /// Delonix Call Quality Score médio (0–100) das amostras que o trazem.
    /// `None` enquanto não houver clientes com a versão que o reporta — e
    /// `None` diz isso mesmo, ao contrário de um `0` que pareceria «péssimo».
    pub avg_score: Option<i64>,
    /// % de amostras com pontuação abaixo de 60 («fraca» ou pior).
    pub pct_low_score: Option<i64>,
    /// % de amostras em que a media passou por TURN relay. Sobe = custo de
    /// banda no relay e latência acrescida; é a métrica de factura.
    pub pct_turn_relay: Option<i64>,
    /// % de amostras em que o encoder estava travado por CPU. É a única forma
    /// portável de ver que o problema era a MÁQUINA do cliente, não a rede —
    /// sem isto, um portátil velho conta como «rede má» e enviesa o
    /// diagnóstico da plataforma inteira.
    pub pct_cpu_limited: Option<i64>,
    /// Período homólogo anterior (30–60 dias atrás) para os deltas dos KPIs.
    pub meetings_prev_30d: i64,
    pub meeting_minutes_prev_30d: i64,
    pub active_users_prev_30d: i64,
}

#[derive(Debug, Serialize, utoipa::ToSchema, sqlx::FromRow)]
pub struct Organizer {
    pub username: String,
    pub count: i64,
}

/// KPIs agregados da organização: reuniões e minutos dos últimos 30 dias,
/// utilizadores envolvidos, gravações e série semanal (8 semanas).
/// Uma reunião "pertence" à org se o dono ou um convidado for membro. Só admin.
#[utoipa::path(
    get, path = "/api/orgs/{org_id}/stats", tag = "orgs",
    security(("session" = [])),
    params(("org_id" = Uuid, Path, description = "Organização.")),
    responses(
        (status = 200, body = OrgStats),
        (status = 401, description = "Sem sessão.", body = crate::openapi::ErrorBody),
        (status = 403, description = "Membro sem papel de admin.", body = crate::openapi::ErrorBody),
        (status = 404, description = "A organização não existe ou quem pede não é membro activo.", body = crate::openapi::ErrorBody),
    )
)]
pub async fn org_stats(
    State(state): State<Arc<AppState>>,
    Path(org_id): Path<Uuid>,
    auth: AuthUser,
) -> Result<Json<OrgStats>, ApiError> {
    // Least-privilege: KPIs da org (volume, leaderboard nominal) só para admins.
    require_admin(&state, org_id, auth.user_id).await?;

    const ORG_MEETING: &str =
        "(EXISTS (SELECT 1 FROM org_members om WHERE om.org_id = $1 AND om.user_id = m.owner_id)
         OR EXISTS (SELECT 1 FROM meeting_invitees mi
                    JOIN org_members om2 ON om2.user_id = mi.user_id AND om2.org_id = $1
                    WHERE mi.meeting_id = m.id))";

    let (meetings_30d, meeting_minutes_30d, video_30d, voice_30d): (i64, i64, i64, i64) =
        sqlx::query_as(&format!(
            "SELECT COUNT(*), COALESCE(SUM(m.duration_min), 0)::bigint,
                    COUNT(*) FILTER (WHERE m.kind = 'video'),
                    COUNT(*) FILTER (WHERE m.kind = 'voice')
             FROM meetings m
             WHERE m.starts_at >= now() - interval '30 days' AND m.starts_at <= now() AND {ORG_MEETING}"
        ))
        .bind(org_id)
        .fetch_one(&state.db)
        .await?;
    let (meetings_prev_30d, meeting_minutes_prev_30d): (i64, i64) = sqlx::query_as(&format!(
        "SELECT COUNT(*), COALESCE(SUM(m.duration_min), 0)::bigint
         FROM meetings m
         WHERE m.starts_at >= now() - interval '60 days'
           AND m.starts_at < now() - interval '30 days' AND {ORG_MEETING}"
    ))
    .bind(org_id)
    .fetch_one(&state.db)
    .await?;

    let avg_duration_min = if meetings_30d > 0 {
        meeting_minutes_30d / meetings_30d
    } else {
        0
    };

    let top_organizers: Vec<Organizer> = sqlx::query_as(&format!(
        "SELECT u.username, COUNT(*) AS count FROM meetings m
         JOIN users u ON u.id = m.owner_id
         WHERE m.starts_at >= now() - interval '30 days' AND m.starts_at <= now() AND {ORG_MEETING}
         GROUP BY u.username ORDER BY count DESC, u.username LIMIT 5"
    ))
    .bind(org_id)
    .fetch_all(&state.db)
    .await?;

    let (active_users_30d,): (i64,) = sqlx::query_as(
        "SELECT COUNT(DISTINCT u) FROM (
             SELECT m.owner_id AS u FROM meetings m
             WHERE m.starts_at >= now() - interval '30 days'
           UNION
             SELECT mi.user_id FROM meeting_invitees mi
             JOIN meetings m ON m.id = mi.meeting_id
             WHERE m.starts_at >= now() - interval '30 days'
         ) act
         JOIN org_members om ON om.user_id = act.u AND om.org_id = $1",
    )
    .bind(org_id)
    .fetch_one(&state.db)
    .await?;

    let (active_users_prev_30d,): (i64,) = sqlx::query_as(
        "SELECT COUNT(DISTINCT u) FROM (
             SELECT m.owner_id AS u FROM meetings m
             WHERE m.starts_at >= now() - interval '60 days' AND m.starts_at < now() - interval '30 days'
           UNION
             SELECT mi.user_id FROM meeting_invitees mi
             JOIN meetings m ON m.id = mi.meeting_id
             WHERE m.starts_at >= now() - interval '60 days' AND m.starts_at < now() - interval '30 days'
         ) act
         JOIN org_members om ON om.user_id = act.u AND om.org_id = $1",
    )
    .bind(org_id)
    .fetch_one(&state.db)
    .await?;

    let (members_total,): (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM org_members WHERE org_id = $1")
            .bind(org_id)
            .fetch_one(&state.db)
            .await?;

    let (recordings_total, recordings_bytes): (i64, i64) = sqlx::query_as(
        "SELECT COUNT(*), COALESCE(SUM(r.size_bytes), 0)::bigint FROM recordings r
         WHERE EXISTS (SELECT 1 FROM org_members om WHERE om.org_id = $1 AND om.user_id = r.uploader_id)",
    )
    .bind(org_id)
    .fetch_one(&state.db)
    .await?;

    let meetings_per_week: Vec<WeekBucket> = sqlx::query_as(&format!(
        "SELECT date_trunc('week', m.starts_at) AS week_start, COUNT(*) AS count,
                COALESCE(SUM(m.duration_min), 0)::bigint AS minutes
         FROM meetings m
         WHERE m.starts_at >= date_trunc('week', now()) - interval '7 weeks'
           AND m.starts_at < date_trunc('week', now()) + interval '1 week'
           AND {ORG_MEETING}
         GROUP BY 1 ORDER BY 1",
    ))
    .bind(org_id)
    .fetch_all(&state.db)
    .await?;

    // Qualidade das chamadas: agregado das amostras QoS dos membros da org
    // (rooms não têm org_id — o scoping é pelo utilizador que reporta).
    // As percentagens novas dividem pelo número de amostras que TRAZEM o campo
    // (`COUNT(q.score)`), não pelo total. Dividir pelo total misturaria clientes
    // antigos — que não reportam — com clientes bons, e a percentagem de
    // problemas apareceria artificialmente baixa à medida que a versão nova
    // fosse sendo adoptada. `NULLIF(...,0)` devolve NULL enquanto não houver
    // nenhuma amostra com o campo, e NULL lê-se «ainda não sei», que é a
    // verdade — ao contrário de um zero.
    #[allow(clippy::type_complexity)]
    let (
        quality_samples_30d,
        avg_rtt_ms,
        avg_loss_pct,
        pct_good,
        pct_poor,
        avg_score,
        pct_low_score,
        pct_turn_relay,
        pct_cpu_limited,
    ): (
        i64,
        Option<f64>,
        Option<f64>,
        Option<f64>,
        Option<f64>,
        Option<f64>,
        Option<f64>,
        Option<f64>,
        Option<f64>,
    ) = sqlx::query_as(
        "SELECT COUNT(*),
                AVG(q.rtt_ms)::float8,
                AVG(q.loss_pct)::float8,
                (100.0 * COUNT(*) FILTER (WHERE q.loss_pct < 2.0) / NULLIF(COUNT(*), 0))::float8,
                (100.0 * COUNT(*) FILTER (WHERE q.loss_pct > 5.0) / NULLIF(COUNT(*), 0))::float8,
                AVG(q.score)::float8,
                (100.0 * COUNT(*) FILTER (WHERE q.score < 60)
                    / NULLIF(COUNT(q.score), 0))::float8,
                (100.0 * COUNT(*) FILTER (WHERE q.turn_relay)
                    / NULLIF(COUNT(q.turn_relay), 0))::float8,
                (100.0 * COUNT(*) FILTER (WHERE q.limited_by = 'cpu')
                    / NULLIF(COUNT(q.limited_by), 0))::float8
         FROM call_quality_samples q
         JOIN org_members om ON om.user_id = q.user_id AND om.org_id = $1
         WHERE q.created_at > now() - interval '30 days'",
    )
    .bind(org_id)
    .fetch_one(&state.db)
    .await?;

    Ok(Json(OrgStats {
        meetings_30d,
        meeting_minutes_30d,
        active_users_30d,
        members_total,
        recordings_total,
        recordings_bytes,
        video_30d,
        voice_30d,
        avg_duration_min,
        top_organizers,
        meetings_per_week,
        quality_samples_30d,
        avg_rtt_ms: avg_rtt_ms.map(|v| v.round() as i64),
        avg_loss_pct: (avg_loss_pct.unwrap_or(0.0) * 10.0).round() / 10.0,
        pct_good: pct_good.unwrap_or(0.0).round() as i64,
        pct_poor: pct_poor.unwrap_or(0.0).round() as i64,
        avg_score: avg_score.map(|v| v.round() as i64),
        pct_low_score: pct_low_score.map(|v| v.round() as i64),
        pct_turn_relay: pct_turn_relay.map(|v| v.round() as i64),
        pct_cpu_limited: pct_cpu_limited.map(|v| v.round() as i64),
        meetings_prev_30d,
        meeting_minutes_prev_30d,
        active_users_prev_30d,
    }))
}

/// user_ids dos membros de um grupo (para iniciar chamada de grupo).
/// Organizações a que um utilizador pertence (para disparar webhooks dos
/// eventos das suas reuniões/gravações).
pub async fn orgs_of_user(state: &AppState, user_id: Uuid) -> Vec<Uuid> {
    sqlx::query_as::<_, (Uuid,)>(
        "SELECT org_id FROM org_members WHERE user_id = $1 AND archived_at IS NULL",
    )
    .bind(user_id)
    .fetch_all(&state.db)
    .await
    .map(|rows| rows.into_iter().map(|r| r.0).collect())
    .unwrap_or_default()
}

/// Organizações onde `user_id` é admin (para analytics/ações administrativas).
pub async fn admin_orgs_of_user(state: &AppState, user_id: Uuid) -> Vec<Uuid> {
    sqlx::query_as::<_, (Uuid,)>(
        "SELECT org_id FROM org_members WHERE user_id = $1 AND role = 'admin' AND archived_at IS NULL",
    )
    .bind(user_id)
    .fetch_all(&state.db)
    .await
    .map(|rows| rows.into_iter().map(|r| r.0).collect())
    .unwrap_or_default()
}

/// Utilizadores que partilham pelo menos uma organização com `user_id` (exclui
/// o próprio). Base do isolamento multi-tenant em presença/pesquisa/chamadas.
pub async fn org_co_members(state: &AppState, user_id: Uuid) -> Vec<Uuid> {
    sqlx::query_as::<_, (Uuid,)>(
        "SELECT DISTINCT b.user_id FROM org_members a
         JOIN org_members b ON a.org_id = b.org_id
         WHERE a.user_id = $1 AND b.user_id <> $1
           AND a.archived_at IS NULL AND b.archived_at IS NULL",
    )
    .bind(user_id)
    .fetch_all(&state.db)
    .await
    .map(|rows| rows.into_iter().map(|r| r.0).collect())
    .unwrap_or_default()
}

/// Domínio de produção da primeira organização do utilizador (para links
/// partilháveis); vazio se nenhuma org tiver domínio configurado.
pub async fn primary_domain(state: &AppState, user_id: Uuid) -> String {
    sqlx::query_as::<_, (String,)>(
        "SELECT o.domain FROM organizations o
         JOIN org_members m ON m.org_id = o.id
         WHERE m.user_id = $1 AND o.domain <> '' ORDER BY o.name LIMIT 1",
    )
    .bind(user_id)
    .fetch_optional(&state.db)
    .await
    .ok()
    .flatten()
    .map(|r| r.0)
    .unwrap_or_default()
}

// ---------- SSO Config (admin) ----------

#[derive(Debug, Serialize, utoipa::ToSchema, sqlx::FromRow)]
pub struct SsoConfigPublic {
    pub org_id: Uuid,
    pub issuer_url: String,
    pub client_id: String,
    pub enforce_sso: bool,
}

#[derive(Deserialize, utoipa::ToSchema)]
pub struct SsoConfigReq {
    pub issuer_url: String,
    pub client_id: String,
    /// Vazio → mantém o segredo existente (no-op em update sem nova rotação).
    #[serde(default)]
    pub client_secret: String,
    pub enforce_sso: bool,
}

/// `GET /api/orgs/:org_id/sso` — lê a config OIDC (sem devolver o segredo).
/// `null` quando a organização não tem SSO configurado. Só admin.
#[utoipa::path(
    get, path = "/api/orgs/{org_id}/sso", tag = "orgs",
    security(("session" = [])),
    params(("org_id" = Uuid, Path, description = "Organização.")),
    responses(
        (status = 200, description = "Configuração OIDC, ou `null` se não houver.", body = Option<SsoConfigPublic>),
        (status = 401, description = "Sem sessão.", body = crate::openapi::ErrorBody),
        (status = 403, description = "Membro sem papel de admin.", body = crate::openapi::ErrorBody),
        (status = 404, description = "A organização não existe ou quem pede não é membro activo.", body = crate::openapi::ErrorBody),
    )
)]
pub async fn get_sso_config(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(org_id): Path<Uuid>,
) -> Result<Json<Option<SsoConfigPublic>>, ApiError> {
    require_admin(&state, org_id, auth.user_id).await?;
    let row: Option<SsoConfigPublic> = sqlx::query_as(
        "SELECT org_id, issuer_url, client_id, enforce_sso
         FROM org_sso_configs WHERE org_id = $1",
    )
    .bind(org_id)
    .fetch_optional(&state.db)
    .await?;
    Ok(Json(row))
}

/// `PUT /api/orgs/:org_id/sso` — cria ou atualiza a config OIDC.
/// O `client_secret` só é atualizado se for enviado não-vazio. Só admin.
#[utoipa::path(
    put, path = "/api/orgs/{org_id}/sso", tag = "orgs",
    security(("session" = [])),
    params(("org_id" = Uuid, Path, description = "Organização.")),
    request_body = SsoConfigReq,
    responses(
        (status = 200, description = "{\"ok\": true} (forma herdada)", body = serde_json::Value),
        (status = 400, description = "`issuer_url`/`client_id` em falta, ou `issuer_url` sem `https://`.", body = crate::openapi::ErrorBody),
        (status = 401, description = "Sem sessão.", body = crate::openapi::ErrorBody),
        (status = 403, description = "Membro sem papel de admin.", body = crate::openapi::ErrorBody),
        (status = 404, description = "A organização não existe ou quem pede não é membro activo.", body = crate::openapi::ErrorBody),
    )
)]
pub async fn upsert_sso_config(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(org_id): Path<Uuid>,
    Json(req): Json<SsoConfigReq>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_admin(&state, org_id, auth.user_id).await?;

    let issuer = req.issuer_url.trim().to_string();
    let client_id = req.client_id.trim().to_string();
    if issuer.is_empty() || client_id.is_empty() {
        return Err(ApiError::BadRequest(
            "issuer_url e client_id são obrigatórios".into(),
        ));
    }
    // Validação mínima do issuer URL.
    if !issuer.starts_with("https://") {
        return Err(ApiError::BadRequest(
            "issuer_url deve começar por https://".into(),
        ));
    }

    if req.client_secret.trim().is_empty() {
        // Atualizar sem tocar no segredo (rotação lazy).
        sqlx::query(
            "INSERT INTO org_sso_configs (org_id, issuer_url, client_id, client_secret, enforce_sso)
             VALUES ($1, $2, $3, '', $4)
             ON CONFLICT (org_id) DO UPDATE
             SET issuer_url = EXCLUDED.issuer_url,
                 client_id  = EXCLUDED.client_id,
                 enforce_sso = EXCLUDED.enforce_sso,
                 updated_at = now()",
        )
        .bind(org_id)
        .bind(&issuer)
        .bind(&client_id)
        .bind(req.enforce_sso)
        .execute(&state.db)
        .await?;
    } else {
        sqlx::query(
            "INSERT INTO org_sso_configs (org_id, issuer_url, client_id, client_secret, enforce_sso)
             VALUES ($1, $2, $3, $4, $5)
             ON CONFLICT (org_id) DO UPDATE
             SET issuer_url     = EXCLUDED.issuer_url,
                 client_id      = EXCLUDED.client_id,
                 client_secret  = EXCLUDED.client_secret,
                 enforce_sso    = EXCLUDED.enforce_sso,
                 updated_at     = now()",
        )
        .bind(org_id)
        .bind(&issuer)
        .bind(&client_id)
        .bind(req.client_secret.trim())
        .bind(req.enforce_sso)
        .execute(&state.db)
        .await?;
    }

    tracing::info!(%org_id, %issuer, "SSO config upserted");
    Ok(Json(serde_json::json!({ "ok": true })))
}

/// `DELETE /api/orgs/:org_id/sso` — remove a config OIDC (desativa SSO). Só
/// admin; idempotente.
#[utoipa::path(
    delete, path = "/api/orgs/{org_id}/sso", tag = "orgs",
    security(("session" = [])),
    params(("org_id" = Uuid, Path, description = "Organização.")),
    responses(
        (status = 200, description = "{\"ok\": true} (forma herdada)", body = serde_json::Value),
        (status = 401, description = "Sem sessão.", body = crate::openapi::ErrorBody),
        (status = 403, description = "Membro sem papel de admin.", body = crate::openapi::ErrorBody),
        (status = 404, description = "A organização não existe ou quem pede não é membro activo.", body = crate::openapi::ErrorBody),
    )
)]
pub async fn delete_sso_config(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(org_id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_admin(&state, org_id, auth.user_id).await?;
    sqlx::query("DELETE FROM org_sso_configs WHERE org_id = $1")
        .bind(org_id)
        .execute(&state.db)
        .await?;
    tracing::info!(%org_id, "SSO config deleted");
    Ok(Json(serde_json::json!({ "ok": true })))
}

pub async fn group_member_ids(state: &AppState, group_id: Uuid) -> Result<Vec<Uuid>, ApiError> {
    let rows: Vec<(Uuid,)> =
        sqlx::query_as("SELECT user_id FROM group_members WHERE group_id = $1")
            .bind(group_id)
            .fetch_all(&state.db)
            .await?;
    Ok(rows.into_iter().map(|r| r.0).collect())
}

#[cfg(test)]
mod tests {
    use super::{slugify, OrgSettingsUpdated};

    /// O tipo que substituiu o `json!` (OpenAPI) serializa igual.
    #[test]
    fn settings_updated_serializa_como_antes() {
        let v = serde_json::to_value(OrgSettingsUpdated {
            ok: true,
            domain: "meet.acme.com".into(),
            retention_days: 30,
        })
        .unwrap();
        // Campo a campo, e não um `json!` literal: a catraca da arquitectura
        // conta os `{"ok": true}` do código, e um teste não é dívida.
        assert_eq!(v.as_object().unwrap().len(), 3);
        assert_eq!(v["ok"], serde_json::Value::Bool(true));
        assert_eq!(v["domain"], "meet.acme.com");
        assert_eq!(v["retention_days"], 30);
    }

    #[test]
    fn slugify_basic() {
        assert_eq!(slugify("Kaeso Lda"), "kaeso-lda");
        assert_eq!(slugify("  Açores & Cia!  "), "a-ores-cia");
        assert_eq!(slugify("***"), "org");
    }
}
