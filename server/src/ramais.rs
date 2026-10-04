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
//! (migração `0065_ramais_did.sql`, estende `voice_did`).
//!
//! Fase 3 (R273): um ramal entra numa reunião marcando o NÚMERO DE ACESSO ÀS
//! REUNIÕES — um número curto reservado (`VOICE_MEETING_ACCESS_NUMBER`, por
//! omissão `8000`), o mesmo para todas as organizações. Não há ponte nova: o
//! `resolve-extension` diz ao `ramais_dial.lua` que o número marcado é o de
//! acesso, o Lua entrega a chamada ao IVR do dial-in em modo `ramal`
//! (`dialin_ivr.lua`), e esse valida o PIN em
//! `/internal/v1/voice/ivr/validate-extension` (`voice::validate_pin_for_extension`),
//! que só encontra salas da ORGANIZAÇÃO do ramal autenticado. Daí em diante é a
//! ponte telefone↔sala do ADR-0010, com o mesmo recuo. O que este módulo
//! garante: o número reservado nunca é de um ramal (`ramais.extension_reserved`)
//! e as leituras dos ramais dizem qual é (`meeting_access_number`).
//!
//! Lote 1 do item 3.8 (R276): um ramal pode não ter pessoa (ramal da EMPRESA
//! — recepção, sala, portaria; `member_id` nulo, etiqueta obrigatória); os
//! números automáticos saem de um intervalo por organização
//! (`voice_extension_ranges`, por omissão 1000–1999) com a acção em massa
//! `assign-missing`; e cada ramal tem um PIN secreto, que vive em
//! `extension_pin.rs`.
//!
//! **Não provado:** nenhuma chamada real percorreu este caminho. A regra do
//! servidor está medida contra Postgres (`tests/ramal_entra_na_sala.rs`); o
//! Lua só tem a sintaxe verificada. O que continua a não existir: um DID de
//! ramal (Fase 2) a entrar numa sala — quem liga para esse número fala com a
//! pessoa do ramal — e o nome de quem entra por ramal no censo (aparece como
//! «Telefone», anónimo, como qualquer outro telefone).
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
//! pelo MESMO segredo partilhado que `voice.rs` já usa (`check_media_secret`:
//! o cabeçalho `X-Voice-Secret`, ou HTTP Basic com o segredo como password —
//! nunca no URL, R227):
//! - `POST /internal/v1/voice/ivr/directory` — o FreeSWITCH chama isto no REGISTER
//!   (via `mod_xml_curl`, secção "directory") para obter o `a1-hash` SIP
//!   Digest do ramal que se está a registar.
//! - `POST /internal/v1/voice/ivr/resolve-extension` — o dialplan interno
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
use std::{collections::HashSet, sync::Arc};
use uuid::Uuid;

use crate::{auth::AuthUser, error::ApiError, voice::check_media_secret, AppState};
use delonix_meet_core::DomainError;
use delonix_meet_domain::telephony::{extension as ext_rules, extension_pin as pin_rules};

// ---------- Helpers ----------

