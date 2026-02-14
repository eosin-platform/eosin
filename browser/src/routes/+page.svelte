<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import { browser } from '$app/environment';
  import { env } from '$env/dynamic/public';
  import {
    createFrustaClient,
    type ConnectionState,
    type TileData,
    type ProgressEvent,
    type SlideCreatedEvent,
  } from '$lib/frusta';
  import SplitPaneContainer from '$lib/components/SplitPaneContainer.svelte';
  import { liveProgress } from '$lib/stores/progress';
  import { newSlides } from '$lib/stores/newSlides';
  import { tabStore, type Tab } from '$lib/stores/tabs';
  import { performanceMetrics, type PerformanceMetrics } from '$lib/stores/metrics';
  import { settings, FACTORY_IMAGE_DEFAULTS } from '$lib/stores/settings';
  import { getUrlSyncManager } from '$lib/stores/urlSync';
  import { toastStore } from '$lib/stores/toast';
  import type { SlideInfo, ParsedSession } from './+page.server';

  // Server-provided data
  let { data } = $props<{ data: { slide: SlideInfo | null; error: string | null; session: ParsedSession | null } }>();

  // Connection state
  let connectionState = $state<ConnectionState>('disconnected');
  let lastError = $state<string | null>(null);

  // Toast notification state
  let toastMessage = $state<string | null>(null);
  let toastType = $state<'error' | 'success' | 'warning' | 'info'>('error');
  let toastTimeout: ReturnType<typeof setTimeout> | null = null;
  let hasBeenConnected = false;

  // Subscribe to global toast store for toasts from other components
  const unsubToast = toastStore.subscribe((state) => {
    if (state.current) {
      toastMessage = state.current.message;
      toastType = state.current.type;
    } else {
      toastMessage = null;
    }
  });

  // Progress info map for all slides (passed to SplitPaneContainer)
  let progressInfo = $state<Map<string, { steps: number; total: number; trigger: number }>>(new Map());

  // Performance metrics from store
  let metrics = $state<PerformanceMetrics | null>(null);
  const unsubMetrics = performanceMetrics.subscribe((m) => {
    metrics = m;
  });

  // URL sync manager
  const urlSyncManager = getUrlSyncManager();

  /**
   * Format bytes to human readable string (KB, MB, etc.)
   */
  function formatBytes(bytes: number): string {
    if (bytes < 1024) return `${bytes} B`;
    if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
    if (bytes < 1024 * 1024 * 1024) return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
    return `${(bytes / (1024 * 1024 * 1024)).toFixed(2)} GB`;
  }

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
    toastStore.dismiss();
    if (toastTimeout) {
      clearTimeout(toastTimeout);
      toastTimeout = null;
    }
  }

  // WebSocket endpoint from environment (required)
  const wsUrl = env.PUBLIC_FRUSTA_ENDPOINT!;

  // Client instance
  let client = $state<ReturnType<typeof createFrustaClient> | null>(null);

  // Tile router function provided by SplitPaneContainer
  let tileRouter: ((tile: TileData) => void) | null = null;

  function handleRegisterTileRouter(router: (tile: TileData) => void) {
    tileRouter = router;
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
      maxReconnectAttempts: 0,
      connectTimeout: 6000,
      onStateChange: (state) => {
        connectionState = state;

        if (state === 'connected') {
          if (hasBeenConnected) {
            showToast('Reconnected.', 3000, 'success');
          }
          hasBeenConnected = true;
        }
      },
      onTile: (tile: TileData) => {
        // Route tile to the correct ViewerPane via SplitPaneContainer
        if (tileRouter) {
          tileRouter(tile);
        }
      },
      onProgress: (event: ProgressEvent) => {
        const eventSlideId = formatUuid(event.slideId);
        // Update progress info map
        progressInfo = new Map(progressInfo).set(eventSlideId, {
          steps: event.progressSteps,
          total: event.progressTotal,
          trigger: Date.now(),
        });
        // Publish to shared store so sidebar can display live progress
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
        newSlides.push({
          id: event.id,
          dataset: event.dataset,
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

  onMount(() => {
    // Check if we have a session state from URL (?v= parameter)
    if (data.session) {
      // Restore session from URL - this is handled by the tab store restoration
      // The session state contains the full pane/tab layout
      tabStore.restoreFromSession(data.session.state);
      
      // Reset image settings to factory defaults first, then apply URL values.
      // This ensures permalinks replicate the exact view, not influenced by user's local settings.
      settings.setSetting('image', 'brightness', FACTORY_IMAGE_DEFAULTS.brightness);
      settings.setSetting('image', 'contrast', FACTORY_IMAGE_DEFAULTS.contrast);
      settings.setSetting('image', 'gamma', FACTORY_IMAGE_DEFAULTS.gamma);
      settings.setSetting('image', 'sharpeningIntensity', FACTORY_IMAGE_DEFAULTS.sharpeningIntensity);
      settings.setSetting('image', 'sharpeningEnabled', false);
      settings.setSetting('image', 'stainEnhancement', FACTORY_IMAGE_DEFAULTS.stainEnhancement);
      settings.setSetting('image', 'stainNormalization', FACTORY_IMAGE_DEFAULTS.stainNormalization);
      
      // Apply image settings from session (overriding defaults where specified)
      const imgSettings = data.session.imageSettings;
      if (imgSettings.stainEnhancement) {
        settings.setSetting('image', 'stainEnhancement', imgSettings.stainEnhancement);
      }
      if (imgSettings.stainNormalization) {
        settings.setSetting('image', 'stainNormalization', imgSettings.stainNormalization);
      }
      if (imgSettings.sharpeningIntensity !== null) {
        settings.setSetting('image', 'sharpeningIntensity', imgSettings.sharpeningIntensity);
        settings.setSetting('image', 'sharpeningEnabled', imgSettings.sharpeningIntensity > 0);
      }
      if (imgSettings.gamma !== null) {
        settings.setSetting('image', 'gamma', imgSettings.gamma);
      }
      if (imgSettings.brightness !== null) {
        settings.setSetting('image', 'brightness', imgSettings.brightness);
      }
      if (imgSettings.contrast !== null) {
        settings.setSetting('image', 'contrast', imgSettings.contrast);
      }
    } else if (data.error) {
      // Error is shown in toast
    } else if (data.slide) {
      // Load single slide from server-provided data
      tabStore.open(
        data.slide.id,
        data.slide.filename,
        data.slide.width,
        data.slide.height,
        data.slide.viewport,
        data.slide.measurement,
        data.slide.roi,
      );
      
      // Reset image settings to factory defaults first, then apply URL values.
      // This ensures permalinks replicate the exact view, not influenced by user's local settings.
      settings.setSetting('image', 'brightness', FACTORY_IMAGE_DEFAULTS.brightness);
      settings.setSetting('image', 'contrast', FACTORY_IMAGE_DEFAULTS.contrast);
      settings.setSetting('image', 'gamma', FACTORY_IMAGE_DEFAULTS.gamma);
      settings.setSetting('image', 'sharpeningIntensity', FACTORY_IMAGE_DEFAULTS.sharpeningIntensity);
      settings.setSetting('image', 'sharpeningEnabled', false);
      settings.setSetting('image', 'stainEnhancement', FACTORY_IMAGE_DEFAULTS.stainEnhancement);
      settings.setSetting('image', 'stainNormalization', FACTORY_IMAGE_DEFAULTS.stainNormalization);
      
      // Apply image processing settings from URL (overriding defaults where specified)
      if (data.slide.stainEnhancement) {
        settings.setSetting('image', 'stainEnhancement', data.slide.stainEnhancement);
      }
      if (data.slide.stainNormalization) {
        settings.setSetting('image', 'stainNormalization', data.slide.stainNormalization);
      }
      if (data.slide.sharpeningIntensity !== null) {
        settings.setSetting('image', 'sharpeningIntensity', data.slide.sharpeningIntensity);
        settings.setSetting('image', 'sharpeningEnabled', data.slide.sharpeningIntensity > 0);
      }
      if (data.slide.gamma !== null) {
        settings.setSetting('image', 'gamma', data.slide.gamma);
      }
      if (data.slide.brightness !== null) {
        settings.setSetting('image', 'brightness', data.slide.brightness);
      }
      if (data.slide.contrast !== null) {
        settings.setSetting('image', 'contrast', data.slide.contrast);
      }
    }

    connect();
    
    // Start URL sync after initial state is loaded
    // Use a small delay to let the initial viewport settle
    setTimeout(() => {
      urlSyncManager.start();
    }, 500);
  });

  onDestroy(() => {
    urlSyncManager.stop();
    unsubMetrics();
    unsubToast();
    client?.disconnect();
    if (toastTimeout) {
      clearTimeout(toastTimeout);
    }
  });
</script>

<main>
  <SplitPaneContainer
    {client}
    {connectionState}
    {progressInfo}
    onRegisterTileRouter={handleRegisterTileRouter}
  />

  <div class="connection-bar">
    <div class="perf-stats">
      {#if metrics}
        <span class="metric" title="Frame render time">🖼️ {metrics.renderTimeMs.toFixed(1)}ms</span>
        <span class="metric" title="Frames per second">⏱️ {metrics.fps.toFixed(0)} FPS</span>
        <span class="metric" title="Visible tiles">👁️ {metrics.visibleTiles}</span>
        <span class="metric" title="Rendered / Fallback / Placeholder">✅{metrics.renderedTiles} ⬇️{metrics.fallbackTiles} ⬜{metrics.placeholderTiles}</span>
        <span class="metric" title="Cache: tiles / memory">📦 {metrics.cacheSize} / {formatBytes(metrics.cacheMemoryBytes)}</span>
        <span class="metric" title="Tiles received">📥 {metrics.tilesReceived}</span>
        {#if metrics.pendingDecodes > 0}
          <span class="metric pending" title="Tiles being decoded">🔄 {metrics.pendingDecodes}</span>
        {/if}
      {/if}
    </div>
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
    min-height: 0; /* Allow flex children to shrink */
  }

  .connection-bar {
    display: flex;
    padding: 0.25rem 0.75rem;
    background: #111;
    border-top: 1px solid #222;
    align-items: center;
    justify-content: space-between;
    flex-shrink: 0;
    min-height: 2rem;
    z-index: 10;
  }

  .perf-stats {
    display: flex;
    gap: 0.5rem;
    font-family: 'SF Mono', 'Monaco', 'Inconsolata', 'Fira Code', monospace;
    font-size: 0.7rem;
    color: #888;
    flex-wrap: wrap;
    overflow: hidden;
  }

  .perf-stats .metric {
    padding: 0.125rem 0.375rem;
    background: #1a1a1a;
    border-radius: 3px;
    white-space: nowrap;
  }

  /* Hide less important metrics on mobile to prevent overflow */
  @media (max-width: 768px) {
    .connection-bar {
      padding: 0.25rem 0.5rem;
      gap: 0.5rem;
    }
    
    .perf-stats {
      gap: 0.25rem;
      font-size: 0.625rem;
    }

    .perf-stats .metric {
      padding: 0.0625rem 0.25rem;
    }

    /* Hide some metrics on very small screens */
    .perf-stats .metric:nth-child(n+5) {
      display: none;
    }
  }

  @media (max-width: 480px) {
    .perf-stats .metric:nth-child(n+4) {
      display: none;
    }
  }

  .perf-stats .metric.pending {
    color: #f59e0b;
    animation: pulse 1s ease-in-out infinite;
  }

  @keyframes pulse {
    0%, 100% { opacity: 1; }
    50% { opacity: 0.6; }
  }

  .toast {
    position: absolute;
    bottom: 3rem;
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

  .spinner {
    width: 14px;
    height: 14px;
    border: 2px solid #333;
    border-top-color: var(--secondary-hex);
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
    font-size: 0.75rem;
    color: #666;
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
</style>
