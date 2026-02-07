<script lang="ts">
  import { browser } from '$app/environment';
  import { onMount, onDestroy } from 'svelte';
  import { liveProgress, type SlideProgress } from '$lib/stores/progress';
  import { newSlides, type NewSlide } from '$lib/stores/newSlides';
  import { tabStore } from '$lib/stores/tabs';
  import ActivityIndicator from './ActivityIndicator.svelte';
  import ContextMenu from './ContextMenu.svelte';

  interface SlideListItem {
    id: string;
    width: number;
    height: number;
    /** Original filename extracted from the S3 key */
    filename: string;
    /** Full size of the original slide file in bytes */
    full_size: number;
    /** Current processing progress in steps */
    progress_steps: number;
    /** Total tiles to process */
    progress_total: number;
  }

  interface Props {
    initialSlides: SlideListItem[];
    totalCount: number;
    hasMore: boolean;
    pageSize: number;
    collapsed?: boolean;
    onToggle?: () => void;
  }

  let { initialSlides, totalCount, hasMore, pageSize, collapsed = false, onToggle }: Props = $props();

  let slides = $state<SlideListItem[]>([]);
  let loading = $state(false);
  let canLoadMore = $state(false);
  let currentOffset = $state(0);
  let error = $state<string | null>(null);

  let scrollContainer: HTMLElement;
  let sentinel: HTMLDivElement;

  /** Slide IDs that just arrived via WebSocket and should play the entrance animation */
  let animatingSlideIds = $state<Set<string>>(new Set());

  // Live progress from WebSocket (shared store)
  let progressMap = $state<Map<string, SlideProgress>>(new Map());
  const unsubscribe = liveProgress.subscribe((value) => {
    progressMap = value;
    // Update matching slides in the list so percentages stay in sync
    for (const [slideId, progress] of value) {
      const existing = slides.find((s) => s.id === slideId);
      // Only reassign (creating a new array) when values actually changed,
      // otherwise the new reference triggers an infinite reactive loop.
      if (
        existing &&
        (existing.progress_steps !== progress.progressSteps ||
          existing.progress_total !== progress.progressTotal)
      ) {
        slides = slides.map((s) =>
          s.id === slideId
            ? {
                ...s,
                progress_steps: progress.progressSteps,
                progress_total: progress.progressTotal,
              }
            : s
        );
      }
    }
  });

  onDestroy(() => {
    unsubscribe();
    unsubNewSlides();
  });

  // Subscribe to new slides arriving via WebSocket
  const unsubNewSlides = newSlides.subscribe((incoming) => {
    for (const ns of incoming) {
      // Only add if not already in the list (deduplicate by id)
      if (!slides.some((s) => s.id === ns.id)) {
        animatingSlideIds = new Set([...animatingSlideIds, ns.id]);
        slides = [
          {
            id: ns.id,
            width: ns.width,
            height: ns.height,
            filename: ns.filename,
            full_size: ns.full_size,
            progress_steps: 0,
            progress_total: 0,
          },
          ...slides,
        ];
        currentOffset += 1;
        totalCount += 1;
      }
    }
  });

  // Initialize and reset state when initialSlides changes
  $effect(() => {
    slides = [...initialSlides];
    currentOffset = initialSlides.length;
    canLoadMore = hasMore;
  });

  async function loadMore() {
    if (loading || !canLoadMore) return;

    loading = true;
    error = null;

    try {
      const response = await fetch(`/api/slides?offset=${currentOffset}&limit=${pageSize}`);
      
      if (!response.ok) {
        throw new Error('Failed to load slides');
      }

      const data = await response.json();
      
      slides = [...slides, ...data.items];
      currentOffset += data.items.length;
      canLoadMore = currentOffset < data.full_count;
    } catch (err) {
      error = err instanceof Error ? err.message : 'Failed to load slides';
      console.error('Error loading slides:', err);
    } finally {
      loading = false;
    }
  }

  onMount(() => {
    if (!browser || !sentinel) return;

    const observer = new IntersectionObserver(
      (entries) => {
        if (entries[0].isIntersecting && canLoadMore && !loading) {
          loadMore();
        }
      },
      {
        root: scrollContainer,
        rootMargin: '100px',
        threshold: 0,
      }
    );

    observer.observe(sentinel);

    return () => {
      observer.disconnect();
    };
  });

  function formatDimensions(width: number, height: number): string {
    if (width >= 1000000 || height >= 1000000) {
      return `${(width / 1000000).toFixed(1)}M × ${(height / 1000000).toFixed(1)}M`;
    } else if (width >= 1000 || height >= 1000) {
      return `${(width / 1000).toFixed(1)}K × ${(height / 1000).toFixed(1)}K`;
    }
    return `${width} × ${height}`;
  }

  function formatSize(bytes: number): string {
    if (bytes >= 1073741824) {
      return `${(bytes / 1073741824).toFixed(1)} GB`;
    } else if (bytes >= 1048576) {
      return `${(bytes / 1048576).toFixed(1)} MB`;
    } else if (bytes >= 1024) {
      return `${(bytes / 1024).toFixed(1)} KB`;
    }
    return `${bytes} B`;
  }

  function getSlideLabel(slide: SlideListItem): string {
    // Use filename if available, otherwise fall back to shortened ID
    return slide.filename || slide.id.slice(0, 8);
  }

  function formatProgress(slide: SlideListItem): string | null {
    if (slide.progress_total === 0) {
      return null; // Not yet started
    }
    if (slide.progress_steps >= slide.progress_total) {
      return null; // Complete - don't show percentage
    }
    const pct = (slide.progress_steps / slide.progress_total) * 100;
    return `${pct.toPrecision(3)}%`;
  }

  function handleAnimationEnd(slideId: string) {
    animatingSlideIds = new Set([...animatingSlideIds].filter((id) => id !== slideId));
  }

  function handleToggle() {
    if (onToggle) {
      onToggle();
    }
  }

  // Context menu state
  let contextMenuVisible = $state(false);
  let contextMenuX = $state(0);
  let contextMenuY = $state(0);
  let contextMenuSlide = $state<SlideListItem | null>(null);

  // Long press state for mobile
  let longPressTimer: ReturnType<typeof setTimeout> | null = null;
  const LONG_PRESS_MS = 500;

  function showContextMenu(x: number, y: number, slide: SlideListItem) {
    contextMenuX = x;
    contextMenuY = y;
    contextMenuSlide = slide;
    contextMenuVisible = true;
  }

  function handleContextMenu(e: MouseEvent, slide: SlideListItem) {
    e.preventDefault();
    e.stopPropagation();
    showContextMenu(e.clientX, e.clientY, slide);
  }

  function handleSlideClick(slide: SlideListItem) {
    // Default click opens in current tab
    contextMenuVisible = false;
    tabStore.open(slide.id, getSlideLabel(slide), slide.width, slide.height);
  }

  function handleTouchStartSlide(e: TouchEvent, slide: SlideListItem) {
    longPressTimer = setTimeout(() => {
      longPressTimer = null;
      const touch = e.touches[0];
      showContextMenu(touch.clientX, touch.clientY, slide);
    }, LONG_PRESS_MS);
  }

  function handleTouchEndSlide() {
    if (longPressTimer) {
      clearTimeout(longPressTimer);
      longPressTimer = null;
    }
  }

  function handleTouchMoveSlide() {
    // Cancel long press if finger moves
    if (longPressTimer) {
      clearTimeout(longPressTimer);
      longPressTimer = null;
    }
  }

  function handleContextMenuOpen() {
    if (!contextMenuSlide) return;
    tabStore.open(contextMenuSlide.id, getSlideLabel(contextMenuSlide), contextMenuSlide.width, contextMenuSlide.height);
  }

  function handleContextMenuOpenInNewTab() {
    if (!contextMenuSlide) return;
    tabStore.openInNewTab(contextMenuSlide.id, getSlideLabel(contextMenuSlide), contextMenuSlide.width, contextMenuSlide.height);
  }

  function handleContextMenuClose() {
    contextMenuVisible = false;
    contextMenuSlide = null;
  }

  // Track the active tab's slideId for highlighting
  let activeSlideId = $state<string | null>(null);
  const unsubActiveTab = tabStore.activeTab.subscribe((tab) => {
    activeSlideId = tab?.slideId ?? null;
  });

  onDestroy(() => {
    unsubActiveTab();
  });

  // --- Pull-to-refresh ---
  let pullStartY = $state(0);
  let pullDistance = $state(0);
  let isPulling = $state(false);
  let refreshing = $state(false);
  const PULL_THRESHOLD = 64;

  async function refreshSlides() {
    refreshing = true;
    error = null;
    try {
      const response = await fetch(`/api/slides?offset=0&limit=${pageSize}`);
      if (!response.ok) throw new Error('Failed to refresh slides');
      const data = await response.json();
      slides = data.items;
      currentOffset = data.items.length;
      canLoadMore = currentOffset < data.full_count;
      totalCount = data.full_count;
    } catch (err) {
      error = err instanceof Error ? err.message : 'Failed to refresh slides';
      console.error('Error refreshing slides:', err);
    } finally {
      refreshing = false;
      pullDistance = 0;
      isPulling = false;
    }
  }

  function handlePullTouchStart(e: TouchEvent) {
    // Only start pull tracking if scrolled to the top
    if (scrollContainer && scrollContainer.scrollTop <= 0) {
      pullStartY = e.touches[0].clientY;
      isPulling = true;
    }
  }

  function handlePullTouchMove(e: TouchEvent) {
    if (!isPulling || refreshing) return;
    const currentY = e.touches[0].clientY;
    const diff = currentY - pullStartY;
    if (diff > 0) {
      // Apply dampening so the pull feels elastic
      pullDistance = Math.min(diff * 0.5, PULL_THRESHOLD * 2);
      // Prevent default scroll when pulling down at the top
      if (scrollContainer && scrollContainer.scrollTop <= 0) {
        e.preventDefault();
      }
    } else {
      pullDistance = 0;
    }
  }

  function handlePullTouchEnd() {
    if (!isPulling || refreshing) return;
    if (pullDistance >= PULL_THRESHOLD) {
      refreshSlides();
    } else {
      pullDistance = 0;
      isPulling = false;
    }
  }
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<aside
  class="sidebar"
  class:collapsed
