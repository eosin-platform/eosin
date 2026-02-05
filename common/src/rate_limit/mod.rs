use anyhow::{Context, Result};
use deadpool_redis::{Pool, redis::Script};
use std::{
    ops::Deref,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use crate::args::RateLimiterArgs;

pub mod middleware;

#[derive(Clone, Debug)]
pub struct RateLimiterConfig {
    /// Max requests allowed in the burst window
    pub burst_limit: i64,
    /// Burst window length in milliseconds (e.g. 5000 = 5s)
    pub burst_window_ms: i64,
    /// Max requests allowed in the long-term window
    pub long_limit: i64,
    /// Long-term window length in milliseconds (e.g. 60000 = 60s)
    pub long_window_ms: i64,
    /// Max list length to keep per key (upper bound on work per check)
    pub max_list_size: i64,
    /// Optional key prefix
    pub key_prefix: String,
}

impl From<RateLimiterArgs> for RateLimiterConfig {
    fn from(args: RateLimiterArgs) -> Self {
        Self {
            burst_limit: args.burst_limit,
            burst_window_ms: args.burst_window_ms,
            long_limit: args.long_limit,
            long_window_ms: args.long_window_ms,
            max_list_size: args.max_list_size,
            key_prefix: args.key_prefix,
        }
    }
}

impl Default for RateLimiterConfig {
    fn default() -> Self {
        Self {
            burst_limit: 20,
            burst_window_ms: 5_000,
            long_limit: 200,
            long_window_ms: 60_000,
            max_list_size: 512, // cap list length to keep scanning cheap
            key_prefix: "rate:".into(),
        }
    }
}

pub struct RateLimiterInner {
    pool: Pool,
    script: Script,
    config: RateLimiterConfig,
}

#[derive(Clone)]
pub struct RateLimiter {
    inner: Arc<RateLimiterInner>,
}

impl Deref for RateLimiter {
    type Target = RateLimiterInner;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl RateLimiter {
    pub fn new(pool: Pool, config: RateLimiterConfig) -> Self {
        // KEYS[1]  = list key
        // ARGV[1]  = burst_limit
        // ARGV[2]  = burst_window_ms
        // ARGV[3]  = long_limit
        // ARGV[4]  = long_window_ms
        // ARGV[5]  = now_ms
        // ARGV[6]  = max_list_size
        //
        // Returns 1 if allowed, 0 if limited.
        const LUA: &str = include_str!("rate_limit.lua");

        let script = Script::new(LUA);
        Self {
            inner: Arc::new(RateLimiterInner {
                pool,
                script,
                config,
            }),
        }
    }

    pub fn with_defaults(pool: Pool) -> Self {
        Self::new(pool, RateLimiterConfig::default())
    }

    /// Returns Ok(true) if allowed, Ok(false) if rate-limited.
    pub async fn check_raw(&self, key: &str) -> Result<bool> {
        let mut conn = self
            .pool
            .get()
            .await
            .context("Failed to get Redis connection")?;

        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("Time went backwards")
            .as_millis() as i64;

        let list_key = format!("{}{}", self.config.key_prefix, key);

        let result: i32 = self
            .script
            .key(list_key)
            .arg(self.config.burst_limit)
            .arg(self.config.burst_window_ms)
            .arg(self.config.long_limit)
            .arg(self.config.long_window_ms)
            .arg(now_ms)
            .arg(self.config.max_list_size)
            .invoke_async(&mut conn)
            .await?;

        Ok(result == 1)
    }

    /// Convenience: swallow Redis errors and default to `true` (allow).
    /// You can flip this to `false` if you prefer "fail closed".
    pub async fn check(&self, key: &str) -> bool {
        match self.check_raw(key).await {
            Ok(allowed) => allowed,
            Err(e) => {
                eprintln!("Rate limiter failed for key {}: {:?}", key, e);
                false
            }
        }
    }
}
