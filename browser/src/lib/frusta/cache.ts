/**
 * Tile cache for storing WebP images received over WebSocket.
 * Provides O(1) lookup by tile key and LRU eviction.
 * Supports cancellation of pending decodes for tiles that leave the viewport.
 */

import type { TileMeta } from './protocol';
import type { TileCoord } from './viewport';

/** Tile size in pixels (must match server TILE_SIZE) */
export const TILE_SIZE = 512;

/** Bit layout for tile key (must match server) */
const X_BITS = 20n;
const Y_BITS = 20n;

/**
 * Compute a unique key for a tile, matching the server's TileMeta::index_unchecked.
 */
export function tileKey(x: number, y: number, level: number): bigint {
  const xb = BigInt(x);
  const yb = BigInt(y);
  const lb = BigInt(level);
  return xb | (yb << X_BITS) | (lb << (X_BITS + Y_BITS));
}

/**
 * Tracks a pending decode operation that can be cancelled.
 * Since createImageBitmap doesn't support AbortController, we track
 * cancellation state and discard results for cancelled decodes.
 */
interface PendingDecode {
  /** Set to true when this decode should be discarded on completion */
  cancelled: boolean;
}

export function tileKeyFromMeta(meta: TileMeta): bigint {
  return tileKey(meta.x, meta.y, meta.level);
}

/** Cached tile entry */
export interface CachedTile {
  meta: TileMeta;
  /** Decoded image blob URL for rendering */
  blobUrl: string;
  /** Pre-decoded ImageBitmap ready for immediate canvas drawing (no async load needed) */
  bitmap: ImageBitmap | null;
  /** Timestamp when this tile was last accessed */
  lastAccessed: number;
  /** Original WebP data size in bytes */
  dataSize: number;
}

export interface TileCacheOptions {
  /** Maximum number of tiles to cache (default: 2000) */
  maxTiles?: number;
  /** Called when a new tile is cached */
  onTileCached?: (meta: TileMeta) => void;
}

/**
 * LRU cache for tiles with blob URL management.
 * Supports cancellation of pending decodes when tiles leave the viewport.
 */
export class TileCache {
  private cache = new Map<bigint, CachedTile>();
  /** Track pending decode operations for cancellation */
  private pendingDecodes = new Map<bigint, PendingDecode>();
  private maxTiles: number;
  private onTileCached: (meta: TileMeta) => void;

  constructor(options: TileCacheOptions = {}) {
    this.maxTiles = options.maxTiles ?? 1000;
    this.onTileCached = options.onTileCached ?? (() => {});
  }

  /** Get a tile from the cache, updating its access time */
  get(x: number, y: number, level: number): CachedTile | undefined {
    const key = tileKey(x, y, level);
    const tile = this.cache.get(key);
    if (tile) {
      tile.lastAccessed = Date.now();
    }
    return tile;
  }

  /** Check if a tile exists in the cache */
  has(x: number, y: number, level: number): boolean {
    return this.cache.has(tileKey(x, y, level));
  }

  /**
   * Store a tile in the cache from WebP data.
   *
   * The tile is inserted **synchronously** (with bitmap = null) and
   * `onTileCached` is fired immediately so the renderer can use coarser
   * fallback tiles that are already decoded.  The ImageBitmap is then
   * decoded in the background; when it's ready the entry is updated and
   * `onTileCached` fires a second time so the renderer can swap in the
   * crisp version.  This two-phase approach is what enables progressive
   * "coarse then fine" display.
   *
   * Decodes can be cancelled via `cancelDecodesNotIn()` - if cancelled,
   * the decoded bitmap is discarded when the decode completes, freeing
   * resources and avoiding unnecessary work for tiles that are no longer
   * visible.
   *
   * Returns `{ tile, bitmapReady }` where `bitmapReady` resolves once the
   * ImageBitmap has been decoded and the cache entry updated.
   */
  set(meta: TileMeta, data: Uint8Array): { tile: CachedTile; bitmapReady: Promise<void> } {
    const key = tileKeyFromMeta(meta);

    // Cancel any existing pending decode for this tile (we have fresh data)
    const existingDecode = this.pendingDecodes.get(key);
    if (existingDecode) {
      existingDecode.cancelled = true;
      this.pendingDecodes.delete(key);
    }

    // If this tile already exists with a decoded bitmap, keep it — the
    // existing version is strictly better than starting a fresh decode.
    // Without this guard, re-sent tiles cause a crisp→blurry→crisp
    // flicker because the decoded bitmap is destroyed and re-decoded.
    const existing = this.cache.get(key);
    if (existing?.bitmap) {
      existing.lastAccessed = Date.now();
      return { tile: existing, bitmapReady: Promise.resolve() };
    }

    // Revoke old resources if replacing (tile exists but bitmap is null)
    if (existing) {
      URL.revokeObjectURL(existing.blobUrl);
    }

    // Create blob from WebP data
    const blob = new Blob([data.slice().buffer], { type: 'image/webp' });
    const blobUrl = URL.createObjectURL(blob);

    // Insert immediately with bitmap = null so the tile is "known" to the
    // cache.  The renderer will skip tiles whose bitmap is null and fall
    // back to coarser tiles that ARE decoded.
    const tile: CachedTile = {
      meta,
      blobUrl,
      bitmap: null,
      lastAccessed: Date.now(),
      dataSize: data.length,
    };

    this.cache.set(key, tile);

    // Track this pending decode so it can be cancelled if the tile
    // leaves the viewport before decoding completes
    const pendingDecode: PendingDecode = { cancelled: false };
    this.pendingDecodes.set(key, pendingDecode);

    // Evict old tiles if over capacity
    this.evictIfNeeded();

    // Fire immediately so the renderer can re-evaluate fallbacks.
    this.onTileCached(meta);

    // Decode ImageBitmap in the background.  When ready, patch the entry
    // and notify again so the renderer draws the crisp version.
    const bitmapReady = createImageBitmap(blob).then(
      (bitmap) => {
        // Remove from pending set
        this.pendingDecodes.delete(key);

        // Check if decode was cancelled (tile left viewport during decode)
        if (pendingDecode.cancelled) {
          bitmap.close();
          return;
        }

        // The entry may have been evicted or replaced while we were
        // decoding — only update if it's still the same object.
        const current = this.cache.get(key);
        if (current === tile) {
          tile.bitmap = bitmap;
          this.onTileCached(meta);
        } else {
          bitmap.close();
        }
      },
      (err) => {
        // Remove from pending set on error too
        this.pendingDecodes.delete(key);
        console.error('Failed to decode tile bitmap:', meta, err);
      },
    );

    return { tile, bitmapReady };
  }

