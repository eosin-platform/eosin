<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import type { ImageDesc } from './protocol';
  import { TileCache, TILE_SIZE, type CachedTile } from './cache';
  import {
    type ViewportState,
    type TileCoord,
    computeIdealLevel,
    visibleTilesForLevel,
    tileScreenRect,
  } from './viewport';
  import { TileRetryManager } from './retryManager';
  import type { FrustaClient } from './client';

  interface Props {
    image: ImageDesc;
    viewport: ViewportState;
    cache: TileCache;
    /** Frusta client for requesting tiles */
    client?: FrustaClient;
    /** Slot number for this slide */
    slot?: number;
    /** Force re-render trigger */
    renderTrigger?: number;
  }

  let { image, viewport, cache, client, slot, renderTrigger = 0 }: Props = $props();

  let canvas: HTMLCanvasElement;
  let ctx: CanvasRenderingContext2D | null = null;
  let imageCache = new Map<string, HTMLImageElement>();
  let pendingImages = new Set<string>();
  let animationFrameId: number | null = null;
  let retryManager: TileRetryManager | null = null;

  // Debug mode state
  let shiftHeld = $state(false);
  let mouseX = $state(0);
  let mouseY = $state(0);
  // Forced mip level for debugging (null = normal behavior, 0-9 = force that level)
  let forcedMipLevel: number | null = $state(null);

  function handleKeyDown(e: KeyboardEvent) {
    if (e.key === 'Shift') {
      shiftHeld = true;
    }
    // Number keys 0-9 force that mip level for debugging
    if (e.key >= '0' && e.key <= '9') {
      forcedMipLevel = parseInt(e.key, 10);
    }
  }

  function handleKeyUp(e: KeyboardEvent) {
    if (e.key === 'Shift') {
      shiftHeld = false;
    }
    // Release forced mip level when number key is released
    if (e.key >= '0' && e.key <= '9' && forcedMipLevel === parseInt(e.key, 10)) {
      forcedMipLevel = null;
    }
  }

  function handleMouseMove(e: MouseEvent) {
    const rect = canvas?.getBoundingClientRect();
    if (rect) {
      mouseX = e.clientX - rect.left;
      mouseY = e.clientY - rect.top;
    }
  }

  // Initialize retry manager when client and slot are available
  $effect(() => {
    if (client && slot !== undefined) {
      retryManager = new TileRetryManager({
        onRequestTile: (coord: TileCoord) => {
          if (client && slot !== undefined) {
            client.requestTile(slot, coord.x, coord.y, coord.level);
          }
        },
      });
    } else {
      retryManager?.clear();
      retryManager = null;
    }
  });

  onMount(() => {
    ctx = canvas.getContext('2d');
    window.addEventListener('keydown', handleKeyDown);
    window.addEventListener('keyup', handleKeyUp);
    render();
  });

  onDestroy(() => {
    if (animationFrameId !== null) {
      cancelAnimationFrame(animationFrameId);
    }
    // Clear image cache
    imageCache.clear();
    // Clear retry manager
    retryManager?.clear();
    window.removeEventListener('keydown', handleKeyDown);
    window.removeEventListener('keyup', handleKeyUp);
  });

  // Re-render when viewport, renderTrigger, or debug state changes
  $effect(() => {
    // Access reactive dependencies
    void viewport;
    void renderTrigger;
    void shiftHeld;
    void mouseX;
    void mouseY;
    void forcedMipLevel;
    render();
  });

  function render() {
    if (!ctx || !canvas) return;

    // Handle high DPI displays
    const dpr = window.devicePixelRatio || 1;
    const displayWidth = viewport.width;
    const displayHeight = viewport.height;

    // Set canvas size if needed
    if (canvas.width !== displayWidth * dpr || canvas.height !== displayHeight * dpr) {
      canvas.width = displayWidth * dpr;
      canvas.height = displayHeight * dpr;
      canvas.style.width = `${displayWidth}px`;
      canvas.style.height = `${displayHeight}px`;
      ctx.scale(dpr, dpr);
    }

    // Clear canvas with a background color
    ctx.fillStyle = '#1a1a1a';
    ctx.fillRect(0, 0, displayWidth, displayHeight);

    // Compute the ideal mip level for current zoom
    const dpi = window.devicePixelRatio * 96;
    const idealLevel = computeIdealLevel(viewport.zoom, image.levels, dpi);

    // Compute finer level for 2x screen DPI (one level below ideal, clamped to 0)
    const finerLevel = Math.max(0, idealLevel - 1);

    // Get visible tiles at the ideal level only (for retries, we only request at screen resolution)
    const idealTiles = visibleTilesForLevel(viewport, image, idealLevel);

    // Get visible tiles at finer level for 2x DPI requests
    const finerTiles = finerLevel < idealLevel ? visibleTilesForLevel(viewport, image, finerLevel) : [];

    // Build set of all tiles we want to track (ideal + finer)
    const allTrackableTiles = [...idealTiles, ...finerTiles];

    // Cancel retry tracking for tiles no longer visible at ideal or finer level
    if (retryManager) {
      retryManager.cancelTilesNotIn(allTrackableTiles);
    }

    // Render tiles with mip fallback
    // Strategy: For each tile position at the ideal level,
    // find the best available tile (finest resolution first, then fallback to coarser)
    for (const coord of idealTiles) {
      renderTileWithFallback(coord, idealLevel, finerLevel, forcedMipLevel);
    }

    // Debug overlay when shift is held or mip level is forced
    if (shiftHeld || forcedMipLevel !== null) {
      renderDebugOverlay(idealTiles, idealLevel);
    }
  }

  function renderDebugOverlay(idealTiles: TileCoord[], idealLevel: number) {
    if (!ctx) return;

    // Find the tile under the cursor
    for (const coord of idealTiles) {
      const rect = tileScreenRect(coord, viewport);

      // Check if mouse is within this tile
      if (
        mouseX >= rect.x &&
        mouseX < rect.x + rect.width &&
        mouseY >= rect.y &&
        mouseY < rect.y + rect.height
      ) {
        // Determine what mip level is actually being displayed
        let displayedLevel = coord.level;
        let cachedTile = cache.get(coord.x, coord.y, coord.level);

        if (!cachedTile) {
          // Look for fallback level being used
          for (let level = idealLevel + 1; level < image.levels; level++) {
            const scale = Math.pow(2, level - idealLevel);
            const coarseX = Math.floor(coord.x / scale);
            const coarseY = Math.floor(coord.y / scale);
            const coarse = cache.get(coarseX, coarseY, level);
            if (coarse) {
              displayedLevel = level;
              break;
            }
          }
          // If no fallback found, show -1 to indicate placeholder
          if (!cachedTile && displayedLevel === coord.level) {
            displayedLevel = -1;
          }
        }

        // Draw debug frame around the tile
        ctx.strokeStyle = '#00ff00';
        ctx.lineWidth = 2;
        ctx.strokeRect(rect.x + 1, rect.y + 1, rect.width - 2, rect.height - 2);

        // Prepare mip level label
        const label = displayedLevel === -1 ? 'N/A' : `L${displayedLevel}`;
        const fontSize = 14; // Constant size
        ctx.font = `bold ${fontSize}px monospace`;
        ctx.textAlign = 'center';
        ctx.textBaseline = 'middle';

        // Calculate label position (center of tile)
        let labelX = rect.x + rect.width / 2;
        let labelY = rect.y + rect.height / 2;

        // Measure text for background
        const textMetrics = ctx.measureText(label);
        const textWidth = textMetrics.width + 8;
        const textHeight = fontSize + 6;

        // Clamp label position to keep it on screen
        labelX = Math.max(textWidth / 2 + 4, Math.min(viewport.width - textWidth / 2 - 4, labelX));
        labelY = Math.max(textHeight / 2 + 4, Math.min(viewport.height - textHeight / 2 - 4, labelY));

        // Draw background for label
        ctx.fillStyle = 'rgba(0, 0, 0, 0.8)';
        ctx.fillRect(
          labelX - textWidth / 2,
          labelY - textHeight / 2,
          textWidth,
          textHeight
        );

        // Draw label text
        ctx.fillStyle = displayedLevel === idealLevel ? '#00ff00' : '#ffff00';
        ctx.fillText(label, labelX, labelY);

        // Only highlight one tile (the one under cursor)
        break;
      }
    }
  }

  function renderTileWithFallback(
    targetCoord: TileCoord,
    idealLevel: number,
    finerLevel: number,
    forcedLevel: number | null = null
  ) {
    if (!ctx) return;

    const rect = tileScreenRect(targetCoord, viewport);

    // Skip tiles completely outside the viewport
    if (
      rect.x + rect.width < 0 ||
      rect.y + rect.height < 0 ||
      rect.x > viewport.width ||
      rect.y > viewport.height
    ) {
      return;
    }

    // If forcing a specific mip level, only use that level (render as fallback)
    if (forcedLevel !== null) {
      const clampedLevel = Math.min(forcedLevel, image.levels - 1);
      
      if (clampedLevel === idealLevel) {
        // Forced level matches ideal - render directly if available
        const cachedTile = cache.get(targetCoord.x, targetCoord.y, targetCoord.level);
        if (cachedTile) {
          renderTile(cachedTile, rect);
        } else {
          renderPlaceholder(rect);
        }
      } else if (clampedLevel > idealLevel) {
        // Forced level is coarser - render as fallback (sub-portion of coarser tile)
        const scale = Math.pow(2, clampedLevel - idealLevel);
        const coarseX = Math.floor(targetCoord.x / scale);
        const coarseY = Math.floor(targetCoord.y / scale);
        const coarse = cache.get(coarseX, coarseY, clampedLevel);
        if (coarse) {
          renderFallbackTile(targetCoord, coarse, clampedLevel, idealLevel, rect);
        } else {
          renderPlaceholder(rect);
        }
      } else {
        // Forced level is finer than ideal - compute which finer tiles cover this area
        const scale = Math.pow(2, idealLevel - clampedLevel);
        let anyRendered = false;
        
        for (let dy = 0; dy < scale; dy++) {
          for (let dx = 0; dx < scale; dx++) {
            const finerX = targetCoord.x * scale + dx;
            const finerY = targetCoord.y * scale + dy;
            const finerTile = cache.get(finerX, finerY, clampedLevel);
            
            if (finerTile) {
              // Compute the sub-rect for this finer tile within the target rect
              const subWidth = rect.width / scale;
              const subHeight = rect.height / scale;
              const subRect = {
                x: rect.x + dx * subWidth,
                y: rect.y + dy * subHeight,
                width: subWidth,
                height: subHeight,
              };
              renderTile(finerTile, subRect);
              anyRendered = true;
            }
          }
        }
        
        if (!anyRendered) {
          renderPlaceholder(rect);
        }
      }
      return;
    }

    // Normal rendering with fallback...
    // Try to find the best available tile
    // First check the ideal level
    let cachedTile = cache.get(targetCoord.x, targetCoord.y, targetCoord.level);
    let tileLevel = targetCoord.level;

    // If found at ideal level, mark as received in retry manager and render
    if (cachedTile) {
      if (retryManager) {
        retryManager.tileReceived(targetCoord.x, targetCoord.y, targetCoord.level);
      }
      renderTile(cachedTile, rect);
      return;
    }

    // Tile not found at ideal level - start tracking for retry
    // Track tiles at the ideal level (screen resolution)
    if (retryManager && targetCoord.level === idealLevel) {
      retryManager.trackTile(targetCoord);
    }

    // Also track finer level tiles (up to 2x screen DPI) when ideal tile is missing
    if (retryManager && finerLevel < idealLevel) {
      // Each ideal tile maps to 4 finer tiles (2x2 grid)
      const scale = Math.pow(2, idealLevel - finerLevel);
      for (let dy = 0; dy < scale; dy++) {
        for (let dx = 0; dx < scale; dx++) {
          const finerX = targetCoord.x * scale + dx;
          const finerY = targetCoord.y * scale + dy;
          retryManager.trackTile({ x: finerX, y: finerY, level: finerLevel });
        }
      }
    }

    // If not found at ideal level, look for coarser fallbacks
    for (let level = idealLevel + 1; level < image.levels; level++) {
      // At coarser levels, multiple fine tiles map to one coarse tile
      const scale = Math.pow(2, level - idealLevel);
      const coarseX = Math.floor(targetCoord.x / scale);
      const coarseY = Math.floor(targetCoord.y / scale);

      const coarse = cache.get(coarseX, coarseY, level);
      if (coarse) {
        // Found a fallback - render the appropriate portion
        renderFallbackTile(targetCoord, coarse, level, idealLevel, rect);
        return;
      }
    }

    // No tile available - show placeholder
    renderPlaceholder(rect);
  }

  function renderTile(tile: CachedTile, rect: { x: number; y: number; width: number; height: number }) {
    if (!ctx) return;

    const img = getOrLoadImage(tile);
    if (img && img.complete && img.naturalWidth > 0) {
      ctx.drawImage(img, rect.x, rect.y, rect.width, rect.height);
    } else {
      // Image still loading - show placeholder
      renderPlaceholder(rect);
    }
  }

  function renderFallbackTile(
    targetCoord: TileCoord,
    fallbackTile: CachedTile,
    fallbackLevel: number,
    idealLevel: number,
    targetRect: { x: number; y: number; width: number; height: number }
  ) {
    if (!ctx) return;

    const img = getOrLoadImage(fallbackTile);
    if (!img || !img.complete || img.naturalWidth === 0) {
      renderPlaceholder(targetRect);
      return;
    }

    // Compute which portion of the fallback tile to use
    const scale = Math.pow(2, fallbackLevel - idealLevel);

    // Position within the fallback tile (0 to scale-1)
    const subX = targetCoord.x % scale;
    const subY = targetCoord.y % scale;

    // Source rectangle in the fallback tile
    const srcSize = TILE_SIZE / scale;
    const srcX = subX * srcSize;
    const srcY = subY * srcSize;

    ctx.drawImage(
      img,
      srcX,
      srcY,
      srcSize,
      srcSize,
      targetRect.x,
      targetRect.y,
      targetRect.width,
      targetRect.height
    );
  }

  function renderPlaceholder(rect: { x: number; y: number; width: number; height: number }) {
    if (!ctx) return;

    // Draw a subtle grid pattern for missing tiles
    ctx.fillStyle = '#2a2a2a';
    ctx.fillRect(rect.x, rect.y, rect.width, rect.height);

    ctx.strokeStyle = '#333';
    ctx.lineWidth = 1;
    ctx.strokeRect(rect.x + 0.5, rect.y + 0.5, rect.width - 1, rect.height - 1);
  }

  function getOrLoadImage(tile: CachedTile): HTMLImageElement | null {
    const key = tile.blobUrl;

    // Return cached HTMLImageElement if available
    const existing = imageCache.get(key);
    if (existing) {
      return existing;
    }

    // Don't start duplicate loads
    if (pendingImages.has(key)) {
      return null;
    }

    // Start loading the image
    pendingImages.add(key);
    const img = new Image();

    img.onload = () => {
      pendingImages.delete(key);
      imageCache.set(key, img);
      // Trigger re-render
      render();
    };

    img.onerror = () => {
      pendingImages.delete(key);
      console.error('Failed to load tile image:', key);
    };

    img.src = tile.blobUrl;
    return null;
  }
</script>

<canvas
  bind:this={canvas}
  class="tile-canvas"
  style="width: {viewport.width}px; height: {viewport.height}px"
  onmousemove={handleMouseMove}
></canvas>

<style>
  .tile-canvas {
    display: block;
    image-rendering: auto;
  }
</style>
