use std::{
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};

use axum::{
    body::Body,
    http::{Request, Response, StatusCode, header::HeaderName},
};
use tower::{Layer, Service};

use crate::rate_limit::RateLimiter;

/// A Tower layer for Axum that rate-limits requests using `X-Forwarded-For`.
///
/// Behavior:
/// - If `X-Forwarded-For` is **absent**: skips rate limiting (passes through).
/// - If present: uses the **first** IP in the list as the key (trimmed).
/// - If denied by the limiter: returns `429 Too Many Requests`.
#[derive(Clone)]
pub struct RateLimitLayer {
    limiter: RateLimiter,
}

impl RateLimitLayer {
    pub fn new(limiter: RateLimiter) -> Self {
        Self { limiter }
    }
}

impl<S> Layer<S> for RateLimitLayer {
    type Service = RateLimitMiddleware<S>;

    fn layer(&self, inner: S) -> Self::Service {
        RateLimitMiddleware {
            inner,
            limiter: self.limiter.clone(),
        }
    }
}

#[derive(Clone)]
pub struct RateLimitMiddleware<S> {
    inner: S,
    limiter: RateLimiter,
}

impl<S> Service<Request<Body>> for RateLimitMiddleware<S>
where
    S: Service<Request<Body>, Response = Response<Body>> + Clone + Send + 'static,
    S::Future: Send + 'static,
    S::Error: Send + 'static,
{
    type Response = Response<Body>;
    type Error = S::Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Request<Body>) -> Self::Future {
        let limiter = self.limiter.clone();
        let mut inner = self.inner.clone();

        Box::pin(async move {
            // Only apply if X-Forwarded-For exists
            let xff_header_name = HeaderName::from_static("x-forwarded-for");
            let Some(hv) = req.headers().get(&xff_header_name) else {
                return inner.call(req).await;
            };

            let Ok(xff_str) = hv.to_str() else {
                // Malformed header -> treat as missing (skip limiting)
                return inner.call(req).await;
            };

            // Take first entry in XFF (client IP), trim spaces.
            // Example: "203.0.113.10, 70.41.3.18, 150.172.238.178"
            let Some(first) = xff_str.split(',').next() else {
                return inner.call(req).await;
            };
            let ip = first.trim();
            if ip.is_empty() {
                return inner.call(req).await;
            }

            // Ignore internal cluster traffic
            if ip.starts_with("10.") || ip.starts_with("192.168.") || ip.starts_with("172.") {
                return inner.call(req).await;
            }

            // Key format (namespaced) – tune as desired
            let key = format!("ip:{ip}");

            let allowed = limiter.check(&key).await;
            if !allowed {
                return Ok(Response::builder()
                    .status(StatusCode::TOO_MANY_REQUESTS)
                    .header("content-type", "text/plain; charset=utf-8")
                    .body(Body::from("Too Many Requests"))
                    .unwrap());
            }

            inner.call(req).await
        })
    }
}
