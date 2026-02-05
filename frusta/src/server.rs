use anyhow::{bail, ensure, Context, Result};
use async_channel::Sender;
use async_nats::Client as NatsClient;
use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        State,
    },
    http::HeaderMap,
    response::IntoResponse,
    routing::get,
    Router,
};
use bytes::Bytes;
use futures_util::{stream::SplitSink, SinkExt, StreamExt};
use histion_common::{
    rate_limit::{RateLimiter, RateLimiterConfig},
    redis::init_redis,
    shutdown::shutdown_signal,
};
use histion_storage::client::StorageClient;
use std::{net::SocketAddr, time::Duration};
use tokio_util::sync::CancellationToken;
use tower_http::cors::{Any, CorsLayer};
use uuid::Uuid;

use crate::{
    args::ServerArgs,
    protocol::{
        MessageBuilder, MessageType, DPI_SIZE, IMAGE_DESC_SIZE, TILE_REQUEST_SIZE, UUID_SIZE,
        VIEWPORT_SIZE,
    },
    viewport::{ImageDesc, RetrieveTileWork, TileMeta, ViewManager, Viewport},
    worker::worker_main,
};
use rustc_hash::FxHashMap;

/// Shared application state
#[derive(Clone)]
pub struct AppState {
    pub storage_endpoint: String,
    pub tx: async_channel::Sender<RetrieveTileWork>,
    pub nats_client: NatsClient,
    pub rate_limiter: RateLimiter,
}

/// Run the frusta WebSocket server.
pub async fn run_server(args: ServerArgs) -> Result<()> {
    let storage = StorageClient::connect(&args.storage_endpoint)
        .await
        .context("failed to connect to storage endpoint")?;

    // Connect to NATS
    let nats_client = args.nats.connect().await?;
    tracing::info!(url = %args.nats.nats_url, "connected to NATS");

    // Initialize Redis for rate limiting
    let redis_pool = init_redis(&args.redis).await;
    let rate_limiter = RateLimiter::new(
        redis_pool,
        RateLimiterConfig {
            // 1,000 requests per minute (short window)
            burst_limit: 1_000,
            burst_window_ms: 60_000,
            // 10,000 requests per 10 minutes (long window)
            long_limit: 10_000,
            long_window_ms: 600_000,
            max_list_size: 10_000,
            key_prefix: "frusta:tile:".into(),
        },
    );
    tracing::info!("rate limiter initialized: 1000 req/min, 10000 req/10min");

    let cancel = CancellationToken::new();
    let (tx, rx) = async_channel::bounded(1000);
    let workers = (0..args.worker_count)
        .map(|_| {
            tokio::spawn({
                let cancel = cancel.clone();
                let storage = storage.clone();
                let rx = rx.clone();
                async move { worker_main(cancel, storage, rx).await }
            })
        })
        .collect::<Vec<_>>();
    let state = AppState {
        storage_endpoint: args.storage_endpoint.clone(),
        tx,
        nats_client,
        rate_limiter,
    };
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);
    let app = Router::new()
        .route("/ws", get(ws_handler))
        .route("/readyz", get(health))
        .route("/healthz", get(health))
        .layer(cors)
        .with_state(state);
    let addr: SocketAddr = format!("0.0.0.0:{}", args.port).parse()?;
    tracing::info!(%addr, "starting frusta WebSocket server");
    let listener = tokio::net::TcpListener::bind(addr).await?;
    histion_common::signal_ready();
    let shutdown_cancel = cancel.clone();
    axum::serve(listener, app)
        .with_graceful_shutdown(async move {
            shutdown_signal().await;
            shutdown_cancel.cancel();
        })
        .await?;
    tracing::info!("server stopped, waiting for workers to finish...");
    cancel.cancel();
    for worker in workers {
        let _ = worker.await;
    }
    Ok(())
}

/// Health check endpoint
async fn health() -> impl IntoResponse {
    "OK"
}

/// Extract client IP from X-Forwarded-For header
fn extract_client_ip(headers: &HeaderMap) -> Option<String> {
    let xff = headers.get("x-forwarded-for")?;
    let xff_str = xff.to_str().ok()?;
    let first = xff_str.split(',').next()?;
    let ip = first.trim();
    if ip.is_empty() {
        return None;
    }
    // Skip internal cluster traffic
    if ip.starts_with("10.") || ip.starts_with("192.168.") || ip.starts_with("172.") {
        return None;
    }
    Some(ip.to_string())
}

/// WebSocket upgrade handler
async fn ws_handler(
    ws: WebSocketUpgrade,
    headers: HeaderMap,
    State(state): State<AppState>,
) -> impl IntoResponse {
    let client_ip = extract_client_ip(&headers);
    ws.on_upgrade(async move |socket| {
        if let Err(e) = handle_socket(socket, state, client_ip).await {
            tracing::error!("WebSocket connection error: {}", e);
        }
    })
}

