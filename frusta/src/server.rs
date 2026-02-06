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
    streams::{topics, SlideEvent},
};
use histion_storage::client::StorageClient;
use std::time::Instant;
use std::{net::SocketAddr, time::Duration};
use tokio_util::sync::CancellationToken;
use tower_http::cors::{Any, CorsLayer};
use uuid::Uuid;

use crate::{
    args::ServerArgs,
    protocol::{
        MessageBuilder, MessageType, DPI_SIZE, IMAGE_DESC_SIZE, TILE_REQUEST_SIZE, VIEWPORT_SIZE,
    },
    viewport::{ImageDesc, RetrieveTileWork, TileMeta, ViewManager, Viewport},
    worker::worker_main,
};

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

    // Spawn a session-wide NATS subscription for progress events on ALL slides.
    // This sends progress updates for every slide to the client regardless of
    // which slides the client has open.
    tokio::spawn({
        let send_tx = send_tx.clone();
        let nats_client = state.nats_client.clone();
        let cancel = cancel.clone();
        async move {
            if let Err(e) = progress_subscription_task(send_tx, nats_client, cancel).await {
                tracing::warn!(error = %e, "progress subscription task ended");
            }
        }
    });

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
            ensure!(data.len() >= 1 + VIEWPORT_SIZE, "Update message too short");
            let slot = data[0];
            let viewport = Viewport::from_slice(&data[1..1 + VIEWPORT_SIZE])?;
            let start = Instant::now();
            session
                .get_viewport_mut(slot)?
                .update(&viewport)
                .await
                .context("failed to update viewport")?;
            let elapsed = Instant::now() - start;
            tracing::info!(slot, elapsed_ms = %elapsed.as_millis(), "viewport updated");
            Ok(())
        }
        MessageType::Open => {
            tracing::debug!("handling Open message");
            ensure!(
                data.len() >= 1 + DPI_SIZE + IMAGE_DESC_SIZE,
                "Open message too short"
            );
            let slot = data[0];
            let dpi = f32::from_le_bytes(data[1..1 + DPI_SIZE].try_into().unwrap());
            let image = ImageDesc::from_slice(&data[1 + DPI_SIZE..1 + DPI_SIZE + IMAGE_DESC_SIZE])?;
            session.open_slide(slot, dpi, image)?;
            Ok(())
        }
        MessageType::Close => {
            tracing::debug!("handling Close message");
            ensure!(data.len() >= 1, "Close message too short");
            let slot = data[0];
            session.close_slide(slot).context("failed to close slide")
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
            // Fast path: if we're in a cooldown period, silently drop without hitting Redis
            if let Some(until) = session.rate_limit_until {
                if Instant::now() < until {
                    return Ok(());
                }
                // Cooldown expired, clear it
                session.rate_limit_until = None;
            }
            // Rate limit tile requests by client IP
            if let Some(ref ip) = session.client_ip {
                let key = format!("ip:{}", ip);
                if !session.rate_limiter.check(&key).await {
                    tracing::warn!(ip = %ip, "rate limited tile request");
                    // Enter 5-second cooldown: silently drop all tile requests
                    session.rate_limit_until = Some(Instant::now() + Duration::from_secs(5));
                    // Notify the client at most once every 10 seconds
                    let should_notify = match session.last_rate_limit_notify {
                        Some(last) => last.elapsed() >= Duration::from_secs(10),
                        None => true,
                    };
                    if should_notify {
                        session.last_rate_limit_notify = Some(Instant::now());
                        let payload = MessageBuilder::rate_limited();
                        let _ = send_tx.try_send(Message::Binary(payload));
                    }
                    return Ok(());
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
        MessageType::RateLimited => {
            // RateLimited messages are server-to-client only, not expected from client
            tracing::warn!("received unexpected RateLimited message from client");
            Ok(())
        }
        MessageType::SlideCreated => {
            // SlideCreated messages are server-to-client only, not expected from client
            tracing::warn!("received unexpected SlideCreated message from client");
            Ok(())
        }
    }
}

/// Background task that subscribes to NATS progress events for ALL slides
/// using a wildcard topic. When a progress event is received, it extracts the
/// slide UUID from the topic and forwards it to the client via WebSocket.
async fn progress_subscription_task(
    send_tx: Sender<Message>,
    nats_client: NatsClient,
    cancel: CancellationToken,
) -> Result<()> {
    let topic = topics::SLIDE_PROGRESS_ALL;
    let mut subscriber = nats_client
        .subscribe(topic.to_string())
        .await
        .context("failed to subscribe to progress wildcard topic")?;

    tracing::debug!(topic = %topic, "subscribed to all slide progress events");

    loop {
        tokio::select! {
            _ = cancel.cancelled() => {
                tracing::debug!("progress subscription cancelled");
                break;
            }
            msg = subscriber.next() => {
                let Some(msg) = msg else {
                    tracing::warn!("progress subscription stream ended");
                    break;
                };

                // Extract the slide UUID from the NATS subject.
                // Subject format: "histion.slide.progress.<uuid>"
                let slide_id = match msg.subject.strip_prefix("histion.slide.progress.") {
                    Some(id_str) => match Uuid::parse_str(id_str) {
                        Ok(id) => id,
                        Err(e) => {
                            tracing::warn!(subject = %msg.subject, error = %e, "failed to parse slide UUID from progress topic");
                            continue;
                        }
                    },
                    None => {
                        tracing::warn!(subject = %msg.subject, "unexpected progress topic format");
                        continue;
                    }
                };

                // Parse the event from the payload
                let Some(event) = SlideEvent::from_bytes(&msg.payload) else {
                    tracing::warn!("invalid slide event payload size: {}", msg.payload.len());
                    continue;
                };

                match event {
                    SlideEvent::Progress(progress) => {
                        tracing::debug!(
                            slide_id = %slide_id,
                            progress_steps = progress.progress_steps,
                            progress_total = progress.progress_total,
                            "received progress event"
                        );
                        let payload = MessageBuilder::progress(slide_id, progress.progress_steps, progress.progress_total);
                        if let Err(e) = send_tx.send(Message::Binary(payload)).await {
                            tracing::warn!(error = %e, "failed to send progress to client");
                        }
                    }
                    SlideEvent::Created(created) => {
                        tracing::info!(
                            slide_id = %slide_id,
                            filename = %created.filename,
                            width = created.width,
                            height = created.height,
                            "received slide created event"
                        );
                        let payload = MessageBuilder::slide_created(&created);
                        if let Err(e) = send_tx.send(Message::Binary(payload)).await {
                            tracing::warn!(error = %e, "failed to send slide created to client");
                        }
                    }
                }
            }
        }
    }

    Ok(())
}

pub struct Session {
    worker_tx: Sender<RetrieveTileWork>,
    slides: [Option<Uuid>; 256],
    viewports: Vec<Option<ViewManager>>,
    send_tx: Sender<Bytes>,
    nats_client: NatsClient,
    rate_limiter: RateLimiter,
    client_ip: Option<String>,
    /// Last time we sent a RateLimited notification to the client.
    /// Throttled to at most once per 10 seconds.
    last_rate_limit_notify: Option<Instant>,
    /// When set, all tile requests are silently dropped until this instant.
    /// Activated for 5 seconds after a rate limit hit to avoid hammering Redis.
    rate_limit_until: Option<Instant>,
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
            send_tx,
            nats_client,
            rate_limiter,
            client_ip,
            last_rate_limit_notify: None,
            rate_limit_until: None,
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

    /// Place a slide into the client-specified slot.
    pub fn open_slide(&mut self, slot: u8, dpi: f32, image: ImageDesc) -> Result<()> {
        // If the slot is already occupied, close it first
        if self.slides[slot as usize].is_some() {
            self.close_slide(slot)?;
        }

        self.slides[slot as usize] = Some(image.id);
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
        Ok(())
    }

    /// Free the given slot.
    pub fn close_slide(&mut self, slot: u8) -> Result<()> {
        ensure!(
            self.slides[slot as usize].is_some(),
            "slot {} is not open",
            slot
        );
        self.slides[slot as usize] = None;
        self.viewports[slot as usize] = None;
        Ok(())
    }
}
