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
    /// Retenção do chat da sala em dias corridos (G9); `null` = promessa por
    /// omissão (até ao fim do dia UTC da última mensagem).
    #[serde(default)]
    pub chat_retention_days: Option<i32>,
    pub max_groups: Option<i32>,
    pub max_rooms: Option<i32>,
    pub max_meetings: Option<i32>,
    /// O papel de quem pede (ADR-0008). `role` continua a ser o texto herdado
    /// (`admin` | `member`), derivado deste.
    pub role_id: Uuid,
    pub role_name: String,
    /// `owner` | `admin` | `member` | `external_guest`, ou `null` num papel personalizado.
    pub role_key: Option<String>,
    /// A org tem membros humanos activos e nenhum Proprietário activo.
    pub owner_missing: bool,
}

#[derive(Deserialize, utoipa::ToSchema)]
pub struct OrgSettingsReq {
    #[serde(default)]
    pub domain: String,
    #[serde(default)]
    pub retention_days: i32,
    /// `null`/omisso = promessa por omissão (fim do dia UTC da última
    /// mensagem); como os outros campos deste pedido, o valor gravado é
    /// sempre o que vier aqui, não um `PATCH` parcial.
    #[serde(default)]
    pub chat_retention_days: Option<i32>,
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
        get_org,
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
    pub chat_retention_days: Option<i32>,
}

/// Definições da organização (só admin): domínio de produção + retenção.
/// O domínio (ex.: `meet.acme.com`) é usado nos links partilháveis; a
/// retenção (>0) apaga gravações mais antigas que N dias. Quotas negativas ou
/// omissas ficam ilimitadas; valores de voz fora do enum são ignorados.
#[utoipa::path(
    patch, path = "/api/orgs/{org_id}", tag = "orgs",
    security(("session" = [])),
    params(("org_id" = Uuid, Path, description = "Organização.")),
    request_body = OrgSettingsReq,
    responses(
        (status = 200, body = OrgSettingsUpdated),
        (status = 400, description = "Domínio inválido (>253 caracteres ou com espaços).", body = crate::openapi::ErrorBody),
        (status = 401, description = "Sem sessão.", body = crate::openapi::ErrorBody),
        (status = 403, description = "Sem `admin.change_retention` (`authz.missing_capability`), ou o papel pede aprovação (`authz.approval_required`, id do pedido em `details`).", body = crate::openapi::ErrorBody),
        (status = 404, description = "A organização não existe ou quem pede não é membro activo.", body = crate::openapi::ErrorBody),
    )
)]
pub async fn update_settings(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(org_id): Path<Uuid>,
    Json(req): Json<OrgSettingsReq>,
) -> Result<Json<OrgSettingsUpdated>, ApiError> {
    // `admin.change_retention` (ADR-0008 §4): o `admin` de sistema tem-na, como
    // hoje. A aprovação, quando o papel a pede, fica ligada a ESTA alteração.
    require_capability_for(
        &state,
        org_id,
        auth.user_id,
        Capability::AdminChangeRetention,
        ResourceScope::Organization,
        &Action {
            name: "org.update_settings",
            target: serde_json::json!({
                "org_id": org_id, "domain": req.domain, "retention_days": req.retention_days,
                "max_groups": req.max_groups, "max_rooms": req.max_rooms,
                "max_meetings": req.max_meetings, "voice_media_backend": req.voice_media_backend,
                "voice_did_model": req.voice_did_model,
            }),
        },
    )
    .await?;
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
    // G9: None mantém a promessa por omissão (NULL); um valor fora de
    // 0-3650 é tratado como se não tivesse vindo, em vez de rejeitar o
    // pedido inteiro por um único campo opcional.
    let chat_retention = req.chat_retention_days.filter(|n| (0..=3650).contains(n));
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
             voice_did_model = COALESCE($8, voice_did_model),
             chat_retention_days = $9
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
    .bind(chat_retention)
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
        chat_retention_days: chat_retention,
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

/// «É administrador da org» = a capacidade de sistema `org.administer` (ADR-0008
/// §4): o `owner` e o `admin` de sistema, exactamente o `admin` de hoje.
/// Não membro activo → 404; sem a capacidade → 403 `authz.missing_capability`.
async fn require_admin(state: &AppState, org_id: Uuid, user_id: Uuid) -> Result<(), ApiError> {
    require_capability(
        state,
        org_id,
        user_id,
        Capability::OrgAdminister,
        ResourceScope::Organization,
    )
    .await
    .map(|_| ())
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

/// Consulta das organizações de um utilizador, com o seu papel. Só pertenças
/// ACTIVAS: um membro arquivado deixava de alcançar as rotas da org (S3) mas
/// continuava a vê-la na lista, com o papel antigo (R152).
const MY_ORGS_SQL: &str = r#"
    SELECT o.id, o.name, o.slug, m.role,
           (SELECT COUNT(*) FROM org_members mm
             WHERE mm.org_id = o.id AND mm.archived_at IS NULL) AS member_count,
           o.domain, o.retention_days, o.chat_retention_days,
           o.max_groups, o.max_rooms, o.max_meetings,
           m.role_id, r.name AS role_name, r.system_key AS role_key,
           (EXISTS (SELECT 1 FROM org_members hm JOIN users hu ON hu.id = hm.user_id
                     WHERE hm.org_id = o.id AND hm.archived_at IS NULL
                       AND hu.email <> 'provisioning@delonix.internal')
            AND NOT EXISTS (SELECT 1 FROM org_members om JOIN org_roles orr ON orr.id = om.role_id
                             WHERE om.org_id = o.id AND om.archived_at IS NULL
                               AND orr.system_key = 'owner')) AS owner_missing
    FROM organizations o
    JOIN org_members m ON m.org_id = o.id AND m.user_id = $1 AND m.archived_at IS NULL
    JOIN org_roles r ON r.id = m.role_id
    WHERE ($2::uuid IS NULL OR o.id = $2)
    ORDER BY o.name
"#;

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
    let orgs: Vec<OrgSummary> = sqlx::query_as(MY_ORGS_SQL)
        .bind(auth.user_id)
        .bind(None::<Uuid>)
        .fetch_all(&state.db)
        .await?;
    Ok(Json(orgs))
}

/// Uma organização de que se é membro activo, com o papel.
#[utoipa::path(
    get, path = "/api/orgs/{org_id}", tag = "orgs",
    security(("session" = [])),
    params(("org_id" = Uuid, Path)),
    responses(
        (status = 200, body = OrgSummary),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 404, body = crate::openapi::ErrorBody, description = "não existe ou não és membro activo"),
    )
)]
pub async fn get_org(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(org_id): Path<Uuid>,
) -> Result<Json<OrgSummary>, ApiError> {
    let org: Option<OrgSummary> = sqlx::query_as(MY_ORGS_SQL)
        .bind(auth.user_id)
        .bind(Some(org_id))
        .fetch_optional(&state.db)
        .await?;
    Ok(Json(org.ok_or(ApiError::NotFound)?))
}

