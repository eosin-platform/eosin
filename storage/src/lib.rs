pub mod api;
pub mod args;
pub mod client;
pub mod cluster;
pub mod health;
pub mod metrics;
pub mod server;
pub mod shard;

pub use client::StorageClient;

pub mod proto {
    pub mod storage {
        tonic::include_proto!("storage");
    }
    pub mod cluster {
        tonic::include_proto!("cluster");
    }
}
