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

  interface Props {
    image: ImageDesc;
    viewport: ViewportState;
    cache: TileCache;
    /** Force re-render trigger */
    renderTrigger?: number;
  }

  let { image, viewport, cache, renderTrigger = 0 }: Props = $props();

  let canvas: HTMLCanvasElement;
  let ctx: CanvasRenderingContext2D | null = null;
  let imageCache = new Map<string, HTMLImageElement>();
  let pendingImages = new Set<string>();
  let animationFrameId: number | null = null;

  onMount(() => {
    ctx = canvas.getContext('2d');
    render();
  });

  onDestroy(() => {
    if (animationFrameId !== null) {
      cancelAnimationFrame(animationFrameId);
    }
    // Clear image cache
    imageCache.clear();
  });

  // Re-render when viewport or renderTrigger changes
  $effect(() => {
    // Access reactive dependencies
    void viewport;
    void renderTrigger;
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

    // Get visible tiles at the ideal level
    const idealTiles = visibleTilesForLevel(viewport, image, idealLevel);

    // Render tiles with mip fallback
    // Strategy: For each tile position at the ideal level,
    // find the best available tile (finest resolution first, then fallback to coarser)
    for (const coord of idealTiles) {
      renderTileWithFallback(coord, idealLevel);
    }
  }

  function renderTileWithFallback(targetCoord: TileCoord, idealLevel: number) {
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

    // Try to find the best available tile
    // First check the ideal level
    let cachedTile = cache.get(targetCoord.x, targetCoord.y, targetCoord.level);
    let tileLevel = targetCoord.level;

    // If not found at ideal level, look for coarser fallbacks
    if (!cachedTile) {
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
      return;
    }

    // Render the exact tile
    renderTile(cachedTile, rect);
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
></canvas>

<style>
  .tile-canvas {
    display: block;
    image-rendering: auto;
  }
</style>
