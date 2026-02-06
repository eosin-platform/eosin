<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import { browser } from '$app/environment';
  import {
    type ConnectionState,
    type TileData,
    type ImageDesc,
    type ViewportState,
    type TileCache,
    type RenderMetrics,
    TileRenderer,
    toProtocolViewport,
    zoomAround,
    pan,
    clampViewport,
    centerViewport,
    TILE_SIZE,
    computeIdealLevel,
    visibleTilesForLevel,
    MIN_ZOOM,
    MAX_ZOOM,
  } from '$lib/frusta';
  import Minimap from '$lib/components/Minimap.svelte';
  import ActivityIndicator from '$lib/components/ActivityIndicator.svelte';
  import ViewerHud from '$lib/components/viewer/ViewerHud.svelte';
  import ScaleBar from '$lib/components/viewer/ScaleBar.svelte';
  import { tabStore, type Tab } from '$lib/stores/tabs';
  import { acquireCache, releaseCache } from '$lib/stores/slideCache';
  import { updatePerformanceMetrics } from '$lib/stores/metrics';
  import { settings, navigationSettings, imageSettings } from '$lib/stores/settings';

  interface Props {
    /** The pane ID this viewer belongs to */
    paneId: string;
    /** The shared frusta WebSocket client */
    client: any;
    /** Current connection state */
    connectionState: ConnectionState;
    /** Map of slideId -> progress info for activity indicators */
    progressInfo: Map<string, { steps: number; total: number; trigger: number }>;
    /** Callback to register this pane's tile handler with the parent */
    onRegisterTileHandler: (paneId: string, handler: { getSlot: () => number | null; handleTile: (tile: TileData) => void }) => void;
    /** Callback to unregister this pane's tile handler */
    onUnregisterTileHandler: (paneId: string) => void;
  }

  let { paneId, client, connectionState, progressInfo, onRegisterTileHandler, onUnregisterTileHandler }: Props = $props();

  // Image state
  let imageDesc = $state<ImageDesc | null>(null);
  let currentSlot = $state<number | null>(null);
  let loadError = $state<string | null>(null);

  // Track the currently active tab handle (tabId) for the viewer
  let activeTabHandle = $state<string | null>(null);
  // The slide ID of the currently displayed slide
  let activeSlideId = $state<string | null>(null);

  // Viewport state
  let viewport = $state<ViewportState>({
    x: 0,
    y: 0,
    width: 800,
    height: 600,
    zoom: 0.1,
  });

  // Tile cache and render trigger
  let cache = $state<TileCache | null>(null);
  let cacheSize = $state(0);
  let tilesReceived = $state(0);
  let renderTrigger = $state(0);

  // Performance metrics
  let renderMetrics = $state<RenderMetrics | null>(null);
  let cacheMemoryBytes = $state(0);
  let pendingDecodes = $state(0);

  // Container ref for sizing
  let container: HTMLDivElement;

  // Debounce timer for viewport updates
  let viewportUpdateTimeout: ReturnType<typeof setTimeout> | null = null;
  const VIEWPORT_UPDATE_DEBOUNCE_MS = 16;

  // Mouse interaction state
  let isDragging = false;
  let lastMouseX = 0;
  let lastMouseY = 0;

  // Progress
  let progressSteps = $state(0);
  let progressTotal = $state(0);
  let progressUpdateTrigger = $state(0);

  // Settings-derived values for zoom/pan sensitivity
  const sensitivityMap = { low: 0.5, medium: 1.0, high: 2.0 };
  let zoomSensitivityFactor = $derived(sensitivityMap[$navigationSettings.zoomSensitivity] || 1.0);
  let panSensitivityFactor = $derived(sensitivityMap[$navigationSettings.panSensitivity] || 1.0);
  let minimapVisible = $derived($navigationSettings.minimapVisible);

  // Zoom slider: convert linear slider value to logarithmic zoom
  // Slider value 0-100 maps to MIN_ZOOM to MAX_ZOOM logarithmically
  let zoomSliderValue = $derived({
    get value() {
      // Convert zoom to slider position (0-100)
      const logMin = Math.log(MIN_ZOOM);
      const logMax = Math.log(MAX_ZOOM);
      const logZoom = Math.log(viewport.zoom);
      return ((logZoom - logMin) / (logMax - logMin)) * 100;
    }
  });

  function handleZoomSliderChange(e: Event) {
    if (!imageDesc || !container) return;
    const target = e.target as HTMLInputElement;
    const sliderValue = parseFloat(target.value);
    
    // Convert slider position (0-100) to zoom level (logarithmic)
    const logMin = Math.log(MIN_ZOOM);
    const logMax = Math.log(MAX_ZOOM);
    const logZoom = logMin + (sliderValue / 100) * (logMax - logMin);
    const newZoom = Math.exp(logZoom);
    
    // Apply zoom centered on viewport
    const rect = container.getBoundingClientRect();
    const centerX = rect.width / 2;
    const centerY = rect.height / 2;
    
    // Calculate zoom delta from current to new
    const zoomDelta = newZoom / viewport.zoom;
    viewport = zoomAround(viewport, centerX, centerY, zoomDelta, imageDesc.width, imageDesc.height);
    scheduleViewportUpdate();
  }

  function stopSliderPropagation(e: Event) {
    e.stopPropagation();
  }

  // Image adjustment settings - compute CSS filter string
  // Brightness: -100 to 100 maps to CSS brightness 0 to 2 (0 = black, 1 = normal, 2 = double)
  // Contrast: -100 to 100 maps to CSS contrast 0 to 2
  // Gamma: applied via a combination of brightness adjustment (approximation)
  let imageFilter = $derived(() => {
    const b = $imageSettings.brightness;
    const c = $imageSettings.contrast;
    const g = $imageSettings.gamma;
    
    // Map -100..100 to 0..2 for brightness and contrast
    const brightness = 1 + (b / 100);
    const contrast = 1 + (c / 100);
    
    // Gamma is approximated using brightness adjustment
    // gamma < 1 = brighter midtones, gamma > 1 = darker midtones
    // We'll use a subtle additional brightness shift
    const gammaBrightness = g !== 1 ? Math.pow(0.5, g - 1) : 1;
    
    const filters: string[] = [];
    if (brightness !== 1) filters.push(`brightness(${brightness.toFixed(2)})`);
    if (contrast !== 1) filters.push(`contrast(${contrast.toFixed(2)})`);
    if (gammaBrightness !== 1) filters.push(`brightness(${gammaBrightness.toFixed(2)})`);
    
    return filters.length > 0 ? filters.join(' ') : 'none';
  });

  // React to progressInfo changes for our slide
  $effect(() => {
    if (activeSlideId && progressInfo.has(activeSlideId)) {
      const info = progressInfo.get(activeSlideId)!;
      progressSteps = info.steps;
      progressTotal = info.total;
      progressUpdateTrigger = info.trigger;
    }
  });

  /**
   * Convert slide info to ImageDesc for the frusta protocol.
   */
  function slideInfoToImageDesc(tab: Tab): ImageDesc | null {
    const hex = tab.slideId.replace(/-/g, '');
    if (hex.length !== 32) return null;

    const bytes = new Uint8Array(16);
    for (let i = 0; i < 16; i++) {
      bytes[i] = parseInt(hex.substr(i * 2, 2), 16);
    }

    const maxDim = Math.max(tab.width, tab.height);
    const levels = Math.ceil(Math.log2(maxDim / TILE_SIZE)) + 1;

    return {
      id: bytes,
      width: tab.width,
      height: tab.height,
      levels,
    };
  }

  /**
   * Format bytes to human readable string (KB, MB, etc.)
   */
  function formatBytes(bytes: number): string {
    if (bytes < 1024) return `${bytes} B`;
    if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
    if (bytes < 1024 * 1024 * 1024) return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
    return `${(bytes / (1024 * 1024 * 1024)).toFixed(2)} GB`;
  }

  /**
   * Center the viewport on the current image.
   */
  function centerOnImage() {
    if (!imageDesc || !container) return;
    const rect = container.getBoundingClientRect();
    viewport = centerViewport(rect.width, rect.height, imageDesc.width, imageDesc.height);
  }

  /**
   * Close the currently open slide over the WebSocket, freeing the slot.
   */
  function closeCurrentSlide() {
    if (currentSlot !== null && client) {
      client.closeSlide(currentSlot);
    }
    currentSlot = null;
  }

  /**
   * Activate a tab: save the previous tab's viewport, close its slot,
   * then set up the new tab's slide for viewing.
   */
  function activateTab(tab: Tab) {
    const newImageDesc = slideInfoToImageDesc(tab);
    if (!newImageDesc) {
      loadError = 'Failed to parse slide info';
      return;
    }

    // Save the current tab's viewport before switching away
    if (activeTabHandle && activeTabHandle !== tab.tabId) {
      tabStore.saveViewport(activeTabHandle, {
        x: viewport.x,
        y: viewport.y,
        zoom: viewport.zoom,
      });
    }

    // Close the previous tab's slot
    closeCurrentSlide();

    const prevSlideId = activeSlideId;

    imageDesc = newImageDesc;
    activeTabHandle = tab.tabId;
    activeSlideId = tab.slideId;
    loadError = null;

    // Reset progress state for new slide
    progressSteps = 0;
    progressTotal = 0;

    // Swap to the shared cache for the new slide
    if (prevSlideId && prevSlideId !== tab.slideId) {
      releaseCache(prevSlideId);
    }
    if (prevSlideId !== tab.slideId) {
      cache = acquireCache(tab.slideId);
      cacheSize = cache.size;
      tilesReceived = cache.size;
    }

    // Restore saved viewport or center on the image
    if (tab.savedViewport) {
      viewport = { ...viewport, x: tab.savedViewport.x, y: tab.savedViewport.y, zoom: tab.savedViewport.zoom };
      if (container) {
        viewport = clampViewport(viewport, newImageDesc.width, newImageDesc.height);
      }
    } else if (container) {
      const rect = container.getBoundingClientRect();
      viewport = centerViewport(rect.width, rect.height, newImageDesc.width, newImageDesc.height);
    }

    // Open the slide on the WebSocket if connected
    openSlide();
  }

  // Subscribe to this pane's active tab
  let paneActiveTab = $state<Tab | null>(null);

  const unsubSplit = tabStore.splitState.subscribe((s) => {
    const pane = s.panes.find((p) => p.paneId === paneId);
    if (!pane || !pane.activeTabId) {
      paneActiveTab = null;
    } else {
      paneActiveTab = pane.tabs.find((t) => t.tabId === pane.activeTabId) ?? null;
    }
  });

  $effect(() => {
    if (!paneActiveTab) {
      closeCurrentSlide();
      if (activeSlideId) {
        releaseCache(activeSlideId);
        cache = null;
        cacheSize = 0;
        tilesReceived = 0;
      }
      imageDesc = null;
      activeTabHandle = null;
      activeSlideId = null;
      return;
    }
    if (paneActiveTab.tabId !== activeTabHandle || paneActiveTab.slideId !== activeSlideId) {
      activateTab(paneActiveTab);
    }
  });

  // Reactive trigger: when the WebSocket connects (or reconnects), ensure the
  // slide is open and a viewport update is sent so the backend starts streaming
  // tiles.  This covers the permalink-load case where `activateTab` allocates a
  // slot before the socket is ready — the open message is replayed by the
  // client's `reopenTrackedSlides`, but the viewport update was lost.
  //
  // Use scheduleViewportUpdate (debounced) rather than sendViewportUpdate
  // (immediate) to coalesce with other rapid-fire viewport updates during
  // initial layout (resize, center, etc.).  Without this, the server receives
  // many back-to-back updates that cancel each other's tile dispatches.
  $effect(() => {
    if (connectionState === 'connected' && imageDesc && activeTabHandle) {
      if (currentSlot === null) {
        // Slot not yet allocated — full open + viewport update
        openSlide();
      } else {
        // Slot was allocated before the connection was ready.  The client
        // already replayed the open message; we just need to push the
        // current viewport so the server knows which tiles to send.
        scheduleViewportUpdate();
      }
    }
  });

  function openSlide() {
    if (!client || !imageDesc) return;

    const dpi = window.devicePixelRatio * 96;
    const slot = client.openSlide(dpi, imageDesc);
    if (slot === -1) {
      loadError = 'No free slots available';
      return;
    }
    currentSlot = slot;
    // Re-register with updated slot
    registerHandler();
    // Use debounced update instead of immediate — setting currentSlot
    // (a $state variable) will re-trigger the $effect above, which also
    // calls scheduleViewportUpdate().  The two calls coalesce via the
    // shared timeout, so the server receives exactly one Update message
    // instead of two back-to-back (which would cause the second to cancel
    // the first's tile dispatches).
    scheduleViewportUpdate();
  }

  function handleTileReceived(tile: TileData) {
    if (!cache) return;
    const { bitmapReady } = cache.set(tile.meta, tile.data);
    cacheSize = cache.size;
    tilesReceived++;
    // Update memory metrics
    cacheMemoryBytes = cache.getMemoryUsage();
    pendingDecodes = cache.getPendingDecodeCount();
    // Update global store
    updatePerformanceMetrics({
      cacheMemoryBytes,
      pendingDecodes,
      tilesReceived,
      cacheSize,
    });
    // Trigger an immediate render so coarse fallbacks are displayed.
    renderTrigger++;
    // When the bitmap finishes decoding, trigger another render so the
    // crisp version replaces the blurry fallback (progressive loading).
    bitmapReady.then(() => {
      renderTrigger++;
      // Update pending decodes after decode completes
      if (cache) {
        pendingDecodes = cache.getPendingDecodeCount();
        cacheMemoryBytes = cache.getMemoryUsage();
        updatePerformanceMetrics({
          pendingDecodes,
          cacheMemoryBytes,
        });
      }
    });
  }

  function handleRenderMetrics(metrics: RenderMetrics) {
    renderMetrics = metrics;
    // Update global store with render metrics
    updatePerformanceMetrics({
      renderTimeMs: metrics.renderTimeMs,
      fps: metrics.fps,
      visibleTiles: metrics.visibleTiles,
      renderedTiles: metrics.renderedTiles,
      fallbackTiles: metrics.fallbackTiles,
      placeholderTiles: metrics.placeholderTiles,
    });
  }

  function registerHandler() {
    onRegisterTileHandler(paneId, {
      getSlot: () => currentSlot,
      handleTile: handleTileReceived,
    });
  }

  function sendViewportUpdate() {
    if (!client || currentSlot === null) return;
    client.updateViewport(currentSlot, toProtocolViewport(viewport));
  }

  /**
   * Cancel pending decodes for tiles that are no longer visible.
   * This is called when the viewport changes to avoid wasting CPU time
   * decoding tiles that have scrolled out of view.
   */
  function cancelNonVisibleDecodes() {
    if (!cache || !imageDesc) return;

    // Compute visible tiles at the ideal level and one level finer (for 2x DPI)
    const dpi = window.devicePixelRatio * 96;
    const idealLevel = computeIdealLevel(viewport.zoom, imageDesc.levels, dpi);
    const finerLevel = Math.max(0, idealLevel - 1);

    const idealTiles = visibleTilesForLevel(viewport, imageDesc, idealLevel);
    const finerTiles = finerLevel < idealLevel
      ? visibleTilesForLevel(viewport, imageDesc, finerLevel)
      : [];

    // Also include coarser levels as they're used for fallback rendering
    const coarserTiles = [];
    for (let level = idealLevel + 1; level < imageDesc.levels; level++) {
      coarserTiles.push(...visibleTilesForLevel(viewport, imageDesc, level));
    }

    const allVisibleTiles = [...finerTiles, ...idealTiles, ...coarserTiles];
    cache.cancelDecodesNotIn(allVisibleTiles);
  }

  function scheduleViewportUpdate() {
    if (viewportUpdateTimeout) {
      clearTimeout(viewportUpdateTimeout);
    }
    
    // Cancel decodes for tiles that are no longer visible IMMEDIATELY
    // (don't wait for the debounce) to free up decode capacity ASAP
    cancelNonVisibleDecodes();
    
    viewportUpdateTimeout = setTimeout(() => {
      sendViewportUpdate();
      // Keep the tab store's savedViewport in sync so that Copy Permalink
      // (and other consumers) always have the latest viewport.
      if (activeTabHandle) {
        tabStore.saveViewport(activeTabHandle, {
          x: viewport.x,
          y: viewport.y,
          zoom: viewport.zoom,
        });
      }
      viewportUpdateTimeout = null;
    }, VIEWPORT_UPDATE_DEBOUNCE_MS);
  }

  // Handler for minimap viewport changes
  function handleMinimapViewportChange(newViewport: ViewportState) {
    if (!imageDesc) return;
    viewport = clampViewport(newViewport, imageDesc.width, imageDesc.height);
    scheduleViewportUpdate();
  }

  // Mouse event handlers
  function handleMouseDown(e: MouseEvent) {
    if (e.button !== 0) return;
    isDragging = true;
    lastMouseX = e.clientX;
    lastMouseY = e.clientY;
    tabStore.setFocusedPane(paneId);
    e.preventDefault();
  }

  function handleMouseMove(e: MouseEvent) {
    if (!isDragging || !imageDesc) return;

    const deltaX = e.clientX - lastMouseX;
    const deltaY = e.clientY - lastMouseY;
    lastMouseX = e.clientX;
    lastMouseY = e.clientY;

    // Apply pan sensitivity from settings
    viewport = pan(viewport, deltaX * panSensitivityFactor, deltaY * panSensitivityFactor, imageDesc.width, imageDesc.height);
    scheduleViewportUpdate();
  }

  function handleMouseUp() {
    isDragging = false;
  }

  function handleWheel(e: WheelEvent) {
    if (!imageDesc) return;
    e.preventDefault();

    const rect = container.getBoundingClientRect();
    const mouseX = e.clientX - rect.left;
    const mouseY = e.clientY - rect.top;

    // Apply zoom sensitivity from settings
    const baseZoom = 1.15;
    const sensitiveZoom = 1 + (baseZoom - 1) * zoomSensitivityFactor;
    const zoomFactor = e.deltaY < 0 ? sensitiveZoom : 1 / sensitiveZoom;
    viewport = zoomAround(viewport, mouseX, mouseY, zoomFactor, imageDesc.width, imageDesc.height);
    scheduleViewportUpdate();
  }

  // HUD zoom controls - zoom centered on viewport
  function handleHudZoomIn() {
    if (!imageDesc || !container) return;
    const rect = container.getBoundingClientRect();
    const centerX = rect.width / 2;
    const centerY = rect.height / 2;
    const zoomFactor = 1.5; // Larger step for button click
    viewport = zoomAround(viewport, centerX, centerY, zoomFactor, imageDesc.width, imageDesc.height);
    scheduleViewportUpdate();
  }

  function handleHudZoomOut() {
    if (!imageDesc || !container) return;
    const rect = container.getBoundingClientRect();
    const centerX = rect.width / 2;
    const centerY = rect.height / 2;
    const zoomFactor = 1 / 1.5;
    viewport = zoomAround(viewport, centerX, centerY, zoomFactor, imageDesc.width, imageDesc.height);
    scheduleViewportUpdate();
  }

  function handleHudFitView() {
    centerOnImage();
    scheduleViewportUpdate();
  }

  // Touch event handlers for mobile
  let lastTouchDistance = 0;
  let lastTouchCenter = { x: 0, y: 0 };

  function handleTouchStart(e: TouchEvent) {
    if (e.touches.length === 1) {
      isDragging = true;
      lastMouseX = e.touches[0].clientX;
      lastMouseY = e.touches[0].clientY;
    } else if (e.touches.length === 2) {
      isDragging = false;
      lastTouchDistance = getTouchDistance(e.touches);
      lastTouchCenter = getTouchCenter(e.touches);
    }
    tabStore.setFocusedPane(paneId);
    e.preventDefault();
  }

  function handleTouchMove(e: TouchEvent) {
    if (!imageDesc) return;

    if (e.touches.length === 1 && isDragging) {
      const deltaX = e.touches[0].clientX - lastMouseX;
      const deltaY = e.touches[0].clientY - lastMouseY;
      lastMouseX = e.touches[0].clientX;
      lastMouseY = e.touches[0].clientY;

      viewport = pan(viewport, deltaX, deltaY, imageDesc.width, imageDesc.height);
      scheduleViewportUpdate();
    } else if (e.touches.length === 2) {
      const distance = getTouchDistance(e.touches);
      const center = getTouchCenter(e.touches);

      if (lastTouchDistance > 0) {
        const rect = container.getBoundingClientRect();
        const zoomFactor = distance / lastTouchDistance;
        const centerX = center.x - rect.left;
        const centerY = center.y - rect.top;

        viewport = zoomAround(viewport, centerX, centerY, zoomFactor, imageDesc.width, imageDesc.height);
        scheduleViewportUpdate();
      }

      lastTouchDistance = distance;
      lastTouchCenter = center;
    }

    e.preventDefault();
  }

  function handleTouchEnd(e: TouchEvent) {
    if (e.touches.length === 0) {
      isDragging = false;
      lastTouchDistance = 0;
    } else if (e.touches.length === 1) {
      isDragging = true;
      lastMouseX = e.touches[0].clientX;
      lastMouseY = e.touches[0].clientY;
      lastTouchDistance = 0;
    }
  }

  function getTouchDistance(touches: TouchList): number {
    const dx = touches[0].clientX - touches[1].clientX;
    const dy = touches[0].clientY - touches[1].clientY;
    return Math.sqrt(dx * dx + dy * dy);
  }

  function getTouchCenter(touches: TouchList): { x: number; y: number } {
    return {
      x: (touches[0].clientX + touches[1].clientX) / 2,
      y: (touches[0].clientY + touches[1].clientY) / 2,
    };
  }

  // Update viewport size on resize
  function updateViewportSize() {
    if (!container) return;
    const rect = container.getBoundingClientRect();
    viewport = { ...viewport, width: rect.width, height: rect.height };

    if (imageDesc) {
      viewport = clampViewport(viewport, imageDesc.width, imageDesc.height);
    }

    scheduleViewportUpdate();
  }

  let resizeObserver: ResizeObserver | null = null;

  onMount(() => {
    if (container) {
      const rect = container.getBoundingClientRect();
      viewport = { ...viewport, width: rect.width, height: rect.height };

      // Use ResizeObserver to handle pane resizing (from divider drag)
      resizeObserver = new ResizeObserver(() => {
        updateViewportSize();
      });
      resizeObserver.observe(container);
    }

    // Register tile handler
    registerHandler();

    window.addEventListener('mouseup', handleMouseUp);
  });

  onDestroy(() => {
    unsubSplit();
    onUnregisterTileHandler(paneId);
    closeCurrentSlide();
    if (activeSlideId) {
      releaseCache(activeSlideId);
    }
    resizeObserver?.disconnect();
    if (viewportUpdateTimeout) {
      clearTimeout(viewportUpdateTimeout);
    }
    if (browser) {
      window.removeEventListener('mouseup', handleMouseUp);
    }
  });
