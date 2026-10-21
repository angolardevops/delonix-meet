//! Web Push — o toque de uma reunião chega a quem tem a aplicação fechada
//! (lacuna `A-03` da fila de paridade, ADR-0032).
//!
//! **PORQUE EXISTE.** Um toque só chegava por WebSocket (`PresenceHub::notify`).
//! Quem não tinha a aba aberta ficava em `offline` na resposta do `ring` e com
//! uma chamada perdida para ver DEPOIS — nunca era avisado a tempo. Isto envia,
//! a quem não tem socket, um aviso pelo serviço de push do browser dele
//! (RFC 8030), cifrado de ponta a ponta (RFC 8291) e assinado com a chave VAPID
//! da instalação (RFC 8292).
//!
//! **Contrato** (rotas pessoais, como as notificações — não entram no
//! `isolamento.mjs`, porque não têm escopo de organização):
//! - `GET  /api/users/me/push`                              estado: ligado, chave pública, quantos browsers
//! - `POST /api/users/me/push/subscriptions`                regista a subscrição deste browser (`201`)
//! - `POST /api/users/me/push/subscriptions/unsubscribe`    retira-a (`204`), só a da própria pessoa
//!
//! **Três decisões, escritas no ADR-0032:**
//! 1. **Desligado por omissão.** Sem `WEB_PUSH_VAPID_PRIVATE_KEY` e
//!    `WEB_PUSH_SUBJECT`, nada sai: é a primeira saída do produto para a Google,
//!    a Mozilla e a Apple, e quem a abre é o operador. O que sai vai cifrado — o
//!    serviço de push vê que houve um aviso, para que browser e quando; não o lê.
//! 2. **O aviso não diz o assunto nem quem chama** ([`aviso`]). Um aviso no ecrã
//!    de bloqueio é uma fuga de DLP: o título de uma reunião («Despedimento
//!    colectivo — Q4») não se mostra a quem pega no telemóvel de outra pessoa.
//!    Diz que há uma reunião a chamar, e o link abre-a.
//! 3. **Só serviços de push conhecidos** ([`host_de_push`]). O endpoint vem do
//!    browser, logo do cliente: sem esta lista, registar
//!    `https://intranet.cliente/…` punha o servidor a fazer POSTs para onde
//!    alguém quisesse. Os nomes de `OUTBOUND_ALLOW_HOSTS` também valem — é
//!    assim que os testes apontam a um serviço falso.
//!
//! **O que NÃO faz:** tentar outra vez. Um aviso perdido é aceitável (fica a
//! chamada perdida); uma fila de retentativas é outra fatia. E «aceite pelo
//! serviço de push» não é «visto»: é o que o servidor pode saber.

use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use axum::{extract::State, http::StatusCode, Json};
use base64::Engine as _;
use delonix_meet_core::DomainError;
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use web_push_native::{
    jwt_simple::algorithms::{ECDSAP256PublicKeyLike, ES256KeyPair},
    p256::PublicKey,
    Auth, WebPushBuilder,
};

use crate::{auth::AuthUser, config::Config, error::ApiError, AppState};

/// Quanto tempo o serviço de push guarda o aviso se o aparelho estiver
/// desligado. Um toque de reunião com mais de dois minutos já não é um toque.
const TTL: Duration = Duration::from_secs(120);
/// Quanto se espera por um serviço de push antes de dar o aviso por falhado.
/// Curto: isto corre dentro do `ring`, e quem chama está à espera da resposta.
const PRAZO: Duration = Duration::from_secs(5);
/// Browsers por pessoa. Os mais antigos saem: sem tecto, um cliente em ciclo
/// enchia a tabela e cada `ring` fazia N pedidos de saída.
const MAX_POR_PESSOA: i64 = 10;
const MAX_ENDPOINT: usize = 2048;

/// Os serviços de push dos browsers que existem. Um sufixo de domínio, e não
/// um prefixo de URL, porque cada serviço tem vários nós
/// (`wns2-par02p.notify.windows.com`, `updates.push.services.mozilla.com`).
const SERVICOS_DE_PUSH: &[&str] = &[
    "fcm.googleapis.com",
    "android.googleapis.com",
    "push.services.mozilla.com",
    "notify.windows.com",
    "push.apple.com",
];

/// O host é de um serviço de push conhecido? Comparação exacta ou por
/// sufixo com ponto — `evilfcm.googleapis.com.example` não passa, nem
/// `xfcm.googleapis.com`.
pub(crate) fn host_de_push(host: &str) -> bool {
    let h = host.trim_end_matches('.').to_ascii_lowercase();
    SERVICOS_DE_PUSH
        .iter()
        .any(|s| h == *s || h.ends_with(&format!(".{s}")))
}

