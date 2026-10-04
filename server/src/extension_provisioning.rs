//! Provisionamento do Linphone por QR de uso único (plano de produção, item
//! 3.8, lote 3 — R278).
//!
//! A password SIP de um ramal é longa, aleatória e mostrada uma vez, e na
//! atribuição em massa ninguém a vê. A decisão do dono é que ninguém a digite:
//!
//! 1. **Emissão** (sessão): a própria pessoa para o seu ramal
//!    (`POST …/my-extension/provisioning-ticket`) ou o administrador para
//!    qualquer ramal da organização (`POST …/extensions/{id}/provisioning-ticket`
//!    — já pode regenerar a password SIP de qualquer ramal, por isso isto não
//!    lhe dá poder novo). Sai um URL com um token de 256 bits, válido dez
//!    minutos; só o SHA-256 do token fica na base, e emitir outro apaga o
//!    anterior. A consola põe o URL num QR.
//! 2. **Resgate** (público, sem sessão — o telefone não tem conta):
//!    `GET /api/public/extension-provisioning/{token}`. O bilhete gasta-se de
//!    forma atómica (um `UPDATE … WHERE consumed_at IS NULL` — de dois resgates
//!    em paralelo só um ganha), o ramal recebe uma password SIP NOVA e a
//!    resposta é a configuração `lpconfig` do Linphone com ela. Bilhete
//!    inexistente, já usado, expirado, mal formado, ou ramal inactivo: sempre o
//!    mesmo `404 ramais.provisioning_invalid`.
//!
//! **Ler o QR troca a password SIP**: o aparelho que estava registado com a
//! anterior deixa de registar. A consola di-lo antes de mostrar o QR.
//!
//! **Onde o token fica e onde não fica.** NÃO fica na auditoria (o alvo é o
//! número do ramal e o IP de quem resgatou) nem no `tracing` do servidor (o
//! caminho desta rota sai redigido do span HTTP — `redact_path`, abaixo). FICA
//! no registo de acessos de qualquer proxy à frente que não tenha sido
//! instruído a calar esta rota: os três nginx do repositório têm uma
//! `location` com `access_log off` para ela; um ingress-nginx de cluster não.
//! Um bilhete que aparece num registo já foi gasto ou expira em dez minutos.
//!
//! **O que protege a rota pública** são os 256 bits do token e o uso único —
//! não o limite por IP. Esse existe (`provisioning_limiter`), mas
//! `rate_limit::client_ip` confia no primeiro elemento de `X-Forwarded-For`
//! quando o par é um proxy, e os proxies do repositório ACRESCENTAM ao que o
//! cliente mandou: quem forjar o cabeçalho escolhe o seu balde (defeito
//! anterior a este módulo, aberto na R278).
//!
//! **Não verificado:** nenhum Linphone real leu um destes QR. O formato da
//! configuração é o documentado (`delonix_meet_domain::telephony::extension_provisioning`).

