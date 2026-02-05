use std::{net::SocketAddr, sync::Arc};

use anyhow::Result;
use axum::{Router, extract::State, http::StatusCode, response::IntoResponse, routing::get};
use clap::Parser;
use tonic::transport::{Channel, Server};

mod api;
mod args;
mod cluster;

pub mod proto {
    pub mod storage {
        tonic::include_proto!("storage");
    }
    pub mod cluster {
        tonic::include_proto!("cluster");
    }
}

use api::ApiService;
use args::{Cli, Commands};
use cluster::ClusterServiceImpl;
use proto::cluster::cluster_service_server::ClusterServiceServer;
use proto::storage::storage_api_server::StorageApiServer;
use proto::storage::{HealthCheckRequest, storage_api_client::StorageApiClient};

#[derive(Clone)]
struct HealthState {
    grpc_target: Arc<String>,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    let cli = Cli::parse();

    match cli.command {
        Commands::Server(args) => run_server(args).await,
        Commands::Health(args) => run_health(args).await,
    }
}

async fn run_server(args: args::ServerArgs) -> Result<()> {
    let api_addr: SocketAddr = format!("0.0.0.0:{}", args.api_port).parse()?;
    tracing::info!(%api_addr, "starting API server");

    // Build the API server
    let api_service = ApiService::new();
    let api_server = Server::builder()
        .add_service(StorageApiServer::new(api_service))
        .serve(api_addr);

    // Optionally start the cluster server
    if let Some(cluster_port) = args.cluster_port {
        let cluster_addr: SocketAddr = format!("0.0.0.0:{}", cluster_port).parse()?;
        tracing::info!(%cluster_addr, "starting cluster server");

        let cluster_service = ClusterServiceImpl::new();
        let cluster_server = Server::builder()
            .add_service(ClusterServiceServer::new(cluster_service))
            .serve(cluster_addr);

        // Run both servers concurrently
        tokio::select! {
            result = api_server => {
                tracing::error!(?result, "API server exited");
                result?;
            }
            result = cluster_server => {
                tracing::error!(?result, "cluster server exited");
                result?;
            }
        }
    } else {
        // Run only the API server
        api_server.await?;
    }

    Ok(())
}

async fn run_health(args: args::HealthArgs) -> Result<()> {
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
    axum::serve(listener, app).await?;

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
