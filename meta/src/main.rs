use anyhow::{Context, Result};
use clap::Parser;
use eosin_common::shutdown::shutdown_signal;
use tokio_util::sync::CancellationToken;

mod annotation_db;
mod annotation_models;
mod args;
mod bitmask;
mod cli;
mod client;
mod db;
pub mod metrics;
mod models;
mod server;

use args::{Cli, Commands, DatasetCommands, DatasetSourceCommands, SlideCommands};

#[tokio::main]
async fn main() -> Result<()> {
    eosin_common::init();

    tracing_subscriber::fmt::init();

    let cli = Cli::parse();

    match cli.command {
        Commands::Server(args) => run_servers(args).await,
        Commands::Slide(args) => match args.command {
            SlideCommands::Create(args) => cli::run_create_slide(args).await,
            SlideCommands::Get(args) => cli::run_get_slide(args).await,
            SlideCommands::Update(args) => cli::run_update_slide(args).await,
            SlideCommands::Delete(args) => cli::run_delete_slide(args).await,
            SlideCommands::List(args) => cli::run_list_slides(args).await,
        },
        Commands::Dataset(args) => match args.command {
            DatasetCommands::Create(args) => cli::run_create_dataset(args).await,
            DatasetCommands::Get(args) => cli::run_get_dataset(args).await,
            DatasetCommands::Update(args) => cli::run_update_dataset(args).await,
            DatasetCommands::Delete(args) => cli::run_delete_dataset(args).await,
            DatasetCommands::List(args) => cli::run_list_datasets(args).await,
            DatasetCommands::Source(args) => match args.command {
                DatasetSourceCommands::Add(args) => cli::run_add_dataset_source(args).await,
                DatasetSourceCommands::Delete(args) => cli::run_delete_dataset_source(args).await,
                DatasetSourceCommands::List(args) => cli::run_list_dataset_sources(args).await,
            },
        },
        Commands::Health(args) => cli::run_health(args.endpoint).await,
    }
}

async fn run_servers(args: args::ServerArgs) -> Result<()> {
    eosin_common::metrics::maybe_spawn_metrics_server();

    // Initialize database
    let pool = eosin_common::postgres::create_pool(args.postgres).await;
    db::init_schema(&pool)
        .await
        .context("failed to initialize database schema")?;
    annotation_db::init_annotation_schema(&pool)
        .await
        .context("failed to initialize annotation schema")?;

    let state = server::AppState { pool };

    // Set up cancellation
    let cancel = CancellationToken::new();
    let cancel_clone = cancel.clone();
    tokio::spawn(async move {
        shutdown_signal().await;
        cancel_clone.cancel();
    });

    // Start internal server
    let cancel_internal = cancel.clone();
    let state_internal = state.clone();
    let internal_port = args.internal_port;
    let mut internal_join = Box::pin(tokio::spawn(async move {
        server::internal::run_server(cancel_internal, internal_port, state_internal).await
    }));

    // Start public server
    let cancel_public = cancel.clone();
    let kc = args.kc.clone();
    let public_port = args.public_port;
    let mut public_join = Box::pin(tokio::spawn(async move {
        server::public::run_server(cancel_public, public_port, state, kc).await
    }));

    eosin_common::signal_ready();

    // Wait for either server to finish
    tokio::select! {
        res = &mut internal_join => {
            cancel.cancel();
            public_join
                .await
                .context("failed to join public server task")?
                .context("public server task failed")?;
            res.context("failed to join internal server task")?.context("internal server task failed")?;
        }
        res = &mut public_join => {
            cancel.cancel();
            internal_join
                .await
                .context("failed to join internal server task")?
                .context("internal server task failed")?;
            res.context("failed to join public server task")?.context("public server task failed")?;
        }
    }

    tracing::info!("all servers stopped gracefully");
    Ok(())
}