/// Retenção do chat (G9) de quem é dono de uma sala: a mais longa entre as
/// organizações onde é membro activo com `chat_retention_days` definido.
/// `None` = nenhuma organização do dono define um valor — quem chama aplica
/// a promessa por omissão (fim do dia UTC). A regra 1 do ADR-0004 §5 exige
/// que a pertença a `org_members` só se leia daqui — por isso é `room_chat`
/// quem chama esta função, em vez de escrever o `JOIN` no seu próprio módulo.
pub async fn chat_retention_days_for_owner(
    state: &AppState,
    owner_id: Uuid,
) -> Result<Option<i32>, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT max(o.chat_retention_days) FROM org_members om
         JOIN organizations o ON o.id = om.org_id
         WHERE om.user_id = $1 AND om.archived_at IS NULL
           AND o.chat_retention_days IS NOT NULL",
    )
    .bind(owner_id)
    .fetch_one(&state.db)
    .await
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
    post, path = "/api/orgs/{org_id}/members", tag = "orgs",
    security(("session" = [])),
    params(("org_id" = Uuid, Path, description = "Organização.")),
    request_body = AddEmployeeReq,
    responses(
        (status = 200, body = AddEmployeeResp, description = "Sem `password` no pedido e conta nova: `temporary_password` vem preenchida (uma só vez)."),
        (status = 400, description = "Email/password inválidos, email fora do domínio da organização, ou `role` diferente de `admin`/`member`.", body = crate::openapi::ErrorBody),
        (status = 401, description = "Sem sessão.", body = crate::openapi::ErrorBody),
        (status = 403, description = "Sem `admin.manage_accounts`, ou a atribuição do papel seria escalada (`authz.escalation`).", body = crate::openapi::ErrorBody),
        (status = 404, description = "A organização não existe ou quem pede não é membro activo.", body = crate::openapi::ErrorBody),
        (status = 409, description = "A conta já pertence a outra organização, ou email/username já existe, ou despromovia o último dono (`role.last_owner`).", body = crate::openapi::ErrorBody),
        (status = 422, description = "Sem lugares livres (`seats.limit_reached`).", body = crate::openapi::ErrorBody),
    )
)]
pub async fn add_employee(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(org_id): Path<Uuid>,
    Json(req): Json<AddEmployeeReq>,
) -> Result<Json<AddEmployeeResp>, ApiError> {
    require_capability(
        &state,
        org_id,
        auth.user_id,
        Capability::AdminManageAccounts,
        ResourceScope::Organization,
    )
    .await?;
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

    // Papel pedido → papel de sistema, com a regra da escalada (ADR-0008 §5).
    let wanted = if role == "admin" {
        SystemRole::Admin
    } else {
        SystemRole::Member
    };
    crate::roles::ensure_can_assign_system(&state, org_id, auth.user_id, wanted).await?;
    // Pertença nova ocupa lugar: o tecto verifica-se com a org bloqueada.
    let mut tx = state.db.begin().await?;
    let existing_member = member_state(&mut *tx, org_id, user_id).await?;
    if existing_member.is_none() {
        let (used, limit) = seat_usage_tx(&mut tx, org_id, true).await?;
        delonix_meet_domain::organization::seats::check_activation(used, 1, limit)?;
    }
    // O papel NÃO se escreve aqui: `role` é derivado de `role_id` (0060). Uma
    // pertença nova recebe o papel de sistema pelo gatilho; uma existente muda
    // por `set_system_role` a seguir.
    sqlx::query(
        "INSERT INTO org_members (org_id, user_id, branch_id, role, title) VALUES ($1, $2, $3, $4, $5)
         ON CONFLICT (org_id, user_id) DO UPDATE SET branch_id = $3, title = $5",
    )
    .bind(org_id)
    .bind(user_id)
    .bind(req.branch_id)
    .bind(role)
    .bind(req.title.trim())
    .execute(&mut *tx)
    .await
    .map_err(map_member_write_error)?;
    tx.commit().await.map_err(map_member_write_error)?;
    // Reenviar a mesma pessoa mudava o papel (`DO UPDATE SET role`): mantém-se,
    // mas só quando o pedido diz qual — um reenvio sem `role` já não despromove.
    if existing_member.is_some() && req.role.is_some() {
        set_system_role(&state, org_id, user_id, wanted).await?;
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
    get, path = "/api/orgs/{org_id}/members", tag = "orgs",
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
    patch, path = "/api/orgs/{org_id}/members/{user_id}", tag = "orgs",
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
    require_capability(
        &state,
        org_id,
        auth.user_id,
        Capability::AdminManageAccounts,
        ResourceScope::Organization,
    )
    .await?;
    if let Some(role) = &req.role {
        if !matches!(role.as_str(), "admin" | "member") {
            return Err(ApiError::BadRequest("role inválido".into()));
        }
    }
    if let Some(role) = &req.role {
        let wanted = if role == "admin" {
            SystemRole::Admin
        } else {
            SystemRole::Member
        };
        // Só quem é membro desta org; o 404 do fim continua a ser a resposta a um estranho.
        if role_in_org(&state, org_id, user_id).await?.is_some() {
            crate::roles::ensure_can_assign_system(&state, org_id, auth.user_id, wanted).await?;
            set_system_role(&state, org_id, user_id, wanted).await?;
        }
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

/// Arquiva o acesso de um membro (soft delete, só admin). Um utilizador que
/// não é membro activo desta organização dá `404`.
#[utoipa::path(
    delete, path = "/api/orgs/{org_id}/members/{user_id}", tag = "orgs",
    security(("session" = [])),
    params(("org_id" = Uuid, Path, description = "Organização."), ("user_id" = Uuid, Path, description = "Utilizador membro.")),
    responses(
        (status = 204, description = "Acesso arquivado."),
        (status = 400, description = "Tentativa de arquivar o próprio acesso.", body = crate::openapi::ErrorBody),
        (status = 401, description = "Sem sessão.", body = crate::openapi::ErrorBody),
        (status = 403, description = "Membro sem papel de admin.", body = crate::openapi::ErrorBody),
        (status = 404, description = "A organização não existe, quem pede não é membro activo, ou o utilizador não é membro activo desta organização.", body = crate::openapi::ErrorBody),
    )
)]
pub async fn remove_employee(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path((org_id, user_id)): Path<(Uuid, Uuid)>,
) -> Result<axum::http::StatusCode, ApiError> {
    require_capability(
        &state,
        org_id,
        auth.user_id,
        Capability::AdminManageAccounts,
        ResourceScope::Organization,
    )
    .await?;
    if user_id == auth.user_id {
        return Err(ApiError::BadRequest(
            "não podes arquivar o teu próprio acesso".into(),
        ));
    }
    // Soft delete com razão `removed` (volta por convite, não por reactivar) e
    // as regras do último dono e das aprovações (ADR-0008 §5–§7). Contrato do
    // #90: quem não é membro activo → 404.
    let mut tx = state.db.begin().await?;
    if !archive_member_tx(&mut tx, org_id, user_id, "removed", Some(auth.user_id)).await? {
        return Err(ApiError::NotFound);
    }
    tx.commit().await.map_err(map_member_write_error)?;
    crate::audit::log(
        &state.db,
        Some(org_id),
        auth.user_id,
        "member.removed",
        &user_id.to_string(),
    )
    .await;
    Ok(axum::http::StatusCode::NO_CONTENT)
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

    let (recordings_total, recordings_bytes): (i64, i64) = sqlx::query_as(&format!(
        "SELECT COUNT(*), COALESCE(SUM(r.size_bytes), 0)::bigint FROM recordings r WHERE {}",
        recording_uploader_in_org_sql("$1", "r.uploader_id")
    ))
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

/// A gravação carregada por `uploader` conta para a organização `org`: quem a
/// carregou é, ou FOI, membro dela. Predicado SQL com as expressões dadas
/// (parâmetros `$n` ou colunas).
///
/// Não filtra `archived_at`, e de propósito: é atribuição, não acesso. A
/// gravação de quem saiu continua da empresa (S3) — continua nas estatísticas
/// e continua a ocupar a quota de armazenamento (G3). Um só dono para as duas
/// contas, para o painel e a quota nunca darem números diferentes.
pub(crate) fn recording_uploader_in_org_sql(org: &str, uploader: &str) -> String {
    format!(
        "EXISTS (SELECT 1 FROM org_members om WHERE om.org_id = {org} AND om.user_id = {uploader})"
    )
}

/// A quarentena de `subject` conta para a organização `org`: é, ou FOI,
/// membro dela. Mesma razão que `recording_uploader_in_org_sql` — é
/// atribuição, não acesso: quem saiu continua no histórico da empresa. A
/// leitura da analítica e a varredura que a precede usam ESTE predicado, para
/// a varredura marcar exactamente o conjunto que a leitura conta.
pub(crate) fn quarantine_subject_in_org_sql(org: &str, subject: &str) -> String {
    format!(
        "EXISTS (SELECT 1 FROM org_members om WHERE om.org_id = {org} AND om.user_id = {subject})"
    )
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

/// Cargo (`org_members.title`) de `user_id` numa organização que partilha,
/// como membro ACTIVO, com `owner_id` (o dono da sala). É o que a sala mostra
/// ao lado do nome: o cargo só aparece a quem está na mesma organização, nunca
/// o de outra empresa. Com várias orgs em comum, a primeira do dono — estável.
pub(crate) async fn title_alongside(
    state: &AppState,
    owner_id: Uuid,
    user_id: Uuid,
) -> Option<String> {
    sqlx::query_scalar::<_, String>(
        "SELECT b.title FROM org_members a
         JOIN org_members b ON a.org_id = b.org_id
         WHERE a.user_id = $1 AND b.user_id = $2
           AND a.archived_at IS NULL AND b.archived_at IS NULL
           AND b.title <> ''
         ORDER BY a.created_at, a.org_id
         LIMIT 1",
    )
    .bind(owner_id)
    .bind(user_id)
    .fetch_optional(&state.db)
    .await
    .ok()
    .flatten()
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
    /// Há `client_secret` guardado? (o segredo nunca é devolvido).
    pub has_client_secret: bool,
}

#[derive(Deserialize, utoipa::ToSchema)]
pub struct SsoConfigReq {
    pub issuer_url: String,
    pub client_id: String,
    /// Vazio → mantém o segredo existente (no-op em update sem nova rotação).
    /// Guarda-se cifrado; não vazio sem `DATA_ENCRYPTION_KEYS` → `422`.
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
        "SELECT org_id, issuer_url, client_id, enforce_sso,
                client_secret <> '' AS has_client_secret
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
        (status = 200, body = SsoConfigPublic, description = "A configuração como ficou gravada (a mesma forma do `GET`, sem o segredo)."),
        (status = 400, description = "`issuer_url`/`client_id` em falta, ou `issuer_url` sem `https://`, ou a apontar para um endereço interno (guarda de saída, `OUTBOUND_ALLOW_HOSTS`).", body = crate::openapi::ErrorBody),
        (status = 401, description = "Sem sessão.", body = crate::openapi::ErrorBody),
        (status = 403, description = "Membro sem papel de admin.", body = crate::openapi::ErrorBody),
        (status = 404, description = "A organização não existe ou quem pede não é membro activo.", body = crate::openapi::ErrorBody),
        (status = 422, description = "`client_secret` não vazio sem DATA_ENCRYPTION_KEYS (`secrets.encryption_unconfigured`).", body = crate::openapi::ErrorBody),
    )
)]
pub async fn upsert_sso_config(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(org_id): Path<Uuid>,
    Json(req): Json<SsoConfigReq>,
) -> Result<Json<Option<SsoConfigPublic>>, ApiError> {
    require_admin(&state, org_id, auth.user_id).await?;

    let issuer = req.issuer_url.trim().to_string();
    let client_id = req.client_id.trim().to_string();
    if issuer.is_empty() || client_id.is_empty() {
        return Err(ApiError::BadRequest(
            "issuer_url e client_id são obrigatórios".into(),
        ));
    }
    if !issuer.starts_with("https://") {
        return Err(ApiError::BadRequest(
            "issuer_url deve começar por https://".into(),
        ));
    }
    // O servidor vai buscar a descoberta OIDC a este URL: guarda anti-SSRF já
    // ao gravar, para o erro chegar a quem configura e não a quem tenta entrar.
    state.outbound.check_tenant_config_url(&issuer).await?;

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
        .bind(seal_sso_client_secret(
            &state.config,
            org_id,
            req.client_secret.trim(),
        )?)
        .bind(req.enforce_sso)
        .execute(&state.db)
        .await?;
    }

    tracing::info!(%org_id, %issuer, "SSO config upserted");
    get_sso_config(State(state), auth, Path(org_id)).await
}