>
  <div class="sidebar-header">
    <button class="logo-btn" onclick={collapsed ? handleToggle : undefined} aria-label={collapsed ? 'Expand sidebar' : undefined} role={collapsed ? 'button' : 'presentation'} tabindex={collapsed ? 0 : -1}>
      <img src="/logo_half.png" alt="App logo" class="app-logo" />
    </button>
    {#if !collapsed}
      <h2>Slides</h2>
      <span class="slide-count">({totalCount})</span>
      <button class="toggle-btn" onclick={handleToggle} aria-label="Collapse sidebar">
        <svg xmlns="http://www.w3.org/2000/svg" width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
          <!-- Panel left close icon (collapse sidebar) -->
          <rect x="3" y="3" width="18" height="18" rx="2"></rect>
          <path d="M9 3v18"></path>
          <path d="M17 9l-3 3 3 3"></path>
        </svg>
      </button>
    {/if}
  </div>

  <!-- Pull-to-refresh indicator -->
  <div
    class="pull-indicator"
    class:visible={pullDistance > 0 || refreshing}
    style="height: {refreshing ? PULL_THRESHOLD : pullDistance}px"
  >
    <div class="pull-indicator-content">
      {#if refreshing}
        <div class="spinner"></div>
        <span>Refreshing…</span>
      {:else if pullDistance >= PULL_THRESHOLD}
        <svg class="pull-arrow released" xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
          <polyline points="17 11 12 6 7 11"></polyline>
          <line x1="12" y1="18" x2="12" y2="6"></line>
        </svg>
        <span>Release to refresh</span>
      {:else}
        <svg class="pull-arrow" style="transform: rotate({Math.min(pullDistance / PULL_THRESHOLD, 1) * 180}deg)" xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
          <polyline points="7 13 12 18 17 13"></polyline>
          <line x1="12" y1="6" x2="12" y2="18"></line>
        </svg>
        <span>Pull to refresh</span>
      {/if}
    </div>
  </div>

  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <nav
    class="slide-list"
    bind:this={scrollContainer}
    ontouchstart={handlePullTouchStart}
    ontouchmove={handlePullTouchMove}
    ontouchend={handlePullTouchEnd}
  >
    {#each slides as slide (slide.id)}
      {@const progress = formatProgress(slide)}
      {@const slideProgress = progressMap.get(slide.id)}
      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <!-- svelte-ignore a11y_click_events_have_key_events -->
      <div
        class="slide-item"
        class:active={activeSlideId === slide.id}
        class:slide-new={animatingSlideIds.has(slide.id)}
        title={collapsed ? `${getSlideLabel(slide)} - ${formatDimensions(slide.width, slide.height)} - ${formatSize(slide.full_size)}${progress ? ` - ${progress}` : ''}` : undefined}
        onanimationend={() => handleAnimationEnd(slide.id)}
        onclick={() => handleSlideClick(slide)}
        onkeydown={(e) => { if (e.key === 'Enter' || e.key === ' ') handleSlideClick(slide); }}
        oncontextmenu={(e) => handleContextMenu(e, slide)}
        ontouchstart={(e) => handleTouchStartSlide(e, slide)}
        ontouchend={handleTouchEndSlide}
        ontouchmove={handleTouchMoveSlide}
        role="button"
        tabindex="0"
      >
        {#if collapsed}
          <span class="slide-icon">{(slide.filename || slide.id).slice(0, 2).toUpperCase()}</span>
        {:else}
          <div class="slide-row">
            <span class="slide-name">{getSlideLabel(slide)}</span>
            {#if progress}
              <span class="slide-progress">
                {#if slideProgress}
                  <ActivityIndicator trigger={slideProgress.lastUpdate} />
                {/if}
                {progress}
              </span>
            {/if}
          </div>
          <span class="slide-meta">
            <span class="slide-dimensions">{formatDimensions(slide.width, slide.height)}</span>
            <span class="slide-size">{formatSize(slide.full_size)}</span>
          </span>
        {/if}
      </div>
    {/each}

    {#if slides.length === 0 && !loading}
      <div class="empty-state">
        <p>No slides available</p>
      </div>
    {/if}

    <!-- Sentinel for infinite scroll -->
    <div bind:this={sentinel} class="sentinel">
      {#if loading}
        <div class="loading-indicator">
          <div class="spinner"></div>
          <span>Loading...</span>
        </div>
      {:else if error}
        <div class="error-state">
          <p>{error}</p>
          <button onclick={loadMore}>Retry</button>
        </div>
      {:else if canLoadMore}
        <div class="load-more-hint">Scroll for more</div>
      {/if}
    </div>
  </nav>
</aside>

<ContextMenu
  x={contextMenuX}
  y={contextMenuY}
  visible={contextMenuVisible}
  onOpen={handleContextMenuOpen}
  onOpenInNewTab={handleContextMenuOpenInNewTab}
  onClose={handleContextMenuClose}
/>

<style>
  .sidebar {
    width: 280px;
    min-width: 280px;
    height: 100vh;
    background: #141414;
    border-right: 1px solid #333;
    display: flex;
    flex-direction: column;
    overflow: hidden;
    transition: width 0.2s ease, min-width 0.2s ease;
  }

  .sidebar.collapsed {
    width: 56px;
    min-width: 56px;
  }

  .logo-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 32px;
    height: 32px;
    padding: 0;
    background: transparent;
    border: none;
    cursor: default;
    border-radius: 6px;
    flex-shrink: 0;
    transition: background-color 0.15s;
  }

  .sidebar.collapsed .logo-btn {
    cursor: pointer;
    width: 32px;
    height: 32px;
    border-radius: 6px;
  }

  .sidebar.collapsed .logo-btn:hover {
    background: #333;
  }

  .toggle-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 32px;
    height: 32px;
    padding: 0;
    background: transparent;
    border: none;
    color: #aaa;
    cursor: pointer;
    border-radius: 6px;
    transition: background-color 0.15s, color 0.15s;
    flex-shrink: 0;
    margin-left: auto;
  }

  .toggle-btn:hover {
    background: #333;
    color: #fff;
  }

  .sidebar-header {
    display: flex;
    align-items: center;
    padding: 0.75rem;
    border-bottom: 1px solid #333;
    position: sticky;
    top: 0;
    background: #141414;
    z-index: 10;
    gap: 0.5rem;
    min-height: 48px;
    box-sizing: border-box;
  }

  .app-logo {
    width: 24px;
    height: 24px;
    object-fit: contain;
    flex-shrink: 0;
  }

  .sidebar-header h2 {
    margin: 0;
    font-size: 1rem;
    font-weight: 600;
    color: #eee;
  }

  .slide-count {
    color: #777;
    font-size: 0.75rem;
    font-weight: 400;
  }

  .slide-list {
    flex: 1;
    display: flex;
    flex-direction: column;
    padding: 0.5rem;
    gap: 2px;
    overflow-y: auto;
    overflow-x: hidden;
    scrollbar-width: thin;
    scrollbar-color: #333 transparent;
  }

  .slide-list::-webkit-scrollbar {
    width: 9px;
  }

  .slide-list::-webkit-scrollbar-track {
    background: transparent;
  }

  .slide-list::-webkit-scrollbar-thumb {
    background: #333;
    border-radius: 3px;
  }

  .slide-list::-webkit-scrollbar-thumb:hover {
    background: #555;
  }

  .sidebar.collapsed .slide-list {
    padding: 0.25rem;
  }

  .slide-item {
    display: flex;
    flex-direction: column;
    padding: 0.75rem;
    border-radius: 6px;
    color: #ccc;
    transition: background-color 0.15s, color 0.15s;
    gap: 0.25rem;
    cursor: pointer;
    user-select: none;
    -webkit-user-select: none;
  }

  .sidebar.collapsed .slide-item {
    padding: 0.5rem;
    align-items: center;
    justify-content: center;
  }

  .slide-icon {
    font-size: 0.75rem;
    font-weight: 600;
    width: 28px;
    height: 28px;
    display: flex;
    align-items: center;
    justify-content: center;
    background: #222;
    border-radius: 4px;
  }

  .sidebar.collapsed .slide-item.active .slide-icon {
    background: rgba(255, 255, 255, 0.2);
  }

  /* New-slide entrance animation */
  .slide-item.slide-new {
    animation: slideEntrance 0.5s cubic-bezier(0.22, 1, 0.36, 1) forwards;
  }

  @keyframes slideEntrance {
    0% {
      opacity: 0;
      transform: translateY(-12px) scale(0.97);
      box-shadow: 0 0 0 0 rgba(0, 102, 204, 0);
    }
    40% {
      opacity: 1;
      transform: translateY(0) scale(1);
    }
    50% {
      box-shadow: 0 0 12px 2px rgba(0, 102, 204, 0.4);
    }
    100% {
      opacity: 1;
      transform: translateY(0) scale(1);
      box-shadow: 0 0 0 0 rgba(0, 102, 204, 0);
    }
  }

  .slide-item:hover {
    background: #222;
    color: #fff;
  }

  .slide-item.active {
    background: #0066cc;
    color: #fff;
  }

  .slide-item.active .slide-dimensions {
    color: rgba(255, 255, 255, 0.8);
  }

  .slide-name {
    font-size: 0.875rem;
    font-weight: 500;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .slide-row {
    display: flex;
    justify-content: space-between;
    align-items: center;
    width: 100%;
    gap: 0.5rem;
  }

  .slide-progress {
    font-size: 0.75rem;
    color: #f59e0b;
    font-weight: 500;
    flex-shrink: 0;
    display: inline-flex;
    align-items: center;
    gap: 0.25rem;
  }

  .slide-item.active .slide-progress {
    color: rgba(255, 255, 255, 0.9);
  }

  .slide-dimensions {
    font-size: 0.75rem;
    color: #888;
  }

  .slide-meta {
    display: flex;
    justify-content: space-between;
    align-items: center;
    width: 100%;
  }

  .slide-size {
    font-size: 0.75rem;
    color: #888;
    text-align: right;
  }

  .slide-item.active .slide-size {
    color: rgba(255, 255, 255, 0.8);
  }

  .empty-state {
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 2rem;
    color: #666;
    font-size: 0.875rem;
  }

  .sentinel {
    min-height: 1px;
    padding: 0.5rem;
  }

  .loading-indicator {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 0.5rem;
    padding: 1rem;
    color: #888;
    font-size: 0.875rem;
  }

  .spinner {
    width: 16px;
    height: 16px;
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

  .error-state {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 0.5rem;
    padding: 1rem;
    color: #ef4444;
    font-size: 0.875rem;
  }

  .error-state button {
    padding: 0.25rem 0.75rem;
    font-size: 0.75rem;
    cursor: pointer;
    border: none;
    border-radius: 4px;
    background-color: #333;
    color: #ccc;
    transition: background-color 0.15s;
  }

  .error-state button:hover {
    background-color: #444;
  }

  .load-more-hint {
    text-align: center;
    padding: 0.5rem;
    color: #555;
    font-size: 0.75rem;
  }

  /* Pull-to-refresh */
  .pull-indicator {
    overflow: hidden;
    height: 0;
    display: flex;
    align-items: flex-end;
    justify-content: center;
    transition: height 0.2s ease;
    flex-shrink: 0;
  }

  .pull-indicator.visible {
    /* height is set inline via style binding */
  }

  .pull-indicator-content {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 0.5rem;
    padding: 0.5rem;
    color: #888;
    font-size: 0.75rem;
    white-space: nowrap;
  }

  .pull-arrow {
    transition: transform 0.15s ease;
    flex-shrink: 0;
    color: #888;
  }

  .pull-arrow.released {
    color: #0066cc;
  }

  .pull-indicator .spinner {
    width: 16px;
    height: 16px;
    border: 2px solid #333;
    border-top-color: #0066cc;
    border-radius: 50%;
    animation: spin 0.8s linear infinite;
  }

  /* Touch device adaptations - larger touch targets */
  @media (pointer: coarse) {
    .toggle-btn {
      width: 44px;
      height: 44px;
    }

    .toggle-btn svg {
      width: 20px;
      height: 20px;
    }

    /* Larger slide items for easier tapping */
    .slide-item {
      padding: 1rem;
      gap: 0.375rem;
      min-height: 60px;
    }

    .slide-name {
      font-size: 1rem;
    }

    .slide-dimensions,
    .slide-size,
    .slide-progress {
      font-size: 0.875rem;
    }

    .slide-icon {
      width: 36px;
      height: 36px;
      font-size: 0.875rem;
    }

    /* Larger error button */
    .error-state button {
      padding: 0.625rem 1rem;
      font-size: 0.875rem;
      min-height: 44px;
    }
  }
</style>
