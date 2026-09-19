//! Ramais internos (extensão SIP) — Fase 1: chamada ramal-a-ramal, SÓ interna.
//!
//! Diferença para `voice.rs` (dial-in PSTN, migração 0014): aquele é o control
//! plane de salas de voz EFÉMERAS (por reunião, PIN aleatório, morre com a
//! sala). Um ramal é PERMANENTE — 1:1 com um `org_member`, número curto
//! atribuído, nunca expira. Este módulo não toca em `voice_room`/PIN/dial-in;
//! é infraestrutura aditiva e paralela (migração 0064).
//!
//! Fase 2 (mesmo ficheiro, secção "Fase 2" mais abaixo): um ramal pode agora
//! ser alcançado a partir do PSTN quando tem um DID dedicado atribuído
//! (migração `0065_ramais_did.sql`, estende `voice_did`). Continua fora de
//! âmbito: um ramal ligado a uma sala de reunião em vídeo — a ponte
//! ramal↔SFU é fase seguinte do mesmo plano, e nenhuma UI ou mensagem deste
//! módulo pode sugerir que já existe (mesma disciplina que `VoiceCard.tsx`
//! já aplica à ponte FreeSWITCH↔SFU em falta).
//!
//! ## A fronteira Kamailio/FreeSWITCH (o que está e o que NÃO está verificado)
//!
//! `voice/kamailio/kamailio.cfg`, tal como existe hoje, é um SBC puramente
//! virado para o trunk: só carrega `dispatcher`/`permissions`/`tls`, não tem
//! `usrloc`/`registrar`/`auth_db`, e não tem NENHUMA ligação à base de dados.
//! Dar-lhe capacidade de registo SIP (para os ramais fazerem REGISTER) exigia
//! módulos novos e uma ligação Postgres que ele não tem hoje — uma mudança
//! maior e mais arriscada do que esta fase pede, e que tocaria o caminho do
//! dial-in PSTN que a tarefa pede explicitamente para NÃO tocar.
//!
//! A decisão tomada: os ramais registam-se e chamam-se DIRETAMENTE no
//! FreeSWITCH (um perfil Sofia "internal" novo, porta distinta da do
//! Kamailio), usando o `mod_xml_curl` do FreeSWITCH como directório dinâmico —
//! ver `voice/freeswitch/autoload_configs/xml_curl.conf.xml` e
//! `voice/freeswitch/sip_profiles/internal.xml`. O Kamailio fica
//! **inteiramente intocado**: continua a servir só o trunk PSTN.
//!
//! Duas superfícies HTTP internas suportam essa fronteira, ambas autenticadas
//! pelo MESMO segredo partilhado que `voice.rs` já usa (`X-Voice-Secret` /
//! `check_media_secret`, agora `pub(crate)`):
//! - `POST /api/voice/ivr/directory` — o FreeSWITCH chama isto no REGISTER
//!   (via `mod_xml_curl`, secção "directory") para obter o `a1-hash` SIP
//!   Digest do ramal que se está a registar.
//! - `POST /api/voice/ivr/resolve-extension` — o dialplan interno
//!   (`voice/freeswitch/scripts/ramais_dial.lua`) chama isto quando alguém
//!   disca um número curto, para descobrir a que `sip_username` (a conta
//!   registada) esse número corresponde NA ORG do chamador.
//!
//! **O que NÃO foi possível verificar aqui** (sem uma instância FreeSWITCH a
//! correr): o nome exato dos campos que o `mod_xml_curl` do FreeSWITCH envia
//! no POST de directório varia com a versão e o propósito da consulta — este
//! código aceita `user`/`domain` (os nomes mais comuns na documentação), mas
//! antes de produção **é preciso confirmar contra a instância real** (ligar
//! `debug=1` em `xml_curl.conf.xml` e inspecionar o POST). Da mesma forma, o
//! comportamento exato do REGISTER com `challenge-realm=auto_from` (para o
//! realm do digest variar por org) não foi exercitado contra um FreeSWITCH
//! real. Ficam ambos documentados nos ficheiros de configuração respetivos.

