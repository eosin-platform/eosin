use std::{net::IpAddr, str::FromStr};

use anyhow::Result;
use axum::{Json, http::HeaderMap, response::IntoResponse};
use owo_colors::OwoColorize;
use reqwest::StatusCode;
use rustls::{ClientConfig, RootCertStore, pki_types::CertificateDer};
use serde::de::{self, Deserializer, Visitor};
use serde::{Deserialize, Serialize};
use tokio_postgres_rustls::MakeRustlsConnect;

pub mod args;
pub mod cli;
pub mod cors;
pub mod metrics;
pub mod postgres;
pub mod rate_limit;
pub mod rbac;
pub mod redis;
mod request_context;
pub mod shutdown;
pub mod streams;
pub mod types;
pub mod wait;
pub mod wait_registry;

pub use request_context::*;

pub const DEFAULT_ENDPOINT: &str = "https://api.psyslop.tv";

pub fn signal_ready() {
    std::fs::write("/etc/ready", "ready").expect("Failed to write readiness file");
}

pub mod annotations {
    pub const STABLE_ID: &str = "dorch.beebs.dev/stable-id";
    pub const CREATED_BY: &str = "dorch.beebs.dev/created-by";
    pub const CREATED_BY_USER: &str = "dorch.beebs.dev/created-by-user";
    pub const SPEC_HASH: &str = "dorch.beebs.dev/spec-hash";
}

/// Redis topic for master server updates
pub const MASTER_TOPIC: &str = "dorch.master";

// Internal time system notes:
// The epoch of slop is: Wed, 7 June 1967 00:06:07 GMT
// This is the birthday of Slopicus Topicus.
// A single increment of the slop time unit is 30 minutes.
// Therefore, to convert from unix time to slop/chunk time:
// slop_time_ms = floor((unix_time_ms - SLOP_EPOCH) / 1800000.0)
pub const SLOP_EPOCH: i64 = -81129233067; // ms since (or rather, until) unix epoch
pub const MS_PER_SEGMENT: i64 = 1800000; // 30 minutes
pub const DEFAULT_WINDOW_CHUNKS: i64 = 4; // 4 chunks (2 hours)
pub const DEFAULT_WINDOW_DURATION: i64 = DEFAULT_WINDOW_CHUNKS * MS_PER_SEGMENT; // 2 hours
pub const DEFAULT_WINDOW_RIGHT: i64 = MS_PER_SEGMENT; // 30 minutes in ms
pub const DEFAULT_WINDOW_LEFT: i64 = DEFAULT_WINDOW_DURATION - DEFAULT_WINDOW_RIGHT;
pub const MAX_GUIDE_WINDOW_DURATION: i64 = 4 * 60 * 60 * 1000; // 4 hours

pub fn unix_to_chunk(unix_time_ms: i64) -> i64 {
    (((unix_time_ms as f64) - (SLOP_EPOCH as f64)) / (MS_PER_SEGMENT as f64)).floor() as i64
}

pub fn chunk_to_unix(chunk_time: i64) -> i64 {
    SLOP_EPOCH + (chunk_time * MS_PER_SEGMENT)
}

pub fn init() {
    let disable_colors = ["1", "true"].contains(
        &std::env::var("DISABLE_COLORS")
            .unwrap_or_else(|_| String::new())
            .to_lowercase()
            .as_str(),
    );
    owo_colors::set_override(!disable_colors);

    install_rustls_provider();
}

pub fn install_rustls_provider() {
    rustls::crypto::aws_lc_rs::default_provider()
        .install_default()
        .expect("install aws-lc-rs provider");
}

pub fn make_rustls(certs: Vec<CertificateDer<'_>>) -> Result<MakeRustlsConnect> {
    let mut roots = RootCertStore::empty();
    for cert in rustls_native_certs::load_native_certs().expect("could not load platform certs") {
        roots.add(cert).unwrap();
    }
    for cert in certs {
        roots.add(cert)?;
    }
    let config = ClientConfig::builder()
        .with_root_certificates(roots)
        .with_no_client_auth();
    Ok(MakeRustlsConnect::new(config))
}

#[derive(Deserialize, Default, Clone, Debug)]
pub struct Pagination {
    #[serde(default, deserialize_with = "deserialize_i64_from_string_or_int")]
    pub offset: i64,

    #[serde(default, deserialize_with = "deserialize_opt_i64_from_string_or_int")]
    pub limit: Option<i64>,
}