/// `aad` do `client_secret` do SSO de uma org (a linha é a org).
pub(crate) fn sso_client_secret_aad(org_id: Uuid) -> String {
    crate::secrets_at_rest::aad("org_sso_configs", "client_secret", org_id)
}

/// O `client_secret` cifrado para gravar em `org_sso_configs` (S5). Quem
/// escreve essa coluna passa por aqui; sem `DATA_ENCRYPTION_KEYS` é `422`.
pub(crate) fn seal_sso_client_secret(
    config: &crate::config::Config,
    org_id: Uuid,
    plain: &str,
) -> Result<String, ApiError> {
    crate::secrets_at_rest::seal(config, plain, &sso_client_secret_aad(org_id))
}

/// `DELETE /api/orgs/:org_id/sso` — remove a config OIDC (desativa SSO). Só
/// admin.
#[utoipa::path(
    delete, path = "/api/orgs/{org_id}/sso", tag = "orgs",
    security(("session" = [])),
    params(("org_id" = Uuid, Path, description = "Organização.")),
    responses(
        (status = 204, description = "SSO desligado."),
        (status = 401, description = "Sem sessão.", body = crate::openapi::ErrorBody),
        (status = 403, description = "Membro sem papel de admin.", body = crate::openapi::ErrorBody),
        (status = 404, description = "A organização não existe, quem pede não é membro activo, ou não havia SSO configurado.", body = crate::openapi::ErrorBody),
    )
)]
pub async fn delete_sso_config(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(org_id): Path<Uuid>,
) -> Result<axum::http::StatusCode, ApiError> {
    require_admin(&state, org_id, auth.user_id).await?;
    let res = sqlx::query("DELETE FROM org_sso_configs WHERE org_id = $1")
        .bind(org_id)
        .execute(&state.db)
        .await?;
    if res.rows_affected() == 0 {
        return Err(ApiError::NotFound);
    }
    tracing::info!(%org_id, "SSO config deleted");
    Ok(axum::http::StatusCode::NO_CONTENT)
}

pub async fn group_member_ids(state: &AppState, group_id: Uuid) -> Result<Vec<Uuid>, ApiError> {
    let rows: Vec<(Uuid,)> =
        sqlx::query_as("SELECT user_id FROM group_members WHERE group_id = $1")
            .bind(group_id)
            .fetch_all(&state.db)
            .await?;
    Ok(rows.into_iter().map(|r| r.0).collect())
}

// ═══════════════════════════════════════════════════════════════════════════
//  Autorização por capacidades (ADR-0008 §4) — o ÚNICO ponto de imposição.
//
//  Tudo o que lê ou escreve `org_members` para decidir ou mudar papéis, estado
//  ou departamento está AQUI (catraca `pertenca_org_fora_de_org_rs`). Os módulos
//  `roles`, `directory` e `approvals` chamam estas funções.
// ═══════════════════════════════════════════════════════════════════════════

use delonix_meet_core::DomainError;
use delonix_meet_domain::identity::authorization::{
    self as authz, Capability, Decision, ResourceScope, SystemRole,
};

/// O utilizador de serviço que «possui» as orgs provisionadas. Nunca é dono,
/// nunca ocupa lugar, nunca é atribuível (ADR-0008 §3).
pub(crate) const SERVICE_ACCOUNT_EMAIL: &str = "provisioning@delonix.internal";

/// O que `require_capability` devolve a quem passou.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Grant {
    pub role_id: Uuid,
    pub system: Option<SystemRole>,
    pub department_id: Option<Uuid>,
}

/// A acção concreta que se autoriza — o que liga um pedido de aprovação ao
/// alvo (ADR-0008 §6).
pub(crate) struct Action<'a> {
    pub name: &'a str,
    pub target: serde_json::Value,
}

#[derive(sqlx::FromRow)]
struct AuthzRow {
    role_id: Uuid,
    system_key: Option<String>,
    department_id: Option<Uuid>,
    org_decision: Option<String>,
    dept_decision: Option<String>,
}

fn parse_decision(raw: Option<&str>) -> Decision {
    match raw {
        Some("allow") => Decision::Allow,
        Some("requires_approval") => Decision::RequiresApproval,
        _ => Decision::Deny,
    }
}

/// Decisão para `user_id` em `org_id`. `None` = não é membro activo.
///
/// UMA query: a pertença activa (a mesma regra de `role_in_org`) e a decisão
/// efectiva materializada pela policy do domínio (`org_role_effective_capabilities`).
pub(crate) async fn decide(
    state: &AppState,
    org_id: Uuid,
    user_id: Uuid,
    cap: Capability,
    scope: ResourceScope,
) -> Result<Option<(Grant, Decision)>, ApiError> {
    let row: Option<AuthzRow> = sqlx::query_as(
        "SELECT m.role_id, r.system_key, m.department_id, e.org_decision, e.dept_decision
           FROM org_members m
           JOIN org_roles r ON r.id = m.role_id
      LEFT JOIN org_role_effective_capabilities e
             ON e.role_id = m.role_id AND e.capability = $3
          WHERE m.org_id = $1 AND m.user_id = $2 AND m.archived_at IS NULL",
    )
    .bind(org_id)
    .bind(user_id)
    .bind(cap.as_str())
    .fetch_optional(&state.db)
    .await?;
    Ok(row.map(|r| {
        let in_own_department = match scope {
            ResourceScope::Organization => false,
            ResourceScope::Department { department_id } => r.department_id == Some(department_id),
        };
        let decision = if in_own_department {
            parse_decision(r.dept_decision.as_deref())
        } else {
            parse_decision(r.org_decision.as_deref())
        };
        (
            Grant {
                role_id: r.role_id,
                system: r.system_key.as_deref().and_then(SystemRole::parse),
                department_id: r.department_id,
            },
            decision,
        )
    }))
}

fn missing_capability(cap: Capability) -> ApiError {
    DomainError::forbidden("authz.missing_capability")
        .with_message(format!("falta a capacidade {}", cap.as_str()))
        .with_field("capability", cap.as_str())
        .into()
}

/// O ponto de imposição (ADR-0008 §4).
///
/// - não é membro activo (ou a org não existe) → `404`;
/// - sem a capacidade → `403 authz.missing_capability`;
/// - `requires_approval` → consome uma aprovação válida para ESTA acção e alvo,
///   ou cria/reutiliza o pedido e responde `403 authz.approval_required`.
pub(crate) async fn require_capability(
    state: &AppState,
    org_id: Uuid,
    user_id: Uuid,
    cap: Capability,
    scope: ResourceScope,
) -> Result<Grant, ApiError> {
    let action = Action {
        name: cap.as_str(),
        target: serde_json::json!({ "org_id": org_id }),
    };
    require_capability_for(state, org_id, user_id, cap, scope, &action).await
}

pub(crate) async fn require_capability_for(
    state: &AppState,
    org_id: Uuid,
    user_id: Uuid,
    cap: Capability,
    scope: ResourceScope,
    action: &Action<'_>,
) -> Result<Grant, ApiError> {
    match decide(state, org_id, user_id, cap, scope).await? {
        None => Err(ApiError::NotFound),
        Some((grant, Decision::Allow)) => Ok(grant),
        Some((_, Decision::Deny)) => Err(missing_capability(cap)),
        Some((grant, Decision::RequiresApproval)) => {
            if crate::approvals::consume(state, org_id, user_id, cap, action).await? {
                return Ok(grant);
            }
            let request_id =
                crate::approvals::open_request(state, org_id, user_id, cap, action).await?;
            Err(DomainError::forbidden("authz.approval_required")
                .with_message(format!(
                    "{} requer aprovação — pedido {request_id} criado; repita depois de aprovado",
                    cap.as_str()
                ))
                .with_field("approval_request_id", request_id.to_string())
                .into())
        }
    }
}

#[derive(sqlx::FromRow)]
pub(crate) struct MyCapabilityRow {
    pub role_id: Uuid,
    pub system_key: Option<String>,
    pub department_id: Option<Uuid>,
    pub capability: String,
    pub org_decision: String,
    pub dept_decision: String,
}