/// A chave VAPID da instalação, quando o operador a configurou.
struct Chaves {
    par: ES256KeyPair,
    /// A chave pública em base64url, sem preenchimento — é o que o browser
    /// recebe como `applicationServerKey`.
    publica: String,
    contacto: String,
}

fn b64(v: &str) -> Option<Vec<u8>> {
    base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(v.trim().trim_end_matches('='))
        .ok()
}

fn chaves(cfg: &Config) -> Option<Chaves> {
    let privada = cfg.web_push_vapid_private_key.as_deref()?;
    let contacto = cfg.web_push_subject.clone()?;
    let par = ES256KeyPair::from_bytes(&b64(privada)?).ok()?;
    let publica = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .encode(par.public_key().public_key().to_bytes_uncompressed());
    Some(Chaves {
        par,
        publica,
        contacto,
    })
}

/// Validação no ARRANQUE. Uma chave mal colada não pode ficar a desligar o
/// Web Push em silêncio — o operador configurou-o e julgaria que está ligado.
/// Metade configurada (só a chave, ou só o contacto) também é erro.
pub fn validar_config(cfg: &Config) -> Result<(), String> {
    match (&cfg.web_push_vapid_private_key, &cfg.web_push_subject) {
        (None, None) => Ok(()),
        (Some(_), None) | (None, Some(_)) => Err(
            "WEB_PUSH_VAPID_PRIVATE_KEY e WEB_PUSH_SUBJECT vão juntas: falta uma das duas".into(),
        ),
        (Some(_), Some(contacto)) => {
            if !(contacto.starts_with("mailto:") || contacto.starts_with("https://")) {
                return Err(format!(
                    "WEB_PUSH_SUBJECT tem de ser `mailto:` ou `https://` (RFC 8292), veio «{contacto}»"
                ));
            }
            chaves(cfg).map(|_| ()).ok_or_else(|| {
                "WEB_PUSH_VAPID_PRIVATE_KEY não é um escalar P-256 de 32 bytes em base64url".into()
            })
        }
    }
}

/// O texto do aviso, na língua da pessoa. **Sem o assunto da reunião nem o nome
/// de quem chama** — ver a decisão 2 no cabeçalho. A função nem os recebe, para
/// ninguém os acrescentar «só desta vez».
fn aviso(room_code: &str, locale: &str) -> serde_json::Value {
    let corpo = match locale.get(..2).unwrap_or("pt") {
        "en" => "You are being called into a meeting. Tap to join.",
        "fr" => "On vous appelle à une réunion. Touchez pour la rejoindre.",
        "zh" => "有人邀请您加入会议。点按即可加入。",
        _ => "Estão a chamar-te para uma reunião. Toca para entrar.",
    };
    serde_json::json!({
        "kind": "ring",
        "title": "Delonix Meet",
        "body": corpo,
        "room_code": room_code,
        "link": format!("/#/r/{room_code}"),
    })
}

// ---------- contrato ----------

/// O que o browser precisa de saber antes de pedir permissão.
#[derive(Serialize, utoipa::ToSchema)]
pub struct WebPushStatus {
    /// `false` quando a instalação não configurou o Web Push: o ecrã não
    /// oferece o que o servidor não envia.
    pub enabled: bool,
    /// A `applicationServerKey` (base64url). Nula com o Web Push desligado.
    pub public_key: Option<String>,
    /// Quantos browsers desta pessoa estão subscritos.
    pub subscriptions: i64,
}

#[derive(Deserialize, utoipa::ToSchema)]
pub struct WebPushKeys {
    pub p256dh: String,
    pub auth: String,
}

