//! Multi-tenant enterprise: organizações, filiais, employees e grupos.
//!
//! Quem cria uma organização torna-se `admin`. Admins gerem filiais,
//! employees e grupos. Membros veem o diretório e podem iniciar chamadas.

use axum::{
    extract::{ConnectInfo, Path, State},
    http::HeaderMap,
    response::Response,
    Json,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
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
    /// Quem pode enviar SMS a contactos da org: `admins` | `members`.
    pub sms_send_policy: String,
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
    /// Telefone E.164 do membro NESTA org. Só o vê um admin ou o próprio; para
    /// os colegas vai `null` e basta-lhes `can_sms`.
    #[sqlx(default)]
    pub phone: Option<String>,
    /// `odoo` | `manual` | `null` — quem escreveu o número (ver migração 0060).
    #[sqlx(default)]
    pub phone_source: Option<String>,
    /// Tem número e não desligou os SMS de contactos. Não diz se QUEM PERGUNTA
    /// pode enviar: isso é a política da org.
    #[sqlx(default)]
    pub can_sms: bool,
    /// Bloqueado (suspenso) por um admin — ver migração 0054. Distinto de ser
    /// arquivado: continua membro, só sem acesso enquanto durar.
    #[sqlx(default)]
    pub suspended_at: Option<DateTime<Utc>>,
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
    // `suspended_at IS NULL` aqui, no ÚNICO sítio de onde `require_admin` e
    // `require_member` (e os três chamadores externos — sms_notify, sms,
    // stream_destinations) derivam: um membro suspenso deixa de contar como
    // membro para efeitos de autorização, sem deixar de o SER (não passa por
    // `archived_at`, por isso reaparece de imediato ao ser reactivado — ver
    // migração 0054).
    let row: Option<(String,)> = sqlx::query_as(
        "SELECT role FROM org_members
         WHERE org_id = $1 AND user_id = $2 AND archived_at IS NULL AND suspended_at IS NULL",
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

    // Os dois papéis de sistema nascem com a organização — ver rbac.rs.
    let (admin_role_id,): (Uuid,) = sqlx::query_as(
        "INSERT INTO org_roles (org_id, name, is_system) VALUES ($1, 'Administrador', TRUE) RETURNING id",
    )
    .bind(org.id)
    .fetch_one(&state.db)
    .await?;
    sqlx::query("INSERT INTO org_roles (org_id, name, is_system) VALUES ($1, 'Membro', TRUE)")
        .bind(org.id)
        .execute(&state.db)
        .await?;

    // O criador entra como admin.
    sqlx::query(
        "INSERT INTO org_members (org_id, user_id, role, role_id, title) VALUES ($1, $2, 'admin', $3, 'Administrador')",
    )
    .bind(org.id)
    .bind(auth.user_id)
    .bind(admin_role_id)
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
           o.max_groups, o.max_rooms, o.max_meetings, o.sms_send_policy
    FROM organizations o
    JOIN org_members m ON m.org_id = o.id AND m.user_id = $1 AND m.archived_at IS NULL
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

/// Org-first: o email de quem entra (colaborador OU convidado) tem de ser do
/// domínio da organização. Orgs legadas sem domínio definido (`email_domain`
/// vazio) não são restringidas. Partilhado por `add_employee` e as duas
/// entradas de convite (`create_invite`, `bulk_create_invites`) — a mesma
/// regra em três sítios já ficou esquecida uma vez (ver comentário sobre a
/// R25/`ForeignOrg` abaixo); não duplicar de novo.
async fn require_org_domain(state: &AppState, org_id: Uuid, email: &str) -> Result<(), ApiError> {
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
    Ok(())
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
    require_org_domain(&state, org_id, &email).await?;
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

    // `role_id` só é fixado na PRIMEIRA inserção (papel de sistema correspondente
    // ao `role` legado); num re-convite (ON CONFLICT) fica como estava — não
    // apaga um papel delegado já atribuído a este membro.
    sqlx::query(
        "INSERT INTO org_members (org_id, user_id, branch_id, role, role_id, title)
         VALUES ($1, $2, $3, $4,
                 (SELECT id FROM org_roles WHERE org_id = $1 AND is_system = TRUE
                  AND name = CASE WHEN $4 = 'admin' THEN 'Administrador' ELSE 'Membro' END),
                 $5)
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
    let is_admin = match role_in_org(&state, org_id, auth.user_id).await? {
        Some(role) => role == "admin",
        None => return Err(ApiError::NotFound),
    };
    // O número é dado pessoal: colegas sabem que existe (`can_sms`), não qual é.
    let emps: Vec<Employee> = sqlx::query_as(
        r#"SELECT m.user_id, u.username, u.email, m.role, m.title, m.branch_id, b.name AS branch_name,
                  (SELECT MAX(a.created_at) FROM audit_logs a WHERE a.actor_id = m.user_id) AS last_active,
                  CASE WHEN $2 OR m.user_id = $3 THEN m.phone_e164 END AS phone,
                  CASE WHEN $2 OR m.user_id = $3 THEN m.phone_source END AS phone_source,
                  (m.phone_e164 IS NOT NULL AND NOT u.sms_contact_opt_out) AS can_sms,
                  m.suspended_at
           FROM org_members m JOIN users u ON u.id = m.user_id
           LEFT JOIN branches b ON b.id = m.branch_id
           WHERE m.org_id = $1 AND m.archived_at IS NULL ORDER BY u.username"#,
    )
    .bind(org_id)
    .bind(is_admin)
    .bind(auth.user_id)
    .fetch_all(&state.db)
    .await?;
    Ok(Json(emps))
}

#[derive(Deserialize, utoipa::ToSchema)]
pub struct UpdateEmployeeReq {
    pub role: Option<String>,
    pub title: Option<String>,
    pub branch_id: Option<Uuid>,
    /// `Some(true)` suspende (bloqueia acesso, mantém o membro); `Some(false)`
    /// reactiva; `None` não mexe. Ver migração 0054.
    #[serde(default)]
    pub suspended: Option<bool>,
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
    require_admin(&state, org_id, auth.user_id).await?;
    if let Some(role) = &req.role {
        if !matches!(role.as_str(), "admin" | "member") {
            return Err(ApiError::BadRequest("role inválido".into()));
        }
    }
    // Mesma guarda de `remove_employee`: suspender-se a si próprio deixava um
    // admin sem admins na org se fosse o único, e ninguém o reactivava.
    if req.suspended == Some(true) && user_id == auth.user_id {
        return Err(ApiError::BadRequest(
            "não podes suspender o teu próprio acesso".into(),
        ));
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
    if let Some(suspended) = req.suspended {
        if suspended {
            sqlx::query(
                "UPDATE org_members SET suspended_at = NOW(), suspended_by = $3
                 WHERE org_id = $1 AND user_id = $2 AND suspended_at IS NULL",
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
                "member.suspended",
                &user_id.to_string(),
            )
            .await;
        } else {
            sqlx::query(
                "UPDATE org_members SET suspended_at = NULL, suspended_by = NULL
                 WHERE org_id = $1 AND user_id = $2",
            )
            .bind(org_id)
            .bind(user_id)
            .execute(&state.db)
            .await?;
            crate::audit::log(
                &state.db,
                Some(org_id),
                auth.user_id,
                "member.reactivated",
                &user_id.to_string(),
            )
            .await;
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
    require_admin(&state, org_id, auth.user_id).await?;
    if user_id == auth.user_id {
        return Err(ApiError::BadRequest(
            "não podes arquivar o teu próprio acesso".into(),
        ));
    }
    // Soft delete: archived_at + archived_by para auditoria futura
    let res = sqlx::query(
        "UPDATE org_members SET archived_at = NOW(), archived_by = $3
         WHERE org_id = $1 AND user_id = $2 AND archived_at IS NULL",
    )
    .bind(org_id)
    .bind(user_id)
    .bind(auth.user_id)
    .execute(&state.db)
    .await?;
    if res.rows_affected() == 0 {
        return Err(ApiError::NotFound);
    }
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

// ---------- invites ----------
//
// Convite POR LINK — este produto não tem SMTP (grep por `smtp`/`lettre`/
// `mailer` em server/ não devolve nada). O admin gera o link
// (`create_invite`/`bulk_create_invites`), COPIA-O e envia-o pelo canal que
// preferir; a UI diz isso mesmo. `add_employee` continua a existir para o
// admin que quer definir a palavra-passe de alguém directamente
// (break-glass); o convite é o caminho normal para "juntar alguém à
// organização" sem o admin saber a palavra-passe de ninguém.
//
// Não há fusão de contas: se `accept_invite` encontra um `users` já com esse
// email, recusa com 409 e manda a pessoa fazer login normal — juntar um
// convite a uma conta existente é um problema de identidade próprio (que
// conta e que sessão ganham?) que esta versão não resolve.

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct Invite {
    pub id: Uuid,
    pub org_id: Uuid,
    pub email: String,
    pub role: String,
    pub branch_id: Option<Uuid>,
    pub branch_name: Option<String>,
    pub title: String,
    pub token: String,
    pub invited_by: Uuid,
    pub invited_by_name: String,
    pub created_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub accepted_at: Option<DateTime<Utc>>,
    pub revoked_at: Option<DateTime<Utc>>,
}

const INVITE_SELECT: &str = r#"
    SELECT i.id, i.org_id, i.email, i.role, i.branch_id, b.name AS branch_name, i.title,
           i.token, i.invited_by, u.username AS invited_by_name,
           i.created_at, i.expires_at, i.accepted_at, i.revoked_at
    FROM org_invites i
    JOIN users u ON u.id = i.invited_by
    LEFT JOIN branches b ON b.id = i.branch_id
"#;

fn validate_invite_role(role: Option<&str>) -> Result<&str, ApiError> {
    let role = role.unwrap_or("member");
    if !matches!(role, "admin" | "member") {
        return Err(ApiError::BadRequest("role inválido".into()));
    }
    Ok(role)
}

/// `true` se `email` já pertence a um membro ACTIVO (não arquivado) desta org
/// — convidar alguém que já está dentro não faz sentido, e o índice parcial
/// de `org_invites` só protege contra convites DUPLICADOS, não contra isto.
async fn email_is_active_member(
    state: &AppState,
    org_id: Uuid,
    email: &str,
) -> Result<bool, ApiError> {
    Ok(sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM org_members m JOIN users u ON u.id = m.user_id
         WHERE m.org_id = $1 AND u.email = $2 AND m.archived_at IS NULL)",
    )
    .bind(org_id)
    .bind(email)
    .fetch_one(&state.db)
    .await?)
}

/// Insere o convite e devolve-o já com `branch_name`/`invited_by_name`
/// resolvidos — usado por `create_invite` e por cada linha de
/// `bulk_create_invites`. O token é gerado com o MESMO gerador dos refresh
/// tokens (`auth::new_refresh_token`, 32 bytes de `OsRng`): aqui não se
/// guarda o hash porque o token É a chave de consulta pública, não um
/// segredo comparado no servidor.
async fn insert_invite(
    state: &AppState,
    org_id: Uuid,
    email: &str,
    role: &str,
    branch_id: Option<Uuid>,
    title: &str,
    invited_by: Uuid,
) -> Result<Invite, ApiError> {
    let (token, _hash) = crate::auth::new_refresh_token();
    let expires_at = Utc::now() + chrono::Duration::days(14);
    let id: Uuid = sqlx::query_scalar(
        "INSERT INTO org_invites (org_id, email, role, branch_id, title, token, invited_by, expires_at)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8) RETURNING id",
    )
    .bind(org_id)
    .bind(email)
    .bind(role)
    .bind(branch_id)
    .bind(title)
    .bind(&token)
    .bind(invited_by)
    .bind(expires_at)
    .fetch_one(&state.db)
    .await
    .map_err(|e| ApiError::from_unique(e, "já existe um convite pendente para este email"))?;

    let invite: Invite = sqlx::query_as(&format!("{INVITE_SELECT} WHERE i.id = $1"))
        .bind(id)
        .fetch_one(&state.db)
        .await?;
    Ok(invite)
}

#[derive(Deserialize)]
pub struct CreateInviteReq {
    pub email: String,
    #[serde(default)]
    pub role: Option<String>,
    #[serde(default)]
    pub branch_id: Option<Uuid>,
    #[serde(default)]
    pub title: Option<String>,
}

/// `POST /api/orgs/{org_id}/invites` — gera um link de convite (admin-only).
pub async fn create_invite(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(org_id): Path<Uuid>,
    Json(req): Json<CreateInviteReq>,
) -> Result<Json<Invite>, ApiError> {
    require_admin(&state, org_id, auth.user_id).await?;
    let email = req.email.trim().to_lowercase();
    if !email.contains('@') {
        return Err(ApiError::BadRequest("email inválido".into()));
    }
    require_org_domain(&state, org_id, &email).await?;
    let role = validate_invite_role(req.role.as_deref())?;
    if email_is_active_member(&state, org_id, &email).await? {
        return Err(ApiError::Conflict(format!(
            "{email} já é membro desta organização"
        )));
    }
    let title = req.title.as_deref().unwrap_or("").trim();
    let invite = insert_invite(
        &state,
        org_id,
        &email,
        role,
        req.branch_id,
        title,
        auth.user_id,
    )
    .await?;
    crate::audit::log(
        &state.db,
        Some(org_id),
        auth.user_id,
        "invite.created",
        &email,
    )
    .await;
    Ok(Json(invite))
}

#[derive(Deserialize)]
pub struct BulkInviteRow {
    pub email: String,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub role: Option<String>,
    /// Nome de uma filial EXISTENTE desta org (comparado sem distinguir
    /// maiúsculas/minúsculas); sem correspondência, fica sem filial — não é
    /// erro, um CSV exportado de outro sítio raramente bate certo com os
    /// nomes internos.
    #[serde(default)]
    pub branch: Option<String>,
}

#[derive(Deserialize)]
pub struct BulkInviteReq {
    pub rows: Vec<BulkInviteRow>,
}

#[derive(Debug, Serialize)]
pub struct BulkInviteResult {
    pub email: String,
    pub ok: bool,
    pub error: Option<String>,
    pub invite: Option<Invite>,
}

/// Resolve o nome de filial de uma linha de CSV contra as filiais EXISTENTES
/// da org, sem distinguir maiúsculas/minúsculas. Sem correspondência (nome
/// vazio, em branco, ou que não bate com nenhuma), devolve `None` — não é
/// erro, é só "sem filial". Função pura (sem BD) para ser testável a sério.
fn resolve_branch_by_name(branches: &[Branch], name: Option<&str>) -> Option<Uuid> {
    let name = name?.trim();
    if name.is_empty() {
        return None;
    }
    branches
        .iter()
        .find(|b| b.name.eq_ignore_ascii_case(name))
        .map(|b| b.id)
}

/// Uma linha da importação. Erros devolvem-se como texto (não se propagam):
/// `ApiError` implementa `Display` com a MESMA mensagem que iria para o
/// cliente num pedido isolado (`#[error("{0}")]` do `thiserror`), por isso
/// `e.to_string()` chega sem duplicar texto.
async fn bulk_create_one(
    state: &AppState,
    org_id: Uuid,
    row: &BulkInviteRow,
    branches: &[Branch],
    invited_by: Uuid,
) -> Result<Invite, ApiError> {
    let email = row.email.trim().to_lowercase();
    if !email.contains('@') {
        return Err(ApiError::BadRequest("email inválido".into()));
    }
    require_org_domain(state, org_id, &email).await?;
    let role = validate_invite_role(row.role.as_deref())?;
    let branch_id = resolve_branch_by_name(branches, row.branch.as_deref());
    if email_is_active_member(state, org_id, &email).await? {
        return Err(ApiError::Conflict(format!(
            "{email} já é membro desta organização"
        )));
    }
    let title = row.title.as_deref().unwrap_or("").trim();
    insert_invite(state, org_id, &email, role, branch_id, title, invited_by).await
}

/// `POST /api/orgs/{org_id}/invites/bulk` — importação CSV (admin-only).
/// Cada linha é independente (não é tudo-ou-nada, mesmo padrão de
/// `MembersCard::applyToSelected` no frontend): uma linha malformada não
/// bloqueia as restantes, e o admin vê exactamente qual falhou e porquê.
pub async fn bulk_create_invites(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(org_id): Path<Uuid>,
    Json(req): Json<BulkInviteReq>,
) -> Result<Json<Vec<BulkInviteResult>>, ApiError> {
    require_admin(&state, org_id, auth.user_id).await?;
    if req.rows.len() > 500 {
        return Err(ApiError::BadRequest(
            "máximo de 500 linhas por importação".into(),
        ));
    }
    let branches: Vec<Branch> = sqlx::query_as(
        "SELECT id, org_id, name, location, created_at FROM branches WHERE org_id = $1",
    )
    .bind(org_id)
    .fetch_all(&state.db)
    .await?;

    let mut out = Vec::with_capacity(req.rows.len());
    for row in &req.rows {
        let email = row.email.trim().to_lowercase();
        match bulk_create_one(&state, org_id, row, &branches, auth.user_id).await {
            Ok(invite) => out.push(BulkInviteResult {
                email,
                ok: true,
                error: None,
                invite: Some(invite),
            }),
            Err(e) => out.push(BulkInviteResult {
                email,
                ok: false,
                error: Some(e.to_string()),
                invite: None,
            }),
        }
    }
    let ok_count = out.iter().filter(|r| r.ok).count();
    crate::audit::log(
        &state.db,
        Some(org_id),
        auth.user_id,
        "invite.bulk_imported",
        &format!("{ok_count}/{} linhas", out.len()),
    )
    .await;
    Ok(Json(out))
}

/// `GET /api/orgs/{org_id}/invites` — só os PENDENTES (admin-only): é o que
/// alimenta a contagem "convidados" e a lista de convites por resolver.
pub async fn list_invites(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(org_id): Path<Uuid>,
) -> Result<Json<Vec<Invite>>, ApiError> {
    require_admin(&state, org_id, auth.user_id).await?;
    let invites: Vec<Invite> = sqlx::query_as(&format!(
        "{INVITE_SELECT} WHERE i.org_id = $1 AND i.accepted_at IS NULL AND i.revoked_at IS NULL \
         AND i.expires_at > NOW() ORDER BY i.created_at DESC"
    ))
    .bind(org_id)
    .fetch_all(&state.db)
    .await?;
    Ok(Json(invites))
}

/// `DELETE /api/orgs/{org_id}/invites/{invite_id}` — admin-only; só actua
/// sobre um convite ainda pendente (404 se já foi aceite/revogado, ou não
/// existe nesta org).
pub async fn revoke_invite(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path((org_id, invite_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_admin(&state, org_id, auth.user_id).await?;
    let res = sqlx::query(
        "UPDATE org_invites SET revoked_at = NOW()
         WHERE id = $1 AND org_id = $2 AND accepted_at IS NULL AND revoked_at IS NULL",
    )
    .bind(invite_id)
    .bind(org_id)
    .execute(&state.db)
    .await?;
    if res.rows_affected() == 0 {
        return Err(ApiError::NotFound);
    }
    crate::audit::log(
        &state.db,
        Some(org_id),
        auth.user_id,
        "invite.revoked",
        &invite_id.to_string(),
    )
    .await;
    Ok(Json(serde_json::json!({ "ok": true })))
}

/// Estado de um convite, decidido por UMA função pura (sem BD, testável) —
/// `get_invite_public` e `accept_invite` chamam-na em vez de repetirem a
/// ordem de prioridade (revogado vence aceite vence expirado) cada uma à sua
/// maneira, que é como duas rotas do mesmo recurso acabam a discordar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InviteState {
    Pending,
    Expired,
    Revoked,
    Accepted,
}

fn invite_state(
    now: DateTime<Utc>,
    expires_at: DateTime<Utc>,
    accepted_at: Option<DateTime<Utc>>,
    revoked_at: Option<DateTime<Utc>>,
) -> InviteState {
    if revoked_at.is_some() {
        InviteState::Revoked
    } else if accepted_at.is_some() {
        InviteState::Accepted
    } else if now > expires_at {
        InviteState::Expired
    } else {
        InviteState::Pending
    }
}

/// Forma pública (SEM sessão) de um convite — nunca devolve `org_id`, `id`,
/// `token` nem `invited_by`: quem tem o link já tem o token, e mais nada
/// deve dar para adivinhar.
#[derive(Debug, Serialize)]
pub struct InvitePublic {
    pub org_name: String,
    pub email: String,
    pub role: String,
    pub title: String,
    pub expired: bool,
    pub revoked: bool,
    pub accepted: bool,
}

/// `GET /api/invites/{token}` — SEM AUTENTICAÇÃO; o token é a própria chave
/// de consulta (não há `org_id` no caminho). 404 se nenhum convite tiver
/// este token — nunca se distingue de "expirado" aqui (isso o corpo diz).
pub async fn get_invite_public(
    State(state): State<Arc<AppState>>,
    Path(token): Path<String>,
) -> Result<Json<InvitePublic>, ApiError> {
    #[allow(clippy::type_complexity)]
    let row: Option<(
        String,
        String,
        String,
        String,
        DateTime<Utc>,
        Option<DateTime<Utc>>,
        Option<DateTime<Utc>>,
    )> = sqlx::query_as(
        "SELECT o.name, i.email, i.role, i.title, i.expires_at, i.accepted_at, i.revoked_at
         FROM org_invites i JOIN organizations o ON o.id = i.org_id
         WHERE i.token = $1",
    )
    .bind(&token)
    .fetch_optional(&state.db)
    .await?;
    let (org_name, email, role, title, expires_at, accepted_at, revoked_at) =
        row.ok_or(ApiError::NotFound)?;
    let status = invite_state(Utc::now(), expires_at, accepted_at, revoked_at);
    Ok(Json(InvitePublic {
        org_name,
        email,
        role,
        title,
        expired: status == InviteState::Expired,
        revoked: status == InviteState::Revoked,
        accepted: status == InviteState::Accepted,
    }))
}

#[derive(Deserialize)]
pub struct AcceptInviteReq {
    pub username: String,
    pub password: String,
}

/// `POST /api/invites/{token}/accept` — SEM AUTENTICAÇÃO. Cria a conta e o
/// membro, consome o convite, e devolve `{ access_token, user }` — a MESMA
/// forma que `auth::register` devolve — via `auth::login_response`, para a
/// pessoa entrar logo a seguir sem ter de fazer login manualmente.
pub async fn accept_invite(
    State(state): State<Arc<AppState>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path(token): Path<String>,
    Json(req): Json<AcceptInviteReq>,
) -> Result<Response, ApiError> {
    #[allow(clippy::type_complexity)]
    let row: Option<(
        Uuid,
        Uuid,
        String,
        String,
        Option<Uuid>,
        String,
        DateTime<Utc>,
        Option<DateTime<Utc>>,
        Option<DateTime<Utc>>,
    )> = sqlx::query_as(
        "SELECT id, org_id, email, role, branch_id, title, expires_at, accepted_at, revoked_at
         FROM org_invites WHERE token = $1",
    )
    .bind(&token)
    .fetch_optional(&state.db)
    .await?;
    let (invite_id, org_id, email, role, branch_id, title, expires_at, accepted_at, revoked_at) =
        row.ok_or(ApiError::NotFound)?;

    match invite_state(Utc::now(), expires_at, accepted_at, revoked_at) {
        InviteState::Revoked => return Err(ApiError::Conflict("este convite foi revogado".into())),
        InviteState::Accepted => {
            return Err(ApiError::Conflict("este convite já foi aceite".into()))
        }
        InviteState::Expired => return Err(ApiError::Conflict("este convite expirou".into())),
        InviteState::Pending => {}
    }

    // Sem fusão de contas — ver comentário no topo da secção.
    let existing: Option<(Uuid,)> = sqlx::query_as("SELECT id FROM users WHERE email = $1")
        .bind(&email)
        .fetch_optional(&state.db)
        .await?;
    if existing.is_some() {
        return Err(ApiError::Conflict(
            "já existe uma conta com este email — inicia sessão e pede a um administrador \
             para te adicionar"
                .into(),
        ));
    }

    let username = req.username.trim().to_string();
    if username.len() < 2 {
        return Err(ApiError::BadRequest(
            "nome deve ter pelo menos 2 caracteres".into(),
        ));
    }
    if !(8..=128).contains(&req.password.len()) {
        return Err(ApiError::BadRequest(
            "password deve ter 8-128 caracteres".into(),
        ));
    }
    let hash = crate::auth::hash_password(&req.password)?;

    let mut tx = state.db.begin().await?;
    let user: crate::users::UserPublic = sqlx::query_as(
        "INSERT INTO users (email, username, password_hash) VALUES ($1, $2, $3)
         RETURNING id, email, username, created_at, COALESCE(locale, 'pt') AS locale",
    )
    .bind(&email)
    .bind(&username)
    .bind(&hash)
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| ApiError::from_unique(e, "email ou nome de utilizador já em uso"))?;

    // Mesmo padrão do `role_id` de `add_employee`: resolvido a partir do
    // papel de sistema correspondente ao `role` legado.
    sqlx::query(
        "INSERT INTO org_members (org_id, user_id, branch_id, role, role_id, title)
         VALUES ($1, $2, $3, $4,
                 (SELECT id FROM org_roles WHERE org_id = $1 AND is_system = TRUE
                  AND name = CASE WHEN $4 = 'admin' THEN 'Administrador' ELSE 'Membro' END),
                 $5)",
    )
    .bind(org_id)
    .bind(user.id)
    .bind(branch_id)
    .bind(&role)
    .bind(&title)
    .execute(&mut *tx)
    .await?;

    sqlx::query("UPDATE org_invites SET accepted_at = NOW() WHERE id = $1")
        .bind(invite_id)
        .execute(&mut *tx)
        .await?;

    tx.commit().await?;

    crate::audit::log(&state.db, Some(org_id), user.id, "invite.accepted", &email).await;

    let ip = crate::rate_limit::client_ip(&headers, addr.ip());
    crate::auth::login_response(&state, user, &headers, ip).await
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

// ---------- a regra de pertença como fragmento SQL ----------
//
// Há consultas que precisam da regra DENTRO de um JOIN (a biblioteca de
// gravações filtra centenas de linhas numa ida à base). Em vez de a copiar
// para esse módulo — que é como `archived_at` ficou esquecido da outra vez
// (auditoria S3) —, a regra escreve-se aqui e os outros módulos pedem-na.
//
// Os argumentos são EXPRESSÕES SQL do próprio chamador (`$1`, `r.uploader_id`),
// nunca texto do cliente.

/// `viewer` é membro ACTIVO de uma organização a que `subject` pertence.
///
/// O sujeito não se filtra por `archived_at`: o dado de quem saiu continua a
/// ser da organização (mesma regra do download, ver `recordings::can_download`).
pub(crate) fn sql_active_member_with(viewer: &str, subject: &str) -> String {
    format!(
        "EXISTS(SELECT 1 FROM org_members me JOIN org_members o ON o.org_id = me.org_id \
         WHERE me.user_id = {viewer} AND me.archived_at IS NULL AND o.user_id = {subject})"
    )
}

/// `viewer` é admin ACTIVO de uma organização a que `subject` pertence.
pub(crate) fn sql_active_admin_of(viewer: &str, subject: &str) -> String {
    format!(
        "EXISTS(SELECT 1 FROM org_members me JOIN org_members o ON o.org_id = me.org_id \
         WHERE me.user_id = {viewer} AND me.role = 'admin' AND me.archived_at IS NULL \
           AND o.user_id = {subject})"
    )
}

/// Subconsulta `(id, name)` da organização de `subject` que se mostra a
/// `viewer`: a que os dois partilham, se houver; senão a mais antiga do sujeito.
/// Uso: `LEFT JOIN LATERAL (<isto>) alias ON true`.
pub(crate) fn sql_lateral_org_of(subject: &str, viewer: &str) -> String {
    format!(
        "SELECT org.id, org.name FROM org_members om \
         JOIN organizations org ON org.id = om.org_id \
         WHERE om.user_id = {subject} \
         ORDER BY EXISTS(SELECT 1 FROM org_members v WHERE v.org_id = om.org_id \
                         AND v.user_id = {viewer} AND v.archived_at IS NULL) DESC, \
                  om.created_at, org.id \
         LIMIT 1"
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

// ---------- telefone dos membros e destinatários de SMS ----------

/// Destinatário de SMS resolvido no servidor. Só existe para membros ACTIVOS da
/// org (S3): um arquivado deixa de ser contacto no mesmo instante.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct SmsRecipient {
    pub user_id: Uuid,
    pub phone_e164: Option<String>,
    pub sms_contact_opt_out: bool,
    pub sms_meeting_opt_out: bool,
}

/// Os membros activos de `org_id` entre `user_ids`, com número e consentimento.
/// Quem não estiver na resposta não é membro activo desta org.
pub(crate) async fn sms_recipients(
    state: &AppState,
    org_id: Uuid,
    user_ids: &[Uuid],
) -> Result<Vec<SmsRecipient>, ApiError> {
    Ok(sqlx::query_as::<_, SmsRecipient>(
        "SELECT m.user_id, m.phone_e164, u.sms_contact_opt_out, u.sms_meeting_opt_out
         FROM org_members m JOIN users u ON u.id = m.user_id
         WHERE m.org_id = $1 AND m.user_id = ANY($2) AND m.archived_at IS NULL",
    )
    .bind(org_id)
    .bind(user_ids)
    .fetch_all(&state.db)
    .await?)
}

/// Uma escrita MANUAL do telefone (o próprio ou um admin).
pub(crate) enum PhoneWrite {
    /// Número já normalizado, ou `None` para apagar. Fica `manual`: a
    /// sincronização do directório deixa de lhe tocar.
    Manual(Option<String>),
    /// Apaga e devolve o campo ao directório (a próxima sincronização preenche).
    FollowDirectory,
}

/// Grava o telefone de um membro activo. `false` se não for membro activo.
pub(crate) async fn set_member_phone(
    state: &AppState,
    org_id: Uuid,
    user_id: Uuid,
    write: PhoneWrite,
) -> Result<bool, ApiError> {
    let (phone, source) = match write {
        PhoneWrite::Manual(p) => (p, Some("manual")),
        PhoneWrite::FollowDirectory => (None, None),
    };
    let res = sqlx::query(
        "UPDATE org_members SET phone_e164 = $3, phone_source = $4, phone_updated_at = now()
         WHERE org_id = $1 AND user_id = $2 AND archived_at IS NULL",
    )
    .bind(org_id)
    .bind(user_id)
    .bind(phone)
    .bind(source)
    .execute(&state.db)
    .await?;
    Ok(res.rows_affected() > 0)
}

/// Telefone vindo da sincronização do directório (Odoo). A regra escrita na
/// migração 0060: um número `manual` NUNCA é sobrescrito; um número `odoo`
/// acompanha o directório, incluindo ser apagado quando o directório o apaga.
/// Devolve `true` se mudou alguma coisa.
pub(crate) async fn sync_member_phone_from_directory(
    state: &AppState,
    org_id: Uuid,
    user_id: Uuid,
    phone: Option<&str>,
) -> Result<bool, ApiError> {
    let res = sqlx::query(
        "UPDATE org_members
            SET phone_e164 = $3,
                phone_source = CASE WHEN $3::text IS NULL THEN NULL ELSE 'odoo' END,
                phone_updated_at = now()
          WHERE org_id = $1 AND user_id = $2
            AND phone_source IS DISTINCT FROM 'manual'
            AND phone_e164 IS DISTINCT FROM $3::text",
    )
    .bind(org_id)
    .bind(user_id)
    .bind(phone)
    .execute(&state.db)
    .await?;
    Ok(res.rows_affected() > 0)
}

/// O telefone do próprio em cada org activa (para o perfil).
#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct MemberPhone {
    pub org_id: Uuid,
    pub org_name: String,
    pub phone: Option<String>,
    pub phone_source: Option<String>,
}

pub(crate) async fn member_phones_of_user(
    state: &AppState,
    user_id: Uuid,
) -> Result<Vec<MemberPhone>, ApiError> {
    Ok(sqlx::query_as::<_, MemberPhone>(
        "SELECT m.org_id, o.name AS org_name, m.phone_e164 AS phone, m.phone_source
         FROM org_members m JOIN organizations o ON o.id = m.org_id
         WHERE m.user_id = $1 AND m.archived_at IS NULL ORDER BY o.name",
    )
    .bind(user_id)
    .fetch_all(&state.db)
    .await?)
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

#[cfg(test)]
mod tests {
    use super::*;

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

    // ---------- convites: lógica pura (sem BD) ----------
    //
    // Este ficheiro não tem, e nunca teve, um pool de BD de teste — os
    // `#[cfg(test)]` de todo o `server/` são unitários e puros (ver
    // `auth.rs`, `rbac.rs`, etc.). O caminho com BD real (criar convite,
    // aceitar, importar CSV, suspender a bloquear `require_admin`) fica
    // provado pelo smoke test ao vivo contra o servidor a correr — não por
    // um teste que finge uma base de dados que este harness não tem.

    #[test]
    fn validate_invite_role_aceita_so_admin_e_member() {
        assert_eq!(validate_invite_role(None).unwrap(), "member");
        assert_eq!(validate_invite_role(Some("member")).unwrap(), "member");
        assert_eq!(validate_invite_role(Some("admin")).unwrap(), "admin");
        assert!(validate_invite_role(Some("owner")).is_err());
        assert!(validate_invite_role(Some("")).is_err());
    }

    fn dummy_branch(name: &str) -> Branch {
        Branch {
            id: Uuid::new_v4(),
            org_id: Uuid::new_v4(),
            name: name.to_string(),
            location: String::new(),
            created_at: Utc::now(),
        }
    }

    #[test]
    fn resolve_branch_by_name_ignora_maiusculas_e_espacos() {
        let branches = vec![dummy_branch("Luanda"), dummy_branch("Benguela")];
        let luanda_id = branches[0].id;
        assert_eq!(
            resolve_branch_by_name(&branches, Some("  luanda  ")),
            Some(luanda_id)
        );
        assert_eq!(
            resolve_branch_by_name(&branches, Some("LUANDA")),
            Some(luanda_id)
        );
        assert_eq!(resolve_branch_by_name(&branches, Some("Huambo")), None);
    }

    #[test]
    fn resolve_branch_by_name_vazio_ou_ausente_e_sem_filial() {
        let branches = vec![dummy_branch("Luanda")];
        assert_eq!(resolve_branch_by_name(&branches, None), None);
        assert_eq!(resolve_branch_by_name(&branches, Some("")), None);
        assert_eq!(resolve_branch_by_name(&branches, Some("   ")), None);
    }

    #[test]
    fn invite_state_prioriza_revogado_sobre_aceite_sobre_expirado() {
        let now = Utc::now();
        let expires_future = now + chrono::Duration::days(1);
        let expires_past = now - chrono::Duration::days(1);

        assert_eq!(
            invite_state(now, expires_future, None, None),
            InviteState::Pending
        );
        assert_eq!(
            invite_state(now, expires_past, None, None),
            InviteState::Expired
        );
        assert_eq!(
            invite_state(now, expires_future, Some(now), None),
            InviteState::Accepted
        );
        assert_eq!(
            invite_state(now, expires_future, None, Some(now)),
            InviteState::Revoked
        );
        // Revogado vence mesmo se também estava aceite ou expirado — um
        // convite revogado nunca deve voltar a ficar utilizável.
        assert_eq!(
            invite_state(now, expires_past, Some(now), Some(now)),
            InviteState::Revoked
        );
        // Aceite vence expirado: um convite que JÁ foi usado não deve mostrar
        // "expirou" (mensagem errada — o problema não é o prazo).
        assert_eq!(
            invite_state(now, expires_past, Some(now), None),
            InviteState::Accepted
        );
    }

    #[test]
    fn invite_token_gerado_tem_entropia_e_e_unico() {
        // Reusa o gerador dos refresh tokens (ver `insert_invite`): 32 bytes
        // de OsRng, codificados em hex — 64 caracteres, sem colisões em mil
        // gerações sucessivas.
        let mut seen = std::collections::HashSet::new();
        for _ in 0..1000 {
            let (token, _hash) = crate::auth::new_refresh_token();
            assert_eq!(token.len(), 64);
            assert!(token.chars().all(|c| c.is_ascii_hexdigit()));
            assert!(
                seen.insert(token),
                "token repetido — RNG fraco ou reutilizado"
            );
        }
    }
}