/// Número de ramal: 3 a 5 dígitos, nada mais. Curto o suficiente para se
/// discar de cor, longo o suficiente para uma org de algumas centenas de
/// pessoas não esgotar o espaço.
fn validate_extension_format(s: &str) -> Result<(), ApiError> {
    if !ext_rules::is_short_number(s) {
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

/// Uma password SIP nova: a password em claro e o seu Argon2.
/// É o que a regeneração pelo administrador e o resgate de um bilhete de
/// provisionamento (`extension_provisioning.rs`) gravam — a mesma regra nos
/// dois, para o directório do FreeSWITCH (que só lê `sip_ha1`) os aceitar.
pub(crate) struct SipSecret {
    /// Em claro: só existe até sair na resposta que a entrega.
    pub(crate) password: String,
    /// Argon2, para repouso. É a parte cara — gera-se UMA vez por ramal, fora
    /// de qualquer ciclo de tentativas e de qualquer transacção.
    pub(crate) hash: String,
}

impl SipSecret {
    pub(crate) fn generate() -> Result<Self, ApiError> {
        let password = gen_sip_password();
        let hash = crate::auth::hash_password(&password)?;
        Ok(Self { password, hash })
    }

    /// O HA1 depende do AOR e do domínio; é um MD5, barato.
    pub(crate) fn ha1(&self, sip_username: &str, sip_domain: &str) -> String {
        compute_ha1(sip_username, sip_domain, &self.password)
    }
}

/// O domínio SIP a partir do slug da organização.
pub(crate) fn sip_domain_of_slug(state: &AppState, slug: &str) -> String {
    format!("{slug}.{}", state.config.voice_ramais_domain_suffix)
}

/// AOR SIP: globalmente único (não escopado por org — é o directório do
/// registar que exige isto, ver o comentário no topo do ficheiro).
fn gen_sip_username() -> String {
    format!("ramal_{}", crate::crypto::random_hex(8))
}

fn ha1_aad(id: Uuid) -> String {
    crate::secrets_at_rest::aad("voice_extensions", "sip_ha1", id)
}

/// Cifra o HA1 de um ramal para a base. O HA1 (`MD5(user:realm:password)`) é
/// o que o digest SIP usa: quem o tiver regista-se como o ramal sem nunca ter
/// visto a password — em claro, uma fuga da tabela era uma fuga das
/// credenciais de todos os ramais. Sem chaves de cifra é `422`, como qualquer
/// segredo novo.
pub(crate) fn seal_ha1(state: &AppState, id: Uuid, ha1: &str) -> Result<String, ApiError> {
    crate::secrets_at_rest::seal(&state.config, ha1, &ha1_aad(id))
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
pub(crate) async fn sip_domain_for_org(state: &AppState, org_id: Uuid) -> Result<String, ApiError> {
    let slug: String = sqlx::query_scalar("SELECT slug FROM organizations WHERE id = $1")
        .bind(org_id)
        .fetch_one(&state.db)
        .await?;
    Ok(sip_domain_of_slug(state, &slug))
}

/// Caminho inverso: de um domínio SIP para o `org_id`. `None` se o sufixo não
/// bater ou a org não existir — o chamador trata isso como "não encontrado",
/// nunca como erro (um FreeSWITCH mal configurado não deve ver 500s).
pub(crate) async fn org_id_by_sip_domain(state: &AppState, domain: &str) -> Option<Uuid> {
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
    /// A pessoa dona do ramal. `null` num ramal da EMPRESA (recepção, sala,
    /// portaria), que se identifica pela etiqueta.
    pub member_id: Option<Uuid>,
    /// Nome do membro dono — junção com `users`, só para exibição na consola.
    /// `null` num ramal da empresa.
    pub member_username: Option<String>,
    pub member_email: Option<String>,
    pub extension: String,
    pub sip_username: String,
    pub label: String,
    pub active: bool,
    pub created_at: DateTime<Utc>,
    /// Estado do PIN: `unset` (por definir), `set` (definido) ou `locked`
    /// (bloqueado por falhas seguidas). O valor do PIN nunca sai numa leitura.
    #[schema(example = "unset")]
    pub pin_state: String,
    /// Número curto que este ramal marca para entrar numa reunião: o
    /// FreeSWITCH atende e pede o PIN da sala. É o mesmo para todos os ramais
    /// (configuração do servidor) e nunca é o número de um ramal.
    #[sqlx(default)]
    pub meeting_access_number: String,
    /// Endereço PÚBLICO do servidor SIP onde o softphone deste ramal se liga
    /// (`VOICE_RAMAIS_PUBLIC_HOST`/`_PORT`/`_TRANSPORT`). `null` quando a
    /// instalação não o configurou — o servidor não adivinha um valor. Não é o
    /// `sip_domain`: esse é o realm do digest, um nome lógico.
    #[sqlx(skip)]
    pub sip_server: Option<SipServerInfo>,
}

/// Servidor/proxy SIP público dos ramais. É o mesmo para todos os ramais da
/// instalação (configuração do servidor).
#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
pub struct SipServerInfo {
    /// Nome DNS ou IP público.
    #[schema(example = "meet.exemplo.ao")]
    pub host: String,
    #[schema(example = 5070)]
    pub port: u16,
    /// `udp`, `tcp` ou `tls`.
    #[schema(example = "udp")]
    pub transport: String,
    /// O proxy pronto a colar no softphone: `sip:host:porta;transport=x`.
    #[schema(example = "sip:meet.exemplo.ao:5070;transport=udp")]
    pub uri: String,
}

impl From<&ext_rules::SipServer> for SipServerInfo {
    fn from(s: &ext_rules::SipServer) -> Self {
        Self {
            host: s.host().to_string(),
            port: s.port(),
            transport: s.transport().as_str().to_string(),
            uri: s.proxy_uri(),
        }
    }
}

impl VoiceExtensionInfo {
    /// O número de acesso e o endereço público não são colunas: vêm da
    /// configuração, e todas as leituras de um ramal passam por aqui antes de
    /// saírem.
    fn with_server_config(mut self, state: &AppState) -> Self {
        self.meeting_access_number = state.config.voice_meeting_access_number.clone();
        self.sip_server = state.config.voice_ramais_public.as_ref().map(Into::into);
        self
    }
}

/// `LEFT JOIN`: um ramal da empresa não tem pessoa. O estado do PIN calcula-se
/// aqui para nenhuma leitura ter de tocar em `pin_hash`.
const SELECT_EXTENSION_INFO: &str =
    "SELECT e.id, e.org_id, e.member_id, u.username AS member_username,
            u.email AS member_email, e.extension, e.sip_username, e.label, e.active, e.created_at,
            CASE WHEN e.pin_hash IS NULL THEN 'unset'
                 WHEN e.pin_locked_until > now() THEN 'locked'
                 ELSE 'set' END AS pin_state
     FROM voice_extensions e LEFT JOIN users u ON u.id = e.member_id";

/// Um ramal da organização, pronto a sair numa resposta. `None` se não existe
/// NESTA organização.
pub(crate) async fn extension_info(
    state: &AppState,
    org_id: Uuid,
    id: Uuid,
) -> Result<Option<VoiceExtensionInfo>, ApiError> {
    let info: Option<VoiceExtensionInfo> = sqlx::query_as(&format!(
        "{SELECT_EXTENSION_INFO} WHERE e.id = $1 AND e.org_id = $2"
    ))
    .bind(id)
    .bind(org_id)
    .fetch_optional(&state.db)
    .await?;
    Ok(info.map(|i| i.with_server_config(state)))
}

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
    /// configuração da conta do softphone. É o realm do digest — um nome
    /// lógico, que pode não resolver em DNS; o endereço a que o softphone se
    /// liga é `sip_server`.
    pub sip_domain: String,
}

// ============================================================
//  Gestão (admin da org, sessão) — mesma permissão que os DIDs de voz.
// ============================================================

#[derive(Deserialize, utoipa::ToSchema)]
pub struct CreateExtensionReq {
    /// A pessoa dona do ramal. Ausente = ramal da EMPRESA (recepção, sala,
    /// portaria): a etiqueta passa a ser obrigatória e o ramal nasce sem PIN.
    #[serde(default)]
    pub member_id: Option<Uuid>,
    pub extension: String,
    #[serde(default)]
    pub label: Option<String>,
}

/// Cria um ramal (admin): de uma pessoa da org, ou — sem `member_id` — da
/// empresa, com etiqueta obrigatória. Gera credenciais SIP novas e devolve a
/// password em claro UMA VEZ. O PIN nasce «por definir».
#[utoipa::path(
    post, path = "/api/orgs/{org_id}/extensions", tag = "voice",
    security(("session" = [])),
    params(("org_id" = Uuid, Path, description = "Organização.")),
    request_body = CreateExtensionReq,
    responses(
        (status = 200, body = CreatedExtension, description = "A palavra-passe SIP sai UMA vez."),
        (status = 400, body = crate::openapi::ErrorBody, description = "Número fora da forma, pessoa de outra organização, ou ramal da empresa sem etiqueta (`ramais.label_required`)."),
        (status = 409, body = crate::openapi::ErrorBody, description = "O número de ramal já existe na organização, ou é o número de acesso às reuniões (`ramais.extension_reserved`)."),
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
    // O número de acesso às reuniões é do IVR da sala (R273): com um ramal
    // nele, quem o marcasse nunca chegava a essa pessoa.
    let reserved = &state.config.voice_meeting_access_number;
    if ext_rules::is_meeting_access_number(extension, reserved) {
        return Err(DomainError::conflict(
            "ramais.extension_reserved",
            format!("o número {reserved} está reservado para entrar em reuniões"),
        )
        .into());
    }

    // Erro claro em vez de deixar a FK composta rebentar com algo opaco. A
    // pertença decide-se SEMPRE em org.rs (regra 1, ADR-0004 §5) — não se
    // escreve a consulta de pertença à mão aqui.
    if let Some(member_id) = req.member_id {
        if crate::org::role_in_org(&state, org_id, member_id)
            .await?
            .is_none()
        {
            return Err(ApiError::BadRequest(
                "o membro não pertence a esta organização".into(),
            ));
        }
    }

    let label: String = req
        .label
        .unwrap_or_default()
        .trim()
        .chars()
        .take(80)
        .collect();
    // Um ramal sem pessoa só se reconhece pela etiqueta.
    if req.member_id.is_none() && label.is_empty() {
        return Err(DomainError::invalid(
            "ramais.label_required",
            "um ramal da empresa precisa de uma etiqueta (recepção, sala, portaria)",
        )
        .into());
    }
    let sip_domain = sip_domain_for_org(&state, org_id).await?;
    let secret = SipSecret::generate()?;
    let id = match insert_extension(
        &state,
        org_id,
        &sip_domain,
        req.member_id,
        extension,
        &label,
        &secret,
    )
    .await?
    {
        Inserted::Created { id } => id,
        Inserted::NumberTaken => {
            return Err(ApiError::Conflict(
                "já existe um ramal com esse número nesta organização".into(),
            ))
        }
        Inserted::MemberHasOne => {
            return Err(ApiError::Conflict(
                "este membro já tem um ramal atribuído".into(),
            ))
        }
    };

    let info = extension_info(&state, org_id, id)
        .await?
        .ok_or(ApiError::NotFound)?;

    crate::audit::log(
        &state.db,
        Some(org_id),
        auth.user_id,
        "ramal.criado",
        &format!("{} ({})", info.extension, info.sip_username),
    )
    .await;

    Ok(Json(CreatedExtension {
        sip_password: secret.password,
        sip_domain,
        extension: info,
    }))
}

/// O que aconteceu ao tentar gravar um ramal novo.
enum Inserted {
    /// Gravado, com o segredo que quem chamou trouxe.
    Created { id: Uuid },
    /// O número já é de outro ramal desta organização.
    NumberTaken,
    /// A pessoa já tem ramal.
    MemberHasOne,
}

/// Grava um ramal com credenciais SIP novas. É o ÚNICO `INSERT` em
/// `voice_extensions`: a criação pelo admin e a atribuição em massa passam as
/// duas por aqui. Quem chama já validou a forma do número, o número reservado
/// e a pertença da pessoa — e traz o segredo SIP já gerado: quem tenta vários
/// números para o mesmo ramal (um ocupado, tenta o seguinte) paga UM Argon2,
/// não um por tentativa.
async fn insert_extension(
    state: &AppState,
    org_id: Uuid,
    sip_domain: &str,
    member_id: Option<Uuid>,
    extension: &str,
    label: &str,
    secret: &SipSecret,
) -> Result<Inserted, ApiError> {
    // Retenta só em colisão do AOR globalmente único (extremamente
    // improvável com 64 bits) — colisão de extensão/membro é definitiva,
    // não um acidente de geração aleatória, e não se retenta.
    for _ in 0..5 {
        let sip_username = gen_sip_username();
        // O id nasce aqui e não no `DEFAULT` da tabela: é ele que amarra o HA1
        // cifrado a ESTA linha (aad `voice_extensions.sip_ha1:<id>`).
        let id = Uuid::new_v4();
        let ha1 = seal_ha1(state, id, &secret.ha1(&sip_username, sip_domain))?;
        let res: Result<(Uuid,), sqlx::Error> = sqlx::query_as(
            "INSERT INTO voice_extensions
                 (id, org_id, member_id, extension, sip_username, sip_password_hash, sip_ha1, label)
             VALUES ($8, $1, $2, $3, $4, $5, $6, $7)
             RETURNING id",
        )
        .bind(org_id)
        .bind(member_id)
        .bind(extension)
        .bind(&sip_username)
        .bind(&secret.hash)
        .bind(&ha1)
        .bind(label)
        .bind(id)
        .fetch_one(&state.db)
        .await;
        match res {
            Ok((id,)) => return Ok(Inserted::Created { id }),
            Err(sqlx::Error::Database(dbe)) if dbe.is_unique_violation() => {
                match dbe.constraint() {
                    Some("voice_extensions_sip_username_uidx") => continue, // retenta com outro AOR
                    Some("voice_extensions_org_ext_uidx") => return Ok(Inserted::NumberTaken),
                    Some("voice_extensions_org_member_uidx") => return Ok(Inserted::MemberHasOne),
                    _ => return Err(ApiError::Conflict("ramal em conflito".into())),
                }
            }
            Err(e) => return Err(e.into()),
        }
    }
    Err(ApiError::internal("não foi possível gerar o AOR SIP"))
}

/// Lista os ramais da org (admin). Nunca devolve hash nem HA1 — nem o PIN: só
/// o estado dele (`pin_state`).
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
    Ok(Json(
        rows.into_iter()
            .map(|r| r.with_server_config(&state))
            .collect(),
    ))
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
        (status = 400, body = crate::openapi::ErrorBody, description = "Um ramal da empresa não pode ficar sem etiqueta (`ramais.label_required`)."),
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
    if label.as_deref() == Some("") {
        let company: Option<bool> = sqlx::query_scalar(
            "SELECT member_id IS NULL FROM voice_extensions WHERE id = $1 AND org_id = $2",
        )
        .bind(id)
        .bind(org_id)
        .fetch_optional(&state.db)
        .await?;
        if company == Some(true) {
            return Err(DomainError::invalid(
                "ramais.label_required",
                "um ramal da empresa precisa de uma etiqueta (recepção, sala, portaria)",
            )
            .into());
        }
    }
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
    let info = extension_info(&state, org_id, id)
        .await?
        .ok_or(ApiError::NotFound)?;
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

    let secret = SipSecret::generate()?;
    let ha1 = seal_ha1(&state, id, &secret.ha1(&sip_username, &sip_domain))?;
    sqlx::query(
        "UPDATE voice_extensions SET sip_password_hash = $3, sip_ha1 = $4
          WHERE id = $1 AND org_id = $2",
    )
    .bind(id)
    .bind(org_id)
    .bind(&secret.hash)
    .bind(&ha1)
    .execute(&state.db)
    .await?;
    // Um QR do Linphone ainda por ler trocaria esta password outra vez (R278).
    crate::extension_provisioning::revoke_tickets(&state.db, org_id, id).await?;

    let info = extension_info(&state, org_id, id)
        .await?
        .ok_or(ApiError::NotFound)?;
    crate::audit::log(
        &state.db,
        Some(org_id),
        auth.user_id,
        "ramal.password_regenerada",
        &info.extension,
    )
    .await;
    Ok(Json(CreatedExtension {
        sip_password: secret.password,
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
//  Numeração automática — o intervalo da org e a atribuição em massa (R276)
// ============================================================

/// O intervalo de onde saem os números automáticos de uma organização.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, utoipa::ToSchema)]
pub struct ExtensionRange {
    /// Primeiro número do intervalo (3 a 5 dígitos, sem zero à esquerda).
    #[schema(example = 1000)]
    pub range_start: u32,
    /// Último número do intervalo, inclusive.
    #[schema(example = 1999)]
    pub range_end: u32,
    /// Atribuir um ramal automaticamente a quem entra na organização (R278).
    /// Desligado por omissão.
    #[serde(default)]
    pub auto_assign_on_join: bool,
}

/// O que o `PUT` do intervalo aceita.
#[derive(Debug, Clone, Copy, Deserialize, utoipa::ToSchema)]
pub struct PutExtensionRangeReq {
    #[schema(example = 1000)]
    pub range_start: u32,
    #[schema(example = 1999)]
    pub range_end: u32,
    /// Ausente = MANTER o que está gravado. Um cliente que só conhece o
    /// intervalo não desliga a atribuição automática sem querer.
    #[serde(default)]
    pub auto_assign_on_join: Option<bool>,
}

/// O intervalo gravado, ou a omissão (1000–1999) se a org nunca escolheu um.
async fn range_for_org(state: &AppState, org_id: Uuid) -> Result<ExtensionRange, ApiError> {
    let row: Option<(i32, i32, bool)> = sqlx::query_as(
        "SELECT range_start, range_end, auto_assign_on_join
           FROM voice_extension_ranges WHERE org_id = $1",
    )
    .bind(org_id)
    .fetch_optional(&state.db)
    .await?;
    Ok(match row {
        Some((s, e, auto)) => ExtensionRange {
            range_start: u32::try_from(s).unwrap_or(pin_rules::DEFAULT_RANGE_START),
            range_end: u32::try_from(e).unwrap_or(pin_rules::DEFAULT_RANGE_END),
            auto_assign_on_join: auto,
        },
        None => ExtensionRange {
            range_start: pin_rules::DEFAULT_RANGE_START,
            range_end: pin_rules::DEFAULT_RANGE_END,
            auto_assign_on_join: false,
        },
    })
}

/// O intervalo de numeração automática da org (admin).
#[utoipa::path(
    get, path = "/api/orgs/{org_id}/extension-range", tag = "voice",
    security(("session" = [])),
    params(("org_id" = Uuid, Path, description = "Organização.")),
    responses(
        (status = 200, body = ExtensionRange, description = "O intervalo gravado, ou 1000–1999 e a atribuição automática desligada se a organização nunca escolheu."),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 403, body = crate::openapi::ErrorBody),
        (status = 404, body = crate::openapi::ErrorBody),
    )
)]
pub async fn get_extension_range(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(org_id): Path<Uuid>,
) -> Result<Json<ExtensionRange>, ApiError> {
    crate::org::require_admin_pub(&state, org_id, auth.user_id).await?;
    range_for_org(&state, org_id).await.map(Json)
}

/// Define o intervalo de numeração automática da org (admin) e se quem entra
/// recebe um ramal sozinho (`auto_assign_on_join`; ausente = manter). Não renumera nem apaga os
/// ramais que já existem fora dele: só decide de onde saem os próximos números
/// automáticos. Ligar a atribuição automática não dá ramal a quem já cá está —
/// isso é `assign-missing`.
#[utoipa::path(
    put, path = "/api/orgs/{org_id}/extension-range", tag = "voice",
    security(("session" = [])),
    params(("org_id" = Uuid, Path, description = "Organização.")),
    request_body = PutExtensionRangeReq,
    responses(
        (status = 200, body = ExtensionRange),
        (status = 400, body = crate::openapi::ErrorBody, description = "Intervalo fora de 100–99999 ou invertido (`ramais.range_invalid`)."),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 403, body = crate::openapi::ErrorBody),
        (status = 404, body = crate::openapi::ErrorBody),
    )
)]
pub async fn put_extension_range(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(org_id): Path<Uuid>,
    Json(req): Json<PutExtensionRangeReq>,
) -> Result<Json<ExtensionRange>, ApiError> {
    crate::org::require_admin_pub(&state, org_id, auth.user_id).await?;
    if !pin_rules::is_valid_range(req.range_start, req.range_end) {
        return Err(DomainError::invalid(
            "ramais.range_invalid",
            format!(
                "o intervalo tem de estar entre {} e {}, com o início antes do fim",
                pin_rules::RANGE_MIN,
                pin_rules::RANGE_MAX
            ),
        )
        .into());
    }
    // `is_valid_range` garante que os dois cabem num i32.
    // `$4` nulo (campo ausente) mantém o que está gravado; numa linha nova é FALSE.
    let auto: bool = sqlx::query_scalar(
        "INSERT INTO voice_extension_ranges (org_id, range_start, range_end, auto_assign_on_join)
         VALUES ($1, $2, $3, COALESCE($4, FALSE))
         ON CONFLICT (org_id) DO UPDATE
            SET range_start = EXCLUDED.range_start, range_end = EXCLUDED.range_end,
                auto_assign_on_join = COALESCE($4, voice_extension_ranges.auto_assign_on_join),
                updated_at = now()
         RETURNING auto_assign_on_join",
    )
    .bind(org_id)
    .bind(req.range_start as i32)
    .bind(req.range_end as i32)
    .bind(req.auto_assign_on_join)
    .fetch_one(&state.db)
    .await?;
    let req = ExtensionRange {
        range_start: req.range_start,
        range_end: req.range_end,
        auto_assign_on_join: auto,
    };
    crate::audit::log(
        &state.db,
        Some(org_id),
        auth.user_id,
        "ramal.intervalo_alterado",
        &format!(
            "{}–{}; ramal automático ao entrar: {}",
            req.range_start,
            req.range_end,
            if req.auto_assign_on_join {
                "ligado"
            } else {
                "desligado"
            }
        ),
    )
    .await;
    Ok(Json(req))
}