fn deserialize_i64_from_string_or_int<'de, D>(deserializer: D) -> Result<i64, D::Error>
where
    D: Deserializer<'de>,
{
    struct I64Visitor;

    impl<'de> Visitor<'de> for I64Visitor {
        type Value = i64;

        fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
            formatter.write_str("an integer or a string containing an integer")
        }

        fn visit_i64<E>(self, v: i64) -> Result<Self::Value, E> {
            Ok(v)
        }

        fn visit_u64<E>(self, v: u64) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            i64::try_from(v).map_err(|_| E::custom("integer out of range"))
        }

        fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            v.trim()
                .parse::<i64>()
                .map_err(|e| E::custom(format!("invalid integer: {e}")))
        }

        fn visit_string<E>(self, v: String) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            self.visit_str(&v)
        }
    }

    deserializer.deserialize_any(I64Visitor)
}

fn deserialize_opt_i64_from_string_or_int<'de, D>(deserializer: D) -> Result<Option<i64>, D::Error>
where
    D: Deserializer<'de>,
{
    struct OptI64Visitor;

    impl<'de> Visitor<'de> for OptI64Visitor {
        type Value = Option<i64>;

        fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
            formatter.write_str("an optional integer or a string containing an integer")
        }

        fn visit_none<E>(self) -> Result<Self::Value, E> {
            Ok(None)
        }

        fn visit_unit<E>(self) -> Result<Self::Value, E> {
            Ok(None)
        }

        fn visit_some<D2>(self, deserializer: D2) -> Result<Self::Value, D2::Error>
        where
            D2: Deserializer<'de>,
        {
            deserialize_i64_from_string_or_int(deserializer).map(Some)
        }

        fn visit_i64<E>(self, v: i64) -> Result<Self::Value, E> {
            Ok(Some(v))
        }

        fn visit_u64<E>(self, v: u64) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(Some(
                i64::try_from(v).map_err(|_| E::custom("integer out of range"))?,
            ))
        }

        fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            let trimmed = v.trim();
            if trimmed.is_empty() {
                return Ok(None);
            }
            trimmed
                .parse::<i64>()
                .map(Some)
                .map_err(|e| E::custom(format!("invalid integer: {e}")))
        }

        fn visit_string<E>(self, v: String) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            self.visit_str(&v)
        }
    }

    deserializer.deserialize_any(OptI64Visitor)
}

pub mod response {
    use std::fmt::{Debug, Display};

    use anyhow::Error;
    use axum::response::Response;

    use super::*;

    pub fn print_error<T>(e: T)
    where
        T: Into<Error> + Display + Debug,
    {
        eprintln!(
            "❌ {}",
            format!("{:?}", e.into())
                .split("\n")
                .map(|s| s.red().to_string())
                .collect::<Vec<_>>()
                .join("\n"),
        );
    }

    pub fn print_warning<T>(e: T)
    where
        T: Into<Error> + Display + Debug,
    {
        eprintln!(
            "⚠️ {}",
            format!("{:?}", e.into())
                .split("\n")
                .map(|s| s.yellow().to_string())
                .collect::<Vec<_>>()
                .join("\n"),
        );
    }

    pub fn err_resp<T>(e: T, code: StatusCode) -> Response
    where
        T: Into<Error> + Display + Debug,
    {
        let reason = format!("{}", e);
        print_error(e);
        (code, Json(serde_json::json!({"reason": reason}))).into_response()
    }

    pub fn not_found<T>(e: T) -> Response
    where
        T: Into<Error> + Display + Debug,
    {
        err_resp(e, StatusCode::NOT_FOUND)
    }

    pub fn too_many_requests<T>(e: T) -> Response
    where
        T: Into<Error> + Display + Debug,
    {
        err_resp(e, StatusCode::TOO_MANY_REQUESTS)
    }

    pub fn conflict<T>(e: T) -> Response
    where
        T: Into<Error> + Display + Debug,
    {
        err_resp(e, StatusCode::CONFLICT)
    }

    pub fn error<T>(e: T) -> Response
    where
        T: Into<Error> + Display + Debug,
    {
        internal_server_error(e)
    }

    pub fn internal_server_error<T>(e: T) -> Response
    where
        T: Into<Error> + Display + Debug,
    {
        err_resp(e, StatusCode::INTERNAL_SERVER_ERROR)
    }

    pub fn bad_gateway<T>(e: T) -> Response
    where
        T: Into<Error> + Display + Debug,
    {
        err_resp(e, StatusCode::BAD_GATEWAY)
    }

    pub fn bad_request<T>(e: T) -> Response
    where
        T: Into<Error> + Display + Debug,
    {
        err_resp(e, StatusCode::BAD_REQUEST)
    }

    pub fn forbidden<T>(e: T) -> Response
    where
        T: Into<Error> + Display + Debug,
    {
        err_resp(e, StatusCode::FORBIDDEN)
    }

    pub fn unauthorized<T>(e: T) -> Response
    where
        T: Into<Error> + Display + Debug,
    {
        err_resp(e, StatusCode::UNAUTHORIZED)
    }

