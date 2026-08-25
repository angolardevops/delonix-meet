//! Recurso `meetings` da **superfície pública v1** — criar, actualizar e
//! cancelar reuniões a partir de um sistema externo (hoje o módulo Odoo
//! `nk_delonix_meet`; amanhã o SDK público).
//!
//! # Porque é que isto não é `POST /api/v1/rooms`
//!
//! Uma *sala* é um artefacto efémero: tem código e dono, não tem horário,
//! convidados, ata nem lugar no calendário. Um integrador que só cria salas
//! produz links que ninguém consegue usar:
//!
//!   * o dono da sala fica a ser o utilizador de serviço do provisionamento,
//!     que nunca faz login — logo não há anfitrião;
//!   * sem registo em `meetings` não há `meeting_invitees`, e `room_access`
//!     só concede entrada DIRECTA a dono/convidado/co-anfitrião. Toda a gente
//!     cai na sala de espera à espera de um anfitrião que não existe (e a
//!     espera não tem timeout);
//!   * sem reunião não há ata, nem `minutes_ai_at`, nem entrada no
//!     `GET /api/v1/meetings` — o sync de MoM não tem a que se agarrar.
//!
//! Este módulo cria a reunião **e** a sala, com um anfitrião humano real
//! (`host_email`) e os convidados resolvidos por email.
//!
//! # Idempotência
//!
//! `external_ref` é a referência opaca do lado de lá (ex.:
//! `odoo:kaeso_prod:calendar.event:4821`). Está sob PK `(org_id, external_ref)`
//! em `meeting_external_refs` (migração 0031): repetir o POST devolve a reunião
//! existente em vez de duplicar. Sem isto, cada gravação do evento no Odoo
//! criaria uma sala nova.

use axum::{
    extract::{Path, State},
    Json,
};
use chrono::{DateTime, Utc};
use rand::{rngs::OsRng, RngCore};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use uuid::Uuid;

use crate::{apikeys::ApiKeyAuth, error::ApiError, meetings::Meeting, AppState};

/// Lista de colunas de `meetings` que cobre **todos** os campos de `Meeting`.
/// O `FromRow` derivado faz `try_get` de cada campo — uma coluna em falta é um
/// erro em runtime, não em compilação. Ter a lista num sítio só evita repetir
/// o erro que a migração 0022 introduziu em `start`/`ics`.
const MEETING_COLS: &str = "id, owner_id, title, description, kind, starts_at, duration_min, \
     room_code, created_at, room_ref, minutes, transcript, recurrence_freq, \
     recurrence_interval, recurrence_until, recurrence_count, recurrence_byday, \
     recurrence_parent_id";

// ---------- DTOs ----------

#[derive(Deserialize)]
pub struct InviteeReq {
    pub email: String,
    #[serde(default)]
    pub name: Option<String>,
}

#[derive(Deserialize)]
pub struct CreateMeetingReq {
    /// Referência do sistema chamador. Opcional, mas **sem ela não há
    /// idempotência** — recomendada para qualquer integração de calendário.
    #[serde(default)]
    pub external_ref: Option<String>,
    pub title: String,
    #[serde(default)]
    pub description: String,
    pub starts_at: DateTime<Utc>,
    #[serde(default = "default_duration")]
    pub duration_min: i32,
    /// Email do anfitrião. **Obrigatório**: é ele que fica `owner_id` da sala
    /// e o único que pode admitir convidados (ver `signaling::decide_waiting`).
    pub host_email: String,
    #[serde(default)]
    pub host_name: Option<String>,
    #[serde(default)]
    pub invitees: Vec<InviteeReq>,
    #[serde(default = "default_kind")]
    pub kind: String,
    #[serde(default)]
    pub e2ee: bool,
    #[serde(default)]
    pub waiting_room: bool,
}

fn default_duration() -> i32 {
    30
}
fn default_kind() -> String {
    "video".into()
}

