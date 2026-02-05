use std::net::SocketAddr;

use anyhow::Result;
use axum::{
    Router,
    extract::{
        State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    response::IntoResponse,
    routing::get,
};
use futures_util::{SinkExt, StreamExt};
use tower_http::cors::{Any, CorsLayer};

use crate::args::ServerArgs;

/// Shared application state
#[derive(Clone)]
pub struct AppState {
    pub storage_endpoint: String,
}

/// Run the frusta WebSocket server.
pub async fn run_server(args: ServerArgs) -> Result<()> {
    let state = AppState {
        storage_endpoint: args.storage_endpoint.clone(),
    };

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    let app = Router::new()
        .route("/", get(index))
        .route("/ws", get(ws_handler))
        .route("/health", get(health))
        .layer(cors)
        .with_state(state);

    let addr: SocketAddr = format!("0.0.0.0:{}", args.port).parse()?;
    tracing::info!(%addr, "starting frusta WebSocket server");

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}

/// Health check endpoint
async fn health() -> impl IntoResponse {
    "OK"
}

/// Index endpoint
async fn index() -> impl IntoResponse {
    "Frusta WebSocket Server"
}

/// WebSocket upgrade handler
async fn ws_handler(ws: WebSocketUpgrade, State(state): State<AppState>) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_socket(socket, state))
}

/// Handle an individual WebSocket connection
async fn handle_socket(socket: WebSocket, state: AppState) {
    let (mut sender, mut receiver) = socket.split();

    tracing::info!(
        storage_endpoint = %state.storage_endpoint,
        "new WebSocket connection established"
    );

    // Send a welcome message
    if let Err(e) = sender
        .send(Message::Text("Connected to Frusta".into()))
        .await
    {
        tracing::error!("failed to send welcome message: {}", e);
        return;
    }

    // Process incoming messages
    while let Some(msg) = receiver.next().await {
        match msg {
            Ok(Message::Text(text)) => {
                tracing::debug!("received text message: {}", text);

                // Echo the message back (placeholder - implement your logic here)
                if let Err(e) = sender
                    .send(Message::Text(format!("Echo: {}", text).into()))
                    .await
                {
                    tracing::error!("failed to send response: {}", e);
                    break;
                }
            }
            Ok(Message::Binary(data)) => {
                tracing::debug!("received binary message: {} bytes", data.len());

                // Echo binary data back (placeholder - implement your logic here)
                if let Err(e) = sender.send(Message::Binary(data)).await {
                    tracing::error!("failed to send binary response: {}", e);
                    break;
                }
            }
            Ok(Message::Ping(data)) => {
                if let Err(e) = sender.send(Message::Pong(data)).await {
                    tracing::error!("failed to send pong: {}", e);
                    break;
                }
            }
            Ok(Message::Pong(_)) => {
                // Pong received, connection is alive
            }
            Ok(Message::Close(_)) => {
                tracing::info!("client requested close");
                break;
            }
            Err(e) => {
                tracing::error!("websocket error: {}", e);
                break;
            }
        }
    }

    tracing::info!("WebSocket connection closed");
}
