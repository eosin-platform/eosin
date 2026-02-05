use std::net::SocketAddr;

use anyhow::Result;
use clap::Parser;
use tonic::transport::Server;

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

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    let cli = Cli::parse();

    match cli.command {
        Commands::Server(args) => run_server(args).await,
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