#[derive(Deserialize)]
pub struct PatchMeetingReq {
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub starts_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub duration_min: Option<i32>,
    /// Se presente, **substitui** o conjunto de convidados. O estado de
    /// resposta (aceite/recusado) de quem se mantém é preservado.
    #[serde(default)]
    pub invitees: Option<Vec<InviteeReq>>,
}

#[derive(Serialize)]
pub struct MeetingResp {
    pub id: Uuid,
    pub external_ref: Option<String>,
    pub title: String,
    pub starts_at: DateTime<Utc>,
    pub duration_min: i32,
    pub kind: String,
    pub room_code: String,
    pub join_url: String,
    pub host_email: String,
    /// Convidados efectivamente registados (email -> id do utilizador).
    pub invitees: Vec<InviteeResp>,
    /// Emails NÃO registados e porquê. Nunca falha o pedido por causa de um
    /// convidado — o link tem de sair na mesma.
    pub skipped: Vec<SkippedInvitee>,
    /// `true` quando o pedido reencontrou uma reunião já existente pela
    /// `external_ref` em vez de criar uma nova.
    pub existing: bool,
}

#[derive(Serialize)]
pub struct InviteeResp {
    pub email: String,
    pub user_id: Uuid,
}

#[derive(Serialize)]
pub struct SkippedInvitee {
    pub email: String,
    pub reason: String,
}

// ---------- resolução de utilizadores por email ----------

enum Resolved {
    User(Uuid),
    /// O email existe mas pertence a utilizador de OUTRA organização. Não o
    /// arrastamos para cá: uma chave de API não pode capturar utilizadores de
    /// outro tenant só por saber o endereço.
    ForeignOrg,
    Invalid,
}

/// True quando o endereço serve de identidade de conta. Exposto porque o
/// login por Odoo aplica a MESMA regra ao `email`/`login` que vem de lá — se
/// as duas divergissem, um utilizador criado por um caminho não conseguiria
/// entrar pelo outro.
pub fn is_usable_email(raw: &str) -> bool {
    normalize_email(raw).is_some()
}

fn normalize_email(raw: &str) -> Option<String> {
    let e = raw.trim().to_lowercase();
    // Validação deliberadamente mínima: o Odoo já valida, e regras apertadas
    // rejeitam endereços válidos. Só garantimos algo utilizável como chave.
    if e.len() < 3 || e.len() > 254 || !e.contains('@') || e.starts_with('@') || e.ends_with('@') {
        return None;
    }
    // Rejeita a forma com nome («Ana Silva» <ana@x.ao>) e listas separadas por
    // vírgula. Não é teoria: uma BD real de produção tem estes valores no campo
    // `email` do contacto, e sem isto criávamos um utilizador cujo email é a
    // linha toda — que depois nunca corresponde a login nenhum.
    if e.chars()
        .any(|c| c.is_whitespace() || matches!(c, '<' | '>' | ',' | ';' | '"'))
    {
        return None;
    }
    // Exactamente um '@', e algo de cada lado.
    let mut parts = e.split('@');
    let (local, domain) = (parts.next()?, parts.next()?);
    if parts.next().is_some() || local.is_empty() || !domain.contains('.') {
        return None;
    }
    Some(e)
}

/// Ver `unique_username`. Exposto para o provisionamento por login Odoo.
pub async fn unique_username_pub(db: &sqlx::PgPool, base: &str) -> String {
    unique_username(db, base).await
}

/// Deriva um `username` único (a coluna é UNIQUE em toda a plataforma).
async fn unique_username(db: &sqlx::PgPool, base: &str) -> String {
    let base: String = base
        .trim()
        .chars()
        .take(40)
        .collect::<String>()
        .trim()
        .to_string();
    let base = if base.is_empty() {
        "utilizador".to_string()
    } else {
        base
    };
    for i in 0..12 {
        let candidate = if i == 0 {
            base.clone()
        } else {
            format!("{base} {i}")
        };
        let taken: Option<(Uuid,)> = sqlx::query_as("SELECT id FROM users WHERE username = $1")
            .bind(&candidate)
            .fetch_optional(db)
            .await
            .ok()
            .flatten();
        if taken.is_none() {
            return candidate;
        }
    }
    // Último recurso: sufixo aleatório (colisão é praticamente impossível e o
    // INSERT trata a corrida na mesma).
    let mut b = [0u8; 4];
    OsRng.fill_bytes(&mut b);
    format!("{base} {}", hex::encode(b))
}

