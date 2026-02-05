/**
 * Tile cache for storing WebP images received over WebSocket.
 * Provides O(1) lookup by tile key and LRU eviction.
 */

import type { TileMeta } from './protocol';

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

export function tileKeyFromMeta(meta: TileMeta): bigint {
  return tileKey(meta.x, meta.y, meta.level);
}

/** Cached tile entry */
export interface CachedTile {
  meta: TileMeta;
  /** Decoded image blob URL for rendering */
  blobUrl: string;
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
 */
export class TileCache {
  private cache = new Map<bigint, CachedTile>();
  private maxTiles: number;
  private onTileCached: (meta: TileMeta) => void;

  constructor(options: TileCacheOptions = {}) {
    this.maxTiles = options.maxTiles ?? 2000;
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
   * Creates a blob URL for rendering.
   */
  async set(meta: TileMeta, data: Uint8Array): Promise<CachedTile> {
    const key = tileKeyFromMeta(meta);

    // Revoke old blob URL if replacing
    const existing = this.cache.get(key);
    if (existing) {
      URL.revokeObjectURL(existing.blobUrl);
    }

    // Create blob URL from WebP data
    const blob = new Blob([data.slice().buffer], { type: 'image/webp' });
    const blobUrl = URL.createObjectURL(blob);

    const tile: CachedTile = {
      meta,
      blobUrl,
      lastAccessed: Date.now(),
      dataSize: data.length,
    };

    this.cache.set(key, tile);
    this.onTileCached(meta);

    // Evict old tiles if over capacity
    this.evictIfNeeded();

    return tile;
  }

  /** Get cache size */
  get size(): number {
    return this.cache.size;
  }

  /** Clear all cached tiles */
  clear(): void {
    for (const tile of this.cache.values()) {
      URL.revokeObjectURL(tile.blobUrl);
    }
    this.cache.clear();
  }

  /** Clear tiles for a specific level */
  clearLevel(level: number): void {
    for (const [key, tile] of this.cache.entries()) {
      if (tile.meta.level === level) {
        URL.revokeObjectURL(tile.blobUrl);
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
      this.cache.delete(key);
    }
  }
}
