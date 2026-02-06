<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import type { ImageDesc } from './protocol';
  import { TileCache, TILE_SIZE, type CachedTile, tileKeyFromMeta } from './cache';
  import {
    type ViewportState,
    type TileCoord,
    computeIdealLevel,
    visibleTilesForLevel,
    tileScreenRect,
  } from './viewport';
  import { TileRetryManager } from './retryManager';
  import type { FrustaClient } from './client';
  import { createEnhancedBitmap } from './stainEnhancement';
import {
  createNormalizedBitmap,
  getOrComputeNormalizationParams,
  type NormalizationParams,
  type StainNormalizationMode,
} from './stainNormalization';
import { settings, type StainEnhancementMode, type StainNormalization } from '$lib/stores/settings';
import { getProcessingPool, type ProcessingWorkerPool } from './processingPool';
  /** Performance metrics exposed via callback */
  export interface RenderMetrics {
    /** Last render time in milliseconds */
    renderTimeMs: number;
    /** Frames per second (rolling average) */
    fps: number;
    /** Number of visible tiles at current level */
    visibleTiles: number;
    /** Number of visible tiles rendered from cache */
    renderedTiles: number;
    /** Number of tiles using fallback (coarser level) */
    fallbackTiles: number;
    /** Number of tiles showing placeholder */
    placeholderTiles: number;
  }

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
    /** Stain normalization mode (Macenko/Vahadane) */
    stainNormalization?: StainNormalization;
    /** Stain enhancement mode for post-processing */
    stainEnhancement?: StainEnhancementMode;
    /** Callback for performance metrics */
    onMetrics?: (metrics: RenderMetrics) => void;
  }

  let { image, viewport, cache, client, slot, renderTrigger = 0, stainNormalization = 'none', stainEnhancement = 'none', onMetrics }: Props = $props();

  let canvas: HTMLCanvasElement;
  let ctx: CanvasRenderingContext2D | null = null;
  let animationFrameId: number | null = null;
  let renderScheduled = false;
  
  // FPS tracking
  let lastFrameTime = 0;
  let frameTimesMs: number[] = [];
  const FPS_SAMPLE_SIZE = 30;
  let retryManager: TileRetryManager | null = null;
  let checkerboardPattern: CanvasPattern | null = null;
  
  // Worker pool for off-main-thread processing
  let workerPool: ProcessingWorkerPool | null = null;
  
  // Zoom detection for worker pool throttling
  let lastZoom = viewport.zoom;
  let zoomChangeTimer: number | null = null;

  /** Convert UUID bytes to hex string for use as cache key */
  function uuidToString(uuid: Uint8Array | undefined): string {
    if (!uuid) return 'unknown';
    return Array.from(uuid).map(b => b.toString(16).padStart(2, '0')).join('');
  }
  
  /** Get the current slide ID as a string */
  function getSlideId(): string {
    return uuidToString(image.id);
  }

  // ============================================================================
  // Processed Bitmap Cache (Normalization + Enhancement)
  // ============================================================================
  // Caches processed ImageBitmaps keyed by (tileKey, normMode, enhanceMode) to avoid
  // re-computing stain normalization and enhancement every frame. This is critical
  // for performance since getImageData/putImageData are slow GPU→CPU→GPU round-trips.
  //
  // Processing order: Normalization → Enhancement (normalization standardizes colors
  // first, then enhancement can be applied on top for specific stain visibility).
  
  interface ProcessedBitmapEntry {
    bitmap: ImageBitmap;
    normMode: StainNormalizationMode;
    enhanceMode: StainEnhancementMode;
    lastAccessed: number;
  }
  
  /** Cache for processed bitmaps: key is `${tileKey}_${normMode}_${enhanceMode}` */
  const processedBitmapCache = new Map<string, ProcessedBitmapEntry>();
  /** Track pending processing computations to avoid duplicate work */
  const pendingProcessing = new Set<string>();
  /** Maximum processed bitmaps to cache (separate from main tile cache) */
  const MAX_PROCESSED_CACHE_SIZE = 500;
  
  /** Cached normalization parameters per slide */
  let cachedNormParams: NormalizationParams | null = null;
  let cachedNormSlideId: string | null = null;
  let cachedNormMode: StainNormalizationMode | null = null;
  
  // Sharpening settings from store (reactive)
  let sharpeningEnabled = $derived($settings.image.sharpeningEnabled);
  let sharpeningIntensity = $derived($settings.image.sharpeningIntensity);
  
  /** Generate cache key for processed bitmap (includes slideId to avoid cross-slide collisions) */
  function processedCacheKey(
    tileKey: bigint,
    normMode: StainNormalizationMode,
    enhanceMode: StainEnhancementMode,
    slideId: string,
    sharpeningEnabled: boolean,
    sharpeningIntensity: number
  ): string {
    // Include sharpening in key so tiles are re-processed when sharpening settings change
    const sharpenKey = sharpeningEnabled ? `sharp${sharpeningIntensity}` : 'nosharp';
    return `${slideId}_${tileKey}_${normMode}_${enhanceMode}_${sharpenKey}`;
  }
  
  /** Get cached processed bitmap if available */
  function getProcessedBitmap(
    tile: CachedTile,
    normMode: StainNormalizationMode,
    enhanceMode: StainEnhancementMode,
    slideId: string
  ): ImageBitmap | null {
    const key = processedCacheKey(
      tileKeyFromMeta(tile.meta),
      normMode,
      enhanceMode,
      slideId,
      sharpeningEnabled,
      sharpeningIntensity
    );
    const entry = processedBitmapCache.get(key);
    if (entry && entry.normMode === normMode && entry.enhanceMode === enhanceMode) {
      entry.lastAccessed = Date.now();
      return entry.bitmap;
    }
    return null;
  }
  
  /**
   * Get normalization parameters, computing them if needed.
   * Uses the first tile encountered for parameter estimation.
   */
  function getNormalizationParams(
    tile: CachedTile,
    normMode: StainNormalizationMode,
    slideId: string
  ): NormalizationParams | null {
    if (normMode === 'none') return null;
    
    // Return cached params if they match
    if (cachedNormParams && cachedNormSlideId === slideId && cachedNormMode === normMode) {
      return cachedNormParams;
    }
    
    // Need to compute - extract pixels from tile bitmap
    if (!tile.bitmap) return null;
    
    // Create a temporary canvas to extract pixel data
    const canvas = document.createElement('canvas');
    canvas.width = tile.bitmap.width;
    canvas.height = tile.bitmap.height;
    const ctx = canvas.getContext('2d');
    if (!ctx) return null;
    
    ctx.drawImage(tile.bitmap, 0, 0);
    const imageData = ctx.getImageData(0, 0, canvas.width, canvas.height);
    
    // Compute normalization parameters
    cachedNormParams = getOrComputeNormalizationParams(slideId, imageData.data, normMode);
    cachedNormSlideId = slideId;
    cachedNormMode = normMode;
    
    return cachedNormParams;
  }
  
  /**
   * Calculate priority for a tile based on distance from viewport center.
   * Lower values = higher priority (processed first).
   */
  function calculateTilePriority(tile: CachedTile): number {
    const viewportCenterX = viewport.x + (viewport.width / viewport.zoom) / 2;
    const viewportCenterY = viewport.y + (viewport.height / viewport.zoom) / 2;
    
    const downsample = Math.pow(2, tile.meta.level);
    const pxPerTile = downsample * TILE_SIZE;
    const tileCenterX = (tile.meta.x + 0.5) * pxPerTile;
    const tileCenterY = (tile.meta.y + 0.5) * pxPerTile;
    
    const dx = tileCenterX - viewportCenterX;
    const dy = tileCenterY - viewportCenterY;
    return Math.sqrt(dx * dx + dy * dy);
  }

  /** Start async processing computation for a tile (normalization + enhancement + sharpening) */
  function scheduleProcessing(
    tile: CachedTile,
    normMode: StainNormalizationMode,
    enhanceMode: StainEnhancementMode,
    slideId: string
  ): void {
    if (!tile.bitmap) return;
    
    // Skip if no processing needed (no normalization, enhancement, or sharpening)
    const needsProcessing = normMode !== 'none' || enhanceMode !== 'none' || 
      (sharpeningEnabled && sharpeningIntensity > 0);
    if (!needsProcessing) return;
    
    const tileKey = tileKeyFromMeta(tile.meta);
    const key = processedCacheKey(
      tileKey,
      normMode,
      enhanceMode,
      slideId,
      sharpeningEnabled,
      sharpeningIntensity
    );
    
    // Skip if already cached or in progress
    if (processedBitmapCache.has(key) || pendingProcessing.has(key)) {
      return;
    }
    
    pendingProcessing.add(key);
    
    // Calculate priority (tiles closer to viewport center are processed first)
    const priority = calculateTilePriority(tile);
    
    // Process bitmap using worker pool (off main thread)
    processImageBitmapWithWorker(tile, normMode, enhanceMode, slideId, key, priority)
      .then((processedBitmap) => {
        pendingProcessing.delete(key);
        
        // Evict old entries if over limit before adding
        evictProcessedCacheIfNeeded();
        
        processedBitmapCache.set(key, {
          bitmap: processedBitmap,
          normMode,
          enhanceMode,
          lastAccessed: Date.now(),
        });
        
        // Trigger re-render to display the processed bitmap
        scheduleRender();
      })
      .catch((err) => {
        pendingProcessing.delete(key);
        // Don't warn on cancellation (expected during rapid zoom)
        if (err?.message !== 'Cancelled') {
          console.warn('Failed to process bitmap:', err);
        }
      });
  }
  
  /**
   * Process an ImageBitmap using the worker pool.
   * Offloads normalization and enhancement to a background thread.
   */
  async function processImageBitmapWithWorker(
    tile: CachedTile,
    normMode: StainNormalizationMode,
    enhanceMode: StainEnhancementMode,
    slideId: string,
    key: string,
    priority: number
  ): Promise<ImageBitmap> {
    if (!tile.bitmap) throw new Error('No bitmap to process');
    
    // Get normalization params (computed on main thread, cached)
    const normParams = normMode !== 'none' 
      ? getNormalizationParams(tile, normMode, slideId)
      : null;
    
    // Extract ImageData from the bitmap for worker processing
    const width = tile.bitmap.width;
    const height = tile.bitmap.height;
    
    // Use OffscreenCanvas if available for better performance
    let imageData: ImageData;
    if (typeof OffscreenCanvas !== 'undefined') {
      const offscreen = new OffscreenCanvas(width, height);
      const ctx = offscreen.getContext('2d');
      if (!ctx) throw new Error('Failed to get 2D context');
      ctx.drawImage(tile.bitmap, 0, 0);
      imageData = ctx.getImageData(0, 0, width, height);
    } else {
      const tempCanvas = document.createElement('canvas');
      tempCanvas.width = width;
      tempCanvas.height = height;
      const tempCtx = tempCanvas.getContext('2d');
      if (!tempCtx) throw new Error('Failed to get 2D context');
      tempCtx.drawImage(tile.bitmap, 0, 0);
      imageData = tempCtx.getImageData(0, 0, width, height);
    }
    
    // Get worker pool and process
    const pool = workerPool ?? getProcessingPool();
    const processedData = await pool.process(
      key,
      imageData,
      normMode,
      enhanceMode,
      normParams,
      priority
    );
    
    // Convert back to ImageBitmap
    return createImageBitmap(processedData);
  }

  /**
   * Process an ImageBitmap by applying normalization then enhancement.
   * Fallback for when worker pool is not available.
   * Returns a new ImageBitmap with both transformations applied.
   */
  async function processImageBitmap(
    source: ImageBitmap,
    normMode: StainNormalizationMode,
    enhanceMode: StainEnhancementMode,
    slideId: string
  ): Promise<ImageBitmap> {
    let result = source;
    
    // Step 1: Apply normalization (if enabled)
    if (normMode !== 'none') {
      // Get or compute normalization params
      const params = getNormalizationParams({ bitmap: source, meta: { x: 0, y: 0, level: 0 } } as CachedTile, normMode, slideId);
      result = await createNormalizedBitmap(source, normMode, params);
    }
    
    // Step 2: Apply enhancement (if enabled)
    if (enhanceMode !== 'none') {
      const enhanced = await createEnhancedBitmap(result, enhanceMode);
      // Close intermediate result if we created it
      if (result !== source) {
        result.close();
      }
      result = enhanced;
    }
    
    return result;
  }
  
  /** Evict oldest entries from processed cache when over limit */
  function evictProcessedCacheIfNeeded(): void {
    if (processedBitmapCache.size < MAX_PROCESSED_CACHE_SIZE) return;
    
    // Sort by last accessed (oldest first)
    const entries = Array.from(processedBitmapCache.entries())
      .sort((a, b) => a[1].lastAccessed - b[1].lastAccessed);
    
    // Evict until under 80% of limit
    const targetSize = Math.floor(MAX_PROCESSED_CACHE_SIZE * 0.8);
    while (processedBitmapCache.size > targetSize && entries.length > 0) {
      const [key, entry] = entries.shift()!;
      entry.bitmap.close();
      processedBitmapCache.delete(key);
    }
  }
  
  /** Clear processed cache (called on destroy or slide change) */
  function clearProcessedCache(): void {
    for (const entry of processedBitmapCache.values()) {
      entry.bitmap.close();
    }
    processedBitmapCache.clear();
    pendingProcessing.clear();
    
    // Also clear normalization params cache
    cachedNormParams = null;
    cachedNormSlideId = null;
    cachedNormMode = null;
  }
  
  // Legacy aliases for backward compatibility (used in existing code)
  const enhancedBitmapCache = processedBitmapCache;
  const pendingEnhancements = pendingProcessing;
  
  function getEnhancedBitmap(tile: CachedTile, mode: StainEnhancementMode): ImageBitmap | null {
    return getProcessedBitmap(tile, stainNormalization, mode, getSlideId());
  }
  
  function scheduleEnhancement(tile: CachedTile, mode: StainEnhancementMode): void {
    scheduleProcessing(tile, stainNormalization, mode, getSlideId());
  }
  
  function clearEnhancedCache(): void {
    clearProcessedCache();
  }
  
  /** Maximum concurrent processing operations to prevent overwhelming the main thread */
  const MAX_CONCURRENT_PROCESSING = 8;
  
  /**
   * Prefetch and pre-process tiles in a margin around the visible viewport.
   * This prepares tiles for smooth panning without flickering.
   */
  function prefetchProcessing(
    idealLevel: number,
    visibleMinTx: number,
    visibleMinTy: number,
    visibleMaxTx: number,
    visibleMaxTy: number,
    normMode: StainNormalizationMode,
    enhanceMode: StainEnhancementMode
  ): void {
    // Skip if no processing needed
    if (normMode === 'none' && enhanceMode === 'none') return;
    
    // Limit concurrent processing to avoid overwhelming the thread
    if (pendingProcessing.size >= MAX_CONCURRENT_PROCESSING) return;
    
    // Prefetch margin: 3 tiles in each direction for smoother panning
    const margin = 3;
    
    const downsample = Math.pow(2, idealLevel);
    const pxPerTile = downsample * TILE_SIZE;
    const tilesX = Math.ceil(image.width / pxPerTile);
    const tilesY = Math.ceil(image.height / pxPerTile);
    
    const prefetchMinTx = Math.max(0, visibleMinTx - margin);
    const prefetchMinTy = Math.max(0, visibleMinTy - margin);
    const prefetchMaxTx = Math.min(tilesX, visibleMaxTx + margin);
    const prefetchMaxTy = Math.min(tilesY, visibleMaxTy + margin);
    
    // Schedule processing for tiles in the prefetch zone but not visible
    for (let ty = prefetchMinTy; ty < prefetchMaxTy && pendingProcessing.size < MAX_CONCURRENT_PROCESSING; ty++) {
      for (let tx = prefetchMinTx; tx < prefetchMaxTx && pendingProcessing.size < MAX_CONCURRENT_PROCESSING; tx++) {
        // Skip tiles that are already visible (they're handled during render)
        if (tx >= visibleMinTx && tx < visibleMaxTx && ty >= visibleMinTy && ty < visibleMaxTy) {
          continue;
        }
        
        // Check if tile is in cache and has a decoded bitmap
        const tile = cache.get(tx, ty, idealLevel);
        if (tile?.bitmap) {
          scheduleProcessing(tile, normMode, enhanceMode, getSlideId());
        }
      }
    }
  }

  /** Create a checkerboard transparency pattern (like Photoshop). */
  function createCheckerboardPattern(context: CanvasRenderingContext2D): CanvasPattern | null {
    const size = 16; // size of each checker square in pixels
    const patternCanvas = document.createElement('canvas');
    patternCanvas.width = size * 2;
    patternCanvas.height = size * 2;
    const pctx = patternCanvas.getContext('2d');
    if (!pctx) return null;
    // Lighter squares
    pctx.fillStyle = '#2a2a2a';
    pctx.fillRect(0, 0, size * 2, size * 2);
    // Darker squares
    pctx.fillStyle = '#222222';
    pctx.fillRect(0, 0, size, size);
    pctx.fillRect(size, size, size, size);
    return context.createPattern(patternCanvas, 'repeat');
  }

  // Debug mode state
  let dKeyHeld = $state(false);
  let mouseX = $state(0);
  let mouseY = $state(0);
  // Forced mip level for debugging (null = normal behavior, 0-9 = force that level)
  let forcedMipLevel: number | null = $state(null);

  function handleKeyDown(e: KeyboardEvent) {
    if (e.key === 'd' || e.key === 'D') {
      dKeyHeld = true;
    }
    // Number keys 0-9 force that mip level for debugging
    if (e.key >= '0' && e.key <= '9') {
      forcedMipLevel = parseInt(e.key, 10);
    }
  }

  function handleKeyUp(e: KeyboardEvent) {
    if (e.key === 'd' || e.key === 'D') {
      dKeyHeld = false;
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
        // Check if tile is already in the cache before retrying.
        // This prevents unnecessary retry requests for tiles that arrived
        // but haven't been acknowledged by the render loop yet.
        isTileCached: (coord: TileCoord) => {
          return cache.has(coord.x, coord.y, coord.level);
        },
      });
    } else {
      retryManager?.clear();
      retryManager = null;
    }
  });

  /** Schedule a render on the next animation frame, coalescing multiple requests. */
  function scheduleRender() {
    if (renderScheduled) return;
    renderScheduled = true;
    animationFrameId = requestAnimationFrame(() => {
      renderScheduled = false;
      render();
    });
  }

  onMount(() => {
    ctx = canvas.getContext('2d');
    window.addEventListener('keydown', handleKeyDown);
    window.addEventListener('keyup', handleKeyUp);
    // Initialize worker pool for off-main-thread processing
    workerPool = getProcessingPool();
    scheduleRender();
  });

  onDestroy(() => {
    if (animationFrameId !== null) {
      cancelAnimationFrame(animationFrameId);
    }
    // Clear zoom detection timer
    if (zoomChangeTimer !== null) {
      clearTimeout(zoomChangeTimer);
    }
    // Clear retry manager
    retryManager?.clear();
    // Clear enhanced bitmap cache
    clearEnhancedCache();
    window.removeEventListener('keydown', handleKeyDown);
    window.removeEventListener('keyup', handleKeyUp);
  });

  // Keep the cache's viewport context up-to-date for smart eviction
  // This protects coarse tiles (higher mip levels) intersecting the viewport
  // from being evicted, ensuring smooth zoom-out behavior
  $effect(() => {
    cache.setViewportContext(viewport, image);
  });

  // Detect zoom changes and notify worker pool for throttling
  $effect(() => {
    const currentZoom = viewport.zoom;
    if (Math.abs(currentZoom - lastZoom) > 0.001) {
      // Zoom is changing - notify pool to reduce processing load
      workerPool?.notifyZoomStart();
      lastZoom = currentZoom;
      
      // Clear existing timer
      if (zoomChangeTimer !== null) {
        clearTimeout(zoomChangeTimer);
      }
      
      // Set timer to detect when zoom stops
      zoomChangeTimer = window.setTimeout(() => {
        workerPool?.notifyZoomEnd();
        zoomChangeTimer = null;
      }, 100);
    }
  });

  // Re-render when viewport, renderTrigger, stainNormalization, stainEnhancement, sharpening, or debug state changes
  $effect(() => {
    // Access reactive dependencies
    void viewport;
    void renderTrigger;
    void stainNormalization;
    void stainEnhancement;
    void sharpeningEnabled;
    void sharpeningIntensity;
    void dKeyHeld;
    void mouseX;
    void mouseY;
    void forcedMipLevel;
    scheduleRender();
  });

  function render() {
    if (!ctx || !canvas) return;

    const renderStart = performance.now();
    
    // Track render stats
    let renderedTiles = 0;
    let fallbackTiles = 0;
    let placeholderTiles = 0;

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
      // Invalidate pattern since context was reset
      checkerboardPattern = null;
    }

    // Clear canvas with a checkerboard transparency pattern
    if (!checkerboardPattern) {
      checkerboardPattern = createCheckerboardPattern(ctx);
    }
    if (checkerboardPattern) {
      ctx.fillStyle = checkerboardPattern;
    } else {
      ctx.fillStyle = '#ffffff';
    }
    ctx.fillRect(0, 0, displayWidth, displayHeight);

    // Compute the ideal mip level for current zoom
    const dpi = window.devicePixelRatio * 96;
    const idealLevel = computeIdealLevel(viewport.zoom, image.levels, dpi);

    // Compute finer level for 2x screen DPI (one level below ideal, clamped to 0)
    const finerLevel = Math.max(0, idealLevel - 1);

    // Get visible tiles at the ideal level only (for retries, we only request at screen resolution)
    const idealTiles = visibleTilesForLevel(viewport, image, idealLevel);

    // Compute visible tile bounds for prefetching (derive from idealTiles)
    let visibleMinTx = Infinity, visibleMinTy = Infinity;
    let visibleMaxTx = -Infinity, visibleMaxTy = -Infinity;
    for (const t of idealTiles) {
      if (t.x < visibleMinTx) visibleMinTx = t.x;
      if (t.y < visibleMinTy) visibleMinTy = t.y;
      if (t.x >= visibleMaxTx) visibleMaxTx = t.x + 1;
      if (t.y >= visibleMaxTy) visibleMaxTy = t.y + 1;
    }

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
      const result = renderTileWithFallback(coord, idealLevel, finerLevel, forcedMipLevel);
      if (result === 'rendered') renderedTiles++;
      else if (result === 'fallback') fallbackTiles++;
      else if (result === 'placeholder') placeholderTiles++;
    }

    // Debug overlay when 'd' is held or mip level is forced
    if (dKeyHeld || forcedMipLevel !== null) {
      renderDebugOverlay(idealTiles, idealLevel);
    }

    // Calculate render time and FPS
    const renderEnd = performance.now();
    const renderTimeMs = renderEnd - renderStart;
    
    // Update FPS tracking
    if (lastFrameTime > 0) {
      const frameDelta = renderEnd - lastFrameTime;
      frameTimesMs.push(frameDelta);
      if (frameTimesMs.length > FPS_SAMPLE_SIZE) {
        frameTimesMs.shift();
      }
    }
    lastFrameTime = renderEnd;
    
    // Calculate rolling average FPS
    const avgFrameTime = frameTimesMs.length > 0 
      ? frameTimesMs.reduce((a, b) => a + b, 0) / frameTimesMs.length 
      : 16.67;
    const fps = 1000 / avgFrameTime;
    
    // Report metrics via callback
    if (onMetrics) {
      onMetrics({
        renderTimeMs,
        fps,
        visibleTiles: idealTiles.length,
        renderedTiles,
        fallbackTiles,
        placeholderTiles,
      });
    }
    
    // Prefetch processing for tiles just outside the viewport (smooth panning)
    if ((stainNormalization !== 'none' || stainEnhancement !== 'none') && idealTiles.length > 0) {
      // Also pre-process coarser levels so fallbacks are ready
      prefetchCoarseLevelProcessing(idealLevel, stainNormalization, stainEnhancement);
      prefetchProcessing(idealLevel, visibleMinTx, visibleMinTy, visibleMaxTx, visibleMaxTy, stainNormalization, stainEnhancement);
    }
  }
  
  /**
   * Pre-process coarser fallback levels to ensure smooth transitions.
   * This ensures fallback tiles are already processed when we need them.
   */
  function prefetchCoarseLevelProcessing(
    idealLevel: number,
    normMode: StainNormalizationMode,
    enhanceMode: StainEnhancementMode
  ): void {
    if (normMode === 'none' && enhanceMode === 'none') return;
    if (pendingProcessing.size >= MAX_CONCURRENT_PROCESSING) return;
    
    // Process tiles at coarser levels (up to 2 levels coarser)
    for (let level = idealLevel + 1; level <= Math.min(idealLevel + 2, image.levels - 1); level++) {
      const downsample = Math.pow(2, level);
      const pxPerTile = downsample * TILE_SIZE;
      const tilesX = Math.ceil(image.width / pxPerTile);
      const tilesY = Math.ceil(image.height / pxPerTile);
      
      // Find tiles that cover the current viewport
      const scale = Math.pow(2, level - idealLevel);
      const minTx = Math.floor(viewport.x / pxPerTile);
      const minTy = Math.floor(viewport.y / pxPerTile);
      const maxTx = Math.ceil((viewport.x + viewport.width / viewport.zoom) / pxPerTile);
      const maxTy = Math.ceil((viewport.y + viewport.height / viewport.zoom) / pxPerTile);
      
      for (let ty = Math.max(0, minTy); ty <= Math.min(tilesY - 1, maxTy) && pendingProcessing.size < MAX_CONCURRENT_PROCESSING; ty++) {
        for (let tx = Math.max(0, minTx); tx <= Math.min(tilesX - 1, maxTx) && pendingProcessing.size < MAX_CONCURRENT_PROCESSING; tx++) {
          const tile = cache.get(tx, ty, level);
          if (tile?.bitmap) {
            // Check if already processed
            const cached = getProcessedBitmap(tile, normMode, enhanceMode, getSlideId());
            if (!cached) {
              scheduleProcessing(tile, normMode, enhanceMode, getSlideId());
            }
          }
        }
      }
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

  type RenderResult = 'rendered' | 'fallback' | 'placeholder' | 'skipped';

  function renderTileWithFallback(
    targetCoord: TileCoord,
    idealLevel: number,
    finerLevel: number,
    forcedLevel: number | null = null
  ): RenderResult {
    if (!ctx) return 'skipped';

    const rect = tileScreenRect(targetCoord, viewport);

    // Skip tiles completely outside the viewport
    if (
      rect.x + rect.width < 0 ||
      rect.y + rect.height < 0 ||
      rect.x > viewport.width ||
      rect.y > viewport.height
    ) {
      return 'skipped';
    }

    // If forcing a specific mip level, only use that level (render as fallback)
    if (forcedLevel !== null) {
      const clampedLevel = Math.min(forcedLevel, image.levels - 1);
      
      if (clampedLevel === idealLevel) {
        // Forced level matches ideal - render directly if available
        const cachedTile = cache.get(targetCoord.x, targetCoord.y, targetCoord.level);
        if (!cachedTile || !renderTile(cachedTile, rect)) {
          renderPlaceholder(rect);
          return 'placeholder';
        }
        return 'rendered';
      } else if (clampedLevel > idealLevel) {
        // Forced level is coarser - render as fallback (sub-portion of coarser tile)
        const scale = Math.pow(2, clampedLevel - idealLevel);
        const coarseX = Math.floor(targetCoord.x / scale);
        const coarseY = Math.floor(targetCoord.y / scale);
        const coarse = cache.get(coarseX, coarseY, clampedLevel);
        if (!coarse || !renderFallbackTile(targetCoord, coarse, clampedLevel, idealLevel, rect)) {
          renderPlaceholder(rect);
          return 'placeholder';
        }
        return 'fallback';
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
              if (renderTile(finerTile, subRect)) {
                anyRendered = true;
              }
            }
          }
        }
        
        if (!anyRendered) {
          renderPlaceholder(rect);
          return 'placeholder';
        }
        return 'rendered';
      }
    }

    // Normal rendering with fallback...
    // Try to find the best available tile
    // First check the ideal level
    let cachedTile = cache.get(targetCoord.x, targetCoord.y, targetCoord.level);

    // If found at ideal level AND its bitmap is decoded, render it directly.
    if (cachedTile) {
      if (retryManager) {
        retryManager.tileReceived(targetCoord.x, targetCoord.y, targetCoord.level);
      }
      if (renderTile(cachedTile, rect)) {
        return 'rendered';   // drawn successfully
      }
      // Bitmap not decoded yet — fall through to coarser fallback so
      // the user sees *something* immediately (progressive loading).
    }

    // Tile not found (or not decoded) at ideal level — track it for retry.
    if (retryManager && targetCoord.level === idealLevel && !cachedTile) {
      retryManager.trackTile(targetCoord);
    }

    // Look for coarser fallbacks whose bitmaps ARE decoded.
    for (let level = idealLevel + 1; level < image.levels; level++) {
      const scale = Math.pow(2, level - idealLevel);
      const coarseX = Math.floor(targetCoord.x / scale);
      const coarseY = Math.floor(targetCoord.y / scale);

      const coarse = cache.get(coarseX, coarseY, level);
      if (coarse) {
        if (renderFallbackTile(targetCoord, coarse, level, idealLevel, rect)) {
          return 'fallback';   // drawn from fallback
        }
        // This fallback's bitmap isn't decoded either — keep searching.
      }
    }

    // No decoded tile available at any level — show placeholder.
    renderPlaceholder(rect);
    return 'placeholder';
  }

  /**
   * Draw a tile's bitmap onto the canvas, applying stain normalization and enhancement if active.
   * Returns `true` if the tile was drawn, `false` if the bitmap isn't
   * decoded yet or processed version isn't ready (caller should fall back to a coarser tile).
   */
  function renderTile(tile: CachedTile, rect: { x: number; y: number; width: number; height: number }): boolean {
    if (!ctx) return false;

    if (tile.bitmap) {
      // Apply stain normalization, enhancement, and/or sharpening if enabled
      const needsProcessing = stainNormalization !== 'none' || stainEnhancement !== 'none' ||
        (sharpeningEnabled && sharpeningIntensity > 0);
      
      if (needsProcessing) {
        // Check for cached processed bitmap first (fast path)
        const cachedProcessed = getProcessedBitmap(tile, stainNormalization, stainEnhancement, getSlideId());
        if (cachedProcessed) {
          // Draw cached processed bitmap directly (fast!)
          ctx.drawImage(cachedProcessed, rect.x, rect.y, rect.width, rect.height);
          return true;
        }
        
        // No cached version - schedule async processing
        // Return false to trigger fallback to coarser tiles (which may already be processed)
        // This prevents flickering when panning by avoiding drawing unprocessed tiles
        scheduleProcessing(tile, stainNormalization, stainEnhancement, getSlideId());
        return false;
      } else {
        // No processing — draw directly (fast path)
        ctx.drawImage(tile.bitmap, rect.x, rect.y, rect.width, rect.height);
      }
      return true;
    }

    // Bitmap not yet decoded — tell the caller so it can try coarser fallbacks.
    return false;
  }

  /**
   * Draw a sub-region of a coarser fallback tile, applying stain normalization and enhancement if active.
   * Returns `true` if drawn.
   */
  function renderFallbackTile(
    targetCoord: TileCoord,
    fallbackTile: CachedTile,
    fallbackLevel: number,
    idealLevel: number,
    targetRect: { x: number; y: number; width: number; height: number }
  ): boolean {
    if (!ctx) return false;

    if (!fallbackTile.bitmap) {
      return false;
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

    // Apply stain normalization, enhancement, and/or sharpening if enabled
    const needsProcessing = stainNormalization !== 'none' || stainEnhancement !== 'none' ||
      (sharpeningEnabled && sharpeningIntensity > 0);
    
    if (needsProcessing) {
      // Check for cached processed bitmap first (fast path)
      const cachedProcessed = getProcessedBitmap(fallbackTile, stainNormalization, stainEnhancement, getSlideId());
      if (cachedProcessed) {
        // Draw sub-region from cached processed bitmap (fast!)
        ctx.drawImage(
          cachedProcessed,
          srcX,
          srcY,
          srcSize,
          srcSize,
          targetRect.x,
          targetRect.y,
          targetRect.width,
          targetRect.height
        );
        return true;
      }
      
      // No cached version - schedule async processing
      // Draw unprocessed tile immediately for smooth panning (processing pops in when ready)
      scheduleProcessing(fallbackTile, stainNormalization, stainEnhancement, getSlideId());
      ctx.drawImage(
        fallbackTile.bitmap,
        srcX,
        srcY,
        srcSize,
        srcSize,
        targetRect.x,
        targetRect.y,
        targetRect.width,
        targetRect.height
      );
    } else {
      ctx.drawImage(
        fallbackTile.bitmap,
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

    return true;
  }

  function renderPlaceholder(rect: { x: number; y: number; width: number; height: number }) {
    if (!ctx) return;

    // Draw a checkerboard transparency pattern for missing tiles
    if (!checkerboardPattern) {
      checkerboardPattern = createCheckerboardPattern(ctx);
    }
    if (checkerboardPattern) {
      ctx.fillStyle = checkerboardPattern;
    } else {
      ctx.fillStyle = '#ffffff';
    }
    ctx.fillRect(rect.x, rect.y, rect.width, rect.height);

    ctx.strokeStyle = '#ccc';
    ctx.lineWidth = 1;
    ctx.strokeRect(rect.x + 0.5, rect.y + 0.5, rect.width - 1, rect.height - 1);
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
