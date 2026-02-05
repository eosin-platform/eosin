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

pub struct ImageDesc {
    pub width: u32,
    pub height: u32,
    pub levels: u32,
}

pub struct TileMeta {
    pub x: u32,
    pub y: u32,
    pub level: u32,
}

pub struct Tile {
    pub meta: TileMeta,
    pub data: Vec<u8>,
}

pub struct SentTile {
    pub send_count: u32,
    pub last_sent: i64, // ms since epoch
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
    sent: FxHashMap<TileKey, SentTile>,
    image: ImageDesc,
    tx: Sender<Tile>,
    cancel_update: Option<CancellationToken>,
    worker_tx: Sender<RetrieveTileWork>,
    dpi: f32,
}

pub struct Viewport {
    pub x: f32,
    pub y: f32,
    pub width: u32,
    pub height: u32,
    pub zoom: f32,
}

pub struct RetrieveTileWork {
    pub id: Uuid,
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
            },
            rx,
        )
    }

    pub async fn clear_cache(&mut self) {
        self.sent.clear();
    }

    pub async fn update(&mut self, viewport: &Viewport) -> Result<()> {
        let cancel = CancellationToken::new();
        if let Some(old_cancel) = self.cancel_update.replace(cancel.clone()) {
            old_cancel.cancel();
        }

        const MAX_TILES_PER_UPDATE: usize = 64; // tune this
        let now = chrono::Utc::now().timestamp_millis();

        let mut candidates: Vec<TileMeta> = Vec::new();

        // Compute the minimum mip level worth fetching based on zoom and DPI.
        // When zoomed out, there's no point fetching tiles at resolutions higher
        // than the display can actually render.
        //
        // The effective scale combines zoom with DPI normalization (assuming 96 DPI baseline).
        // Each mip level is 2x downsampled, so min_level = -log2(effective_scale).
        let min_level = self.compute_min_level(viewport);

        // Decide which mip levels to consider, coarse → fine or vice versa.
        // For "higher mips first" meaning *more downsampled*, we iterate
        // from coarse (high level index) down to fine (min_level).
        //
        // ASSUMPTION: level 0 = highest resolution, image.levels-1 = coarsest.
        for level in (min_level..self.image.levels).rev() {
            let mut tiles = self.visible_tiles_for_level(viewport, level);
            // Optional: sort center-out so we prioritize tiles near the viewport center.
            let center_x = viewport.x + (viewport.width as f32 / 2.0);
            let center_y = viewport.y + (viewport.height as f32 / 2.0);
            tiles.sort_by(|a, b| {
                let ax = a.x as f32 * 512.0; // TODO: TILE_SIZE
                let ay = a.y as f32 * 512.0;
                let bx = b.x as f32 * 512.0;
                let by = b.y as f32 * 512.0;
                let da = (ax - center_x).hypot(ay - center_y);
                let db = (bx - center_x).hypot(by - center_y);
                da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
            });
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
        for meta in candidates {
            self.mark_tile_sent_at(&meta, now);
            let work = RetrieveTileWork {
                cancel: cancel.clone(),
                tx: self.tx.clone(),
                meta,
            };
            self.worker_tx
                .send(work)
                .await
                .context("failed to send tile retrieval work")?;
        }
        Ok(())
    }

    fn mark_tile_sent_at(&mut self, tile: &TileMeta, timestamp: i64) {
        let key = tile.index_unchecked();
        self.sent
            .entry(key)
            .and_modify(|existing| {
                existing.send_count += 1;
                existing.last_sent = timestamp;
            })
            .or_insert_with(|| SentTile {
                send_count: 1,
                last_sent: timestamp,
            });
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

        // Effective scale combines zoom with DPI normalization.
        // A higher DPI display can make use of finer detail at the same zoom level.
        let dpi_scale = self.dpi / BASE_DPI;
        let effective_scale = viewport.zoom * dpi_scale;

        // Each mip level is 2x downsampled, so we want the level where
        // 2^level >= 1 / effective_scale, i.e., level >= -log2(effective_scale).
        // When effective_scale >= 1.0, min_level = 0 (use full resolution).
        // When effective_scale = 0.5, min_level = 1 (skip level 0).
        // When effective_scale = 0.25, min_level = 2 (skip levels 0 and 1).
        let min_level = if effective_scale >= 1.0 {
            0
        } else {
            (-effective_scale.log2()).ceil() as u32
        };

        // Clamp to valid range: can't exceed the coarsest level available.
        min_level.min(self.image.levels.saturating_sub(1))
    }

    /// Compute which tiles (x, y) at a given level intersect the viewport.
    fn visible_tiles_for_level(&self, viewport: &Viewport, level: u32) -> Vec<TileMeta> {
        const TILE_SIZE: f32 = 512.0; // TODO: make this configurable

        // ASSUMPTION:
        // - viewport.x, viewport.y are in *base-level* pixel coordinates (level 0 = highest res).
        // - Each level is downsampled by approximately 2^level in each dimension.
        //
        // Adjust this if your pyramid uses a different scheme.
        let downsample = 2f32.powi(level as i32);

        let inv_tile_size = 1.0 / TILE_SIZE;

        let view_x0 = viewport.x / (downsample * TILE_SIZE);
        let view_y0 = viewport.y / (downsample * TILE_SIZE);
        let view_x1 =
            (viewport.x + viewport.width as f32 / viewport.zoom) / (downsample * TILE_SIZE);
        let view_y1 =
            (viewport.y + viewport.height as f32 / viewport.zoom) / (downsample * TILE_SIZE);

        // Clamp to image bounds (in tiles)
        let max_tiles_x = (self.image.width as f32 / (downsample * TILE_SIZE))
            .ceil()
            .max(0.0);
        let max_tiles_y = (self.image.height as f32 / (downsample * TILE_SIZE))
            .ceil()
            .max(0.0);

        let min_tx = view_x0.floor().max(0.0) as u32;
        let min_ty = view_y0.floor().max(0.0) as u32;
        let max_tx = view_x1.ceil().min(max_tiles_x) as u32;
        let max_ty = view_y1.ceil().min(max_tiles_y) as u32;

        let mut tiles = Vec::new();
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
}
