use clap::{Parser, Subcommand};
use histion_common::args::NatsArgs;

#[derive(Parser, Debug, Clone)]
#[command(author, version, about, long_about = None)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand, Debug, Clone)]
pub enum Commands {
    Server(ServerArgs),
}

#[derive(Parser, Debug, Clone)]
pub struct ServerArgs {
    #[arg(long, env = "PORT", default_value_t = 3000)]
    pub port: u16,

    #[arg(long, env = "STORAGE_ENDPOINT", required = true)]
    pub storage_endpoint: String,

    #[arg(long, env = "WORKER_COUNT", default_value_t = 4)]
    pub worker_count: usize,

    #[command(flatten)]
    pub nats: NatsArgs,
}