/// A forma do `PushSubscription.toJSON()` do browser — `expirationTime` e o
/// resto são ignorados.
#[derive(Deserialize, utoipa::ToSchema)]
pub struct WebPushSubscribeReq {
    pub endpoint: String,
    pub keys: WebPushKeys,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct WebPushSubscribeResp {
    pub id: Uuid,
}

#[derive(Deserialize, utoipa::ToSchema)]
pub struct WebPushUnsubscribeReq {
    pub endpoint: String,
}

#[derive(utoipa::OpenApi)]
#[openapi(paths(status, subscribe, unsubscribe))]
pub struct ApiDoc;

/// O endpoint pode receber avisos deste servidor? URL de inquilino (nada
/// interno, pelo `net_guard`), HTTPS, e um serviço de push conhecido — salvo
/// os nomes que o operador declarou em `OUTBOUND_ALLOW_HOSTS`.
async fn endpoint_permitido(state: &AppState, endpoint: &str) -> Result<reqwest::Url, ApiError> {
    if endpoint.len() > MAX_ENDPOINT {
        return Err(
            DomainError::invalid("push.endpoint_invalid", "endpoint demasiado longo").into(),
        );
    }
    let url = state.outbound.check_tenant_config_url(endpoint).await?;
    let host = url.host_str().unwrap_or_default();
    let declarado =
        delonix_meet_core::egress::host_is_allowlisted(host, &state.config.outbound_allow_hosts);
    if !declarado && !host_de_push(host) {
        return Err(DomainError::invalid(
            "push.endpoint_not_allowed",
            "o endpoint não é de um serviço de push conhecido",
        )
        .into());
    }
    if !declarado && url.scheme() != "https" {
        return Err(
            DomainError::invalid("push.endpoint_not_https", "o endpoint tem de ser HTTPS").into(),
        );
    }
    Ok(url)
}

/// As chaves do browser: um ponto P-256 válido e um segredo de 16 bytes. Uma
/// chave inválida guardada fazia falhar TODOS os avisos a esta pessoa, calados.
fn chaves_do_browser(p256dh: &str, auth: &str) -> Result<(PublicKey, Auth), ApiError> {
    let invalido = || {
        DomainError::invalid(
            "push.keys_invalid",
            "as chaves da subscrição não são válidas",
        )
    };
    let publica = b64(p256dh)
        .and_then(|b| PublicKey::from_sec1_bytes(&b).ok())
        .ok_or_else(invalido)?;
    let auth = b64(auth).filter(|b| b.len() == 16).ok_or_else(invalido)?;
    Ok((publica, Auth::clone_from_slice(&auth)))
}

/// `GET /api/users/me/push`
#[utoipa::path(
    get, path = "/api/users/me/push", tag = "notifications",
    security(("session" = [])),
    responses(
        (status = 200, body = WebPushStatus, description = "Se o Web Push está ligado nesta instalação, a chave pública VAPID e quantos browsers desta pessoa estão subscritos."),
        (status = 401, body = crate::openapi::ErrorBody),
    )
)]
pub async fn status(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
) -> Result<Json<WebPushStatus>, ApiError> {
    let ch = chaves(&state.config);
    let subscriptions: i64 =
        sqlx::query_scalar("SELECT count(*) FROM web_push_subscriptions WHERE user_id = $1")
            .bind(auth.user_id)
            .fetch_one(&state.db)
            .await?;
    Ok(Json(WebPushStatus {
        enabled: ch.is_some(),
        public_key: ch.map(|c| c.publica),
        subscriptions,
    }))
}

/// `POST /api/users/me/push/subscriptions`
#[utoipa::path(
    post, path = "/api/users/me/push/subscriptions", tag = "notifications",
    security(("session" = [])),
    request_body = WebPushSubscribeReq,
    responses(
        (status = 201, body = WebPushSubscribeResp, description = "Subscrição registada. O mesmo endpoint registado por outra pessoa passa a ser desta — é o mesmo browser."),
        (status = 400, body = crate::openapi::ErrorBody, description = "`push.endpoint_invalid`, `push.endpoint_not_allowed`, `push.endpoint_not_https` ou `push.keys_invalid`."),
        (status = 401, body = crate::openapi::ErrorBody),
        (status = 409, body = crate::openapi::ErrorBody, description = "`push.not_configured`: esta instalação não configurou o Web Push."),
    )
)]
pub async fn subscribe(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    headers: axum::http::HeaderMap,
    Json(req): Json<WebPushSubscribeReq>,
) -> Result<(StatusCode, Json<WebPushSubscribeResp>), ApiError> {
    if chaves(&state.config).is_none() {
        return Err(DomainError::conflict(
            "push.not_configured",
            "esta instalação não configurou o Web Push",
        )
        .into());
    }
    endpoint_permitido(&state, &req.endpoint).await?;
    chaves_do_browser(&req.keys.p256dh, &req.keys.auth)?;
    let user_agent: Option<String> = headers
        .get(axum::http::header::USER_AGENT)
        .and_then(|v| v.to_str().ok())
        .map(|v| v.chars().take(256).collect());

    let mut tx = state.db.begin().await?;
    let id: Uuid = sqlx::query_scalar(
        "INSERT INTO web_push_subscriptions (user_id, endpoint, p256dh, auth, user_agent)
         VALUES ($1, $2, $3, $4, $5)
         ON CONFLICT (endpoint) DO UPDATE
            SET user_id = EXCLUDED.user_id, p256dh = EXCLUDED.p256dh, auth = EXCLUDED.auth,
                user_agent = EXCLUDED.user_agent, created_at = now(),
                failures = 0, last_failure_at = NULL
         RETURNING id",
    )
    .bind(auth.user_id)
    .bind(&req.endpoint)
    .bind(&req.keys.p256dh)
    .bind(&req.keys.auth)
    .bind(user_agent)
    .fetch_one(&mut *tx)
    .await?;
    sqlx::query(
        "DELETE FROM web_push_subscriptions
          WHERE user_id = $1
            AND id NOT IN (SELECT id FROM web_push_subscriptions WHERE user_id = $1
                            ORDER BY created_at DESC LIMIT $2)",
    )
    .bind(auth.user_id)
    .bind(MAX_POR_PESSOA)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok((StatusCode::CREATED, Json(WebPushSubscribeResp { id })))
}

