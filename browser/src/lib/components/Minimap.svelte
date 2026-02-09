<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import type { ViewportState } from '$lib/frusta/viewport';
  import type { ImageDesc, TileMeta } from '$lib/frusta/protocol';
  import { TileCache, TILE_SIZE, tileKey, type CachedTile } from '$lib/frusta/cache';

  /**
   * Dedicated tile store for the minimap.
   * Stores copies of coarse tiles that won't be evicted when the main cache
   * evicts tiles due to viewport changes. This prevents the minimap from
   * appearing corrupted when the user pans/zooms and causes tile eviction.
   */
  interface MinimapTile {
    meta: TileMeta;
    bitmap: ImageBitmap;
  }

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

  // Dedicated tile store for the minimap - immune to main cache eviction.
  // Only stores tiles from coarse mip levels (high level numbers).
  // This is lightweight since coarse levels have very few tiles.
  let minimapTiles = new Map<bigint, MinimapTile>();

  // Track the image ID to detect when we switch images
  let currentImageId: string | null = null;

  /**
   * Compute the minimum mip level needed for the minimap.
   * We only need tiles from levels where the total tile count is reasonable.
   * For a 200px minimap, we typically only need the top 3-4 mip levels.
   */
  function getMinimapMinLevel(): number {
    // We want to store tiles from levels where there are at most ~64 tiles total.
    // This ensures the minimap always has enough coverage without storing too many tiles.
    const maxTilesPerDimension = 8; // 8x8 = 64 tiles max
    
    for (let level = image.levels - 1; level >= 0; level--) {
      const downsample = Math.pow(2, level);
      const pxPerTile = downsample * TILE_SIZE;
      const tilesX = Math.ceil(image.width / pxPerTile);
      const tilesY = Math.ceil(image.height / pxPerTile);
      
      if (tilesX <= maxTilesPerDimension && tilesY <= maxTilesPerDimension) {
        // Return this level - we'll store tiles from this level and coarser
        return level;
      }
    }
    
    // Fallback: use the coarsest 3 levels
    return Math.max(0, image.levels - 3);
  }

  // Track pending copies to trigger re-render when complete
  let pendingCopies = 0;

  /**
   * Copy a tile from the main cache to our dedicated minimap store.
   * Creates a copy of the ImageBitmap so we own it independently.
   * Returns true if a copy was started.
   */
  function copyTileToMinimapStore(tile: CachedTile): boolean {
    if (!tile.bitmap) return false;
    
    const key = tileKey(tile.meta.x, tile.meta.y, tile.meta.level);
    
    // Skip if we already have this tile
    if (minimapTiles.has(key)) return false;
    
    // Mark this key as pending to avoid duplicate copies
    minimapTiles.set(key, null as any); // Placeholder
    pendingCopies++;
    
    createImageBitmap(tile.bitmap).then(
      (bitmapCopy) => {
        minimapTiles.set(key, {
          meta: tile.meta,
          bitmap: bitmapCopy,
        });
        pendingCopies--;
        // Trigger re-render when copy completes
        if (pendingCopies === 0) {
          renderThumbnail();
        }
      },
      (err) => {
        // Bitmap may have been closed, remove placeholder
        minimapTiles.delete(key);
        pendingCopies--;
        console.debug('Failed to copy tile for minimap:', tile.meta, err);
      }
    );
    
    return true;
  }

  /**
   * Sync tiles from the main cache to our dedicated store.
   * Syncs ALL available tiles so we can composite from coarse to fine.
   * This ensures we always have something to show even if coarse tiles
   * haven't been loaded yet.
   */
  function syncTilesFromCache(): void {
    // Sync all levels from the cache - we'll render coarsest first, finest on top
    for (let level = 0; level < image.levels; level++) {
      const tiles = cache.getTilesForLevel(level);
      for (const tile of tiles) {
        if (tile.bitmap) {
          copyTileToMinimapStore(tile);
        }
      }
    }
  }

  /**
   * Clear the minimap tile store (e.g., when switching images).
   */
  function clearMinimapStore(): void {
    for (const tile of minimapTiles.values()) {
      // Skip null placeholders for pending copies
      if (tile?.bitmap) {
        tile.bitmap.close();
      }
    }
    minimapTiles.clear();
    pendingCopies = 0;
  }

  /**
   * Get the image ID as a string for comparison.
   */
  function getImageIdString(): string {
    return Array.from(image.id).map(b => b.toString(16).padStart(2, '0')).join('');
  }

  // Calculate the scale to fit the image in the minimap (CSS pixels per image pixel)
  // Use $derived.by for clearer semantics
  const scaleValue = $derived.by(() => {
    if (!image.width || !image.height) return 0.001; // Safety fallback
    const scaleX = size / image.width;
    const scaleY = size / image.height;
    return Math.min(scaleX, scaleY);
  });

  // Minimap dimensions in CSS pixels (maintaining aspect ratio)
  const minimapWidth = $derived(image.width * scaleValue);
  const minimapHeight = $derived(image.height * scaleValue);

  // Calculate the viewport rectangle position and size in minimap coordinates
  const viewportRect = $derived.by(() => {
    const s = scaleValue;
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
    currentImageId = getImageIdString();
    renderThumbnail();
  });

  onDestroy(() => {
    // Clean up our dedicated tile store
    clearMinimapStore();
  });

  // Re-render when cache updates or image changes
  $effect(() => {
    void renderTrigger;
    void minimapWidth;
    void minimapHeight;
    
    // Check if we switched to a different image
    const newImageId = getImageIdString();
    if (newImageId !== currentImageId) {
      clearMinimapStore();
      currentImageId = newImageId;
    }
    
    // Sync tiles from main cache to our dedicated store
    syncTilesFromCache();
    
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

    // Render tiles from our dedicated minimap store (immune to main cache eviction)
    // Render from coarsest to finest level so finer tiles draw on top
    
    // Group tiles by level, skipping null placeholders
    const tilesByLevel = new Map<number, MinimapTile[]>();
    for (const tile of minimapTiles.values()) {
      // Skip null placeholders for pending copies
      if (!tile?.bitmap) continue;
      
      const level = tile.meta.level;
      if (!tilesByLevel.has(level)) {
        tilesByLevel.set(level, []);
      }
      tilesByLevel.get(level)!.push(tile);
    }
    
    // Render from coarsest (highest level) to finest (lowest level)
    const levels = Array.from(tilesByLevel.keys()).sort((a, b) => b - a);
    for (const level of levels) {
      const tiles = tilesByLevel.get(level)!;
      renderTilesAtLevel(level, tiles);
    }
  }

  function renderTilesAtLevel(level: number, tiles: MinimapTile[]) {
    if (!ctx) return;

    const s = scaleValue;
    const downsample = Math.pow(2, level);
    const pxPerTile = downsample * TILE_SIZE;

    for (const tile of tiles) {
      // Calculate tile position in minimap coordinates
      const tileX = tile.meta.x * pxPerTile * s;
      const tileY = tile.meta.y * pxPerTile * s;
      
      // Use actual bitmap dimensions to handle edge tiles correctly.
      // Edge tiles may be smaller than TILE_SIZE, and we need to scale
      // based on their actual size, not the full tile size.
      const bitmapWidth = tile.bitmap.width;
      const bitmapHeight = tile.bitmap.height;
      
      // Scale the bitmap dimensions to minimap coordinates
      // The bitmap covers (bitmapWidth * downsample) level-0 pixels
      const destWidth = bitmapWidth * downsample * s;
      const destHeight = bitmapHeight * downsample * s;

      ctx.drawImage(tile.bitmap, tileX, tileY, destWidth, destHeight);
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
    const s = scaleValue;
    const zoom = Math.max(viewport.zoom, 1e-6);
    
    // Visible area in image pixels
    const visibleWidth = viewport.width / zoom;
    const visibleHeight = viewport.height / zoom;
    
    // Mouse position relative to minimap
    const mouseX = e.clientX - rect.left;
    const mouseY = e.clientY - rect.top;
    
    // Convert to image coordinates, centering the viewport on click point
    const newX = (mouseX / s) - (visibleWidth / 2);
    const newY = (mouseY / s) - (visibleHeight / 2);
    
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
    const s = scaleValue;
    const zoom = Math.max(viewport.zoom, 1e-6);
    
    const visibleWidth = viewport.width / zoom;
    const visibleHeight = viewport.height / zoom;
    
    const touchX = touch.clientX - rect.left;
    const touchY = touch.clientY - rect.top;
    
    const newX = (touchX / s) - (visibleWidth / 2);
    const newY = (touchY / s) - (visibleHeight / 2);
    
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
      left: {viewportRect.x}px;
      top: {viewportRect.y}px;
      width: {Math.max(4, viewportRect.width)}px;
      height: {Math.max(4, viewportRect.height)}px;
    "
  ></div>
</div>

<style>
  .minimap {
    position: relative;
    background: rgba(30, 30, 30, 0.9);
    border: 1px solid rgba(255, 255, 255, 0.2);
    border-radius: 4px;
    cursor: pointer;
    overflow: hidden;
    box-shadow: 0 2px 8px rgba(0, 0, 0, 0.4);
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
