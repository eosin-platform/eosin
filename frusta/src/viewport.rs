use async_channel::Sender;
use async_nats::Client as NatsClient;
use bytes::Bytes;
use futures_util::StreamExt;
use histion_common::streams::topics;
use parking_lot::RwLock;
use rustc_hash::FxHashMap;
use std::sync::Arc;
use tokio::sync::RwLock as TokioRwLock;

use anyhow::{ensure, Context, Result};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

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

/// Shared state for tracking which tiles have been sent to the client.
/// This is shared between the main update loop and the NATS subscription task
/// to prevent sending duplicate tiles.
type SentTiles = Arc<RwLock<FxHashMap<TileKey, RequestedTile>>>;

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
    /// Set to `true` when the tile has been successfully sent to the client.
    /// Workers check this to avoid sending duplicate tiles.
    pub delivered: bool,
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
    sent: SentTiles,
    image: ImageDesc,
    cancel_update: Option<CancellationToken>,
    worker_tx: Sender<RetrieveTileWork>,
    dpi: f32,
    candidates: Vec<TileMeta>,
    send_tx: Sender<Bytes>,
    slot: u8,
    last_viewport: Arc<TokioRwLock<Option<Viewport>>>,
    nats_cancel: CancellationToken,
    client_ip: Option<String>,
    /// Tile keys dispatched in the most recent `update()` call.
    /// When the next `update()` cancels the current one, these keys are
    /// removed from `sent` so they can be re-dispatched immediately
    /// instead of being stuck behind the 30-second dedup window.
    last_dispatched_keys: Vec<TileKey>,
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
    pub client_ip: Option<String>,
    pub viewport: Arc<TokioRwLock<Option<Viewport>>>,
    pub image: ImageDesc,
    pub dpi: f32,
    /// Shared set of tiles that have been sent to this client.
    /// The worker checks this right before sending to avoid duplicates.
    pub sent: SentTiles,
}

impl ViewManager {
    pub fn new(
        slot: u8,
        dpi: f32,
        image: ImageDesc,
        worker_tx: Sender<RetrieveTileWork>,
        send_tx: Sender<Bytes>,
        nats_client: NatsClient,
        client_ip: Option<String>,
    ) -> Self {
        let nats_cancel = CancellationToken::new();
        let last_viewport = Arc::new(TokioRwLock::new(None));
        let sent: SentTiles = Arc::new(RwLock::new(FxHashMap::default()));

        // Spawn NATS subscription task for tile events
        tokio::spawn({
            let image = image.clone();
            let worker_tx = worker_tx.clone();
            let send_tx = send_tx.clone();
            let cancel = nats_cancel.clone();
            let viewport_ref = last_viewport.clone();
            let nats = nats_client.clone();
            let sent = sent.clone();
            let client_ip = client_ip.clone();
            async move {
                if let Err(e) = tile_subscription_task(
                    slot,
                    dpi,
                    image,
                    worker_tx,
                    send_tx,
                    nats,
                    cancel,
                    viewport_ref,
                    sent,
                    client_ip,
                )
                .await
                {
                    tracing::warn!(error = %e, "tile subscription task ended");
                }
            }
        });

        Self {
            slot,
            dpi,
            sent,
            image,
            worker_tx,
            cancel_update: None,
            candidates: Vec::with_capacity(MAX_TILES_PER_UPDATE),
            send_tx,
            last_viewport,
            nats_cancel,
            client_ip,
            last_dispatched_keys: Vec::new(),
        }
    }

    pub fn clear_cache(&mut self) {
        self.sent.write().clear();
        if let Some(cancel) = self.cancel_update.take() {
            cancel.cancel();
        }
    }

    pub fn maybe_soft_prune_cache(&mut self) {
        let mut sent = self.sent.write();
        let len = sent.len();
        if len <= SOFT_MAX_CACHE_SIZE {
            return;
        }

        // Collect timestamps only.
        let mut times: Vec<i64> = sent.values().map(|info| info.last_requested_at).collect();

        use std::cmp::min;
        let nth = min(PRUNE_BATCH_SIZE, times.len() - 1);
        times.select_nth_unstable(nth);
        let cutoff = times[nth];

        // Keep entries with timestamp >= cutoff.
        sent.retain(|_, info| info.last_requested_at >= cutoff);
    }

    fn maybe_hard_prune_cache(&mut self) {
        let mut sent = self.sent.write();
        let len = sent.len();
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
        let mut times: Vec<i64> = sent.values().map(|info| info.last_requested_at).collect();

        use std::cmp::min;
        let nth = min(drop, times.len() - 1);
        times.select_nth_unstable(nth);
        let cutoff = times[nth];

        // Keep entries with timestamp >= cutoff (the newer half).
        sent.retain(|_, info| info.last_requested_at >= cutoff);
    }