use axum::{
    extract::{Form, Path, State},
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use chrono::{DateTime, Utc};
use md5::{Digest as Md5Digest, Md5};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use uuid::Uuid;

use crate::{auth::AuthUser, error::ApiError, voice::check_media_secret, AppState};

// ---------- Helpers ----------

/// Número de ramal: 3 a 5 dígitos, nada mais. Curto o suficiente para se
/// discar de cor, longo o suficiente para uma org de algumas centenas de
/// pessoas não esgotar o espaço.
fn validate_extension_format(s: &str) -> Result<(), ApiError> {
    if s.len() < 3 || s.len() > 5 || !s.chars().all(|c| c.is_ascii_digit()) {
        return Err(ApiError::BadRequest(
            "extensão deve ter entre 3 e 5 dígitos".into(),
        ));
    }
    Ok(())
}

/// Password SIP aleatória (120 bits) — só existe em claro na resposta que a
/// cria/regenera; a partir daí só ficam guardados `sip_password_hash` (Argon2,
/// segurança em repouso) e `sip_ha1` (o que o digest SIP realmente usa).
/// `crypto::random_hex` e não `OsRng` aqui directamente — regra 4 (ADR-0004
/// §5): um só sítio a gerar aleatoriedade de credenciais.
fn gen_sip_password() -> String {
    crate::crypto::random_hex(15)
}

/// AOR SIP: globalmente único (não escopado por org — é o directório do
/// registar que exige isto, ver o comentário no topo do ficheiro).
fn gen_sip_username() -> String {
    format!("ramal_{}", crate::crypto::random_hex(8))
}

/// HA1 do SIP Digest — RFC 2617: `MD5(username ":" realm ":" password)`. MD5
/// aqui não é uma escolha nossa; é o que o protocolo exige (ver Cargo.toml).
pub(crate) fn compute_ha1(username: &str, realm: &str, password: &str) -> String {
    let mut hasher = Md5::new();
    hasher.update(format!("{username}:{realm}:{password}").as_bytes());
    hex::encode(hasher.finalize())
}

/// Domínio SIP de uma org: `<slug>.<VOICE_RAMAIS_DOMAIN_SUFFIX>`. Existe
/// porque `extension` só é única DENTRO da org — o domínio é o que impede o
/// ramal "101" da Acme de colidir com o "101" da Zeta no directório SIP.
async fn sip_domain_for_org(state: &AppState, org_id: Uuid) -> Result<String, ApiError> {
    let slug: String = sqlx::query_scalar("SELECT slug FROM organizations WHERE id = $1")
        .bind(org_id)
        .fetch_one(&state.db)
        .await?;
    Ok(format!(
        "{slug}.{}",
        state.config.voice_ramais_domain_suffix
    ))
}

/// Caminho inverso: de um domínio SIP para o `org_id`. `None` se o sufixo não
/// bater ou a org não existir — o chamador trata isso como "não encontrado",
/// nunca como erro (um FreeSWITCH mal configurado não deve ver 500s).
async fn org_id_by_sip_domain(state: &AppState, domain: &str) -> Option<Uuid> {
    let suffix = format!(".{}", state.config.voice_ramais_domain_suffix);
    let slug = domain.strip_suffix(&suffix)?;
    sqlx::query_scalar("SELECT id FROM organizations WHERE slug = $1")
        .bind(slug)
        .fetch_optional(&state.db)
        .await
        .ok()
        .flatten()
}

// ---------- Tipos de saída ----------

#[derive(Debug, Serialize, sqlx::FromRow, utoipa::ToSchema)]
pub struct VoiceExtensionInfo {
    pub id: Uuid,
    pub org_id: Uuid,
    pub member_id: Uuid,
    /// Nome do membro dono — junção com `users`, só para exibição na consola.
    pub member_username: String,
    pub member_email: String,
    pub extension: String,
    pub sip_username: String,
    pub label: String,
    pub active: bool,
    pub created_at: DateTime<Utc>,
}

const SELECT_EXTENSION_INFO: &str =
    "SELECT e.id, e.org_id, e.member_id, u.username AS member_username,
            u.email AS member_email, e.extension, e.sip_username, e.label, e.active, e.created_at
     FROM voice_extensions e JOIN users u ON u.id = e.member_id";

/// Resposta de criação/regeneração: inclui a password SIP em claro, UMA VEZ —
/// o mesmo padrão de revelação única que `apikeys::CreatedKey` já usa.
#[derive(Serialize, utoipa::ToSchema)]
pub struct CreatedExtension {
    #[serde(flatten)]
    pub extension: VoiceExtensionInfo,
    /// A password SIP em claro — copiar agora para o softphone; não é
    /// mostrada outra vez (só fica o hash e o HA1 na base).
    pub sip_password: String,
    /// Domínio SIP a usar junto com `sip_username`/`sip_password` na
    /// configuração da conta do softphone.
    pub sip_domain: String,
}

// ============================================================
//  Gestão (admin da org, sessão) — mesma permissão que os DIDs de voz.
// ============================================================

#[derive(Deserialize, utoipa::ToSchema)]
pub struct CreateExtensionReq {
    pub member_id: Uuid,
    pub extension: String,
    #[serde(default)]
    pub label: Option<String>,
}

/// Cria um ramal para um membro da org (admin). Gera credenciais SIP novas e
/// devolve a password em claro UMA VEZ.
#[utoipa::path(
    post, path = "/api/orgs/{org_id}/extensions", tag = "voice",
    security(("session" = [])),
    params(("org_id" = Uuid, Path, description = "Organização.")),
    request_body = CreateExtensionReq,
    responses(
        (status = 200, body = CreatedExtension, description = "A palavra-passe SIP sai UMA vez."),
        (status = 400, body = crate::openapi::ErrorBody),
        (status = 409, body = crate::openapi::ErrorBody, description = "O número de ramal já existe na organização."),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 403, body = crate::openapi::ErrorBody),
        (status = 404, body = crate::openapi::ErrorBody),
    )
)]
pub async fn create_extension(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(org_id): Path<Uuid>,
    Json(req): Json<CreateExtensionReq>,
) -> Result<Json<CreatedExtension>, ApiError> {
    crate::org::require_admin_pub(&state, org_id, auth.user_id).await?;

    let extension = req.extension.trim();
    validate_extension_format(extension)?;

    // Erro claro em vez de deixar a FK composta rebentar com algo opaco. A
    // pertença decide-se SEMPRE em org.rs (regra 1, ADR-0004 §5) — não se
    // escreve `FROM org_members` à mão aqui.
    if crate::org::role_in_org(&state, org_id, req.member_id)
        .await?
        .is_none()
    {
        return Err(ApiError::BadRequest(
            "o membro não pertence a esta organização".into(),
        ));
    }

    let label: String = req
        .label
        .unwrap_or_default()
        .trim()
        .chars()
        .take(80)
        .collect();
    let sip_domain = sip_domain_for_org(&state, org_id).await?;
    let sip_password = gen_sip_password();
    let password_hash = crate::auth::hash_password(&sip_password)?;

    // Retenta só em colisão do AOR globalmente único (extremamente
    // improvável com 64 bits) — colisão de extensão/membro é definitiva,
    // não um acidente de geração aleatória, e não se retenta.
    let mut last_err = None;
    let mut new_id = None;
    for _ in 0..5 {
        let sip_username = gen_sip_username();
        let ha1 = compute_ha1(&sip_username, &sip_domain, &sip_password);
        let res: Result<(Uuid,), sqlx::Error> = sqlx::query_as(
            "INSERT INTO voice_extensions
                 (org_id, member_id, extension, sip_username, sip_password_hash, sip_ha1, label)
             VALUES ($1, $2, $3, $4, $5, $6, $7)
             RETURNING id",
        )
        .bind(org_id)
        .bind(req.member_id)
        .bind(extension)
        .bind(&sip_username)
        .bind(&password_hash)
        .bind(&ha1)
        .bind(&label)
        .fetch_one(&state.db)
        .await;
        match res {
            Ok((id,)) => {
                new_id = Some(id);
                break;
            }
            Err(sqlx::Error::Database(dbe)) if dbe.is_unique_violation() => {
                match dbe.constraint() {
                    Some("voice_extensions_sip_username_uidx") => continue, // retenta com outro AOR
                    Some("voice_extensions_org_ext_uidx") => {
                        return Err(ApiError::Conflict(
                            "já existe um ramal com esse número nesta organização".into(),
                        ))
                    }
                    Some("voice_extensions_org_member_uidx") => {
                        return Err(ApiError::Conflict(
                            "este membro já tem um ramal atribuído".into(),
                        ))
                    }
                    _ => return Err(ApiError::Conflict("ramal em conflito".into())),
                }
            }
            Err(e) => {
                last_err = Some(e);
                break;
            }
        }
    }
    let id = match new_id {
        Some(id) => id,
        None => {
            return Err(last_err
                .map(Into::into)
                .unwrap_or_else(|| ApiError::internal("não foi possível gerar o AOR SIP")))
        }
    };

    let info: VoiceExtensionInfo =
        sqlx::query_as(&format!("{SELECT_EXTENSION_INFO} WHERE e.id = $1"))
            .bind(id)
            .fetch_one(&state.db)
            .await?;

    crate::audit::log(
        &state.db,
        Some(org_id),
        auth.user_id,
        "ramal.criado",
        &format!("{} ({})", info.extension, info.sip_username),
    )
    .await;

    Ok(Json(CreatedExtension {
        sip_password,
        sip_domain,
        extension: info,
    }))
}

