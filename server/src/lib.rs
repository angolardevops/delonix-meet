//! Delonix Meet — biblioteca do servidor.
//!
//! O binário (`main.rs`) só chama [`run`]. Tudo o resto vive aqui para que os
//! testes de integração em `tests/` possam montar o router e o estado sem
//! arrancar um processo (ADR-0004 §6, passo 1).

mod actions;
mod ai;
mod apikeys;
mod audit;
mod auth;
mod broadcast;
pub mod config;
mod crypto;
mod dlp;
mod error;
pub mod grpc;
mod meetings;
mod meetings_v1;
mod metrics;
mod mfa;
mod mls;
pub mod net_guard;
pub mod nodes;
mod notifications;
mod odoo;
mod odoo_sso;
pub mod openapi;
mod org;
mod presence;
mod pubsub;
mod rate_limit;
mod recorder;
mod recordings;
mod redis_state;
mod room_chat;
mod room_tools;
mod rooms;
pub mod secrets_at_rest;
mod sfu;
#[cfg(test)]
mod sfu_e2e;
mod signaling;
mod sms;
mod sms_codec;
mod sms_smpp;
mod storage;
mod stream_destinations;
mod transcription;
mod ui;
mod usage;
mod users;
mod voice;
mod webhooks;
mod whiteboards;

/// A varredura da quarentena, exposta aos testes de integração sem abrir o
/// módulo inteiro (os handlers já não a chamam — ver `meetings::quarantine_sweep`).
pub use meetings::{quarantine_sweep, run_quarantine_sweeper};

use axum::{
    extract::DefaultBodyLimit,
    http::HeaderValue,
    middleware,
    response::{IntoResponse, Response},
    routing::{get, post},
    Router,
};
use sqlx::postgres::PgPoolOptions;
use std::{net::SocketAddr, sync::Arc, time::Duration};
use tower_http::{cors::CorsLayer, trace::TraceLayer};

/// Limite de corpo por omissão (endpoints JSON). Rotas de upload/quadros
/// sobrepõem este valor com o seu próprio limite.
const DEFAULT_BODY_LIMIT: usize = 1024 * 1024; // 1 MB
const WHITEBOARD_BODY_LIMIT: usize = 12 * 1024 * 1024; // PNG 8MB → base64 ~11MB

/// Cabeçalhos de segurança nas respostas da API (defesa-em-profundidade;
/// a CSP completa da SPA é definida no Nginx — ver deploy/nginx-delonix.conf).
async fn security_headers(req: axum::extract::Request, next: middleware::Next) -> Response {
    let mut res = next.run(req).await;
    let h = res.headers_mut();
    h.insert(
        "X-Content-Type-Options",
        HeaderValue::from_static("nosniff"),
    );
    h.insert("X-Frame-Options", HeaderValue::from_static("DENY"));
    h.insert("Referrer-Policy", HeaderValue::from_static("no-referrer"));
    h.insert(
        "Strict-Transport-Security",
        HeaderValue::from_static("max-age=31536000; includeSubDomains"),
    );
    res
}

use config::Config;
use rate_limit::RateLimiter;
use signaling::SignalingHub;

pub struct AppState {
    pub config: Config,
    /// O pod está a DRENAR (recebeu SIGTERM e vai fechar).
    ///
    /// Enquanto está a true: o `/ready` devolve 503 (o K8s tira o pod dos
    /// endpoints do Service, e as entradas NOVAS deixam de chegar aqui), as
    /// salas em curso continuam, e os participantes são avisados para
    /// migrarem. É a diferença entre uma actualização que ninguém nota e uma
    /// que derruba todas as reuniões do pod — que era o comportamento antes.
    pub draining: std::sync::atomic::AtomicBool,
    /// Instante de arranque (para o uptime da status page).
    pub started: std::time::Instant,
    pub db: sqlx::PgPool,
    pub hub: SignalingHub,
    pub sfu: Arc<sfu::SfuState>,
    /// Emissões em directo a decorrer neste pod (ADR-0003).
    pub directos: Arc<broadcast::Registo>,
    pub presence: presence::PresenceHub,
    pub auth_limiter: RateLimiter,
    /// Anti-brute-force por conta (email) no login.
    pub login_limiter: RateLimiter,
    /// Rate-limit da API pública v1: balde por chave `dlx_` válida, por IP
    /// sem ela (`rate_limit::v1_bucket`). Partilhado com `/api/ice-servers` (por IP).
    pub v1_limiter: RateLimiter,
    /// Anti-brute-force de PIN no dial-in PSTN (por DID). Só conta falhas.
    pub voice_pin_limiter: RateLimiter,
    /// Envios de SMS por organização (ADR-0005). Um SMS custa dinheiro: é o
    /// travão contra um admin comprometido ou um script descontrolado.
    pub sms_send_limiter: RateLimiter,
    /// Anti-força-bruta do código MFA na activação e na desactivação (por
    /// conta). Só conta falhas; trava também o código certo (R131).
    pub mfa_limiter: RateLimiter,
    /// Salas de grupo ativas: sala principal -> conjunto de salas filhas.
    pub breakouts: dashmap::DashMap<uuid::Uuid, signaling::BreakoutSet>,
    /// Clientes HTTP de saída, com a guarda anti-SSRF (ver `net_guard`). Os
    /// únicos do servidor: criados uma vez para reutilizar o pool TLS.
    pub outbound: net_guard::Outbound,
    pub redis_bus: Option<Arc<pubsub::PubSubBus>>,
    /// Contadores de observabilidade expostos em `/metrics` (ver metrics.rs).
    pub metrics: Arc<metrics::Metrics>,
}

impl AppState {
    /// Abre uma transação com o CONTEXTO DE TENANT do utilizador para as
    /// políticas Row-Level Security (RLS). Define `app.user_id` (LOCAL à
    /// transação) — as políticas filtram por `org_id IN (orgs do utilizador)`.
    /// FAIL-CLOSED: numa tabela com RLS FORCE, uma query fora deste contexto
    /// (sem `app.user_id`) devolve ZERO linhas em vez de vazar cross-org.
    /// Ver docs/adr/0002-tenant-isolation-rls.md e a migração 0024. As queries
    /// a tabelas com RLS TÊM de correr nesta `tx` (e no fim `tx.commit()`).
    pub async fn tenant_tx(
        &self,
        user_id: uuid::Uuid,
    ) -> Result<sqlx::Transaction<'_, sqlx::Postgres>, sqlx::Error> {
        let mut tx = self.db.begin().await?;
        // set_config(name, value, is_local=true) — parametrizado (sem injeção).
        sqlx::query("SELECT set_config('app.user_id', $1, true)")
            .bind(user_id.to_string())
            .execute(&mut *tx)
            .await?;
        Ok(tx)
    }
}

