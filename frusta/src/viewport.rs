use async_channel::Sender;
use async_nats::Client as NatsClient;
use bytes::Bytes;
use futures_util::StreamExt;
use histion_common::streams::{topics, SlideProgressEvent};
use rustc_hash::FxHashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

use anyhow::{ensure, Context, Result};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::protocol::MessageBuilder;

const X_BITS: u64 = 20;
const Y_BITS: u64 = 20;
const LEVEL_BITS: u64 = 20;
const X_MASK: u64 = (1 << X_BITS) - 1;
const Y_MASK: u64 = (1 << Y_BITS) - 1;
const LEVEL_MASK: u64 = (1 << LEVEL_BITS) - 1;
const TILE_SIZE: f32 = 512.0;
const MAX_TILES_PER_UPDATE: usize = 64; // tune this
const SOFT_MAX_CACHE_SIZE: usize = 5_00; // tune this
const HARD_MAX_CACHE_SIZE: usize = 1_000; // tune this
const PRUNE_BATCH_SIZE: usize = 256; // max removals per update

#[derive(Debug, Clone, Copy)]
pub struct ImageDesc {
    pub id: Uuid,
    pub width: u32,
    pub height: u32,
    pub levels: u32,
}

impl ImageDesc {
    pub fn from_slice(data: &[u8]) -> Result<Self> {
        ensure!(data.len() == 16 + 4 + 4 + 4, "invalid image desc length");
        let id = Uuid::from_slice(&data[0..16])?;
        let width = u32::from_le_bytes(data[16..20].try_into().unwrap());
        let height = u32::from_le_bytes(data[20..24].try_into().unwrap());
        let levels = u32::from_le_bytes(data[24..28].try_into().unwrap());
        Ok(Self {
            id,
            width,
            height,
            levels,
        })
    }
}

#[derive(Debug, Clone, Copy)]
pub struct TileMeta {
    pub x: u32,
    pub y: u32,
    pub level: u32,
}

#[derive(Debug, Clone, Copy)]
pub struct RequestedTile {
    pub count: u32,
    pub last_requested_at: i64, // ms since epoch
}

type TileKey = u64;

impl TileMeta {
    #[inline]
    pub fn index_unchecked(&self) -> TileKey {
        let x = self.x as u64;
        let y = self.y as u64;
        let level = self.level as u64;
        debug_assert!(x <= X_MASK, "x out of range for index packing");
        debug_assert!(y <= Y_MASK, "y out of range for index packing");
        debug_assert!(level <= LEVEL_MASK, "level out of range for index packing");
        x | (y << X_BITS) | (level << (X_BITS + Y_BITS))
    }
}

pub struct ViewManager {
    sent: FxHashMap<TileKey, RequestedTile>,
    image: ImageDesc,
    cancel_update: Option<CancellationToken>,
    worker_tx: Sender<RetrieveTileWork>,
    dpi: f32,
    candidates: Vec<TileMeta>,
    send_tx: Sender<Bytes>,
    slot: u8,
    last_viewport: Arc<RwLock<Option<Viewport>>>,
    nats_cancel: CancellationToken,
}

#[derive(Debug, Clone, Copy)]
pub struct Viewport {
    pub x: f32,
    pub y: f32,
    pub width: u32,
    pub height: u32,
    pub zoom: f32,
}

impl Viewport {
    pub fn safe_zoom(&self) -> f32 {
        self.zoom.max(1e-6)
    }

    pub fn from_slice(data: &[u8]) -> Result<Self> {
        ensure!(data.len() == 5 * 4, "invalid viewport data length");
        Ok(Self {
            x: f32::from_le_bytes(data[0..4].try_into().unwrap()),
            y: f32::from_le_bytes(data[4..8].try_into().unwrap()),
            width: u32::from_le_bytes(data[8..12].try_into().unwrap()),
            height: u32::from_le_bytes(data[12..16].try_into().unwrap()),
            zoom: f32::from_le_bytes(data[16..20].try_into().unwrap()),
        })
    }
}

pub struct RetrieveTileWork {
    pub slide_id: Uuid,
    pub slot: u8,
    pub cancel: CancellationToken,
    pub tx: Sender<Bytes>,
    pub meta: TileMeta,
}