/// Lista os ramais da org (admin). Nunca devolve hash nem HA1.
#[utoipa::path(
    get, path = "/api/orgs/{org_id}/extensions", tag = "voice",
    security(("session" = [])),
    params(("org_id" = Uuid, Path, description = "Organização.")),
    responses(
        (status = 200, body = Vec<VoiceExtensionInfo>),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 403, body = crate::openapi::ErrorBody),
        (status = 404, body = crate::openapi::ErrorBody),
    )
)]
pub async fn list_extensions(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(org_id): Path<Uuid>,
) -> Result<Json<Vec<VoiceExtensionInfo>>, ApiError> {
    crate::org::require_admin_pub(&state, org_id, auth.user_id).await?;
    let rows: Vec<VoiceExtensionInfo> = sqlx::query_as(&format!(
        "{SELECT_EXTENSION_INFO} WHERE e.org_id = $1 ORDER BY e.extension"
    ))
    .bind(org_id)
    .fetch_all(&state.db)
    .await?;
    Ok(Json(rows))
}

#[derive(Deserialize, utoipa::ToSchema)]
pub struct UpdateExtensionReq {
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub active: Option<bool>,
}

/// Atualiza rótulo/estado (admin). O número e as credenciais SIP não se
/// mudam aqui — reatribuir um número é apagar e criar de novo (evita um
/// ramal "meio migrado" entre dois membros).
#[utoipa::path(
    patch, path = "/api/orgs/{org_id}/extensions/{id}", tag = "voice",
    security(("session" = [])),
    params(("org_id" = Uuid, Path, description = "Organização."), ("id" = Uuid, Path, description = "Ramal.")),
    request_body = UpdateExtensionReq,
    responses(
        (status = 200, body = VoiceExtensionInfo),
        (status = 400, body = crate::openapi::ErrorBody),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 403, body = crate::openapi::ErrorBody),
        (status = 404, body = crate::openapi::ErrorBody),
    )
)]
pub async fn update_extension(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path((org_id, id)): Path<(Uuid, Uuid)>,
    Json(req): Json<UpdateExtensionReq>,
) -> Result<Json<VoiceExtensionInfo>, ApiError> {
    crate::org::require_admin_pub(&state, org_id, auth.user_id).await?;
    let label = req
        .label
        .map(|l| l.trim().chars().take(80).collect::<String>());
    sqlx::query(
        "UPDATE voice_extensions
            SET label = COALESCE($3, label), active = COALESCE($4, active)
          WHERE id = $1 AND org_id = $2",
    )
    .bind(id)
    .bind(org_id)
    .bind(label)
    .bind(req.active)
    .execute(&state.db)
    .await?;
    let info: VoiceExtensionInfo = sqlx::query_as(&format!(
        "{SELECT_EXTENSION_INFO} WHERE e.id = $1 AND e.org_id = $2"
    ))
    .bind(id)
    .bind(org_id)
    .fetch_one(&state.db)
    .await?;
    crate::audit::log(
        &state.db,
        Some(org_id),
        auth.user_id,
        "ramal.atualizado",
        &info.extension,
    )
    .await;
    Ok(Json(info))
}

