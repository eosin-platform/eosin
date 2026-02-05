use clap::{Parser, Subcommand};
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
}

#[derive(Parser, Debug, Clone)]
pub struct ServerArgs {
    #[arg(long, env = "API_PORT", default_value_t = 3500)]
    pub api_port: u16,

    #[arg(long, env = "CLUSTER_PORT")]
    pub cluster_port: Option<u16>,

    #[arg(long, env = "MASTER")]
    pub master: Option<SocketAddr>,

    #[arg(long, env = "SHARD")]
    pub shard: Option<u32>,
}