/// O que a atribuição automática fez a quem entrou.
#[derive(Debug, PartialEq, Eq)]
enum JoinAssignment {
    /// A organização não a tem ligada, a pessoa não ocupa lugar (convidado
    /// externo, utilizador de serviço, arquivado) ou já tinha ramal.
    Skipped,
    Assigned(String),
    RangeExhausted,
}

/// Dá um ramal a quem ACABOU de entrar na organização, se ela tiver a
/// atribuição automática ligada (R278). É o ÚNICO ponto: cada caminho que cria
/// uma pertença chama isto DEPOIS do seu commit, e mais nada.
///
/// **Nunca faz falhar a entrada.** Não devolve erro: o intervalo esgotado fica
/// na auditoria (`ramal.atribuicao_automatica_falhou`) e uma avaria fica no
/// registo — a pessoa entra sem ramal e o administrador resolve com
/// «Atribuir ramais a todos».
///
/// Quem recebe: quem ocupa lugar (`org::seat_holder_username`) — activo,
/// humano e não convidado externo. Idempotente: quem já tem ramal não é tocado.
pub(crate) async fn assign_on_join(state: &AppState, org_id: Uuid, user_id: Uuid) {
    // Actor de sistema: ninguém pediu este ramal, foi a regra da organização.
    let actor = Uuid::nil();
    match try_assign_on_join(state, org_id, user_id).await {
        Ok(JoinAssignment::Skipped) => {}
        Ok(JoinAssignment::Assigned(number)) => {
            crate::audit::log(
                &state.db,
                Some(org_id),
                actor,
                "ramal.atribuido_ao_entrar",
                &format!("{number} → {user_id}"),
            )
            .await;
        }
        Ok(JoinAssignment::RangeExhausted) => {
            tracing::warn!(%org_id, %user_id, "ramal automático: o intervalo esgotou-se — o membro entrou sem ramal");
            crate::audit::log(
                &state.db,
                Some(org_id),
                actor,
                "ramal.atribuicao_automatica_falhou",
                &format!("intervalo esgotado → {user_id}"),
            )
            .await;
        }
        Err(e) => {
            tracing::error!(%org_id, %user_id, error = %e, "ramal automático: falhou — o membro entrou sem ramal");
        }
    }
}