/// As capacidades `allow` de quem pede, nos dois âmbitos (para gatear botões e
/// para a regra da escalada). `None` = não é membro activo.
pub(crate) async fn my_capabilities(
    state: &AppState,
    org_id: Uuid,
    user_id: Uuid,
) -> Result<Option<(Grant, Vec<MyCapabilityRow>)>, ApiError> {
    let rows: Vec<MyCapabilityRow> = sqlx::query_as(
        "SELECT m.role_id, r.system_key, m.department_id, e.capability, e.org_decision, e.dept_decision
           FROM org_members m
           JOIN org_roles r ON r.id = m.role_id
           JOIN org_role_effective_capabilities e ON e.role_id = m.role_id
          WHERE m.org_id = $1 AND m.user_id = $2 AND m.archived_at IS NULL
          ORDER BY e.capability",
    )
    .bind(org_id)
    .bind(user_id)
    .fetch_all(&state.db)
    .await?;
    let Some(first) = rows.first() else {
        // Membro activo sem linhas efectivas não existe (semeadas por gatilho);
        // mas pertença sem linhas conta como pertença.
        return Ok(role_in_org(state, org_id, user_id).await?.map(|_| {
            (
                Grant {
                    role_id: Uuid::nil(),
                    system: None,
                    department_id: None,
                },
                Vec::new(),
            )
        }));
    };
    let grant = Grant {
        role_id: first.role_id,
        system: first.system_key.as_deref().and_then(SystemRole::parse),
        department_id: first.department_id,
    };
    Ok(Some((grant, rows)))
}

/// `sessions.create` (ADR-0008 §1): o poder de CRIAR uma sessão, avaliado sobre
/// quem fica DONO dela.
///
/// - `org = Some` (v1): na organização da chave; um dono que não é membro activo
///   dela é recusado como sem capacidade (a v1 já garante que o anfitrião é da org).
/// - `org = None` (BFF): em todas as pertenças activas — basta uma dar `allow`;
///   sem nenhuma pertença, é uma conta pessoal e cria.
pub(crate) async fn require_session_create(
    state: &AppState,
    owner_id: Uuid,
    org: Option<Uuid>,
) -> Result<(), ApiError> {
    let cap = Capability::SessionsCreate;
    let rows: Vec<(Uuid, Option<String>)> = sqlx::query_as(
        "SELECT m.org_id, e.org_decision
           FROM org_members m
      LEFT JOIN org_role_effective_capabilities e
             ON e.role_id = m.role_id AND e.capability = $2
          WHERE m.user_id = $1 AND m.archived_at IS NULL
            AND ($3::uuid IS NULL OR m.org_id = $3)
          ORDER BY m.created_at, m.org_id",
    )
    .bind(owner_id)
    .bind(cap.as_str())
    .bind(org)
    .fetch_all(&state.db)
    .await?;
    if rows.is_empty() {
        return match org {
            None => Ok(()),
            Some(_) => Err(missing_capability(cap)),
        };
    }
    if rows
        .iter()
        .any(|(_, d)| parse_decision(d.as_deref()) == Decision::Allow)
    {
        return Ok(());
    }
    // Nenhuma dá allow: se alguma pede aprovação, é por ela (a mais antiga).
    if let Some((org_id, _)) = rows
        .iter()
        .find(|(_, d)| parse_decision(d.as_deref()) == Decision::RequiresApproval)
    {
        require_capability(state, *org_id, owner_id, cap, ResourceScope::Organization).await?;
        return Ok(());
    }
    Err(missing_capability(cap))
}

/// `actor` tem `cap` (âmbito organização) numa organização ACTIVA de que
/// `subject_user` é ou foi membro? Para poderes sobre recursos de OUTROS
/// (`recordings.publish`). Só `allow` conta.
pub(crate) async fn has_capability_over_colleague(
    state: &AppState,
    actor: Uuid,
    subject_user: Uuid,
    cap: Capability,
) -> Result<bool, ApiError> {
    let ok: bool = sqlx::query_scalar(
        "SELECT EXISTS (
            SELECT 1 FROM org_members me
              JOIN org_members o ON o.org_id = me.org_id
              JOIN org_role_effective_capabilities e
                ON e.role_id = me.role_id AND e.capability = $3 AND e.org_decision = 'allow'
             WHERE me.user_id = $1 AND me.archived_at IS NULL AND o.user_id = $2)",
    )
    .bind(actor)
    .bind(subject_user)
    .bind(cap.as_str())
    .fetch_one(&state.db)
    .await?;
    Ok(ok)
}

/// Limite efectivo de destinos em simultâneo do papel (`None` = sem limite).
pub(crate) async fn role_destination_limit(
    state: &AppState,
    role_id: Uuid,
) -> Result<Option<i32>, ApiError> {
    Ok(
        sqlx::query_scalar("SELECT eff_max_simultaneous_destinations FROM org_roles WHERE id = $1")
            .bind(role_id)
            .fetch_optional(&state.db)
            .await?
            .flatten(),
    )
}

/// Traduz as recusas dos gatilhos da 0060 para o envelope (409), em vez de 500.
pub(crate) fn map_member_write_error(e: sqlx::Error) -> ApiError {
    if let sqlx::Error::Database(db) = &e {
        let msg = db.message();
        if msg.starts_with("role.last_owner") {
            return DomainError::conflict(
                "role.last_owner",
                "a organização ficava sem Proprietário",
            )
            .into();
        }
        if msg.starts_with("role.service_account") {
            return DomainError::precondition(
                "role.service_account",
                "o utilizador de serviço não é atribuível",
            )
            .into();
        }
    }
    e.into()
}

// ---------- estado da pertença (papel, departamento, suspensão) ----------

#[derive(Debug, Clone, sqlx::FromRow)]
pub(crate) struct MemberState {
    pub role_id: Uuid,
    pub system_key: Option<String>,
    pub role_source: String,
    pub department_id: Option<Uuid>,
    pub archived_at: Option<DateTime<Utc>>,
    pub archived_reason: Option<String>,
    pub service_account: bool,
    pub odoo_managed_here: bool,
}

pub(crate) async fn member_state<'e, E>(
    db: E,
    org_id: Uuid,
    user_id: Uuid,
) -> Result<Option<MemberState>, ApiError>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    Ok(sqlx::query_as(
        "SELECT m.role_id, r.system_key, m.role_source, m.department_id,
                m.archived_at, m.archived_reason,
                (u.email = $3) AS service_account,
                (u.odoo_org_id IS NOT DISTINCT FROM m.org_id AND u.odoo_managed) AS odoo_managed_here
           FROM org_members m
           JOIN org_roles r ON r.id = m.role_id
           JOIN users u ON u.id = m.user_id
          WHERE m.org_id = $1 AND m.user_id = $2
          FOR UPDATE OF m",
    )
    .bind(org_id)
    .bind(user_id)
    .bind(SERVICE_ACCOUNT_EMAIL)
    .fetch_optional(db)
    .await?)
}

async fn active_owner_count(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    org_id: Uuid,
) -> Result<i64, ApiError> {
    Ok(sqlx::query_scalar(
        "SELECT COUNT(*) FROM org_members m JOIN org_roles r ON r.id = m.role_id
          WHERE m.org_id = $1 AND m.archived_at IS NULL AND r.system_key = 'owner'",
    )
    .bind(org_id)
    .fetch_one(&mut **tx)
    .await?)
}

/// Muda o papel de uma pertença ACTIVA, com as regras do ADR-0008 §5 (último
/// dono, utilizador de serviço) e a invalidação das aprovações de quem muda.
/// A regra da escalada é de quem chama (precisa do actor). `Ok(false)` = já
/// tinha o papel.
pub(crate) async fn set_member_role_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    org_id: Uuid,
    user_id: Uuid,
    role_id: Uuid,
    source: &str,
) -> Result<bool, ApiError> {
    let Some(cur) = member_state(&mut **tx, org_id, user_id).await? else {
        return Err(DomainError::not_found("member.not_found").into());
    };
    if cur.archived_at.is_some() {
        return Err(DomainError::precondition(
            "member.not_active",
            "a pessoa não é membro activo — reactive-a primeiro",
        )
        .into());
    }
    let target_key: Option<Option<String>> =
        sqlx::query_scalar("SELECT system_key FROM org_roles WHERE id = $1 AND org_id = $2")
            .bind(role_id)
            .bind(org_id)
            .fetch_optional(&mut **tx)
            .await?;
    let Some(target_key) = target_key else {
        return Err(DomainError::not_found("role.not_found").into());
    };
    if cur.service_account {
        return Err(DomainError::precondition(
            "role.service_account",
            "o utilizador de serviço não é atribuível",
        )
        .into());
    }
    if cur.role_id == role_id {
        if cur.role_source != source {
            sqlx::query(
                "UPDATE org_members SET role_source = $3 WHERE org_id = $1 AND user_id = $2",
            )
            .bind(org_id)
            .bind(user_id)
            .bind(source)
            .execute(&mut **tx)
            .await?;
        }
        return Ok(false);
    }
    let is_owner = cur.system_key.as_deref() == Some("owner");
    authz::check_last_owner(
        active_owner_count(tx, org_id).await?,
        is_owner,
        target_key.as_deref() == Some("owner"),
    )?;
    sqlx::query(
        "UPDATE org_members SET role_id = $3, role_source = $4 WHERE org_id = $1 AND user_id = $2",
    )
    .bind(org_id)
    .bind(user_id)
    .bind(role_id)
    .bind(source)
    .execute(&mut **tx)
    .await
    .map_err(map_member_write_error)?;
    crate::approvals::invalidate_for(tx, org_id, user_id).await?;
    Ok(true)
}