/// `POST /api/users/me/push/subscriptions/unsubscribe`
#[utoipa::path(
    post, path = "/api/users/me/push/subscriptions/unsubscribe", tag = "notifications",
    security(("session" = [])),
    request_body = WebPushUnsubscribeReq,
    responses(
        (status = 204, description = "A subscrição deste browser saiu. Um endpoint que não é desta pessoa não muda nada — e a resposta é a mesma, para não revelar de quem é."),
        (status = 401, body = crate::openapi::ErrorBody),
    )
)]
pub async fn unsubscribe(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Json(req): Json<WebPushUnsubscribeReq>,
) -> Result<StatusCode, ApiError> {
    sqlx::query("DELETE FROM web_push_subscriptions WHERE user_id = $1 AND endpoint = $2")
        .bind(auth.user_id)
        .bind(&req.endpoint)
        .execute(&state.db)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

// ---------- envio ----------

#[derive(sqlx::FromRow)]
struct Subscricao {
    id: Uuid,
    user_id: Uuid,
    endpoint: String,
    p256dh: String,
    auth: String,
    locale: String,
}

enum Resultado {
    Aceite,
    Expirada,
    Falhou(String),
}

/// Avisa, por Web Push, as pessoas que não têm socket. Devolve as pessoas a
/// quem pelo menos um aviso foi ACEITE por um serviço de push.
///
/// Os envios correm em paralelo e cada um tem [`PRAZO`]: isto corre dentro do
/// `ring`, e um serviço de push lento não pode atrasar a resposta a quem chama.
/// Nunca falha: um erro aqui é um aviso a menos, não um toque que não aconteceu.
pub(crate) async fn ring(state: &Arc<AppState>, users: &[Uuid], room_code: &str) -> HashSet<Uuid> {
    let Some(ch) = chaves(&state.config) else {
        return HashSet::new();
    };
    if users.is_empty() {
        return HashSet::new();
    }
    let subs: Vec<Subscricao> = match sqlx::query_as(
        "SELECT s.id, s.user_id, s.endpoint, s.p256dh, s.auth, u.locale
           FROM web_push_subscriptions s JOIN users u ON u.id = s.user_id
          WHERE s.user_id = ANY($1)",
    )
    .bind(users)
    .fetch_all(&state.db)
    .await
    {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!(error = %e, "web push: não foi possível ler as subscrições");
            return HashSet::new();
        }
    };
    let ch = &ch;
    let envios = subs.iter().map(|s| async move {
        let r = match tokio::time::timeout(PRAZO, entregar(state, ch, s, room_code)).await {
            Ok(r) => r,
            Err(_) => Resultado::Falhou("sem resposta no prazo".into()),
        };
        (s, r)
    });
    let mut aceites = HashSet::new();
    for (s, r) in futures_util::future::join_all(envios).await {
        match r {
            Resultado::Aceite => {
                crate::metrics::Metrics::bump(&state.metrics.web_push_sent_total);
                let _ = sqlx::query(
                    "UPDATE web_push_subscriptions SET last_success_at = now(), failures = 0 WHERE id = $1",
                )
                .bind(s.id)
                .execute(&state.db)
                .await;
                aceites.insert(s.user_id);
            }
            Resultado::Expirada => {
                crate::metrics::Metrics::bump(&state.metrics.web_push_expired_total);
                let _ = sqlx::query("DELETE FROM web_push_subscriptions WHERE id = $1")
                    .bind(s.id)
                    .execute(&state.db)
                    .await;
            }
            Resultado::Falhou(razao) => {
                crate::metrics::Metrics::bump(&state.metrics.web_push_failed_total);
                tracing::warn!(subscricao = %s.id, %razao, "web push: aviso não entregue");
                let _ = sqlx::query(
                    "UPDATE web_push_subscriptions
                        SET failures = failures + 1, last_failure_at = now() WHERE id = $1",
                )
                .bind(s.id)
                .execute(&state.db)
                .await;
            }
        }
    }
    aceites
}

