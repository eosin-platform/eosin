use std::net::SocketAddr;

use anyhow::Result;
use async_nats::jetstream;
use eosin_common::shutdown::shutdown_signal;
use tokio_util::sync::CancellationToken;
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

    // Build the API server with graceful shutdown
    let cancel = CancellationToken::new();
    let api_service = ApiService::new(&args.data_root, jetstream);
    let api_cancel = cancel.clone();
    let api_server = Server::builder()
        .add_service(StorageApiServer::new(api_service))
        .serve_with_shutdown(api_addr, async move {
            api_cancel.cancelled().await;
        });

    // Spawn shutdown signal handler
    let signal_cancel = cancel.clone();
    tokio::spawn(async move {
        shutdown_signal().await;
        signal_cancel.cancel();
    });

    eosin_common::signal_ready();

    // Optionally start the cluster server
    if let Some(cluster_port) = args.cluster_port {
        let cluster_addr: SocketAddr = format!("0.0.0.0:{}", cluster_port).parse()?;
        tracing::info!(%cluster_addr, "starting cluster server");

        let cluster_cancel = cancel.clone();
        let cluster_service = ClusterServiceImpl::new();
        let cluster_server = Server::builder()
            .add_service(ClusterServiceServer::new(cluster_service))
            .serve_with_shutdown(cluster_addr, async move {
                cluster_cancel.cancelled().await;
            });

        // Run both servers concurrently
        tokio::select! {
            result = api_server => {
                if let Err(e) = &result {
                    tracing::error!(?e, "API server exited with error");
                }
                result?;
            }
            result = cluster_server => {
                if let Err(e) = &result {
                    tracing::error!(?e, "cluster server exited with error");
                }
                result?;
            }
        }
    } else {
        // Run only the API server
        api_server.await?;
    }

    tracing::info!("server stopped gracefully");
    Ok(())
}