/// Resolve um email para um utilizador membro de `org_id`, criando-o se ainda
/// não existir (JIT provisioning — o mesmo modelo de `odoo::provision`).
///
/// Regra de isolamento: um utilizador que já pertença a outra organização
/// **não** é adicionado a esta. É devolvido `ForeignOrg` e o chamador decide
/// (erro, se for o anfitrião; ignorar, se for convidado).
async fn resolve_org_user(
    state: &AppState,
    org_id: Uuid,
    email: &str,
    name: Option<&str>,
) -> Result<Resolved, ApiError> {
    let Some(email) = normalize_email(email) else {
        return Ok(Resolved::Invalid);
    };

    let existing: Option<(Uuid,)> = sqlx::query_as("SELECT id FROM users WHERE email = $1")
        .bind(&email)
        .fetch_optional(&state.db)
        .await?;

    let user_id = match existing {
        Some((uid,)) => {
            let is_member: bool = sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM org_members WHERE org_id = $1 AND user_id = $2)",
            )
            .bind(org_id)
            .bind(uid)
            .fetch_one(&state.db)
            .await?;
            if is_member {
                return Ok(Resolved::User(uid));
            }
            let has_any_org: bool =
                sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM org_members WHERE user_id = $1)")
                    .bind(uid)
                    .fetch_one(&state.db)
                    .await?;
            if has_any_org {
                return Ok(Resolved::ForeignOrg);
            }
            uid
        }
        None => {
            let username = unique_username(
                &state.db,
                name.unwrap_or_else(|| email.split('@').next().unwrap_or("utilizador")),
            )
            .await;
            // Sem password: o utilizador entra por SSO ou pelo login híbrido
            // contra o Odoo (ver odoo::odoo_authenticate). Um hash vazio nunca
            // valida em `verify_password`, portanto isto não abre uma conta.
            let res: Result<(Uuid,), sqlx::Error> = sqlx::query_as(
                "INSERT INTO users (email, username, password_hash, odoo_managed)
                 VALUES ($1, $2, '', TRUE) RETURNING id",
            )
            .bind(&email)
            .bind(&username)
            .fetch_one(&state.db)
            .await;
            match res {
                Ok((uid,)) => uid,
                // Corrida: outro pedido criou-o entretanto → relê.
                Err(sqlx::Error::Database(db)) if db.is_unique_violation() => {
                    match sqlx::query_as::<_, (Uuid,)>("SELECT id FROM users WHERE email = $1")
                        .bind(&email)
                        .fetch_optional(&state.db)
                        .await?
                    {
                        Some((uid,)) => uid,
                        // Colidiu no `username`, não no email — desiste deste
                        // convidado em vez de falhar a reunião inteira.
                        None => return Ok(Resolved::Invalid),
                    }
                }
                Err(e) => return Err(e.into()),
            }
        }
    };

    sqlx::query(
        "INSERT INTO org_members (org_id, user_id, role) VALUES ($1, $2, 'member')
         ON CONFLICT (org_id, user_id) DO NOTHING",
    )
    .bind(org_id)
    .bind(user_id)
    .execute(&state.db)
    .await?;

    Ok(Resolved::User(user_id))
}

// ---------- helpers ----------

/// A reunião pertence à organização da chave? Mesma regra do
/// `GET /api/v1/meetings` já existente: o dono é membro da org.
async fn meeting_in_org(state: &AppState, org_id: Uuid, id: Uuid) -> Result<Meeting, ApiError> {
    let meeting: Option<Meeting> = sqlx::query_as(&format!(
        "SELECT {MEETING_COLS} FROM meetings m
         WHERE m.id = $1
           AND EXISTS (SELECT 1 FROM org_members om
                       WHERE om.org_id = $2 AND om.user_id = m.owner_id)"
    ))
    .bind(id)
    .bind(org_id)
    .fetch_optional(&state.db)
    .await?;
    meeting.ok_or(ApiError::NotFound)
}

