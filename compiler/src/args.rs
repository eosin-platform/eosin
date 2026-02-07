use clap::{Parser, Subcommand};
use eosin_common::args::{NatsArgs, PostgresArgs};

#[derive(Parser, Debug, Clone)]
#[command(author, version, about, long_about = None)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand, Debug, Clone)]
pub enum Commands {
    /// Dispatch processing jobs for TIF files in S3
    Dispatch(DispatchArgs),

    /// Process slides from the `PROCESS_SLIDE` topic
    Process(ProcessArgs),
}

#[derive(Parser, Debug, Clone)]
pub struct S3Args {
    /// S3 bucket name
    #[arg(long, env = "S3_BUCKET", required = true)]
    pub bucket: String,

    /// S3 path prefix to scan for TIF files
    #[arg(long, env = "S3_PATH_PREFIX", default_value = "")]
    pub path_prefix: String,

    /// S3 endpoint URL (for S3-compatible storage)
    #[arg(long, env = "S3_ENDPOINT")]
    pub endpoint: Option<String>,

    /// S3 region
    #[arg(long, env = "S3_REGION", default_value = "us-east-1")]
    pub region: String,
}

#[derive(Parser, Debug, Clone)]
pub struct DispatchArgs {
    #[command(flatten)]
    pub s3: S3Args,

    #[command(flatten)]
    pub nats: NatsArgs,

    #[command(flatten)]
    pub postgres: PostgresArgs,

    /// Name of the `JetStream` stream to publish to
    #[arg(long, env = "STREAM_NAME", default_value = "eosin")]
    pub stream_name: String,

    /// Maximum number of messages to dispatch before terminating (0 = unlimited)
    #[arg(long, env = "MAX_DISPATCH", default_value = "0")]
    pub max_dispatch: usize,
}

#[derive(Parser, Debug, Clone)]
pub struct ProcessArgs {
    #[command(flatten)]
    pub s3: S3Args,

    #[command(flatten)]
    pub nats: NatsArgs,

    #[command(flatten)]
    pub postgres: PostgresArgs,

    /// Name of the `JetStream` stream to consume from
    #[arg(long, env = "STREAM_NAME", default_value = "eosin")]
    pub stream_name: String,

    /// Consumer name for durable consumption
    #[arg(long, env = "CONSUMER_NAME", default_value = "compiler")]
    pub consumer_name: String,

    /// Directory to download TIF files to
    #[arg(long, env = "DOWNLOAD_DIR", default_value = "/tmp/eosin/full")]
    pub download_dir: String,

    /// Meta service endpoint URL
    #[arg(long, env = "META_ENDPOINT", default_value = "http://localhost:8080")]
    pub meta_endpoint: String,

    /// Storage service gRPC endpoint URL
    #[arg(
        long,
        env = "STORAGE_ENDPOINT",
        default_value = "http://localhost:50051"
    )]
    pub storage_endpoint: String,

    /// Number of threads for parallel tile processing (0 = use all available CPUs)
    #[arg(long, env = "TILE_THREADS", default_value = "0")]
    pub tile_threads: usize,

    /// Interval in seconds between NATS in-progress heartbeats (0 = disabled)
    #[arg(long, env = "HEARTBEAT_INTERVAL", default_value = "12")]
    pub heartbeat_interval: u64,
}