impl ViewManager {
    pub fn new(
        slot: u8,
        dpi: f32,
        image: ImageDesc,
        worker_tx: Sender<RetrieveTileWork>,
        send_tx: Sender<Bytes>,
        nats_client: NatsClient,
    ) -> Self {
        let nats_cancel = CancellationToken::new();
        let last_viewport = Arc::new(RwLock::new(None));

        // Spawn NATS subscription task for tile events
        tokio::spawn({
            let image = image.clone();
            let worker_tx = worker_tx.clone();
            let send_tx = send_tx.clone();
            let cancel = nats_cancel.clone();
            let viewport_ref = last_viewport.clone();
            let nats = nats_client.clone();
            async move {
                if let Err(e) = tile_subscription_task(
                    slot,
                    image,
                    worker_tx,
                    send_tx,
                    nats,
                    cancel,
                    viewport_ref,
                )
                .await
                {
                    tracing::warn!(error = %e, "tile subscription task ended");
                }
            }
        });

        // Spawn NATS subscription task for progress events
        tokio::spawn({
            let image_id = image.id;
            let send_tx = send_tx.clone();
            let cancel = nats_cancel.clone();
            async move {
                if let Err(e) =
                    progress_subscription_task(slot, image_id, send_tx, nats_client, cancel).await
                {
                    tracing::warn!(error = %e, "progress subscription task ended");
                }
            }
        });

        Self {
            slot,
            dpi,
            sent: FxHashMap::default(),
            image,
            worker_tx,
            cancel_update: None,
            candidates: Vec::with_capacity(MAX_TILES_PER_UPDATE),
            send_tx,
            last_viewport,
            nats_cancel,
        }
    }

    pub fn clear_cache(&mut self) {
        self.sent.clear();
        if let Some(cancel) = self.cancel_update.take() {
            cancel.cancel();
        }
    }

    pub fn maybe_soft_prune_cache(&mut self) {
        let len = self.sent.len();
        if len <= SOFT_MAX_CACHE_SIZE {
            return;
        }

        // Collect timestamps only.
        let mut times: Vec<i64> = self
            .sent
            .values()
            .map(|info| info.last_requested_at)
            .collect();

        use std::cmp::min;
        let nth = min(PRUNE_BATCH_SIZE, times.len() - 1);
        times.select_nth_unstable(nth);
        let cutoff = times[nth];

        // Keep entries with timestamp >= cutoff.
        self.sent.retain(|_, info| info.last_requested_at >= cutoff);
    }

    fn maybe_hard_prune_cache(&mut self) {
        let len = self.sent.len();
        if len <= HARD_MAX_CACHE_SIZE {
            return;
        }

        // Drop us down to a size suitable for soft pruning.
        let keep = SOFT_MAX_CACHE_SIZE;
        let drop = len.saturating_sub(keep);

        if drop == 0 {
            return;
        }

        // Collect timestamps only.
        let mut times: Vec<i64> = self
            .sent
            .values()
            .map(|info| info.last_requested_at)
            .collect();

        use std::cmp::min;
        let nth = min(drop, times.len() - 1);
        times.select_nth_unstable(nth);
        let cutoff = times[nth];

        // Keep entries with timestamp >= cutoff (the newer half).
        self.sent.retain(|_, info| info.last_requested_at >= cutoff);
    }

    pub async fn update(&mut self, viewport: &Viewport) -> Result<()> {
        // Store the viewport for use by the NATS subscription task
        *self.last_viewport.write().await = Some(*viewport);

        let cancel = CancellationToken::new();
        if let Some(old_cancel) = self.cancel_update.replace(cancel.clone()) {
            old_cancel.cancel();
        }

        self.maybe_hard_prune_cache();

        let now = chrono::Utc::now().timestamp_millis();

        // Compute the minimum mip level worth fetching based on zoom and DPI.
        // When zoomed out, there's no point fetching tiles at resolutions higher
        // than the display can actually render.
        //
        // The effective scale combines zoom with DPI normalization (assuming 96 DPI baseline).
        // Each mip level is 2x downsampled, so min_level = -log2(effective_scale).
        let min_level = self.compute_min_level(viewport);

        let candidates = &mut self.candidates;
        candidates.clear();

        let center_x = viewport.x + (viewport.width as f32 / 2.0);
        let center_y = viewport.y + (viewport.height as f32 / 2.0);

        // Decide which mip levels to consider, coarse → fine or vice versa.
        // For "higher mips first" meaning *more downsampled*, we iterate
        // from coarse (high level index) down to fine (min_level).
        //
        // ASSUMPTION: level 0 = highest resolution, image.levels-1 = coarsest.
        for level in (min_level..self.image.levels).rev() {
            let mut tiles = visible_tiles_for_level(viewport, &self.image, level);
            // Optional: sort center-out so we prioritize tiles near the viewport center.
            let downsample = 2f32.powi(level as i32);
            let px_per_tile = downsample * TILE_SIZE;

            let cmp = |a: &TileMeta, b: &TileMeta| {
                let ax = a.x as f32 * px_per_tile;
                let ay = a.y as f32 * px_per_tile;
                let bx = b.x as f32 * px_per_tile;
                let by = b.y as f32 * px_per_tile;

                let dax = ax - center_x;
                let day = ay - center_y;
                let dbx = bx - center_x;
                let dby = by - center_y;

                let da2 = dax * dax + day * day;
                let db2 = dbx * dbx + dby * dby;

                da2.partial_cmp(&db2).unwrap_or(std::cmp::Ordering::Equal)
            };

            if tiles.len() > MAX_TILES_PER_UPDATE {
                let nth = MAX_TILES_PER_UPDATE - 1;
                tiles.select_nth_unstable_by(nth, cmp);
                tiles[..MAX_TILES_PER_UPDATE].sort_by(cmp);
                tiles.truncate(MAX_TILES_PER_UPDATE);
            } else {
                tiles.sort_by(cmp);
            }

            for meta in tiles {
                let key = meta.index_unchecked();
                // Skip tiles we've already sent recently (simple de-dupe).
                if self.sent.contains_key(&key) {
                    continue;
                }
                candidates.push(meta);
                if candidates.len() >= MAX_TILES_PER_UPDATE {
                    break;
                }
            }
            if candidates.len() >= MAX_TILES_PER_UPDATE {
                break;
            }
        }
        for meta in candidates.iter() {
            let work = RetrieveTileWork {
                slot: self.slot,
                slide_id: self.image.id,
                cancel: cancel.clone(),
                tx: self.send_tx.clone(),
                meta: *meta,
            };
            self.worker_tx
                .send(work)
                .await
                .context("failed to send tile retrieval work")?;
            let key = meta.index_unchecked();
            self.sent
                .entry(key)
                .and_modify(|existing| {
                    existing.count += 1;
                    existing.last_requested_at = now;
                })
                .or_insert_with(|| RequestedTile {
                    count: 1,
                    last_requested_at: now,
                });
        }
        Ok(())
    }