async fn try_assign_on_join(
    state: &AppState,
    org_id: Uuid,
    user_id: Uuid,
) -> Result<JoinAssignment, ApiError> {
    let range = range_for_org(state, org_id).await?;
    if !range.auto_assign_on_join {
        return Ok(JoinAssignment::Skipped);
    }
    // Pertença: decide-se em org.rs (regra 1, ADR-0004 §5).
    if crate::org::seat_holder_username(&state.db, org_id, user_id)
        .await?
        .is_none()
    {
        return Ok(JoinAssignment::Skipped);
    }
    let existing: Vec<(String, Option<Uuid>)> =
        sqlx::query_as("SELECT extension, member_id FROM voice_extensions WHERE org_id = $1")
            .bind(org_id)
            .fetch_all(&state.db)
            .await?;
    if existing.iter().any(|(_, m)| *m == Some(user_id)) {
        return Ok(JoinAssignment::Skipped);
    }
    let taken: HashSet<String> = existing.into_iter().map(|(e, _)| e).collect();
    let sip_domain = sip_domain_for_org(state, org_id).await?;
    let mut free = pin_rules::free_numbers(
        range.range_start,
        range.range_end,
        &taken,
        &state.config.voice_meeting_access_number,
    )
    .peekable();
    // Intervalo esgotado: nem se paga o Argon2.
    if free.peek().is_none() {
        return Ok(JoinAssignment::RangeExhausted);
    }
    // Um Argon2 por entrada, fora do ciclo — não um por número tentado.
    let secret = SipSecret::generate()?;
    for number in free {
        match insert_extension(
            state,
            org_id,
            &sip_domain,
            Some(user_id),
            &number,
            "",
            &secret,
        )
        .await?
        {
            Inserted::Created { .. } => return Ok(JoinAssignment::Assigned(number)),
            // Outra entrada ficou com o número entretanto: tenta o seguinte.
            Inserted::NumberTaken => continue,
            Inserted::MemberHasOne => return Ok(JoinAssignment::Skipped),
        }
    }
    Ok(JoinAssignment::RangeExhausted)
}