async fn external_ref_of(state: &AppState, meeting_id: Uuid) -> Option<String> {
    sqlx::query_scalar("SELECT external_ref FROM meeting_external_refs WHERE meeting_id = $1")
        .bind(meeting_id)
        .fetch_optional(&state.db)
        .await
        .ok()
        .flatten()
}

async fn invitees_of(state: &AppState, meeting_id: Uuid) -> Result<Vec<InviteeResp>, ApiError> {
    let rows: Vec<(Uuid, String)> = sqlx::query_as(
        "SELECT u.id, u.email FROM meeting_invitees i
         JOIN users u ON u.id = i.user_id
         WHERE i.meeting_id = $1 ORDER BY u.email",
    )
    .bind(meeting_id)
    .fetch_all(&state.db)
    .await?;
    Ok(rows
        .into_iter()
        .map(|(user_id, email)| InviteeResp { email, user_id })
        .collect())
}

/// Regista convidados, ignorando (com motivo) os que não se podem resolver.
async fn add_invitees(
    state: &AppState,
    org_id: Uuid,
    meeting_id: Uuid,
    host_id: Uuid,
    reqs: &[InviteeReq],
) -> Result<(Vec<Uuid>, Vec<SkippedInvitee>), ApiError> {
    let mut ids = Vec::new();
    let mut skipped = Vec::new();
    for inv in reqs {
        match resolve_org_user(state, org_id, &inv.email, inv.name.as_deref()).await? {
            Resolved::User(uid) => {
                if uid == host_id {
                    continue; // o anfitrião não é convidado de si próprio
                }
                sqlx::query(
                    "INSERT INTO meeting_invitees (meeting_id, user_id) VALUES ($1, $2)
                     ON CONFLICT DO NOTHING",
                )
                .bind(meeting_id)
                .bind(uid)
                .execute(&state.db)
                .await?;
                ids.push(uid);
            }
            Resolved::ForeignOrg => skipped.push(SkippedInvitee {
                email: inv.email.trim().to_lowercase(),
                reason: "o utilizador pertence a outra organização".into(),
            }),
            Resolved::Invalid => skipped.push(SkippedInvitee {
                email: inv.email.trim().to_lowercase(),
                reason: "email inválido".into(),
            }),
        }
    }
    Ok((ids, skipped))
}

async fn build_resp(
    state: &AppState,
    org_id: Uuid,
    meeting: &Meeting,
    host_email: String,
    skipped: Vec<SkippedInvitee>,
    existing: bool,
) -> Result<MeetingResp, ApiError> {
    let room_code = meeting.room_code.clone().unwrap_or_default();
    Ok(MeetingResp {
        id: meeting.id,
        external_ref: external_ref_of(state, meeting.id).await,
        title: meeting.title.clone(),
        starts_at: meeting.starts_at,
        duration_min: meeting.duration_min,
        kind: meeting.kind.clone(),
        join_url: crate::apikeys::room_link(state, org_id, &room_code).await,
        room_code,
        host_email,
        invitees: invitees_of(state, meeting.id).await?,
        skipped,
        existing,
    })
}

async fn email_of(state: &AppState, user_id: Uuid) -> String {
    sqlx::query_scalar::<_, String>("SELECT email FROM users WHERE id = $1")
        .bind(user_id)
        .fetch_optional(&state.db)
        .await
        .ok()
        .flatten()
        .unwrap_or_default()
}

// ---------- handlers ----------

