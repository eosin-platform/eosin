/**
 * Tile cache for storing WebP images received over WebSocket.
 * Provides O(1) lookup by tile key and LRU eviction.
 * Supports cancellation of pending decodes for tiles that leave the viewport.
 */

import type { TileMeta, ImageDesc } from './protocol';
import type { TileCoord, ViewportState } from './viewport';

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
  /** Maximum memory usage in bytes (default: 384MB) */
  maxMemoryBytes?: number;
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
  /** Maximum memory usage in bytes (default: 384MB) */
  private maxMemoryBytes: number;
  /** Cached memory usage to avoid recalculating on every insert */
  private currentMemoryBytes: number = 0;
  private onTileCached: (meta: TileMeta) => void;

  /** Current viewport context for protecting coarse tiles during eviction */
  private viewportContext: { viewport: ViewportState; image: ImageDesc } | null = null;

  /** Default memory limit: 384MB */
  static readonly DEFAULT_MAX_MEMORY_BYTES = 384 * 1024 * 1024;

  constructor(options: TileCacheOptions = {}) {
    this.maxTiles = options.maxTiles ?? 1000;
    this.maxMemoryBytes = options.maxMemoryBytes ?? TileCache.DEFAULT_MAX_MEMORY_BYTES;
    this.onTileCached = options.onTileCached ?? (() => {});
  }

  /**
   * Update the current viewport context.
   * This is used during eviction to protect coarse tiles (higher mip levels)
   * that intersect the viewport, ensuring smooth zoom-out behavior.
   */
  setViewportContext(viewport: ViewportState, image: ImageDesc): void {
    this.viewportContext = { viewport, image };
  }

  /**
   * Clear the viewport context (e.g., when switching slides).
   */
  clearViewportContext(): void {
    this.viewportContext = null;
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
      // Subtract old tile's memory before replacing
      this.currentMemoryBytes -= this.getTileMemorySize(existing);
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

    // Track memory for the new tile (just the compressed data for now)
    this.currentMemoryBytes += tile.dataSize;

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
          // Add the decoded bitmap memory (RGBA, 4 bytes per pixel)
          this.currentMemoryBytes += TILE_SIZE * TILE_SIZE * 4;
          tile.bitmap = bitmap;
          // Evict if the new bitmap pushed us over the limit
          this.evictIfNeeded();
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
    return this.currentMemoryBytes;
  }

  /** Recalculate memory usage from scratch (for debugging/verification) */
  private recalculateMemoryUsage(): number {
    let total = 0;
    for (const tile of this.cache.values()) {
      total += this.getTileMemorySize(tile);
    }
    return total;
  }

  /** Get the memory size of a single tile */
  private getTileMemorySize(tile: CachedTile): number {
    // dataSize is the compressed WebP size (kept in memory as blob)
    let size = tile.dataSize;
    // Each decoded ImageBitmap is ~4 bytes per pixel (RGBA)
    if (tile.bitmap) {
      size += TILE_SIZE * TILE_SIZE * 4;
    }
    return size;
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
    this.currentMemoryBytes = 0;
  }

  /** Clear tiles for a specific level */
  clearLevel(level: number): void {
    for (const [key, tile] of this.cache.entries()) {
      if (tile.meta.level === level) {
        this.currentMemoryBytes -= this.getTileMemorySize(tile);
        URL.revokeObjectURL(tile.blobUrl);
        tile.bitmap?.close();
        this.cache.delete(key);
      }
    }
    // Ensure memory tracking doesn't go negative
    if (this.currentMemoryBytes < 0) {
      this.currentMemoryBytes = 0;
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

  /**
   * Compute the set of tile keys that should be protected from eviction.
   * These are tiles at higher mip levels (coarser resolution) that intersect
   * the current viewport. Protecting them ensures smooth zoom-out behavior.
   */
  private computeProtectedTileKeys(): Set<bigint> {
    const protectedKeys = new Set<bigint>();
    
    if (!this.viewportContext) {
      return protectedKeys;
    }
    
    const { viewport, image } = this.viewportContext;
    const zoom = Math.max(viewport.zoom, 1e-6);
    
    // Viewport bounds in level-0 pixels
    const viewX0 = viewport.x;
    const viewY0 = viewport.y;
    const viewX1 = viewport.x + viewport.width / zoom;
    const viewY1 = viewport.y + viewport.height / zoom;
    
    // Protect tiles at all mip levels (from level 0 to the coarsest level)
    // that intersect the current viewport
    for (let level = 0; level < image.levels; level++) {
      const downsample = Math.pow(2, level);
      const pxPerTile = downsample * TILE_SIZE;
      
      // Convert viewport bounds to tile indices at this level
      const tilesX = Math.ceil(image.width / pxPerTile);
      const tilesY = Math.ceil(image.height / pxPerTile);
      
      const minTx = Math.max(0, Math.floor(viewX0 / pxPerTile));
      const minTy = Math.max(0, Math.floor(viewY0 / pxPerTile));
      const maxTx = Math.min(tilesX, Math.ceil(viewX1 / pxPerTile));
      const maxTy = Math.min(tilesY, Math.ceil(viewY1 / pxPerTile));
      
      // Add all tiles at this level that intersect the viewport
      for (let ty = minTy; ty < maxTy; ty++) {
        for (let tx = minTx; tx < maxTx; tx++) {
          protectedKeys.add(tileKey(tx, ty, level));
        }
      }
    }
    
    return protectedKeys;
  }

  private evictIfNeeded(): void {
    // Check both tile count and memory limits
    const overTileLimit = this.cache.size > this.maxTiles;
    const overMemoryLimit = this.currentMemoryBytes > this.maxMemoryBytes;
    
    if (!overTileLimit && !overMemoryLimit) return;

    // Build a set of protected tile keys (coarse tiles intersecting viewport)
    const protectedKeys = this.computeProtectedTileKeys();

    // Collect entries sorted by last accessed time (oldest first)
    const entries = Array.from(this.cache.entries()).sort(
      (a, b) => a[1].lastAccessed - b[1].lastAccessed
    );

    // Calculate target thresholds (80% of limits)
    const targetTileCount = Math.floor(this.maxTiles * 0.8);
    const targetMemoryBytes = Math.floor(this.maxMemoryBytes * 0.8);

    // Evict tiles until we're under both thresholds
    for (const [key, tile] of entries) {
      // Stop if we're under both limits
      if (this.cache.size <= targetTileCount && this.currentMemoryBytes <= targetMemoryBytes) {
        break;
      }

      // Skip protected tiles (coarse tiles intersecting viewport)
      if (protectedKeys.has(key)) {
        continue;
      }

      // Cancel any pending decode for this tile
      const pendingDecode = this.pendingDecodes.get(key);
      if (pendingDecode) {
        pendingDecode.cancelled = true;
        this.pendingDecodes.delete(key);
      }

      // Update memory tracking before removing
      this.currentMemoryBytes -= this.getTileMemorySize(tile);

      URL.revokeObjectURL(tile.blobUrl);
      tile.bitmap?.close();
      this.cache.delete(key);
    }

    // Ensure memory tracking doesn't go negative due to rounding
    if (this.currentMemoryBytes < 0) {
      this.currentMemoryBytes = 0;
    }
  }
}
