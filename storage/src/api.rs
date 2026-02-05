use std::path::PathBuf;
use std::sync::Arc;

use async_nats::jetstream::{self, message::PublishMessage};
use histion_common::streams::{CacheMissEvent, topics::CACHE_MISS};
use tokio::fs;
use tonic::{Request, Response, Status};
use uuid::Uuid;

use crate::proto::storage::{
    GetTileRequest, GetTileResponse, HealthCheckRequest, HealthCheckResponse, PutTileRequest,
    PutTileResponse, storage_api_server::StorageApi,
};

#[derive(Clone)]
pub struct ApiService {
    data_root: PathBuf,
    jetstream: Arc<jetstream::Context>,
}

impl ApiService {
    pub fn new(data_root: impl Into<PathBuf>, jetstream: jetstream::Context) -> Self {
        Self {
            data_root: data_root.into(),
            jetstream: Arc::new(jetstream),
        }
    }

    /// Publish a cache miss event to JetStream.
    async fn publish_cache_miss(&self, event: CacheMissEvent) -> Result<(), async_nats::Error> {
        let payload = serde_json::to_vec(&event).expect("failed to serialize CacheMissEvent");
        let publish = PublishMessage::build()
            .payload(payload.into())
            .message_id(event.hash());
        self.jetstream.send_publish(CACHE_MISS, publish).await?;
        Ok(())
    }

    /// Returns the path to the tile file: {data_root}/{id}/{level}/{x}_{y}.webp
    fn tile_path(&self, id: &Uuid, level: u32, x: u32, y: u32) -> PathBuf {
        self.data_root
            .join(id.to_string())
            .join(level.to_string())
            .join(format!("{}_{}.webp", x, y))
    }
}

#[tonic::async_trait]
impl StorageApi for ApiService {
    async fn get_tile(
        &self,
        request: Request<GetTileRequest>,
    ) -> Result<Response<GetTileResponse>, Status> {
        let req = request.into_inner();
        let id = Uuid::from_slice(&req.id).map_err(|_| Status::invalid_argument("invalid UUID"))?;
        let path = self.tile_path(&id, req.level, req.x, req.y);
        tracing::info!(
            %id,
            x = req.x,
            y = req.y,
            level = req.level,
            ?path,
            "get_tile request"
        );

        let data = fs::read(&path).await.map_err(|e| match e.kind() {
            std::io::ErrorKind::NotFound => {
                // Publish cache miss event to JetStream
                let event = CacheMissEvent {
                    id,
                    x: req.x,
                    y: req.y,
                    level: req.level,
                };
                let service = self.clone();
                tokio::spawn(async move {
                    if let Err(e) = service.publish_cache_miss(event).await {
                        tracing::error!(?e, "failed to publish cache miss event");
                    }
                });
                Status::not_found("tile not found")
            }
            _ => {
                tracing::error!(?e, ?path, "failed to read tile");
                Status::internal("failed to read tile")
            }
        })?;

        Ok(Response::new(GetTileResponse { data }))
    }

    async fn put_tile(
        &self,
        request: Request<PutTileRequest>,
    ) -> Result<Response<PutTileResponse>, Status> {
        let req = request.into_inner();
        let id = Uuid::from_slice(&req.id).map_err(|_| Status::invalid_argument("invalid UUID"))?;
        let path = self.tile_path(&id, req.level, req.x, req.y);
        //tracing::info!(
        //    %id,
        //    x = req.x,
        //    y = req.y,
        //    level = req.level,
        //    data_len = req.data.len(),
        //    ?path,
        //    "put_tile request"
        //);

        // Create parent directories if they don't exist
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).await.map_err(|e| {
                tracing::error!(?e, ?parent, "failed to create directories");
                Status::internal("failed to create directories")
            })?;
        }

        fs::write(&path, &req.data).await.map_err(|e| {
            tracing::error!(?e, ?path, "failed to write tile");
            Status::internal("failed to write tile")
        })?;

        Ok(Response::new(PutTileResponse { success: true }))
    }

    async fn health_check(
        &self,
        _request: Request<HealthCheckRequest>,
    ) -> Result<Response<HealthCheckResponse>, Status> {
        Ok(Response::new(HealthCheckResponse { healthy: true }))
    }
}
