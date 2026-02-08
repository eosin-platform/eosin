<script lang="ts">
  import { tabStore, type SplitState } from '$lib/stores/tabs';
  import TabBar from './TabBar.svelte';
  import ViewerPane from './ViewerPane.svelte';
  import { onDestroy } from 'svelte';
  import type { ConnectionState, TileData } from '$lib/frusta';

  interface TileHandler {
    getSlot: () => number | null;
    handleTile: (tile: TileData) => void;
  }

  interface Props {
    /** The shared frusta WebSocket client */
    client: any;
    /** Current connection state */
    connectionState: ConnectionState;
    /** Map of slideId -> progress info */
    progressInfo: Map<string, { steps: number; total: number; trigger: number }>;
    /** Callback to register the tile routing function with the parent */
    onRegisterTileRouter: (router: (tile: TileData) => void) => void;
  }

  let { client, connectionState, progressInfo, onRegisterTileRouter }: Props = $props();

  let splitState = $state<SplitState>({ panes: [], focusedPaneId: '', splitRatio: 0.5 });
  const unsub = tabStore.splitState.subscribe((v) => (splitState = v));
  onDestroy(() => unsub());

  let panes = $derived(splitState.panes.filter(p => p != null));
  let isSplit = $derived(panes.length > 1);
  let splitRatio = $derived(splitState.splitRatio);
  
  // Safe pane accessors - guaranteed non-null or fallback
  let pane0 = $derived(panes[0] ?? { paneId: '', tabs: [], activeTabId: null });
  let pane1 = $derived(panes[1] ?? { paneId: '', tabs: [], activeTabId: null });

  // Map of paneId -> tile handler for routing incoming tiles
  let tileHandlers = new Map<string, TileHandler>();

  function registerTileHandler(paneId: string, handler: TileHandler) {
    tileHandlers.set(paneId, handler);
  }

  function unregisterTileHandler(paneId: string) {
    tileHandlers.delete(paneId);
  }

  /** Route an incoming tile to the correct ViewerPane by matching slot number */
  function routeTile(tile: TileData) {
    for (const handler of tileHandlers.values()) {
      if (handler.getSlot() === tile.slot) {
        handler.handleTile(tile);
        return;
      }
    }
  }

  // Register our tile router with the parent on init
  $effect(() => {
    onRegisterTileRouter(routeTile);
  });

  // --- Divider drag ---
  let isDragging = $state(false);
  let containerEl = $state<HTMLDivElement>();

  function handleDividerMouseDown(e: MouseEvent) {
    e.preventDefault();
    isDragging = true;

    function handleMouseMove(e: MouseEvent) {
      if (!containerEl) return;
      const rect = containerEl.getBoundingClientRect();
      const ratio = (e.clientX - rect.left) / rect.width;
      tabStore.setSplitRatio(ratio);
    }

    function handleMouseUp() {
      isDragging = false;
      window.removeEventListener('mousemove', handleMouseMove);
      window.removeEventListener('mouseup', handleMouseUp);
    }

    window.addEventListener('mousemove', handleMouseMove);
    window.addEventListener('mouseup', handleMouseUp);
  }

  function handlePaneFocus(paneId: string) {
    tabStore.setFocusedPane(paneId);
  }
</script>

<div class="split-container" bind:this={containerEl} class:dragging={isDragging}>
  {#if !isSplit}
    <!-- Single pane mode -->
    <!-- svelte-ignore a11y_click_events_have_key_events -->
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div class="pane" onclick={() => pane0.paneId && handlePaneFocus(pane0.paneId)}>
      <TabBar paneId={pane0.paneId} />
      <div class="pane-content">
        <ViewerPane
          paneId={pane0.paneId}
          {client}
          {connectionState}
          {progressInfo}
          onRegisterTileHandler={registerTileHandler}
          onUnregisterTileHandler={unregisterTileHandler}
        />
      </div>
    </div>
  {:else}
    <!-- Split mode: left pane -->
    <!-- svelte-ignore a11y_click_events_have_key_events -->
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div
      class="pane"
      class:focused={splitState.focusedPaneId === pane0.paneId}
      style="width: {splitRatio * 100}%"
      onclick={() => pane0.paneId && handlePaneFocus(pane0.paneId)}
    >
      <TabBar paneId={pane0.paneId} />
      <div class="pane-content">
        <ViewerPane
          paneId={pane0.paneId}
          {client}
          {connectionState}
          {progressInfo}
          onRegisterTileHandler={registerTileHandler}
          onUnregisterTileHandler={unregisterTileHandler}
        />
      </div>
    </div>

    <!-- Divider -->
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div class="divider" onmousedown={handleDividerMouseDown}>
      <div class="divider-line"></div>
    </div>

    <!-- Split mode: right pane -->
    <!-- svelte-ignore a11y_click_events_have_key_events -->
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div
      class="pane"
      class:focused={splitState.focusedPaneId === pane1.paneId}
      style="width: {(1 - splitRatio) * 100}%"
      onclick={() => pane1.paneId && handlePaneFocus(pane1.paneId)}
    >
      <TabBar paneId={pane1.paneId} />
      <div class="pane-content">
        <ViewerPane
          paneId={pane1.paneId}
          {client}
          {connectionState}
          {progressInfo}
          onRegisterTileHandler={registerTileHandler}
          onUnregisterTileHandler={unregisterTileHandler}
        />
      </div>
    </div>
  {/if}
</div>

<style>
  .split-container {
    display: flex;
    flex: 1;
    overflow: hidden;
    position: relative;
  }

  .split-container.dragging {
    cursor: col-resize;
    user-select: none;
  }

  .pane {
    display: flex;
    flex-direction: column;
    overflow: hidden;
    min-width: 0;
    flex: 1;
    position: relative;
  }

  /* When split, panes have explicit widths */
  .split-container:has(.divider) .pane {
    flex: none;
  }

  .pane-content {
    flex: 1;
    overflow: hidden;
    position: relative;
    display: flex;
    flex-direction: column;
  }

  /* Focus indicator for split view - uses inset box-shadow for visible border without layout shift */
  .split-container:has(.divider) .pane.focused {
    box-shadow: inset 0 0 0 2px #0088ff;
  }

  .divider {
    width: 5px;
    background: #1a1a1a;
    cursor: col-resize;
    display: flex;
    align-items: center;
    justify-content: center;
    flex-shrink: 0;
    z-index: 10;
    transition: background-color 0.15s;
  }

  .divider:hover {
    background: #0066cc;
  }

  .divider-line {
    width: 1px;
    height: 100%;
    background: #333;
  }

  .divider:hover .divider-line {
    background: #0066cc;
  }
</style>
