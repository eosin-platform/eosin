use clap::{Parser, Subcommand};
use eosin_common::args::NatsArgs;
use std::net::SocketAddr;

#[derive(Parser, Debug, Clone)]
#[command(author, version, about, long_about = None)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand, Debug, Clone)]
pub enum Commands {
    Server(ServerArgs),
    Health(HealthArgs),
}

#[derive(Parser, Debug, Clone)]
pub struct ServerArgs {
    /// gRPC port to listen on for API requests
    #[arg(long, env = "API_PORT", default_value_t = 3500)]
    pub api_port: u16,

    /// Root directory for storing data files
    #[arg(long, env = "DATA_ROOT", default_value = "/var/eosin")]
    pub data_root: String,

    /// Port for replica sync communication (between nodes)
    #[arg(long, env = "CLUSTER_PORT")]
    pub cluster_port: Option<u16>,

    /// Port for control plane communication (from the operator, e.g. topology changes)
    #[arg(long, env = "CONTROL_PORT")]
    pub control_port: Option<u16>,

    /// Address of the master node to connect to (for replicas)
    #[arg(long, env = "MASTER")]
    pub master: Option<SocketAddr>,

    /// Shard ID for this node (if using sharding)
    #[arg(long, env = "SHARD")]
    pub shard: Option<u32>,

    #[command(flatten)]
    pub nats: NatsArgs,
}

#[derive(Parser, Debug, Clone)]
pub struct HealthArgs {
    /// HTTP port to listen on for health checks
    #[arg(long, env = "HTTP_PORT", default_value_t = 8080)]
    pub http_port: u16,

    /// gRPC target address of the API server to health check
    #[arg(long, env = "GRPC_TARGET", default_value = "http://127.0.0.1:3500")]
    pub grpc_target: String,
}
