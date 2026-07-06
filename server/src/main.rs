mod auth;
mod config;
mod error;
mod meetings;
mod org;
mod presence;
mod rate_limit;
mod recordings;
mod rooms;
mod sfu;
mod signaling;
mod users;

use axum::{
    middleware,
    routing::{get, post},
    Router,
};
use sqlx::postgres::PgPoolOptions;
use std::{net::SocketAddr, sync::Arc, time::Duration};
use tower_http::{cors::CorsLayer, trace::TraceLayer};

use config::Config;
use rate_limit::RateLimiter;
use signaling::SignalingHub;

pub struct AppState {
    pub config: Config,
    pub db: sqlx::PgPool,
    pub hub: SignalingHub,
    pub sfu: Arc<sfu::SfuState>,
    pub presence: presence::PresenceHub,
    pub auth_limiter: RateLimiter,
    /// Salas de grupo ativas: sala principal -> conjunto de salas filhas.
    pub breakouts: dashmap::DashMap<uuid::Uuid, signaling::BreakoutSet>,
}

pub fn build_router(state: Arc<AppState>) -> Router {
    let auth_routes = Router::new()
        .route("/register", post(auth::register))
        .route("/login", post(auth::login))
        .route("/refresh", post(auth::refresh))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            rate_limit::auth_rate_limit,
        ));

    Router::new()
        .route("/health", get(|| async { "ok" }))
        .nest("/api/auth", auth_routes)
        .route("/api/users/me", get(users::me))
        .route("/api/users/search", get(users::search))
        .route("/api/rooms", post(rooms::create_room))
        .route("/api/rooms/{code}", get(rooms::get_room))
        .route("/api/rooms/{code}/join", post(rooms::join_room))
        .route(
            "/api/rooms/{code}/recordings",
            get(recordings::list).post(recordings::upload),
        )
        .route("/api/recordings", get(recordings::library))
        .route("/api/recordings/{id}", get(recordings::download))
        .route(
            "/api/recordings/{id}/share",
            post(recordings::share).get(recordings::shares),
        )
        .route("/api/recordings/{id}/share/{user_id}", axum::routing::delete(recordings::unshare))
        .route("/api/meetings", get(meetings::list).post(meetings::create))
        .route("/api/meetings/conflicts", post(meetings::check_conflicts))
        .route("/api/meetings/{id}", axum::routing::delete(meetings::delete))
        .route("/api/meetings/{id}/start", post(meetings::start))
        .route("/api/meetings/{id}/minutes", post(meetings::save_minutes))
        .route("/api/meetings/{id}/invitees", get(meetings::invitees))
        .route("/api/meetings/{id}/respond", post(meetings::respond))
        .route("/api/quarantine/analytics", get(meetings::quarantine_analytics))
        .route("/api/missed-calls/ack", post(presence::ack_missed_calls))
        .route("/api/rooms/{code}/minutes", post(meetings::save_minutes_by_room))
        .route("/api/rooms/{code}/notes", get(meetings::notes_by_room))
        // ---- Enterprise: organizações / diretório ----
        .route("/api/orgs", get(org::my_orgs).post(org::create_org))
        .route("/api/orgs/{org_id}/branches", get(org::list_branches).post(org::create_branch))
        .route("/api/orgs/{org_id}/employees", get(org::list_employees).post(org::add_employee))
        .route("/api/orgs/{org_id}/employees/{user_id}", axum::routing::delete(org::remove_employee))
        .route("/api/orgs/{org_id}/groups", get(org::list_groups).post(org::create_group))
        .route("/api/orgs/{org_id}/meeting-rooms", get(org::list_meeting_rooms).post(org::create_meeting_room))
        .route("/api/orgs/{org_id}/stats", get(org::org_stats))
        .route("/api/ice", get(rooms::ice_servers))
        .route("/ws", get(signaling::ws_handler))
        .route("/rtc", get(presence::rtc_handler))
        .layer(axum::extract::DefaultBodyLimit::max(
            recordings::MAX_RECORDING_BYTES,
        ))
        .layer(TraceLayer::new_for_http())
        .layer(CorsLayer::permissive()) // dev only; restrict origins in production
        .with_state(state)
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "delonix_server=info,tower_http=info".into()),
        )
        .init();

    let config = Config::from_env();

    let db = PgPoolOptions::new()
        .max_connections(10)
        .acquire_timeout(Duration::from_secs(5))
        .connect(&config.database_url)
        .await
        .expect("failed to connect to Postgres — is `docker compose up -d postgres` running?");

    sqlx::migrate!("./migrations")
        .run(&db)
        .await
        .expect("migrations failed");

    let state = Arc::new(AppState {
        db,
        hub: SignalingHub::default(),
        breakouts: dashmap::DashMap::new(),
        sfu: Arc::new(sfu::SfuState::default()),
        presence: presence::PresenceHub::default(),
        auth_limiter: RateLimiter::new(20, Duration::from_secs(60)),
        config: config.clone(),
    });

    // Cron: sweep de quarentena a cada 5 min (marca não-respondentes de
    // reuniões já começadas). Idempotente; as leituras fazem sweep na mesma.
    {
        let db = state.db.clone();
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(Duration::from_secs(300));
            loop {
                ticker.tick().await;
                match meetings::quarantine_sweep(&db).await {
                    Ok(n) if n > 0 => tracing::info!(added = n, "quarantine sweep"),
                    Ok(_) => {}
                    Err(e) => tracing::warn!(error = %e, "quarantine sweep failed"),
                }
            }
        });
    }

    let app = build_router(state);
    let listener = tokio::net::TcpListener::bind(&config.bind_addr)
        .await
        .expect("failed to bind");
    tracing::info!("Delonix Meet server listening on {}", config.bind_addr);
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await
    .unwrap();
}