use axum::{
    extract::{ConnectInfo, Path, State},
    http::{header, HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use chrono::{DateTime, Duration, Utc};
use serde::Serialize;
use std::{net::SocketAddr, sync::Arc};
use uuid::Uuid;

use crate::{auth::AuthUser, error::ApiError, AppState};
use delonix_meet_core::DomainError;
use delonix_meet_domain::telephony::{extension::SipServer, extension_provisioning as rules};

/// O caminho público do resgate, sem o token. `lib.rs` monta a rota com ele e
/// redige o que vem a seguir nos registos.
pub(crate) const REDEEM_PREFIX: &str = "/api/public/extension-provisioning/";

/// Um bilhete acabado de emitir. O URL leva o token: mostra-se uma vez. Sem
/// `Debug` de propósito — o URL É a credencial, e um `{:?}` punha-o num registo.
#[derive(Serialize, utoipa::ToSchema)]
pub struct ProvisioningTicket {
    /// O URL que o Linphone descarrega («remote provisioning»). A consola põe-no
    /// num QR; serve uma vez.
    #[schema(example = "https://meet.exemplo.ao/api/public/extension-provisioning/3f9c…")]
    pub provisioning_url: String,
    /// Depois disto o URL já não serve.
    pub expires_at: DateTime<Utc>,
    /// O ramal que o QR configura.
    #[schema(example = "1004")]
    pub extension: String,
}

/// O que é preciso para um QR levar a algum lado: o endereço público do
/// servidor SIP (onde o aparelho regista) e a origem pública desta instalação
/// (de onde o aparelho descarrega). Sem um deles não se emite nada.
fn provisioning_targets(state: &AppState) -> Result<(&SipServer, &str), ApiError> {
    let server = state.config.voice_ramais_public.as_ref().ok_or_else(|| {
        DomainError::precondition(
            "ramais.sip_server_missing",
            "a instalação não tem o endereço público do servidor SIP (VOICE_RAMAIS_PUBLIC_HOST): \
             um QR não levaria o aparelho a lado nenhum",
        )
    })?;
    // A origem pública é a primeira de `CORS_ORIGINS` — a mesma de onde o
    // retorno do SSO se constrói (`auth.rs`). Nunca o `Host` do pedido.
    let base = state
        .config
        .cors_origins
        .first()
        .and_then(|o| rules::public_base_url(o))
        .ok_or_else(|| {
            DomainError::precondition(
                "ramais.public_url_missing",
                "a instalação não tem um endereço https público (CORS_ORIGINS): \
                 o telefone não conseguiria descarregar a configuração",
            )
        })?;
    Ok((server, base))
}

/// Grava um bilhete novo para o ramal (apagando os anteriores) e devolve-o.
async fn issue(
    state: &AppState,
    org_id: Uuid,
    extension_id: Uuid,
    extension: String,
    active: bool,
    actor: Uuid,
) -> Result<ProvisioningTicket, ApiError> {
    if !active {
        return Err(DomainError::precondition(
            "ramais.extension_inactive",
            "o ramal está inactivo: active-o antes de configurar um aparelho",
        )
        .into());
    }
    let (_, base) = provisioning_targets(state)?;
    let token = crate::crypto::random_hex(rules::TICKET_BYTES);
    let expires_at = Utc::now() + Duration::seconds(rules::TICKET_TTL_SECS);

    let mut tx = state.db.begin().await?;
    // Um bilhete vivo por ramal: o QR anterior deixa de servir.
    sqlx::query(
        "DELETE FROM voice_extension_provisioning_tickets WHERE extension_id = $1 AND org_id = $2",
    )
    .bind(extension_id)
    .bind(org_id)
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "INSERT INTO voice_extension_provisioning_tickets
             (org_id, extension_id, token_hash, created_by, expires_at)
         VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(org_id)
    .bind(extension_id)
    .bind(crate::crypto::sha256_hex(&token))
    .bind(actor)
    .bind(expires_at)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;

    crate::audit::log(
        &state.db,
        Some(org_id),
        actor,
        "ramal.provisionamento_emitido",
        &extension,
    )
    .await;
    Ok(ProvisioningTicket {
        provisioning_url: format!("{base}{REDEEM_PREFIX}{token}"),
        expires_at,
        extension,
    })
}

/// A pessoa pede um QR para configurar o Linphone do SEU ramal (*custom
/// method*). Cada pedido substitui o QR anterior.
#[utoipa::path(
    post, path = "/api/orgs/{org_id}/my-extension/provisioning-ticket", tag = "voice",
    security(("session" = [])),
    params(("org_id" = Uuid, Path, description = "Organização.")),
    responses(
        (status = 200, body = ProvisioningTicket, description = "O URL serve UMA vez, durante dez minutos. Lê-lo troca a password SIP do ramal."),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 404, body = crate::openapi::ErrorBody, description = "Não é membro activo, ou não tem ramal (`ramais.no_extension`)."),
        (status = 422, body = crate::openapi::ErrorBody, description = "`ramais.sip_server_missing`, `ramais.public_url_missing` ou `ramais.extension_inactive`."),
    )
)]
pub async fn issue_my_ticket(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(org_id): Path<Uuid>,
) -> Result<Json<ProvisioningTicket>, ApiError> {
    crate::org::require_member_pub(&state, org_id, auth.user_id).await?;
    let (id, extension, active): (Uuid, String, bool) = sqlx::query_as(
        "SELECT id, extension, active FROM voice_extensions WHERE org_id = $1 AND member_id = $2",
    )
    .bind(org_id)
    .bind(auth.user_id)
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(|| DomainError::not_found("ramais.no_extension"))?;
    issue(&state, org_id, id, extension, active, auth.user_id)
        .await
        .map(Json)
}