</script>

<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<!-- svelte-ignore a11y_no_noninteractive_tabindex -->
<div
  class="viewer-container"
  bind:this={container}
  onmousedown={handleMouseDown}
  onmousemove={handleMouseMove}
  onwheel={handleWheel}
  ontouchstart={handleTouchStart}
  ontouchmove={handleTouchMove}
  ontouchend={handleTouchEnd}
  role="application"
  tabindex="0"
  aria-label="Tile viewer - use mouse to pan, scroll to zoom"
>
  {#if imageDesc && cache}
    <!-- Image layer with brightness/contrast/gamma filters applied -->
    <div class="image-layer" style="filter: {imageFilter()}">
      <TileRenderer image={imageDesc} {viewport} {cache} {renderTrigger} client={client ?? undefined} slot={currentSlot ?? undefined} onMetrics={handleRenderMetrics} />
    </div>
    
    <!-- Scale bar (bottom-left) - controlled by settings -->
    <ScaleBar {viewport} />
    
    <!-- Viewer HUD overlay (top-left) -->
    <ViewerHud
      zoom={viewport.zoom}
      onZoomIn={handleHudZoomIn}
      onZoomOut={handleHudZoomOut}
      onFitView={handleHudFitView}
    />
    
    <!-- Minimap (bottom-right) - controlled by settings -->
    {#if minimapVisible}
      <div class="bottom-right-controls">
        <!-- Vertical zoom slider -->
        <!-- svelte-ignore a11y_no_static_element_interactions -->
        <div 
          class="zoom-slider-container"
          onmousedown={stopSliderPropagation}
          ontouchstart={stopSliderPropagation}
          onwheel={stopSliderPropagation}
        >
          <span class="zoom-slider-label">+</span>
          <input
            type="range"
            min="0"
            max="100"
            step="0.5"
            value={zoomSliderValue.value}
            oninput={handleZoomSliderChange}
            class="zoom-slider"
            aria-label="Zoom level"
          />
          <span class="zoom-slider-label">−</span>
        </div>
        <Minimap
          image={imageDesc}
          {viewport}
          {cache}
          {renderTrigger}
          onViewportChange={handleMinimapViewportChange}
        />
      </div>
    {/if}
  {:else}
    <div class="no-image">
      <h2>No Image Loaded</h2>
      <p>Select a slide from the sidebar, or add a slide ID to the URL:</p>
      <code>?slide=&lt;uuid&gt;</code>
    </div>
  {/if}

  <footer class="controls">
    <div class="stats">
      <span>Zoom: {(viewport.zoom * 100).toFixed(1)}%</span>
      {#if imageDesc}
        <span>Image: {imageDesc.width}×{imageDesc.height} ({imageDesc.levels} levels)</span>
      {/if}
      {#if progressTotal > 0 && progressSteps < progressTotal}
        <span class="progress-indicator"><ActivityIndicator trigger={progressUpdateTrigger} />Processing: {((progressSteps / progressTotal) * 100).toPrecision(3)}%</span>
      {/if}
      {#if loadError}
        <span class="error">{loadError}</span>
      {/if}
    </div>
  </footer>
</div>

<style>
  .viewer-container {
    flex: 1;
    position: relative;
    overflow: hidden;
    cursor: grab;
    touch-action: none;
    background: white;
    display: flex;
    flex-direction: column;
  }

  .viewer-container:active {
    cursor: grabbing;
  }

  /* Image layer wrapper for applying CSS filters (brightness/contrast/gamma) */
  .image-layer {
    position: absolute;
    inset: 0;
    z-index: 0;
  }

  .no-image {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    height: 100%;
    color: #888;
    text-align: center;
  }

  .no-image h2 {
    margin-bottom: 1rem;
  }

  .no-image code {
    background: #2a2a2a;
    padding: 0.5rem 1rem;
    border-radius: 4px;
    font-size: 0.875rem;
  }

  .controls {
    display: flex;
    flex-wrap: wrap;
    gap: 1rem;
    padding: 0.5rem 0.75rem;
    background: #1a1a1a;
    border-top: 1px solid #333;
    align-items: center;
    justify-content: flex-end;
    flex-shrink: 0;
  }

  .stats {
    display: flex;
    gap: 1rem;
    font-size: 0.8125rem;
    color: #aaa;
  }

  .progress-indicator {
    color: #f59e0b;
    font-weight: 500;
    display: inline-flex;
    align-items: center;
    gap: 0.375rem;
  }

  .error {
    color: #ef4444;
    margin: 0;
    font-size: 0.8125rem;
  }

  .bottom-right-controls {
    position: absolute;
    bottom: 1rem;
    right: 1rem;
    display: flex;
    align-items: center;
    gap: 0.75rem;
    z-index: 10;
  }

  .zoom-slider-container {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 0.25rem;
    padding: 0.5rem 0.25rem;
    background: rgba(20, 20, 20, 0.75);
    backdrop-filter: blur(12px);
    border-radius: 0.5rem;
    border: 1px solid rgba(255, 255, 255, 0.1);
  }

  .zoom-slider-label {
    font-size: 0.875rem;
    font-weight: 600;
    color: #9ca3af;
    user-select: none;
    line-height: 1;
  }

  .zoom-slider {
    writing-mode: vertical-lr;
    direction: rtl;
    width: 6px;
    height: 120px;
    appearance: none;
    background: #374151;
    border-radius: 3px;
    cursor: pointer;
    margin: 0.25rem 0;
  }

  .zoom-slider::-webkit-slider-thumb {
    appearance: none;
    width: 14px;
    height: 14px;
    background: #3b82f6;
    border-radius: 50%;
    cursor: pointer;
    transition: transform 0.1s;
  }

  .zoom-slider::-webkit-slider-thumb:hover {
    transform: scale(1.2);
  }

  .zoom-slider::-moz-range-thumb {
    width: 14px;
    height: 14px;
    background: #3b82f6;
    border: none;
    border-radius: 50%;
    cursor: pointer;
  }
</style>