/// `POST /api/v1/meetings` — cria (ou reencontra) uma reunião agendada com
/// sala, anfitrião e convidados.
pub async fn create(
    State(state): State<Arc<AppState>>,
    key: ApiKeyAuth,
    Json(req): Json<CreateMeetingReq>,
) -> Result<Json<MeetingResp>, ApiError> {
    let title: String = req.title.trim().chars().take(120).collect();
    if title.is_empty() {
        return Err(ApiError::BadRequest("title é obrigatório".into()));
    }
    if !matches!(req.kind.as_str(), "video" | "voice") {
        return Err(ApiError::BadRequest(
            "kind tem de ser 'video' ou 'voice'".into(),
        ));
    }
    if !(1..=24 * 60).contains(&req.duration_min) {
        return Err(ApiError::BadRequest(
            "duration_min tem de estar entre 1 e 1440".into(),
        ));
    }
    if req.invitees.len() > 200 {
        return Err(ApiError::BadRequest("máximo de 200 convidados".into()));
    }
    let external_ref: Option<String> = req
        .external_ref
        .as_deref()
        .map(|r| r.trim().chars().take(200).collect::<String>())
        .filter(|r| !r.is_empty());

    // Idempotência: já existe uma reunião para esta referência nesta org?
    if let Some(ext) = &external_ref {
        let found: Option<(Uuid,)> = sqlx::query_as(
            "SELECT meeting_id FROM meeting_external_refs
             WHERE org_id = $1 AND external_ref = $2",
        )
        .bind(key.org_id)
        .bind(ext)
        .fetch_optional(&state.db)
        .await?;
        if let Some((meeting_id,)) = found {
            // A linha de mapeamento cai em CASCATA com a reunião (migração 0031),
            // portanto se a linha existe a reunião existe. Quando mesmo assim não
            // se consegue resolver, o estado é incoerente e dizê-lo é o correcto.
            //
            // BUG CORRIGIDO AQUI: isto era `if let Ok(...)`, e o `Err` caía para o
            // "criar de novo" logo abaixo. Esse caminho não podia ter sucesso: a
            // linha de mapeamento velha continua lá, portanto o INSERT do
            // `external_ref` colide sempre; o tratamento da colisão APAGA a
            // reunião e a sala acabadas de criar, relê o MESMO `meeting_id` que
            // já falhara e propaga o erro à mesma. Ou seja — devolvia erro na
            // mesma, depois de escrever e apagar na base de dados para nada.
            let meeting = meeting_in_org(&state, key.org_id, meeting_id).await?;
            let host_email = email_of(&state, meeting.owner_id).await;
            return Ok(Json(
                build_resp(&state, key.org_id, &meeting, host_email, vec![], true).await?,
            ));
        }
    }

    // Anfitrião: sem um utilizador humano real a sala fica sem quem admita.
    let host_email = normalize_email(&req.host_email)
        .ok_or_else(|| ApiError::BadRequest("host_email inválido".into()))?;
    let host_id =
        match resolve_org_user(&state, key.org_id, &host_email, req.host_name.as_deref()).await? {
            Resolved::User(uid) => uid,
            Resolved::ForeignOrg => {
                return Err(ApiError::Conflict(
                    "o anfitrião pertence a outra organização".into(),
                ))
            }
            Resolved::Invalid => return Err(ApiError::BadRequest("host_email inválido".into())),
        };

    // A sala nasce já aqui (e não só no `start`) porque o chamador precisa do
    // link para o gravar no evento de calendário dele.
    let room = crate::rooms::insert_room(
        &state.db,
        host_id,
        &title,
        "sfu",
        req.waiting_room,
        req.e2ee,
        "normal",
    )
    .await?;

    let meeting: Meeting = sqlx::query_as(&format!(
        "INSERT INTO meetings (owner_id, title, description, kind, starts_at, duration_min, room_code)
         VALUES ($1, $2, $3, $4, $5, $6, $7)
         RETURNING {MEETING_COLS}"
    ))
    .bind(host_id)
    .bind(&title)
    .bind(req.description.trim().chars().take(4000).collect::<String>())
    .bind(&req.kind)
    .bind(req.starts_at)
    .bind(req.duration_min)
    .bind(&room.code)
    .fetch_one(&state.db)
    .await?;

    if let Some(ext) = &external_ref {
        let res = sqlx::query(
            "INSERT INTO meeting_external_refs (org_id, external_ref, meeting_id)
             VALUES ($1, $2, $3)",
        )
        .bind(key.org_id)
        .bind(ext)
        .bind(meeting.id)
        .execute(&state.db)
        .await;
        if let Err(sqlx::Error::Database(db)) = &res {
            if db.is_unique_violation() {
                // Corrida com outro pedido para a mesma referência: desfaz o
                // que criámos e devolve o vencedor. A sala cai por cascata
                // não — `rooms` não referencia `meetings`, por isso é
                // apagada explicitamente.
                let _ = sqlx::query("DELETE FROM meetings WHERE id = $1")
                    .bind(meeting.id)
                    .execute(&state.db)
                    .await;
                let _ = sqlx::query("DELETE FROM rooms WHERE id = $1")
                    .bind(room.id)
                    .execute(&state.db)
                    .await;
                let (winner,): (Uuid,) = sqlx::query_as(
                    "SELECT meeting_id FROM meeting_external_refs
                     WHERE org_id = $1 AND external_ref = $2",
                )
                .bind(key.org_id)
                .bind(ext)
                .fetch_one(&state.db)
                .await?;
                let meeting = meeting_in_org(&state, key.org_id, winner).await?;
                let host_email = email_of(&state, meeting.owner_id).await;
                return Ok(Json(
                    build_resp(&state, key.org_id, &meeting, host_email, vec![], true).await?,
                ));
            }
        }
        res?;
    }

    let (_, skipped) = add_invitees(&state, key.org_id, meeting.id, host_id, &req.invitees).await?;

    crate::meetings::fire_meeting_webhook(&state, &meeting, host_id, "meeting.created").await;
    crate::audit::log(
        &state.db,
        Some(key.org_id),
        host_id,
        "meeting.created_via_api",
        &title,
    )
    .await;

    Ok(Json(
        build_resp(&state, key.org_id, &meeting, host_email, skipped, false).await?,
    ))
}