/// Rotas de máquina e de observação. Com `INTERNAL_BIND_ADDR` vivem SÓ no
/// listener interno (porta que nenhum ingress publica, ADR-0006 §3); sem ele
/// ficam no público, como sempre estiveram — uma instalação existente não perde
/// o IVR por actualizar o binário.
fn internal_routes() -> Router<Arc<AppState>> {
    Router::new()
        // `/metrics` — exposição Prometheus. Só contadores agregados, nenhum
        // dado de inquilino.
        .route("/metrics", get(metrics_handler))
        // API interna de IVR (autenticada por segredo partilhado, usada pela media)
        .route(
            "/internal/v1/voice/ivr/validate",
            post(voice::ivr_validate_pin),
        )
        .route("/internal/v1/voice/ivr/cdr", post(voice::ivr_record_cdr))
}

/// Router do listener INTERNO (`INTERNAL_BIND_ADDR`).
pub fn build_internal_router(state: Arc<AppState>) -> Router {
    internal_routes()
        .route("/health", get(|| async { "ok" }))
        .layer(DefaultBodyLimit::max(DEFAULT_BODY_LIMIT))
        .layer(middleware::from_fn(error::normalize_error_body))
        .layer(middleware::from_fn(request_id))
        .with_state(state)
}

pub fn build_router(state: Arc<AppState>) -> Router {
    // O mapa destas rotas (e de onde vieram) está em docs/reference/api-routes.md.
    // Uma superfície por público (ADR-0004 §4): BFF (sessão), v1 do inquilino
    // (chave `dlx_`), operador, integrações (Odoo `dlxo_`, agente SMS `dlxg_`) e
    // interna (listener interno).

    // ---- BFF: autenticação (com rate-limit por IP) ----
    let auth_routes = Router::new()
        .route("/register", post(auth::register))
        .route("/login", post(auth::login))
        // Segunda metade do login quando o MFA está activo: troca o desafio
        // de curta duração + o código pelos tokens de sessão.
        .route("/login/mfa", post(auth::mfa_login))
        .route("/refresh", post(auth::refresh))
        .route("/logout", post(auth::logout))
        // SSO / OIDC
        .route("/sso/discovery", get(auth::sso_check))
        .route("/sso/authorize", get(auth::sso_login))
        .route("/sso/callback", get(auth::sso_callback))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            rate_limit::auth_rate_limit,
        ));

    // ---- Pública do inquilino: /api/v1 (chave `dlx_` com escopos) ----
    let v1_routes = Router::new()
        .route("/organization", get(apikeys::v1_org))
        .route("/rooms", post(apikeys::v1_create_room))
        .route("/rooms/{room_code}", get(apikeys::v1_get_room))
        .route("/rooms/{room_code}/bots", post(apikeys::v1_join_bot_room))
        .route("/recordings", get(apikeys::v1_recordings))
        // O POST/PATCH/DELETE vive em `meetings_v1.rs`: cria a REUNIÃO (com
        // anfitrião humano e convidados), não só uma sala solta.
        .route(
            "/meetings",
            get(apikeys::v1_meetings).post(meetings_v1::create),
        )
        .route(
            "/meetings/{meeting_id}",
            get(meetings_v1::get_one)
                .patch(meetings_v1::patch)
                .delete(meetings_v1::delete),
        )
        // "Começar agora": faz tocar nos dispositivos dos convidados.
        .route("/meetings/{meeting_id}/ring", post(meetings_v1::ring))
        .route(
            "/meetings/{meeting_id}/minutes",
            get(apikeys::v1_meeting_notes),
        )
        .layer(middleware::from_fn_with_state(
            state.clone(),
            rate_limit::v1_rate_limit,
        ));

    // ---- Operador: quem opera a plataforma (fora do SDK do inquilino) ----
    let operator_routes = Router::new()
        // Provisão de org — segredo de plataforma (a org ainda não existe).
        .route("/organizations", post(apikeys::v1_provision_org))
        // Armazenamento remoto da plataforma: TrueNAS NFS / Nextcloud WebDAV.
        .route(
            "/storage",
            get(storage::get_storage).put(storage::save_storage),
        )
        .route("/storage/test", post(storage::test_storage))
        .route("/storage/pvc-manifest", get(storage::pvc_manifest))
        // Inventário de nós de media (G10).
        .route("/nodes", get(nodes::list))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            rate_limit::ip_rate_limit,
        ));

    // ---- Integração Odoo (módulo nk_delonix_meet, token `dlxo_`) ----
    let odoo_integration_routes = Router::new()
        .route("/provision", post(odoo::provision))
        .route("/users", get(odoo::list_users))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            rate_limit::ip_rate_limit,
        ));

    // ---- Agente de SMS por USB (token `dlxg_`, ADR-0005) ----
    let sms_agent_routes = Router::new()
        .route("/devices", axum::routing::put(sms::agent_put_devices))
        .route("/claim", post(sms::agent_claim))
        .route("/messages/{message_id}/result", post(sms::agent_result));

    Router::new()
        // LIVENESS: o processo está vivo? Responde `ok` mesmo a drenar — um
        // pod a drenar não deve ser REINICIADO, deve ser deixado terminar.
        .route("/health", get(|| async { "ok" }))
        // READINESS: pode receber tráfego NOVO? Enquanto drena, NÃO.
        .route("/ready", get(readiness))
        .route("/api/status", get(status))
        .route("/api/public/settings", get(odoo::public_settings))
        // Contratos OpenAPI gerados do código (ADR-0006 §3).
        .route("/api/openapi.json", get(openapi::bff_json))
        .route("/api/v1/openapi.json", get(openapi::v1_json))
        .route("/api/operator/v1/openapi.json", get(openapi::operator_json))
        .route("/api/integrations/openapi.json", get(openapi::integrations_json))
        .nest("/api/auth", auth_routes)
        .nest("/api/v1", v1_routes)
        .nest("/api/operator/v1", operator_routes)
        .nest("/api/integrations/odoo/v1", odoo_integration_routes)
        .nest("/api/integrations/sms-agent/v1", sms_agent_routes)
        // ══════════════════════════════════════════════════════════════════
        //  BFF — o web Delonix (sessão). Contrato instável, OpenAPI em
        //  /api/openapi.json. /api/mls NÃO está registado de propósito: os
        //  handlers eram stubs que respondiam «feito» a qualquer pessoa (R41).
        // ══════════════════════════════════════════════════════════════════
        // ---- Utilizador ----
        .route("/api/users", get(users::search))
        .route("/api/users/me", get(users::me).patch(users::update_me))
        // «A minha sala» (G2).
        .route(
            "/api/users/me/room",
            get(users::my_room).patch(users::update_my_room),
        )
        .route(
            "/api/users/me/room/rotate-code",
            post(users::rotate_my_room_code),
        )
        .route("/api/users/me/storage-usage", get(usage::my_storage_usage))
        // MFA (TOTP, RFC 6238).
        .route("/api/users/me/mfa", get(mfa::estado))
        .route("/api/users/me/mfa/enroll", post(mfa::inscrever))
        .route("/api/users/me/mfa/activate", post(mfa::activar))
        .route("/api/users/me/mfa/disable", post(mfa::desactivar))
        // Centro de notificações pessoal (G8).
        .route("/api/users/me/notifications", get(notifications::list))
        .route(
            "/api/users/me/notifications/mark-all-read",
            post(notifications::mark_all_read),
        )
        .route(
            "/api/users/me/notifications/{notification_id}",
            get(notifications::get_one)
                .patch(notifications::update)
                .delete(notifications::delete),
        )
        .route(
            "/api/users/me/missed-calls/acknowledge",
            post(presence::ack_missed_calls),
        )
        // ---- Salas ----
        .route("/api/rooms", post(rooms::create_room))
        .route("/api/rooms/{room_code}", get(rooms::get_room))
        // Estado vivo da emissão (G1) — rótulos, bytes, débito. Ver o /live
        // (WebSocket) mais abaixo, que é o que a alimenta.
        .route(
            "/api/rooms/{room_code}/live/status",
            get(broadcast::estado_directo),
        )
        .route("/api/rooms/{room_code}/join", post(rooms::join_room))
        .route("/api/rooms/{room_code}/messages", get(rooms::room_chat))
        .route("/api/rooms/{room_code}/invitations", post(rooms::invite_to_room))
        .route("/api/rooms/{room_code}/quality-samples", post(rooms::post_qos))
        // Tempos de estabelecimento (um por sessão) — ver callTimings.ts.
        .route("/api/rooms/{room_code}/join-timings", post(rooms::post_timings))
        .route(
            "/api/rooms/{room_code}/minutes",
            get(meetings::notes_by_room).put(meetings::save_minutes_by_room),
        )
        .route(
            "/api/rooms/{room_code}/recordings",
            get(recordings::list)
                .post(recordings::upload)
                // Só o upload de gravações pode ser grande.
                .layer(DefaultBodyLimit::max(recordings::MAX_RECORDING_BYTES)),
        )
        // /api/ice-servers devolve credenciais TURN de curta duração —
        // rate-limit só por IP, para uma conta não esgotar o relay do coturn.
        .route(
            "/api/ice-servers",
            get(rooms::ice_servers).layer(middleware::from_fn_with_state(
                state.clone(),
                rate_limit::ip_rate_limit,
            )),
        )
        // Tradução de legendas em tempo real via LLM local (ai.rs / Ollama).
        .route("/api/ai/translations", post(ai::translate_caption))
        // ---- Reuniões, agenda e plano de acção 5W2H ----
        .route("/api/meetings", get(meetings::list).post(meetings::create))
        .route("/api/meetings/check-conflicts", post(meetings::check_conflicts))
        .route(
            "/api/meetings/{meeting_id}",
            get(meetings::get_one).delete(meetings::delete),
        )
        .route("/api/meetings/{meeting_id}/start", post(meetings::start))
        .route("/api/meetings/{meeting_id}/calendar.ics", get(meetings::ics))
        .route("/api/meetings/{meeting_id}/minutes", axum::routing::put(meetings::save_minutes))
        .route("/api/meetings/{meeting_id}/invitees", get(meetings::invitees))
        .route("/api/meetings/{meeting_id}/invitees/me", axum::routing::put(meetings::respond))
        .route(
            "/api/meetings/{meeting_id}/agenda-items",
            get(actions::list_agenda).post(actions::add_agenda_item),
        )
        .route(
            "/api/meetings/{meeting_id}/agenda-items/{item_id}",
            axum::routing::patch(actions::patch_agenda_item)
                .delete(actions::delete_agenda_item),
        )
        .route(
            "/api/meetings/{meeting_id}/action-plan",
            get(actions::get_action_plan).put(actions::upsert_action_plan),
        )
        .route(
            "/api/meetings/{meeting_id}/action-plan/items",
            post(actions::add_action_item),
        )
        .route(
            "/api/meetings/{meeting_id}/action-plan/items/{item_id}",
            axum::routing::patch(actions::patch_action_item)
                .delete(actions::delete_action_item),
        )
        // ---- Gravações ----
        .route("/api/recordings", get(recordings::library))
        .route(
            "/api/recordings/{recording_id}",
            get(recordings::get_metadata).patch(recordings::update),
        )
        .route("/api/recordings/{recording_id}/content", get(recordings::download))
        .route(
            "/api/recordings/{recording_id}/chapters",
            get(recordings::list_chapters).post(recordings::create_chapter),
        )
        .route(
            "/api/recordings/{recording_id}/chapters/{chapter_id}",
            get(recordings::get_chapter).delete(recordings::delete_chapter),
        )
        .route(
            "/api/recordings/{recording_id}/comments",
            get(recordings::list_comments).post(recordings::create_comment),
        )
        .route(
            "/api/recordings/{recording_id}/comments/{comment_id}",
            get(recordings::get_comment)
                .patch(recordings::update_comment)
                .delete(recordings::delete_comment),
        )
        .route(
            "/api/recordings/{recording_id}/shares",
            get(recordings::shares).post(recordings::share),
        )
        .route(
            "/api/recordings/{recording_id}/shares/{user_id}",
            axum::routing::delete(recordings::unshare),
        )
        .route(
            "/api/recordings/{recording_id}/public-link",
            get(recordings::get_link)
                .put(recordings::create_link)
                .delete(recordings::revoke_link),
        )
        .route("/api/public/recordings/{token}", get(recordings::public_share))
        .route(
            "/api/public/recordings/{token}/content",
            get(recordings::public_share_download),
        )
        // ---- Quadros ----
        .route(
            "/api/whiteboards",
            get(whiteboards::list)
                .post(whiteboards::save)
                .layer(DefaultBodyLimit::max(WHITEBOARD_BODY_LIMIT)),
        )
        .route(
            "/api/whiteboards/{whiteboard_id}",
            get(whiteboards::get_one).delete(whiteboards::delete),
        )
        .route("/api/whiteboards/{whiteboard_id}/image", get(whiteboards::png))
        // URL assinado da imagem (G11): o `<img>` carrega-o sem sessão.
        .route(
            "/api/whiteboards/{whiteboard_id}/signed-url",
            post(whiteboards::signed_url),
        )
        .route(
            "/api/whiteboards/{whiteboard_id}/public-link",
            axum::routing::put(whiteboards::set_share),
        )
        .route(
            "/api/public/whiteboards/{token}/image",
            get(whiteboards::shared_png),
        )
        // ---- Organizações ----
        .route("/api/orgs", get(org::my_orgs).post(org::create_org))
        .route(
            "/api/orgs/{org_id}",
            get(org::get_org).patch(org::update_settings),
        )
        .route("/api/orgs/{org_id}/stats", get(org::org_stats))
        .route(
            "/api/orgs/{org_id}/analytics/quarantine",
            get(meetings::quarantine_analytics),
        )
        .route("/api/orgs/{org_id}/branches", get(org::list_branches).post(org::create_branch))
        .route("/api/orgs/{org_id}/members", get(org::list_employees).post(org::add_employee))
        .route(
            "/api/orgs/{org_id}/members/{user_id}",
            axum::routing::patch(org::update_employee).delete(org::remove_employee),
        )
        .route("/api/orgs/{org_id}/groups", get(org::list_groups).post(org::create_group))
        .route(
            "/api/orgs/{org_id}/meeting-rooms",
            get(org::list_meeting_rooms).post(org::create_meeting_room),
        )
        .route("/api/orgs/{org_id}/audit-events", get(audit::list))
        // Verificação da cadeia de hash: diz se alguém mexeu na trilha.
        .route(
            "/api/orgs/{org_id}/audit-events/verification",
            get(audit::verify),
        )
        .route(
            "/api/orgs/{org_id}/sso",
            get(org::get_sso_config)
                .put(org::upsert_sso_config)
                .delete(org::delete_sso_config),
        )
        .route("/api/orgs/{org_id}/storage-usage", get(usage::org_storage_usage))
        .route("/api/orgs/{org_id}/api-keys", get(apikeys::list).post(apikeys::create))
        .route(
            "/api/orgs/{org_id}/api-keys/{key_id}",
            axum::routing::delete(apikeys::revoke),
        )
        .route("/api/orgs/{org_id}/webhooks", get(webhooks::list).post(webhooks::create))
        .route(
            "/api/orgs/{org_id}/webhooks/{hook_id}",
            get(webhooks::get_one).delete(webhooks::delete),
        )
        .route(
            "/api/orgs/{org_id}/webhooks/{hook_id}/deliveries",
            get(webhooks::list_deliveries),
        )
        .route(
            "/api/orgs/{org_id}/webhooks/{hook_id}/deliveries/{delivery_id}",
            get(webhooks::get_delivery),
        )
        .route(
            "/api/orgs/{org_id}/webhooks/{hook_id}/deliveries/{delivery_id}/redeliver",
            post(webhooks::redeliver),
        )
        .route(
            "/api/orgs/{org_id}/stream-destinations",
            get(stream_destinations::list).post(stream_destinations::create),
        )
        .route(
            "/api/orgs/{org_id}/stream-destinations/{dest_id}",
            get(stream_destinations::get_one)
                .patch(stream_destinations::update)
                .delete(stream_destinations::delete),
        )
        .route(
            "/api/orgs/{org_id}/stream-destinations/{dest_id}/rotate-key",
            post(stream_destinations::rotate_key),
        )
        .route(
            "/api/orgs/{org_id}/integrations/odoo",
            get(odoo::get_config).put(odoo::save_config),
        )
        .route(
            "/api/orgs/{org_id}/integrations/odoo/rotate-token",
            post(odoo::rotate_token),
        )
        // Dial-in PSTN (plano de controlo).
        .route("/api/orgs/{org_id}/voice/rooms", post(voice::create_room))
        .route(
            "/api/orgs/{org_id}/voice/rooms/{voice_room_id}",
            get(voice::get_room),
        )
        .route(
            "/api/orgs/{org_id}/voice/rooms/{voice_room_id}/participants",
            get(voice::list_participants),
        )
        .route(
            "/api/orgs/{org_id}/voice/rooms/{voice_room_id}/close",
            post(voice::close_room),
        )
        .route("/api/orgs/{org_id}/voice/dids", get(voice::list_dids).post(voice::create_did))
        .route("/api/orgs/{org_id}/voice/call-records", get(voice::list_cdr))
        .route("/api/orgs/{org_id}/voice/billing", get(voice::billing_summary))
        // Gateway de SMS (ADR-0005): consola da org (sessão, admin).
        .route("/api/orgs/{org_id}/sms/gateways", get(sms::list_gateways).post(sms::create_gateway))
        .route(
            "/api/orgs/{org_id}/sms/gateways/{gateway_id}",
            axum::routing::delete(sms::revoke_gateway),
        )
        .route("/api/orgs/{org_id}/sms/devices", get(sms::list_devices))
        .route("/api/orgs/{org_id}/sms/route", get(sms::get_route).put(sms::put_route))
        .route("/api/orgs/{org_id}/sms/messages", get(sms::list_messages).post(sms::send_message))
        .route(
            "/api/orgs/{org_id}/sms/messages/{message_id}",
            get(sms::get_message),
        )
        // ---- Tempo real (WebSocket) ----
        .route("/ws", get(signaling::ws_handler))
        // Directo: o browser empurra a emissão já composta e codificada, e o
        // servidor remultiplexa para RTMP (ADR-0003). Autenticada pelo token de
        // sala na query, como o /ws — um WebSocket não leva cabeçalhos nossos.
        .route("/api/rooms/{room_code}/live", get(broadcast::ws_directo))
        .route("/rtc", get(presence::rtc_handler))
        .merge(if state.config.internal_bind_addr.is_none() {
            internal_routes()
        } else {
            Router::new()
        })
        .fallback({
            let ui_dir = state.config.ui_dir.clone();
            move |req: axum::extract::Request| async move {
                match ui_dir {
                    Some(dir) => ui::serve(dir, req).await,
                    None => axum::http::StatusCode::NOT_FOUND.into_response(),
                }
            }
        })
        .layer(DefaultBodyLimit::max(DEFAULT_BODY_LIMIT))
        .layer(middleware::from_fn(security_headers))
        .layer(build_cors(&state))
        // Só regista método + caminho (NUNCA a query — evita gravar os JWT que
        // vão em ?token= nas ligações WebSocket).
        .layer(TraceLayer::new_for_http().make_span_with(
            |req: &axum::http::Request<axum::body::Body>| {
                let request_id = req
                    .headers()
                    .get(REQUEST_ID_HEADER)
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or("");
                tracing::info_span!("http", method = %req.method(), path = %req.uri().path(), request_id)
            },
        ))
        // Por DENTRO do request_id (o envelope leva o id) e por fora de tudo o
        // resto: apanha também as recusas dos extractores do axum.
        .layer(middleware::from_fn(error::normalize_error_body))
        // Por FORA do trace: o id tem de existir quando o span nasce.
        .layer(middleware::from_fn(request_id))
        .with_state(state)
}