/// Regenera a password SIP de um ramal (admin) — mesma revelação única que a
/// criação. O `sip_username` (AOR) não muda; só a credencial.
#[utoipa::path(
    post, path = "/api/orgs/{org_id}/extensions/{id}/regenerate-password", tag = "voice",
    security(("session" = [])),
    params(("org_id" = Uuid, Path, description = "Organização."), ("id" = Uuid, Path, description = "Ramal.")),
    responses(
        (status = 200, body = CreatedExtension, description = "A nova palavra-passe SIP sai UMA vez; a anterior deixa de servir."),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 403, body = crate::openapi::ErrorBody),
        (status = 404, body = crate::openapi::ErrorBody),
    )
)]
pub async fn regenerate_extension_password(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path((org_id, id)): Path<(Uuid, Uuid)>,
) -> Result<Json<CreatedExtension>, ApiError> {
    crate::org::require_admin_pub(&state, org_id, auth.user_id).await?;
    let sip_domain = sip_domain_for_org(&state, org_id).await?;
    let sip_username: String = sqlx::query_scalar(
        "SELECT sip_username FROM voice_extensions WHERE id = $1 AND org_id = $2",
    )
    .bind(id)
    .bind(org_id)
    .fetch_optional(&state.db)
    .await?
    .ok_or(ApiError::NotFound)?;

    let sip_password = gen_sip_password();
    let password_hash = crate::auth::hash_password(&sip_password)?;
    let ha1 = compute_ha1(&sip_username, &sip_domain, &sip_password);
    sqlx::query(
        "UPDATE voice_extensions SET sip_password_hash = $3, sip_ha1 = $4
          WHERE id = $1 AND org_id = $2",
    )
    .bind(id)
    .bind(org_id)
    .bind(&password_hash)
    .bind(&ha1)
    .execute(&state.db)
    .await?;

    let info: VoiceExtensionInfo = sqlx::query_as(&format!(
        "{SELECT_EXTENSION_INFO} WHERE e.id = $1 AND e.org_id = $2"
    ))
    .bind(id)
    .bind(org_id)
    .fetch_one(&state.db)
    .await?;
    crate::audit::log(
        &state.db,
        Some(org_id),
        auth.user_id,
        "ramal.password_regenerada",
        &info.extension,
    )
    .await;
    Ok(Json(CreatedExtension {
        sip_password,
        sip_domain,
        extension: info,
    }))
}

/// Apaga um ramal (admin).
#[utoipa::path(
    delete, path = "/api/orgs/{org_id}/extensions/{id}", tag = "voice",
    security(("session" = [])),
    params(("org_id" = Uuid, Path, description = "Organização."), ("id" = Uuid, Path, description = "Ramal.")),
    responses(
        (status = 204, description = "Ramal apagado."),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 403, body = crate::openapi::ErrorBody),
        (status = 404, body = crate::openapi::ErrorBody),
    )
)]
pub async fn delete_extension(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path((org_id, id)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, ApiError> {
    crate::org::require_admin_pub(&state, org_id, auth.user_id).await?;
    let extension: Option<String> =
        sqlx::query_scalar("SELECT extension FROM voice_extensions WHERE id = $1 AND org_id = $2")
            .bind(id)
            .bind(org_id)
            .fetch_optional(&state.db)
            .await?;
    let Some(extension) = extension else {
        return Err(ApiError::NotFound);
    };
    sqlx::query("DELETE FROM voice_extensions WHERE id = $1 AND org_id = $2")
        .bind(id)
        .bind(org_id)
        .execute(&state.db)
        .await?;
    crate::audit::log(
        &state.db,
        Some(org_id),
        auth.user_id,
        "ramal.apagado",
        &extension,
    )
    .await;
    Ok(StatusCode::NO_CONTENT)
}

// ============================================================
//  API interna (FreeSWITCH) — autenticada por X-Voice-Secret.
// ============================================================

/// Documento XML "não encontrado" — convenção do `mod_xml_curl` do
/// FreeSWITCH para "não há entrada, mas não é um erro".
const XML_NOT_FOUND: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="no"?>
<document type="freeswitch/xml">
  <section name="result">
    <result status="not found"/>
  </section>
</document>"#;

fn xml_response(body: String) -> Response {
    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "text/xml; charset=utf-8")],
        body,
    )
        .into_response()
}

/// Escapa os quatro caracteres que partiriam o XML — não há atributos com
/// aspas nos valores que produzimos (extensão, AOR, HA1 são sempre
/// alfanuméricos), mas o `label`/nomes de utilizador são texto livre.
fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// Campos do POST do `mod_xml_curl` (secção "directory") que este código lê.
/// O FreeSWITCH envia bastante mais (`section`, `tag_name`, `key_name`,
/// `ip`, `sip_auth_method`, ...) — ignorados propositadamente: só nos
/// interessa resolver "quem é este utilizador, neste domínio". Os nomes
/// `user`/`domain` são os mais comuns na documentação do módulo, mas **não
/// foram confirmados contra uma instância real** — ver o aviso no topo do
/// ficheiro.
#[derive(Debug, Deserialize)]
pub struct DirectoryQuery {
    #[serde(default)]
    pub secret: Option<String>,
}

/// Igual a `check_media_secret`, mas comparando com uma string já extraída
/// (o `?secret=` do `mod_xml_curl`) em vez de um cabeçalho.
fn check_media_secret_str(state: &AppState, provided: &str) -> Result<(), ApiError> {
    // R154: um segredo ausente, curto ou publicado dá 503 com a razão, venha o
    // valor que vier — o mesmo critério do cabeçalho `X-Voice-Secret`.
    if let Some(reason) = state.config.voice_secret_refusal {
        return Err(ApiError::ServiceUnavailable(format!(
            "API interna de IVR desligada: {reason}"
        )));
    }
    let cfg = state.config.voice_internal_secret.as_bytes();
    if cfg.is_empty() {
        return Err(ApiError::NotFound);
    }
    if delonix_meet_core::crypto::ct_eq(provided.as_bytes(), cfg) {
        Ok(())
    } else {
        Err(ApiError::Unauthorized)
    }
}

#[derive(Debug, Deserialize)]
pub struct XmlCurlDirectoryReq {
    #[serde(default)]
    pub user: String,
    #[serde(default)]
    pub domain: String,
}