/// O administrador pede um QR para configurar o Linphone de um ramal da
/// organização — de pessoa ou da empresa (*custom method*). Não lhe dá poder
/// novo: já pode regenerar a password SIP de qualquer ramal.
#[utoipa::path(
    post, path = "/api/orgs/{org_id}/extensions/{id}/provisioning-ticket", tag = "voice",
    security(("session" = [])),
    params(("org_id" = Uuid, Path, description = "Organização."), ("id" = Uuid, Path, description = "Ramal.")),
    responses(
        (status = 200, body = ProvisioningTicket, description = "O URL serve UMA vez, durante dez minutos. Lê-lo troca a password SIP do ramal."),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 403, body = crate::openapi::ErrorBody),
        (status = 404, body = crate::openapi::ErrorBody),
        (status = 422, body = crate::openapi::ErrorBody, description = "`ramais.sip_server_missing`, `ramais.public_url_missing` ou `ramais.extension_inactive`."),
    )
)]
pub async fn issue_extension_ticket(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path((org_id, id)): Path<(Uuid, Uuid)>,
) -> Result<Json<ProvisioningTicket>, ApiError> {
    crate::org::require_admin_pub(&state, org_id, auth.user_id).await?;
    let (extension, active): (String, bool) = sqlx::query_as(
        "SELECT extension, active FROM voice_extensions WHERE id = $1 AND org_id = $2",
    )
    .bind(id)
    .bind(org_id)
    .fetch_optional(&state.db)
    .await?
    .ok_or(ApiError::NotFound)?;
    issue(&state, org_id, id, extension, active, auth.user_id)
        .await
        .map(Json)
}

/// A recusa única do resgate: não diz se o bilhete não existe, já foi usado,
/// expirou ou o ramal está inactivo.
fn invalid() -> ApiError {
    DomainError::not_found("ramais.provisioning_invalid").into()
}

/// Apaga os bilhetes de um ramal — os QR ainda por ler deixam de servir. É o
/// que a regeneração da password pelo administrador chama: sem isto, um QR
/// emitido antes trocava a password nova outra vez.
pub(crate) async fn revoke_tickets(
    db: &sqlx::PgPool,
    org_id: Uuid,
    extension_id: Uuid,
) -> Result<(), ApiError> {
    sqlx::query(
        "DELETE FROM voice_extension_provisioning_tickets WHERE extension_id = $1 AND org_id = $2",
    )
    .bind(extension_id)
    .bind(org_id)
    .execute(db)
    .await?;
    Ok(())
}

#[derive(sqlx::FromRow)]
struct RedeemRow {
    org_id: Uuid,
    extension_id: Uuid,
}

#[derive(sqlx::FromRow)]
struct ExtensionRow {
    extension: String,
    sip_username: String,
    member_id: Option<Uuid>,
    label: String,
    active: bool,
    member_username: Option<String>,
    org_slug: String,
}

