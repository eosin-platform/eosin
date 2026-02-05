use tonic::{Request, Response, Status};
use uuid::Uuid;

use crate::proto::storage::{
    GetTileRequest, GetTileResponse, HealthCheckRequest, HealthCheckResponse, PutTileRequest,
    PutTileResponse, storage_api_server::StorageApi,
};

#[derive(Debug, Default)]
pub struct ApiService;

impl ApiService {
    pub fn new() -> Self {
        Self
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
        tracing::info!(
            %id,
            x = req.x,
            y = req.y,
            level = req.level,
            "get_tile request"
        );

        // TODO: Implement actual tile retrieval
        Ok(Response::new(GetTileResponse { data: vec![] }))
    }

    async fn put_tile(
        &self,
        request: Request<PutTileRequest>,
    ) -> Result<Response<PutTileResponse>, Status> {
        let req = request.into_inner();
        let id = Uuid::from_slice(&req.id).map_err(|_| Status::invalid_argument("invalid UUID"))?;
        tracing::info!(
            %id,
            x = req.x,
            y = req.y,
            level = req.level,
            data_len = req.data.len(),
            "put_tile request"
        );

        // TODO: Implement actual tile storage
        Ok(Response::new(PutTileResponse { success: true }))
    }

    async fn health_check(
        &self,
        _request: Request<HealthCheckRequest>,
    ) -> Result<Response<HealthCheckResponse>, Status> {
        tracing::debug!("health_check request");

        Ok(Response::new(HealthCheckResponse { healthy: true }))
    }
}
