<script lang="ts">
  import { page } from '$app/stores';
  import { browser } from '$app/environment';
  import { onMount } from 'svelte';

  interface SlideListItem {
    id: string;
    width: number;
    height: number;
    /** Full size of the original slide file in bytes */
    full_size: number;
    /** Current processing progress in steps of 10,000 tiles */
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

  // Get current slide ID from URL
  let currentSlideId = $derived($page.url.searchParams.get('id'));

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
    // Use shortened ID as label
    return slide.id.slice(0, 8);
  }

  function formatProgress(slide: SlideListItem): string | null {
    if (slide.progress_total === 0) {
      return null; // Not yet started
    }
    if (slide.progress_steps >= slide.progress_total) {
      return null; // Complete - don't show percentage
    }
    const pct = (slide.progress_steps / slide.progress_total) * 100;
    return `${pct.toFixed(0)}%`;
  }

  function handleToggle() {
    if (onToggle) {
      onToggle();
    }
  }
</script>

<aside class="sidebar" class:collapsed bind:this={scrollContainer}>
  <div class="sidebar-header">
    <button class="toggle-btn" onclick={handleToggle} aria-label={collapsed ? 'Expand sidebar' : 'Collapse sidebar'}>
      <svg xmlns="http://www.w3.org/2000/svg" width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
        {#if collapsed}
          <line x1="3" y1="12" x2="21" y2="12"></line>
          <line x1="3" y1="6" x2="21" y2="6"></line>
          <line x1="3" y1="18" x2="21" y2="18"></line>
        {:else}
          <polyline points="15 18 9 12 15 6"></polyline>
        {/if}
      </svg>
    </button>
    {#if !collapsed}
      <h2>Slides</h2>
      <span class="slide-count">{totalCount}</span>
    {/if}
  </div>

  <nav class="slide-list">
    {#each slides as slide (slide.id)}
      {@const progress = formatProgress(slide)}
      <a 
        href="/?id={slide.id}" 
        class="slide-item"
        class:active={currentSlideId === slide.id}
        title={collapsed ? `${getSlideLabel(slide)} - ${formatDimensions(slide.width, slide.height)} - ${formatSize(slide.full_size)}${progress ? ` - ${progress}` : ''}` : undefined}
      >
        {#if collapsed}
          <span class="slide-icon">{slide.id.slice(0, 2).toUpperCase()}</span>
        {:else}
          <div class="slide-row">
            <span class="slide-name">{getSlideLabel(slide)}</span>
            {#if progress}
              <span class="slide-progress">{progress}</span>
            {/if}
          </div>
          <span class="slide-meta">
            <span class="slide-dimensions">{formatDimensions(slide.width, slide.height)}</span>
            <span class="slide-size">{formatSize(slide.full_size)}</span>
          </span>
        {/if}
      </a>
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

<style>
  .sidebar {
    width: 280px;
    min-width: 280px;
    height: 100vh;
    background: #141414;
    border-right: 1px solid #333;
    display: flex;
    flex-direction: column;
    overflow-y: auto;
    overflow-x: hidden;
    transition: width 0.2s ease, min-width 0.2s ease;
  }

  .sidebar.collapsed {
    width: 56px;
    min-width: 56px;
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
  }

  .toggle-btn:hover {
    background: #333;
    color: #fff;
  }

  .sidebar-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 0.75rem;
    border-bottom: 1px solid #333;
    position: sticky;
    top: 0;
    background: #141414;
    z-index: 10;
    gap: 0.5rem;
  }

  .sidebar.collapsed .sidebar-header {
    justify-content: center;
    padding: 0.75rem 0.5rem;
  }

  .sidebar-header h2 {
    margin: 0;
    font-size: 1rem;
    font-weight: 600;
    color: #eee;
  }

  .slide-count {
    background: #333;
    color: #aaa;
    padding: 0.125rem 0.5rem;
    border-radius: 9999px;
    font-size: 0.75rem;
  }

  .slide-list {
    flex: 1;
    display: flex;
    flex-direction: column;
    padding: 0.5rem;
    gap: 2px;
  }

  .sidebar.collapsed .slide-list {
    padding: 0.25rem;
  }

  .slide-item {
    display: flex;
    flex-direction: column;
    padding: 0.75rem;
    border-radius: 6px;
    text-decoration: none;
    color: #ccc;
    transition: background-color 0.15s, color 0.15s;
    gap: 0.25rem;
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
</style>