const REQUEST_ID_HEADER: &str = "x-request-id";

/// Dá a cada pedido um identificador: aceita o `X-Request-Id` do proxy (se for
/// curto e só com caracteres seguros — vai parar aos logs) ou gera um. O mesmo
/// id sai no cabeçalho da resposta, no span de log e no envelope de erro.
async fn request_id(mut req: axum::extract::Request, next: middleware::Next) -> Response {
    let id = req
        .headers()
        .get(REQUEST_ID_HEADER)
        .and_then(|v| v.to_str().ok())
        .filter(|v| {
            !v.is_empty()
                && v.len() <= 64
                && v.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
        })
        .map(str::to_string)
        .unwrap_or_else(|| uuid::Uuid::new_v4().simple().to_string());
    if let Ok(v) = HeaderValue::from_str(&id) {
        req.headers_mut().insert(REQUEST_ID_HEADER, v.clone());
        let mut res = error::REQUEST_ID.scope(id, next.run(req)).await;
        res.headers_mut().insert(REQUEST_ID_HEADER, v);
        return res;
    }
    next.run(req).await
}

/// CORS por allowlist (`CORS_ORIGINS`). Sem origens configuradas: só same-origin
/// em produção; permissive apenas quando DELONIX_ALLOW_INSECURE=1 (dev).
fn build_cors(state: &Arc<AppState>) -> CorsLayer {
    use tower_http::cors::Any;
    if !state.config.cors_origins.is_empty() {
        let origins: Vec<HeaderValue> = state
            .config
            .cors_origins
            .iter()
            .filter_map(|o| o.parse().ok())
            .collect();
        CorsLayer::new()
            .allow_origin(origins)
            .allow_methods(Any)
            .allow_headers(Any)
    } else if state.config.allow_insecure {
        CorsLayer::permissive()
    } else {
        CorsLayer::new() // same-origin: o Nginx serve app e API no mesmo host
    }
}

