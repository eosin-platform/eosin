/**
 * Frusta client module - WebSocket client for tile streaming service
 */

export { FrustaClient, createFrustaClient } from './client';
export type { FrustaClientOptions, ConnectionState } from './client';
export type { ImageDesc, Viewport, TileData, TileMeta, OpenResponse } from './protocol';
export {
  MessageType,
  UUID_SIZE,
  buildOpenMessage,
  buildUpdateMessage,
  buildCloseMessage,
  buildClearCacheMessage,
  parseOpenResponse,
  parseTileData,
} from './protocol';

export { TileCache, TILE_SIZE, tileKey, tileKeyFromMeta } from './cache';
export type { CachedTile, TileCacheOptions } from './cache';

export {
  computeIdealLevel,
  visibleTilesForLevel,
  computeVisibleTiles,
  clampViewport,
  toProtocolViewport,
  tileScreenRect,
  zoomAround,
  pan,
  MIN_ZOOM,
  MAX_ZOOM,
} from './viewport';
export type { ViewportState, TileCoord } from './viewport';

export { default as TileRenderer } from './TileRenderer.svelte';
