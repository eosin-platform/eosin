/**
 * Store for renderer performance metrics, shared between ViewerPane and the main page.
 */
import { writable } from 'svelte/store';

export interface PerformanceMetrics {
  /** Frame render time in milliseconds */
  renderTimeMs: number;
  /** Frames per second (rolling average) */
  fps: number;
  /** Number of visible tiles at current zoom level */
  visibleTiles: number;
  /** Tiles rendered from cache at ideal resolution */
  renderedTiles: number;
  /** Tiles using coarser fallback */
  fallbackTiles: number;
  /** Tiles showing placeholder (missing) */
  placeholderTiles: number;
  /** Cache memory usage in bytes */
  cacheMemoryBytes: number;
  /** Number of tiles still being decoded */
  pendingDecodes: number;
  /** Total tiles received */
  tilesReceived: number;
  /** Cache size (tile count) */
  cacheSize: number;
}

const defaultMetrics: PerformanceMetrics = {
  renderTimeMs: 0,
  fps: 0,
  visibleTiles: 0,
  renderedTiles: 0,
  fallbackTiles: 0,
  placeholderTiles: 0,
  cacheMemoryBytes: 0,
  pendingDecodes: 0,
  tilesReceived: 0,
  cacheSize: 0,
};

export const performanceMetrics = writable<PerformanceMetrics>(defaultMetrics);

export function updatePerformanceMetrics(partial: Partial<PerformanceMetrics>) {
  performanceMetrics.update((current) => ({ ...current, ...partial }));
}

export function resetPerformanceMetrics() {
  performanceMetrics.set(defaultMetrics);
}