async fn sender_main(
    mut sender: SplitSink<WebSocket, Message>,
    image_rx: async_channel::Receiver<Bytes>,
    send_rx: async_channel::Receiver<Message>,
    cancel: CancellationToken,
) {
    let close_reason = loop {
        tokio::select! {
            _ = cancel.cancelled() => break None,
            data = image_rx.recv() => {
                match data {
                    Ok(data) => {
                        let msg = Message::Binary(data);
                        if let Err(e) = sender.send(msg).await {
                            break Some(format!("failed to send message: {}", e));
                        }
                    }
                    Err(e) => {
                        break Some(format!("failed to receive image data: {}", e));
                    }
                }
            }
            msg = send_rx.recv() => {
                match msg {
                    Ok(msg) => {
                        if let Err(e) = sender.send(msg).await {
                            break Some(format!("failed to send message: {}", e));
                        }
                    }
                    Err(e) => {
                        break Some(format!("failed to receive message to send: {}", e));
                    }
                }
            }
        }
    };
    if let Some(reason) = close_reason {
        tracing::error!("{}", reason);
        let _ = sender
            .send(Message::Close(Some(axum::extract::ws::CloseFrame {
                code: 1011, // Internal Error
                reason: reason.into(),
            })))
            .await;
    }
    let _ = sender.close().await;
    cancel.cancel();
}

/// Handle an individual WebSocket connection
async fn handle_socket(
    socket: WebSocket,
    state: AppState,
    client_ip: Option<String>,
) -> Result<()> {
    let (sender, mut receiver) = socket.split();
    let cancel = CancellationToken::new();
    let (image_tx, image_rx) = async_channel::bounded::<Bytes>(100);
    let (send_tx, send_rx) = async_channel::bounded::<Message>(100);
    tokio::spawn({
        let cancel = cancel.clone();
        async move { sender_main(sender, image_rx, send_rx, cancel).await }
    });
    let mut session = Session::new(
        state.tx.clone(),
        image_tx,
        state.nats_client.clone(),
        state.rate_limiter.clone(),
        client_ip,
    );

    tracing::info!(
        storage_endpoint = %state.storage_endpoint,
        "new WebSocket connection established"
    );

    let mut prune_ticker = tokio::time::interval(Duration::from_secs(30));

    // Process incoming messages
    loop {
        let msg = tokio::select! {
            _ = cancel.cancelled() => bail!("Context cancelled"),
            msg = receiver.next() => msg, // msg: Option<Message>
            _ = prune_ticker.tick() => {
                session.tick_soft_prune();
                continue;
            }
        };
        let Some(msg) = msg else {
            break;
        };
        match msg {
            Ok(Message::Text(text)) => {
                tracing::debug!("received text message: {}", text);
            }
            Ok(Message::Binary(data)) => {
                tracing::debug!("received binary message: {} bytes", data.len());
                if data.len() < 1 {
                    tracing::error!("binary message too short");
                    continue;
                }
                let Ok(ty) = data[0].try_into() else {
                    tracing::error!("invalid message type: {}", data[0]);
                    continue;
                };
                if let Err(e) = handle_message(ty, data.slice(1..), &mut session, &send_tx).await {
                    tracing::error!("failed to handle message: {}", e);
                }
            }
            Ok(Message::Ping(data)) => {
                if let Err(e) = send_tx.try_send(Message::Pong(data)) {
                    tracing::warn!("failed to send pong (channel full or closed): {}", e);
                }
            }
            Ok(Message::Pong(_)) => {
                // Pong received, connection is alive
            }
            Ok(Message::Close(_)) => bail!("client requested close"),
            Err(e) => bail!("websocket error: {}", e),
        }
    }
    tracing::info!("WebSocket connection closed gracefully");
    Ok(())
}

