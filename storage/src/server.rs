use std::net::SocketAddr;

use anyhow::Result;
use async_nats::jetstream;
use tonic::transport::Server;

use crate::api::ApiService;
use crate::args::ServerArgs;
use crate::cluster::ClusterServiceImpl;
use crate::proto::cluster::cluster_service_server::ClusterServiceServer;
use crate::proto::storage::storage_api_server::StorageApiServer;

/// Run the storage server with both API and optional cluster services.
pub async fn run_server(args: ServerArgs) -> Result<()> {
    // Connect to NATS
    let nats = args.nats.connect().await?;
    tracing::info!(url = %args.nats.nats_url, "connected to NATS");

    // Create JetStream context
    let jetstream = jetstream::new(nats);

    let api_addr: SocketAddr = format!("0.0.0.0:{}", args.api_port).parse()?;
    tracing::info!(%api_addr, "starting API server");

    // Build the API server
    let api_service = ApiService::new(&args.data_root, jetstream);
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
