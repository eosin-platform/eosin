use anyhow::{Context, Result};
use axum::{
    Router,
    routing::{get, patch, post},
};
use axum_keycloak_auth::{
    PassthroughMode,
    instance::{KeycloakAuthInstance, KeycloakConfig},
    layer::KeycloakAuthLayer,
};
use deadpool_postgres::Pool;
use eosin_common::shutdown::shutdown_signal;
use reqwest::Url;
use std::net::SocketAddr;
use tower_http::cors::{Any, CorsLayer};

use crate::{annotation_db, annotation_handlers, args::ServerArgs, db, handlers};

/// Shared application state
#[derive(Clone)]
pub struct AppState {
    pub pool: Pool,
}

/// Run the metadata HTTP server.
pub async fn run_server(args: ServerArgs) -> Result<()> {
    let kc = args.kc;
    let keycloak_auth_instance = KeycloakAuthInstance::new(
        KeycloakConfig::builder()
            .server(Url::parse(&kc.endpoint).unwrap())
            .realm(kc.realm)
            .build(),
    );
    let keycloak_layer = KeycloakAuthLayer::<String>::builder()
        .instance(keycloak_auth_instance)
        .passthrough_mode(PassthroughMode::Block)
        .persist_raw_claims(true)
        .expected_audiences(vec![kc.client_id])
        .build();
    let pool = eosin_common::postgres::create_pool(args.postgres).await;
    db::init_schema(&pool)
        .await
        .context("failed to initialize database schema")?;
    annotation_db::init_annotation_schema(&pool)
        .await
        .context("failed to initialize annotation schema")?;
    let state = AppState { pool };
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);
    let public = Router::new()
        .route("/readyz", get(handlers::health))
        .route("/healthz", get(handlers::health))
        .route("/slides", post(handlers::create_slide))
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
        .route(
            "/slides/{id}/progress",
            axum::routing::put(handlers::update_slide_progress),
        )
        // Annotation set routes
        .route(
            "/slides/{slide_id}/annotation-sets",
            get(annotation_handlers::list_annotation_sets),
        )
        .route(
            "/annotation-sets/{id}",
            get(annotation_handlers::get_annotation_set),
        )
        // Annotation routes
        .route(
            "/annotation-sets/{annotation_set_id}/annotations",
            get(annotation_handlers::list_annotations),
        )
        .route(
            "/annotations/{id}",
            get(annotation_handlers::get_annotation),
        )
        .layer(cors.clone())
        .with_state(state.clone());
    let protected = Router::new()
        .route(
            "/slides/{id}",
            patch(handlers::update_slide).delete(handlers::delete_slide),
        )
        .route(
            "/slides/{id}/progress",
            axum::routing::put(handlers::update_slide_progress),
        )
        // Annotation set routes
        .route(
            "/slides/{slide_id}/annotation-sets",
            get(annotation_handlers::list_annotation_sets),
        )
        .route(
            "/annotation-sets/{id}",
            patch(annotation_handlers::update_annotation_set)
                .delete(annotation_handlers::delete_annotation_set),
        )
        // Annotation routes
        .route(
            "/annotation-sets/{annotation_set_id}/annotations",
            post(annotation_handlers::create_annotation),
        )
        .route(
            "/annotations/{id}",
            patch(annotation_handlers::update_annotation)
                .delete(annotation_handlers::delete_annotation),
        )
        .layer(keycloak_layer)
        .layer(cors)
        .with_state(state);
    let addr: SocketAddr = format!("0.0.0.0:{}", args.port).parse()?;
    tracing::info!(%addr, "starting meta HTTP server");
    let listener = tokio::net::TcpListener::bind(addr).await?;
    eosin_common::signal_ready();
    axum::serve(listener, protected.merge(public))
        .with_graceful_shutdown(shutdown_signal())
        .await?;
    tracing::info!("server stopped gracefully");
    Ok(())
}