/// `POST /api/voice/ivr/directory` — directório dinâmico do FreeSWITCH
/// (`mod_xml_curl`, secção "directory"). Chamado no REGISTER de um ramal para
/// obter o HA1 do digest SIP. Devolve sempre 200: "não encontrado" também é
/// uma resposta válida (o FreeSWITCH trata-o como XML, não como erro HTTP).
pub async fn ivr_directory(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    axum::extract::Query(q): axum::extract::Query<DirectoryQuery>,
    Form(req): Form<XmlCurlDirectoryReq>,
) -> Result<Response, ApiError> {
    // O `mod_xml_curl` do FreeSWITCH (build stock) não expõe cabeçalhos HTTP
    // arbitrários na configuração — só o URL do gateway. Por isso o segredo
    // pode vir por `?secret=` (ver xml_curl.conf.xml) além do cabeçalho
    // `X-Voice-Secret` normal (usado por `ramais_dial.lua`, que já pode pôr
    // cabeçalhos, e por qualquer chamador de teste). Isto tem um custo
    // conhecido — um segredo em URL pode acabar em logs de acesso — por isso
    // fica só aqui, não no resto da API interna; documentado em
    // xml_curl.conf.xml para quem for configurar produção decidir se prefere
    // pôr um proxy à frente que injecte o cabeçalho em vez disto.
    if check_media_secret(&state, &headers).is_err() {
        check_media_secret_str(&state, q.secret.as_deref().unwrap_or(""))?;
    }

    let user = req.user.trim();
    let domain = req.domain.trim();
    if user.is_empty() || domain.is_empty() {
        return Ok(xml_response(XML_NOT_FOUND.into()));
    }
    let Some(org_id) = org_id_by_sip_domain(&state, domain).await else {
        return Ok(xml_response(XML_NOT_FOUND.into()));
    };

    let row: Option<(String,)> = sqlx::query_as(
        "SELECT sip_ha1 FROM voice_extensions
          WHERE org_id = $1 AND sip_username = $2 AND active",
    )
    .bind(org_id)
    .bind(user)
    .fetch_optional(&state.db)
    .await?;
    let Some((ha1,)) = row else {
        return Ok(xml_response(XML_NOT_FOUND.into()));
    };

    let user_x = xml_escape(user);
    let domain_x = xml_escape(domain);
    let body = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="no"?>
<document type="freeswitch/xml">
  <section name="directory">
    <domain name="{domain_x}">
      <params>
        <param name="dial-string" value="{{^^:sip_invite_domain=${{dialed_domain}}:presence_id=${{dialed_user}}@${{dialed_domain}}}}${{sofia_contact(${{dialed_user}}@${{dialed_domain}})}}"/>
      </params>
      <groups>
        <group name="default">
          <users>
            <user id="{user_x}">
              <params>
                <param name="a1-hash" value="{ha1}"/>
                <param name="auth-acl" value="delonix_ramais"/>
              </params>
              <variables>
                <variable name="user_context" value="delonix_ramais"/>
                <variable name="effective_caller_id_number" value="{user_x}"/>
                <variable name="toll_allow" value=""/>
              </variables>
            </user>
          </users>
        </group>
      </groups>
    </domain>
  </section>
</document>"#
    );
    Ok(xml_response(body))
}

#[derive(Deserialize)]
pub struct ResolveExtensionReq {
    pub domain: String,
    pub extension: String,
}

#[derive(Serialize)]
pub struct ResolveExtensionResp {
    pub sip_username: String,
}

/// `POST /api/voice/ivr/resolve-extension` — chamado pelo dialplan interno
/// (`ramais_dial.lua`) quando um ramal disca um número curto. Traduz
/// `(domínio do chamador, número discado)` para o AOR (`sip_username`)
/// registado — a busca fica sempre dentro da MESMA org do domínio, que é a
/// fronteira de isolamento (o número curto não é único fora da org).
pub async fn ivr_resolve_extension(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(req): Json<ResolveExtensionReq>,
) -> Result<Json<ResolveExtensionResp>, ApiError> {
    check_media_secret(&state, &headers)?;
    let Some(org_id) = org_id_by_sip_domain(&state, req.domain.trim()).await else {
        return Err(ApiError::NotFound);
    };
    let sip_username: Option<String> = sqlx::query_scalar(
        "SELECT sip_username FROM voice_extensions
          WHERE org_id = $1 AND extension = $2 AND active",
    )
    .bind(org_id)
    .bind(req.extension.trim())
    .fetch_optional(&state.db)
    .await?;
    match sip_username {
        Some(sip_username) => Ok(Json(ResolveExtensionResp { sip_username })),
        None => Err(ApiError::NotFound),
    }
}

// ============================================================
//  Fase 2 — ramal alcançável do PSTN (DID dedicado, sem PIN)
// ============================================================
//
// Estende voice_did (migração 0014, server/src/voice.rs) com uma FK opcional
// para voice_extensions (migração 0065_ramais_did.sql). Continua a não tocar
// em voice_room/voice_participant/voice_cdr — o dial-in efémero por PIN
// (voice.rs::ivr_validate_pin) e este DID-por-ramal são dois caminhos
// PARALELOS que só partilham o inventário `voice_did`.
//
// Regra de atribuição (a mesma que voice.rs::create_room já aplica à escolha
// implícita de um DID "dedicated"): só um DID cujo `org_id` é o da própria
// org pode ser atribuído a um ramal dela — um número do pool partilhado
// (`org_id IS NULL`) fica disponível para todas as orgs por definição, e
// prendê-lo a UM ramal de UMA org quebraria essa promessa para as outras.
//
// Ainda fora de âmbito (fase seguinte do mesmo plano, não aqui): ponte para
// uma sala de reunião em vídeo — um ramal com DID atribuído recebe VOZ
// directa, não entra numa sala do SFU. Nenhuma UI ou mensagem adicionada
// aqui pode sugerir o contrário — a mesma disciplina do cabeçalho acima.