    /// Compute the minimum mip level worth fetching for the current viewport.
    ///
    /// When the user is zoomed out, fetching high-resolution tiles is wasteful
    /// because the display cannot render that level of detail. This function
    /// returns the lowest (finest) mip level that provides useful detail given
    /// the current zoom and client DPI.
    ///
    /// The calculation uses a baseline of 96 DPI. Higher DPI displays can
    /// benefit from slightly finer mip levels.
    fn compute_min_level(&self, viewport: &Viewport) -> u32 {
        const BASE_DPI: f32 = 96.0;

        if self.image.levels == 0 {
            return 0;
        }

        let dpi_scale = self.dpi / BASE_DPI;
        let effective_scale = viewport.safe_zoom() * dpi_scale;

        let min_level = if effective_scale >= 1.0 {
            0
        } else {
            let raw = -effective_scale.log2();
            raw.max(0.0).ceil() as u32
        };

        min_level.min(self.image.levels - 1)
    }
}

impl Drop for ViewManager {
    fn drop(&mut self) {
        // Cancel the NATS subscription task when ViewManager is dropped
        self.nats_cancel.cancel();
    }
}

/// Compute which tiles (x, y) at a given level intersect the viewport.
fn visible_tiles_for_level(viewport: &Viewport, image: &ImageDesc, level: u32) -> Vec<TileMeta> {
    let downsample = 2f32.powi(level as i32);
    let px_per_tile = downsample * TILE_SIZE;

    // viewport.x / viewport.y assumed to be level-0 pixels
    let zoom = viewport.safe_zoom();
    let view_x0 = viewport.x / px_per_tile;
    let view_y0 = viewport.y / px_per_tile;
    let view_x1 = (viewport.x + viewport.width as f32 / zoom) / px_per_tile;
    let view_y1 = (viewport.y + viewport.height as f32 / zoom) / px_per_tile;
    let tiles_x = (image.width as f32 / px_per_tile).ceil().max(0.0);
    let tiles_y = (image.height as f32 / px_per_tile).ceil().max(0.0);

    // these are tile indices, [min, max)
    let min_tx = view_x0.floor().max(0.0) as u32;
    let min_ty = view_y0.floor().max(0.0) as u32;
    let max_tx = view_x1.ceil().max(0.0).min(tiles_x) as u32;
    let max_ty = view_y1.ceil().max(0.0).min(tiles_y) as u32;

    let tile_range_x = max_tx.saturating_sub(min_tx);
    let tile_range_y = max_ty.saturating_sub(min_ty);

    if tile_range_x == 0 || tile_range_y == 0 {
        return Vec::new();
    }

    let mut tiles = Vec::with_capacity((tile_range_x * tile_range_y) as usize);
    for ty in min_ty..max_ty {
        for tx in min_tx..max_tx {
            tiles.push(TileMeta {
                x: tx,
                y: ty,
                level,
            });
        }
    }

    tiles
}