/// `PATCH /api/v1/meetings/{id}` — reagendar / renomear / actualizar convidados.
pub async fn patch(
    State(state): State<Arc<AppState>>,
    key: ApiKeyAuth,
    Path(id): Path<Uuid>,
    Json(req): Json<PatchMeetingReq>,
) -> Result<Json<MeetingResp>, ApiError> {
    let current = meeting_in_org(&state, key.org_id, id).await?;

    let title: Option<String> = match &req.title {
        Some(t) => {
            let t: String = t.trim().chars().take(120).collect();
            if t.is_empty() {
                return Err(ApiError::BadRequest("title não pode ser vazio".into()));
            }
            Some(t)
        }
        None => None,
    };
    if let Some(d) = req.duration_min {
        if !(1..=24 * 60).contains(&d) {
            return Err(ApiError::BadRequest(
                "duration_min tem de estar entre 1 e 1440".into(),
            ));
        }
    }

    let meeting: Meeting = sqlx::query_as(&format!(
        "UPDATE meetings SET
            title = COALESCE($2, title),
            description = COALESCE($3, description),
            starts_at = COALESCE($4, starts_at),
            duration_min = COALESCE($5, duration_min)
         WHERE id = $1
         RETURNING {MEETING_COLS}"
    ))
    .bind(id)
    .bind(&title)
    .bind(
        req.description
            .as_ref()
            .map(|d| d.trim().chars().take(4000).collect::<String>()),
    )
    .bind(req.starts_at)
    .bind(req.duration_min)
    .fetch_one(&state.db)
    .await?;

    // O nome da sala segue o título — é o que aparece na UI de quem entra.
    if let (Some(t), Some(code)) = (&title, &current.room_code) {
        let _ = sqlx::query("UPDATE rooms SET name = $1 WHERE code = $2")
            .bind(t)
            .bind(code)
            .execute(&state.db)
            .await;
    }

    let mut skipped = Vec::new();
    if let Some(list) = &req.invitees {
        if list.len() > 200 {
            return Err(ApiError::BadRequest("máximo de 200 convidados".into()));
        }
        let (_keep, sk) =
            add_invitees(&state, key.org_id, meeting.id, meeting.owner_id, list).await?;
        skipped = sk;
        // Remove quem saiu da lista — decidido pelos EMAILS PEDIDOS, não pelos
        // que resolveram para um utilizador.
        //
        // BUG CORRIGIDO AQUI: a condição era `NOT (user_id = ANY(<resolvidos>))`.
        // Um convidado que o `add_invitees` devolvia como `skipped` (de outra
        // org, ou email inválido) não entra nos resolvidos — e era portanto
        // APAGADO da reunião, apesar de o chamador o ter mesmo listado e de a
        // resposta lhe dizer «ignorado», não «removido». Pior: com a lista toda
        // ignorada os resolvidos ficam vazios, e em Postgres
        // `x = ANY('{}')` é FALSE, logo `NOT FALSE` é TRUE para todas as linhas
        // — a lista de convidados inteira desaparecia.
        //
        // Pelo email, quem foi pedido fica, tenha ou não sido possível
        // (re)adicioná-lo, e quem o chamador omitiu sai. Quem se manteve conserva
        // o `status` (aceite/recusado) — reescrever o conjunto todo perderia
        // respostas.
        let requested: Vec<String> = list
            .iter()
            .filter_map(|i| normalize_email(&i.email))
            .collect();
        sqlx::query(
            "DELETE FROM meeting_invitees mi
             USING users u
             WHERE mi.meeting_id = $1
               AND u.id = mi.user_id
               AND NOT (lower(u.email) = ANY($2))",
        )
        .bind(meeting.id)
        .bind(&requested)
        .execute(&state.db)
        .await?;
    }

    let host_email = email_of(&state, meeting.owner_id).await;
    Ok(Json(
        build_resp(&state, key.org_id, &meeting, host_email, skipped, false).await?,
    ))
}

