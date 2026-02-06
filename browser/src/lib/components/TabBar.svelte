<script lang="ts">
  import { tabStore, type Tab, type SplitState } from '$lib/stores/tabs';
  import { liveProgress, type SlideProgress } from '$lib/stores/progress';
  import { settings } from '$lib/stores/settings';
  import ActivityIndicator from './ActivityIndicator.svelte';
  import TabContextMenu from './TabContextMenu.svelte';

  interface Props {
    paneId: string;
  }

  let { paneId }: Props = $props();

  let splitState = $state<SplitState>({ panes: [], focusedPaneId: '', splitRatio: 0.5 });
  let progressMap = $state<Map<string, SlideProgress>>(new Map());

  const unsubSplit = tabStore.splitState.subscribe((v) => (splitState = v));
  const unsubProgress = liveProgress.subscribe((v) => (progressMap = v));

  import { onDestroy } from 'svelte';
  onDestroy(() => {
    unsubSplit();
    unsubProgress();
  });

  let pane = $derived(splitState.panes.find((p) => p.paneId === paneId));
  let tabs = $derived(pane?.tabs ?? []);
  let activeTabId = $derived(pane?.activeTabId ?? null);
  let isFocused = $derived(splitState.focusedPaneId === paneId);

  function handleTabClick(tabId: string) {
    tabStore.setActiveInPane(paneId, tabId);
  }

  function handleCloseTab(e: MouseEvent, tabId: string) {
    e.stopPropagation();
    tabStore.closeTab(tabId);
  }

  function handleCloseKeydown(e: KeyboardEvent, tabId: string) {
    if (e.key === 'Enter' || e.key === ' ') {
      e.stopPropagation();
      tabStore.closeTab(tabId);
    }
  }

  function handlePaneFocus() {
    tabStore.setFocusedPane(paneId);
  }

  // --- Drag-and-drop reordering ---
  let dragTabId = $state<string | null>(null);
  let dropTargetIndex = $state<number | null>(null);

  function handleDragStart(e: DragEvent, tabId: string) {
    dragTabId = tabId;
    if (e.dataTransfer) {
      e.dataTransfer.effectAllowed = 'move';
      e.dataTransfer.setData('text/plain', tabId);
      e.dataTransfer.setData('application/x-pane-id', paneId);
    }
  }

  function handleDragOver(e: DragEvent, index: number) {
    e.preventDefault();
    if (e.dataTransfer) {
      e.dataTransfer.dropEffect = 'move';
    }
    dropTargetIndex = index;
  }

  function handleDrop(e: DragEvent, index: number) {
    e.preventDefault();
    const droppedTabId = e.dataTransfer?.getData('text/plain');
    const sourcePaneId = e.dataTransfer?.getData('application/x-pane-id');

    if (!droppedTabId) return;

    if (sourcePaneId && sourcePaneId !== paneId) {
      // Cross-pane drag
      tabStore.moveTabToPane(droppedTabId, paneId, index);
    } else {
      // Same pane reorder
      const fromIndex = tabs.findIndex((t) => t.tabId === droppedTabId);
      if (fromIndex !== -1 && fromIndex !== index) {
        tabStore.reorder(paneId, fromIndex, index);
      }
    }
    dragTabId = null;
    dropTargetIndex = null;
  }

  function handleDragEnd() {
    dragTabId = null;
    dropTargetIndex = null;
  }

  // Allow dropping on the empty area of the tab bar
  function handleBarDragOver(e: DragEvent) {
    e.preventDefault();
    if (e.dataTransfer) {
      e.dataTransfer.dropEffect = 'move';
    }
  }

  function handleBarDrop(e: DragEvent) {
    e.preventDefault();
    const droppedTabId = e.dataTransfer?.getData('text/plain');
    const sourcePaneId = e.dataTransfer?.getData('application/x-pane-id');

    if (!droppedTabId) return;

    if (sourcePaneId && sourcePaneId !== paneId) {
      tabStore.moveTabToPane(droppedTabId, paneId);
    }
    dragTabId = null;
    dropTargetIndex = null;
  }

  // --- Right-click context menu ---
  let contextMenuVisible = $state(false);
  let contextMenuX = $state(0);
  let contextMenuY = $state(0);
  let contextMenuTabId = $state<string | null>(null);

  // --- Long press for mobile context menu ---
  let longPressTimer: ReturnType<typeof setTimeout> | null = null;
  let longPressTriggered = $state(false);
  let longPressTabId = $state<string | null>(null);
  let longPressX = $state(0);
  let longPressY = $state(0);
  let longPressMoved = $state(false);
  let isTouchDevice = $state(false);
  const LONG_PRESS_MS = 500;
  const LONG_PRESS_MOVE_THRESHOLD = 30; // Higher threshold to account for drag behavior

  function handleTouchStart(e: TouchEvent, tabId: string) {
    isTouchDevice = true; // Detected touch - disable draggable
    const touch = e.touches[0];
    longPressX = touch.clientX;
    longPressY = touch.clientY;
    longPressTabId = tabId;
    longPressTriggered = false;
    longPressMoved = false;
    
    // Prevent default to stop browser from initiating drag on touch
    e.preventDefault();

    longPressTimer = setTimeout(() => {
      // Only trigger if user hasn't moved too much
      if (!longPressMoved) {
        longPressTriggered = true;
        // Optional: provide haptic feedback if available
        if (navigator.vibrate) {
          navigator.vibrate(50);
        }
      }
      longPressTimer = null;
    }, LONG_PRESS_MS);
  }

  function handleTouchMove(e: TouchEvent) {
    const touch = e.touches[0];
    const dx = Math.abs(touch.clientX - longPressX);
    const dy = Math.abs(touch.clientY - longPressY);
    
    // Track if user has moved beyond threshold
    if (dx > LONG_PRESS_MOVE_THRESHOLD || dy > LONG_PRESS_MOVE_THRESHOLD) {
      longPressMoved = true;
      // Cancel pending timer if still waiting
      if (longPressTimer) {
        clearTimeout(longPressTimer);
        longPressTimer = null;
      }
      // Also cancel if already triggered (user started moving after long press)
      longPressTriggered = false;
    }
  }

  function handleTouchEnd(e: TouchEvent) {
    const wasLongPressTriggered = longPressTriggered;
    const touchTabId = longPressTabId;
    
    if (longPressTimer) {
      clearTimeout(longPressTimer);
      longPressTimer = null;
    }

    // If long press was triggered, show context menu on release
    if (wasLongPressTriggered && touchTabId) {
      e.preventDefault();
      contextMenuX = longPressX;
      contextMenuY = longPressY;
      contextMenuTabId = touchTabId;
      contextMenuVisible = true;
    } else if (!longPressMoved && touchTabId) {
      // Short tap without movement - activate the tab
      handleTabClick(touchTabId);
    }

    longPressTriggered = false;
    longPressTabId = null;
    longPressMoved = false;
  }

  function handleContextMenu(e: MouseEvent, tabId: string) {
    e.preventDefault();
    e.stopPropagation();
    contextMenuX = e.clientX;
    contextMenuY = e.clientY;
    contextMenuTabId = tabId;
    contextMenuVisible = true;
  }

  function closeContextMenu() {
    contextMenuVisible = false;
    contextMenuTabId = null;
  }

  let contextMenuTabIndex = $derived(
    contextMenuTabId ? tabs.findIndex((t) => t.tabId === contextMenuTabId) : -1
  );
  let disableCloseOthers = $derived(tabs.length <= 1);
  let disableCloseRight = $derived(
    contextMenuTabIndex === -1 || contextMenuTabIndex >= tabs.length - 1
  );
  // Split Right is always available — if this is the only tab, it will be duplicated
  let disableSplitRight = false;