/// Status público (roadmap "status page"): saúde dos componentes + uptime.
/// `/metrics` — exposição Prometheus (ver metrics.rs). Sem autenticação: só
/// contadores de saúde agregados, nenhum dado de tenant. Em produção, restringir
/// o acesso ao scraper via NetworkPolicy/ingress interno.
async fn metrics_handler(
    axum::extract::State(state): axum::extract::State<Arc<AppState>>,
) -> impl axum::response::IntoResponse {
    let mut body = state.metrics.render(state.started.elapsed().as_secs());
    body.push_str(&state.sfu.census().await.render());
    (
        [(
            axum::http::header::CONTENT_TYPE,
            "text/plain; version=0.0.4; charset=utf-8",
        )],
        body,
    )
}

/// Estado público da instalação.
#[derive(serde::Serialize, utoipa::ToSchema)]
pub struct StatusResp {
    /// `ok` | `degraded` (base inacessível).
    pub status: &'static str,
    pub api: bool,
    pub db: bool,
    pub uptime_secs: u64,
    pub version: &'static str,
}

/// Sem autenticação — não expõe dados, só disponibilidade.
#[utoipa::path(
    get, path = "/api/status", tag = "platform",
    responses((status = 200, body = StatusResp))
)]
pub(crate) async fn status(
    axum::extract::State(state): axum::extract::State<Arc<AppState>>,
) -> axum::Json<StatusResp> {
    let db_ok = sqlx::query("SELECT 1").execute(&state.db).await.is_ok();
    axum::Json(StatusResp {
        status: if db_ok { "ok" } else { "degraded" },
        api: true,
        db: db_ok,
        uptime_secs: state.started.elapsed().as_secs(),
        version: env!("CARGO_PKG_VERSION"),
    })
}

