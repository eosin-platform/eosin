<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import { browser } from '$app/environment';
  import { replaceState } from '$app/navigation';
  import { env } from '$env/dynamic/public';
  import {
    createFrustaClient,
    type ConnectionState,
    type TileData,
    type ImageDesc,
    type ViewportState,
    type ProgressEvent,
    type SlideCreatedEvent,
    TileCache,
    TileRenderer,
    toProtocolViewport,
    zoomAround,
    pan,
    clampViewport,
    centerViewport,
    TILE_SIZE,
  } from '$lib/frusta';
  import Minimap from '$lib/components/Minimap.svelte';
  import ActivityIndicator from '$lib/components/ActivityIndicator.svelte';
  import { liveProgress } from '$lib/stores/progress';
  import { newSlides } from '$lib/stores/newSlides';
  import { tabStore, type Tab } from '$lib/stores/tabs';
  import type { SlideInfo } from './+page.server';

  // Server-provided data
  let { data } = $props<{ data: { slide: SlideInfo | null; error: string | null } }>();

  // Connection state
  let connectionState = $state<ConnectionState>('disconnected');
  let tilesReceived = $state(0);
  let cacheSize = $state(0);
  let lastError = $state<string | null>(null);
  
  // Progress state
  let progressSteps = $state(0);
  let progressTotal = $state(0);
  let progressUpdateTrigger = $state(0);

  // Toast notification state
  let toastMessage = $state<string | null>(null);
  let toastType = $state<'error' | 'success'>('error');
  let toastTimeout: ReturnType<typeof setTimeout> | null = null;
  let hasBeenConnected = false;

  function showToast(message: string, duration = 5000, type: 'error' | 'success' = 'error') {
    toastMessage = message;
    toastType = type;
    if (toastTimeout) {
      clearTimeout(toastTimeout);
    }
    toastTimeout = setTimeout(() => {
      toastMessage = null;
      toastTimeout = null;
    }, duration);
  }

  function dismissToast() {
    toastMessage = null;
    if (toastTimeout) {
      clearTimeout(toastTimeout);
      toastTimeout = null;
    }
  }

  // WebSocket endpoint from environment (required)
  const wsUrl = env.PUBLIC_FRUSTA_ENDPOINT!;

  // Image state from server data
  let imageDesc = $state<ImageDesc | null>(null);
  let currentSlot = $state<number | null>(null);
  let loadError = $state<string | null>(null);

  // Track the currently active tab handle (tabId) for the viewer
  let activeTabHandle = $state<string | null>(null);
  // The slide ID of the currently displayed slide (for URL sync and progress)
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
  let renderTrigger = $state(0);

  // Client instance
  let client = $state<ReturnType<typeof createFrustaClient> | null>(null);

  // Container ref for sizing
  let container: HTMLDivElement;

  // Debounce timer for viewport updates
  let viewportUpdateTimeout: ReturnType<typeof setTimeout> | null = null;
  const VIEWPORT_UPDATE_DEBOUNCE_MS = 16; // ~60fps

  // Debounce timer for URL query updates
  let urlUpdateTimeout: ReturnType<typeof setTimeout> | null = null;
  const URL_UPDATE_DEBOUNCE_MS = 300;

  /** Whether we've finished the initial URL-driven load (prevents re-centering) */
  let initialUrlApplied = false;

  /** Set to true after onMount — prevents replaceState calls during hydration */
  let mounted = false;

  /**
   * Update the browser URL query string to reflect the current slide + viewport.
   * Uses replaceState so it doesn't create new history entries on every pan/zoom.
   */
  function syncUrlToViewport() {
    if (!browser || !mounted || !activeSlideId) return;
    const params = new URLSearchParams();
    params.set('slide', activeSlideId);
    params.set('x', viewport.x.toFixed(1));
    params.set('y', viewport.y.toFixed(1));
    params.set('zoom', viewport.zoom.toPrecision(4));
    replaceState(`?${params.toString()}`, {});
  }

  function scheduleSyncUrl() {
    if (urlUpdateTimeout) clearTimeout(urlUpdateTimeout);
    urlUpdateTimeout = setTimeout(syncUrlToViewport, URL_UPDATE_DEBOUNCE_MS);
  }

  // Mouse interaction state
  let isDragging = false;
  let lastMouseX = 0;
  let lastMouseY = 0;

  /**
   * Convert server SlideInfo to ImageDesc for the frusta protocol.
   */
  function slideInfoToImageDesc(slide: SlideInfo): ImageDesc | null {
    const uuidBytes = parseUuid(slide.id);
    if (!uuidBytes) {
      console.error('Invalid UUID format:', slide.id);
      return null;
    }

    return {
      id: uuidBytes,
      width: slide.width,
      height: slide.height,
      levels: slide.levels,
    };
  }

  /**
   * Parse a UUID string (with or without dashes) to a 16-byte Uint8Array.
   */
  function parseUuid(uuidStr: string): Uint8Array | null {
    // Remove dashes
    const hex = uuidStr.replace(/-/g, '');
    if (hex.length !== 32) return null;

    const bytes = new Uint8Array(16);
    for (let i = 0; i < 16; i++) {
      bytes[i] = parseInt(hex.substr(i * 2, 2), 16);
    }
    return bytes;
  }

  /**
   * Format UUID bytes to string for display.
   */
  function formatUuid(bytes: Uint8Array): string {
    const hex = Array.from(bytes)
      .map((b) => b.toString(16).padStart(2, '0'))
      .join('');
    return `${hex.slice(0, 8)}-${hex.slice(8, 12)}-${hex.slice(12, 16)}-${hex.slice(16, 20)}-${hex.slice(20)}`;
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
      console.log(`Slide closed: handle=${activeTabHandle}, slot=${currentSlot}`);
    }
    currentSlot = null;
  }

  /**
   * Activate a tab: save the previous tab's viewport, close its slot,
   * then set up the new tab's slide for viewing.
   */
  function activateTab(tab: Tab) {
    const newImageDesc = slideInfoToImageDesc({
      id: tab.slideId,
      width: tab.width,
      height: tab.height,
      levels: computeLevels(tab.width, tab.height),
      filename: tab.label,
    });
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

    const sameSlide = activeSlideId === tab.slideId;

    imageDesc = newImageDesc;
    activeTabHandle = tab.tabId;
    activeSlideId = tab.slideId;
    loadError = null;

    // Reset progress state for new slide
    progressSteps = 0;
    progressTotal = 0;

    // Only clear tile cache when switching to a different slide
    if (!sameSlide && cache) {
      cache.clear();
      cacheSize = 0;
      tilesReceived = 0;
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

      // On initial load, apply viewport from URL query if present
      if (!initialUrlApplied && browser) {
        initialUrlApplied = true;
        const params = new URL(window.location.href).searchParams;
        const ux = params.get('x');
        const uy = params.get('y');
        const uz = params.get('zoom');
        if (ux !== null && uy !== null && uz !== null) {
          const px = parseFloat(ux);
          const py = parseFloat(uy);
          const pz = parseFloat(uz);
          if (isFinite(px) && isFinite(py) && isFinite(pz) && pz > 0) {
            viewport = { ...viewport, x: px, y: py, zoom: pz };
            viewport = clampViewport(viewport, newImageDesc.width, newImageDesc.height);
          }
        }
      }
    }

    scheduleSyncUrl();
    // Open the slide on the WebSocket if connected
    openSlide();
  }

  /**
   * Compute number of mip levels for an image pyramid.
   */
  function computeLevels(width: number, height: number): number {
    const maxDim = Math.max(width, height);
    return Math.ceil(Math.log2(maxDim / TILE_SIZE)) + 1;
  }

  // If server returned an error (e.g. invalid slide ID), surface it
  $effect(() => {
    if (!data.slide && data.error) {
      loadError = data.error;
      imageDesc = null;
    }
  });

  // Subscribe to the active tab so sidebar clicks actually load the slide.
  let activeTab = $state<Tab | null>(null);
  const unsubActiveTab = tabStore.activeTab.subscribe((tab) => {
    activeTab = tab;
  });

  $effect(() => {
    if (!activeTab) {
      // No tab open — close the current slide and clear URL
      const hadSlide = !!activeSlideId;
      closeCurrentSlide();
      imageDesc = null;
      activeTabHandle = null;
      activeSlideId = null;
      if (browser && mounted && hadSlide) {
        replaceState('/', {});
      }
      return;
    }
    // Activate if this is a different tab OR the active tab's slide changed
    // (tabStore.open() replaces the active tab's slideId in-place)
    if (activeTab.tabId !== activeTabHandle || activeTab.slideId !== activeSlideId) {
      activateTab(activeTab);
    }
  });

  // Reactive trigger: open the slide on the WebSocket when connection comes up
  // after activateTab() has already set imageDesc. This handles the race where
  // activateTab() fires before the WebSocket is connected.
  $effect(() => {
    if (connectionState === 'connected' && imageDesc && activeTabHandle && currentSlot === null) {
      openSlide();
    }
  });

  function connect() {
    if (client) {
      client.disconnect();
    }

    lastError = null;

    client = createFrustaClient({
      url: wsUrl,
      reconnectDelay: 1000,
      maxReconnectAttempts: 0, // Infinite retries
      connectTimeout: 6000,
      onStateChange: (state) => {
        connectionState = state;

        if (state === 'disconnected' || state === 'error') {
          // Server session is gone — reset local slot state.
          // currentSlideId is NOT cleared: the client tracks open slides
          // and will automatically re-send Open messages on reconnect.
          currentSlot = null;
        }

        if (state === 'connected') {
          if (hasBeenConnected) {
            showToast('Reconnected.', 3000, 'success');
          }
          hasBeenConnected = true;
          // openSlide() is handled by the reactive $effect that watches
          // connectionState + imageDesc, so it works regardless of ordering.
        }
      },
      onTile: (tile: TileData) => {
        tilesReceived++;
        handleTileReceived(tile);
      },
      onProgress: (event: ProgressEvent) => {
        const eventSlideId = formatUuid(event.slideId);
        // Update local progress display if this event is for the currently viewed slide
        if (eventSlideId === activeSlideId) {
          progressSteps = event.progressSteps;
          progressTotal = event.progressTotal;
          progressUpdateTrigger++;
        }
        // Publish to shared store so sidebar can display live progress for all slides
        liveProgress.set({
          slideId: eventSlideId,
          progressSteps: event.progressSteps,
          progressTotal: event.progressTotal,
          lastUpdate: Date.now(),
        });
      },
      onRateLimited: () => {
        showToast('You are being rate limited. Please slow down.', 5000);
      },
      onSlideCreated: (event: SlideCreatedEvent) => {
        console.log('Slide created:', event);
        newSlides.push({
          id: event.id,
          width: event.width,
          height: event.height,
          filename: event.filename,
          full_size: event.full_size,
          url: event.url,
          receivedAt: Date.now(),
        });
      },
      onError: (error) => {
        const msg = error instanceof Error ? error.message : 'Connection error';
        lastError = msg;
        showToast(msg);
        console.error('WebSocket error:', error);
      },
    });

    client.connect();
  }

  function openSlide() {
    if (!client || !imageDesc) return;

    const dpi = window.devicePixelRatio * 96;
    const slot = client.openSlide(dpi, imageDesc);
    if (slot === -1) {
      loadError = 'No free slots available';
      return;
    }
    currentSlot = slot;
    console.log(`Slide opened: handle=${activeTabHandle}, slot=${slot}`);
    // Send initial viewport update now that slot is assigned
    sendViewportUpdate();
  }

  async function handleTileReceived(tile: TileData) {
    if (!cache) return;
    await cache.set(tile.meta, tile.data);
    cacheSize = cache.size;
    // Trigger re-render
    renderTrigger++;
  }

  function sendViewportUpdate() {
    if (!client || currentSlot === null) return;
    client.updateViewport(currentSlot, toProtocolViewport(viewport));
  }

  function scheduleViewportUpdate() {
    if (viewportUpdateTimeout) {
      clearTimeout(viewportUpdateTimeout);
    }
    viewportUpdateTimeout = setTimeout(() => {
      sendViewportUpdate();
      viewportUpdateTimeout = null;
    }, VIEWPORT_UPDATE_DEBOUNCE_MS);
    scheduleSyncUrl();
  }

  // Handler for minimap viewport changes
  function handleMinimapViewportChange(newViewport: ViewportState) {
    if (!imageDesc) return;
    viewport = clampViewport(newViewport, imageDesc.width, imageDesc.height);
    scheduleViewportUpdate();
  }

  // Mouse event handlers
  function handleMouseDown(e: MouseEvent) {
    if (e.button !== 0) return; // Left button only
    isDragging = true;
    lastMouseX = e.clientX;
    lastMouseY = e.clientY;
    e.preventDefault();
  }

  function handleMouseMove(e: MouseEvent) {
    if (!isDragging || !imageDesc) return;

    const deltaX = e.clientX - lastMouseX;
    const deltaY = e.clientY - lastMouseY;
    lastMouseX = e.clientX;
    lastMouseY = e.clientY;

    viewport = pan(viewport, deltaX, deltaY, imageDesc.width, imageDesc.height);
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

    // Zoom factor: scroll up = zoom in, scroll down = zoom out
    const zoomFactor = e.deltaY < 0 ? 1.15 : 1 / 1.15;

    viewport = zoomAround(viewport, mouseX, mouseY, zoomFactor, imageDesc.width, imageDesc.height);
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
      // Pinch zoom
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

  onMount(() => {
    // Initialize cache
    const tileCache = new TileCache({
      maxTiles: 1000,
      onTileCached: () => {
        cacheSize = tileCache.size;
      },
    });
    cache = tileCache;

    // Set initial viewport size from container
    if (container) {
      const rect = container.getBoundingClientRect();
      viewport = { ...viewport, width: rect.width, height: rect.height };
    }

    // Load initial slide from server-provided data.
    // If it came from a permalink URL (?slide=...), also open a tab for it.
    if (data.error) {
      loadError = data.error;
    } else if (data.slide) {
      // Open a tab so the slide appears in the tab bar
      tabStore.open(
        data.slide.id,
        data.slide.filename,
        data.slide.width,
        data.slide.height,
      );
      // loadSlide will be triggered by the activeTab effect above
    }

    // Always auto-connect on page load
    connect();

    // Listen for resize
    window.addEventListener('resize', updateViewportSize);

    // Global mouse up to handle dragging outside container
    window.addEventListener('mouseup', handleMouseUp);

    // Allow URL syncing now that the component is fully mounted
    mounted = true;
  });

  onDestroy(() => {
    unsubActiveTab();
    closeCurrentSlide();
    client?.disconnect();
    cache?.clear();
    if (viewportUpdateTimeout) {
      clearTimeout(viewportUpdateTimeout);
    }
    if (urlUpdateTimeout) {
      clearTimeout(urlUpdateTimeout);
    }
    if (toastTimeout) {
      clearTimeout(toastTimeout);
    }
    if (browser) {
      window.removeEventListener('resize', updateViewportSize);
      window.removeEventListener('mouseup', handleMouseUp);
    }
  });
</script>

<main>
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
      <TileRenderer image={imageDesc} {viewport} {cache} {renderTrigger} client={client ?? undefined} slot={currentSlot ?? undefined} />
      <Minimap
        image={imageDesc}
        {viewport}
        {cache}
        {renderTrigger}
        onViewportChange={handleMinimapViewportChange}
      />
    {:else}
      <div class="no-image">
        <h2>No Image Loaded</h2>
        <p>Select a slide from the sidebar, or add a slide ID to the URL:</p>
        <code>?slide=&lt;uuid&gt;</code>
      </div>
    {/if}
  </div>

  <footer class="controls">
    <div class="stats">
      <span>Tiles: {tilesReceived}</span>
      <span>Cache: {cacheSize}</span>
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

    <div class="connection-status">
      <span class="status">
        {#if connectionState === 'connecting'}
          <span class="spinner"></span>
        {:else}
          <span
            class="status-indicator"
            class:connected={connectionState === 'connected'}
            class:error={connectionState === 'error' || connectionState === 'disconnected'}
          ></span>
        {/if}
        {connectionState}
      </span>
    </div>
  </footer>

  {#if toastMessage}
    <div class="toast {toastType === 'success' ? 'toast-success' : ''}" role="alert">
      <span class="toast-message">{toastMessage}</span>
      <button class="toast-dismiss" onclick={dismissToast} aria-label="Dismiss">
        ×
      </button>
    </div>
  {/if}
</main>

<style>
  :global(body) {
    margin: 0;
    padding: 0;
    overflow: hidden;
    background: #0a0a0a;
    color: #eee;
    font-family: system-ui, -apple-system, sans-serif;
  }

  main {
    display: flex;
    flex-direction: column;
    height: 100%;
    flex: 1;
    position: relative;
  }

  .toast {
    position: absolute;
    bottom: 1.5rem;
    left: 50%;
    transform: translateX(-50%);
    display: flex;
    align-items: center;
    gap: 0.75rem;
    padding: 0.75rem 1rem;
    background: #dc2626;
    color: white;
    border-radius: 8px;
    box-shadow: 0 4px 12px rgba(0, 0, 0, 0.4);
    font-size: 0.875rem;
    z-index: 1000;
    animation: slideUp 0.2s ease-out;
  }

  @keyframes slideUp {
    from {
      opacity: 0;
      transform: translateX(-50%) translateY(1rem);
    }
    to {
      opacity: 1;
      transform: translateX(-50%) translateY(0);
    }
  }

  .toast-success {
    background: #16a34a;
  }

  .toast-message {
    max-width: 400px;
  }

  .toast-dismiss {
    background: none;
    border: none;
    color: white;
    font-size: 1.25rem;
    cursor: pointer;
    padding: 0;
    line-height: 1;
    opacity: 0.8;
  }

  .toast-dismiss:hover {
    opacity: 1;
  }

  .controls {
    display: flex;
    flex-wrap: wrap;
    gap: 1rem;
    padding: 0.75rem 1rem;
    background: #1a1a1a;
    border-top: 1px solid #333;
    align-items: center;
    justify-content: space-between;
  }

  .connection-status {
    display: flex;
    gap: 0.5rem;
    align-items: center;
  }

  .stats {
    display: flex;
    gap: 1rem;
    font-size: 0.875rem;
    color: #aaa;
  }

  .progress-indicator {
    color: #f59e0b;
    font-weight: 500;
    display: inline-flex;
    align-items: center;
    gap: 0.375rem;
  }

  .spinner {
    width: 14px;
    height: 14px;
    border: 2px solid #333;
    border-top-color: #0066cc;
    border-radius: 50%;
    animation: spin 0.8s linear infinite;
  }

  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }

  .status {
    display: flex;
    align-items: center;
    gap: 0.375rem;
    font-size: 0.875rem;
  }

  .status-indicator {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background-color: #666;
  }

  .status-indicator.connected {
    background-color: #22c55e;
  }

  .status-indicator.error {
    background-color: #ef4444;
  }

  .error {
    color: #ef4444;
    margin: 0;
    font-size: 0.875rem;
  }

  .viewer-container {
    flex: 1;
    position: relative;
    overflow: hidden;
    cursor: grab;
    touch-action: none;
    background: white;
  }

  .viewer-container:active {
    cursor: grabbing;
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
</style>