/// Papel de SISTEMA para os escritores herdados (`add_employee`,
/// `update_employee`, a sincronização do Odoo). É a única escrita de papel fora
/// dos caminhos novos — o teste `escritas_de_papel_so_em_set_system_role` varre
/// `src/` para o garantir.
pub(crate) async fn set_system_role(
    state: &AppState,
    org_id: Uuid,
    user_id: Uuid,
    role: SystemRole,
) -> Result<bool, ApiError> {
    let mut tx = state.db.begin().await?;
    let role_id: Uuid =
        sqlx::query_scalar("SELECT id FROM org_roles WHERE org_id = $1 AND system_key = $2")
            .bind(org_id)
            .bind(role.key())
            .fetch_one(&mut *tx)
            .await?;
    let changed = set_member_role_tx(&mut tx, org_id, user_id, role_id, "manual").await?;
    tx.commit().await.map_err(map_member_write_error)?;
    Ok(changed)
}

/// Razões que se podem reactivar (ADR-0008 §7).
pub(crate) const REACTIVATABLE: [&str; 3] = ["suspended", "inactive", "guest_expired"];

/// Arquiva (suspende) uma pertença activa, com a razão. Último dono e aprovações.
pub(crate) async fn archive_member_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    org_id: Uuid,
    user_id: Uuid,
    reason: &str,
    actor: Option<Uuid>,
) -> Result<bool, ApiError> {
    let Some(cur) = member_state(&mut **tx, org_id, user_id).await? else {
        return Err(DomainError::not_found("member.not_found").into());
    };
    if cur.archived_at.is_some() {
        return Ok(false);
    }
    authz::check_last_owner(
        active_owner_count(tx, org_id).await?,
        cur.system_key.as_deref() == Some("owner"),
        false,
    )?;
    sqlx::query(
        "UPDATE org_members SET archived_at = now(), archived_by = $3, archived_reason = $4
          WHERE org_id = $1 AND user_id = $2 AND archived_at IS NULL",
    )
    .bind(org_id)
    .bind(user_id)
    .bind(actor)
    .bind(reason)
    .execute(&mut **tx)
    .await
    .map_err(map_member_write_error)?;
    crate::approvals::invalidate_for(tx, org_id, user_id).await?;
    Ok(true)
}

/// Lugares: tecto e uso MEDIDO (ADR-0008 §7). Bloqueia a linha da org quando
/// `lock` — duas activações concorrentes serializam-se aqui.
pub(crate) async fn seat_usage_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    org_id: Uuid,
    lock: bool,
) -> Result<(i64, Option<i64>), ApiError> {
    let limit: Option<Option<i64>> = sqlx::query_scalar(if lock {
        "SELECT max_seats FROM organizations WHERE id = $1 FOR UPDATE"
    } else {
        "SELECT max_seats FROM organizations WHERE id = $1"
    })
    .bind(org_id)
    .fetch_optional(&mut **tx)
    .await?;
    let Some(limit) = limit else {
        return Err(ApiError::NotFound);
    };
    let used: i64 = sqlx::query_scalar(&format!(
        "SELECT COUNT(*) FROM org_members m {SEAT_JOIN} WHERE m.org_id = $1 AND {SEAT_OCCUPIED}"
    ))
    .bind(org_id)
    .bind(SERVICE_ACCOUNT_EMAIL)
    .fetch_one(&mut **tx)
    .await?;
    Ok((used, limit))
}

/// Quem ocupa lugar: activo, humano, e não convidado externo. `$2` = email do serviço.
const SEAT_JOIN: &str = "JOIN users su ON su.id = m.user_id JOIN org_roles sr ON sr.id = m.role_id";
const SEAT_OCCUPIED: &str = "m.archived_at IS NULL AND su.email <> $2 \
     AND sr.system_key IS DISTINCT FROM 'external_guest'";

/// Reactiva uma pertença suspensa (só de razões reactiváveis), com lugares.
pub(crate) async fn reactivate_member_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    org_id: Uuid,
    user_id: Uuid,
) -> Result<bool, ApiError> {
    let Some(cur) = member_state(&mut **tx, org_id, user_id).await? else {
        return Err(DomainError::not_found("member.not_found").into());
    };
    if cur.archived_at.is_none() {
        return Ok(false);
    }
    let reason = cur.archived_reason.as_deref().unwrap_or("removed");
    if !REACTIVATABLE.contains(&reason) {
        return Err(DomainError::precondition(
            "member.not_reactivatable",
            format!(
                "uma saída «{reason}» volta por convite ou pela sincronização, não por reactivar"
            ),
        )
        .into());
    }
    let (used, limit) = seat_usage_tx(tx, org_id, true).await?;
    let occupies = cur.system_key.as_deref() != Some("external_guest");
    delonix_meet_domain::organization::seats::check_activation(used, i64::from(occupies), limit)?;
    sqlx::query(
        "UPDATE org_members SET archived_at = NULL, archived_by = NULL, archived_reason = NULL
          WHERE org_id = $1 AND user_id = $2",
    )
    .bind(org_id)
    .bind(user_id)
    .execute(&mut **tx)
    .await
    .map_err(map_member_write_error)?;
    crate::approvals::invalidate_for(tx, org_id, user_id).await?;
    Ok(true)
}

/// Junta (ou devolve) alguém à org por convite, com papel, departamento e
/// lugares. Uma pertença arquivada é reactivada com o papel do convite.
pub(crate) async fn activate_membership_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    org_id: Uuid,
    user_id: Uuid,
    role_id: Uuid,
    department_id: Option<Uuid>,
    origin: &str,
    access_expires_at: Option<DateTime<Utc>>,
) -> Result<(), ApiError> {
    let foreign: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM org_members
                         WHERE user_id = $1 AND org_id <> $2 AND archived_at IS NULL)",
    )
    .bind(user_id)
    .bind(org_id)
    .fetch_one(&mut **tx)
    .await?;
    let role_key: Option<String> =
        sqlx::query_scalar("SELECT system_key FROM org_roles WHERE id = $1")
            .bind(role_id)
            .fetch_one(&mut **tx)
            .await?;
    // A mesma regra do `add_employee` (R122): uma conta activa noutra org não é
    // capturada — excepto como convidado externo, que é exactamente isso.
    if foreign && role_key.as_deref() != Some("external_guest") {
        return Err(DomainError::conflict(
            "invitation.foreign_org",
            "a conta já pertence a outra organização",
        )
        .into());
    }
    let cur = member_state(&mut **tx, org_id, user_id).await?;
    if cur.as_ref().is_some_and(|c| c.archived_at.is_none()) {
        return Err(
            DomainError::conflict("invitation.already_member", "a pessoa já é membro").into(),
        );
    }
    let (used, limit) = seat_usage_tx(tx, org_id, true).await?;
    delonix_meet_domain::organization::seats::check_activation(
        used,
        i64::from(role_key.as_deref() != Some("external_guest")),
        limit,
    )?;
    match cur {
        None => {
            sqlx::query(
                "INSERT INTO org_members (org_id, user_id, role, role_id, department_id, origin, access_expires_at)
                 VALUES ($1, $2, 'member', $3, $4, $5, $6)",
            )
            .bind(org_id)
            .bind(user_id)
            .bind(role_id)
            .bind(department_id)
            .bind(origin)
            .bind(access_expires_at)
            .execute(&mut **tx)
            .await
            .map_err(map_member_write_error)?;
        }
        Some(_) => {
            sqlx::query(
                "UPDATE org_members
                    SET archived_at = NULL, archived_by = NULL, archived_reason = NULL,
                        role_id = $3, role_source = 'manual', department_id = $4,
                        origin = $5, access_expires_at = $6
                  WHERE org_id = $1 AND user_id = $2",
            )
            .bind(org_id)
            .bind(user_id)
            .bind(role_id)
            .bind(department_id)
            .bind(origin)
            .bind(access_expires_at)
            .execute(&mut **tx)
            .await
            .map_err(map_member_write_error)?;
            crate::approvals::invalidate_for(tx, org_id, user_id).await?;
        }
    }
    Ok(())
}

pub(crate) async fn set_member_department_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    org_id: Uuid,
    user_id: Uuid,
    department_id: Option<Uuid>,
) -> Result<bool, ApiError> {
    let Some(cur) = member_state(&mut **tx, org_id, user_id).await? else {
        return Err(DomainError::not_found("member.not_found").into());
    };
    if cur.department_id == department_id {
        return Ok(false);
    }
    sqlx::query("UPDATE org_members SET department_id = $3 WHERE org_id = $1 AND user_id = $2")
        .bind(org_id)
        .bind(user_id)
        .bind(department_id)
        .execute(&mut **tx)
        .await?;
    // O âmbito da pessoa mudou: as aprovações que tinha deixam de valer.
    crate::approvals::invalidate_for(tx, org_id, user_id).await?;
    Ok(true)
}

