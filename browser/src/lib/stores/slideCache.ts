/**
 * Global slide-level tile cache manager.
 *
 * Maintains one TileCache per slideId, shared across all tabs and panes that
 * display the same slide.  Reference-counted: the cache is only disposed when
 * the last consumer releases it.
 */

import { TileCache, type TileCacheOptions } from '$lib/frusta';

const DEFAULT_MAX_TILES = 2000;

interface CacheEntry {
  cache: TileCache;
  refCount: number;
}

const caches = new Map<string, CacheEntry>();

/**
 * Acquire (or create) the shared TileCache for a slide.
 * Increments the internal reference count.
 */
export function acquireCache(slideId: string, opts?: TileCacheOptions): TileCache {
  let entry = caches.get(slideId);
  if (entry) {
    entry.refCount++;
    return entry.cache;
  }
  const cache = new TileCache({ maxTiles: DEFAULT_MAX_TILES, ...opts });
  caches.set(slideId, { cache, refCount: 1 });
  return cache;
}

/**
 * Release a reference to the shared TileCache for a slide.
 * When the last reference is released the cache is cleared and removed.
 */
export function releaseCache(slideId: string): void {
  const entry = caches.get(slideId);
  if (!entry) return;
  entry.refCount--;
  if (entry.refCount <= 0) {
    entry.cache.clear();
    caches.delete(slideId);
  }
}
