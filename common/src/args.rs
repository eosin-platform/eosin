use clap::Parser;

#[derive(Parser, Debug, Clone)]
pub struct RateLimiterArgs {
    /// Max requests allowed in the burst window
    #[arg(long, env = "RATE_LIMITER_BURST_LIMIT", default_value_t = 50)]
    pub burst_limit: i64,

    /// Burst window length in milliseconds (e.g. 5000 = 5s)
    #[arg(long, env = "RATE_LIMITER_BURST_WINDOW_MS", default_value_t = 5000)]
    pub burst_window_ms: i64,

    /// Max requests allowed in the long-term window
    #[arg(long, env = "RATE_LIMITER_LONG_LIMIT", default_value_t = 250)]
    pub long_limit: i64,

    /// Long-term window length in milliseconds (e.g. 60000 = 60s)
    #[arg(long, env = "RATE_LIMITER_LONG_WINDOW_MS", default_value_t = 60000)]
    pub long_window_ms: i64,

    /// Max list length to keep per key (upper bound on work per check)
    #[arg(long, env = "RATE_LIMITER_MAX_LIST_SIZE", default_value_t = 1000)]
    pub max_list_size: i64,

    /// Optional key prefix
    #[arg(long, env = "RATE_LIMITER_KEY_PREFIX", default_value = "")]
    pub key_prefix: String,
}

#[derive(Parser, Debug, Clone)]
pub struct NatsArgs {
    #[arg(long, env = "NATS_URL", required = true)]
    pub nats_url: String,

    #[arg(long, env = "NATS_USER", default_value = "app")]
    pub nats_user: String,

    #[arg(long, env = "NATS_PASSWORD", default_value = "devpass")]
    pub nats_password: String,
}

#[derive(Parser, Debug, Clone)]
pub struct DatabaseArgs {
    #[arg(long, default_value = "postgres")]
    pub db: String,

    #[clap(flatten)]
    pub postgres: PostgresArgs,
}

#[derive(Parser, Debug, Clone)]
pub struct PostgresArgs {
    #[arg(long, env = "POSTGRES_HOST", default_value = "localhost")]
    pub postgres_host: String,

    #[arg(long, env = "POSTGRES_PORT", default_value_t = 5432)]
    pub postgres_port: u16,

    #[arg(long, env = "POSTGRES_DATABASE", default_value = "postgres")]
    pub postgres_database: String,

    #[arg(long, env = "POSTGRES_USERNAME", default_value = "postgres")]
    pub postgres_username: String,

    #[arg(long, env = "POSTGRES_PASSWORD")]
    pub postgres_password: Option<String>,

    #[arg(long, env = "POSTGRES_CA_CERT")]
    pub postgres_ca_cert: Option<String>,

    #[arg(long, env = "POSTGRES_SSL_MODE", default_value = "prefer")]
    pub postgres_ssl_mode: String,
}

#[derive(Parser, Debug, Clone)]
pub struct RedisArgs {
    #[arg(long, env = "REDIS_HOST", default_value = "127.0.0.1")]
    pub redis_host: String,

    #[arg(long, env = "REDIS_PORT", default_value_t = 6379)]
    pub redis_port: u16,

    #[arg(long, env = "REDIS_USERNAME")]
    pub redis_username: Option<String>,

    #[arg(long, env = "REDIS_PASSWORD")]
    pub redis_password: Option<String>,

    #[arg(long, env = "REDIS_PROTO", default_value = "redis")]
    pub redis_proto: String,
}

impl RedisArgs {
    pub fn url_redacted(&self) -> String {
        format!(
            "{}://{}:{}@{}:{}",
            if self.redis_proto.is_empty() {
                "redis"
            } else {
                &self.redis_proto
            },
            self.redis_username.as_deref().unwrap_or(""),
            self.redis_password.as_deref().map(|_| "****").unwrap_or(""),
            self.redis_host,
            self.redis_port
        )
    }

    pub fn url(&self) -> String {
        let proto = if self.redis_proto.is_empty() {
            "redis"
        } else {
            &self.redis_proto
        };
        let mut url = format!("{}://", proto);
        if let Some(ref username) = self.redis_username {
            url.push_str(username);
            if let Some(ref password) = self.redis_password {
                url.push(':');
                url.push_str(password);
            }
            url.push('@');
        } else if let Some(ref password) = self.redis_password {
            url.push(':');
            url.push_str(password);
            url.push('@');
        }
        url.push_str(&format!("{}:{}/", self.redis_host, self.redis_port));
        url
    }
}

#[derive(Parser, Debug, Clone)]
pub struct KeycloakArgs {
    #[arg(long, env = "KC_ENDPOINT", required = true)]
    pub endpoint: String,

    #[arg(long, env = "KC_REALM", required = true)]
    pub realm: String,

    #[arg(long, env = "KC_ADMIN_REALM")]
    pub admin_realm: Option<String>,

    #[arg(long, env = "KC_ADMIN_USERNAME")]
    pub admin_username: Option<String>,

    #[arg(long, env = "KC_ADMIN_PASSWORD")]
    pub admin_password: Option<String>,

    #[arg(long, env = "KC_CLIENT_ID", required = true)]
    pub client_id: String,

    #[arg(long, env = "KC_CLIENT_SECRET", required = true)]
    pub client_secret: String,
}
