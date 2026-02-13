use anyhow::{Context, Result};
use clap::Parser;
use owo_colors::OwoColorize;
use eosin_common::shutdown::shutdown_signal;
use tokio_util::sync::CancellationToken;

use crate::app::App;

mod app;
mod args;
mod server;

#[tokio::main]
pub async fn main() -> Result<()> {
    eosin_common::init();
    let cli = args::Cli::parse();
    match cli.command {
        args::Commands::Server(args) => run_servers(args).await,
    }
}

async fn run_servers(args: args::ServerArgs) -> Result<()> {
    eosin_common::metrics::maybe_spawn_metrics_server();
    let cancel = CancellationToken::new();
    let cancel_clone = cancel.clone();
    tokio::spawn(async move {
        shutdown_signal().await;
        cancel_clone.cancel();
    });
    let cancel_clone = cancel.clone();
    let app_state = App::new(args.kc.clone());
    let app_state_clone = app_state.clone();
    let mut internal_join = Box::pin(tokio::spawn(async move {
        server::internal::run_server(cancel_clone, args.internal_port, app_state_clone).await
    }));
    let cancel_clone = cancel.clone();
    let kc = args.kc.clone();
    let mut pub_join = Box::pin(tokio::spawn(async move {
        server::public::run_server(cancel_clone, args.public_port, app_state, kc).await
    }));
    tokio::select! {
        res = &mut internal_join => {
            cancel.cancel();
            pub_join
                .await
                .context("Failed to join public server task")?
                .context("Public server task failed")?;
            res.context("Internal server task failed")?.context("Failed to join internal server task")?;
        }
        res = &mut pub_join => {
            cancel.cancel();
            internal_join
                .await
                .context("Failed to join internal server task")?
                .context("Internal server task failed")?;
            res.context("Public server task failed")?.context("Failed to join public server task")?;
        }
    }
    println!("{}", "🛑 All servers shut down gracefully.".red());
    Ok(())
}