async fn entregar(state: &AppState, ch: &Chaves, s: &Subscricao, room_code: &str) -> Resultado {
    // De novo no envio, e não só ao registar: uma linha escrita antes de a
    // regra existir, ou um nome que entretanto passou a resolver para dentro,
    // não pode pôr o servidor a fazer POSTs para onde não deve.
    let url = match endpoint_permitido(state, &s.endpoint).await {
        Ok(u) => u,
        Err(_) => return Resultado::Falhou("endpoint recusado pela guarda de saída".into()),
    };
    let Ok((publica, auth)) = chaves_do_browser(&s.p256dh, &s.auth) else {
        return Resultado::Falhou("chaves da subscrição inválidas".into());
    };
    let Ok(uri) = s.endpoint.parse::<axum::http::Uri>() else {
        return Resultado::Falhou("endpoint não é um URI".into());
    };
    let corpo = aviso(room_code, &s.locale).to_string();
    let pedido = match WebPushBuilder::new(uri, publica, auth)
        .with_valid_duration(TTL)
        .with_vapid(&ch.par, &ch.contacto)
        .build(corpo)
    {
        Ok(p) => p,
        Err(e) => return Resultado::Falhou(format!("não se cifrou o aviso: {e}")),
    };
    let (partes, corpo) = pedido.into_parts();
    let mut rb = state.outbound.tenant().post(url);
    for (nome, valor) in partes.headers.iter() {
        rb = rb.header(nome.as_str(), valor.as_bytes());
    }
    // RFC 8030 §5.3: um toque é urgente — sem isto, um telemóvel em poupança
    // de bateria pode guardar o aviso para mais tarde.
    rb = rb.header("Urgency", "high");
    match rb.body(corpo).send().await {
        Ok(r) if r.status().is_success() => Resultado::Aceite,
        Ok(r) if r.status().as_u16() == 404 || r.status().as_u16() == 410 => Resultado::Expirada,
        Ok(r) => Resultado::Falhou(format!("o serviço de push respondeu {}", r.status())),
        Err(e) => Resultado::Falhou(format!("o pedido falhou: {e}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn so_servicos_de_push_conhecidos() {
        for bom in [
            "fcm.googleapis.com",
            "updates.push.services.mozilla.com",
            "wns2-par02p.notify.windows.com",
            "web.push.apple.com",
            "FCM.googleapis.com.",
        ] {
            assert!(host_de_push(bom), "{bom}");
        }
        for mau in [
            "xfcm.googleapis.com",
            "fcm.googleapis.com.evil.example",
            "push.apple.com.example",
            "intranet.cliente.ao",
            "169.254.169.254",
            "",
        ] {
            assert!(!host_de_push(mau), "{mau}");
        }
    }

    /// A decisão de DLP, como teste: o aviso não leva o assunto nem quem chama
    /// porque a função nem os recebe — e o que leva é só isto.
    #[test]
    fn o_aviso_so_leva_o_link_e_um_texto_generico() {
        let a = aviso("abc-defg-hij", "pt");
        let mut chaves: Vec<&str> = a.as_object().unwrap().keys().map(|k| k.as_str()).collect();
        chaves.sort();
        assert_eq!(chaves, ["body", "kind", "link", "room_code", "title"]);
        assert_eq!(a["link"], "/#/r/abc-defg-hij");
        assert_eq!(
            aviso("x", "en-GB")["body"],
            "You are being called into a meeting. Tap to join."
        );
        // Uma língua desconhecida cai no português, não num aviso vazio.
        assert_eq!(aviso("x", "de")["body"], aviso("x", "pt")["body"]);
    }

    #[test]
    fn chaves_do_browser_invalidas_sao_recusadas() {
        assert!(chaves_do_browser("lixo", "AAAAAAAAAAAAAAAAAAAAAA").is_err());
        // 16 bytes em base64url são 22 caracteres; 15 bytes não servem.
        let sk = web_push_native::p256::SecretKey::from_slice(&[7u8; 32]).unwrap();
        let p = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .encode(sk.public_key().to_sec1_bytes());
        assert!(chaves_do_browser(&p, "AAAAAAAAAAAAAAAAAAAAAA").is_ok());
        assert!(chaves_do_browser(&p, "AAAAAAAAAAAAAAAAAAAA").is_err());
    }
}