async fn handle_message(
    ty: MessageType,
    data: Bytes,
    session: &mut Session,
    send_tx: &Sender<Message>,
) -> Result<()> {
    match ty {
        MessageType::Update => {
            tracing::debug!("handling Update message");
            ensure!(data.len() >= 1 + VIEWPORT_SIZE, "Update message too short");
            let slot = data[0];
            let viewport = Viewport::from_slice(&data[1..1 + VIEWPORT_SIZE])?;
            session
                .get_viewport_mut(slot)?
                .update(&viewport)
                .await
                .context("failed to update viewport")
        }
        MessageType::Open => {
            tracing::debug!("handling Open message");
            ensure!(
                data.len() >= DPI_SIZE + IMAGE_DESC_SIZE,
                "Open message too short"
            );
            let dpi = f32::from_le_bytes(data[0..DPI_SIZE].try_into().unwrap());
            let image = ImageDesc::from_slice(&data[DPI_SIZE..DPI_SIZE + IMAGE_DESC_SIZE])?;
            let slot = session.open_slide(dpi, image)?;
            let payload = MessageBuilder::open_response(slot, image.id);
            send_tx
                .send(Message::Binary(payload))
                .await
                .context("failed to send Open response")?;
            Ok(())
        }
        MessageType::Close => {
            tracing::debug!("handling Close message");
            ensure!(data.len() >= UUID_SIZE, "Close message too short");
            let id = Uuid::from_slice(&data[..UUID_SIZE])?;
            session.close_slide(id).context("failed to close slide")
        }
        MessageType::ClearCache => {
            tracing::debug!("handling ClearCache message");
            ensure!(data.len() >= 1, "ClearCache message too short");
            let slot = data[0];
            session.get_viewport_mut(slot)?.clear_cache();
            Ok(())
        }
        MessageType::Progress => {
            // Progress messages are server-to-client only, not expected from client
            tracing::warn!("received unexpected Progress message from client");
            Ok(())
        }
        MessageType::RequestTile => {
            tracing::debug!("handling RequestTile message");
            // Rate limit tile requests by client IP
            if let Some(ref ip) = session.client_ip {
                let key = format!("ip:{}", ip);
                if !session.rate_limiter.check(&key).await {
                    tracing::warn!(ip = %ip, "rate limited tile request");
                    bail!("rate limited");
                }
            }
            ensure!(
                data.len() >= TILE_REQUEST_SIZE,
                "RequestTile message too short"
            );
            let slot = data[0];
            let x = u32::from_le_bytes(data[1..5].try_into().unwrap());
            let y = u32::from_le_bytes(data[5..9].try_into().unwrap());
            let level = u32::from_le_bytes(data[9..13].try_into().unwrap());
            let meta = TileMeta { x, y, level };
            session
                .get_viewport_mut(slot)?
                .request_tile(meta)
                .await
                .context("failed to request tile")
        }
    }
}

pub struct Session {
    worker_tx: Sender<RetrieveTileWork>,
    slides: [Option<Uuid>; 256],
    viewports: Vec<Option<ViewManager>>,
    free: Vec<u8>,
    send_tx: Sender<Bytes>,
    /// O(1) lookup from UUID to slot index
    uuid_to_slot: FxHashMap<Uuid, u8>,
    nats_client: NatsClient,
    rate_limiter: RateLimiter,
    client_ip: Option<String>,
}

impl Session {
    pub fn new(
        worker_tx: Sender<RetrieveTileWork>,
        send_tx: Sender<Bytes>,
        nats_client: NatsClient,
        rate_limiter: RateLimiter,
        client_ip: Option<String>,
    ) -> Self {
        Self {
            worker_tx,
            slides: [None; 256],
            viewports: (0..256).map(|_| None).collect(),
            free: (0..=255u8).rev().collect(),
            send_tx,
            uuid_to_slot: FxHashMap::default(),
            nats_client,
            rate_limiter,
            client_ip,
        }
    }

    pub fn tick_soft_prune(&mut self) {
        for vp in self.viewports.iter_mut().flatten() {
            vp.maybe_soft_prune_cache();
        }
    }

    /// Get a mutable reference to a viewport by slot index.
    pub fn get_viewport_mut(&mut self, slot: u8) -> Result<&mut ViewManager> {
        self.viewports
            .get_mut(slot as usize)
            .and_then(|v| v.as_mut())
            .context("invalid slide slot")
    }

    /// Allocate a slot for this slide ID. O(1).
    pub fn open_slide(&mut self, dpi: f32, image: ImageDesc) -> Result<u8> {
        // O(1) check if already open
        if let Some(&slot) = self.uuid_to_slot.get(&image.id) {
            return Ok(slot);
        }

        // Allocate new slot
        let slot = self.free.pop().context("no free slide slots available")?;

        self.slides[slot as usize] = Some(image.id);
        self.uuid_to_slot.insert(image.id, slot);
        let manager = ViewManager::new(
            slot,
            dpi,
            image,
            self.worker_tx.clone(),
            self.send_tx.clone(),
            self.nats_client.clone(),
            self.client_ip.clone(),
        );
        self.viewports[slot as usize] = Some(manager);
        Ok(slot)
    }

    /// Free the slot for this slide ID. O(1).
    pub fn close_slide(&mut self, id: Uuid) -> Result<()> {
        let slot = self.uuid_to_slot.remove(&id).context("slide not found")?;

        self.slides[slot as usize] = None;
        self.viewports[slot as usize] = None;
        self.free.push(slot);
        Ok(())
    }
}
