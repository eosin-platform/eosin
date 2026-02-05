use anyhow::{Context, Result};
use axum::{
    body::Body,
    extract::{ConnectInfo, FromRequestParts},
    http::{HeaderMap, Request, StatusCode, request::Parts},
    middleware::Next,
    response::Response,
};
use serde::{Deserialize, Serialize};
use std::{
    future::Future,
    net::{IpAddr, SocketAddr},
    time::SystemTime,
};
use uuid::Uuid;

use crate::response;

/// Stored per-request in `request.extensions()`.
#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct RequestContext {
    pub request_id: Uuid,
    pub client_ip: Option<IpAddr>,
    pub user_agent: Option<String>,
    pub received_at: SystemTime,
}

/// Axum extractor usage: `RequestContextExtractor(ctx): RequestContextExtractor`
pub struct RequestContextExtractor(pub RequestContext);

impl std::ops::Deref for RequestContextExtractor {
    type Target = RequestContext;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<S> FromRequestParts<S> for RequestContextExtractor
where
    S: Send + Sync,
{
    type Rejection = (StatusCode, &'static str);

    fn from_request_parts(
        parts: &mut Parts,
        _state: &S,
    ) -> impl Future<Output = Result<Self, Self::Rejection>> + Send {
        // Clone what we need *before* the async block to avoid borrowing `parts` across await.
        let ctx = parts.extensions.get::<RequestContext>().cloned();

        async move {
            ctx.map(RequestContextExtractor).ok_or((
                StatusCode::INTERNAL_SERVER_ERROR,
                "RequestContext missing (did you add the middleware?)",
            ))
        }
    }
}

pub mod middleware {
    use super::*;

    pub async fn extract_context(req: Request<Body>, next: Next) -> Response {
        request_context_base(req, next, false).await
    }

    pub async fn create_context(req: Request<Body>, next: Next) -> Response {
        request_context_base(req, next, true).await
    }

    async fn request_context_base(
        mut req: Request<Body>,
        next: Next,
        generate_id: bool,
    ) -> Response {
        let headers = req.headers();
        let request_id = if generate_id {
            Uuid::new_v4()
        } else {
            match extract_or_generate_request_id(headers) {
                Ok(id) => id,
                Err(e) => return response::bad_request(e),
            }
        };
        let client_ip = extract_client_ip(headers).or_else(|| {
            // If you add `into_make_service_with_connect_info::<SocketAddr>()`,
            // Axum will populate ConnectInfo for you.
            req.extensions()
                .get::<ConnectInfo<SocketAddr>>()
                .map(|ConnectInfo(addr)| addr.ip())
        });
        let user_agent = headers
            .get(axum::http::header::USER_AGENT)
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string());
        let ctx = RequestContext {
            request_id,
            client_ip,
            user_agent,
            received_at: SystemTime::now(),
        };
        req.extensions_mut().insert(ctx);
        let mut res = next.run(req).await;
        res.headers_mut()
            .insert("x-request-id", request_id.to_string().parse().unwrap());
        res
    }
}

fn extract_or_generate_request_id(headers: &HeaderMap) -> Result<Uuid> {
    Ok(headers
        .get("x-request-id")
        .map(|v| v.to_str())
        .transpose()
        .context("Failed to convert x-request-id header to string")?
        .map(|s| Uuid::parse_str(s.trim()))
        .transpose()
        .context("Failed to parse x-request-id as UUID")?
        .unwrap_or_else(Uuid::new_v4))
}

fn extract_client_ip(headers: &HeaderMap) -> Option<IpAddr> {
    // Prefer first IP in X-Forwarded-For (client, proxy1, proxy2...)
    if let Some(xff) = headers.get("x-forwarded-for").and_then(|v| v.to_str().ok())
        && let Some(ip) = xff
            .split(',')
            .map(|s| s.trim())
            .find_map(|s| s.parse::<IpAddr>().ok())
    {
        return Some(ip);
    }

    // Some proxies set X-Real-IP
    if let Some(xri) = headers.get("x-real-ip").and_then(|v| v.to_str().ok())
        && let Ok(ip) = xri.trim().parse::<IpAddr>()
    {
        return Some(ip);
    }

    None
}