/// Quantos ramais uma chamada de `assign-missing` cria no máximo. Cada ramal
/// custa um Argon2 (a password SIP); sem tecto, uma organização grande
/// segurava o pedido — e um núcleo — durante dezenas de segundos. A acção é
/// idempotente: quem chama repete enquanto `remaining` for maior que zero.
const ASSIGN_BATCH: usize = 100;

/// Um ramal criado pela atribuição em massa. Sem password SIP: ninguém a viu
/// — o administrador regenera-a por ramal quando for configurar o aparelho.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct AssignedExtension {
    pub id: Uuid,
    pub member_id: Uuid,
    pub member_username: String,
    pub extension: String,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct AssignMissingResp {
    /// Ramais criados NESTA chamada.
    pub assigned: Vec<AssignedExtension>,
    /// Pessoas activas que já tinham ramal antes da chamada.
    pub already_assigned: usize,
    /// Pessoas activas que continuam sem ramal depois desta chamada, por
    /// tecto do lote ou por o intervalo se ter esgotado.
    pub remaining: usize,
    /// `true` se não há mais números livres no intervalo: alargar o intervalo
    /// é a única forma de `remaining` chegar a zero.
    pub range_exhausted: bool,
    pub range_start: u32,
    pub range_end: u32,
}