    pub fn invalid_credentials() -> Response {
        err_resp(
            anyhow::anyhow!("Invalid username or password"),
            StatusCode::UNAUTHORIZED,
        )
    }

    pub fn service_unavailable<T>(e: T) -> Response
    where
        T: Into<Error> + Display + Debug,
    {
        err_resp(e, StatusCode::SERVICE_UNAVAILABLE)
    }
}

pub mod access_log {
    use super::*;

    pub async fn public(
        req: axum::extract::Request,
        next: axum::middleware::Next,
    ) -> axum::response::Response {
        request("PUBLIC", req, next, false).await
    }

    pub async fn public_error_only(
        req: axum::extract::Request,
        next: axum::middleware::Next,
    ) -> axum::response::Response {
        request("PUBLIC", req, next, true).await
    }

    pub async fn internal(
        req: axum::extract::Request,
        next: axum::middleware::Next,
    ) -> axum::response::Response {
        request("INTERNAL", req, next, false).await
    }

    pub async fn admin(
        req: axum::extract::Request,
        next: axum::middleware::Next,
    ) -> axum::response::Response {
        request("ADMIN", req, next, false).await
    }

    pub async fn internal_errors_only(
        req: axum::extract::Request,
        next: axum::middleware::Next,
    ) -> axum::response::Response {
        request("INTERNAL", req, next, true).await
    }

    pub async fn request(
        prefix: &str,
        req: axum::extract::Request,
        next: axum::middleware::Next,
        errors_only: bool,
    ) -> axum::response::Response {
        let ip = get_source_ip(req.headers())
            .map(|ip| ip.to_string())
            .unwrap_or("unknown".into());
        let method = req.method().clone();
        let path = req.uri().path().to_string();
        let start = std::time::Instant::now();
        let response = next.run(req).await;
        let duration = start.elapsed();
        let is_success =
            response.status().is_success() || response.status() == StatusCode::SWITCHING_PROTOCOLS;
        if is_success && errors_only {
            return response; // Skip non-error logs
        }
        let (a, b) = if is_success {
            // Note that 101 Switching Protocols is used for WebSocket upgrades
            ((20, 163, 73), (25, 163, 118))
        } else {
            ((230, 126, 16), (171, 85, 17))
        };
        println!(
            "🧾 {} {} {} {} {} {}{}{} {}{}",
            format!("[{}]", prefix).truecolor(a.0, a.1, a.2),
            method.truecolor(b.0, b.1, b.2),
            path.truecolor(b.0, b.1, b.2),
            "→".truecolor(a.0, a.1, a.2),
            response.status().truecolor(b.0, b.1, b.2),
            "(".truecolor(a.0, a.1, a.2),
            format!("{:?}", duration).truecolor(b.0, b.1, b.2),
            ")".truecolor(a.0, a.1, a.2),
            "xff=".magenta(),
            ip.magenta().dimmed(),
        );
        response
    }
}

pub fn get_source_ip(headers: &HeaderMap) -> Option<IpAddr> {
    // Prefer X-Forwarded-For (may contain multiple)
    if let Some(forwarded_for) = headers.get("x-forwarded-for")
        && let Ok(forwarded_for) = forwarded_for.to_str()
        && let Some(ip_str) = forwarded_for.split(',').next()
        && let Ok(ip) = ip_str.trim().parse()
    {
        return Some(ip);
    }

    // Fallback to X-Real-IP
    if let Some(real_ip) = headers.get("x-real-ip")
        && let Ok(ip_str) = real_ip.to_str()
        && let Ok(ip) = ip_str.trim().parse()
    {
        return Some(ip);
    }

    None
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug)]
#[serde(rename_all = "lowercase")]
pub enum ImageFormat {
    Png,
    Jpeg,
}

impl From<ImageFormat> for &'static str {
    fn from(v: ImageFormat) -> &'static str {
        match v {
            ImageFormat::Png => "png",
            ImageFormat::Jpeg => "jpeg",
        }
    }
}

impl FromStr for ImageFormat {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "png" => Ok(ImageFormat::Png),
            "jpeg" | "jpg" => Ok(ImageFormat::Jpeg),
            _ => Err(anyhow::anyhow!("unknown image format: {}", s)),
        }
    }
}

impl ImageFormat {
    pub fn mime_type(&self) -> &'static str {
        match self {
            ImageFormat::Png => "image/png",
            ImageFormat::Jpeg => "image/jpeg",
        }
    }

    pub fn file_extension(&self) -> &'static str {
        match self {
            ImageFormat::Png => "png",
            ImageFormat::Jpeg => "jpg",
        }
    }

    pub fn to_str(&self) -> &'static str {
        match self {
            ImageFormat::Png => "png",
            ImageFormat::Jpeg => "jpeg",
        }
    }
}