/// Pessoas activas por papel (contagem do ecrã).
pub(crate) async fn member_counts_by_role(
    db: &sqlx::PgPool,
    org_id: Uuid,
) -> Result<std::collections::HashMap<Uuid, i64>, ApiError> {
    let rows: Vec<(Uuid, i64)> = sqlx::query_as(
        "SELECT role_id, COUNT(*) FROM org_members
          WHERE org_id = $1 AND archived_at IS NULL GROUP BY role_id",
    )
    .bind(org_id)
    .fetch_all(db)
    .await?;
    Ok(rows.into_iter().collect())
}

/// Passa toda a gente de `from` para `to` (eliminar papel, ADR-0008 §3).
pub(crate) async fn reassign_role_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    org_id: Uuid,
    from: Uuid,
    to: Uuid,
) -> Result<Vec<Uuid>, ApiError> {
    let moved: Vec<Uuid> = sqlx::query_scalar(
        "UPDATE org_members SET role_id = $3, role_source = 'manual'
          WHERE org_id = $1 AND role_id = $2 RETURNING user_id",
    )
    .bind(org_id)
    .bind(from)
    .bind(to)
    .fetch_all(&mut **tx)
    .await
    .map_err(map_member_write_error)?;
    for u in &moved {
        crate::approvals::invalidate_for(tx, org_id, *u).await?;
    }
    Ok(moved)
}

/// Pertenças activas humanas com papel e departamento (SoD, simulação).
pub(crate) async fn active_member_subjects(
    db: &sqlx::PgPool,
    org_id: Uuid,
    role_id: Option<Uuid>,
) -> Result<Vec<(Uuid, String, String, Uuid, Option<Uuid>)>, ApiError> {
    Ok(sqlx::query_as(
        "SELECT u.id, u.username, u.email, m.role_id, m.department_id
           FROM org_members m JOIN users u ON u.id = m.user_id
          WHERE m.org_id = $1 AND m.archived_at IS NULL AND u.email <> $2
            AND ($3::uuid IS NULL OR m.role_id = $3)
          ORDER BY u.username",
    )
    .bind(org_id)
    .bind(SERVICE_ACCOUNT_EMAIL)
    .bind(role_id)
    .fetch_all(db)
    .await?)
}

/// A org tem humanos activos e nenhum dono activo (ADR-0008 §5).
pub(crate) async fn owner_missing(db: &sqlx::PgPool, org_id: Uuid) -> Result<bool, ApiError> {
    Ok(sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM org_members m JOIN users u ON u.id = m.user_id
                         WHERE m.org_id = $1 AND m.archived_at IS NULL AND u.email <> $2)
            AND NOT EXISTS (SELECT 1 FROM org_members m JOIN org_roles r ON r.id = m.role_id
                             WHERE m.org_id = $1 AND m.archived_at IS NULL AND r.system_key = 'owner')",
    )
    .bind(org_id)
    .bind(SERVICE_ACCOUNT_EMAIL)
    .fetch_one(db)
    .await?)
}

/// Resumo de lugares (ADR-0008 §7).
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct SeatSummary {
    /// Tecto do operador; `null` = sem tecto.
    pub limit: Option<i64>,
    /// Pessoas activas que ocupam lugar (sem o utilizador de serviço nem convidados externos).
    pub used: i64,
    pub available: Option<i64>,
    /// Ocupantes com entrada desde o início do mês (fuso `Africa/Luanda`).
    pub active_this_month: i64,
    /// Dias usados para `inactive`.
    pub inactive_days: i64,
    /// Ocupantes sem entrar há `inactive_days` (inclui quem nunca entrou e existe há mais do que isso).
    pub inactive: i64,
    pub owner_missing: bool,
}

pub(crate) async fn seat_summary(
    state: &AppState,
    org_id: Uuid,
    inactive_days: i64,
) -> Result<SeatSummary, ApiError> {
    let mut tx = state.db.begin().await?;
    let (used, limit) = seat_usage_tx(&mut tx, org_id, false).await?;
    let (active_this_month, inactive): (i64, i64) = sqlx::query_as(&format!(
        "SELECT
            COUNT(*) FILTER (WHERE su.last_access_at >=
                 (date_trunc('month', now() AT TIME ZONE 'Africa/Luanda') AT TIME ZONE 'Africa/Luanda')),
            COUNT(*) FILTER (WHERE COALESCE(su.last_access_at, m.created_at) < now() - make_interval(days => $3))
           FROM org_members m {SEAT_JOIN}
          WHERE m.org_id = $1 AND {SEAT_OCCUPIED}"
    ))
    .bind(org_id)
    .bind(SERVICE_ACCOUNT_EMAIL)
    .bind(inactive_days as i32)
    .fetch_one(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(SeatSummary {
        limit,
        used,
        available: delonix_meet_domain::organization::seats::remaining(used, limit),
        active_this_month,
        inactive_days,
        inactive,
        owner_missing: owner_missing(&state.db, org_id).await?,
    })
}

/// Ocupantes inactivos há `days` (candidatos a «libertar lugares»), sem donos.
pub(crate) async fn inactive_occupants(
    db: &sqlx::PgPool,
    org_id: Uuid,
    days: i64,
) -> Result<Vec<Uuid>, ApiError> {
    Ok(sqlx::query_scalar(&format!(
        "SELECT m.user_id FROM org_members m {SEAT_JOIN}
          WHERE m.org_id = $1 AND {SEAT_OCCUPIED}
            AND sr.system_key IS DISTINCT FROM 'owner'
            AND COALESCE(su.last_access_at, m.created_at) < now() - make_interval(days => $3)
          ORDER BY su.last_access_at NULLS FIRST, m.user_id"
    ))
    .bind(org_id)
    .bind(SERVICE_ACCOUNT_EMAIL)
    .bind(days as i32)
    .fetch_all(db)
    .await?)
}

/// Convidados externos cujo acesso expirou → arquivados com `guest_expired`.
/// Corre no sweeper de hora a hora.
pub(crate) async fn expire_guests(state: &AppState) -> Result<u64, ApiError> {
    let rows: Vec<(Uuid, Uuid)> = sqlx::query_as(
        "SELECT org_id, user_id FROM org_members
          WHERE archived_at IS NULL AND access_expires_at IS NOT NULL AND access_expires_at <= now()",
    )
    .fetch_all(&state.db)
    .await?;
    let mut n = 0;
    for (org, user) in rows {
        let mut tx = state.db.begin().await?;
        match archive_member_tx(&mut tx, org, user, "guest_expired", None).await {
            Ok(true) => {
                tx.commit().await.map_err(map_member_write_error)?;
                crate::audit::log(
                    &state.db,
                    Some(org),
                    Uuid::nil(),
                    "member.guest_expired",
                    &user.to_string(),
                )
                .await;
                n += 1;
            }
            Ok(false) => {}
            Err(e) => tracing::warn!(error = %e, %org, %user, "convidado expirado não arquivado"),
        }
    }
    Ok(n)
}

/// Pessoa → papel efectivo para a simulação: (role_id, departamento).
pub(crate) async fn member_subject(
    db: &sqlx::PgPool,
    org_id: Uuid,
    user_id: Uuid,
) -> Result<Option<(Uuid, Option<Uuid>)>, ApiError> {
    Ok(sqlx::query_as(
        "SELECT role_id, department_id FROM org_members
          WHERE org_id = $1 AND user_id = $2 AND archived_at IS NULL",
    )
    .bind(org_id)
    .bind(user_id)
    .fetch_optional(db)
    .await?)
}

/// Pessoas por papel com grupos Odoo lidos (para «14 de 14 sincronizados»):
/// (total com o grupo, dos quais têm o papel).
pub(crate) async fn odoo_group_sync_counts(
    db: &sqlx::PgPool,
    org_id: Uuid,
    group: &str,
    role_id: Uuid,
) -> Result<(i64, i64), ApiError> {
    Ok(sqlx::query_as(
        "SELECT COUNT(*), COUNT(*) FILTER (WHERE role_id = $3)
           FROM org_members
          WHERE org_id = $1 AND archived_at IS NULL AND $2 = ANY(odoo_groups)",
    )
    .bind(org_id)
    .bind(group)
    .bind(role_id)
    .fetch_one(db)
    .await?)
}

// ═══════════════════════════════════════════════════════════════════════════
//  Directório de pessoas (ecrã «Utilizadores e convites», ADR-0008 §7 e §11).
//  As leituras que juntam `org_members` com convites vivem aqui pela mesma
//  regra da catraca; o módulo `directory` monta os pedidos e as respostas.
// ═══════════════════════════════════════════════════════════════════════════

/// Uma linha do directório: um membro (activo ou suspenso) ou um convite pendente.
#[derive(Debug, Clone, Serialize, sqlx::FromRow, utoipa::ToSchema)]
pub struct DirectoryEntry {
    /// `member` | `invitation`.
    pub kind: String,
    /// `user_id` num membro, `invitation_id` num convite.
    pub id: Uuid,
    pub user_id: Option<Uuid>,
    pub invitation_id: Option<Uuid>,
    pub name: String,
    pub email: String,
    pub role_id: Uuid,
    pub role_name: String,
    pub role_key: Option<String>,
    pub department_id: Option<Uuid>,
    pub department_name: Option<String>,
    /// `odoo_sso` | `invitation` | `code` | `registration` | `sso` | `api` | `manual`.
    pub origin: String,
    /// `active` | `invited` | `suspended`.
    pub status: String,
    /// Razão da suspensão (`suspended` | `inactive` | `guest_expired`), só em `suspended`.
    pub suspension_reason: Option<String>,
    pub last_access_at: Option<DateTime<Utc>>,
    /// Convite: quando expira. Convidado externo activo: quando o acesso expira.
    pub expires_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

/// Filtros já validados pelo módulo `directory`.
#[derive(Debug, Default, Clone)]
pub(crate) struct DirectoryFilter {
    pub terms: Vec<String>,
    pub statuses: Vec<String>,
    pub origins: Vec<String>,
    /// Sem entrar há pelo menos N dias (inclui nunca).
    pub inactive_days: Option<i32>,
    pub never_signed_in: bool,
    pub external_only: bool,
    pub without_department: bool,
    pub department_id: Option<Uuid>,
    pub role_id: Option<Uuid>,
}

/// O CTE com todas as linhas do directório de `$1`. `$2` = email do serviço.
const DIRECTORY_CTE: &str = "WITH entries AS (
    SELECT 'member'::text AS kind, m.user_id AS id, m.user_id AS user_id, NULL::uuid AS invitation_id,
           u.username AS name, u.email, m.role_id, r.name AS role_name, r.system_key AS role_key,
           m.department_id, d.name AS department_name, m.origin,
           CASE WHEN m.archived_at IS NULL THEN 'active' ELSE 'suspended' END AS status,
           m.archived_reason AS suspension_reason,
           u.last_access_at, m.access_expires_at AS expires_at, m.created_at
      FROM org_members m
      JOIN users u ON u.id = m.user_id
      JOIN org_roles r ON r.id = m.role_id
 LEFT JOIN departments d ON d.id = m.department_id
     WHERE m.org_id = $1 AND u.email <> $2
       AND (m.archived_at IS NULL OR m.archived_reason IN ('suspended', 'inactive', 'guest_expired'))
    UNION ALL
    SELECT 'invitation', i.id, NULL, i.id, i.email, i.email, i.role_id, r.name, r.system_key,
           i.department_id, d.name, CASE WHEN i.delivery = 'code' THEN 'code' ELSE 'invitation' END,
           'invited', NULL, NULL, i.expires_at, i.created_at
      FROM org_invitations i
      JOIN org_roles r ON r.id = i.role_id
 LEFT JOIN departments d ON d.id = i.department_id
     WHERE i.org_id = $1 AND i.status = 'pending' AND i.expires_at > now()
)";

/// WHERE sobre `entries` a partir de `$3`. Devolve o SQL e os binds em ordem.
fn directory_where(f: &DirectoryFilter) -> String {
    // Todos os parâmetros entram SEMPRE (ordem fixa), para o SQL não variar em
    // número de binds: $3 termos, $4 estados, $5 origens, $6 dias, $7 nunca,
    // $8 externos, $9 sem departamento, $10 departamento, $11 papel.
    let _ = f;
    "WHERE (cardinality($3::text[]) = 0 OR NOT EXISTS (
               SELECT 1 FROM unnest($3::text[]) t
                WHERE position(t IN lower(e.name || ' ' || e.email || ' ' || e.role_name || ' ' ||
                                         COALESCE(e.department_name, ''))) = 0))
       AND (cardinality($4::text[]) = 0 OR e.status = ANY($4))
       AND (cardinality($5::text[]) = 0 OR e.origin = ANY($5))
       AND ($6::int IS NULL OR (e.kind = 'member' AND
             COALESCE(e.last_access_at, e.created_at) < now() - make_interval(days => $6)))
       AND (NOT $7 OR (e.kind = 'member' AND e.last_access_at IS NULL))
       AND (NOT $8 OR e.role_key = 'external_guest')
       AND (NOT $9 OR e.department_id IS NULL)
       AND ($10::uuid IS NULL OR e.department_id = $10)
       AND ($11::uuid IS NULL OR e.role_id = $11)"
        .to_string()
}

