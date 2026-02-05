use rustc_hash::FxHashMap;

use anyhow::{Context, Result, ensure};
use tokio::sync::mpsc::{Receiver, Sender};
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

#[derive(Debug, Clone, Copy)]
pub struct ImageDesc {
    pub id: Uuid,
    pub width: u32,
    pub height: u32,
    pub levels: u32,
}

#[derive(Debug, Clone, Copy)]
pub struct TileMeta {
    pub x: u32,
    pub y: u32,
    pub level: u32,
}

#[derive(Debug, Clone)]
pub struct Tile {
    pub meta: TileMeta,
    pub data: Vec<u8>,
}

#[derive(Debug, Clone, Copy)]
pub struct RequestedTile {
    pub count: u32,
    pub last_requested_at: i64, // ms since epoch
}

type TileKey = u64;

impl TileMeta {
    pub fn index_checked(&self) -> Result<TileKey> {
        let x = self.x as u64;
        let y = self.y as u64;
        let level = self.level as u64;
        ensure!(x <= X_MASK, "x out of range for index packing");
        ensure!(y <= Y_MASK, "y out of range for index packing");
        ensure!(level <= LEVEL_MASK, "level out of range for index packing");
        let index = x | (y << X_BITS) | (level << (X_BITS + Y_BITS));
        Ok(index)
    }

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
    tx: Sender<Tile>,
    cancel_update: Option<CancellationToken>,
    worker_tx: Sender<RetrieveTileWork>,
    dpi: f32,
    candidates: Vec<TileMeta>,
}

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
}

pub struct RetrieveTileWork {
    pub slide_id: Uuid,
    pub cancel: CancellationToken,
    pub tx: Sender<Tile>,
    pub meta: TileMeta,
}

impl ViewManager {
    pub fn new(
        dpi: f32,
        image: ImageDesc,
        worker_tx: Sender<RetrieveTileWork>,
    ) -> (Self, Receiver<Tile>) {
        let (tx, rx) = tokio::sync::mpsc::channel(100);
        (
            Self {
                dpi,
                sent: FxHashMap::default(),
                image,
                tx,
                worker_tx,
                cancel_update: None,
                candidates: Vec::with_capacity(MAX_TILES_PER_UPDATE),
            },
            rx,
        )
    }

    pub fn clear_cache(&mut self) {
        self.sent.clear();
        if let Some(cancel) = self.cancel_update.take() {
            cancel.cancel();
        }
    }

    pub async fn update(&mut self, viewport: &Viewport) -> Result<()> {
        let cancel = CancellationToken::new();
        if let Some(old_cancel) = self.cancel_update.replace(cancel.clone()) {
            old_cancel.cancel();
        }

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

        // Decide which mip levels to consider, coarse → fine or vice versa.
        // For "higher mips first" meaning *more downsampled*, we iterate
        // from coarse (high level index) down to fine (min_level).
        //
        // ASSUMPTION: level 0 = highest resolution, image.levels-1 = coarsest.
        for level in (min_level..self.image.levels).rev() {
            let mut tiles = visible_tiles_for_level(viewport, &self.image, level);
            // Optional: sort center-out so we prioritize tiles near the viewport center.
            let center_x = viewport.x + (viewport.width as f32 / 2.0);
            let center_y = viewport.y + (viewport.height as f32 / 2.0);
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
                slide_id: self.image.id,
                cancel: cancel.clone(),
                tx: self.tx.clone(),
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

/// Compute which tiles (x, y) at a given level intersect the viewport.
fn visible_tiles_for_level(viewport: &Viewport, image: &ImageDesc, level: u32) -> Vec<TileMeta> {
    const TILE_SIZE: f32 = 512.0; // TODO: configurable

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
