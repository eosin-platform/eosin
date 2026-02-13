use anyhow::Result;
use tonic::transport::Channel;
use uuid::Uuid;

use crate::proto::storage::{
    GetTileRequest, GetTileResponse, PutTileRequest, storage_api_client::StorageApiClient,
};

#[derive(Clone)]
pub struct StorageClient {
    client: StorageApiClient<Channel>,
}

impl StorageClient {
    pub async fn connect(addr: impl Into<String>) -> Result<Self> {
        let client = StorageApiClient::connect(addr.into()).await?;
        Ok(Self { client })
    }

    pub async fn get_tile(&mut self, id: Uuid, x: u32, y: u32, level: u32) -> Result<Vec<u8>> {
        let request = GetTileRequest {
            id: id.as_bytes().to_vec(),
            x,
            y,
            level,
        };
        let response: GetTileResponse = self.client.get_tile(request).await?.into_inner();
        Ok(response.data)
    }

    pub async fn put_tile(
        &mut self,
        id: Uuid,
        x: u32,
        y: u32,
        level: u32,
        data: Vec<u8>,
    ) -> Result<bool> {
        let request = PutTileRequest {
            id: id.as_bytes().to_vec(),
            x,
            y,
            level,
            data,
        };
        let response = self.client.put_tile(request).await?.into_inner();
        Ok(response.success)
    }
}