    pub async fn update(&mut self, viewport: &Viewport) -> Result<()> {
        // Store the viewport for use by the NATS subscription task
        *self.last_viewport.write().await = Some(*viewport);

        let cancel = CancellationToken::new();
        if let Some(old_cancel) = self.cancel_update.replace(cancel.clone()) {
            old_cancel.cancel();
            // The previous update was cancelled.  Instead of removing keys
            // outright (which causes the server to re-send tiles the client
            // already decoded, producing flicker), shorten the dedup window
            // to 2 s so they become eligible again soon but don't cause an
            // immediate redundant dispatch on the very next update.
            if !self.last_dispatched_keys.is_empty() {
                let cutoff = chrono::Utc::now().timestamp_millis() - 28_000;
                let mut sent = self.sent.write();
                for key in self.last_dispatched_keys.drain(..) {
                    if let Some(info) = sent.get_mut(&key) {
                        // Pull the timestamp back so only ~2 s remain on
                        // the 30 s dedup window.  If the tile was already
                        // delivered to the client, the next update will
                        // skip it (it's still in sent).  If it wasn't,
                        // the 2 s cooldown prevents the rapid cancel→
                        // redispatch cycle from flooding the client.
                        if info.last_requested_at > cutoff {
                            info.last_requested_at = cutoff;
                        }
                    }
                }
            }
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

            {
                let sent = self.sent.read();
                for meta in tiles {
                    let key = meta.index_unchecked();
                    // Skip tiles we've already dispatched. The client has a
                    // retry mechanism (TileRetryManager) that will explicitly
                    // request tiles it didn't receive via RequestTile messages.
                    // This prevents the server from re-sending tiles the client
                    // already has when the viewport is stationary.
                    if sent.contains_key(&key) {
                        continue;
                    }
                    candidates.push(meta);
                    if candidates.len() >= MAX_TILES_PER_UPDATE {
                        break;
                    }
                }
            }
            if candidates.len() >= MAX_TILES_PER_UPDATE {
                break;
            }
        }

        // Sort candidates so coarsest (highest level index) tiles are dispatched
        // first. This ensures the overview tiles reach workers before fine tiles,
        // giving the client an immediate (blurry) image while detail fills in.
        candidates.sort_unstable_by(|a, b| b.level.cmp(&a.level));

        // Mark all candidates as sent before dispatching work
        // This prevents duplicate dispatches from concurrent NATS events
        {
            let mut sent = self.sent.write();
            for meta in candidates.iter() {
                let key = meta.index_unchecked();
                sent.entry(key)
                    .and_modify(|existing| {
                        existing.count += 1;
                        existing.last_requested_at = now;
                        // Don't reset delivered flag - if it was already sent, keep it
                    })
                    .or_insert_with(|| RequestedTile {
                        count: 1,
                        last_requested_at: now,
                        delivered: false,
                    });
            }
        }

        // Remember which tile keys we dispatch so we can remove them from
        // `sent` if this update is cancelled before tiles are delivered.
        self.last_dispatched_keys.clear();
        self.last_dispatched_keys
            .extend(candidates.iter().map(|m| m.index_unchecked()));

        // Dispatch work items coarsest-first (already sorted above)
        for meta in candidates.iter() {
            let work = RetrieveTileWork {
                slot: self.slot,
                slide_id: self.image.id,
                cancel: cancel.clone(),
                tx: self.send_tx.clone(),
                meta: *meta,
                client_ip: self.client_ip.clone(),
                viewport: self.last_viewport.clone(),
                image: self.image,
                dpi: self.dpi,
                sent: self.sent.clone(),
            };
            self.worker_tx
                .send(work)
                .await
                .context("failed to send tile retrieval work")?;
        }
        Ok(())
    }

    /// Compute the minimum mip level worth fetching for the current viewport.
    fn compute_min_level(&self, viewport: &Viewport) -> u32 {
        compute_min_level(viewport, self.dpi, self.image.levels)
    }

    /// Request a specific tile from the client.
    /// This is used for retry requests when a tile wasn't received in time.
    /// If the tile doesn't exist, the request is silently discarded.
    pub async fn request_tile(&mut self, meta: TileMeta) -> Result<()> {
        let now = chrono::Utc::now().timestamp_millis();
        let key = meta.index_unchecked();

        // Check if this tile was requested very recently (within 2 seconds).
        // This prevents rapid-fire duplicate requests while still allowing
        // the client's retry mechanism (which has its own exponential backoff
        // starting at 3 seconds) to re-request tiles that failed to load.
        {
            let sent = self.sent.read();
            if let Some(info) = sent.get(&key) {
                if now - info.last_requested_at < 2_000 {
                    tracing::debug!(
                        x = meta.x,
                        y = meta.y,
                        level = meta.level,
                        elapsed_ms = now - info.last_requested_at,
                        "skipping tile request (requested very recently)"
                    );
                    return Ok(());
                }
            }
        }

        // Update or insert the request tracking
        {
            let mut sent = self.sent.write();
            sent.entry(key)
                .and_modify(|existing| {
                    existing.count += 1;
                    existing.last_requested_at = now;
                    // Reset delivered flag for explicit retries - the client
                    // is asking for this tile again because it didn't get it
                    existing.delivered = false;
                })
                .or_insert_with(|| RequestedTile {
                    count: 1,
                    last_requested_at: now,
                    delivered: false,
                });
        }

        // Create a cancellation token for this specific request
        let cancel = self
            .cancel_update
            .clone()
            .unwrap_or_else(CancellationToken::new);

        let work = RetrieveTileWork {
            slot: self.slot,
            slide_id: self.image.id,
            cancel,
            tx: self.send_tx.clone(),
            meta,
            client_ip: self.client_ip.clone(),
            viewport: self.last_viewport.clone(),
            image: self.image,
            dpi: self.dpi,
            sent: self.sent.clone(),
        };

        self.worker_tx
            .send(work)
            .await
            .context("failed to send tile retrieval work")?;

        tracing::debug!(
            x = meta.x,
            y = meta.y,
            level = meta.level,
            "dispatched individual tile request"
        );

        Ok(())
    }
}

