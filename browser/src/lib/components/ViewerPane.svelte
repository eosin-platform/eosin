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
  import MeasurementOverlay from '$lib/components/viewer/MeasurementOverlay.svelte';
  import AnnotationOverlay from '$lib/components/viewer/AnnotationOverlay.svelte';
  import ViewportContextMenu from '$lib/components/ViewportContextMenu.svelte';
  import { tabStore, type Tab } from '$lib/stores/tabs';
  import { acquireCache, releaseCache } from '$lib/stores/slideCache';
  import { updatePerformanceMetrics } from '$lib/stores/metrics';
  import { settings, navigationSettings, imageSettings, helpMenuOpen, type StainNormalization, type StainEnhancementMode } from '$lib/stores/settings';

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

  // Measurement tool state
  interface MeasurementState {
    active: boolean;
    mode: 'drag' | 'toggle' | null;
    startScreen: { x: number; y: number } | null;
    endScreen: { x: number; y: number } | null;
    startImage: { x: number; y: number } | null;
    endImage: { x: number; y: number } | null;
  }
  
  let measurement = $state<MeasurementState>({
    active: false,
    mode: null,
    startScreen: null,
    endScreen: null,
    startImage: null,
    endImage: null,
  });

  // Progress
  let progressSteps = $state(0);
  let progressTotal = $state(0);
  let progressUpdateTrigger = $state(0);

  // Context menu state for viewport
  let contextMenuVisible = $state(false);
  let contextMenuX = $state(0);
  let contextMenuY = $state(0);

  // Long press state for mobile context menu
  let longPressTimer: ReturnType<typeof setTimeout> | null = null;
  const LONG_PRESS_MS = 500;

  // Settings-derived values for zoom/pan sensitivity
  const sensitivityMap = { low: 0.5, medium: 1.0, high: 2.0 };
  let zoomSensitivityFactor = $derived(sensitivityMap[$navigationSettings.zoomSensitivity] || 1.0);
  let panSensitivityFactor = $derived(sensitivityMap[$navigationSettings.panSensitivity] || 1.0);
  let minimapVisible = $derived($navigationSettings.minimapVisible);
  
  // Stain enhancement mode from image settings
  let stainEnhancement = $derived($imageSettings.stainEnhancement);
  
  // Stain normalization mode from image settings
  let stainNormalization = $derived($imageSettings.stainNormalization);

  // HUD notification state for keyboard shortcut feedback
  let hudNotification = $state<string | null>(null);
  let hudNotificationTimeout: ReturnType<typeof setTimeout> | null = null;
  let hudNotificationFading = $state(false);

  // Normalization modes for cycling with 'n' key
  const normalizationModes: StainNormalization[] = ['none', 'macenko', 'vahadane'];

  // Enhancement modes for cycling with 'e' key
  const enhancementModes: StainEnhancementMode[] = ['none', 'gram', 'afb', 'gms'];
  const enhancementModeNames: Record<StainEnhancementMode, string> = {
    none: 'None',
    gram: 'Gram Stain',
    afb: 'AFB <span class="dim">(Acid-Fast Bacilli)</span>',
    gms: 'GMS <span class="dim">(Grocott Methenamine Silver)</span>',
  };

  function showHudNotification(message: string) {
    // Clear any existing timeout
    if (hudNotificationTimeout) {
      clearTimeout(hudNotificationTimeout);
    }
    
    // Show notification
    hudNotification = message;
    hudNotificationFading = false;
    
    // After 800ms, start fade out
    hudNotificationTimeout = setTimeout(() => {
      hudNotificationFading = true;
      // After 600ms fade, hide completely
      hudNotificationTimeout = setTimeout(() => {
        hudNotification = null;
        hudNotificationFading = false;
        hudNotificationTimeout = null;
      }, 600);
    }, 800);
  }

  function cycleNormalization() {
    const currentIndex = normalizationModes.indexOf($imageSettings.stainNormalization);
    const nextIndex = (currentIndex + 1) % normalizationModes.length;
    const nextMode = normalizationModes[nextIndex];
    
    settings.setSetting('image', 'stainNormalization', nextMode);
    
    // Show notification
    if (nextMode === 'none') {
      showHudNotification('Normalization disabled');
    } else {
      const modeName = nextMode.charAt(0).toUpperCase() + nextMode.slice(1);
      showHudNotification(`Normalization: ${modeName}`);
    }
  }

  function cycleEnhancement() {
    const currentIndex = enhancementModes.indexOf($imageSettings.stainEnhancement);
    const nextIndex = (currentIndex + 1) % enhancementModes.length;
    const nextMode = enhancementModes[nextIndex];
    
    settings.setSetting('image', 'stainEnhancement', nextMode);
    
    // Show notification with full name
    if (nextMode === 'none') {
      showHudNotification('Enhancement disabled');
    } else {
      showHudNotification(`Enhancement: ${enhancementModeNames[nextMode]}`);
    }
  }

  function handleKeyDown(e: KeyboardEvent) {
    // Ignore if user is typing in an input field
    const target = e.target as HTMLElement;
    if (target.tagName === 'INPUT' || target.tagName === 'TEXTAREA' || target.isContentEditable) {
      return;
    }
    
    if (e.key === 'n' || e.key === 'N') {
      cycleNormalization();
    }
    if (e.key === 'e' || e.key === 'E') {
      cycleEnhancement();
    }
    if (e.key === 'h' || e.key === 'H') {
      e.preventDefault();
      helpMenuOpen.update(v => !v);
    }
    // 'd' key toggles measurement mode
    if (e.key === 'd' || e.key === 'D') {
      if (!imageDesc || !container) return;
      
      if (measurement.active && measurement.mode === 'toggle') {
        // Cancel measurement if already in toggle mode
        cancelMeasurement();
      } else {
        // Start toggle measurement at current mouse position
        // We'll use the center of the container as default if no mouse position available
        const rect = container.getBoundingClientRect();
        
        // Get current mouse position from last known position or use center
        const screenX = lastMouseX || (rect.left + rect.width / 2);
        const screenY = lastMouseY || (rect.top + rect.height / 2);
        const imagePos = screenToImage(screenX, screenY);
        
        measurement = {
          active: true,
          mode: 'toggle',
          startScreen: { x: screenX, y: screenY },
          endScreen: { x: screenX, y: screenY },
          startImage: imagePos,
          endImage: imagePos,
        };
      }
    }
    // Escape closes help and cancels measurement
    if (e.key === 'Escape') {
      if ($helpMenuOpen) {
        helpMenuOpen.set(false);
      }
      if (measurement.active) {
        cancelMeasurement();
      }
    }
  }

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

    // Hide help when changing slides
    helpMenuOpen.set(false);

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

  // Convert screen coordinates to image coordinates (level 0 pixels)
  function screenToImage(screenX: number, screenY: number): { x: number; y: number } {
    const rect = container.getBoundingClientRect();
    const relX = screenX - rect.left;
    const relY = screenY - rect.top;
    const imageX = viewport.x + relX / viewport.zoom;
    const imageY = viewport.y + relY / viewport.zoom;
    return { x: imageX, y: imageY };
  }

  // Cancel any active measurement
  function cancelMeasurement() {
    measurement = {
      active: false,
      mode: null,
      startScreen: null,
      endScreen: null,
      startImage: null,
      endImage: null,
    };
  }

  // Mouse event handlers
  function handleMouseDown(e: MouseEvent) {
    // Middle mouse button (button 1) - start drag measurement
    if (e.button === 1) {
      e.preventDefault();
      const imagePos = screenToImage(e.clientX, e.clientY);
      measurement = {
        active: true,
        mode: 'drag',
        startScreen: { x: e.clientX, y: e.clientY },
        endScreen: { x: e.clientX, y: e.clientY },
        startImage: imagePos,
        endImage: imagePos,
      };
      return;
    }

    // Left mouse button - regular pan, but also cancel toggle measurement
    if (e.button === 0) {
      // Cancel toggle measurement mode on click
      if (measurement.active && measurement.mode === 'toggle') {
        cancelMeasurement();
      }
      isDragging = true;
      lastMouseX = e.clientX;
      lastMouseY = e.clientY;
      tabStore.setFocusedPane(paneId);
      helpMenuOpen.set(false);
      e.preventDefault();
    }
  }

  function handleMouseMove(e: MouseEvent) {
    // Handle measurement mode (both drag and toggle)
    if (measurement.active && measurement.startImage) {
      const imagePos = screenToImage(e.clientX, e.clientY);
      measurement = {
        ...measurement,
        endScreen: { x: e.clientX, y: e.clientY },
        endImage: imagePos,
      };
      
      // In drag mode, don't pan
      if (measurement.mode === 'drag') {
        return;
      }
    }

    // Regular pan handling
    if (!isDragging || !imageDesc) return;

    const deltaX = e.clientX - lastMouseX;
    const deltaY = e.clientY - lastMouseY;
    lastMouseX = e.clientX;
    lastMouseY = e.clientY;

    // If panning during toggle measurement, cancel the measurement
    if (measurement.active && measurement.mode === 'toggle') {
      cancelMeasurement();
    }

    // Apply pan sensitivity from settings
    viewport = pan(viewport, deltaX * panSensitivityFactor, deltaY * panSensitivityFactor, imageDesc.width, imageDesc.height);
    scheduleViewportUpdate();
  }

  function handleMouseUp(e?: MouseEvent) {
    // Middle mouse button released - end drag measurement
    if (e && e.button === 1 && measurement.active && measurement.mode === 'drag') {
      cancelMeasurement();
      return;
    }

    isDragging = false;
  }

  // Window event handlers (with event parameter)
  function handleWindowMouseUp(e: MouseEvent) {
    handleMouseUp(e);
  }

  function handleWindowMouseMove(e: MouseEvent) {
    // Track mouse position globally for 'd' key to use current mouse position
    lastMouseX = e.clientX;
    lastMouseY = e.clientY;
  }

  function handleWheel(e: WheelEvent) {
    if (!imageDesc) return;
    e.preventDefault();
    helpMenuOpen.set(false);

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

  // HUD zoom change - set zoom to specific level centered on viewport
  function handleHudZoomChange(newZoom: number) {
    if (!imageDesc || !container) return;
    const rect = container.getBoundingClientRect();
    const centerX = rect.width / 2;
    const centerY = rect.height / 2;
    
    // Clamp zoom to valid range
    const clampedZoom = Math.max(MIN_ZOOM, Math.min(MAX_ZOOM, newZoom));
    const zoomDelta = clampedZoom / viewport.zoom;
    viewport = zoomAround(viewport, centerX, centerY, zoomDelta, imageDesc.width, imageDesc.height);
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
    cancelLongPress();
    
    if (e.touches.length === 1) {
      isDragging = true;
      lastMouseX = e.touches[0].clientX;
      lastMouseY = e.touches[0].clientY;
      
      // Start longpress timer for context menu (only when viewing an image)
      if (imageDesc) {
        const touch = e.touches[0];
        longPressTimer = setTimeout(() => {
          longPressTimer = null;
          isDragging = false;
          showContextMenu(touch.clientX, touch.clientY);
        }, LONG_PRESS_MS);
      }
    } else if (e.touches.length === 2) {
      isDragging = false;
      lastTouchDistance = getTouchDistance(e.touches);
      lastTouchCenter = getTouchCenter(e.touches);
    }
    tabStore.setFocusedPane(paneId);
    helpMenuOpen.set(false);
    e.preventDefault();
  }

  function handleTouchMove(e: TouchEvent) {
    // Cancel long press if finger moves
    cancelLongPress();
    
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
    cancelLongPress();
    
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

  // Context menu handlers
  let contextMenuImageX = $state<number | undefined>(undefined);
  let contextMenuImageY = $state<number | undefined>(undefined);

  function showContextMenu(x: number, y: number) {
    contextMenuX = x;
    contextMenuY = y;
    // Convert to image coordinates
    if (container && viewport) {
      const imagePos = screenToImage(x, y);
      contextMenuImageX = imagePos.x;
      contextMenuImageY = imagePos.y;
    }
    contextMenuVisible = true;
  }

  function handleContextMenu(e: MouseEvent) {
    e.preventDefault();
    e.stopPropagation();
    if (!imageDesc) return;
    showContextMenu(e.clientX, e.clientY);
  }

  function handleContextMenuClose() {
    contextMenuVisible = false;
    contextMenuImageX = undefined;
    contextMenuImageY = undefined;
  }

  function cancelLongPress() {
    if (longPressTimer) {
      clearTimeout(longPressTimer);
      longPressTimer = null;
    }
  }

  async function handleSaveImage() {
    const canvas = container?.querySelector('canvas') as HTMLCanvasElement | null;
    if (!canvas) return;

    try {
      const blob = await new Promise<Blob | null>((resolve) => {
        canvas.toBlob(resolve, 'image/png');
      });
      if (!blob) return;

      const url = URL.createObjectURL(blob);
      const link = document.createElement('a');
      link.href = url;
      link.download = `${activeSlideId || 'viewport'}.png`;
      document.body.appendChild(link);
      link.click();
      document.body.removeChild(link);
      URL.revokeObjectURL(url);
    } catch (err) {
      console.error('Failed to save image:', err);
    }
  }

  async function handleCopyImage() {
    const canvas = container?.querySelector('canvas') as HTMLCanvasElement | null;
    if (!canvas) return;

    try {
      const blob = await new Promise<Blob | null>((resolve) => {
        canvas.toBlob(resolve, 'image/png');
      });
      if (!blob) return;

      await navigator.clipboard.write([
        new ClipboardItem({ 'image/png': blob })
      ]);
      
      showHudNotification('Image copied to clipboard');
    } catch (err) {
      console.error('Failed to copy image:', err);
      showHudNotification('Failed to copy image');
    }
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

    window.addEventListener('mouseup', handleWindowMouseUp);
    window.addEventListener('keydown', handleKeyDown, true);
    window.addEventListener('mousemove', handleWindowMouseMove);
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
    if (hudNotificationTimeout) {
      clearTimeout(hudNotificationTimeout);
    }
    cancelLongPress();
    if (browser) {
      window.removeEventListener('mouseup', handleWindowMouseUp);
      window.removeEventListener('keydown', handleKeyDown, true);
      window.removeEventListener('mousemove', handleWindowMouseMove);
    }
  });
</script>

<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<!-- svelte-ignore a11y_no_noninteractive_tabindex -->
<div
  class="viewer-container"
  class:measuring={measurement.active}
  class:measuring-toggle={measurement.active && measurement.mode === 'toggle'}
  bind:this={container}
  onmousedown={handleMouseDown}
  onmousemove={handleMouseMove}
  onwheel={handleWheel}
  oncontextmenu={handleContextMenu}
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
      <TileRenderer image={imageDesc} {viewport} {cache} {renderTrigger} {stainNormalization} {stainEnhancement} client={client ?? undefined} slot={currentSlot ?? undefined} onMetrics={handleRenderMetrics} />
    </div>
    
    <!-- Scale bar (bottom-left) - controlled by settings -->
    <ScaleBar {viewport} />
    
    <!-- Measurement overlay -->
    <MeasurementOverlay {viewport} {measurement} />
    
    <!-- Annotation overlay -->
    <AnnotationOverlay
      viewportX={viewport.x}
      viewportY={viewport.y}
      viewportZoom={viewport.zoom}
      containerWidth={viewport.width}
      containerHeight={viewport.height}
    />
    
    <!-- Viewer HUD overlay (top-left) -->
    <ViewerHud
      zoom={viewport.zoom}
      onZoomChange={handleHudZoomChange}
      onFitView={handleHudFitView}
    />
    
    <!-- Keyboard shortcut notification (center) -->
    {#if hudNotification}
      <div class="hud-notification" class:fading={hudNotificationFading}>
        {@html hudNotification}
      </div>
    {/if}
    
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
    <div class="welcome-screen">
      <div class="welcome-content">
        <img src="/logo_full.png" alt="Eosin Logo" class="welcome-logo" />
        <h2>Welcome to Eosin.</h2>
        <p class="welcome-subtitle">Multi-gigapixel histopathology at your fingertips.</p>
        <div class="getting-started">
          <h3>Getting Started</h3>
          <ul>
            <li><strong>Browse slides:</strong> Open the sidebar to browse available slides</li>
            <li><strong>Open a slide:</strong> Click on any slide in the sidebar to view it</li>
            <li><strong>Navigate:</strong> Drag to pan, scroll to zoom, or use the minimap</li>
            <li><strong>Keyboard shortcuts:</strong> Press <kbd>H</kbd> for help</li>
          </ul>
        </div>
      </div>
    </div>
  {/if}

  {#if imageDesc}
    <footer class="controls">
      <div class="stats">
        <span>Zoom: {(viewport.zoom * 100).toFixed(1)}%</span>
        <span>Image: {imageDesc.width}×{imageDesc.height} ({imageDesc.levels} levels)</span>
        {#if progressTotal > 0 && progressSteps < progressTotal}
          <span class="progress-indicator"><ActivityIndicator trigger={progressUpdateTrigger} />Processing: {((progressSteps / progressTotal) * 100).toPrecision(3)}%</span>
        {/if}
        {#if loadError}
          <span class="error">{loadError}</span>
        {/if}
      </div>
    </footer>
  {/if}

  <!-- Viewport context menu (right-click / longpress) -->
  <ViewportContextMenu
    x={contextMenuX}
    y={contextMenuY}
    visible={contextMenuVisible}
    imageX={contextMenuImageX}
    imageY={contextMenuImageY}
    onSaveImage={handleSaveImage}
    onCopyImage={handleCopyImage}
    onClose={handleContextMenuClose}
  />
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

  /* Measurement mode cursor */
  .viewer-container.measuring {
    cursor: crosshair;
  }

  .viewer-container.measuring:active {
    cursor: crosshair;
  }

  .viewer-container.measuring-toggle {
    cursor: crosshair;
  }

  /* Image layer wrapper for applying CSS filters (brightness/contrast/gamma) */
  .image-layer {
    position: absolute;
    inset: 0;
    z-index: 0;
  }

  .welcome-screen {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    height: 100%;
    background: 
      linear-gradient(135deg, rgba(15, 15, 25, 0.92) 0%, rgba(20, 25, 40, 0.88) 50%, rgba(15, 20, 35, 0.92) 100%),
      url('/background.webp');
    background-size: cover, cover;
    background-position: center, center;
    background-repeat: no-repeat, no-repeat;
    padding: 2rem;
    box-sizing: border-box;
  }

  .welcome-content {
    display: flex;
    flex-direction: column;
    align-items: center;
    text-align: center;
    max-width: 500px;
  }

  .welcome-logo {
    max-width: 280px;
    width: 100%;
    height: auto;
    margin-bottom: 1.5rem;
    filter: drop-shadow(0 4px 12px rgba(0, 0, 0, 0.3));
  }

  .welcome-screen h2 {
    color: #e8e8e8;
    font-size: 1.75rem;
    font-weight: 600;
    margin: 0 0 0.5rem 0;
  }

  .welcome-subtitle {
    color: #94a3b8;
    font-size: 1rem;
    margin: 0 0 2rem 0;
  }

  .getting-started {
    background: rgba(255, 255, 255, 0.05);
    border: 1px solid rgba(255, 255, 255, 0.1);
    border-radius: 12px;
    padding: 1.25rem 1.5rem;
    width: 100%;
    text-align: left;
    margin-bottom: 1.5rem;
  }

  .getting-started h3 {
    color: #e2e8f0;
    font-size: 1rem;
    font-weight: 600;
    margin: 0 0 0.75rem 0;
  }

  .getting-started ul {
    list-style: none;
    padding: 0;
    margin: 0;
  }

  .getting-started li {
    color: #cbd5e1;
    font-size: 0.875rem;
    padding: 0.5rem 0;
    border-bottom: 1px solid rgba(255, 255, 255, 0.05);
  }

  .getting-started li:last-child {
    border-bottom: none;
    padding-bottom: 0;
  }

  .getting-started li strong {
    color: #60a5fa;
  }

  .getting-started kbd {
    display: inline-block;
    background: rgba(255, 255, 255, 0.1);
    border: 1px solid rgba(255, 255, 255, 0.2);
    border-radius: 4px;
    padding: 0.125rem 0.375rem;
    font-family: 'SF Mono', 'Monaco', 'Inconsolata', 'Fira Code', monospace;
    font-size: 0.75rem;
    color: #e2e8f0;
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

  /* HUD notification for keyboard shortcuts */
  .hud-notification {
    position: absolute;
    top: 50%;
    left: 50%;
    transform: translate(-50%, -50%);
    background: rgba(0, 0, 0, 0.8);
    backdrop-filter: blur(8px);
    color: #fff;
    padding: 0.75rem 1.5rem;
    border-radius: 0.5rem;
    font-size: 1rem;
    font-weight: 500;
    pointer-events: none;
    z-index: 100;
    opacity: 1;
    transition: opacity 600ms ease-out;
  }

  .hud-notification.fading {
    opacity: 0;
  }

  .hud-notification :global(.dim) {
    opacity: 0.6;
  }

  /* Responsive styles for welcome screen - height constrained */
  @media (max-height: 600px) {
    .welcome-screen {
      padding: 1rem;
    }

    .welcome-logo {
      max-width: 160px;
      margin-bottom: 0.75rem;
    }

    .welcome-screen h2 {
      font-size: 1.25rem;
      margin-bottom: 0.25rem;
    }

    .welcome-subtitle {
      font-size: 0.875rem;
      margin-bottom: 1rem;
    }

    .getting-started {
      padding: 0.75rem 1rem;
      margin-bottom: 0.75rem;
    }

    .getting-started h3 {
      font-size: 0.875rem;
      margin-bottom: 0.5rem;
    }

    .getting-started li {
      font-size: 0.8125rem;
      padding: 0.375rem 0;
    }
  }

  /* Responsive styles for welcome screen - very height constrained */
  @media (max-height: 480px) {
    .welcome-screen {
      padding: 0.5rem;
      justify-content: flex-start;
      overflow: hidden;
    }

    .welcome-content {
      max-width: 100%;
    }

    .welcome-logo {
      max-width: 100px;
      margin-bottom: 0.5rem;
    }

    .welcome-screen h2 {
      font-size: 1rem;
    }

    .welcome-subtitle {
      font-size: 0.75rem;
      margin-bottom: 0.5rem;
    }

    .getting-started {
      padding: 0.5rem 0.75rem;
      margin-bottom: 0.5rem;
    }

    .getting-started h3 {
      font-size: 0.8125rem;
      margin-bottom: 0.375rem;
    }

    .getting-started li {
      font-size: 0.75rem;
      padding: 0.25rem 0;
    }
  }

  /* Responsive styles for welcome screen - width constrained */
  @media (max-width: 480px) {
    .welcome-screen {
      padding: 1rem;
    }

    .welcome-content {
      max-width: 100%;
      width: 100%;
    }

    .welcome-logo {
      max-width: 200px;
      margin-bottom: 1rem;
    }

    .welcome-screen h2 {
      font-size: 1.375rem;
    }

    .welcome-subtitle {
      font-size: 0.875rem;
      margin-bottom: 1rem;
    }

    .getting-started {
      padding: 1rem;
    }

    .getting-started li {
      font-size: 0.8125rem;
    }
  }

  /* Responsive styles for welcome screen - small mobile (both constrained) */
  @media (max-width: 380px), (max-height: 400px) {
    .welcome-screen {
      padding: 0.5rem;
    }

    .welcome-logo {
      max-width: 80px;
      margin-bottom: 0.5rem;
    }

    .welcome-screen h2 {
      font-size: 1rem;
    }

    .welcome-subtitle {
      font-size: 0.75rem;
      margin-bottom: 0.5rem;
    }

    .getting-started {
      padding: 0.5rem 0.75rem;
      margin-bottom: 0;
    }

    .getting-started h3 {
      font-size: 0.75rem;
      margin-bottom: 0.25rem;
    }

    .getting-started li {
      font-size: 0.6875rem;
      padding: 0.25rem 0;
    }

    .getting-started kbd {
      font-size: 0.625rem;
      padding: 0.0625rem 0.25rem;
    }
  }
</style>