/// Check if a tile at (x, y, level) is visible within the given viewport.
fn is_tile_in_viewport(viewport: &Viewport, image: &ImageDesc, x: u32, y: u32, level: u32) -> bool {
    let downsample = 2f32.powi(level as i32);
    let px_per_tile = downsample * TILE_SIZE;

    let zoom = viewport.safe_zoom();
    let view_x0 = viewport.x / px_per_tile;
    let view_y0 = viewport.y / px_per_tile;
    let view_x1 = (viewport.x + viewport.width as f32 / zoom) / px_per_tile;
    let view_y1 = (viewport.y + viewport.height as f32 / zoom) / px_per_tile;
    let tiles_x = (image.width as f32 / px_per_tile).ceil().max(0.0);
    let tiles_y = (image.height as f32 / px_per_tile).ceil().max(0.0);

    let min_tx = view_x0.floor().max(0.0) as u32;
    let min_ty = view_y0.floor().max(0.0) as u32;
    let max_tx = view_x1.ceil().max(0.0).min(tiles_x) as u32;
    let max_ty = view_y1.ceil().max(0.0).min(tiles_y) as u32;

    x >= min_tx && x < max_tx && y >= min_ty && y < max_ty
}

/// Background task that subscribes to NATS tile events for a specific slide.
/// When a tile event is received, checks if it's in the current viewport and
/// dispatches work to fetch and send it to the client.
async fn tile_subscription_task(
    slot: u8,
    image: ImageDesc,
    worker_tx: Sender<RetrieveTileWork>,
    send_tx: Sender<Bytes>,
    nats_client: NatsClient,
    cancel: CancellationToken,
    viewport_ref: Arc<RwLock<Option<Viewport>>>,
) -> Result<()> {
    let topic = topics::tile_data(image.id);
    let mut subscriber = nats_client
        .subscribe(topic.clone())
        .await
        .context("failed to subscribe to tile topic")?;

    tracing::debug!(topic = %topic, slide_id = %image.id, "subscribed to tile events");

    loop {
        tokio::select! {
            _ = cancel.cancelled() => {
                tracing::debug!(slide_id = %image.id, "tile subscription cancelled");
                break;
            }
            msg = subscriber.next() => {
                let Some(msg) = msg else {
                    tracing::warn!(slide_id = %image.id, "tile subscription stream ended");
                    break;
                };

                // Parse the tile coordinates from the payload
                // Format: x (4 bytes LE) | y (4 bytes LE) | level (4 bytes LE)
                if msg.payload.len() < 12 {
                    tracing::warn!("invalid tile event payload size: {}", msg.payload.len());
                    continue;
                }

                let x = u32::from_le_bytes(msg.payload[0..4].try_into().unwrap());
                let y = u32::from_le_bytes(msg.payload[4..8].try_into().unwrap());
                let level = u32::from_le_bytes(msg.payload[8..12].try_into().unwrap());

                // Check if the tile is within the current viewport
                let viewport_guard = viewport_ref.read().await;
                let Some(viewport) = *viewport_guard else {
                    // No viewport set yet, skip
                    continue;
                };
                drop(viewport_guard);

                if !is_tile_in_viewport(&viewport, &image, x, y, level) {
                    continue;
                }

                // Dispatch work to fetch and send the tile
                let work = RetrieveTileWork {
                    slide_id: image.id,
                    slot,
                    cancel: cancel.clone(),
                    tx: send_tx.clone(),
                    meta: TileMeta { x, y, level },
                };

                if let Err(e) = worker_tx.send(work).await {
                    tracing::warn!(error = %e, "failed to dispatch tile work from NATS event");
                }
            }
        }
    }

    Ok(())
}

/// Background task that subscribes to NATS progress events for a specific slide.
/// When a progress event is received, it is forwarded to the client via WebSocket.
async fn progress_subscription_task(
    slot: u8,
    slide_id: Uuid,
    send_tx: Sender<Bytes>,
    nats_client: NatsClient,
    cancel: CancellationToken,
) -> Result<()> {
    let topic = topics::slide_progress(slide_id);
    let mut subscriber = nats_client
        .subscribe(topic.clone())
        .await
        .context("failed to subscribe to progress topic")?;

    tracing::debug!(topic = %topic, slide_id = %slide_id, "subscribed to progress events");

    loop {
        tokio::select! {
            _ = cancel.cancelled() => {
                tracing::debug!(slide_id = %slide_id, "progress subscription cancelled");
                break;
            }
            msg = subscriber.next() => {
                let Some(msg) = msg else {
                    tracing::warn!(slide_id = %slide_id, "progress subscription stream ended");
                    break;
                };

                // Parse the progress event from the payload
                let Some(event) = SlideProgressEvent::from_bytes(&msg.payload) else {
                    tracing::warn!("invalid progress event payload size: {}", msg.payload.len());
                    continue;
                };

                // Build and send progress message to client
                let payload = MessageBuilder::progress(slot, event.progress_steps, event.progress_total);
                if let Err(e) = send_tx.send(payload).await {
                    tracing::warn!(error = %e, "failed to send progress to client");
                }
            }
        }
    }

    Ok(())
}
