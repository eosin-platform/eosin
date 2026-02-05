use anyhow::{Context, Result};
use axum::{Router, routing::get};
use deadpool_postgres::Pool;
use histion_common::shutdown::shutdown_signal;
use std::net::SocketAddr;
use tower_http::cors::{Any, CorsLayer};

use crate::{args::ServerArgs, db, handlers};

/// Shared application state
#[derive(Clone)]
pub struct AppState {
    pub pool: Pool,
}

/// Run the metadata HTTP server.
pub async fn run_server(args: ServerArgs) -> Result<()> {
    let pool = histion_common::postgres::create_pool(args.postgres).await;
    db::init_schema(&pool)
        .await
        .context("failed to initialize database schema")?;
    let state = AppState { pool };
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);
    let app = Router::new()
        .route("/readyz", get(handlers::health))
        .route("/healthz", get(handlers::health))
        .route(
            "/slides",
            get(handlers::list_slides).post(handlers::create_slide),
        )
        .route(
            "/slides/{id}",
            get(handlers::get_slide)
                .patch(handlers::update_slide)
                .delete(handlers::delete_slide),
        )
        .layer(cors)
        .with_state(state);
    let addr: SocketAddr = format!("0.0.0.0:{}", args.port).parse()?;
    tracing::info!(%addr, "starting meta HTTP server");
    let listener = tokio::net::TcpListener::bind(addr).await?;
    histion_common::signal_ready();
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;
    tracing::info!("server stopped gracefully");
    Ok(())
}
