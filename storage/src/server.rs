use std::net::SocketAddr;

use anyhow::Result;
use async_nats::jetstream;
use eosin_common::shutdown::shutdown_signal;
use tokio_util::sync::CancellationToken;
use tonic::transport::Server;

use crate::api::ApiService;
use crate::args::ServerArgs;
use crate::proto::cluster::{
    control_service_server::ControlServiceServer,
    replication_service_server::ReplicationServiceServer,
};
use crate::proto::storage::storage_api_server::StorageApiServer;
use crate::replication::{ControlServiceImpl, ReplicationServiceImpl, ShardEngine};

/// Run the storage server with both API and optional cluster services.
pub async fn run_server(args: ServerArgs) -> Result<()> {
    // Connect to NATS
    let nats = args.nats.connect().await?;
    tracing::info!(url = %args.nats.nats_url, "connected to NATS");

    // Create JetStream context
    let jetstream = jetstream::new(nats);

    let api_addr: SocketAddr = format!("0.0.0.0:{}", args.api_port).parse()?;
    tracing::info!(%api_addr, "starting API server");

    let shard_id = args.shard.unwrap_or(0);
    let shard = ShardEngine::new(
        &args.data_root,
        shard_id,
        args.backlog_capacity as usize,
    );

    if let Some(master) = args.master {
        let _ = shard
            .clone()
            .become_replica(crate::proto::cluster::BecomeReplicaRequest {
                shard_id,
                epoch: args.epoch,
                master_addr: master.to_string(),
            })
            .await;
    }

    // Build the API server with graceful shutdown
    let cancel = CancellationToken::new();
    let api_service = ApiService::new(shard.clone(), jetstream);
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

    let cluster_addr: SocketAddr = format!("0.0.0.0:{}", args.cluster_port).parse()?;
    let control_addr: SocketAddr = format!("0.0.0.0:{}", args.control_port).parse()?;

    let cluster_cancel = cancel.clone();
    let replication_service = ReplicationServiceImpl {
        shard: shard.clone(),
    };
    let cluster_server = Server::builder()
        .add_service(ReplicationServiceServer::new(replication_service))
        .serve_with_shutdown(cluster_addr, async move {
            cluster_cancel.cancelled().await;
        });

    let control_cancel = cancel.clone();
    let control_service = ControlServiceImpl { shard };
    let control_server = Server::builder()
        .add_service(ControlServiceServer::new(control_service))
        .serve_with_shutdown(control_addr, async move {
            control_cancel.cancelled().await;
        });

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
        result = control_server => {
            if let Err(e) = &result {
                tracing::error!(?e, "control server exited with error");
            }
            result?;
        }
    }

    tracing::info!("server stopped gracefully");
    Ok(())
}
