/**
 * Tile retry manager - handles exponential backoff for missing tiles.
 * Tracks retry state for tiles that haven't been received within timeout.
 */

import { tileKey } from './cache';
import type { TileCoord } from './viewport';

/** Initial timeout before first retry (ms) */
export const INITIAL_TIMEOUT = 1100;
/** Base delay for exponential backoff (ms) */
export const BASE_RETRY_DELAY = 3000;
/** Maximum jitter added to retry delays (ms) */
export const MAX_JITTER = 200;
/** Maximum retry attempts before giving up */
export const MAX_RETRIES = 10;

/** State for a pending tile */
export interface PendingTile {
  coord: TileCoord;
  /** When we first started waiting for this tile */
  firstRequestedAt: number;
  /** How many times we've retried requesting this tile */
  retryCount: number;
  /** When the next retry is scheduled (0 if not yet scheduled) */
  nextRetryAt: number;
  /** Timeout ID for the initial timeout check */
  initialTimeoutId?: ReturnType<typeof setTimeout>;
  /** Timeout ID for the retry */
  retryTimeoutId?: ReturnType<typeof setTimeout>;
}

export interface TileRetryManagerOptions {
  /** Initial timeout before first retry (default: 1100ms) */
  initialTimeout?: number;
  /** Base delay for retry backoff (default: 3000ms) */
  baseRetryDelay?: number;
  /** Maximum jitter (default: 200ms) */
  maxJitter?: number;
  /** Maximum retries (default: 10) */
  maxRetries?: number;
  /** Callback when a tile should be requested */
  onRequestTile: (coord: TileCoord) => void;
}

/**
 * Manages retry logic for tiles with exponential backoff and jitter.
 */
export class TileRetryManager {
  private pending = new Map<bigint, PendingTile>();
  private initialTimeout: number;
  private baseRetryDelay: number;
  private maxJitter: number;
  private maxRetries: number;
  private onRequestTile: (coord: TileCoord) => void;

  constructor(options: TileRetryManagerOptions) {
    this.initialTimeout = options.initialTimeout ?? INITIAL_TIMEOUT;
    this.baseRetryDelay = options.baseRetryDelay ?? BASE_RETRY_DELAY;
    this.maxJitter = options.maxJitter ?? MAX_JITTER;
    this.maxRetries = options.maxRetries ?? MAX_RETRIES;
    this.onRequestTile = options.onRequestTile;
  }

  /**
   * Calculate delay with exponential backoff and jitter.
   * First retry: 3000ms + jitter
   * Second retry: 6000ms + jitter
   * Third retry: 12000ms + jitter
   * etc.
   */
  private calculateRetryDelay(retryCount: number): number {
    const exponentialDelay = this.baseRetryDelay * Math.pow(2, retryCount);
    const jitter = Math.random() * this.maxJitter;
    return exponentialDelay + jitter;
  }

  /**
   * Start tracking a tile that we're waiting for.
   * Sets up initial timeout to trigger retry if tile isn't received.
   */
  trackTile(coord: TileCoord): void {
    const key = tileKey(coord.x, coord.y, coord.level);
    
    // Already tracking this tile
    if (this.pending.has(key)) {
      return;
    }

    const now = Date.now();
    const pending: PendingTile = {
      coord,
      firstRequestedAt: now,
      retryCount: 0,
      nextRetryAt: 0,
    };

    // Set up initial timeout
    pending.initialTimeoutId = setTimeout(() => {
      this.handleInitialTimeout(key);
    }, this.initialTimeout);

    this.pending.set(key, pending);
  }

  /**
   * Called when the initial timeout fires (tile wasn't received in time).
   */
  private handleInitialTimeout(key: bigint): void {
    const pending = this.pending.get(key);
    if (!pending) return;

    // Clear the timeout ID
    pending.initialTimeoutId = undefined;

    // Schedule first retry
    this.scheduleRetry(key, pending);
  }

  /**
   * Schedule a retry for a pending tile.
   */
  private scheduleRetry(key: bigint, pending: PendingTile): void {
    if (pending.retryCount >= this.maxRetries) {
      // Give up after max retries
      this.pending.delete(key);
      return;
    }

    const delay = this.calculateRetryDelay(pending.retryCount);
    pending.nextRetryAt = Date.now() + delay;

    pending.retryTimeoutId = setTimeout(() => {
      this.executeRetry(key);
    }, delay);
  }

  /**
   * Execute a retry - request the tile again.
   */
  private executeRetry(key: bigint): void {
    const pending = this.pending.get(key);
    if (!pending) return;

    pending.retryCount++;
    pending.retryTimeoutId = undefined;

    // Log the tile request for debugging
    console.log(
      `[TileRetry] Requesting tile (${pending.coord.x}, ${pending.coord.y}) level=${pending.coord.level} retry=${pending.retryCount}`
    );

    // Request the tile
    this.onRequestTile(pending.coord);

    // Schedule next retry (in case this one fails too)
    this.scheduleRetry(key, pending);
  }

  /**
   * Mark a tile as received - stop tracking and cancel any pending retries.
   */
  tileReceived(x: number, y: number, level: number): void {
    const key = tileKey(x, y, level);
    const pending = this.pending.get(key);
    
    if (pending) {
      // Clear any pending timeouts
      if (pending.initialTimeoutId) {
        clearTimeout(pending.initialTimeoutId);
      }
      if (pending.retryTimeoutId) {
        clearTimeout(pending.retryTimeoutId);
      }
      this.pending.delete(key);
    }
  }

  /**
   * Cancel tracking for tiles that are no longer visible.
   * Call this when viewport changes to clean up tiles outside the view.
   */
  cancelTilesNotIn(visibleTiles: TileCoord[]): void {
    const visibleKeys = new Set(
      visibleTiles.map(t => tileKey(t.x, t.y, t.level))
    );

    for (const [key, pending] of this.pending) {
      if (!visibleKeys.has(key)) {
        if (pending.initialTimeoutId) {
          clearTimeout(pending.initialTimeoutId);
        }
        if (pending.retryTimeoutId) {
          clearTimeout(pending.retryTimeoutId);
        }
        this.pending.delete(key);
      }
    }
  }

  /**
   * Get retry statistics for a tile.
   */
  getRetryCount(x: number, y: number, level: number): number {
    const key = tileKey(x, y, level);
    const pending = this.pending.get(key);
    return pending?.retryCount ?? 0;
  }

  /**
   * Check if a tile is being tracked.
   */
  isTracking(x: number, y: number, level: number): boolean {
    const key = tileKey(x, y, level);
    return this.pending.has(key);
  }

  /**
   * Get total number of pending tiles.
   */
  get pendingCount(): number {
    return this.pending.size;
  }

  /**
   * Clear all pending tiles and cancel all timeouts.
   */
  clear(): void {
    for (const pending of this.pending.values()) {
      if (pending.initialTimeoutId) {
        clearTimeout(pending.initialTimeoutId);
      }
      if (pending.retryTimeoutId) {
        clearTimeout(pending.retryTimeoutId);
      }
    }
    this.pending.clear();
  }
}
