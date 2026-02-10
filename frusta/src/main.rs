use anyhow::Result;
use clap::Parser;

mod args;
pub mod metrics;
mod priority_queue;
mod protocol;
mod server;
mod viewport;
mod worker;

use args::{Cli, Commands};
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
    }
}