impl Drop for ViewManager {
    fn drop(&mut self) {
        // Cancel the NATS subscription task when ViewManager is dropped
        self.nats_cancel.cancel();
    }
}

/// Compute the minimum mip level worth fetching for the given viewport.
///
/// When the user is zoomed out, fetching high-resolution tiles is wasteful
/// because the display cannot render that level of detail. This function
/// returns the lowest (finest) mip level that provides useful detail given
/// the current zoom and client DPI.
///
/// The calculation mirrors the browser's `computeIdealLevel` in viewport.ts:
///   idealLevel = ceil(-log2(zoom * dpiScale))
/// The browser only requests tiles from `idealLevel - 1` (for HiDPI) to the
/// coarsest level. We subtract 1 here so the server also covers the finer
/// HiDPI level the browser may request.
pub fn compute_min_level(viewport: &Viewport, dpi: f32, levels: u32) -> u32 {
    const BASE_DPI: f32 = 96.0;

    if levels == 0 {
        return 0;
    }

    let dpi_scale = dpi / BASE_DPI;
    let effective_scale = viewport.safe_zoom() * dpi_scale;

    let ideal_level = if effective_scale >= 1.0 {
        0u32
    } else {
        // Use round instead of ceil so the transition to a coarser mip
        // happens at the geometric midpoint between levels, matching the
        // browser's computeIdealLevel.
        let raw = -effective_scale.log2();
        raw.max(0.0).round() as u32
    };

    // Allow two levels finer than ideal so the server pre-fetches tiles
    // that are one level sharper than what the browser currently considers
    // ideal.  This means they're already in the client cache when the user
    // zooms in slightly, and it avoids the appearance of tiles being "one
    // level too coarse".
    let min_level = ideal_level.saturating_sub(2);
    min_level.min(levels - 1)
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
pub fn is_tile_in_viewport(
    viewport: &Viewport,
    image: &ImageDesc,
    x: u32,
    y: u32,
    level: u32,
) -> bool {
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
    dpi: f32,
    image: ImageDesc,
    worker_tx: Sender<RetrieveTileWork>,
    send_tx: Sender<Bytes>,
    nats_client: NatsClient,
    cancel: CancellationToken,
    viewport_ref: Arc<TokioRwLock<Option<Viewport>>>,
    sent: SentTiles,
    client_ip: Option<String>,
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

                let meta = TileMeta { x, y, level };
                let key = meta.index_unchecked();

                // Check if we've already sent this tile to the client
                {
                    let sent_guard = sent.read();
                    if sent_guard.contains_key(&key) {
                        continue;
                    }
                }

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

                // Check if the tile's mip level is appropriate for the current zoom.
                // The browser only uses tiles from (idealLevel - 1) to the coarsest
                // level. Tiles finer than that are wasteful to send.
                let min_level = compute_min_level(&viewport, dpi, image.levels);
                if level < min_level {
                    tracing::debug!(
                        x = x,
                        y = y,
                        level = level,
                        min_level = min_level,
                        "skipping tile: mip level too fine for current zoom"
                    );
                    continue;
                }

                // Mark the tile as sent before dispatching work
                {
                    let now = chrono::Utc::now().timestamp_millis();
                    let mut sent_guard = sent.write();
                    sent_guard
                        .entry(key)
                        .and_modify(|existing| {
                            existing.count += 1;
                            existing.last_requested_at = now;
                            // Don't reset delivered flag
                        })
                        .or_insert_with(|| RequestedTile {
                            count: 1,
                            last_requested_at: now,
                            delivered: false,
                        });
                }

                // Dispatch work to fetch and send the tile
                let work = RetrieveTileWork {
                    slide_id: image.id,
                    slot,
                    cancel: cancel.clone(),
                    tx: send_tx.clone(),
                    meta,
                    client_ip: client_ip.clone(),
                    viewport: viewport_ref.clone(),
                    image,
                    dpi,
                    sent: sent.clone(),
                };

                if let Err(e) = worker_tx.send(work).await {
                    tracing::warn!(error = %e, "failed to dispatch tile work from NATS event");
                }
            }
        }
    }

    Ok(())
}
