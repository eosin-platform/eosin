/**
 * Frusta client module - WebSocket client for tile streaming service
 */

export { FrustaClient, createFrustaClient } from './client';
export type { FrustaClientOptions, ConnectionState } from './client';
export type { ImageDesc, Viewport, TileData, TileMeta, OpenResponse, ProgressEvent } from './protocol';
export {
  MessageType,
  UUID_SIZE,
  buildOpenMessage,
  buildUpdateMessage,
  buildCloseMessage,
  buildClearCacheMessage,
  buildRequestTileMessage,
  parseOpenResponse,
  parseTileData,
  parseProgressEvent,
  isProgressEvent,
  isRateLimited,
} from './protocol';

export { TileCache, TILE_SIZE, tileKey, tileKeyFromMeta } from './cache';
export type { CachedTile, TileCacheOptions } from './cache';

export {
  TileRetryManager,
  INITIAL_TIMEOUT,
  BASE_RETRY_DELAY,
  MAX_JITTER,
  MAX_RETRIES,
} from './retryManager';
export type { PendingTile, TileRetryManagerOptions } from './retryManager';

export {
  computeIdealLevel,
  visibleTilesForLevel,
  computeVisibleTiles,
  clampViewport,
  toProtocolViewport,
  tileScreenRect,
  zoomAround,
  pan,
  centerViewport,
  MIN_ZOOM,
  MAX_ZOOM,
} from './viewport';
export type { ViewportState, TileCoord } from './viewport';

export { default as TileRenderer } from './TileRenderer.svelte';
