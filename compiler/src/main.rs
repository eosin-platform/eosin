use anyhow::Result;
use clap::Parser;

mod args;
mod db;
mod dispatch;
mod meta_client;
mod process;
mod s3;
mod tiler;

use args::{Cli, Commands};
use dispatch::run_dispatch;
use process::run_process;

#[tokio::main]
async fn main() -> Result<()> {
    histion_common::init();

    tracing_subscriber::fmt::init();

    let cli = Cli::parse();

    match cli.command {
        Commands::Dispatch(args) => run_dispatch(args).await,
        Commands::Process(args) => run_process(args).await,
    }
}