/// Monta o estado partilhado a partir da configuração e de uma pool já
/// migrada: clientes de saída, barramento Redis (opcional), hubs de
/// sinalização e presença, e os subscritores entre nós. Não arranca
/// listeners nem tarefas de fundo — é o que os testes de integração usam
/// para montar o router sobre uma base real.
pub async fn build_state(config: Config, db: sqlx::PgPool) -> Arc<AppState> {
    // O host do Odoo da plataforma foi declarado pelo operador ao configurá-lo:
    // as orgs criadas a partir dele guardam-no como `odoo_url`, e o login delas
    // passa pelo cliente de inquilino.
    let mut allow_hosts = config.outbound_allow_hosts.clone();
    if let Some(host) = config
        .platform_odoo_url
        .as_deref()
        .and_then(|u| reqwest::Url::parse(u).ok())
        .and_then(|u| u.host_str().map(str::to_string))
    {
        allow_hosts.push(host);
    }
    let outbound = net_guard::Outbound::new(allow_hosts);

    // Redis pub/sub: opcional — só ativo se REDIS_URL estiver definido.
    let redis_bus = if let Some(url) = &config.redis_url {
        match pubsub::PubSubBus::connect(url).await {
            Ok(bus) => {
                tracing::info!("Redis pub/sub ativo — modo multi-nó HA");
                Some(bus)
            }
            Err(e) => {
                tracing::warn!("Redis indisponível ({e}) — a arrancar em modo single-node");
                None
            }
        }
    } else {
        tracing::info!("REDIS_URL não definido — modo single-node");
        None
    };

    let mut presence_hub = presence::PresenceHub::default();
    presence_hub.bus = redis_bus.clone();

    let metrics = Arc::new(metrics::Metrics::default());
    let mut hub = SignalingHub::default();
    hub.bus = redis_bus.clone();
    // Chat persistido fora do caminho quente (fila limitada + tarefa própria).
    hub.chat_store = Some(room_chat::spawn_writer(db.clone(), metrics.clone()));
    let state = Arc::new(AppState {
        draining: std::sync::atomic::AtomicBool::new(false),
        started: std::time::Instant::now(),
        db,
        hub,
        breakouts: dashmap::DashMap::new(),
        directos: Arc::new(broadcast::Registo::default()),
        sfu: Arc::new(sfu::SfuState::new(
            sfu::IceConfig {
                external_ip: config.sfu_external_ip.clone(),
                udp_ports: Some((config.sfu_udp_min, config.sfu_udp_max)),
                turn_host: config.turn_host.clone(),
                turn_secret: config.turn_secret.clone(),
                force_relay: config.force_turn_relay,
                ice_timeouts: None,
            },
            metrics.clone(),
            config.nego_queue_cap,
            config.rec_queue_cap,
        )),
        presence: presence_hub,
        auth_limiter: RateLimiter::new(config.auth_rate_per_min as u32, Duration::from_secs(60)),
        login_limiter: RateLimiter::new(8, Duration::from_secs(300)),
        v1_limiter: RateLimiter::new(120, Duration::from_secs(60)),
        voice_pin_limiter: RateLimiter::new(10, Duration::from_secs(300)),
        sms_send_limiter: RateLimiter::new(30, Duration::from_secs(60)),
        mfa_limiter: RateLimiter::new(5, Duration::from_secs(300)),
        outbound,
        config: config.clone(),
        redis_bus: redis_bus.clone(),
        metrics,
    });

    // Subscriber Redis: ouve mensagens de outros nós e entrega localmente.
    if let Some(bus) = redis_bus {
        let state_ref = Arc::downgrade(&state);
        pubsub::start_subscriber(bus.clone(), move |user_id, msg| {
            if let Some(s) = state_ref.upgrade() {
                s.presence.send_to_user(user_id, msg);
            }
        });

        let state_ref2 = Arc::downgrade(&state);
        pubsub::start_signaling_subscriber(bus, move |room_id, event| {
            if let Some(s) = state_ref2.upgrade() {
                match event {
                    pubsub::RedisRoomEvent::Broadcast { node_id, from, msg } => {
                        if node_id != *pubsub::NODE_ID {
                            s.hub.broadcast_local(room_id, from, msg);
                        }
                    }
                    pubsub::RedisRoomEvent::SendTo { node_id, to, msg } => {
                        if node_id != *pubsub::NODE_ID && s.hub.has_peer(room_id, to) {
                            s.hub.send_to_local(room_id, to, msg);
                        }
                    }
                    pubsub::RedisRoomEvent::BroadcastAll { node_id, msg } => {
                        if node_id != *pubsub::NODE_ID {
                            s.hub.broadcast_all_local(room_id, msg);
                        }
                    }
                    pubsub::RedisRoomEvent::BroadcastHosts { node_id, msg } => {
                        if node_id != *pubsub::NODE_ID {
                            s.hub.broadcast_hosts_local(room_id, msg);
                        }
                    }
                    pubsub::RedisRoomEvent::BroadcastAdmitters { node_id, msg } => {
                        if node_id != *pubsub::NODE_ID {
                            s.hub.broadcast_admitters_local(room_id, msg);
                        }
                    }
                    pubsub::RedisRoomEvent::BroadcastNonHosts { node_id, msg } => {
                        if node_id != *pubsub::NODE_ID {
                            s.hub.broadcast_non_hosts_local(room_id, msg);
                        }
                    }
                }
            }
        });
    }

    state
}

