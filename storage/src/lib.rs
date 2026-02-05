pub mod api;
pub mod client;
pub mod cluster;
pub mod shard;

pub mod proto {
    pub mod storage {
        tonic::include_proto!("storage");
    }
    pub mod cluster {
        tonic::include_proto!("cluster");
    }
}