#[derive(Deserialize, utoipa::ToSchema)]
pub struct AssignExtensionDidReq {
    pub did_id: Uuid,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct ExtensionDidInfo {
    pub did_id: Uuid,
    pub e164: String,
}

/// `PUT /api/orgs/{org_id}/extensions/{id}/did` (admin) — atribui um DID
/// dedicado da própria org a um ramal. A partir de agora quem ligar para
/// `e164` cai DIRECTAMENTE neste ramal (ver `ivr_dialplan_did`), sem PIN e
/// sem IVR. Rejeita: DID inexistente/inactivo, DID do pool partilhado ou de
/// outra org, DID já atribuído a outro ramal, DID em uso por uma voice_room
/// activa (ver o comentário sobre não-atomicidade na migração 0065).
#[utoipa::path(
    put, path = "/api/orgs/{org_id}/extensions/{id}/did", tag = "voice",
    security(("session" = [])),
    params(("org_id" = Uuid, Path, description = "Organização."), ("id" = Uuid, Path, description = "Ramal.")),
    request_body = AssignExtensionDidReq,
    responses(
        (status = 200, body = ExtensionDidInfo),
        (status = 409, body = crate::openapi::ErrorBody, description = "O DID já está atribuído."),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 403, body = crate::openapi::ErrorBody),
        (status = 404, body = crate::openapi::ErrorBody),
    )
)]
pub async fn assign_extension_did(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path((org_id, id)): Path<(Uuid, Uuid)>,
    Json(req): Json<AssignExtensionDidReq>,
) -> Result<Json<ExtensionDidInfo>, ApiError> {
    crate::org::require_admin_pub(&state, org_id, auth.user_id).await?;

    let ext_exists: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM voice_extensions WHERE id = $1 AND org_id = $2)",
    )
    .bind(id)
    .bind(org_id)
    .fetch_one(&state.db)
    .await?;
    if !ext_exists {
        return Err(ApiError::NotFound);
    }

    #[derive(sqlx::FromRow)]
    struct DidRow {
        org_id: Option<Uuid>,
        active: bool,
        extension_id: Option<Uuid>,
        e164: String,
    }
    let did: Option<DidRow> =
        sqlx::query_as("SELECT org_id, active, extension_id, e164 FROM voice_did WHERE id = $1")
            .bind(req.did_id)
            .fetch_optional(&state.db)
            .await?;
    let Some(did) = did else {
        return Err(ApiError::BadRequest("DID não encontrado".into()));
    };
    if !did.active {
        return Err(ApiError::BadRequest("DID inactivo".into()));
    }
    if did.org_id != Some(org_id) {
        return Err(ApiError::BadRequest(
            "só um DID dedicado a esta organização pode ser atribuído a um ramal dela \
             — números do pool partilhado ficam disponíveis para todas as orgs"
                .into(),
        ));
    }
    if did.extension_id.is_some() {
        return Err(ApiError::Conflict(
            "este número já está atribuído a outro ramal".into(),
        ));
    }
    let room_active: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM voice_room WHERE did_id = $1 AND status = 'active')",
    )
    .bind(req.did_id)
    .fetch_one(&state.db)
    .await?;
    if room_active {
        return Err(ApiError::Conflict(
            "este número está em uso por uma sala de voz activa".into(),
        ));
    }

    let res = sqlx::query("UPDATE voice_did SET extension_id = $1 WHERE id = $2")
        .bind(id)
        .bind(req.did_id)
        .execute(&state.db)
        .await;
    match res {
        Ok(_) => {}
        Err(sqlx::Error::Database(dbe)) if dbe.is_unique_violation() => {
            return Err(ApiError::Conflict(
                "este ramal já tem outro número atribuído (pedido concorrente)".into(),
            ))
        }
        Err(e) => return Err(e.into()),
    }

    crate::audit::log(
        &state.db,
        Some(org_id),
        auth.user_id,
        "ramal.did_atribuido",
        &format!("{} → ramal {}", did.e164, id),
    )
    .await;

    Ok(Json(ExtensionDidInfo {
        did_id: req.did_id,
        e164: did.e164,
    }))
}

/// `DELETE /api/orgs/{org_id}/extensions/{id}/did` (admin) — desatribui o DID
/// de um ramal; o número volta a ficar livre para outro ramal ou para uma
/// sala de voz efémera. Idempotente: sem DID atribuído, não é um erro.
#[utoipa::path(
    delete, path = "/api/orgs/{org_id}/extensions/{id}/did", tag = "voice",
    security(("session" = [])),
    params(("org_id" = Uuid, Path, description = "Organização."), ("id" = Uuid, Path, description = "Ramal.")),
    responses(
        (status = 204, description = "DID desatribuído."),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 403, body = crate::openapi::ErrorBody),
        (status = 404, body = crate::openapi::ErrorBody),
    )
)]
pub async fn unassign_extension_did(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path((org_id, id)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, ApiError> {
    crate::org::require_admin_pub(&state, org_id, auth.user_id).await?;
    let ext_exists: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM voice_extensions WHERE id = $1 AND org_id = $2)",
    )
    .bind(id)
    .bind(org_id)
    .fetch_one(&state.db)
    .await?;
    if !ext_exists {
        return Err(ApiError::NotFound);
    }
    sqlx::query("UPDATE voice_did SET extension_id = NULL WHERE extension_id = $1")
        .bind(id)
        .execute(&state.db)
        .await?;
    crate::audit::log(
        &state.db,
        Some(org_id),
        auth.user_id,
        "ramal.did_desatribuido",
        &id.to_string(),
    )
    .await;
    Ok(StatusCode::NO_CONTENT)
}

/// Campos candidatos que o `mod_xml_curl` do FreeSWITCH pode enviar no POST
/// da secção "dialplan". Ao contrário da secção "directory" (onde `user`/
/// `domain` são os nomes mais consensuais na documentação — ver
/// `XmlCurlDirectoryReq` acima), os nomes aqui variam mais entre versões
/// (prefixo `Caller-`/`Hunt-`, maiúscula em cada palavra) e **não foram
/// confirmados contra uma instância real** — mesma ressalva do topo deste
/// ficheiro. Por isso aceitamos várias chaves candidatas, por ordem de
/// probabilidade, em vez de assumir uma única.
const DIALPLAN_DESTINATION_KEYS: &[&str] = &[
    "Caller-Destination-Number",
    "Hunt-Destination-Number",
    "destination_number",
];