  /**
   * Cancel pending decodes for tiles that are not in the visible set.
   * This should be called when the viewport changes to avoid wasting
   * CPU time decoding tiles that are no longer visible.
   *
   * The cancelled decodes will still complete (createImageBitmap can't
   * be aborted), but their results will be discarded immediately,
   * freeing memory and avoiding unnecessary cache updates.
   */
  cancelDecodesNotIn(visibleTiles: TileCoord[]): number {
    const visibleKeys = new Set(
      visibleTiles.map(t => tileKey(t.x, t.y, t.level))
    );

    let cancelled = 0;
    for (const [key, pending] of this.pendingDecodes) {
      if (!visibleKeys.has(key)) {
        pending.cancelled = true;
        this.pendingDecodes.delete(key);
        cancelled++;
      }
    }

    return cancelled;
  }

  /**
   * Cancel all pending decodes.
   * Useful when switching slides or closing the viewer.
   */
  cancelAllPendingDecodes(): number {
    const count = this.pendingDecodes.size;
    for (const pending of this.pendingDecodes.values()) {
      pending.cancelled = true;
    }
    this.pendingDecodes.clear();
    return count;
  }

  /** Get cache size (number of tiles) */
  get size(): number {
    return this.cache.size;
  }

  /** Get total memory usage in bytes (approximate) */
  getMemoryUsage(): number {
    let total = 0;
    for (const tile of this.cache.values()) {
      // dataSize is the compressed WebP size
      total += tile.dataSize;
      // Each decoded ImageBitmap is ~4 bytes per pixel (RGBA)
      if (tile.bitmap) {
        total += TILE_SIZE * TILE_SIZE * 4;
      }
    }
    return total;
  }

  /** Get count of tiles that have decoded bitmaps ready */
  getDecodedCount(): number {
    let count = 0;
    for (const tile of this.cache.values()) {
      if (tile.bitmap) count++;
    }
    return count;
  }

  /** Get count of tiles still being decoded (pending, not cancelled) */
  getPendingDecodeCount(): number {
    return this.pendingDecodes.size;
  }

  /** Clear all cached tiles and cancel pending decodes */
  clear(): void {
    // Cancel all pending decodes first
    this.cancelAllPendingDecodes();
    
    for (const tile of this.cache.values()) {
      URL.revokeObjectURL(tile.blobUrl);
      tile.bitmap?.close();
    }
    this.cache.clear();
  }

  /** Clear tiles for a specific level */
  clearLevel(level: number): void {
    for (const [key, tile] of this.cache.entries()) {
      if (tile.meta.level === level) {
        URL.revokeObjectURL(tile.blobUrl);
        tile.bitmap?.close();
        this.cache.delete(key);
      }
    }
  }

  /**
   * Get all cached tiles for a given level, optionally filtered by viewport bounds.
   */
  getTilesForLevel(level: number): CachedTile[] {
    const tiles: CachedTile[] = [];
    for (const tile of this.cache.values()) {
      if (tile.meta.level === level) {
        tiles.push(tile);
      }
    }
    return tiles;
  }

  /**
   * Find the best available tile for a given position.
   * Prioritizes lower (finer) mip levels, falls back to higher (coarser) ones.
   * Returns the tile and the level it was found at.
   */
  findBestTile(
    x: number,
    y: number,
    targetLevel: number,
    maxLevel: number
  ): { tile: CachedTile; foundLevel: number } | null {
    // First try the target level
    const exact = this.get(x, y, targetLevel);
    if (exact) {
      return { tile: exact, foundLevel: targetLevel };
    }

    // Try coarser levels (higher mip numbers) as fallback
    for (let level = targetLevel + 1; level <= maxLevel; level++) {
      // At coarser levels, multiple fine tiles map to one coarse tile
      const scale = Math.pow(2, level - targetLevel);
      const coarseX = Math.floor(x / scale);
      const coarseY = Math.floor(y / scale);

      const coarse = this.get(coarseX, coarseY, level);
      if (coarse) {
        return { tile: coarse, foundLevel: level };
      }
    }

    return null;
  }

  private evictIfNeeded(): void {
    if (this.cache.size <= this.maxTiles) return;

    // Collect entries sorted by last accessed time (oldest first)
    const entries = Array.from(this.cache.entries()).sort(
      (a, b) => a[1].lastAccessed - b[1].lastAccessed
    );

    // Remove oldest entries until we're at 80% capacity
    const targetSize = Math.floor(this.maxTiles * 0.8);
    const toRemove = this.cache.size - targetSize;

    for (let i = 0; i < toRemove && i < entries.length; i++) {
      const [key, tile] = entries[i];
      URL.revokeObjectURL(tile.blobUrl);
      tile.bitmap?.close();
      this.cache.delete(key);
    }
  }
}
