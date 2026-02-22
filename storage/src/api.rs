use std::sync::Arc;
use std::time::Instant;

use async_nats::jetstream::{self, message::PublishMessage};
use eosin_common::streams::{CacheMissEvent, topics::CACHE_MISS};
use tonic::{Request, Response, Status};
use uuid::Uuid;

use crate::proto::storage::{
    GetTileRequest, GetTileResponse, HealthCheckRequest, HealthCheckResponse, PutTileRequest,
    PutTileResponse, storage_api_server::StorageApi,
};
use crate::replication::{ShardEngine, TileWrite};

#[derive(Clone)]
pub struct ApiService {
    shard: ShardEngine,
    jetstream: Arc<jetstream::Context>,
}

impl ApiService {
    pub fn new(shard: ShardEngine, jetstream: jetstream::Context) -> Self {
        Self {
            shard,
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

        //metrics::tile_get(&id_str, level);
        //metrics::grpc_request("get_tile");

        let path = self.shard.tile_path(&id, req.level, req.x, req.y);
        tracing::info!(
            %id,
            x = req.x,
            y = req.y,
            level = req.level,
            ?path,
            "get_tile request"
        );

        let data = self.shard.read_tile(&id, req.level, req.x, req.y).await.map_err(|e| match e.kind() {
            std::io::ErrorKind::NotFound => {
                //metrics::tile_get_not_found(&id_str, level);
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
                    } else {
                        //metrics::cache_miss_published(&id_str_clone, level_clone);
                    }
                });
                Status::not_found("tile not found")
            }
            _ => {
                //metrics::tile_get_error(&id_str, level);
                tracing::error!(?e, ?path, "failed to read tile");
                Status::internal("failed to read tile")
            }
        })?;

        let _duration = start.elapsed().as_secs_f64();
        let _size = data.len();
        //metrics::tile_get_latency(&id_str, level, duration);
        //metrics::tile_get_success(&id_str, level, size);
        //metrics::disk_bytes_read(size);
        //metrics::grpc_latency("get_tile", duration);

        Ok(Response::new(GetTileResponse { data }))
    }

    async fn put_tile(
        &self,
        request: Request<PutTileRequest>,
    ) -> Result<Response<PutTileResponse>, Status> {
        let start = Instant::now();
        let req = request.into_inner();
        let id = Uuid::from_slice(&req.id).map_err(|_| Status::invalid_argument("invalid UUID"))?;

        //metrics::tile_put(&id_str, level);
        //metrics::grpc_request("put_tile");

        let mut id_bytes = [0_u8; 16];
        id_bytes.copy_from_slice(id.as_bytes());
        // During resharding, writes are accepted only by the shard that owns the tile
        // under the latest routing table. Source shards reject redirected writes.
        self.shard
            .write_as_master(TileWrite {
                id: id_bytes,
                x: req.x,
                y: req.y,
                level: req.level,
                data: req.data,
            })
            .await?;

        let _duration = start.elapsed().as_secs_f64();
        //metrics::tile_put_latency(&id_str, level, duration);
        //metrics::tile_put_success(&id_str, level, data_size);
        //metrics::disk_bytes_written(data_size);
        //metrics::grpc_latency("put_tile", duration);

        Ok(Response::new(PutTileResponse { success: true }))
    }

    async fn health_check(
        &self,
        _request: Request<HealthCheckRequest>,
    ) -> Result<Response<HealthCheckResponse>, Status> {
        //metrics::health_check();
        //metrics::grpc_request("health_check");
        Ok(Response::new(HealthCheckResponse { healthy: true }))
    }
}
