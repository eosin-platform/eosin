use anyhow::Result;
use clap::Parser;

mod api;
mod args;
mod cluster;
mod health;
pub mod metrics;
mod server;

pub mod proto {
    pub mod storage {
        tonic::include_proto!("storage");
    }
    pub mod cluster {
        tonic::include_proto!("cluster");
    }
}

use args::{Cli, Commands};
use health::run_health;
use server::run_server;

#[tokio::main]
async fn main() -> Result<()> {
    eosin_common::init();

    tracing_subscriber::fmt::init();

    let cli = Cli::parse();

    match cli.command {
        Commands::Server(args) => {
            eosin_common::metrics::maybe_spawn_metrics_server();
            run_server(args).await
        }
        Commands::Health(args) => run_health(args).await,
    }
}