/// Logs em texto (desenvolvimento) ou JSON (`LOG_FORMAT=json`, K8s/Loki).
fn init_tracing(json: bool) {
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| "delonix_server=info,tower_http=info".into());
    if json {
        tracing_subscriber::fmt()
            .json()
            .with_current_span(true)
            .with_env_filter(filter)
            .init();
    } else {
        tracing_subscriber::fmt().with_env_filter(filter).init();
    }
}

/// Arranca o servidor: configuração, base, estado partilhado, tarefas de
/// fundo e o listener HTTP, até ao fim do drain.
pub async fn run() {
    // `delonix-server openapi bff|v1`: imprime o spec e sai. Não precisa de
    // configuração nem de base — é o que o portão e o gerador do cliente usam.
    let mut args = std::env::args().skip(1);
    if args.next().as_deref() == Some("openapi") {
        let doc = match args.next().as_deref() {
            Some("v1") => openapi::v1(),
            Some("operator") => openapi::operator(),
            Some("integrations") => openapi::integrations(),
            _ => openapi::bff(),
        };
        print!("{}", openapi::to_pretty(&doc));
        return;
    }
    let config = Config::from_env();
    init_tracing(config.log_json);
    // Avisado DEPOIS de os logs existirem — antes perdia-se sem ninguém ver.
    if config.allow_insecure {
        tracing::warn!(
            "DELONIX_ALLOW_INSECURE=1 — a usar segredos de desenvolvimento. NÃO usar em produção."
        );
    }
    tracing::info!(
        edition = ?config.edition,
        registration = ?config.registration_mode,
        tenancy = ?config.tenancy_mode,
        "perfil da instalação"
    );

    // `delonix-server migrate`: corre as migrações e sai. É o que o Job de
    // migração do SaaS executa antes do rollout (ADR-0006 §4).
    let migrate_only = std::env::args().nth(1).as_deref() == Some("migrate");

    let db = PgPoolOptions::new()
        .max_connections(10)
        .acquire_timeout(Duration::from_secs(5))
        .connect(&config.database_url)
        .await
        .expect("failed to connect to Postgres — is `docker compose up -d postgres` running?");

    if migrate_only || config.migrate_on_start {
        sqlx::migrate!("./migrations")
            .run(&db)
            .await
            .expect("migrations failed");
        tracing::info!("migrações aplicadas");
    }
    if migrate_only {
        return;
    }

    let state = build_state(config.clone(), db).await;

    // Cron: lugares reservados que passaram da janela viram saídas a sério
    // (R91). O intervalo é uma fracção da janela para o atraso máximo ser
    // pequeno face a ela — com 45 s de janela e 5 s de passo, um lugar sai no
    // máximo 5 s depois de expirar.
    //
    // Porque é um varredor e não um `sleep` na tarefa do socket: um `sleep`
    // prenderia a tarefa durante a janela inteira, e uma sala com muita
    // rotação acumularia tarefas adormecidas sem tecto nenhum.
    {
        let state = state.clone();
        tokio::spawn(async move {
            let janela = state.config.reconnect_grace();
            let mut ticker = tokio::time::interval(Duration::from_secs(5));
            ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            loop {
                ticker.tick().await;
                let n = state.hub.expire_disconnected(janela);
                if n > 0 {
                    state
                        .metrics
                        .seats_expired_total
                        .fetch_add(n as u64, std::sync::atomic::Ordering::Relaxed);
                    tracing::info!(expirados = n, "reserved seats expired");
                }
            }
        });
    }

    // Cron: sweep de quarentena a cada 5 min (marca não-respondentes de
    // reuniões já começadas). Idempotente. Nenhum handler varre a base inteira:
    // a analítica varre só a sua org e o seu período. Pára no shutdown.
    let quarantine_stop = tokio_util::sync::CancellationToken::new();
    let quarantine_sweeper = tokio::spawn(meetings::run_quarantine_sweeper(
        state.db.clone(),
        Duration::from_secs(300),
        quarantine_stop.clone(),
    ));

    // Cron: retenção de gravações (DLP-lite) a cada hora — apaga as que
    // passaram do prazo configurado por organização.
    {
        let state = state.clone();
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(Duration::from_secs(3600));
            loop {
                ticker.tick().await;
                recorder::retention_sweep(&state).await;
            }
        });
    }

    // Cron: extensão do horizonte de recorrência — gera instâncias para os
    // próximos 6 meses para todas as reuniões recorrentes ainda ativas.
    {
        let state = state.clone();
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(Duration::from_secs(86400)); // diário
            loop {
                ticker.tick().await;
                meetings::extend_recurrence_horizon(&state.db).await;
            }
        });
    }

    // Cron: auto-ring — a cada minuto verifica reuniões que começam agora
    // e chama os convidados que ainda não estão na sala.
    {
        let state = state.clone();
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(Duration::from_secs(60));
            ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                ticker.tick().await;
                meetings::ring_upcoming_meetings(&state).await;
            }
        });
    }

    // Cron: retenção do chat da sala (G9) — até ao fim do dia UTC da última
    // mensagem da sala, a promessa da migração 0018.
    {
        let db = state.db.clone();
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(Duration::from_secs(3600));
            ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                ticker.tick().await;
                match room_chat::retention_sweep(&db).await {
                    Ok(n) if n > 0 => tracing::info!(apagadas = n, "chat retention sweep"),
                    Ok(_) => {}
                    Err(e) => tracing::warn!(error = %e, "chat retention sweep failed"),
                }
            }
        });
    }

    // Cron: registo de entregas de webhooks (G7) a cada hora — fecha as
    // `pending` abandonadas por um processo que morreu e apaga as que passaram
    // da retenção (30 dias).
    {
        let db = state.db.clone();
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(Duration::from_secs(3600));
            ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                ticker.tick().await;
                match webhooks::sweep_deliveries(&db).await {
                    Ok((0, 0)) => {}
                    Ok((abandoned, deleted)) => {
                        tracing::info!(abandoned, deleted, "webhook deliveries sweep")
                    }
                    Err(e) => tracing::warn!(error = %e, "webhook deliveries sweep failed"),
                }
            }
        });
    }

    // Cron: segredos de integração herdados em claro (S5) — no arranque (o
    // primeiro `tick` é imediato) e de hora a hora: com DATA_ENCRYPTION_KEYS
    // cifra-os; sem chaves, avisa quantos continuam em claro. A hora apanha
    // escritas em claro que ainda não passem por `secrets_at_rest`.
    {
        let state = state.clone();
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(Duration::from_secs(3600));
            ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                ticker.tick().await;
                secrets_at_rest::reseal_pass(&state.db, &state.config).await;
            }
        });
    }

    // Cron: retenção das notificações (G8) a cada 6 h — lidas com mais de 90
    // dias, todas com mais de 180 (regra em `domain::notification`).
    {
        let db = state.db.clone();
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(Duration::from_secs(6 * 3600));
            ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                ticker.tick().await;
                match notifications::retention_sweep(&db).await {
                    Ok(n) if n > 0 => tracing::info!(deleted = n, "retenção de notificações"),
                    Ok(_) => {}
                    Err(e) => tracing::warn!(error = %e, "retenção de notificações falhou"),
                }
            }
        });
    }

    // Batimento deste nó para o inventário do operador (G10), e limpeza dos
    // nós esquecidos. O primeiro batimento sai já: um pod novo aparece antes
    // de receber a primeira sala.
    {
        let state = state.clone();
        let started_at = chrono::Utc::now();
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(Duration::from_secs(
                delonix_meet_domain::operations::media_node::HEARTBEAT_SECS as u64,
            ));
            ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            let mut n: u64 = 0;
            loop {
                ticker.tick().await;
                if let Err(e) = nodes::heartbeat(&state, started_at).await {
                    tracing::warn!(error = %e, "batimento do nó falhou");
                }
                n += 1;
                if n.is_multiple_of(240) {
                    let _ = nodes::forget_old(&state.db).await;
                }
            }
        });
    }

    // Gateway de SMS: envio pelos operadores e varrimento das mensagens paradas.
    sms::spawn_worker(state.clone());

    if let Some(addr) = config.internal_bind_addr.clone() {
        let internal = build_internal_router(state.clone());
        let listener = tokio::net::TcpListener::bind(&addr)
            .await
            .expect("failed to bind INTERNAL_BIND_ADDR");
        tracing::info!("listener interno (IVR, métricas) em {addr}");
        tokio::spawn(async move {
            if let Err(e) = axum::serve(
                listener,
                internal.into_make_service_with_connect_info::<SocketAddr>(),
            )
            .await
            {
                tracing::error!(error = %e, "listener interno terminou");
            }
        });
    }

    if let Some(addr) = config.grpc_bind_addr.clone() {
        let listener = tokio::net::TcpListener::bind(&addr)
            .await
            .expect("failed to bind GRPC_BIND_ADDR");
        tracing::info!("gRPC interno (IVR, transcrição) em {addr}");
        let grpc_state = state.clone();
        tokio::spawn(async move {
            // Pára com o sinal de shutdown; as chamadas em curso terminam.
            if let Err(e) = grpc::serve(grpc_state, listener, shutdown_signal()).await {
                // Configuração inválida (mTLS em falta) não pode ficar em silêncio.
                tracing::error!(error = %e, "gRPC interno não arrancou");
                std::process::exit(2);
            }
        });
    }

    let app = build_router(state.clone());
    let listener = tokio::net::TcpListener::bind(&config.bind_addr)
        .await
        .expect("failed to bind");
    tracing::info!("Delonix Meet server listening on {}", config.bind_addr);
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown({
        let state = state.clone();
        async move {
            shutdown_signal().await;
            quarantine_stop.cancel();
            drenar(state).await;
        }
    })
    .await
    .unwrap();
    // Uma passagem em curso acaba; nenhuma nova começa.
    let _ = quarantine_sweeper.await;
}

