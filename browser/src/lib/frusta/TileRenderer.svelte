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
  let needsRender = false;
  let keepRendering = false;
  let lastPresentedFrameTime = 0;
  const TARGET_RENDER_FPS = 60;
  const TARGET_FRAME_MS = 1000 / TARGET_RENDER_FPS;
  
  // Double buffering: render to offscreen canvas, then copy to visible canvas
  // This eliminates visual tearing during smooth navigation
  let offscreenCanvas: OffscreenCanvas | null = null;
  let offscreenCtx: OffscreenCanvasRenderingContext2D | null = null;
  let lastOffscreenWidth = 0;
  let lastOffscreenHeight = 0;
  
  // Active render context - set to offscreenCtx during render(), used by helper functions
  let activeRenderCtx: CanvasRenderingContext2D | OffscreenCanvasRenderingContext2D | null = null;
  
  // FPS tracking
  let lastFrameTime = 0;
  let frameTimesMs: number[] = [];
  const FPS_SAMPLE_SIZE = 30;
  let retryManager: TileRetryManager | null = null;
  let checkerboardPattern: CanvasPattern | null = null;
  
  // Worker pool for off-main-thread processing
  let workerPool: ProcessingWorkerPool | null = null;
  
  // Interaction state tracking for throttling expensive operations
  // During active pan/zoom, we skip CPU-intensive work to keep UI responsive
  let isInteracting = false;
  let interactionEndTimer: number | null = null;
  const INTERACTION_DEBOUNCE_MS = 150; // Time after last interaction before resuming heavy work
  
  // Zoom detection for worker pool throttling
  let lastZoom: number | null = null; // Initialized on first $effect run
  let zoomChangeTimer: number | null = null;
  
  // Pending idle callbacks for cleanup
  let pendingIdleCallbacks = new Set<number>();
  
  // Batched sample contribution - collect tile coordinates to sample when idle
  // Use coordinates instead of tile references to avoid holding onto old tiles
  let tileCoordsToSample: Array<{ x: number; y: number; level: number; normMode: StainNormalizationMode; slideId: string }> = [];
  let sampleIdleCallbackId: number | null = null;
  
  // Limit sampledTileKeys to prevent unbounded growth
  const MAX_SAMPLED_KEYS = 1000;

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
  /** Track when pending entries were added for stale cleanup */
  const pendingProcessingTimestamps = new Map<string, number>();
  /** Maximum processed bitmaps to cache (keep low to reduce memory pressure) */
  const MAX_PROCESSED_CACHE_SIZE = 200;
  /** Maximum pending processing entries before cleanup */
  const MAX_PENDING_PROCESSING = 100;
  /** Stale pending entry timeout (ms) */
  const PENDING_STALE_TIMEOUT = 30000;
  
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
   * Accumulates samples from multiple tiles for robust estimation.
   * Returns null while still gathering samples.
   */
  function getNormalizationParams(
    tile: CachedTile,
    normMode: StainNormalizationMode,
    slideId: string
  ): NormalizationParams | null {
    if (normMode === 'none') return null;
    
    // Return cached params if they match and are ready
    if (cachedNormParams && cachedNormSlideId === slideId && cachedNormMode === normMode) {
      return cachedNormParams;
    }
    
    // Need to compute/accumulate - extract pixels from tile bitmap
    if (!tile.bitmap) return null;
    
    // Create a temporary canvas to extract pixel data
    const canvas = document.createElement('canvas');
    canvas.width = tile.bitmap.width;
    canvas.height = tile.bitmap.height;
    const ctx = canvas.getContext('2d');
    if (!ctx) return null;
    
    ctx.drawImage(tile.bitmap, 0, 0);
    const imageData = ctx.getImageData(0, 0, canvas.width, canvas.height);
    
    // Try to compute normalization parameters (may return null if still accumulating)
    const params = getOrComputeNormalizationParams(slideId, imageData.data, normMode);
    
    if (params) {
      // Parameters are ready - cache them
      cachedNormParams = params;
      cachedNormSlideId = slideId;
      cachedNormMode = normMode;
    }
    
    return params;
  }
  
  /** Track which tiles have contributed samples for parameter estimation */
  const sampledTileKeys = new Set<string>();
  
  /**
   * Queue a tile for sample contribution during idle time.
   * This avoids blocking the main thread during pan/zoom.
   */
  function queueSampleContribution(
    tile: CachedTile,
    normMode: StainNormalizationMode,
    slideId: string
  ): void {
    if (normMode === 'none' || !tile.bitmap) return;
    
    // Skip if we already have cached params
    if (cachedNormParams && cachedNormSlideId === slideId && cachedNormMode === normMode) {
      return;
    }
    
    // Skip if this tile has already contributed
    const tileKey = `${slideId}_${tileKeyFromMeta(tile.meta)}`;
    if (sampledTileKeys.has(tileKey)) return;
    
    // Limit sampledTileKeys size to prevent unbounded growth
    if (sampledTileKeys.size >= MAX_SAMPLED_KEYS) {
      // Clear oldest half when limit reached
      const keysToDelete = Array.from(sampledTileKeys).slice(0, MAX_SAMPLED_KEYS / 2);
      for (const k of keysToDelete) {
        sampledTileKeys.delete(k);
      }
    }
    
    // Limit tileCoordsToSample size to prevent unbounded growth
    if (tileCoordsToSample.length >= 500) {
      // Drop oldest entries (front of queue)
      tileCoordsToSample = tileCoordsToSample.slice(-250);
    }
    
    // Add to batch queue using coordinates (not tile references)
    tileCoordsToSample.push({ 
      x: tile.meta.x, 
      y: tile.meta.y, 
      level: tile.meta.level, 
      normMode, 
      slideId 
    });
    
    // Schedule idle callback if not already scheduled
    if (sampleIdleCallbackId === null) {
      sampleIdleCallbackId = requestIdleCallback(processSampleBatch, { timeout: 500 });
      pendingIdleCallbacks.add(sampleIdleCallbackId);
    }
  }
  
  /**
   * Process batched sample contributions during idle time.
   * Processes multiple tiles per callback to amortize callback overhead.
   */
  function processSampleBatch(deadline: IdleDeadline): void {
    // Remove this callback from pending set
    if (sampleIdleCallbackId !== null) {
      pendingIdleCallbacks.delete(sampleIdleCallbackId);
    }
    sampleIdleCallbackId = null;
    
    // Skip if we're interacting or already have params
    if (isInteracting || tileCoordsToSample.length === 0) {
      tileCoordsToSample = [];
      return;
    }
    
    // Process as many tiles as we can within the deadline (at least 1)
    const minTimePerTile = 5; // ms estimate per tile
    let processed = 0;
    
    while (tileCoordsToSample.length > 0 && (deadline.timeRemaining() > minTimePerTile || processed === 0)) {
      const { x, y, level, normMode, slideId } = tileCoordsToSample.shift()!;
      
      // Look up the tile from cache (it may have been evicted)
      const tile = cache.get(x, y, level);
      if (tile?.bitmap) {
        contributeSamplesFromTileSync(tile, normMode, slideId);
      }
      processed++;
      
      // Check if we got params - if so, clear remaining queue
      if (cachedNormParams && cachedNormSlideId === slideId && cachedNormMode === normMode) {
        tileCoordsToSample = [];
        break;
      }
    }
    
    // If more tiles remain, schedule another idle callback
    if (tileCoordsToSample.length > 0) {
      sampleIdleCallbackId = requestIdleCallback(processSampleBatch, { timeout: 500 });
      pendingIdleCallbacks.add(sampleIdleCallbackId);
    }
  }
  
  /**
   * Synchronously contribute samples from a tile (called during idle time).
   */
  function contributeSamplesFromTileSync(
    tile: CachedTile,
    normMode: StainNormalizationMode,
    slideId: string
  ): void {
    if (normMode === 'none' || !tile.bitmap) return;
    
    // Skip if we already have cached params
    if (cachedNormParams && cachedNormSlideId === slideId && cachedNormMode === normMode) {
      return;
    }
    
    // Skip if this tile has already contributed
    const tileKey = `${slideId}_${tileKeyFromMeta(tile.meta)}`;
    if (sampledTileKeys.has(tileKey)) return;
    
    // Limit sampledTileKeys size to prevent unbounded growth
    if (sampledTileKeys.size >= MAX_SAMPLED_KEYS) {
      // Clear oldest half when limit reached
      const keysToDelete = Array.from(sampledTileKeys).slice(0, MAX_SAMPLED_KEYS / 2);
      for (const k of keysToDelete) {
        sampledTileKeys.delete(k);
      }
    }
    
    sampledTileKeys.add(tileKey);
    
    // Extract pixels and contribute to sample accumulation
    const canvas = document.createElement('canvas');
    canvas.width = tile.bitmap.width;
    canvas.height = tile.bitmap.height;
    const ctx = canvas.getContext('2d');
    if (!ctx) return;
    
    ctx.drawImage(tile.bitmap, 0, 0);
    const imageData = ctx.getImageData(0, 0, canvas.width, canvas.height);
    
    // This will accumulate samples and eventually compute params
    const params = getOrComputeNormalizationParams(slideId, imageData.data, normMode);
    
    if (params) {
      cachedNormParams = params;
      cachedNormSlideId = slideId;
      cachedNormMode = normMode;
    }
  }
  
  /**
   * Signal that user interaction has started (pan/zoom).
   * Throttles expensive operations to maintain UI responsiveness.
   */
  function notifyInteractionStart(): void {
    isInteracting = true;
    workerPool?.notifyZoomStart();
    
    // Clear any pending end timer
    if (interactionEndTimer !== null) {
      clearTimeout(interactionEndTimer);
      interactionEndTimer = null;
    }
  }
  
  /**
   * Signal that user interaction may have ended.
   * Waits for debounce period before resuming heavy work.
   */
  function notifyInteractionEnd(): void {
    // Clear existing timer
    if (interactionEndTimer !== null) {
      clearTimeout(interactionEndTimer);
    }
    
    // Set timer to detect when interaction truly stops
    interactionEndTimer = window.setTimeout(() => {
      isInteracting = false;
      workerPool?.notifyZoomEnd();
      interactionEndTimer = null;
      
      // Resume processing after interaction ends
      scheduleRender();
    }, INTERACTION_DEBOUNCE_MS);
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
    
    // Skip during active interaction to keep UI responsive
    if (isInteracting) return;
    
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
    pendingProcessingTimestamps.set(key, Date.now());
    
    // Clean up stale pending entries periodically
    cleanupStalePendingEntries();
    
    // Calculate priority (tiles closer to viewport center are processed first)
    const priority = calculateTilePriority(tile);
    
    // Use requestIdleCallback to defer ImageData extraction to idle time
    const idleCallbackId = requestIdleCallback(() => {
      // Remove from pending set when callback runs
      pendingIdleCallbacks.delete(idleCallbackId);
      
      // Re-check conditions after idle delay
      if (isInteracting || !tile.bitmap) {
        pendingProcessing.delete(key);
        pendingProcessingTimestamps.delete(key);
        return;
      }
      
      // Process bitmap using worker pool (off main thread)
      processImageBitmapWithWorkerDeferred(tile, normMode, enhanceMode, slideId, key, priority);
    }, { timeout: 100 });
    
    pendingIdleCallbacks.add(idleCallbackId);
  }
  
  /** Deferred processing that runs during idle time */
  function processImageBitmapWithWorkerDeferred(
    tile: CachedTile,
    normMode: StainNormalizationMode,
    enhanceMode: StainEnhancementMode,
    slideId: string,
    key: string,
    priority: number
  ): void {
    processImageBitmapWithWorker(tile, normMode, enhanceMode, slideId, key, priority)
      .then((processedBitmap) => {
        pendingProcessing.delete(key);
        pendingProcessingTimestamps.delete(key);
        
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
        pendingProcessingTimestamps.delete(key);
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
  
  /**
   * Clean up stale entries from pendingProcessing.
   * Entries that have been pending for too long are likely orphaned
   * (worker crashed, promise never resolved, etc.)
   */
  function cleanupStalePendingEntries(): void {
    // Only cleanup if we're over the limit
    if (pendingProcessing.size < MAX_PENDING_PROCESSING) return;
    
    const now = Date.now();
    const staleKeys: string[] = [];
    
    for (const [key, timestamp] of pendingProcessingTimestamps) {
      if (now - timestamp > PENDING_STALE_TIMEOUT) {
        staleKeys.push(key);
      }
    }
    
    // Remove stale entries
    for (const key of staleKeys) {
      pendingProcessing.delete(key);
      pendingProcessingTimestamps.delete(key);
    }
    
    // If still over limit after removing stale entries, remove oldest
    if (pendingProcessing.size >= MAX_PENDING_PROCESSING) {
      const sortedEntries = Array.from(pendingProcessingTimestamps.entries())
        .sort((a, b) => a[1] - b[1]);
      
      const toRemove = pendingProcessing.size - Math.floor(MAX_PENDING_PROCESSING * 0.5);
      for (let i = 0; i < toRemove && sortedEntries.length > 0; i++) {
        const [key] = sortedEntries.shift()!;
        pendingProcessing.delete(key);
        pendingProcessingTimestamps.delete(key);
      }
    }
  }
  
  /** Clear processed cache (called on destroy or slide change) */
  function clearProcessedCache(): void {
    for (const entry of processedBitmapCache.values()) {
      entry.bitmap.close();
    }
    processedBitmapCache.clear();
    pendingProcessing.clear();
    pendingProcessingTimestamps.clear();
    sampledTileKeys.clear();
    
    // Also clear normalization params cache
    cachedNormParams = null;
    cachedNormSlideId = null;
    cachedNormMode = null;
  }
  
  /**
   * Debug function to log sizes of all internal data structures.
   * Call from browser console: window.__tileRendererDebug?.()
   */
  function logInternalSizes(): Record<string, unknown> {
    const sizes = {
      processedBitmapCache: processedBitmapCache.size,
      pendingProcessing: pendingProcessing.size,
      pendingProcessingTimestamps: pendingProcessingTimestamps.size,
      sampledTileKeys: sampledTileKeys.size,
      tileCoordsToSample: tileCoordsToSample.length,
      pendingIdleCallbacks: pendingIdleCallbacks.size,
      frameTimesMs: frameTimesMs.length,
      cacheTiles: cache?.size ?? 0,
      cacheMemoryMB: ((cache?.getMemoryUsage() ?? 0) / 1024 / 1024).toFixed(2),
      retryManagerPending: retryManager?.pendingCount ?? 0,
      workerPoolQueue: workerPool?.queueLength ?? 0,
      workerPoolPending: workerPool?.pendingCount ?? 0,
      lastRenderTimings: lastRenderTimings,
    };
    console.table(sizes);
    if (lastRenderTimings) {
      console.log('Last render timing breakdown:');
      console.table(lastRenderTimings);
    }
    return sizes;
  }
  
  // Expose debug function to window for console access
  if (typeof window !== 'undefined') {
    (window as any).__tileRendererDebug = logInternalSizes;
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
  function createCheckerboardPattern(context: CanvasRenderingContext2D | OffscreenCanvasRenderingContext2D): CanvasPattern | null {
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
  let mouseX = $state(0);
  let mouseY = $state(0);
  // Forced mip level for debugging (null = normal behavior, 0-9 = force that level)
  let forcedMipLevel: number | null = $state(null);
  // Debug view mode key state
  let yKeyHeld = $state(false);
  // Track if debug mode is active (for conditional mouse tracking)
  let debugModeActive = $derived(yKeyHeld || forcedMipLevel !== null);

  function handleKeyDown(e: KeyboardEvent) {
    if (e.key === 'y' || e.key === 'Y') {
      yKeyHeld = true;
    }
    // Shift+0-9 force that mip level for debugging
    if (e.shiftKey && e.key >= '0' && e.key <= '9') {
      forcedMipLevel = parseInt(e.key, 10);
    }
  }

  function handleKeyUp(e: KeyboardEvent) {
    if (e.key === 'y' || e.key === 'Y') {
      yKeyHeld = false;
    }
    // Release forced mip level when Shift+number key is released
    if (e.key >= '0' && e.key <= '9' && forcedMipLevel === parseInt(e.key, 10)) {
      forcedMipLevel = null;
    }
  }

  function handleMouseMove(e: MouseEvent) {
    // Only track mouse position when debug overlay is active
    // This prevents expensive re-renders on every mouse move during normal panning
    if (!debugModeActive) return;
    
    const rect = canvas?.getBoundingClientRect();
    if (rect) {
      mouseX = e.clientX - rect.left;
      mouseY = e.clientY - rect.top;
    }
  }

  // Initialize retry manager when client and slot are available
  $effect(() => {
    // Always clear old retry manager first to prevent leaked timeouts
    if (retryManager) {
      retryManager.clear();
      retryManager = null;
    }
    
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
    }
  });

  let scheduleRenderTime: number = 0;

  function ensureRenderLoop() {
    if (animationFrameId !== null) return;
    animationFrameId = requestAnimationFrame(renderLoopTick);
  }

  function renderLoopTick(now: number) {
    animationFrameId = null;

    const hasFrameBudget =
      lastPresentedFrameTime === 0 ||
      now - lastPresentedFrameTime >= TARGET_FRAME_MS - 0.25;

    if ((needsRender || keepRendering) && hasFrameBudget) {
      needsRender = false;
      keepRendering = render();
      lastPresentedFrameTime = now;
    }

    if (needsRender || keepRendering) {
      ensureRenderLoop();
    }
  }
  
  /** Schedule a render loop tick. */
  function scheduleRender() {
    needsRender = true;
    scheduleRenderTime = performance.now();
    ensureRenderLoop();
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
    // Clear pan detection timer
    if (panChangeTimer !== null) {
      clearTimeout(panChangeTimer);
    }
    // Clear interaction end timer
    if (interactionEndTimer !== null) {
      clearTimeout(interactionEndTimer);
    }
    // Cancel pending idle callbacks
    for (const id of pendingIdleCallbacks) {
      cancelIdleCallback(id);
    }
    pendingIdleCallbacks.clear();
    if (sampleIdleCallbackId !== null) {
      cancelIdleCallback(sampleIdleCallbackId);
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

  // Detect zoom changes and notify for throttling
  $effect(() => {
    const currentZoom = viewport.zoom;
    
    // Initialize on first run
    if (lastZoom === null) {
      lastZoom = currentZoom;
      return;
    }
    
    if (Math.abs(currentZoom - lastZoom) > 0.001) {
      // Zoom is changing - throttle expensive operations
      notifyInteractionStart();
      lastZoom = currentZoom;
      
      // Clear existing timer
      if (zoomChangeTimer !== null) {
        clearTimeout(zoomChangeTimer);
      }
      
      // Set timer to detect when zoom stops
      zoomChangeTimer = window.setTimeout(() => {
        notifyInteractionEnd();
        zoomChangeTimer = null;
      }, 100);
    }
  });
  
  // Detect pan changes (viewport x/y movement)
  let lastViewportX: number | null = null;
  let lastViewportY: number | null = null;
  let panChangeTimer: number | null = null;
  
  $effect(() => {
    const currentX = viewport.x;
    const currentY = viewport.y;
    
    // Initialize on first run
    if (lastViewportX === null || lastViewportY === null) {
      lastViewportX = currentX;
      lastViewportY = currentY;
      return;
    }
    
    const dx = Math.abs(currentX - lastViewportX);
    const dy = Math.abs(currentY - lastViewportY);
    
    // Only trigger if meaningful pan occurred (more than 1 pixel)
    if (dx > 1 || dy > 1) {
      notifyInteractionStart();
      lastViewportX = currentX;
      lastViewportY = currentY;
      
      if (panChangeTimer !== null) {
        clearTimeout(panChangeTimer);
      }
      
      panChangeTimer = window.setTimeout(() => {
        notifyInteractionEnd();
        panChangeTimer = null;
      }, 100);
    }
  });

  // Re-render when viewport, renderTrigger, stainNormalization, stainEnhancement, or sharpening changes
  // NOTE: Debug mode (mouseX/mouseY/yKeyHeld/forcedMipLevel) handled in separate effect below
  $effect(() => {
    // Access reactive dependencies for main rendering
    void viewport;
    void renderTrigger;
    void stainNormalization;
    void stainEnhancement;
    void sharpeningEnabled;
    void sharpeningIntensity;
    scheduleRender();
  });
  
  // Separate effect for debug mode changes - always re-render when debug state changes
  // (to show/hide the debug overlay)
  $effect(() => {
    void yKeyHeld;
    void forcedMipLevel;
    scheduleRender();
  });
  
  // Effect for debug mouse tracking - only tracks mouse position when debug mode is active
  // This prevents performance degradation during normal panning
  $effect(() => {
    if (yKeyHeld || forcedMipLevel !== null) {
      void mouseX;
      void mouseY;
      scheduleRender();
    }
  });

  // Render timing debug (exposed for console inspection)
  let lastRenderTimings: Record<string, number> | null = null;
  let lastRenderEndTime: number = 0;
  
  function render(): boolean {
    if (!ctx || !canvas) return false;

    const timings: Record<string, number> = {};
    const renderStart = performance.now();
    
    // Track inter-frame overhead (time between last render end and this render start)
    if (lastRenderEndTime > 0) {
      timings.interFrameMs = renderStart - lastRenderEndTime;
    }
    
    // Track rAF delay (time from scheduleRender to render callback)
    if (scheduleRenderTime > 0) {
      timings.rafDelayMs = renderStart - scheduleRenderTime;
    }
    
    // Track render stats
    let renderedTiles = 0;
    let fallbackTiles = 0;
    let placeholderTiles = 0;

    // Handle high DPI displays
    const dpr = window.devicePixelRatio || 1;
    const displayWidth = viewport.width;
    const displayHeight = viewport.height;
    const pixelWidth = Math.round(displayWidth * dpr);
    const pixelHeight = Math.round(displayHeight * dpr);

    // Set visible canvas size if needed
    if (canvas.width !== pixelWidth || canvas.height !== pixelHeight) {
      canvas.width = pixelWidth;
      canvas.height = pixelHeight;
      canvas.style.width = `${displayWidth}px`;
      canvas.style.height = `${displayHeight}px`;
    }
    
    // Set up offscreen canvas for double buffering (eliminates tearing)
    if (!offscreenCanvas || lastOffscreenWidth !== pixelWidth || lastOffscreenHeight !== pixelHeight) {
      offscreenCanvas = new OffscreenCanvas(pixelWidth, pixelHeight);
      offscreenCtx = offscreenCanvas.getContext('2d');
      if (offscreenCtx) {
        offscreenCtx.scale(dpr, dpr);
      }
      lastOffscreenWidth = pixelWidth;
      lastOffscreenHeight = pixelHeight;
      // Invalidate pattern since we have a new context
      checkerboardPattern = null;
    }
    
    // Use offscreen context for all rendering
    const renderCtx = offscreenCtx;
    if (!renderCtx) return false;
    
    // Set active context for helper functions
    activeRenderCtx = renderCtx;

    timings.setupMs = performance.now() - renderStart;

    // Clear offscreen canvas with a checkerboard transparency pattern
    const clearStart = performance.now();
    if (!checkerboardPattern) {
      checkerboardPattern = createCheckerboardPattern(renderCtx);
    }
    if (checkerboardPattern) {
      renderCtx.fillStyle = checkerboardPattern;
    } else {
      renderCtx.fillStyle = '#ffffff';
    }
    renderCtx.fillRect(0, 0, displayWidth, displayHeight);
    timings.clearMs = performance.now() - clearStart;

    // Compute the ideal mip level for current zoom
    const computeStart = performance.now();
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
    timings.computeMs = performance.now() - computeStart;
    
    // Queue sample contribution for normalization parameter estimation
    // This is deferred to idle time to avoid blocking during pan/zoom
    const sampleStart = performance.now();
    if (stainNormalization !== 'none' && !isInteracting) {
      const slideId = getSlideId();
      for (const coord of idealTiles) {
        const tile = cache.get(coord.x, coord.y, coord.level);
        if (tile?.bitmap) {
          queueSampleContribution(tile, stainNormalization, slideId);
        }
      }
    }
    timings.sampleQueueMs = performance.now() - sampleStart;

    // Cancel retry tracking for tiles no longer visible at ideal or finer level
    const retryStart = performance.now();
    if (retryManager) {
      retryManager.cancelTilesNotIn(allTrackableTiles);
    }
    timings.retryMs = performance.now() - retryStart;

    // Render tiles with mip fallback
    // Strategy: For each tile position at the ideal level,
    // find the best available tile (finest resolution first, then fallback to coarser)
    const drawStart = performance.now();
    for (const coord of idealTiles) {
      const result = renderTileWithFallback(coord, idealLevel, finerLevel, forcedMipLevel);
      if (result === 'rendered') renderedTiles++;
      else if (result === 'fallback') fallbackTiles++;
      else if (result === 'placeholder') placeholderTiles++;
    }
    timings.drawTilesMs = performance.now() - drawStart;

    // Debug overlay when 'y' is held or mip level is forced
    if (yKeyHeld || forcedMipLevel !== null) {
      renderDebugOverlay(idealTiles, idealLevel);
    }

    // Calculate render time and FPS
    const renderEnd = performance.now();
    const renderTimeMs = renderEnd - renderStart;
    timings.totalMs = renderTimeMs;
    timings.tileCount = idealTiles.length;
    lastRenderTimings = timings;
    
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
    
    // Double buffer blit: copy completed offscreen frame to visible canvas in one operation
    // This eliminates visual tearing by ensuring the visible canvas is only updated atomically
    if (ctx && offscreenCanvas) {
      ctx.drawImage(offscreenCanvas, 0, 0);
    }
    
    // Clear active context (rendering complete)
    activeRenderCtx = null;
    
    // Track when render ended for inter-frame timing
    lastRenderEndTime = performance.now();

    // Continue 60fps loop only while interaction or pending visual work exists.
    const pendingRetries = retryManager?.pendingCount ?? 0;
    const pendingDecodes = cache.getPendingDecodeCount();
    const pendingProcessingCount = pendingProcessing.size;

    return isInteracting || pendingRetries > 0 || pendingDecodes > 0 || pendingProcessingCount > 0;
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
    if (!activeRenderCtx) return;

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
        activeRenderCtx.strokeStyle = '#00ff00';
        activeRenderCtx.lineWidth = 2;
        activeRenderCtx.strokeRect(rect.x + 1, rect.y + 1, rect.width - 2, rect.height - 2);

        // Prepare mip level label
        const label = displayedLevel === -1 ? 'N/A' : `L${displayedLevel}`;
        const fontSize = 14; // Constant size
        activeRenderCtx.font = `bold ${fontSize}px monospace`;
        activeRenderCtx.textAlign = 'center';
        activeRenderCtx.textBaseline = 'middle';

        // Calculate label position (center of tile)
        let labelX = rect.x + rect.width / 2;
        let labelY = rect.y + rect.height / 2;

        // Measure text for background
        const textMetrics = activeRenderCtx.measureText(label);
        const textWidth = textMetrics.width + 8;
        const textHeight = fontSize + 6;

        // Clamp label position to keep it on screen
        labelX = Math.max(textWidth / 2 + 4, Math.min(viewport.width - textWidth / 2 - 4, labelX));
        labelY = Math.max(textHeight / 2 + 4, Math.min(viewport.height - textHeight / 2 - 4, labelY));

        // Draw background for label
        activeRenderCtx.fillStyle = 'rgba(0, 0, 0, 0.8)';
        activeRenderCtx.fillRect(
          labelX - textWidth / 2,
          labelY - textHeight / 2,
          textWidth,
          textHeight
        );

        // Draw label text
        activeRenderCtx.fillStyle = displayedLevel === idealLevel ? '#00ff00' : '#ffff00';
        activeRenderCtx.fillText(label, labelX, labelY);

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
    if (!activeRenderCtx) return false;

    if (tile.bitmap) {
      // Apply stain normalization, enhancement, and/or sharpening if enabled
      const needsProcessing = stainNormalization !== 'none' || stainEnhancement !== 'none' ||
        (sharpeningEnabled && sharpeningIntensity > 0);
      
      if (needsProcessing) {
        // Check for cached processed bitmap first (fast path)
        const cachedProcessed = getProcessedBitmap(tile, stainNormalization, stainEnhancement, getSlideId());
        if (cachedProcessed) {
          // Draw cached processed bitmap directly (fast!)
          activeRenderCtx.drawImage(cachedProcessed, rect.x, rect.y, rect.width, rect.height);
          return true;
        }
        
        // No cached version - schedule async processing
        // Return false to trigger fallback to coarser tiles (which may already be processed)
        // This prevents flickering when panning by avoiding drawing unprocessed tiles
        scheduleProcessing(tile, stainNormalization, stainEnhancement, getSlideId());
        return false;
      } else {
        // No processing — draw directly (fast path)
        activeRenderCtx.drawImage(tile.bitmap, rect.x, rect.y, rect.width, rect.height);
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
    if (!activeRenderCtx) return false;

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
        activeRenderCtx.drawImage(
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
      activeRenderCtx.drawImage(
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
      activeRenderCtx.drawImage(
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
    if (!activeRenderCtx) return;

    // Draw a checkerboard transparency pattern for missing tiles
    if (checkerboardPattern) {
      activeRenderCtx.fillStyle = checkerboardPattern;
    } else {
      activeRenderCtx.fillStyle = '#ffffff';
    }
    activeRenderCtx.fillRect(rect.x, rect.y, rect.width, rect.height);

    activeRenderCtx.strokeStyle = '#ccc';
    activeRenderCtx.lineWidth = 1;
    activeRenderCtx.strokeRect(rect.x + 0.5, rect.y + 0.5, rect.width - 1, rect.height - 1);
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
    /* Prevent selection on touch devices (fixes iPad longpress issue) */
    -webkit-touch-callout: none;
    -webkit-user-select: none;
    user-select: none;
  }
</style>
