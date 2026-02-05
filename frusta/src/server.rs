use anyhow::{Context, Result, bail};
use async_channel::Sender;
use axum::{
    Router,
    extract::{
        State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    response::IntoResponse,
    routing::get,
};
use bytes::Bytes;
use futures_util::{SinkExt, StreamExt, stream::SplitSink};
use histion_storage::client::StorageClient;
use std::net::SocketAddr;
use tokio_util::sync::CancellationToken;
use tower_http::cors::{Any, CorsLayer};
use uuid::Uuid;

use crate::{
    args::ServerArgs,
    viewport::{ImageDesc, RetrieveTileWork, ViewManager, Viewport},
    worker::worker_main,
};

/// Shared application state
#[derive(Clone)]
pub struct AppState {
    pub storage_endpoint: String,
    pub tx: async_channel::Sender<RetrieveTileWork>,
}

/// Run the frusta WebSocket server.
pub async fn run_server(args: ServerArgs) -> Result<()> {
    let storage = StorageClient::connect(&args.storage_endpoint)
        .await
        .context("failed to connect to storage endpoint")?;
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

/// Index endpoint
async fn index() -> impl IntoResponse {
    "Frusta WebSocket Server"
}

/// WebSocket upgrade handler
async fn ws_handler(ws: WebSocketUpgrade, State(state): State<AppState>) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_socket(socket, state))
}

async fn sender_main(
    mut sender: SplitSink<WebSocket, Message>,
    image_rx: async_channel::Receiver<Bytes>,
    send_rx: async_channel::Receiver<Message>,
    cancel: CancellationToken,
) {
    loop {
        tokio::select! {
            _ = cancel.cancelled() => break,
            data = image_rx.recv() => {
                match data {
                    Ok(data) => {
                        let msg = Message::Binary(data);
                        if let Err(e) = sender.send(msg).await {
                            tracing::error!("failed to send message: {}", e);
                            break;
                        }
                    }
                    Err(e) => {
                        tracing::error!("failed to receive image data: {}", e);
                        break;
                    }
                }
            }
            msg = send_rx.recv() => {
                match msg {
                    Ok(msg) => {
                        if let Err(e) = sender.send(msg).await {
                            tracing::error!("failed to send message: {}", e);
                            break;
                        }
                    }
                    Err(e) => {
                        tracing::error!("failed to receive message to send: {}", e);
                        break;
                    }
                }
            }
        }
    }
}

/// Handle an individual WebSocket connection
async fn handle_socket(socket: WebSocket, state: AppState) {
    let (sender, mut receiver) = socket.split();
    let cancel = CancellationToken::new();
    let (image_tx, image_rx) = async_channel::bounded::<Bytes>(100);
    let (send_tx, send_rx) = async_channel::bounded::<Message>(100);
    tokio::spawn({
        let cancel = cancel.clone();
        async move { sender_main(sender, image_rx, send_rx, cancel).await }
    });
    let mut session = Session::new(state.tx.clone(), image_tx);

    tracing::info!(
        storage_endpoint = %state.storage_endpoint,
        "new WebSocket connection established"
    );

    // Process incoming messages
    while let Some(msg) = receiver.next().await {
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
                send_tx.send(Message::Pong(data)).await.unwrap_or_else(|e| {
                    tracing::error!("failed to send pong: {}", e);
                });
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

async fn handle_message(
    ty: WebsockMessageType,
    data: Bytes,
    session: &mut Session,
    send_tx: &Sender<Message>,
) -> Result<()> {
    match ty {
        WebsockMessageType::Update => {
            tracing::info!("handling Update message");
            let slot = data[0];
            let viewport = Viewport::from_slice(&data[1..])?;
            if slot as usize >= session.viewports.len() {
                bail!("invalid slide slot");
            }
            session.viewports[slot as usize]
                .as_mut()
                .context("invalid slide slot")?
                .update(&viewport)
                .await
        }
        WebsockMessageType::Open => {
            tracing::info!("handling Open message");
            let dpi = f32::from_le_bytes(data[0..4].try_into().unwrap());
            let image = ImageDesc::from_slice(&data[4..32])?;
            let viewport = Viewport::from_slice(&data[32..])?;
            let slot = session.open_slide(dpi, image, viewport)?;
            let payload = {
                let mut payload = Vec::with_capacity(18);
                payload.push(WebsockMessageType::Open as u8);
                payload.push(slot);
                payload.extend_from_slice(image.id.as_bytes());
                payload.into()
            };
            send_tx.send(Message::Binary(payload)).await?;
            Ok(())
        }
        WebsockMessageType::Close => {
            tracing::info!("handling Close message");
            let id: Uuid = Uuid::from_slice(&data[..])?;
            session.close_slide(id)
        }
    }
}

pub struct Session {
    worker_tx: Sender<RetrieveTileWork>,
    slides: [Option<Uuid>; 256], // fixed capacity, no allocation
    viewports: Vec<Option<ViewManager>>,
    free: Vec<u8>, // stack of free indices (0–255)
    send_tx: Sender<Bytes>,
}

impl Session {
    pub fn new(worker_tx: Sender<RetrieveTileWork>, send_tx: Sender<Bytes>) -> Self {
        Self {
            worker_tx,
            slides: [None; 256],
            viewports: vec![None; 256],
            free: (0..=255u8).rev().collect(), // LIFO free list
            send_tx,
        }
    }

    pub async fn update(&mut self, slot: u8, viewport: Viewport) -> Result<()> {
        if let Some(man) = &mut self.viewports[slot as usize] {
            man.update(&viewport).await?;
            return Ok(());
        } else {
            bail!("invalid slide slot");
        }
    }

    /// Allocate a slot for this slide ID. O(1).
    pub fn open_slide(&mut self, dpi: f32, image: ImageDesc, viewport: Viewport) -> Result<u8> {
        // Check if already open (O(256) worst-case scan)
        // Optional: If you want strict O(1), add a HashMap<Uuid, u8>
        if let Some((idx, _)) = self
            .slides
            .iter()
            .enumerate()
            .find(|(_, s)| s.map(|x| x == image.id).unwrap_or(false))
        {
            return Ok(idx as u8);
        }

        // Allocate new slot
        let Some(slot) = self.free.pop() else {
            bail!("no free slide slots available");
        };

        self.slides[slot as usize] = Some(image.id);
        let manager = ViewManager::new(
            slot,
            dpi,
            image,
            self.worker_tx.clone(),
            self.send_tx.clone(),
        );
        self.viewports[slot as usize] = Some(manager);
        Ok(slot)
    }

    /// Free the slot for this slide ID. O(1).
    pub fn close_slide(&mut self, id: Uuid) -> Result<()> {
        // Find which slot holds it
        for (idx, slot) in self.slides.iter_mut().enumerate() {
            if let Some(sid) = *slot {
                if sid == id {
                    // Free the slot
                    *slot = None;
                    self.viewports[idx] = None;
                    self.free.push(idx as u8);
                    return Ok(());
                }
            }
        }

        bail!("slide not found");
    }

    /// Fast lookup: slot → slide UUID
    pub fn get(&self, slot: u8) -> Option<Uuid> {
        self.slides[slot as usize]
    }
}

#[repr(u8)]
enum WebsockMessageType {
    Update = 0,
    Open = 1,
    Close = 2,
}

impl TryFrom<u8> for WebsockMessageType {
    type Error = ();

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(WebsockMessageType::Update),
            _ => Err(()),
        }
    }
}