/// Dá um ramal a cada pessoa ACTIVA da org que ainda não tem (admin). É um
/// *custom method*: os números saem do intervalo da org por ordem crescente,
/// saltando os ocupados e o número de acesso às reuniões. Idempotente — quem
/// já tem ramal não é tocado, e repetir sem pessoas novas não cria nada.
///
/// As passwords SIP dos ramais criados aqui não saem na resposta (seriam
/// dezenas de segredos num só ecrã): regeneram-se por ramal. O PIN nasce «por
/// definir»; cada pessoa gera o seu na sua área.
#[utoipa::path(
    post, path = "/api/orgs/{org_id}/extensions/assign-missing", tag = "voice",
    security(("session" = [])),
    params(("org_id" = Uuid, Path, description = "Organização.")),
    responses(
        (status = 200, body = AssignMissingResp, description = "No máximo 100 ramais por chamada; repetir enquanto `remaining` > 0 e `range_exhausted` for falso."),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 403, body = crate::openapi::ErrorBody),
        (status = 404, body = crate::openapi::ErrorBody),
    )
)]
pub async fn assign_missing_extensions(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(org_id): Path<Uuid>,
) -> Result<Json<AssignMissingResp>, ApiError> {
    crate::org::require_admin_pub(&state, org_id, auth.user_id).await?;
    let range = range_for_org(&state, org_id).await?;

    // Pertença activa e humana: decide-se em org.rs (regra 1, ADR-0004 §5).
    let people = crate::org::active_member_subjects(&state.db, org_id, None).await?;
    let existing: Vec<(String, Option<Uuid>)> =
        sqlx::query_as("SELECT extension, member_id FROM voice_extensions WHERE org_id = $1")
            .bind(org_id)
            .fetch_all(&state.db)
            .await?;
    let has_one: HashSet<Uuid> = existing.iter().filter_map(|(_, m)| *m).collect();
    let taken: HashSet<String> = existing.into_iter().map(|(e, _)| e).collect();

    let people_total = people.len();
    let missing: Vec<(Uuid, String)> = people
        .into_iter()
        .filter(|p| !has_one.contains(&p.0))
        .map(|p| (p.0, p.1))
        .collect();
    let already_assigned = people_total - missing.len();
    let mut free = pin_rules::free_numbers(
        range.range_start,
        range.range_end,
        &taken,
        &state.config.voice_meeting_access_number,
    );

    let sip_domain = sip_domain_for_org(&state, org_id).await?;
    let mut assigned = Vec::new();
    let mut range_exhausted = false;
    let mut unresolved = 0usize;
    'people: for (i, (member_id, username)) in missing.iter().enumerate() {
        if assigned.len() >= ASSIGN_BATCH {
            unresolved += missing.len() - i;
            break;
        }
        // Um Argon2 por pessoa, não por número tentado.
        let secret = SipSecret::generate()?;
        loop {
            let Some(number) = free.next() else {
                range_exhausted = true;
                unresolved += missing.len() - i;
                break 'people;
            };
            match insert_extension(
                &state,
                org_id,
                &sip_domain,
                Some(*member_id),
                &number,
                "",
                &secret,
            )
            .await?
            {
                Inserted::Created { id, .. } => {
                    assigned.push(AssignedExtension {
                        id,
                        member_id: *member_id,
                        member_username: username.clone(),
                        extension: number,
                    });
                    break;
                }
                // Outro pedido ficou com o número entretanto: tenta o seguinte.
                Inserted::NumberTaken => continue,
                // Outro pedido deu ramal a esta pessoa entretanto: está servida.
                Inserted::MemberHasOne => break,
            }
        }
    }

    if !assigned.is_empty() {
        crate::audit::log(
            &state.db,
            Some(org_id),
            auth.user_id,
            "ramal.atribuicao_em_massa",
            &format!(
                "{} ramais ({}–{})",
                assigned.len(),
                assigned.first().map(|a| a.extension.as_str()).unwrap_or(""),
                assigned.last().map(|a| a.extension.as_str()).unwrap_or(""),
            ),
        )
        .await;
    }
    Ok(Json(AssignMissingResp {
        assigned,
        already_assigned,
        remaining: unresolved,
        range_exhausted,
        range_start: range.range_start,
        range_end: range.range_end,
    }))
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
pub struct XmlCurlDirectoryReq {
    #[serde(default)]
    pub user: String,
    #[serde(default)]
    pub domain: String,
}

