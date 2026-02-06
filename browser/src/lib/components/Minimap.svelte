<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import type { ViewportState } from '$lib/frusta/viewport';
  import type { ImageDesc } from '$lib/frusta/protocol';
  import { TileCache, TILE_SIZE } from '$lib/frusta/cache';

  interface Props {
    /** Image dimensions and metadata */
    image: ImageDesc;
    /** Current viewport state */
    viewport: ViewportState;
    /** Tile cache containing the image tiles */
    cache: TileCache;
    /** Trigger to re-render when new tiles arrive */
    renderTrigger?: number;
    /** Callback when viewport position changes via drag */
    onViewportChange?: (viewport: ViewportState) => void;
    /** Minimap size in pixels */
    size?: number;
  }

  let { image, viewport, cache, renderTrigger = 0, onViewportChange, size = 200 }: Props = $props();

  let isDragging = $state(false);
  let minimapElement: HTMLDivElement;
  let canvas: HTMLCanvasElement;
  let ctx: CanvasRenderingContext2D | null = null;

  // Calculate the scale to fit the image in the minimap
  const scale = $derived(() => {
    const scaleX = size / image.width;
    const scaleY = size / image.height;
    return Math.min(scaleX, scaleY);
  });

  // Minimap dimensions (maintaining aspect ratio)
  const minimapWidth = $derived(image.width * scale());
  const minimapHeight = $derived(image.height * scale());

  // Calculate the viewport rectangle position and size in minimap coordinates
  const viewportRect = $derived(() => {
    const s = scale();
    const zoom = Math.max(viewport.zoom, 1e-6);
    
    // Visible area in image pixels
    const visibleWidth = viewport.width / zoom;
    const visibleHeight = viewport.height / zoom;
    
    return {
      x: viewport.x * s,
      y: viewport.y * s,
      width: visibleWidth * s,
      height: visibleHeight * s,
    };
  });

  onMount(() => {
    ctx = canvas.getContext('2d');
    renderThumbnail();
  });

  onDestroy(() => {
    // No cleanup needed — bitmaps are owned by the TileCache
  });

  // Re-render when cache updates or image changes
  $effect(() => {
    void renderTrigger;
    void image;
    void minimapWidth;
    void minimapHeight;
    renderThumbnail();
  });

  function renderThumbnail() {
    if (!ctx || !canvas) return;

    const dpr = window.devicePixelRatio || 1;
    const displayWidth = minimapWidth;
    const displayHeight = minimapHeight;

    // Set canvas size if needed
    if (canvas.width !== displayWidth * dpr || canvas.height !== displayHeight * dpr) {
      canvas.width = displayWidth * dpr;
      canvas.height = displayHeight * dpr;
      canvas.style.width = `${displayWidth}px`;
      canvas.style.height = `${displayHeight}px`;
      ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    }

    // Clear canvas with background color
    ctx.fillStyle = '#2a2a2a';
    ctx.fillRect(0, 0, displayWidth, displayHeight);

    // Render all cached tiles from coarsest to finest level
    // This composites all available tiles, with finer tiles drawn on top
    const coarsestLevel = image.levels - 1;
    
    for (let level = coarsestLevel; level >= 0; level--) {
      const tiles = cache.getTilesForLevel(level);
      if (tiles.length > 0) {
        renderTilesAtLevel(level, tiles);
      }
    }
  }

  function renderTilesAtLevel(level: number, tiles: ReturnType<typeof cache.getTilesForLevel>) {
    if (!ctx) return;

    const s = scale();
    const downsample = Math.pow(2, level);
    const pxPerTile = downsample * TILE_SIZE;

    for (const tile of tiles) {
      // Use pre-decoded ImageBitmap for immediate, synchronous drawing.
      // This avoids the async HTMLImageElement loading that caused black tiles.
      if (!tile.bitmap) continue;

      // Calculate tile position in minimap coordinates
      const tileX = tile.meta.x * pxPerTile * s;
      const tileY = tile.meta.y * pxPerTile * s;
      const tileSize = pxPerTile * s;

      ctx.drawImage(tile.bitmap, tileX, tileY, tileSize, tileSize);
    }
  }

  function handleMouseDown(e: MouseEvent) {
    e.preventDefault();
    e.stopPropagation();
    isDragging = true;
    updateViewportFromMouse(e);
    
    window.addEventListener('mousemove', handleMouseMove);
    window.addEventListener('mouseup', handleMouseUp);
  }

  function handleMouseMove(e: MouseEvent) {
    if (!isDragging) return;
    updateViewportFromMouse(e);
  }

  function handleMouseUp() {
    isDragging = false;
    window.removeEventListener('mousemove', handleMouseMove);
    window.removeEventListener('mouseup', handleMouseUp);
  }

  function updateViewportFromMouse(e: MouseEvent) {
    if (!minimapElement || !onViewportChange) return;

    const rect = minimapElement.getBoundingClientRect();
    const s = scale();
    const zoom = Math.max(viewport.zoom, 1e-6);
    
    // Visible area in image pixels
    const visibleWidth = viewport.width / zoom;
    const visibleHeight = viewport.height / zoom;
    
    // Mouse position relative to minimap
    const mouseX = e.clientX - rect.left;
    const mouseY = e.clientY - rect.top;
    
    // Convert to image coordinates, centering the viewport on click point
    let newX = (mouseX / s) - (visibleWidth / 2);
    let newY = (mouseY / s) - (visibleHeight / 2);
    
    // Clamp to image bounds
    const maxX = Math.max(0, image.width - visibleWidth);
    const maxY = Math.max(0, image.height - visibleHeight);
    newX = Math.max(0, Math.min(newX, maxX));
    newY = Math.max(0, Math.min(newY, maxY));
    
    onViewportChange({
      ...viewport,
      x: newX,
      y: newY,
    });
  }

  // Touch support
  function handleTouchStart(e: TouchEvent) {
    e.preventDefault();
    e.stopPropagation();
    if (e.touches.length === 1) {
      isDragging = true;
      updateViewportFromTouch(e.touches[0]);
    }
  }

  function handleTouchMove(e: TouchEvent) {
    if (!isDragging || e.touches.length !== 1) return;
    e.preventDefault();
    updateViewportFromTouch(e.touches[0]);
  }

  function handleTouchEnd() {
    isDragging = false;
  }

  function updateViewportFromTouch(touch: Touch) {
    if (!minimapElement || !onViewportChange) return;

    const rect = minimapElement.getBoundingClientRect();
    const s = scale();
    const zoom = Math.max(viewport.zoom, 1e-6);
    
    const visibleWidth = viewport.width / zoom;
    const visibleHeight = viewport.height / zoom;
    
    const touchX = touch.clientX - rect.left;
    const touchY = touch.clientY - rect.top;
    
    let newX = (touchX / s) - (visibleWidth / 2);
    let newY = (touchY / s) - (visibleHeight / 2);
    
    const maxX = Math.max(0, image.width - visibleWidth);
    const maxY = Math.max(0, image.height - visibleHeight);
    newX = Math.max(0, Math.min(newX, maxX));
    newY = Math.max(0, Math.min(newY, maxY));
    
    onViewportChange({
      ...viewport,
      x: newX,
      y: newY,
    });
  }
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div
  class="minimap"
  bind:this={minimapElement}
  style="width: {minimapWidth}px; height: {minimapHeight}px;"
  onmousedown={handleMouseDown}
  ontouchstart={handleTouchStart}
  ontouchmove={handleTouchMove}
  ontouchend={handleTouchEnd}
