<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import { browser } from '$app/environment';
  import { env } from '$env/dynamic/public';
  import {
    createFrustaClient,
    type ConnectionState,
    type TileData,
    type ImageDesc,
    type ViewportState,
    TileCache,
    TileRenderer,
    toProtocolViewport,
    zoomAround,
    pan,
    clampViewport,
  } from '$lib/frusta';
  import type { SlideInfo } from './+page.server';

  // Server-provided data
  let { data } = $props<{ data: { slide: SlideInfo | null; error: string | null } }>();

  // Connection state
  let connectionState = $state<ConnectionState>('disconnected');
  let tilesReceived = $state(0);
  let cacheSize = $state(0);
  let lastError = $state<string | null>(null);

  // WebSocket endpoint from environment (required)
  const wsUrl = env.PUBLIC_FRUSTA_ENDPOINT!;

  // Image state from server data
  let imageDesc = $state<ImageDesc | null>(null);
  let currentSlot = $state<number | null>(null);
  let loadError = $state<string | null>(null);

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
  let client: ReturnType<typeof createFrustaClient> | null = null;

  // Container ref for sizing
  let container: HTMLDivElement;

  // Debounce timer for viewport updates
  let viewportUpdateTimeout: ReturnType<typeof setTimeout> | null = null;
  const VIEWPORT_UPDATE_DEBOUNCE_MS = 16; // ~60fps

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

  function connect() {
    if (client) {
      client.disconnect();
    }

    lastError = null;

    client = createFrustaClient({
      url: wsUrl,
      reconnectDelay: 1000,
      maxReconnectAttempts: 5,
      onStateChange: (state) => {
        connectionState = state;

        // Auto-open slide when connected
        if (state === 'connected' && imageDesc) {
          openSlide();
        }
      },
      onTile: (tile: TileData) => {
        tilesReceived++;
        handleTileReceived(tile);
      },
      onOpenResponse: (response) => {
        currentSlot = response.slot;
        console.log(`Slide opened: slot=${response.slot}, id=${formatUuid(response.id)}`);
        // Send initial viewport update
        sendViewportUpdate();
      },
      onError: (error) => {
        lastError = error instanceof Error ? error.message : 'Connection error';
        console.error('WebSocket error:', error);
      },
    });

    client.connect();
  }

  function disconnect() {
    client?.disconnect();
    client = null;
    currentSlot = null;
  }

  function openSlide() {
    if (!client || !imageDesc) return;

    const dpi = window.devicePixelRatio * 96;
    client.openSlide(dpi, imageDesc);
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
      maxTiles: 3000,
      onTileCached: () => {
        cacheSize = tileCache.size;
      },
    });
    cache = tileCache;

    // Load image from server-provided data
    if (data.error) {
      loadError = data.error;
    } else if (data.slide) {
      imageDesc = slideInfoToImageDesc(data.slide);
    }

    // Set initial viewport size
    if (container) {
      updateViewportSize();
    }

    // Auto-connect if we have an image
    if (imageDesc) {
      connect();
    }

    // Listen for resize
    window.addEventListener('resize', updateViewportSize);

    // Global mouse up to handle dragging outside container
    window.addEventListener('mouseup', handleMouseUp);
  });

  onDestroy(() => {
    client?.disconnect();
    cache?.clear();
    if (viewportUpdateTimeout) {
      clearTimeout(viewportUpdateTimeout);
    }
    if (browser) {
      window.removeEventListener('resize', updateViewportSize);
      window.removeEventListener('mouseup', handleMouseUp);
    }
  });
</script>

<main>
  <header class="controls">
    <div class="connection-controls">
      {#if connectionState === 'disconnected' || connectionState === 'error'}
        <button onclick={connect}>Connect</button>
      {:else}
        <button onclick={disconnect}>Disconnect</button>
      {/if}

      <span class="status">
        <span
          class="status-indicator"
          class:connected={connectionState === 'connected'}
          class:connecting={connectionState === 'connecting'}
          class:error={connectionState === 'error'}
        ></span>
        {connectionState}
      </span>
    </div>

    <div class="stats">
      <span>Tiles: {tilesReceived}</span>
      <span>Cache: {cacheSize}</span>
      <span>Zoom: {(viewport.zoom * 100).toFixed(1)}%</span>
      {#if imageDesc}
        <span>Image: {imageDesc.width}×{imageDesc.height} ({imageDesc.levels} levels)</span>
      {/if}
    </div>

    {#if loadError}
      <p class="error">{loadError}</p>
    {:else if lastError}
      <p class="error">{lastError}</p>
    {/if}
  </header>

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
      <TileRenderer image={imageDesc} {viewport} {cache} {renderTrigger} />
    {:else}
      <div class="no-image">
        <h2>No Image Loaded</h2>
        <p>Add a slide ID to the URL:</p>
        <code>?id=&lt;uuid&gt;</code>
        <p style="margin-top: 1rem;">Example:</p>
        <code>?id=550e8400-e29b-41d4-a716-446655440000</code>
      </div>
    {/if}
  </div>
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
  }

  .controls {
    display: flex;
    flex-wrap: wrap;
    gap: 1rem;
    padding: 0.75rem 1rem;
    background: #1a1a1a;
    border-bottom: 1px solid #333;
    align-items: center;
    justify-content: space-between;
  }

  .connection-controls {
    display: flex;
    gap: 0.5rem;
    align-items: center;
    flex-wrap: wrap;
  }

  .stats {
    display: flex;
    gap: 1rem;
    font-size: 0.875rem;
    color: #aaa;
  }

  button {
    padding: 0.375rem 0.75rem;
    font-size: 0.875rem;
    cursor: pointer;
    border: none;
    border-radius: 4px;
    background-color: #0066cc;
    color: white;
    transition: background-color 0.15s;
  }

  button:hover {
    background-color: #0055aa;
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

  .status-indicator.connecting {
    background-color: #f59e0b;
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
