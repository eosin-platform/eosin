use crate::{app::App, server::internal};
use anyhow::{Context, Result};
use axum::{
    Router, middleware,
    routing::{get, post},
};
use axum_keycloak_auth::{
    PassthroughMode, Url,
    instance::{KeycloakAuthInstance, KeycloakConfig},
    layer::KeycloakAuthLayer,
};
use eosin_common::{access_log, args::KeycloakArgs, cors};
use owo_colors::OwoColorize;
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;

pub async fn run_server(
    cancel: CancellationToken,
    port: u16,
    app_state: App,
    kc: KeycloakArgs,
) -> Result<()> {
    let public_router = Router::new()
        .route("/user/register", post(internal::register))
        .route("/user/login", post(internal::login))
        .route("/user/refresh", post(internal::refresh))
        .route("/user/signout", post(internal::sign_out))
        .with_state(app_state.clone())
        .layer(middleware::from_fn(access_log::public))
        .layer(cors::dev());
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
    let protected_router = Router::new()
        .route(
            "/iam/user/info/{username_or_id}",
            get(internal::get_user_info),
        )
        .with_state(app_state)
        .layer(keycloak_layer)
        .layer(middleware::from_fn(access_log::public))
        .layer(cors::dev());
    let addr = format!("0.0.0.0:{}", port);
    let listener = TcpListener::bind(&addr)
        .await
        .map_err(|e| {
            eprintln!(
                "{}",
                format!("❌ Failed to bind server to {}: {}", addr, e).red()
            );
            e
        })
        .context("Failed to bind server")?;
    println!(
        "{}{}",
        "🚀 Starting public iam server • port=".green(),
        format!("{}", port).green().dimmed()
    );
    axum::serve(listener, public_router.merge(protected_router))
        .with_graceful_shutdown(async move {
            cancel.cancelled().await;
        })
        .await
        .context("Failed to start server")?;
    println!("{}", "🛑 Public server stopped gracefully.".red());
    Ok(())
}
