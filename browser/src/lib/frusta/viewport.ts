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

  // Use round instead of ceil so the transition to a coarser mip level
  // happens at the geometric midpoint between levels rather than at the
  // exact power-of-2 boundary.  With ceil, zoom = 0.499 jumps to level 2
  // (4x downsampled) even though level 1 (2x) would be nearly pixel-
  // perfect.  With round, that transition happens at zoom ≈ 0.354.
  const rawLevel = Math.round(-Math.log2(effectiveScale));
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
 * Clamp viewport position so that the slide is always at least 50% visible
 * on screen, but allow panning past the edges by a comfortable margin
 * (similar to Photoshop / GIMP). The margin on each axis is capped at
 * half the visible area or half the image size, whichever is smaller.
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

  // Allow panning past each edge by up to this many image pixels.
  // Capped so at least half the image remains on-screen.
  const marginX = Math.min(visibleWidth * 0.5, imageWidth * 0.5);
  const marginY = Math.min(visibleHeight * 0.5, imageHeight * 0.5);

  let minX = -marginX;
  let maxX = imageWidth - visibleWidth + marginX;
  let minY = -marginY;
  let maxY = imageHeight - visibleHeight + marginY;

  // When the image (plus margins) is smaller than the viewport,
  // collapse to the midpoint so the image stays roughly centred.
  if (minX > maxX) {
    minX = maxX = (minX + maxX) / 2;
  }
  if (minY > maxY) {
    minY = maxY = (minY + maxY) / 2;
  }

  const x = Math.max(minX, Math.min(viewport.x, maxX));
  const y = Math.max(minY, Math.min(viewport.y, maxY));

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

/**
 * Compute a centered viewport that fits the entire image within the viewport.
 * Returns a viewport with zoom adjusted to fit and position centered.
 */
export function centerViewport(
  viewportWidth: number,
  viewportHeight: number,
  imageWidth: number,
  imageHeight: number,
  padding: number = 0.9 // 90% of viewport to leave some margin
): ViewportState {
  // Calculate zoom to fit the entire image
  const zoomX = (viewportWidth * padding) / imageWidth;
  const zoomY = (viewportHeight * padding) / imageHeight;
  const zoom = Math.max(MIN_ZOOM, Math.min(MAX_ZOOM, Math.min(zoomX, zoomY)));

  // Calculate visible area at this zoom
  const visibleWidth = viewportWidth / zoom;
  const visibleHeight = viewportHeight / zoom;

  // Center the viewport on the image
  const x = (imageWidth - visibleWidth) / 2;
  const y = (imageHeight - visibleHeight) / 2;

  return {
    x,
    y,
    width: viewportWidth,
    height: viewportHeight,
    zoom,
  };
}
