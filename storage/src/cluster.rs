use std::pin::Pin;

use futures::Stream;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tonic::{Request, Response, Status, Streaming};

use crate::proto::cluster::{
    ClusterMessage, JoinRequest, JoinResponse, LeaveRequest, LeaveResponse,
    cluster_service_server::ClusterService,
};

#[derive(Debug, Default)]
pub struct ClusterServiceImpl;

impl ClusterServiceImpl {
    pub fn new() -> Self {
        Self
    }
}

#[tonic::async_trait]
impl ClusterService for ClusterServiceImpl {
    type SyncStream = Pin<Box<dyn Stream<Item = Result<ClusterMessage, Status>> + Send>>;

    async fn sync(
        &self,
        request: Request<Streaming<ClusterMessage>>,
    ) -> Result<Response<Self::SyncStream>, Status> {
        let mut stream = request.into_inner();
        let (tx, rx) = mpsc::channel(128);

        // Spawn a task to handle incoming messages and respond
        tokio::spawn(async move {
            while let Ok(Some(msg)) = stream.message().await {
                tracing::info!(?msg, "received cluster message");

                // TODO: Process the incoming message and generate responses
                // Echo back for now as a stub
                if tx.send(Ok(msg)).await.is_err() {
                    break;
                }
            }
        });

        let output_stream = ReceiverStream::new(rx);
        Ok(Response::new(Box::pin(output_stream)))
    }

    async fn join(&self, request: Request<JoinRequest>) -> Result<Response<JoinResponse>, Status> {
        let req = request.into_inner();
        tracing::info!(
            node_id = %req.node_id,
            address = %req.address,
            "join request"
        );

        // TODO: Implement cluster join logic
        Ok(Response::new(JoinResponse {
            accepted: true,
            peer_addresses: vec![],
        }))
    }

    async fn leave(
        &self,
        request: Request<LeaveRequest>,
    ) -> Result<Response<LeaveResponse>, Status> {
        let req = request.into_inner();
        tracing::info!(node_id = %req.node_id, "leave request");

        // TODO: Implement cluster leave logic
        Ok(Response::new(LeaveResponse { acknowledged: true }))
    }
}