/// `POST /api/v1/meetings/{id}/ring` — toca nos dispositivos dos convidados.
///
/// É o "começar agora" a partir de um sistema externo: o anfitrião carrega em
/// iniciar no ERP e o Delonix faz aparecer a chamada no ecrã de cada convidado,
/// como uma chamada de telefone — não uma notificação que se perde no meio de
/// outras.
///
/// Já existia auto-ring por cron um minuto antes da hora marcada
/// (`meetings::ring_upcoming_meetings`); o que faltava era **sob comando**.
/// Reutiliza a mesma mecânica para o comportamento ser idêntico nos dois
/// casos, incluindo o `register_call` — sem ele, quem atende cairia na sala
/// de espera em vez de entrar directo.
pub async fn ring(
    State(state): State<Arc<AppState>>,
    key: ApiKeyAuth,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let meeting = meeting_in_org(&state, key.org_id, id).await?;
    let Some(room_code) = meeting.room_code.clone() else {
        return Err(ApiError::BadRequest(
            "a reunião ainda não tem sala — criar primeiro".into(),
        ));
    };

    let owner_name: String = sqlx::query_scalar("SELECT username FROM users WHERE id = $1")
        .bind(meeting.owner_id)
        .fetch_optional(&state.db)
        .await?
        .unwrap_or_default();

    // Quem já está na sala não deve ser incomodado.
    let already_in: std::collections::HashSet<Uuid> =
        match sqlx::query_as::<_, (Uuid,)>("SELECT id FROM rooms WHERE code = $1")
            .bind(&room_code)
            .fetch_optional(&state.db)
            .await?
        {
            Some((rid,)) => state.hub.users_in_room(rid),
            None => Default::default(),
        };

    let invitees: Vec<(Uuid,)> = sqlx::query_as(
        "SELECT user_id FROM meeting_invitees
         WHERE meeting_id = $1 AND status <> 'declined'",
    )
    .bind(id)
    .fetch_all(&state.db)
    .await?;

    let targets: std::collections::HashSet<Uuid> = invitees
        .into_iter()
        .map(|(uid,)| uid)
        .filter(|uid| !already_in.contains(uid) && *uid != meeting.owner_id)
        .collect();

    if targets.is_empty() {
        return Ok(Json(serde_json::json!({
            "ringing": [], "offline": [],
            "reason": "sem convidados por chamar (já na sala, ou nenhum)",
            "join_url": crate::apikeys::room_link(&state, key.org_id, &room_code).await,
        })));
    }

    state
        .presence
        .register_call(room_code.clone(), meeting.owner_id, targets.clone());
    let (ringing, offline) = crate::presence::ring_users(
        &state,
        meeting.owner_id,
        &owner_name,
        targets,
        &room_code,
        &meeting.kind,
        &meeting.title,
    )
    .await;

    crate::meetings::fire_meeting_webhook(&state, &meeting, meeting.owner_id, "meeting.started")
        .await;
    tracing::info!(%id, ringing = ringing.len(), offline = offline.len(), "ring por API");

    Ok(Json(serde_json::json!({
        "ringing": ringing,
        "offline": offline,
        "room_code": room_code,
        "join_url": crate::apikeys::room_link(&state, key.org_id, &room_code).await,
    })))
}