>
  <!-- Thumbnail canvas showing the whole slide -->
  <canvas bind:this={canvas} class="thumbnail-canvas"></canvas>
  
  <!-- Viewport rectangle -->
  <div
    class="viewport-rect"
    class:dragging={isDragging}
    style="
      left: {viewportRect().x}px;
      top: {viewportRect().y}px;
      width: {Math.max(4, viewportRect().width)}px;
      height: {Math.max(4, viewportRect().height)}px;
    "
  ></div>
</div>

<style>
  .minimap {
    position: absolute;
    bottom: 16px;
    right: 16px;
    background: rgba(30, 30, 30, 0.9);
    border: 1px solid rgba(255, 255, 255, 0.2);
    border-radius: 4px;
    cursor: pointer;
    overflow: hidden;
    box-shadow: 0 2px 8px rgba(0, 0, 0, 0.4);
    z-index: 100;
    touch-action: none;
  }

  .thumbnail-canvas {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
  }

  .viewport-rect {
    position: absolute;
    border: 2px solid #3b82f6;
    background: rgba(59, 130, 246, 0.2);
    box-sizing: border-box;
    pointer-events: none;
    transition: background 0.1s ease;
  }

  .viewport-rect.dragging {
    background: rgba(59, 130, 246, 0.35);
    border-color: #60a5fa;
  }

  .minimap:hover .viewport-rect {
    border-color: #60a5fa;
  }
</style>
