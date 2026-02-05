use anyhow::Result;
use clap::Parser;

mod args;
mod db;
mod handlers;
mod models;
mod server;

use args::{Cli, Commands};
use server::run_server;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    let cli = Cli::parse();

    match cli.command {
        Commands::Server(args) => run_server(args).await,
    }
}