/// O telefone troca o bilhete pela configuração do Linphone. Rota PÚBLICA: a
/// credencial é o token no caminho, de uso único e com dez minutos. É um `GET`
/// com efeito (gasta o bilhete e troca a password SIP) porque é assim que o
/// Linphone descarrega a configuração — não há outro verbo à escolha.
#[utoipa::path(
    get, path = "/api/public/extension-provisioning/{token}", tag = "voice",
    params(("token" = String, Path, description = "O bilhete do QR (64 hex).")),
    responses(
        (status = 200, body = String, content_type = "application/xml", description = "Configuração `lpconfig` do Linphone, com uma password SIP nova. `Cache-Control: no-store`."),
        (status = 404, body = crate::openapi::ErrorBody, description = "`ramais.provisioning_invalid` — não existe, já usado, expirado ou ramal inactivo (a mesma resposta para todos)."),
        (status = 429, body = crate::openapi::ErrorBody),
    )
)]
pub async fn redeem(
    State(state): State<Arc<AppState>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path(token): Path<String>,
) -> Result<Response, ApiError> {
    let ip = crate::rate_limit::client_ip(&headers, addr.ip(), state.config.trusted_proxy_hops);
    state
        .provisioning_limiter
        .acquire(&ip)
        .map_err(|retry_in| ApiError::RateLimited {
            retry_after_secs: retry_in.as_secs().max(1),
        })?;
    if !rules::is_ticket_shape(&token) {
        return Err(invalid());
    }
    // Sem servidor SIP público já não há o que pôr na configuração (a
    // instalação mudou depois da emissão): o bilhete fica por gastar.
    let Some(server) = state.config.voice_ramais_public.as_ref() else {
        return Err(invalid());
    };
    let token_hash = crate::crypto::sha256_hex(&token);

    // Só quem traz um bilhete vivo custa um Argon2: sem esta pergunta, cada
    // token inventado punha o servidor a calcular um hash.
    let alive: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM voice_extension_provisioning_tickets
                         WHERE token_hash = $1 AND consumed_at IS NULL AND expires_at > now())",
    )
    .bind(&token_hash)
    .fetch_one(&state.db)
    .await?;
    if !alive {
        return Err(invalid());
    }
    // O segredo calcula-se ANTES de abrir a transacção: o Argon2 não corre com
    // a linha do ramal bloqueada.
    let secret = crate::ramais::SipSecret::generate()?;

    // Daqui em diante tudo se lê e escreve pela MESMA ligação: com a linha
    // bloqueada não se vai buscar outra ao pool.
    let mut tx = state.db.begin().await?;
    // Gastar primeiro: de dois resgates em paralelo, o segundo espera pelo
    // bloqueio da linha, volta a avaliar `consumed_at IS NULL` e não a apanha.
    let ticket: Option<RedeemRow> = sqlx::query_as(
        "UPDATE voice_extension_provisioning_tickets SET consumed_at = now()
          WHERE token_hash = $1 AND consumed_at IS NULL AND expires_at > now()
          RETURNING org_id, extension_id",
    )
    .bind(&token_hash)
    .fetch_optional(&mut *tx)
    .await?;
    let Some(ticket) = ticket else {
        return Err(invalid());
    };
    let ext: Option<ExtensionRow> = sqlx::query_as(
        "SELECT e.extension, e.sip_username, e.member_id, e.label, e.active,
                u.username AS member_username, o.slug AS org_slug
           FROM voice_extensions e
           JOIN organizations o ON o.id = e.org_id
           LEFT JOIN users u ON u.id = e.member_id
          WHERE e.id = $1 AND e.org_id = $2
          FOR UPDATE OF e",
    )
    .bind(ticket.extension_id)
    .bind(ticket.org_id)
    .fetch_optional(&mut *tx)
    .await?;
    // Ramal inactivo, ou de quem saiu da organização: não se configura — e o
    // bilhete GASTA-SE na mesma (o commit abaixo). Reactivar o ramal não
    // ressuscita um QR antigo.
    let mut usable = ext.filter(|e| e.active);
    if let Some(member_id) = usable.as_ref().and_then(|e| e.member_id) {
        // A pertença decide-se em org.rs, pela mesma ligação.
        let active_member = crate::org::member_state(&mut *tx, ticket.org_id, member_id)
            .await?
            .is_some_and(|m| m.archived_at.is_none());
        if !active_member {
            usable = None;
        }
    }
    let Some(ext) = usable else {
        tx.commit().await?;
        return Err(invalid());
    };

    let sip_domain = crate::ramais::sip_domain_of_slug(&state, &ext.org_slug);
    // Cifrado em repouso, como em todos os sítios que escrevem o HA1 (R286).
    let ha1 = crate::ramais::seal_ha1(
        &state,
        ticket.extension_id,
        &secret.ha1(&ext.sip_username, &sip_domain),
    )?;
    sqlx::query(
        "UPDATE voice_extensions SET sip_password_hash = $3, sip_ha1 = $4
          WHERE id = $1 AND org_id = $2",
    )
    .bind(ticket.extension_id)
    .bind(ticket.org_id)
    .bind(&secret.hash)
    .bind(&ha1)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;

    // Actor de sistema: quem resgata não tem conta. O alvo é o ramal e a
    // ORIGEM do pedido — é o que deixa perceber, depois, quem leu o QR.
    crate::audit::log(
        &state.db,
        Some(ticket.org_id),
        Uuid::nil(),
        "ramal.provisionado",
        &format!("{} ← {ip}", ext.extension),
    )
    .await;

    let display_name = match (&ext.member_username, ext.label.trim()) {
        (Some(u), _) => u.as_str(),
        (None, label) => label,
    };
    let xml = rules::linphone_config_xml(&rules::LinphoneAccount {
        display_name,
        sip_username: &ext.sip_username,
        sip_domain: &sip_domain,
        sip_password: &secret.password,
        server,
    });
    let mut res = (StatusCode::OK, xml).into_response();
    let h = res.headers_mut();
    h.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/xml; charset=utf-8"),
    );
    h.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    h.insert(header::PRAGMA, HeaderValue::from_static("no-cache"));
    Ok(res)
}

/// O caminho que um registo pode guardar: o do resgate leva o token, e sai
/// sem ele.
pub(crate) fn redact_path(path: &str) -> &str {
    if path.starts_with(REDEEM_PREFIX) {
        "/api/public/extension-provisioning/…"
    } else {
        path
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn o_token_nao_fica_no_caminho_registado() {
        let p = format!("{REDEEM_PREFIX}{}", "ab".repeat(32));
        assert!(!redact_path(&p).contains("abab"));
        assert_eq!(
            redact_path("/api/orgs/x/extensions"),
            "/api/orgs/x/extensions"
        );
    }
}
