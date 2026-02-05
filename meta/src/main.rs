use anyhow::Result;
use clap::Parser;

mod args;
mod cli;
mod client;
mod db;
mod handlers;
mod models;
mod server;

use args::{Cli, Commands};
use server::run_server;

#[tokio::main]
async fn main() -> Result<()> {
    histion_common::init();

    tracing_subscriber::fmt::init();

    let cli = Cli::parse();

    match cli.command {
        Commands::Server(args) => run_server(args).await,
        Commands::Create(args) => cli::run_create_slide(args).await,
        Commands::Get(args) => cli::run_get_slide(args).await,
        Commands::Update(args) => cli::run_update_slide(args).await,
        Commands::Delete(args) => cli::run_delete_slide(args).await,
        Commands::List(args) => cli::run_list_slides(args).await,
        Commands::Health(args) => cli::run_health(args.endpoint).await,
    }
}
