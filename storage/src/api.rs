use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use async_nats::jetstream::{self, message::PublishMessage};
use eosin_common::streams::{CacheMissEvent, topics::CACHE_MISS};
use tokio::fs;
use tonic::{Request, Response, Status};
use uuid::Uuid;

use crate::metrics;
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
        let start = Instant::now();
        let req = request.into_inner();
        let id = Uuid::from_slice(&req.id).map_err(|_| Status::invalid_argument("invalid UUID"))?;
        let id_str = id.to_string();
        let level = req.level;

        metrics::tile_get(&id_str, level);
        metrics::grpc_request("get_tile");

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
                metrics::tile_get_not_found(&id_str, level);
                // Publish cache miss event to JetStream
                let event = CacheMissEvent {
                    id,
                    x: req.x,
                    y: req.y,
                    level: req.level,
                };
                let service = self.clone();
                let id_str_clone = id_str.clone();
                let level_clone = level;
                tokio::spawn(async move {
                    if let Err(e) = service.publish_cache_miss(event).await {
                        metrics::cache_miss_publish_failed(&id_str_clone, level_clone);
                        tracing::error!(?e, "failed to publish cache miss event");
                    } else {
                        metrics::cache_miss_published(&id_str_clone, level_clone);
                    }
                });
                Status::not_found("tile not found")
            }
            _ => {
                metrics::tile_get_error(&id_str, level);
                tracing::error!(?e, ?path, "failed to read tile");
                Status::internal("failed to read tile")
            }
        })?;

        let duration = start.elapsed().as_secs_f64();
        let size = data.len();
        metrics::tile_get_latency(&id_str, level, duration);
        metrics::tile_get_success(&id_str, level, size);
        metrics::disk_bytes_read(size);
        metrics::grpc_latency("get_tile", duration);

        Ok(Response::new(GetTileResponse { data }))
    }

    async fn put_tile(
        &self,
        request: Request<PutTileRequest>,
    ) -> Result<Response<PutTileResponse>, Status> {
        let start = Instant::now();
        let req = request.into_inner();
        let id = Uuid::from_slice(&req.id).map_err(|_| Status::invalid_argument("invalid UUID"))?;
        let id_str = id.to_string();
        let level = req.level;
        let data_size = req.data.len();

        metrics::tile_put(&id_str, level);
        metrics::grpc_request("put_tile");

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
                metrics::tile_put_error(&id_str, level);
                tracing::error!(?e, ?parent, "failed to create directories");
                Status::internal("failed to create directories")
            })?;
            metrics::directory_created();
        }

        // Write to a temporary file first, then atomically rename.
        // Use a helper to ensure temp file cleanup in all cases.
        let temp_path = path.with_extension(format!("tmp.{}", Uuid::new_v4()));
        let result = async {
            fs::write(&temp_path, &req.data).await.map_err(|e| {
                tracing::error!(?e, ?temp_path, "failed to write temp tile");
                Status::internal("failed to write tile")
            })?;

            fs::rename(&temp_path, &path).await.map_err(|e| {
                tracing::error!(?e, ?temp_path, ?path, "failed to rename tile");
                Status::internal("failed to write tile")
            })
        }
        .await;

        // Clean up temp file if it still exists (write failed after creation, or rename failed)
        let _ = std::fs::remove_file(&temp_path);

        if let Err(_) = &result {
            metrics::tile_put_error(&id_str, level);
        }
        result?;

        let duration = start.elapsed().as_secs_f64();
        metrics::tile_put_latency(&id_str, level, duration);
        metrics::tile_put_success(&id_str, level, data_size);
        metrics::disk_bytes_written(data_size);
        metrics::grpc_latency("put_tile", duration);

        Ok(Response::new(PutTileResponse { success: true }))
    }

    async fn health_check(
        &self,
        _request: Request<HealthCheckRequest>,
    ) -> Result<Response<HealthCheckResponse>, Status> {
        metrics::health_check();
        metrics::grpc_request("health_check");
        Ok(Response::new(HealthCheckResponse { healthy: true }))
    }
}
