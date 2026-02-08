<script lang="ts">
  import { browser } from '$app/environment';
  import { onMount, onDestroy } from 'svelte';
  import { liveProgress, type SlideProgress } from '$lib/stores/progress';
  import { newSlides, type NewSlide } from '$lib/stores/newSlides';
  import { tabStore } from '$lib/stores/tabs';
  import { sidebarLayoutStore } from '$lib/stores/annotations';
  import { navigateToPoint } from '$lib/stores/navigation';
  import { authStore } from '$lib/stores/auth';
  import ActivityIndicator from './ActivityIndicator.svelte';
  import ContextMenu from './ContextMenu.svelte';
  import AnnotationsPanel from './AnnotationsPanel.svelte';

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
  const LONG_PRESS_MOVE_THRESHOLD = 90; // Pixels of movement allowed before canceling long press (3x normal for touch)
  let longPressStartX = 0;
  let longPressStartY = 0;

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
    const touch = e.touches[0];
    longPressStartX = touch.clientX;
    longPressStartY = touch.clientY;
    longPressTimer = setTimeout(() => {
      longPressTimer = null;
      showContextMenu(touch.clientX, touch.clientY, slide);
    }, LONG_PRESS_MS);
  }

  function handleTouchEndSlide() {
    if (longPressTimer) {
      clearTimeout(longPressTimer);
      longPressTimer = null;
    }
  }

  function handleTouchMoveSlide(e: TouchEvent) {
    // Cancel long press if finger moves beyond threshold
    if (longPressTimer && e.touches.length > 0) {
      const dx = Math.abs(e.touches[0].clientX - longPressStartX);
      const dy = Math.abs(e.touches[0].clientY - longPressStartY);
      if (dx > LONG_PRESS_MOVE_THRESHOLD || dy > LONG_PRESS_MOVE_THRESHOLD) {
        clearTimeout(longPressTimer);
        longPressTimer = null;
      }
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

  // --- Sidebar layout (slides/annotations split) ---
  let slidesSectionCollapsed = $state(false);
  let annotationsSectionCollapsed = $state(false);
  let annotationsSectionHeight = $state(300);
  
  const unsubSidebarLayout = sidebarLayoutStore.subscribe((state) => {
    slidesSectionCollapsed = state.slidesSectionCollapsed;
    annotationsSectionCollapsed = state.annotationsSectionCollapsed;
    annotationsSectionHeight = state.annotationsSectionHeight;
  });

  onDestroy(() => {
    unsubSidebarLayout();
  });

  // --- Auth state ---
  let isLoggedIn = $state(false);
  const unsubAuth = authStore.subscribe((state) => {
    isLoggedIn = state.user !== null;
  });
  
  onDestroy(() => {
    unsubAuth();
  });

  // --- New layer dialog state ---
  let showNewLayerDialog = $state(false);

  function handleNewLayerClick(e: MouseEvent) {
    e.stopPropagation();
    showNewLayerDialog = true;
  }

  function handleNewLayerDialogClose() {
    showNewLayerDialog = false;
  }

  function toggleSlidesSection() {
    sidebarLayoutStore.toggleSlidesSection();
  }

  function toggleAnnotationsSection() {
    sidebarLayoutStore.toggleAnnotationsSection();
  }

  // Splitter drag logic
  let isDraggingSplitter = $state(false);
  let splitterStartY = 0;
  let splitterStartHeight = 0;
  let sidebarElement: HTMLElement;

  function handleSplitterMouseDown(e: MouseEvent) {
    e.preventDefault();
    isDraggingSplitter = true;
    splitterStartY = e.clientY;
    splitterStartHeight = annotationsSectionHeight;
    
    document.addEventListener('mousemove', handleSplitterMouseMove);
    document.addEventListener('mouseup', handleSplitterMouseUp);
  }

  function handleSplitterMouseMove(e: MouseEvent) {
    if (!isDraggingSplitter) return;
    const delta = splitterStartY - e.clientY;
    const newHeight = Math.max(100, Math.min(splitterStartHeight + delta, sidebarElement?.clientHeight - 200 || 500));
    sidebarLayoutStore.setAnnotationsSectionHeight(newHeight);
  }

  function handleSplitterMouseUp() {
    isDraggingSplitter = false;
    document.removeEventListener('mousemove', handleSplitterMouseMove);
    document.removeEventListener('mouseup', handleSplitterMouseUp);
  }

  // Touch support for splitter
  function handleSplitterTouchStart(e: TouchEvent) {
    e.preventDefault();
    isDraggingSplitter = true;
    splitterStartY = e.touches[0].clientY;
    splitterStartHeight = annotationsSectionHeight;
    
    document.addEventListener('touchmove', handleSplitterTouchMove, { passive: false });
    document.addEventListener('touchend', handleSplitterTouchEnd);
  }

  function handleSplitterTouchMove(e: TouchEvent) {
    if (!isDraggingSplitter) return;
    e.preventDefault();
    const delta = splitterStartY - e.touches[0].clientY;
    const newHeight = Math.max(100, Math.min(splitterStartHeight + delta, sidebarElement?.clientHeight - 200 || 500));
    sidebarLayoutStore.setAnnotationsSectionHeight(newHeight);
  }

  function handleSplitterTouchEnd() {
    isDraggingSplitter = false;
    document.removeEventListener('touchmove', handleSplitterTouchMove);
    document.removeEventListener('touchend', handleSplitterTouchEnd);
  }

  // Handle annotation navigation - center viewport on annotation
  function handleAnnotationClick(annotationId: string, x: number, y: number) {
    if (activeSlideId) {
      navigateToPoint(activeSlideId, x, y, annotationId);
    }
  }

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
  bind:this={sidebarElement}
>
  <!-- Top header with logo and collapse button -->
  <div class="sidebar-header" class:collapsed onclick={collapsed ? handleToggle : undefined} role={collapsed ? 'button' : undefined} tabindex={collapsed ? 0 : -1} aria-label={collapsed ? 'Expand sidebar' : undefined}>
    <div class="logo-container">
      <img src="/logo_half.png" alt="App logo" class="app-logo" />
    </div>
    {#if !collapsed}
      <span class="app-title">Eosin</span>
      <button class="toggle-btn" onclick={(e) => { e.stopPropagation(); handleToggle(); }} aria-label="Collapse sidebar">
        <svg xmlns="http://www.w3.org/2000/svg" width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
          <rect x="3" y="3" width="18" height="18" rx="2"></rect>
          <path d="M9 3v18"></path>
          <path d="M17 9l-3 3 3 3"></path>
        </svg>
      </button>
    {/if}
  </div>

  {#if !collapsed}
    <!-- Slides section (expandable/collapsible) -->
    <div class="sidebar-section slides-section" class:collapsed-section={slidesSectionCollapsed} style={!slidesSectionCollapsed && !annotationsSectionCollapsed ? `flex: 1 1 auto; min-height: 150px;` : !slidesSectionCollapsed ? 'flex: 1 1 auto;' : ''}>
      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <div 
        class="section-header" 
        onclick={toggleSlidesSection}
        onkeydown={(e) => e.key === 'Enter' && toggleSlidesSection()}
        role="button"
        tabindex="0"
      >
        <svg class="chevron" class:rotated={!slidesSectionCollapsed} xmlns="http://www.w3.org/2000/svg" width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
          <polyline points="9 18 15 12 9 6"></polyline>
        </svg>
        <span class="section-title">Slides</span>
        <span class="section-count">({totalCount})</span>
      </div>
      
      {#if !slidesSectionCollapsed}
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
      {/if}
    </div>

    <!-- Splitter between sections (only when annotations not collapsed) -->
    {#if !annotationsSectionCollapsed}
      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <div 
        class="section-splitter"
        onmousedown={handleSplitterMouseDown}
        ontouchstart={handleSplitterTouchStart}
      >
        <div class="splitter-handle"></div>
      </div>
    {/if}

    <!-- Annotations section (expandable/collapsible) -->
    <div 
      class="sidebar-section annotations-section" 
      class:collapsed-section={annotationsSectionCollapsed}
      style={!annotationsSectionCollapsed ? `height: ${annotationsSectionHeight}px; flex-shrink: 0;` : ''}
    >
      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <div 
        class="section-header"
        onclick={toggleAnnotationsSection}
        onkeydown={(e) => e.key === 'Enter' && toggleAnnotationsSection()}
        role="button"
        tabindex="0"
      >
        <svg class="chevron" class:rotated={!annotationsSectionCollapsed} xmlns="http://www.w3.org/2000/svg" width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
          <polyline points="9 18 15 12 9 6"></polyline>
        </svg>
        <span class="section-title">Layers</span>
        
        <!-- New layer button (aligned right) -->
        {#if isLoggedIn}
          <button 
            class="section-action-btn" 
            onclick={handleNewLayerClick}
            title="New Layer"
          >
            <svg xmlns="http://www.w3.org/2000/svg" width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
              <line x1="12" y1="5" x2="12" y2="19"></line>
              <line x1="5" y1="12" x2="19" y2="12"></line>
            </svg>
          </button>
        {:else}
          <button class="section-action-btn disabled" disabled title="Log in to create layers">
            <svg xmlns="http://www.w3.org/2000/svg" width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
              <line x1="12" y1="5" x2="12" y2="19"></line>
              <line x1="5" y1="12" x2="19" y2="12"></line>
            </svg>
          </button>
        {/if}
      </div>
      
      {#if !annotationsSectionCollapsed}
        <div class="section-content">
          <AnnotationsPanel 
            slideId={activeSlideId}
            onAnnotationClick={handleAnnotationClick}
            showNewLayerDialogProp={showNewLayerDialog}
            onNewLayerDialogClose={handleNewLayerDialogClose}
          />
        </div>
      {/if}
    </div>
  {:else}
    <!-- Collapsed sidebar: show icons only -->
    <div class="collapsed-sections">
      <button 
        class="collapsed-section-btn" 
        title="Slides ({totalCount})"
        onclick={handleToggle}
      >
        <svg xmlns="http://www.w3.org/2000/svg" width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
          <rect x="2" y="3" width="20" height="14" rx="2" ry="2"></rect>
          <line x1="8" y1="21" x2="16" y2="21"></line>
          <line x1="12" y1="17" x2="12" y2="21"></line>
        </svg>
      </button>
      <button 
        class="collapsed-section-btn" 
        title="Annotations"
        onclick={handleToggle}
      >
        <svg xmlns="http://www.w3.org/2000/svg" width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
          <polygon points="12 2 22 8.5 22 15.5 12 22 2 15.5 2 8.5 12 2"></polygon>
          <line x1="12" y1="22" x2="12" y2="15.5"></line>
          <polyline points="22 8.5 12 15.5 2 8.5"></polyline>
        </svg>
      </button>
    </div>
  {/if}
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

  .logo-container {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 32px;
    height: 32px;
    flex-shrink: 0;
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

  .sidebar-header.collapsed {
    cursor: pointer;
  }

  .sidebar-header.collapsed:hover {
    background: #1a1a1a;
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
    padding: 0.5rem 0.625rem;
    border-radius: 6px;
    color: #ccc;
    transition: background-color 0.15s, color 0.15s;
    gap: 0.125rem;
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
      padding: 0.75rem;
      gap: 0;
    }

    .slide-name {
      font-size: 0.9375rem;
    }

    .slide-meta {
      flex-wrap: nowrap;
      min-height: 1.25rem;
    }

    .slide-dimensions,
    .slide-size {
      white-space: nowrap;
      flex-shrink: 1;
      min-width: 0;
      overflow: hidden;
      text-overflow: ellipsis;
    }

    .slide-dimensions,
    .slide-size,
    .slide-progress {
      font-size: 0.8125rem;
      line-height: 1.25;
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

    /* Safe area for mobile browser toolbars */
    .sidebar {
      padding-bottom: env(safe-area-inset-bottom, 0px);
    }
  }

  /* Section styles for VS Code-like split view */
  .app-title {
    font-size: 0.9rem;
    font-weight: 600;
    color: #eee;
  }

  .sidebar-section {
    display: flex;
    flex-direction: column;
    min-height: 0;
    overflow: hidden;
  }

  .sidebar-section.collapsed-section {
    flex: 0 0 auto;
  }

  .slides-section {
    flex: 1 1 auto;
  }

  .annotations-section {
    border-top: 1px solid #333;
  }

  .section-header {
    display: flex;
    align-items: center;
    padding: 0.5rem 0.75rem;
    background: #1a1a1a;
    cursor: pointer;
    user-select: none;
    gap: 0.375rem;
    flex-shrink: 0;
    transition: background-color 0.1s;
  }

  .section-header:hover {
    background: #222;
  }

  .section-title {
    font-size: 0.6875rem;
    font-weight: 600;
    color: #888;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    flex: 1;
  }

  .section-action-btn {
    margin-left: auto;
    background: transparent;
    border: none;
    padding: 2px 4px;
    color: #888;
    cursor: pointer;
    border-radius: 3px;
    display: flex;
    align-items: center;
    justify-content: center;
    transition: background-color 0.1s, color 0.1s;
  }

  .section-action-btn:hover:not(.disabled) {
    background: #333;
    color: #ccc;
  }

  .section-action-btn.disabled {
    color: #555;
    cursor: not-allowed;
  }

  .section-count {
    font-size: 0.6875rem;
    color: #666;
  }

  .chevron {
    color: #666;
    transition: transform 0.15s ease;
    flex-shrink: 0;
  }

  .chevron.rotated {
    transform: rotate(90deg);
  }

  .section-content {
    flex: 1;
    min-height: 0;
    overflow: hidden;
  }

  /* Splitter between sections */
  .section-splitter {
    flex-shrink: 0;
    height: 6px;
    background: #1a1a1a;
    cursor: ns-resize;
    display: flex;
    align-items: center;
    justify-content: center;
    transition: background-color 0.1s;
  }

  .section-splitter:hover {
    background: #252525;
  }

  .splitter-handle {
    width: 32px;
    height: 2px;
    background: #444;
    border-radius: 1px;
  }

  .section-splitter:hover .splitter-handle {
    background: #666;
  }

  /* Collapsed sidebar section buttons */
  .collapsed-sections {
    display: flex;
    flex-direction: column;
    align-items: center;
    padding: 0.5rem 0;
    gap: 0.25rem;
  }

  .collapsed-section-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 40px;
    height: 40px;
    padding: 0;
    background: transparent;
    border: none;
    color: #888;
    cursor: pointer;
    border-radius: 6px;
    transition: background-color 0.15s, color 0.15s;
  }

  .collapsed-section-btn:hover {
    background: #333;
    color: #fff;
  }
</style>
