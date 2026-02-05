use anyhow::Result;
use clap::Parser;

mod args;
mod protocol;
mod server;
mod viewport;
mod worker;

use args::{Cli, Commands};
use server::run_server;

#[tokio::main]
async fn main() -> Result<()> {
    histion_common::init();

    tracing_subscriber::fmt::init();

    let cli = Cli::parse();

    match cli.command {
        Commands::Server(args) => run_server(args).await,
    }
}