/// `POST /internal/v1/voice/ivr/directory` — directório dinâmico do FreeSWITCH
/// (`mod_xml_curl`, secção "directory"). Chamado no REGISTER de um ramal para
/// obter o HA1 do digest SIP. Devolve sempre 200: "não encontrado" também é
/// uma resposta válida (o FreeSWITCH trata-o como XML, não como erro HTTP).
pub async fn ivr_directory(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Form(req): Form<XmlCurlDirectoryReq>,
) -> Result<Response, ApiError> {
    // O `mod_xml_curl` não põe cabeçalhos arbitrários, mas sabe enviar HTTP
    // Basic (`gateway-credentials` em xml_curl.conf.xml), e o
    // `check_media_secret` aceita o segredo como password do Basic. O segredo
    // NUNCA se aceita no URL (R227): um `?secret=` fica escrito no log do
    // FreeSWITCH a cada arranque e em qualquer log de acesso pelo caminho.
    check_media_secret(&state, &headers)?;

    let user = req.user.trim();
    let domain = req.domain.trim();
    if user.is_empty() || domain.is_empty() {
        return Ok(xml_response(XML_NOT_FOUND.into()));
    }
    let Some(org_id) = org_id_by_sip_domain(&state, domain).await else {
        return Ok(xml_response(XML_NOT_FOUND.into()));
    };

    let row: Option<(Uuid, String)> = sqlx::query_as(
        "SELECT id, sip_ha1 FROM voice_extensions
          WHERE org_id = $1 AND sip_username = $2 AND active",
    )
    .bind(org_id)
    .bind(user)
    .fetch_optional(&state.db)
    .await?;
    let Some((ext_id, stored)) = row else {
        return Ok(xml_response(XML_NOT_FOUND.into()));
    };
    // O HA1 está cifrado em repouso (R286); um herdado em claro ainda se lê,
    // até a tarefa de fundo o cifrar. Só aqui, à saída para o FreeSWITCH, é
    // que volta a ser o valor que o digest SIP usa.
    let ha1 = crate::secrets_at_rest::open(&state.config, &stored, &ha1_aad(ext_id))?;

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

/// Um dos dois: `sip_username` (o número é de um ramal desta org) ou
/// `meeting_access` (o número é o de acesso às reuniões). O contrato é com
/// `voice/freeswitch/scripts/ramais_dial.lua`.
#[derive(Serialize)]
pub struct ResolveExtensionResp {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sip_username: Option<String>,
    /// `true` => o Lua entrega a chamada ao IVR da sala em vez de tocar num
    /// ramal. Ausente em todos os outros casos.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub meeting_access: bool,
}

/// `POST /internal/v1/voice/ivr/resolve-extension` — chamado pelo dialplan interno
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
    // O número de acesso às reuniões ganha a qualquer ramal (R273), e é igual
    // em todas as orgs: responde-se antes de olhar para o domínio. Isto NÃO
    // autoriza nada — só diz ao dialplan para onde ir; quem decide se o ramal
    // entra numa sala é `voice::validate_pin_for_extension`.
    if ext_rules::is_meeting_access_number(
        req.extension.trim(),
        &state.config.voice_meeting_access_number,
    ) {
        return Ok(Json(ResolveExtensionResp {
            sip_username: None,
            meeting_access: true,
        }));
    }
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
        Some(sip_username) => Ok(Json(ResolveExtensionResp {
            sip_username: Some(sip_username),
            meeting_access: false,
        })),
        None => Err(ApiError::NotFound),
    }
}

