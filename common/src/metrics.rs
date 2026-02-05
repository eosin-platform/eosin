use axum::extract::MatchedPath;
use axum::{Router, routing::get};
use http::{Request, StatusCode};
use metrics::{counter, gauge, histogram};
use metrics_exporter_prometheus::{PrometheusBuilder, PrometheusHandle};
use owo_colors::OwoColorize;
use std::sync::OnceLock;
use std::time::Instant;
use tokio::net::TcpListener;
use tower::{Layer, Service};
use tower_http::classify::{ServerErrorsAsFailures, SharedClassifier};

// If you were using `tokio::sync::futures`, switch to futures_util in Cargo.toml:
// futures-util = "0.3"
use futures_util::future::BoxFuture;

use crate::shutdown::shutdown_signal;

static PROM_HANDLE: OnceLock<PrometheusHandle> = OnceLock::new();
static SERVER_STARTED: OnceLock<()> = OnceLock::new();

fn install_recorder_once() -> &'static PrometheusHandle {
    PROM_HANDLE.get_or_init(|| {
        PrometheusBuilder::new()
            .install_recorder()
            .expect("install global metrics recorder")
    })
}

pub fn maybe_spawn_metrics_server() {
    let Some(port) = metric_port_env() else {
        return;
    };
    let Some(node_id) = node_id_env() else {
        eprintln!("🛑 NODE_ID environment variable not set; cannot start metrics server");
        return;
    };
    // Set the panic hook to exit the process with a non-zero exit code
    // when a panic occurs on any thread. This is desired behavior when
    // running in a container, as the metrics server or controller may
    // panic and we always want to restart the container in that case.
    let default_panic = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        default_panic(info);
        std::process::exit(1);
    }));

    // Only start the HTTP server once even if this is called multiple times.
    if SERVER_STARTED.set(()).is_ok() {
        // Ensure recorder is installed exactly once process-wide.
        let _ = install_recorder_once();
        tokio::spawn(run_metrics_server(port, node_id));
    }
}

pub async fn run_metrics_server(port: u16, node_id: String) {
    let handle = install_recorder_once().clone();
    let metrics_route = {
        let handle = handle.clone();
        axum::routing::get(move || async move { handle.render() })
    };
    let app = Router::new()
        .route("/healthz", get(|| async { "ok" }))
        .route("/readyz", get(|| async { "ok" }))
        .route("/metrics", metrics_route)
        .layer(MetricsLayer::new(node_id));
    let addr = format!("0.0.0.0:{}", port);
    let listener = TcpListener::bind(&addr)
        .await
        .map_err(|e| {
            eprintln!("🛑 Failed to bind metrics server to {}: {}", addr, e);
            e
        })
        .unwrap();
    println!(
        "{}{}",
        "📈 Starting metrics server • port=".green(),
        format!("{}", port).green().dimmed(),
    );
    let started = Instant::now();
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .expect("Failed to serve metrics");
    println!(
        "{} {}",
        "🛑 Metrics server stopped gracefully • uptime was".red(),
        format!("{:.2?}", started.elapsed()).red().dimmed()
    );
}

fn metric_port_env() -> Option<u16> {
    std::env::var("METRICS_PORT")
        .ok()
        .and_then(|s| s.parse().ok())
}

fn node_id_env() -> Option<String> {
    std::env::var("NODE_ID").ok()
}

/// Tower layer that records request count, latency histogram, and in-flight gauge.
/// Uses `MatchedPath` to avoid high-cardinality raw URLs.
#[derive(Clone)]
pub struct MetricsLayer {
    classifier: SharedClassifier<ServerErrorsAsFailures>,
    node_id: String,
}
impl MetricsLayer {
    fn new(node_id: String) -> Self {
        Self {
            classifier: SharedClassifier::new(ServerErrorsAsFailures::new()),
            node_id,
        }
    }
}
impl<S> Layer<S> for MetricsLayer {
    type Service = MetricsService<S>;
    fn layer(&self, inner: S) -> Self::Service {
        MetricsService {
            inner,
            classifier: self.classifier.clone(),
            node_id: self.node_id.clone(),
        }
    }
}

#[derive(Clone)]
pub struct MetricsService<S> {
    inner: S,
    node_id: String,
    pub classifier: SharedClassifier<ServerErrorsAsFailures>,
}

impl<S, B> Service<Request<B>> for MetricsService<S>
where
    S: Service<Request<B>, Response = axum::response::Response> + Clone + Send + 'static,
    S::Future: Send + 'static,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = BoxFuture<'static, Result<Self::Response, Self::Error>>;

    fn poll_ready(
        &mut self,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Request<B>) -> Self::Future {
        let start = Instant::now();
        let method = req.method().clone();
        let method_str = method.as_str().to_owned();

        // Extract the route string here to avoid capturing non-Send types in the async block
        let route: String = req
            .extensions()
            .get::<MatchedPath>()
            .map(|m| m.as_str().to_owned())
            .unwrap_or_else(|| "UNKNOWN".to_string());

        let mut svc = self.inner.clone();

        // Call the inner service and get the future
        let fut = svc.call(req);

        let node_id = self.node_id.clone();

        // Move method_str and route into the async block so their owned values live long enough
        Box::pin(async move {
            let in_flight_gauge =
                gauge!("http_server_in_flight_requests", "method" => method_str.clone());
            in_flight_gauge.increment(1);

            let res = fut.await;
            let elapsed = start.elapsed().as_secs_f64();

            // Status label
            let status = match &res {
                Ok(resp) => resp.status(),
                Err(_) => StatusCode::INTERNAL_SERVER_ERROR,
            };

            let _ = histogram!(
                "http_server_request_duration_seconds",
                "elapsed" => elapsed.to_string(),
                "method" => method_str.clone(),
                "route"  => route.clone(),
                "status" => status.as_u16().to_string(),
                "node_id" => node_id.clone()
            );

            // Request counter
            counter!(
                "http_server_requests_total",
                "method" => method_str.clone(),
                "route"  => route.clone(),
                "status" => status.as_u16().to_string(),
                "node_id" => node_id
            )
            .increment(1);

            // Decrement in-flight
            in_flight_gauge.decrement(1);

            res
        })
    }
}