/// `DELETE /api/v1/meetings/{id}` — cancela a reunião e liberta a sala.
///
/// A sala só é apagada se não tiver gravações: a reunião pode ter acontecido
/// e sido cancelada no calendário depois, e uma gravação é um artefacto que o
/// utilizador espera manter (a FK `recordings.room_id` é ON DELETE CASCADE —
/// apagar a sala apagaria o registo da gravação).
pub async fn delete(
    State(state): State<Arc<AppState>>,
    key: ApiKeyAuth,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let meeting = meeting_in_org(&state, key.org_id, id).await?;

    sqlx::query("DELETE FROM meetings WHERE id = $1")
        .bind(id)
        .execute(&state.db)
        .await?;

    let mut room_deleted = false;
    if let Some(code) = &meeting.room_code {
        let res = sqlx::query(
            "DELETE FROM rooms r WHERE r.code = $1
               AND NOT EXISTS (SELECT 1 FROM recordings rec WHERE rec.room_id = r.id)",
        )
        .bind(code)
        .execute(&state.db)
        .await?;
        room_deleted = res.rows_affected() > 0;
    }

    crate::audit::log(
        &state.db,
        Some(key.org_id),
        meeting.owner_id,
        "meeting.deleted_via_api",
        &meeting.title,
    )
    .await;

    Ok(Json(serde_json::json!({
        "ok": true,
        "room_deleted": room_deleted,
    })))
}

#[cfg(test)]
mod tests {
    use super::normalize_email;

    #[test]
    fn normalizes_and_rejects() {
        assert_eq!(
            normalize_email("  Ana@Kaeso.CO "),
            Some("ana@kaeso.co".into())
        );
        assert_eq!(normalize_email("sem-arroba"), None);
        assert_eq!(normalize_email("@sodominio.ao"), None);
        assert_eq!(normalize_email("local@"), None);
        assert_eq!(normalize_email(""), None);
        // Sem domínio com ponto não é um endereço utilizável.
        assert_eq!(normalize_email("ana@localhost"), None);
    }

    #[test]
    fn rejects_display_name_and_lists() {
        // Valores REAIS encontrados no campo `email` de contactos numa BD de
        // produção. Aceitá-los criaria um utilizador cujo email é a linha
        // inteira — que nunca corresponde a login nenhum.
        assert_eq!(normalize_email("\"Helder T\" <helder.t@expro.com>"), None);
        assert_eq!(normalize_email("a@kaeso.co, b@kaeso.co"), None);
        assert_eq!(normalize_email("a@kaeso.co; b@kaeso.co"), None);
        assert_eq!(normalize_email("ana silva@kaeso.co"), None);
        assert_eq!(normalize_email("a@b@kaeso.co"), None);
    }
}