/// Espera SIGTERM (K8s rollout/drain) ou Ctrl-C. Quando dispara, o axum PÁRA de
/// aceitar novas ligações e deixa os handlers HTTP em curso terminar dentro do
/// `terminationGracePeriodSeconds` (45s, ver 02-server.yaml). O endpoint do pod
/// já foi removido do Service, portanto não chegam novos WS ao pod a terminar;
/// os WS existentes correm até ao fim da graça e o cliente reconecta (reload em
/// Room.tsx). NOTA: o drain PROATIVO dos WS (difundir "server-shutdown" para os
/// clientes fecharem já, em vez de esperar a graça) fica deferido de propósito —
/// mexeria no loop de inbound do signaling (território das regressões R1/R2) e
/// exige teste dedicado de 2 browsers.
/// Readiness: 200 enquanto aceita tráfego novo, 503 enquanto drena.
async fn readiness(
    axum::extract::State(state): axum::extract::State<Arc<AppState>>,
) -> impl axum::response::IntoResponse {
    if state.draining.load(std::sync::atomic::Ordering::Relaxed) {
        (axum::http::StatusCode::SERVICE_UNAVAILABLE, "draining")
    } else {
        (axum::http::StatusCode::OK, "ready")
    }
}

async fn shutdown_signal() {
    use tokio::signal;
    let ctrl_c = async {
        let _ = signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let terminate = async {
        match signal::unix::signal(signal::unix::SignalKind::terminate()) {
            Ok(mut s) => {
                s.recv().await;
            }
            Err(e) => tracing::error!("falha a instalar handler de SIGTERM: {e}"),
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();
    tokio::select! {
        _ = ctrl_c => {}
        _ = terminate => {}
    }
    tracing::info!("sinal de shutdown recebido — a drenar");
}

/// Drena o pod: pára de aceitar tráfego novo, avisa quem está em chamada, e
/// espera que as salas esvaziem antes de deixar o servidor fechar.
///
/// Antes desta função, o SIGTERM só fazia o axum parar de ACEITAR ligações. As
/// WebSockets em curso não fecham sozinhas, por isso o processo ficava a
/// aguardá-las até o K8s mandar SIGKILL ao fim do `terminationGracePeriod` — e
/// aí todas as reuniões daquele pod caíam de uma vez. Com a afinidade por sala
/// (ADR-0001) a concentrar salas no mesmo pod, isso é muita gente ao mesmo
/// tempo.
async fn drenar(state: Arc<AppState>) {
    use std::sync::atomic::Ordering::Relaxed;
    state.draining.store(true, Relaxed);

    // A readiness passa a falhar; o K8s tira o pod dos endpoints. Só depois
    // disso é que avisar os clientes serve de alguma coisa: se avisássemos
    // primeiro, eles reconectavam e o balanceador mandava-os de volta para
    // aqui.
    let espera_readiness = std::time::Duration::from_secs(state.config.drain_readiness_secs);
    tracing::info!(
        segundos = espera_readiness.as_secs(),
        "drain: readiness em 503 — a aguardar que o balanceador retire este pod"
    );
    tokio::time::sleep(espera_readiness).await;

    // Avisa TODA a gente em chamada. O cliente reconecta depois do atraso que
    // vai na mensagem, e como este pod já não está nos endpoints, o hash por
    // sala manda a sala INTEIRA para o mesmo pod novo — que é o que permite a
    // migração sem partir o SFU, que é in-memory por pod.
    let salas = state
        .hub
        .broadcast_draining(state.config.drain_reconnect_ms);
    tracing::info!(salas, "drain: participantes avisados para migrar");

    // Espera que esvaziem. Sai mal a última saia — não se gasta o orçamento
    // todo por hábito.
    let limite = std::time::Duration::from_secs(state.config.drain_grace_secs);
    let inicio = std::time::Instant::now();
    while inicio.elapsed() < limite {
        let restantes = state.hub.peers_ligados();
        if restantes == 0 {
            tracing::info!(
                segundos = inicio.elapsed().as_secs(),
                "drain: todas as salas esvaziaram"
            );
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }
    tracing::warn!(
        restantes = state.hub.peers_ligados(),
        segundos = limite.as_secs(),
        "drain: prazo esgotado — a fechar com participantes ainda ligados"
    );
}
