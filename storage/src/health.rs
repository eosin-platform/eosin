use std::{net::SocketAddr, sync::Arc};

use anyhow::Result;
use axum::{Router, extract::State, http::StatusCode, response::IntoResponse, routing::get};
use histion_common::shutdown::shutdown_signal;
use tonic::transport::Channel;

use crate::args::HealthArgs;
use crate::proto::storage::{HealthCheckRequest, storage_api_client::StorageApiClient};

#[derive(Clone)]
struct HealthState {
    grpc_target: Arc<String>,
}

/// Run the health check HTTP server that proxies health checks to the gRPC API server.
pub async fn run_health(args: HealthArgs) -> Result<()> {
    let http_addr: SocketAddr = format!("0.0.0.0:{}", args.http_port).parse()?;
    let state = HealthState {
        grpc_target: Arc::new(args.grpc_target.clone()),
    };

    tracing::info!(%http_addr, grpc_target = %args.grpc_target, "starting health check HTTP server");

    let app = Router::new()
        .route("/", get(http_health_handler))
        .route("/healthz", get(http_health_handler))
        .route("/readyz", get(http_health_handler))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(http_addr).await?;
    histion_common::signal_ready();
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;
    tracing::info!("health check server stopped gracefully");
    Ok(())
}

async fn http_health_handler(State(state): State<HealthState>) -> impl IntoResponse {
    match grpc_health_check(&state.grpc_target).await {
        Ok(healthy) if healthy => StatusCode::OK,
        Ok(_) => StatusCode::SERVICE_UNAVAILABLE,
        Err(e) => {
            tracing::error!(error = %e, "health check failed");
            StatusCode::SERVICE_UNAVAILABLE
        }
    }
}

async fn grpc_health_check(grpc_target: &str) -> Result<bool> {
    let channel = Channel::from_shared(grpc_target.to_string())?
        .connect()
        .await?;
    let mut client = StorageApiClient::new(channel);
    let response = client.health_check(HealthCheckRequest {}).await?;
    Ok(response.into_inner().healthy)
}