macro_rules! bind_directory {
    ($q:expr, $org:expr, $f:expr) => {
        $q.bind($org)
            .bind(SERVICE_ACCOUNT_EMAIL)
            .bind(&$f.terms)
            .bind(&$f.statuses)
            .bind(&$f.origins)
            .bind($f.inactive_days)
            .bind($f.never_signed_in)
            .bind($f.external_only)
            .bind($f.without_department)
            .bind($f.department_id)
            .bind($f.role_id)
    };
}

/// Ordem da lista do directório.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DirectoryOrder {
    NameAsc,
    NameDesc,
    LastAccessDesc,
    LastAccessAsc,
}

/// Cursor de keyset do directório.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct DirectoryCursor {
    pub text: String,
    pub at: DateTime<Utc>,
    pub id: Uuid,
    pub fp: String,
}

pub(crate) async fn directory_page(
    db: &sqlx::PgPool,
    org_id: Uuid,
    f: &DirectoryFilter,
    order: DirectoryOrder,
    cursor: Option<&DirectoryCursor>,
    limit: i64,
) -> Result<Vec<DirectoryEntry>, ApiError> {
    // `-infinity` para quem nunca entrou: fica no fim na ordem descendente.
    let (order_sql, keyset) = match order {
        DirectoryOrder::NameAsc => ("lower(e.name), e.id", "(lower(e.name), e.id) > ($12, $14)"),
        DirectoryOrder::NameDesc => (
            "lower(e.name) DESC, e.id DESC",
            "(lower(e.name), e.id) < ($12, $14)",
        ),
        DirectoryOrder::LastAccessDesc => (
            "COALESCE(e.last_access_at, '-infinity'::timestamptz) DESC, e.id DESC",
            "(COALESCE(e.last_access_at, '-infinity'::timestamptz), e.id) < ($13, $14)",
        ),
        DirectoryOrder::LastAccessAsc => (
            "COALESCE(e.last_access_at, '-infinity'::timestamptz), e.id",
            "(COALESCE(e.last_access_at, '-infinity'::timestamptz), e.id) > ($13, $14)",
        ),
    };
    let sql = format!(
        "{DIRECTORY_CTE} SELECT e.* FROM entries e {} AND ($15 OR {keyset})
          ORDER BY {order_sql} LIMIT $16",
        directory_where(f)
    );
    let q = sqlx::query_as::<_, DirectoryEntry>(&sql);
    let q = bind_directory!(q, org_id, f);
    Ok(q.bind(cursor.map(|c| c.text.clone()).unwrap_or_default())
        .bind(cursor.map(|c| c.at).unwrap_or_else(Utc::now))
        .bind(cursor.map(|c| c.id).unwrap_or_default())
        .bind(cursor.is_none())
        .bind(limit)
        .fetch_all(db)
        .await?)
}

/// Total filtrado e contagens por estado (estas sobre a org, sem filtros).
pub(crate) async fn directory_counts(
    db: &sqlx::PgPool,
    org_id: Uuid,
    f: &DirectoryFilter,
) -> Result<(i64, i64, i64, i64), ApiError> {
    let sql = format!(
        "{DIRECTORY_CTE} SELECT
            (SELECT COUNT(*) FROM entries e {}),
            (SELECT COUNT(*) FROM entries WHERE status = 'active'
                AND ($10::uuid IS NULL OR department_id = $10)),
            (SELECT COUNT(*) FROM entries WHERE status = 'invited'
                AND ($10::uuid IS NULL OR department_id = $10)),
            (SELECT COUNT(*) FROM entries WHERE status = 'suspended'
                AND ($10::uuid IS NULL OR department_id = $10))",
        directory_where(f)
    );
    let q = sqlx::query_as::<_, (i64, i64, i64, i64)>(&sql);
    Ok(bind_directory!(q, org_id, f).fetch_one(db).await?)
}

/// Grupos do primeiro campo de agrupamento, sobre o conjunto filtrado.
pub(crate) async fn directory_groups(
    db: &sqlx::PgPool,
    org_id: Uuid,
    f: &DirectoryFilter,
    field: &str,
) -> Result<Vec<(Option<String>, Option<String>, i64)>, ApiError> {
    let (key, label) = match field {
        "department" => ("e.department_id::text", "e.department_name"),
        "role" => ("e.role_id::text", "e.role_name"),
        "status" => ("e.status", "e.status"),
        "origin" => ("e.origin", "e.origin"),
        _ => return Err(ApiError::internal("campo de agrupamento não validado")),
    };
    let sql = format!(
        "{DIRECTORY_CTE} SELECT {key} AS k, MIN({label}) AS l, COUNT(*) AS n
           FROM entries e {} GROUP BY {key}
          ORDER BY MIN({label}) NULLS LAST, {key} NULLS LAST LIMIT 100",
        directory_where(f)
    );
    let q = sqlx::query_as::<_, (Option<String>, Option<String>, i64)>(&sql);
    Ok(bind_directory!(q, org_id, f).fetch_all(db).await?)
}