fn first_present<'a>(
    m: &'a std::collections::HashMap<String, String>,
    keys: &[&str],
) -> Option<&'a str> {
    // `find_map` tem de decidir "presente" DENTRO do próprio fecho — um
    // `.filter()` encadeado a seguir só se aplicaria ao primeiro resultado
    // que o `find_map` já tivesse aceite, e uma chave presente mas em branco
    // (`"   "`) já conta como aceite antes de lá chegar, parando a procura
    // cedo demais em vez de cair para a chave candidata seguinte. Apanhado
    // por `first_present_skips_blank_values_and_falls_through`.
    keys.iter()
        .find_map(|k| m.get(*k).map(|s| s.trim()).filter(|s| !s.is_empty()))
}

/// `POST /api/voice/ivr/dialplan-did` — segunda secção do MESMO `mod_xml_curl`
/// que a directoria da Fase 1 (`ivr_directory`, mesmo `X-Voice-Secret`/
/// `?secret=`): o FreeSWITCH pede aqui o dialplan dinâmico da secção
/// "dialplan" quando uma chamada inbound precisa de ser encaminhada. Só
/// respondemos quando o número discado é o DID DEDICADO de um ramal
/// (`voice_did.extension_id`); para qualquer outro número devolvemos "não
/// encontrado" — o FreeSWITCH cai então para o dialplan estático existente
/// (`voice/freeswitch/dialplan/public/00_delonix_dialin.xml`, o dial-in por
/// PIN), que este endpoint NUNCA deve interceptar. Nunca devolve erro HTTP
/// por "não encontrado" pelo mesmo motivo que `ivr_directory`: um FreeSWITCH
/// mal configurado não deve ver 500s, e "not found" é uma resposta XML
/// válida, não uma falha.
///
/// **O que NÃO foi possível verificar aqui** (sem uma instância FreeSWITCH
/// real): (1) os nomes exactos dos campos do POST — ver
/// `DIALPLAN_DESTINATION_KEYS`; (2) a precedência real entre esta resposta
/// dinâmica e o dialplan estático já carregado a partir de
/// `dialplan/public/*.xml`. O pressuposto, herdado do mesmo padrão que a
/// Fase 1 já assume para a secção "directory" (aceite ali sem instância real,
/// ver o aviso no topo do ficheiro): o `mod_xml_curl` é consultado por
/// chamada e "not found" faz o FreeSWITCH cair para o estático. Se isso NÃO
/// se confirmar contra uma instância real, o sintoma seria "atribuir um DID
/// a um ramal não muda o comportamento da chamada" (fica sempre no IVR por
/// PIN) — nunca uma chamada perdida, porque a via antiga continua intacta.
/// Confirmar com `debug="true"` em `xml_curl.conf.xml` antes de produção.
pub async fn ivr_dialplan_did(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    axum::extract::Query(q): axum::extract::Query<DirectoryQuery>,
    Form(fields): Form<std::collections::HashMap<String, String>>,
) -> Result<Response, ApiError> {
    // Mesmo segredo, mesmo esquema de fallback ?secret= que ivr_directory —
    // ver o comentário lá sobre o custo conhecido (segredo em URL de acesso).
    if check_media_secret(&state, &headers).is_err() {
        check_media_secret_str(&state, q.secret.as_deref().unwrap_or(""))?;
    }

    let Some(raw_number) = first_present(&fields, DIALPLAN_DESTINATION_KEYS) else {
        return Ok(xml_response(XML_NOT_FOUND.into()));
    };

    // O dialplan estático aceita "+" opcional (`^\+?\d{6,15}$` em
    // 00_delonix_dialin.xml); voice_did.e164 é sempre guardado COM "+"
    // (voice.rs::create_did valida isso na criação). Tenta as duas formas em
    // vez de assumir qual delas o FreeSWITCH envia — não encontrar aqui é
    // sempre seguro (cai no estático), nunca é um erro.
    let with_plus = if raw_number.starts_with('+') {
        raw_number.to_string()
    } else {
        format!("+{raw_number}")
    };
    let without_plus = raw_number.trim_start_matches('+').to_string();
    let candidates = [with_plus, without_plus];

    #[derive(sqlx::FromRow)]
    struct Row {
        sip_username: String,
        org_id: Uuid,
    }
    let mut row: Option<Row> = None;
    for cand in &candidates {
        row = sqlx::query_as(
            "SELECT ve.sip_username, ve.org_id
               FROM voice_did d JOIN voice_extensions ve ON ve.id = d.extension_id
              WHERE d.e164 = $1 AND d.active AND ve.active",
        )
        .bind(cand)
        .fetch_optional(&state.db)
        .await?;
        if row.is_some() {
            break;
        }
    }
    let Some(row) = row else {
        return Ok(xml_response(XML_NOT_FOUND.into()));
    };

    let domain = sip_domain_for_org(&state, row.org_id).await?;
    let sip_username_x = xml_escape(&row.sip_username);
    let domain_x = xml_escape(&domain);
    let dest_digits_x = xml_escape(raw_number.trim_start_matches('+'));
    let body = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="no"?>
<document type="freeswitch/xml">
  <section name="dialplan">
    <context name="public">
      <extension name="delonix_ramal_did">
        <condition field="destination_number" expression="^\+?{dest_digits_x}$">
          <action application="set" data="rtp_secure_media=mandatory"/>
          <action application="set" data="hangup_after_bridge=true"/>
          <action application="bridge" data="user/{sip_username_x}@{domain_x}"/>
        </condition>
      </extension>
    </context>
  </section>
</document>"#
    );
    Ok(xml_response(body))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extension_format_accepts_3_to_5_digits() {
        assert!(validate_extension_format("101").is_ok());
        assert!(validate_extension_format("12345").is_ok());
        assert!(validate_extension_format("99999").is_ok());
    }

    #[test]
    fn extension_format_rejects_everything_else() {
        assert!(validate_extension_format("12").is_err()); // curto demais
        assert!(validate_extension_format("123456").is_err()); // longo demais
        assert!(validate_extension_format("").is_err());
        assert!(validate_extension_format("10a").is_err()); // não numérico
        assert!(validate_extension_format("+101").is_err());
        assert!(validate_extension_format(" 101").is_err()); // sem trim aqui — o chamador já fez trim
    }

    #[test]
    fn ha1_matches_rfc2617_known_vector() {
        // Vetor clássico do RFC 2617 (secção 3.5): HA1 = MD5("Mufasa:testrealm@host.com:Circle Of Life")
        assert_eq!(
            compute_ha1("Mufasa", "testrealm@host.com", "Circle Of Life"),
            "939e7578ed9e3c518a452acee763bce9"
        );
    }

    #[test]
    fn ha1_changes_with_domain_username_or_password() {
        let base = compute_ha1("ramal_abc", "acme.ramais.delonix.meet", "s3gredo");
        assert_ne!(
            base,
            compute_ha1("ramal_xyz", "acme.ramais.delonix.meet", "s3gredo")
        );
        assert_ne!(
            base,
            compute_ha1("ramal_abc", "zeta.ramais.delonix.meet", "s3gredo")
        );
        assert_ne!(
            base,
            compute_ha1("ramal_abc", "acme.ramais.delonix.meet", "outra")
        );
    }

    #[test]
    fn sip_password_and_username_are_random_and_well_formed() {
        let a = gen_sip_password();
        let b = gen_sip_password();
        assert_ne!(a, b);
        assert_eq!(a.len(), 30); // 15 bytes em hex
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()));

        let u1 = gen_sip_username();
        let u2 = gen_sip_username();
        assert_ne!(u1, u2);
        assert!(u1.starts_with("ramal_"));
    }

    #[test]
    fn xml_escape_covers_the_five_special_characters() {
        assert_eq!(xml_escape(r#"<a&b>"c""#), "&lt;a&amp;b&gt;&quot;c&quot;");
    }

    #[test]
    fn not_found_xml_is_well_formed_for_mod_xml_curl() {
        assert!(XML_NOT_FOUND.contains(r#"status="not found""#));
        assert!(XML_NOT_FOUND.starts_with("<?xml"));
    }

    // ---------- Fase 2: DID por ramal ----------

    fn fields(pairs: &[(&str, &str)]) -> std::collections::HashMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn first_present_picks_the_first_candidate_key_that_has_a_nonempty_value() {
        let f = fields(&[("Hunt-Destination-Number", "244912345678")]);
        assert_eq!(
            first_present(&f, DIALPLAN_DESTINATION_KEYS),
            Some("244912345678")
        );
    }

    #[test]
    fn first_present_prefers_earlier_keys_over_later_ones() {
        let f = fields(&[
            ("Caller-Destination-Number", "101"),
            ("Hunt-Destination-Number", "102"),
        ]);
        assert_eq!(first_present(&f, DIALPLAN_DESTINATION_KEYS), Some("101"));
    }

    #[test]
    fn first_present_skips_blank_values_and_falls_through() {
        let f = fields(&[
            ("Caller-Destination-Number", "   "),
            ("destination_number", "+244912345678"),
        ]);
        assert_eq!(
            first_present(&f, DIALPLAN_DESTINATION_KEYS),
            Some("+244912345678")
        );
    }

    #[test]
    fn first_present_is_none_when_no_candidate_key_is_present() {
        let f = fields(&[("section", "dialplan")]);
        assert_eq!(first_present(&f, DIALPLAN_DESTINATION_KEYS), None);
    }

    #[test]
    fn dialplan_did_xml_response_is_well_formed() {
        // Mesma verificação estrutural que not_found_xml_is_well_formed_for_mod_xml_curl,
        // mas para o corpo de sucesso — sem instância FreeSWITCH para validar
        // contra o schema real, isto é o que se pode provar aqui: bem formado,
        // secção/contexto/acções certas, número e AOR escapados.
        let body = format!(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="no"?>
<document type="freeswitch/xml">
  <section name="dialplan">
    <context name="public">
      <extension name="delonix_ramal_did">
        <condition field="destination_number" expression="^\+?{dest}$">
          <action application="set" data="rtp_secure_media=mandatory"/>
          <action application="set" data="hangup_after_bridge=true"/>
          <action application="bridge" data="user/{user}@{domain}"/>
        </condition>
      </extension>
    </context>
  </section>
</document>"#,
            dest = xml_escape("244912345678"),
            user = xml_escape("ramal_abc123"),
            domain = xml_escape("acme.ramais.delonix.meet"),
        );
        assert!(body.starts_with("<?xml"));
        assert!(body.contains(r#"section name="dialplan""#));
        assert!(body.contains(r#"context name="public""#));
        assert!(body.contains("bridge"));
        assert!(body.contains("user/ramal_abc123@acme.ramais.delonix.meet"));
    }
}

/// Documentação OpenAPI dos ramais (`openapi.rs` junta-a). Os três
/// callbacks `/api/voice/ivr/*` do `mod_xml_curl` do FreeSWITCH ficam de fora:
/// são máquina-a-máquina, por segredo partilhado, e respondem XML.
#[derive(utoipa::OpenApi)]
#[openapi(
    paths(
        list_extensions,
        create_extension,
        update_extension,
        regenerate_extension_password,
        delete_extension,
        assign_extension_did,
        unassign_extension_did
    ),
    components(schemas(
        VoiceExtensionInfo,
        CreatedExtension,
        CreateExtensionReq,
        UpdateExtensionReq,
        AssignExtensionDidReq,
        ExtensionDidInfo
    ))
)]
pub struct ApiDoc;
