use std::env;

#[derive(Clone)]
pub struct Config {
    pub database_url: String,
    pub bind_addr: String,
    pub jwt_secret: String,
    pub turn_host: String,
    pub turn_secret: String,
    pub access_ttl_secs: i64,
    pub refresh_ttl_secs: i64,
    pub room_token_ttl_secs: i64,
}

impl Config {
    pub fn from_env() -> Self {
        Self {
            database_url: env::var("DATABASE_URL").unwrap_or_else(|_| {
                "postgres://delonix:delonix_dev@localhost:5435/delonix_meet".into()
            }),
            bind_addr: env::var("BIND_ADDR").unwrap_or_else(|_| "0.0.0.0:8180".into()),
            jwt_secret: env::var("JWT_SECRET")
                .unwrap_or_else(|_| "dev-only-secret-change-in-production".into()),
            turn_host: env::var("TURN_HOST").unwrap_or_else(|_| "localhost:3478".into()),
            turn_secret: env::var("TURN_SECRET")
                .unwrap_or_else(|_| "delonix_turn_dev_secret".into()),
            access_ttl_secs: 15 * 60,
            refresh_ttl_secs: 30 * 24 * 3600,
            room_token_ttl_secs: 5 * 60,
        }
    }
}