/// Pessoas por departamento (a coluna do ecrã e o «em uso» ao apagar).
pub(crate) async fn department_member_counts(
    db: &sqlx::PgPool,
    org_id: Uuid,
) -> Result<std::collections::HashMap<Uuid, i64>, ApiError> {
    let rows: Vec<(Uuid, i64)> = sqlx::query_as(
        "SELECT department_id, COUNT(*) FROM org_members
          WHERE org_id = $1 AND archived_at IS NULL AND department_id IS NOT NULL
          GROUP BY department_id",
    )
    .bind(org_id)
    .fetch_all(db)
    .await?;
    Ok(rows.into_iter().collect())
}

/// Tira o departamento a quem o tinha (apagar departamento).
pub(crate) async fn clear_department_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    org_id: Uuid,
    department_id: Uuid,
) -> Result<u64, ApiError> {
    Ok(sqlx::query(
        "UPDATE org_members SET department_id = NULL WHERE org_id = $1 AND department_id = $2",
    )
    .bind(org_id)
    .bind(department_id)
    .execute(&mut **tx)
    .await?
    .rows_affected())
}

/// Pertença (qualquer estado) de um email nesta org: (user_id, activo, razão, papel, departamento).
pub(crate) async fn member_by_email(
    db: &sqlx::PgPool,
    org_id: Uuid,
    email: &str,
) -> Result<Option<(Uuid, bool, Option<String>, Uuid, Option<Uuid>)>, ApiError> {
    Ok(sqlx::query_as(
        "SELECT m.user_id, m.archived_at IS NULL, m.archived_reason, m.role_id, m.department_id
           FROM org_members m JOIN users u ON u.id = m.user_id
          WHERE m.org_id = $1 AND u.email = $2",
    )
    .bind(org_id)
    .bind(email)
    .fetch_optional(db)
    .await?)
}

// ---------- sincronização do Odoo (ADR-0008 §9) ----------

/// Pertença de uma conta vinda do Odoo. Substitui o `INSERT … ON CONFLICT DO
/// UPDATE SET role = CASE …` do `odoo_sso::upsert_member`: o texto `role` é
/// derivado, e o «nunca despromove» continua — um admin do Odoo sobe a `admin`
/// só se ainda não for admin nem dono; ninguém desce.
pub(crate) async fn ensure_odoo_membership(
    state: &AppState,
    org_id: Uuid,
    user_id: Uuid,
    odoo_admin: bool,
) -> Result<(), ApiError> {
    sqlx::query(
        "INSERT INTO org_members (org_id, user_id, role, origin) VALUES ($1, $2, $3, 'odoo_sso')
         ON CONFLICT (org_id, user_id) DO NOTHING",
    )
    .bind(org_id)
    .bind(user_id)
    .bind(if odoo_admin { "admin" } else { "member" })
    .execute(&state.db)
    .await
    .map_err(map_member_write_error)?;
    if odoo_admin {
        if let Some(role) = role_in_org(state, org_id, user_id).await? {
            if role != "admin" {
                set_system_role(state, org_id, user_id, SystemRole::Admin).await?;
            }
        }
    }
    Ok(())
}

/// Resultado de aplicar os grupos e o departamento do Odoo a uma pessoa.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OdooApplied {
    Unchanged,
    RoleChanged,
    Conflict,
}

/// Aplica, a uma conta que ESTA org gere (R25) e com pertença ACTIVA (R143), os
/// grupos do Odoo (papel) e o departamento. `groups = None` = não foram lidos.
pub(crate) async fn apply_odoo_attributes(
    state: &AppState,
    org_id: Uuid,
    user_id: Uuid,
    groups: Option<&std::collections::BTreeSet<String>>,
    department: Option<(&str, &str)>,
) -> Result<OdooApplied, ApiError> {
    let mut tx = state.db.begin().await?;
    let Some(cur) = member_state(&mut *tx, org_id, user_id).await? else {
        return Ok(OdooApplied::Unchanged);
    };
    if cur.archived_at.is_some() || !cur.odoo_managed_here || cur.service_account {
        return Ok(OdooApplied::Unchanged);
    }
    if let Some((ext_ref, name)) = department {
        let dept: Uuid = sqlx::query_scalar(
            "INSERT INTO departments (org_id, name, source, external_ref) VALUES ($1, $2, 'odoo', $3)
             ON CONFLICT (org_id, external_ref) WHERE external_ref IS NOT NULL
             DO UPDATE SET name = EXCLUDED.name, updated_at = now()
             RETURNING id",
        )
        .bind(org_id)
        .bind(name)
        .bind(ext_ref)
        .fetch_one(&mut *tx)
        .await?;
        if cur.department_id != Some(dept) {
            set_member_department_tx(&mut tx, org_id, user_id, Some(dept)).await?;
        }
    }
    let mut applied = OdooApplied::Unchanged;
    if let Some(groups) = groups {
        let g: Vec<String> = groups.iter().cloned().collect();
        sqlx::query("UPDATE org_members SET odoo_groups = $3 WHERE org_id = $1 AND user_id = $2")
            .bind(org_id)
            .bind(user_id)
            .bind(&g)
            .execute(&mut *tx)
            .await?;
        let set = crate::roles::role_set_tx(&mut tx, org_id).await?;
        let mappings: std::collections::BTreeMap<String, Uuid> =
            sqlx::query_as::<_, (String, Uuid)>(
                "SELECT odoo_group, id FROM org_roles WHERE org_id = $1 AND odoo_group IS NOT NULL",
            )
            .bind(org_id)
            .fetch_all(&mut *tx)
            .await?
            .into_iter()
            .collect();
        let source = if cur.role_source == "odoo_group" {
            authz::RoleSource::OdooGroup
        } else {
            authz::RoleSource::Manual
        };
        match authz::odoo_group_outcome(&set, cur.role_id, source, Some(groups), &mappings) {
            authz::GroupOutcome::NoChange => {}
            authz::GroupOutcome::Assign(role) => {
                set_member_role_tx(&mut tx, org_id, user_id, role, "odoo_group").await?;
                applied = OdooApplied::RoleChanged;
            }
            authz::GroupOutcome::RevertToMember => {
                set_member_role_tx(&mut tx, org_id, user_id, set.member_role(), "manual").await?;
                applied = OdooApplied::RoleChanged;
            }
            authz::GroupOutcome::Conflict { proposed } => {
                // Um conflito pendente por pessoa: o novo substitui o antigo.
                sqlx::query(
                    "UPDATE role_conflicts SET status = 'superseded'
                      WHERE org_id = $1 AND user_id = $2 AND status = 'pending'",
                )
                .bind(org_id)
                .bind(user_id)
                .execute(&mut *tx)
                .await?;
                sqlx::query(
                    "INSERT INTO role_conflicts (org_id, user_id, current_role_id, proposed_role_ids, odoo_groups)
                     VALUES ($1, $2, $3, $4, $5)",
                )
                .bind(org_id)
                .bind(user_id)
                .bind(cur.role_id)
                .bind(&proposed)
                .bind(&g)
                .execute(&mut *tx)
                .await?;
                applied = OdooApplied::Conflict;
            }
        }
    }
    tx.commit().await.map_err(map_member_write_error)?;
    Ok(applied)
}

/// «Suspender ao sair do Odoo»: contas geridas por ESTA org, activas, que não
/// estão na lista COMPLETA que o Odoo acabou de devolver. Nunca o último dono.
pub(crate) async fn suspend_odoo_leavers(
    state: &AppState,
    org_id: Uuid,
    present: &[Uuid],
) -> Result<usize, ApiError> {
    let leavers: Vec<Uuid> = sqlx::query_scalar(
        "SELECT m.user_id FROM org_members m JOIN users u ON u.id = m.user_id
          WHERE m.org_id = $1 AND m.archived_at IS NULL AND u.odoo_managed
            AND u.odoo_org_id = $1 AND u.email <> $2 AND NOT (m.user_id = ANY($3))",
    )
    .bind(org_id)
    .bind(SERVICE_ACCOUNT_EMAIL)
    .bind(present)
    .fetch_all(&state.db)
    .await?;
    let mut n = 0;
    for user in leavers {
        let mut tx = state.db.begin().await?;
        match archive_member_tx(&mut tx, org_id, user, "odoo_exit", None).await {
            Ok(true) => {
                tx.commit().await.map_err(map_member_write_error)?;
                crate::audit::log(
                    &state.db,
                    Some(org_id),
                    Uuid::nil(),
                    "member.odoo_exit",
                    &user.to_string(),
                )
                .await;
                n += 1;
            }
            Ok(false) => {}
            Err(e) => tracing::warn!(error = %e, %org_id, %user, "saída do Odoo não aplicada"),
        }
    }
    Ok(n)
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
            chat_retention_days: Some(7),
        })
        .unwrap();
        // Campo a campo, e não um `json!` literal: a catraca da arquitectura
        // conta os `{"ok": true}` do código, e um teste não é dívida.
        assert_eq!(v.as_object().unwrap().len(), 4);
        assert_eq!(v["ok"], serde_json::Value::Bool(true));
        assert_eq!(v["domain"], "meet.acme.com");
        assert_eq!(v["retention_days"], 30);
        assert_eq!(v["chat_retention_days"], 7);
    }

    #[test]
    fn slugify_basic() {
        assert_eq!(slugify("Kaeso Lda"), "kaeso-lda");
        assert_eq!(slugify("  Açores & Cia!  "), "a-ores-cia");
        assert_eq!(slugify("***"), "org");
    }
}
