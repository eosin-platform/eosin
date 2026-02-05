/**
 * Viewport state management with reactive updates.
 * Handles pan, zoom, and computes visible tiles.
 */

import { TILE_SIZE } from './cache';
import type { Viewport, ImageDesc } from './protocol';

export interface ViewportState {
  /** Top-left X in image pixels (level 0) */
  x: number;
  /** Top-left Y in image pixels (level 0) */
  y: number;
  /** Viewport width in screen pixels */
  width: number;
  /** Viewport height in screen pixels */
  height: number;
  /** Zoom level (1.0 = 1:1, 0.5 = zoomed out 2x, 2.0 = zoomed in 2x) */
  zoom: number;
}

export interface TileCoord {
  x: number;
  y: number;
  level: number;
}

/**
 * Compute the ideal mip level for the current zoom.
 * Level 0 = full resolution, higher levels = more downsampled.
 */
export function computeIdealLevel(zoom: number, maxLevels: number, dpi: number = 96): number {
  const dpiScale = dpi / 96;
  const effectiveScale = zoom * dpiScale;

  if (effectiveScale >= 1.0) {
    return 0;
  }

  const rawLevel = Math.ceil(-Math.log2(effectiveScale));
  return Math.min(Math.max(0, rawLevel), maxLevels - 1);
}

/**
 * Compute which tiles are visible at a given level for a viewport.
 */
export function visibleTilesForLevel(
  viewport: ViewportState,
  image: ImageDesc,
  level: number
): TileCoord[] {
  const downsample = Math.pow(2, level);
  const pxPerTile = downsample * TILE_SIZE;

  const zoom = Math.max(viewport.zoom, 1e-6);

  // Viewport bounds in level-0 pixels
  const viewX0 = viewport.x;
  const viewY0 = viewport.y;
  const viewX1 = viewport.x + viewport.width / zoom;
  const viewY1 = viewport.y + viewport.height / zoom;

  // Convert to tile indices
  const tilesX = Math.ceil(image.width / pxPerTile);
  const tilesY = Math.ceil(image.height / pxPerTile);

  const minTx = Math.max(0, Math.floor(viewX0 / pxPerTile));
  const minTy = Math.max(0, Math.floor(viewY0 / pxPerTile));
  const maxTx = Math.min(tilesX, Math.ceil(viewX1 / pxPerTile));
  const maxTy = Math.min(tilesY, Math.ceil(viewY1 / pxPerTile));

  const tiles: TileCoord[] = [];
  for (let ty = minTy; ty < maxTy; ty++) {
    for (let tx = minTx; tx < maxTx; tx++) {
      tiles.push({ x: tx, y: ty, level });
    }
  }

  return tiles;
}

/**
 * Compute visible tiles for all relevant mip levels.
 * Returns tiles from finest relevant level to coarsest.
 */
export function computeVisibleTiles(
  viewport: ViewportState,
  image: ImageDesc,
  dpi: number = 96
): TileCoord[] {
  const idealLevel = computeIdealLevel(viewport.zoom, image.levels, dpi);
  const allTiles: TileCoord[] = [];

  // Collect tiles from ideal level up to coarsest
  for (let level = idealLevel; level < image.levels; level++) {
    const tiles = visibleTilesForLevel(viewport, image, level);
    allTiles.push(...tiles);
  }

  return allTiles;
}

/**
 * Clamp viewport position to keep it within image bounds.
 */
export function clampViewport(
  viewport: ViewportState,
  imageWidth: number,
  imageHeight: number
): ViewportState {
  const zoom = Math.max(viewport.zoom, 1e-6);

  // Visible area in image pixels
  const visibleWidth = viewport.width / zoom;
  const visibleHeight = viewport.height / zoom;

  // Clamp position
  let x = viewport.x;
  let y = viewport.y;

  // Don't let the viewport go past the image edges
  const maxX = Math.max(0, imageWidth - visibleWidth);
  const maxY = Math.max(0, imageHeight - visibleHeight);

  x = Math.max(0, Math.min(x, maxX));
  y = Math.max(0, Math.min(y, maxY));

  return { ...viewport, x, y };
}

/**
 * Convert viewport state to the protocol format for sending to server.
 */
export function toProtocolViewport(state: ViewportState): Viewport {
  return {
    x: state.x,
    y: state.y,
    width: state.width,
    height: state.height,
    zoom: state.zoom,
  };
}

/**
 * Compute the screen position and size for a tile at a given level.
 */
export function tileScreenRect(
  tile: TileCoord,
  viewport: ViewportState
): { x: number; y: number; width: number; height: number } {
  const downsample = Math.pow(2, tile.level);
  const pxPerTile = downsample * TILE_SIZE;
  const zoom = Math.max(viewport.zoom, 1e-6);

  // Tile position in level-0 image pixels
  const tileX0 = tile.x * pxPerTile;
  const tileY0 = tile.y * pxPerTile;

  // Convert to screen coordinates
  const screenX = (tileX0 - viewport.x) * zoom;
  const screenY = (tileY0 - viewport.y) * zoom;
  const screenSize = pxPerTile * zoom;

  return {
    x: screenX,
    y: screenY,
    width: screenSize,
    height: screenSize,
  };
}

/** Zoom constraints */
export const MIN_ZOOM = 0.01;
export const MAX_ZOOM = 64;

/**
 * Compute new viewport after zooming around a point.
 */
export function zoomAround(
  viewport: ViewportState,
  screenX: number,
  screenY: number,
  delta: number,
  imageWidth: number,
  imageHeight: number
): ViewportState {
  const oldZoom = viewport.zoom;
  const newZoom = Math.max(MIN_ZOOM, Math.min(MAX_ZOOM, oldZoom * delta));

  if (newZoom === oldZoom) return viewport;

  // Point in image coordinates before zoom
  const imageX = viewport.x + screenX / oldZoom;
  const imageY = viewport.y + screenY / oldZoom;

  // Adjust position so the image point stays under the cursor
  const newX = imageX - screenX / newZoom;
  const newY = imageY - screenY / newZoom;

  return clampViewport(
    { ...viewport, x: newX, y: newY, zoom: newZoom },
    imageWidth,
    imageHeight
  );
}

/**
 * Compute new viewport after panning.
 */
export function pan(
  viewport: ViewportState,
  deltaScreenX: number,
  deltaScreenY: number,
  imageWidth: number,
  imageHeight: number
): ViewportState {
  const zoom = Math.max(viewport.zoom, 1e-6);

  // Convert screen delta to image delta
  const deltaImageX = deltaScreenX / zoom;
  const deltaImageY = deltaScreenY / zoom;

  return clampViewport(
    {
      ...viewport,
      x: viewport.x - deltaImageX,
      y: viewport.y - deltaImageY,
    },
    imageWidth,
    imageHeight
  );
}