/// Avisa no arranque se já existem ramais com o número de acesso às reuniões
/// (criados antes da R273, ou antes de se mudar `VOICE_MEETING_ACCESS_NUMBER`):
/// deixam de ser alcançáveis por esse número, porque o `resolve-extension` o
/// entrega ao IVR da sala. Não se apagam nem se renumeram sozinhos.
pub(crate) async fn warn_if_access_number_is_taken(state: &AppState) {
    let number = &state.config.voice_meeting_access_number;
    match sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM voice_extensions WHERE extension = $1")
        .bind(number)
        .fetch_one(&state.db)
        .await
    {
        Ok(0) => {}
        Ok(n) => tracing::warn!(
            ramais = n,
            numero = %number,
            "há ramais com o número de acesso às reuniões — quem os marcar cai no IVR da sala; renumere-os ou mude VOICE_MEETING_ACCESS_NUMBER"
        ),
        Err(e) => {
            tracing::warn!(error = %e, "não foi possível verificar o número de acesso às reuniões")
        }
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
// Fora de âmbito aqui: um ramal com DID atribuído recebe VOZ directa — quem
// liga para esse número fala com a pessoa do ramal, não entra numa sala do
// SFU. (O ramal ENTRA numa sala marcando o número de acesso: Fase 3, cabeçalho.)

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

/// `POST /internal/v1/voice/ivr/dialplan-did` — segunda secção do MESMO `mod_xml_curl`
/// que a directoria da Fase 1 (`ivr_directory`, mesmo segredo, por
/// `X-Voice-Secret` ou HTTP Basic): o FreeSWITCH pede aqui o dialplan dinâmico da secção
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
    Form(fields): Form<std::collections::HashMap<String, String>>,
) -> Result<Response, ApiError> {
    // O mesmo segredo que `ivr_directory`, pelo cabeçalho ou por HTTP Basic;
    // nunca no URL (R227).
    check_media_secret(&state, &headers)?;

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
/// callbacks `/internal/v1/voice/ivr/*` do `mod_xml_curl` do FreeSWITCH ficam de fora:
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
        unassign_extension_did,
        get_extension_range,
        put_extension_range,
        assign_missing_extensions,
        crate::extension_pin::my_extension,
        crate::extension_pin::set_my_pin,
        crate::extension_pin::regenerate_my_pin,
        crate::extension_pin::set_extension_pin,
        crate::extension_pin::regenerate_extension_pin,
        crate::extension_pin::clear_extension_pin,
        crate::extension_provisioning::issue_my_ticket,
        crate::extension_provisioning::issue_extension_ticket,
        crate::extension_provisioning::redeem
    ),
    components(schemas(
        VoiceExtensionInfo,
        SipServerInfo,
        CreatedExtension,
        CreateExtensionReq,
        UpdateExtensionReq,
        AssignExtensionDidReq,
        ExtensionDidInfo,
        ExtensionRange,
        PutExtensionRangeReq,
        AssignedExtension,
        AssignMissingResp,
        crate::extension_pin::MyExtension,
        crate::extension_pin::SetPinReq,
        crate::extension_pin::GeneratedPin,
        crate::extension_provisioning::ProvisioningTicket
    ))
)]
pub struct ApiDoc;