</script>

<!-- svelte-ignore a11y_interactive_supports_focus -->
<!-- svelte-ignore a11y_click_events_have_key_events -->
<div
  class="tab-bar"
  class:focused={isFocused}
  role="tablist"
  onclick={handlePaneFocus}
  ondragover={handleBarDragOver}
  ondrop={handleBarDrop}
>
  {#if tabs.length === 0}
    <div class="tab-bar-empty">No slides open</div>
  {:else}
    {#each tabs as tab, i (tab.tabId)}
      {@const tabProgress = progressMap.get(tab.slideId)}
      <button
        class="tab"
        class:active={tab.tabId === activeTabId}
        class:dragging={dragTabId === tab.tabId}
        class:drop-before={dropTargetIndex === i && dragTabId !== null && tabs.findIndex((t) => t.tabId === dragTabId) !== i}
        role="tab"
        aria-selected={tab.tabId === activeTabId}
        draggable={!isTouchDevice}
        ondragstart={(e) => handleDragStart(e, tab.tabId)}
        ondragover={(e) => handleDragOver(e, i)}
        ondrop={(e) => handleDrop(e, i)}
        ondragend={handleDragEnd}
        onclick={() => !isTouchDevice && handleTabClick(tab.tabId)}
        oncontextmenu={(e) => handleContextMenu(e, tab.tabId)}
        ontouchstart={(e) => handleTouchStart(e, tab.tabId)}
        ontouchmove={handleTouchMove}
        ontouchend={handleTouchEnd}
      >
        {#if tabProgress && tabProgress.progressSteps < tabProgress.progressTotal}
          <ActivityIndicator trigger={tabProgress.lastUpdate} />
        {/if}
        <span class="tab-label" title={tab.label}>{tab.label}</span>
        <span
          class="tab-close"
          role="button"
          tabindex="0"
          aria-label="Close tab"
          onclick={(e) => handleCloseTab(e, tab.tabId)}
          onkeydown={(e) => handleCloseKeydown(e, tab.tabId)}
        >×</span>
      </button>
    {/each}
  {/if}
</div>

<TabContextMenu
  x={contextMenuX}
  y={contextMenuY}
  visible={contextMenuVisible}
  disableCloseOthers={disableCloseOthers}
  disableCloseRight={disableCloseRight}
  disableSplitRight={disableSplitRight}
  onSplitRight={() => { if (contextMenuTabId) tabStore.splitRight(contextMenuTabId); }}
  onCopyPermalink={() => {
    if (!contextMenuTabId) return;
    const tab = tabs.find((t) => t.tabId === contextMenuTabId);
    if (!tab) return;
    const params = new URLSearchParams();
    params.set('slide', tab.slideId);
    if (tab.savedViewport) {
      params.set('x', tab.savedViewport.x.toFixed(2));
      params.set('y', tab.savedViewport.y.toFixed(2));
      params.set('zoom', tab.savedViewport.zoom.toFixed(6));
    }
    // Include image processing settings if not default
    const currentSettings = settings.get();
    if (currentSettings.image.stainEnhancement !== 'none') {
      params.set('enhance', currentSettings.image.stainEnhancement);
    }
    if (currentSettings.image.stainNormalization !== 'none') {
      params.set('normalize', currentSettings.image.stainNormalization);
    }
    if (currentSettings.image.sharpeningIntensity > 0) {
      params.set('sharpen', currentSettings.image.sharpeningIntensity.toString());
    }
    if (currentSettings.image.gamma !== 1.0) {
      params.set('gamma', currentSettings.image.gamma.toFixed(2));
    }
    if (currentSettings.image.brightness !== 0) {
      params.set('brightness', currentSettings.image.brightness.toString());
    }
    if (currentSettings.image.contrast !== 0) {
      params.set('contrast', currentSettings.image.contrast.toString());
    }
    const url = `${window.location.origin}?${params.toString()}`;
    navigator.clipboard.writeText(url);
  }}
  onCloseTab={() => { if (contextMenuTabId) tabStore.closeTab(contextMenuTabId); }}
  onCloseOthers={() => { if (contextMenuTabId) tabStore.closeOtherTabs(contextMenuTabId); }}
  onCloseRight={() => { if (contextMenuTabId) tabStore.closeTabsToRight(contextMenuTabId); }}
  onCloseAll={() => tabStore.closeAllTabsInPane(paneId)}
  onClose={closeContextMenu}
/>

<style>
  .tab-bar {
    display: flex;
    align-items: stretch;
    background: #111;
    border-bottom: 1px solid #333;
    overflow-x: auto;
    overflow-y: hidden;
    flex-shrink: 0;
    min-height: 36px;
    scrollbar-width: thin;
    scrollbar-color: #444 transparent;
  }

  .tab-bar.focused {
    border-bottom-color: #0066cc44;
  }

  .tab-bar::-webkit-scrollbar {
    height: 3px;
  }

  .tab-bar::-webkit-scrollbar-thumb {
    background: #444;
    border-radius: 2px;
  }

  .tab-bar-empty {
    display: flex;
    align-items: center;
    padding: 0 1rem;
    color: #555;
    font-size: 0.8125rem;
    white-space: nowrap;
  }

  .tab {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    padding: 0 0.375rem 0 0.875rem;
    background: transparent;
    border: none;
    border-right: 1px solid #222;
    color: #888;
    font-size: 0.8125rem;
    cursor: pointer;
    white-space: nowrap;
    max-width: 200px;
    min-width: 0;
    transition: background-color 0.1s, color 0.1s;
    position: relative;
  }

  .tab:hover {
    background: #1a1a1a;
    color: #ccc;
  }

  .tab.active {
    background: #1a1a1a;
    color: #eee;
    border-bottom: 2px solid #0066cc;
  }

  .tab-label {
    overflow: hidden;
    text-overflow: ellipsis;
    flex: 1;
    min-width: 0;
    text-align: left;
  }

  .tab-close {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 20px;
    height: 20px;
    border-radius: 4px;
    font-size: 1rem;
    line-height: 1;
    color: #666;
    flex-shrink: 0;
    transition: background-color 0.1s, color 0.1s;
  }

  .tab-close:hover {
    background: #333;
    color: #fff;
  }

  .tab.active .tab-close {
    color: #999;
  }

  .tab.active .tab-close:hover {
    color: #fff;
    background: #444;
  }

  /* Drag-and-drop styles */
  .tab.dragging {
    opacity: 0.4;
  }

  .tab.drop-before {
    box-shadow: inset 2px 0 0 0 #0066cc;
  }
</style>
