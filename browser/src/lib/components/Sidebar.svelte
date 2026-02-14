<script lang="ts">
  import { browser } from '$app/environment';
  import { onMount, onDestroy } from 'svelte';
  import { liveProgress, type SlideProgress } from '$lib/stores/progress';
  import { newSlides, type NewSlide } from '$lib/stores/newSlides';
  import { tabStore } from '$lib/stores/tabs';
  import { sidebarLayoutStore } from '$lib/stores/annotations';
  import { navigateToPoint } from '$lib/stores/navigation';
  import { authStore } from '$lib/stores/auth';
  import { toastStore } from '$lib/stores/toast';
  import ActivityIndicator from './ActivityIndicator.svelte';
  import ContextMenu from './ContextMenu.svelte';
  import AnnotationsPanel from './AnnotationsPanel.svelte';

  const DATASET_STORAGE_KEY = 'eosin.sidebar.dataset_id';

  interface SlideListItem {
    id: string;
    dataset: string;
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
    /** Optional slide metadata payload */
    metadata?: unknown | null;
  }

  interface DatasetListItem {
    id: string;
    name: string;
    description: string | null;
    credit: string | null;
    created_at: number;
    updated_at: number;
    metadata: unknown | null;
    slide_count: number;
    full_size: number;
  }

  interface Props {
    initialSlides: SlideListItem[];
    initialDatasets: DatasetListItem[];
    initialSelectedDatasetId: string | null;
    totalCount: number;
    hasMore: boolean;
    pageSize: number;
    collapsed?: boolean;
    onToggle?: () => void;
  }

  let {
    initialSlides,
    initialDatasets,
    initialSelectedDatasetId,
    totalCount,
    hasMore,
    pageSize,
    collapsed = false,
    onToggle,
  }: Props = $props();

  let slides = $state<SlideListItem[]>(initialSlides);
  let datasets = $state<DatasetListItem[]>(initialDatasets);
  let selectedDatasetId = $state<string>(initialSelectedDatasetId ?? '');
  let loadedDatasetId = $state<string>(initialSelectedDatasetId ?? '');
  let loading = $state(false);
  let canLoadMore = $state(hasMore);
  let currentOffset = $state(initialSlides.length);
  let error = $state<string | null>(null);
  let datasetModalOpen = $state(false);
  let datasetSearch = $state('');
  let datasetLongPressTriggered = $state(false);

  const selectedDataset = $derived(
    datasets.find((dataset) => dataset.id === selectedDatasetId) ?? null
  );
  const filteredDatasets = $derived.by(() => {
    const query = datasetSearch.trim().toLowerCase();
    if (!query) {
      return datasets;
    }

    return datasets.filter((dataset) => {
      const name = dataset.name.toLowerCase();
      const description = (dataset.description ?? '').toLowerCase();
      const credit = (dataset.credit ?? '').toLowerCase();
      return name.includes(query) || description.includes(query) || credit.includes(query);
    });
  });

  let scrollContainer: HTMLElement;
  let sentinel: HTMLDivElement;

  /** Slide IDs that just arrived via WebSocket and should play the entrance animation */
  let animatingSlideIds = $state<Set<string>>(new Set());

  function buildSlidesUrl(offset: number, limit: number, datasetId: string): string {
    const params = new URLSearchParams({
      offset: offset.toString(),
      limit: limit.toString(),
      dataset_id: datasetId,
    });

    return `/api/slides?${params.toString()}`;
  }

  function isKnownDataset(datasetId: string, availableDatasets: DatasetListItem[]): boolean {
    return availableDatasets.some((dataset) => dataset.id === datasetId);
  }

  function getUrlDatasetId(): string | null {
    if (!browser) return null;
    const value = new URLSearchParams(window.location.search).get('dataset_id');
    return value && value.length > 0 ? value : null;
  }

  function getStoredDatasetId(): string | null {
    if (!browser) return null;
    const value = window.localStorage.getItem(DATASET_STORAGE_KEY);
    return value && value.length > 0 ? value : null;
  }

  function persistDatasetSelection(datasetId: string) {
    if (!browser || !datasetId) return;

    const currentStored = window.localStorage.getItem(DATASET_STORAGE_KEY);
    if (currentStored !== datasetId) {
      window.localStorage.setItem(DATASET_STORAGE_KEY, datasetId);
    }

    const currentUrl = new URL(window.location.href);
    const currentDatasetParam = currentUrl.searchParams.get('dataset_id');
    if (currentDatasetParam !== datasetId) {
      currentUrl.searchParams.set('dataset_id', datasetId);
      const nextUrl = `${currentUrl.pathname}${currentUrl.search}${currentUrl.hash}`;
      window.history.replaceState(window.history.state, '', nextUrl);
    }
  }

  function computePreferredDatasetId(availableDatasets: DatasetListItem[]): string {
    if (availableDatasets.length === 0) {
      return '';
    }

    const urlDatasetId = getUrlDatasetId();
    if (urlDatasetId && isKnownDataset(urlDatasetId, availableDatasets)) {
      return urlDatasetId;
    }

    const storedDatasetId = getStoredDatasetId();
    if (storedDatasetId && isKnownDataset(storedDatasetId, availableDatasets)) {
      return storedDatasetId;
    }

    if (initialSelectedDatasetId && isKnownDataset(initialSelectedDatasetId, availableDatasets)) {
      return initialSelectedDatasetId;
    }

    return availableDatasets[0].id;
  }

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
      const matchesDataset = !!selectedDatasetId && ns.dataset === selectedDatasetId;

      if (!matchesDataset) {
        continue;
      }

      // Only add if not already in the list (deduplicate by id)
      if (!slides.some((s) => s.id === ns.id)) {
        animatingSlideIds = new Set([...animatingSlideIds, ns.id]);
        slides = [
          {
            id: ns.id,
            dataset: ns.dataset ?? '',
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

  // On client mount, check URL/localStorage for preferred dataset and reload if needed
  onMount(() => {
    // Compute preferred dataset: URL > localStorage > SSR > first
    const preferredDatasetId = computePreferredDatasetId(datasets);

    // If preferred differs from SSR-loaded, update and reload
    if (preferredDatasetId && preferredDatasetId !== selectedDatasetId) {
      selectedDatasetId = preferredDatasetId;
      persistDatasetSelection(preferredDatasetId);
      void reloadSlidesForSelectedDataset();
    } else if (preferredDatasetId) {
      // Just persist the current selection to URL/localStorage
      persistDatasetSelection(preferredDatasetId);
    }
  });

  async function reloadSlidesForSelectedDataset() {
    if (!selectedDatasetId) {
      slides = [];
      currentOffset = 0;
      canLoadMore = false;
      totalCount = 0;
      return;
    }

    loading = true;
    error = null;

    try {
      const response = await fetch(buildSlidesUrl(0, pageSize, selectedDatasetId));
      if (!response.ok) {
        throw new Error('Failed to load slides');
      }

      const data = await response.json();
      slides = data.items;
      currentOffset = data.items.length;
      canLoadMore = currentOffset < data.full_count;
      totalCount = data.full_count;
      loadedDatasetId = selectedDatasetId;
    } catch (err) {
      error = err instanceof Error ? err.message : 'Failed to load slides';
      console.error('Error loading slides:', err);
    } finally {
      loading = false;
    }
  }

  function openDatasetModal(e?: Event) {
    if (datasetLongPressTriggered) {
      datasetLongPressTriggered = false;
      return;
    }
    e?.stopPropagation();
    datasetSearch = '';
    datasetModalOpen = true;
  }

  function closeDatasetModal() {
    datasetModalOpen = false;
  }

  function handleGlobalModalKeydown(event: KeyboardEvent) {
    if (event.key !== 'Escape') {
      return;
    }

    if (datasetPropertiesModalOpen) {
      closeDatasetPropertiesModal();
      return;
    }

    if (propertiesModalOpen) {
      closePropertiesModal();
      return;
    }

    if (datasetModalOpen) {
      closeDatasetModal();
    }
  }

  function handleDatasetPick(datasetId: string) {
    if (datasetId === selectedDatasetId) {
      closeDatasetModal();
      return;
    }

    selectedDatasetId = datasetId;
    persistDatasetSelection(selectedDatasetId);
    closeDatasetModal();
    void reloadSlidesForSelectedDataset();
  }

  async function loadMore() {
    if (loading || !canLoadMore || !selectedDatasetId) return;

    loading = true;
    error = null;

    try {
      const response = await fetch(buildSlidesUrl(currentOffset, pageSize, selectedDatasetId));
      
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

  function formatTimestamp(timestampMs: number): string {
    if (!Number.isFinite(timestampMs) || timestampMs <= 0) {
      return '—';
    }
    return new Date(timestampMs).toLocaleString();
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

  // Dataset selector context menu state
  let datasetContextMenuVisible = $state(false);
  let datasetContextMenuX = $state(0);
  let datasetContextMenuY = $state(0);
  let datasetContextMenuDataset = $state<DatasetListItem | null>(null);

  // Slide properties modal state
  let propertiesModalOpen = $state(false);
  let propertiesSlide = $state<SlideListItem | null>(null);

  // Dataset properties modal state
  let datasetPropertiesModalOpen = $state(false);
  let datasetPropertiesDataset = $state<DatasetListItem | null>(null);

  // Long press state for mobile
  let longPressTimer: ReturnType<typeof setTimeout> | null = null;
  const LONG_PRESS_MS = 300;
  const LONG_PRESS_MOVE_THRESHOLD = 20; // Pixels of movement allowed before canceling long press
  let longPressStartX = 0;
  let longPressStartY = 0;
  let datasetLongPressTimer: ReturnType<typeof setTimeout> | null = null;
  let datasetLongPressStartX = 0;
  let datasetLongPressStartY = 0;

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

  function showDatasetContextMenu(x: number, y: number, dataset: DatasetListItem) {
    datasetContextMenuX = x;
    datasetContextMenuY = y;
    datasetContextMenuDataset = dataset;
    datasetContextMenuVisible = true;
  }

  function handleDatasetContextMenu(e: MouseEvent) {
    if (!selectedDataset) return;
    e.preventDefault();
    e.stopPropagation();
    showDatasetContextMenu(e.clientX, e.clientY, selectedDataset);
  }

  function handleDatasetTouchStart(e: TouchEvent) {
    if (!selectedDataset || e.touches.length === 0) return;
    datasetLongPressTriggered = false;
    const touch = e.touches[0];
    datasetLongPressStartX = touch.clientX;
    datasetLongPressStartY = touch.clientY;
    datasetLongPressTimer = setTimeout(() => {
      datasetLongPressTimer = null;
      datasetLongPressTriggered = true;
      showDatasetContextMenu(touch.clientX, touch.clientY, selectedDataset);
    }, LONG_PRESS_MS);
  }

  function handleDatasetTouchEnd() {
    if (datasetLongPressTimer) {
      clearTimeout(datasetLongPressTimer);
      datasetLongPressTimer = null;
    }
  }

  function handleDatasetTouchMove(e: TouchEvent) {
    if (datasetLongPressTimer && e.touches.length > 0) {
      const dx = Math.abs(e.touches[0].clientX - datasetLongPressStartX);
      const dy = Math.abs(e.touches[0].clientY - datasetLongPressStartY);
      if (dx > LONG_PRESS_MOVE_THRESHOLD || dy > LONG_PRESS_MOVE_THRESHOLD) {
        clearTimeout(datasetLongPressTimer);
        datasetLongPressTimer = null;
      }
    }
  }

  function handleDatasetContextMenuClose() {
    datasetContextMenuVisible = false;
    datasetContextMenuDataset = null;
  }

  function handleDatasetContextMenuProperties() {
    if (!datasetContextMenuDataset) return;
    datasetPropertiesDataset = datasetContextMenuDataset;
    datasetPropertiesModalOpen = true;
    handleDatasetContextMenuClose();
  }

  function closeDatasetPropertiesModal() {
    datasetPropertiesModalOpen = false;
    datasetPropertiesDataset = null;
  }

  function handleContextMenuProperties() {
    if (!contextMenuSlide) return;
    propertiesSlide = contextMenuSlide;
    propertiesModalOpen = true;
  }

  function closePropertiesModal() {
    propertiesModalOpen = false;
    propertiesSlide = null;
  }

  function formatPropertyLabel(key: string): string {
    return key
      .replace(/_/g, ' ')
      .replace(/\b\w/g, (char) => char.toUpperCase());
  }

  function formatPropertyValue(value: unknown): string {
    if (value === null || value === undefined) return '—';
    if (typeof value === 'boolean') return value ? 'true' : 'false';
    if (typeof value === 'number') return Number.isFinite(value) ? String(value) : '—';
    if (typeof value === 'string') return value.length > 0 ? value : '—';
    return JSON.stringify(value);
  }

  function asRecord(value: unknown): Record<string, unknown> {
    return value as Record<string, unknown>;
  }

  const propertiesRows = $derived.by(() => {
    if (!propertiesSlide) {
      return [] as { key: string; label: string; value: string }[];
    }

    const entries = Object.entries(asRecord(propertiesSlide)).filter(
      ([key]) => key !== 'metadata'
    );

    return entries.map(([key, value]) => ({
      key,
      label: formatPropertyLabel(key),
      value: formatPropertyValue(value),
    }));
  });

  const propertiesMetadataJson = $derived.by(() => {
    if (!propertiesSlide) {
      return 'null';
    }

    const metadata = asRecord(propertiesSlide).metadata ?? null;
    return JSON.stringify(metadata, null, 2);
  });

  const propertiesDataset = $derived.by(() => {
    const slide = propertiesSlide;
    if (!slide) {
      return null as DatasetListItem | null;
    }

    return datasets.find((dataset) => dataset.id === slide.dataset) ?? null;
  });

  const datasetPropertiesRows = $derived.by(() => {
    if (!datasetPropertiesDataset) {
      return [] as { key: string; label: string; value: string }[];
    }

    const entries = Object.entries(asRecord(datasetPropertiesDataset)).filter(
      ([key]) => key !== 'metadata'
    );

    return entries.map(([key, value]) => ({
      key,
      label: formatPropertyLabel(key),
      value: formatPropertyValue(value),
    }));
  });

  const datasetPropertiesMetadataJson = $derived.by(() => {
    if (!datasetPropertiesDataset) {
      return 'null';
    }

    const metadata = asRecord(datasetPropertiesDataset).metadata ?? null;
    return JSON.stringify(metadata, null, 2);
  });

  async function copyTextToClipboard(text: string, label: string) {
    if (!browser) return;

    try {
      if (navigator.clipboard && typeof navigator.clipboard.writeText === 'function') {
        await navigator.clipboard.writeText(text);
      } else {
        const textArea = document.createElement('textarea');
        textArea.value = text;
        textArea.style.position = 'fixed';
        textArea.style.opacity = '0';
        document.body.appendChild(textArea);
        textArea.focus();
        textArea.select();
        document.execCommand('copy');
        document.body.removeChild(textArea);
      }

      toastStore.success(`${label} copied`);
    } catch {
      toastStore.error(`Failed to copy ${label.toLowerCase()}`);
    }
  }

  async function copyAllProperties() {
    if (!propertiesSlide) return;
    await copyTextToClipboard(JSON.stringify(propertiesSlide, null, 2), 'All properties');
  }

  async function copyMetadataJson() {
    await copyTextToClipboard(propertiesMetadataJson, 'Metadata JSON');
  }

  async function copyAllDatasetProperties() {
    if (!datasetPropertiesDataset) return;
    await copyTextToClipboard(JSON.stringify(datasetPropertiesDataset, null, 2), 'All properties');
  }

  async function copyDatasetMetadataJson() {
    await copyTextToClipboard(datasetPropertiesMetadataJson, 'Metadata JSON');
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

  function handleGlobalPointerDown(event: MouseEvent) {
    if (!datasetContextMenuVisible) return;

    const target = event.target as HTMLElement | null;
    if (target?.closest('.dataset-context-menu')) {
      return;
    }

    handleDatasetContextMenuClose();
  }

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
    await reloadSlidesForSelectedDataset();
    refreshing = false;
    pullDistance = 0;
    isPulling = false;
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

<svelte:window onkeydown={handleGlobalModalKeydown} onclick={handleGlobalPointerDown} />

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
      <span class="app-title">EOSIN</span>
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
        <div
          class="dataset-picker-wrap"
          onclick={(e) => e.stopPropagation()}
          onkeydown={(e) => e.stopPropagation()}
        >
          <button
            class="dataset-picker-btn"
            onclick={openDatasetModal}
            oncontextmenu={handleDatasetContextMenu}
            ontouchstart={handleDatasetTouchStart}
            ontouchend={handleDatasetTouchEnd}
            ontouchmove={handleDatasetTouchMove}
            disabled={datasets.length === 0}
            aria-label="Open dataset picker"
          >
            <span class="dataset-picker-label">{selectedDataset?.name ?? 'No datasets'}</span>
            <svg xmlns="http://www.w3.org/2000/svg" width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
              <polyline points="6 9 12 15 18 9"></polyline>
            </svg>
          </button>
        </div>
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

{#if datasetModalOpen}
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div class="dataset-modal-overlay" onclick={closeDatasetModal}>
    <div class="dataset-modal" onclick={(e) => e.stopPropagation()} role="dialog" aria-modal="true" aria-label="Select dataset" tabindex="-1">
      <div class="dataset-modal-header">
        <h3>Datasets</h3>
        <button class="dataset-modal-close" onclick={closeDatasetModal} aria-label="Close dataset picker">
          <svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 20 20" fill="currentColor">
            <path d="M6.28 5.22a.75.75 0 00-1.06 1.06L8.94 10l-3.72 3.72a.75.75 0 101.06 1.06L10 11.06l3.72 3.72a.75.75 0 101.06-1.06L11.06 10l3.72-3.72a.75.75 0 00-1.06-1.06L10 8.94 6.28 5.22z"/>
          </svg>
        </button>
      </div>

      <div class="dataset-modal-body">
        <input
          class="dataset-search"
          type="text"
          bind:value={datasetSearch}
          placeholder="Search datasets"
          aria-label="Search datasets"
        />

        <div class="dataset-list" role="listbox" aria-label="Dataset list">
          {#if filteredDatasets.length === 0}
            <div class="dataset-empty">No datasets found</div>
          {:else}
            {#each filteredDatasets as dataset (dataset.id)}
              <button
                class="dataset-item"
                class:selected={dataset.id === selectedDatasetId}
                onclick={() => handleDatasetPick(dataset.id)}
                role="option"
                aria-selected={dataset.id === selectedDatasetId}
              >
                <div class="dataset-item-top">
                  <span class="dataset-item-name">{dataset.name}</span>
                  {#if dataset.id === selectedDatasetId}
                    <span class="dataset-item-selected">Selected</span>
                  {/if}
                </div>
                {#if dataset.description}
                  <p class="dataset-item-description">{dataset.description}</p>
                {/if}
                {#if dataset.credit}
                  <p class="dataset-item-credit" title={dataset.credit}>{dataset.credit}</p>
                {/if}
                <div class="dataset-item-meta">
                  <span>Updated {formatTimestamp(dataset.updated_at)}</span>
                  <span>{dataset.slide_count} slides</span>
                  <span>{formatSize(dataset.full_size)}</span>
                  <span>(Warm / in-cache)</span>
                </div>
              </button>
            {/each}
          {/if}
        </div>
      </div>
    </div>
  </div>
{/if}

{#if propertiesModalOpen && propertiesSlide}
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div class="properties-modal-overlay" onclick={closePropertiesModal}>
    <div class="properties-modal" onclick={(e) => e.stopPropagation()} role="dialog" aria-modal="true" aria-label="Slide properties" tabindex="-1">
      <div class="properties-modal-header">
        <div class="properties-modal-title-wrap">
          <h3>Slide Properties</h3>
          <p>{getSlideLabel(propertiesSlide)}</p>
        </div>
        <div class="properties-modal-actions">
          <button class="properties-copy-btn" onclick={copyAllProperties}>Copy All</button>
          <button class="properties-modal-close" onclick={closePropertiesModal} aria-label="Close properties">
            <svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 20 20" fill="currentColor">
              <path d="M6.28 5.22a.75.75 0 00-1.06 1.06L8.94 10l-3.72 3.72a.75.75 0 101.06 1.06L10 11.06l3.72 3.72a.75.75 0 101.06-1.06L11.06 10l3.72-3.72a.75.75 0 00-1.06-1.06L10 8.94 6.28 5.22z"/>
            </svg>
          </button>
        </div>
      </div>

      <div class="properties-modal-body">
        <div class="properties-list" role="list">
          {#each propertiesRows as row (row.key)}
            <div class="properties-row" role="listitem">
              <span class="properties-key">{row.label}</span>
              <span class="properties-value" title={row.value}>{row.value}</span>
            </div>
          {/each}
        </div>

        <div class="properties-metadata">
          <div class="properties-metadata-header">
            <span>Metadata JSON</span>
            <button class="properties-copy-btn" onclick={copyMetadataJson}>Copy JSON</button>
          </div>
          <pre class="properties-metadata-code"><code>{propertiesMetadataJson}</code></pre>
        </div>

        <div class="properties-dataset-card">
          <div class="properties-metadata-header">
            <span>Dataset Credit</span>
          </div>
          <div class="properties-dataset-body">
            <div class="properties-dataset-name">{propertiesDataset?.name ?? '—'}</div>
            <div class="properties-dataset-credit" title={propertiesDataset?.credit ?? '—'}>{propertiesDataset?.credit ?? '—'}</div>
          </div>
        </div>
      </div>
    </div>
  </div>
{/if}

{#if datasetPropertiesModalOpen && datasetPropertiesDataset}
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div class="properties-modal-overlay" onclick={closeDatasetPropertiesModal}>
    <div class="properties-modal" onclick={(e) => e.stopPropagation()} role="dialog" aria-modal="true" aria-label="Dataset properties" tabindex="-1">
      <div class="properties-modal-header">
        <div class="properties-modal-title-wrap">
          <h3>Dataset Properties</h3>
          <p>{datasetPropertiesDataset.name}</p>
        </div>
        <div class="properties-modal-actions">
          <button class="properties-copy-btn" onclick={copyAllDatasetProperties}>Copy All</button>
          <button class="properties-modal-close" onclick={closeDatasetPropertiesModal} aria-label="Close properties">
            <svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 20 20" fill="currentColor">
              <path d="M6.28 5.22a.75.75 0 00-1.06 1.06L8.94 10l-3.72 3.72a.75.75 0 101.06 1.06L10 11.06l3.72 3.72a.75.75 0 101.06-1.06L11.06 10l3.72-3.72a.75.75 0 00-1.06-1.06L10 8.94 6.28 5.22z"/>
            </svg>
          </button>
        </div>
      </div>

      <div class="properties-modal-body">
        <div class="properties-list" role="list">
          {#each datasetPropertiesRows as row (row.key)}
            <div class="properties-row" role="listitem">
              <span class="properties-key">{row.label}</span>
              <span class="properties-value" title={row.value}>{row.value}</span>
            </div>
          {/each}
        </div>

        <div class="properties-metadata">
          <div class="properties-metadata-header">
            <span>Metadata JSON</span>
            <button class="properties-copy-btn" onclick={copyDatasetMetadataJson}>Copy JSON</button>
          </div>
          <pre class="properties-metadata-code"><code>{datasetPropertiesMetadataJson}</code></pre>
        </div>
      </div>
    </div>
  </div>
{/if}

{#if datasetContextMenuVisible}
  <div class="dataset-context-menu" style="left: {Math.min(datasetContextMenuX, (browser ? window.innerWidth : 9999) - 200)}px; top: {Math.min(datasetContextMenuY, (browser ? window.innerHeight : 9999) - 80)}px;" role="menu">
    <button class="context-menu-item" role="menuitem" onclick={handleDatasetContextMenuProperties}>
      <svg xmlns="http://www.w3.org/2000/svg" width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
        <circle cx="12" cy="12" r="3"></circle>
        <path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 1 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 1 1-4 0v-.09a1.65 1.65 0 0 0-1-1.51 1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 1 1-2.83-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 1 1 0-4h.09a1.65 1.65 0 0 0 1.51-1 1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 1 1 2.83-2.83l.06.06a1.65 1.65 0 0 0 1.82.33h0a1.65 1.65 0 0 0 1-1.51V3a2 2 0 1 1 4 0v.09a1.65 1.65 0 0 0 1 1.51h0a1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 1 1 2.83 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82v0a1.65 1.65 0 0 0 1.51 1H21a2 2 0 1 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z"></path>
      </svg>
      Properties
    </button>
  </div>
{/if}

<ContextMenu
  x={contextMenuX}
  y={contextMenuY}
  visible={contextMenuVisible}
  onOpen={handleContextMenuOpen}
  onOpenInNewTab={handleContextMenuOpenInNewTab}
  onProperties={handleContextMenuProperties}
  onClose={handleContextMenuClose}
/>

<style>
  .sidebar {
    width: 280px;
    min-width: 280px;
    height: 100%;
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
    padding: 0 0.75rem;
    border-bottom: 1px solid #333;
    position: sticky;
    top: 0;
    background: #141414;
    z-index: 10;
    gap: 0.5rem;
    min-height: 48px;
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
    background: var(--secondary-muted);
    color: #fff;
  }

  .slide-item.active {
    background: var(--primary-hex);
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
    border-top-color: var(--secondary-hex);
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
    color: var(--primary-hex);
  }

  .pull-indicator .spinner {
    width: 16px;
    height: 16px;
    border: 2px solid #333;
    border-top-color: var(--primary-hex);
    border-radius: 50%;
    animation: spin 0.8s linear infinite;
  }

  /* Touch device adaptations - larger touch targets */
  @media (pointer: coarse) {
    .sidebar-header {
      height: 72px;
      min-height: 72px;
      max-height: 72px;
    }

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
  }

  /* Section styles for VS Code-like split view */
  .app-title {
    font-size: 0.9rem;
    font-family: 'Inter', sans-serif;
    font-weight: 500;
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

  .dataset-picker-wrap {
    max-width: 180px;
    min-width: 100px;
    display: flex;
    align-items: center;
  }

  .dataset-picker-btn {
    width: 100%;
    background: #242424;
    color: #cfcfcf;
    border: 1px solid #3a3a3a;
    border-radius: 4px;
    font-size: 0.6875rem;
    padding: 0.2rem 0.4rem;
    cursor: pointer;
    line-height: 1.25;
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 0.3rem;
  }

  .dataset-picker-btn:hover {
    border-color: #4a4a4a;
    background: #2b2b2b;
  }

  .dataset-picker-btn:focus {
    outline: none;
    border-color: #3a3a3a;
    box-shadow: none;
  }

  .dataset-picker-btn:disabled {
    opacity: 0.65;
    cursor: not-allowed;
  }

  .dataset-picker-label {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    text-align: left;
  }

  .dataset-modal-overlay {
    position: fixed;
    inset: 0;
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 1rem;
    background: rgba(0, 0, 0, 0.6);
    z-index: 1000;
  }

  .dataset-context-menu {
    position: fixed;
    z-index: 10050;
    background: #222;
    border: 1px solid #444;
    border-radius: 6px;
    box-shadow: 0 4px 16px rgba(0, 0, 0, 0.5);
    padding: 4px 0;
    min-width: 180px;
    animation: fadeIn 0.1s ease-out;
    user-select: none;
  }

  .dataset-context-menu .context-menu-item {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    width: 100%;
    padding: 0.5rem 0.75rem;
    background: transparent;
    border: none;
    color: #ddd;
    font-size: 0.8125rem;
    cursor: pointer;
    text-align: left;
    transition: background-color 0.1s;
  }

  .dataset-context-menu .context-menu-item:hover {
    background: var(--secondary-hex);
    color: #fff;
  }

  .dataset-modal {
    width: min(860px, 100%);
    height: 600px;
    background: #1a1a1a;
    border: 1px solid #3a3a3a;
    border-radius: 8px;
    display: flex;
    flex-direction: column;
    overflow: hidden;
  }

  .dataset-modal-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 0.7rem 0.85rem;
    border-bottom: 1px solid #2f2f2f;
  }

  .dataset-modal-header h3 {
    margin: 0;
    font-size: 0.9rem;
    font-weight: 600;
    color: #e3e3e3;
  }

  .dataset-modal-close {
    width: 28px;
    height: 28px;
    border: 1px solid #3a3a3a;
    border-radius: 6px;
    background: #232323;
    color: #bdbdbd;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    cursor: pointer;
  }

  .dataset-modal-close:hover {
    background: #2d2d2d;
    color: #e3e3e3;
  }

  .dataset-modal-close svg {
    width: 14px;
    height: 14px;
  }

  .dataset-modal-body {
    display: flex;
    flex-direction: column;
    gap: 0.6rem;
    padding: 0.75rem;
    min-height: 0;
  }

  .dataset-search {
    width: 100%;
    border: 1px solid #3a3a3a;
    background: #232323;
    color: #ddd;
    border-radius: 6px;
    padding: 0.5rem 0.65rem;
    font-size: 0.8rem;
  }

  .dataset-search:focus {
    outline: none;
    border-color: #4a4a4a;
  }

  .dataset-list {
    min-height: 0;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: 0.4rem;
    padding-right: 0.15rem;
  }

  .dataset-item {
    width: 100%;
    text-align: left;
    border: 1px solid #313131;
    background: #202020;
    color: #ddd;
    border-radius: 6px;
    padding: 0.55rem 0.65rem;
    display: flex;
    flex-direction: column;
    gap: 0.35rem;
    cursor: pointer;
  }

  .dataset-item:hover {
    background: #272727;
    border-color: #404040;
  }

  .dataset-item.selected {
    border-color: var(--secondary-hex);
    background: var(--primary-hex);
    color: rgb(var(--primary-foreground));
  }

  .dataset-item.selected .dataset-item-name,
  .dataset-item.selected .dataset-item-description,
  .dataset-item.selected .dataset-item-meta,
  .dataset-item.selected .dataset-item-selected {
    color: rgb(var(--primary-foreground));
  }

  .dataset-item-top {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 0.5rem;
  }

  .dataset-item-name {
    font-size: 0.83rem;
    font-weight: 600;
    color: #f0f0f0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .dataset-item-selected {
    font-size: 0.65rem;
    color: #9a9a9a;
    flex-shrink: 0;
  }

  .dataset-item-description {
    margin: 0;
    font-size: 0.72rem;
    color: #a8a8a8;
    line-height: 1.3;
  }

  .dataset-item-credit {
    margin: 0;
    font-size: 0.69rem;
    color: #7f7f7f;
    line-height: 1.3;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .dataset-item-meta {
    font-size: 0.68rem;
    color: #8d8d8d;
    display: flex;
    flex-wrap: wrap;
    gap: 0.7rem;
  }

  .dataset-empty {
    padding: 1rem;
    border: 1px dashed #3a3a3a;
    border-radius: 6px;
    font-size: 0.75rem;
    color: #888;
    text-align: center;
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

  .properties-modal-overlay {
    position: fixed;
    inset: 0;
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 1rem;
    background: rgba(0, 0, 0, 0.62);
    z-index: 1010;
  }

  .properties-modal {
    width: min(760px, 100%);
    height: 620px;
    background: #1a1a1a;
    border: 1px solid #3a3a3a;
    border-radius: 8px;
    display: flex;
    flex-direction: column;
    overflow: hidden;
  }

  .properties-modal-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 1rem;
    padding: 0.75rem 0.9rem;
    border-bottom: 1px solid #2f2f2f;
  }

  .properties-modal-title-wrap {
    min-width: 0;
  }

  .properties-modal-title-wrap h3 {
    margin: 0;
    font-size: 0.9rem;
    font-weight: 600;
    color: #e3e3e3;
  }

  .properties-modal-title-wrap p {
    margin: 0.2rem 0 0;
    font-size: 0.75rem;
    color: #9d9d9d;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .properties-modal-actions {
    display: flex;
    align-items: center;
    gap: 0.5rem;
  }

  .properties-copy-btn {
    border: 1px solid #3a3a3a;
    border-radius: 6px;
    background: #242424;
    color: #d6d6d6;
    font-size: 0.75rem;
    font-weight: 500;
    padding: 0.35rem 0.6rem;
    cursor: pointer;
    transition: background-color 0.1s, border-color 0.1s;
  }

  .properties-copy-btn:hover {
    background: #2d2d2d;
    border-color: #4a4a4a;
  }

  .properties-modal-close {
    width: 28px;
    height: 28px;
    border: 1px solid #3a3a3a;
    border-radius: 6px;
    background: #232323;
    color: #bdbdbd;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    cursor: pointer;
  }

  .properties-modal-close:hover {
    background: #2d2d2d;
    color: #e3e3e3;
  }

  .properties-modal-close svg {
    width: 14px;
    height: 14px;
  }

  .properties-modal-body {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
    gap: 0.7rem;
    padding: 0.75rem;
  }

  .properties-list {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    border: 1px solid #2f2f2f;
    border-radius: 6px;
    background: #171717;
    scrollbar-width: thin;
    scrollbar-color: #333 transparent;
  }

  .properties-list::-webkit-scrollbar {
    width: 8px;
  }

  .properties-list::-webkit-scrollbar-track {
    background: transparent;
  }

  .properties-list::-webkit-scrollbar-thumb {
    background: #333;
    border-radius: 3px;
  }

  .properties-list::-webkit-scrollbar-thumb:hover {
    background: #555;
  }

  .properties-row {
    display: grid;
    grid-template-columns: minmax(120px, 180px) minmax(0, 1fr);
    gap: 0.75rem;
    padding: 0.45rem 0.6rem;
    border-bottom: 1px solid #242424;
  }

  .properties-row:last-child {
    border-bottom: none;
  }

  .properties-key {
    font-size: 0.74rem;
    color: #9a9a9a;
  }

  .properties-value {
    font-size: 0.78rem;
    color: #dfdfdf;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .properties-metadata {
    border: 1px solid #2f2f2f;
    border-radius: 6px;
    background: #171717;
    overflow: hidden;
  }

  .properties-dataset-card {
    border: 1px solid #2f2f2f;
    border-radius: 6px;
    background: #171717;
    overflow: hidden;
  }

  .properties-dataset-body {
    padding: 0.55rem 0.6rem;
    display: flex;
    flex-direction: column;
    gap: 0.45rem;
  }

  .properties-dataset-name {
    font-size: 0.74rem;
    color: #9a9a9a;
  }

  .properties-dataset-credit {
    font-size: 0.78rem;
    color: #dfdfdf;
    line-height: 1.35;
    max-height: calc(1.35em * 4);
    overflow-y: auto;
    overflow-x: hidden;
    white-space: pre-wrap;
    word-break: break-word;
    scrollbar-width: thin;
    scrollbar-color: #333 transparent;
  }

  .properties-dataset-credit::-webkit-scrollbar {
    width: 8px;
  }

  .properties-dataset-credit::-webkit-scrollbar-track {
    background: transparent;
  }

  .properties-dataset-credit::-webkit-scrollbar-thumb {
    background: #333;
    border-radius: 3px;
  }

  .properties-dataset-credit::-webkit-scrollbar-thumb:hover {
    background: #555;
  }

  .properties-metadata-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 0.5rem;
    padding: 0.45rem 0.6rem;
    border-bottom: 1px solid #2a2a2a;
    background: #1f1f1f;
    font-size: 0.74rem;
    color: #b5b5b5;
  }

  .properties-metadata-code {
    margin: 0;
    height: 190px;
    overflow: auto;
    padding: 0.6rem;
    font-family: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, 'Liberation Mono', 'Courier New', monospace;
    font-size: 0.72rem;
    line-height: 1.35;
    color: #cfcfcf;
    scrollbar-width: thin;
    scrollbar-color: #333 transparent;
  }

  .properties-metadata-code::-webkit-scrollbar {
    width: 8px;
    height: 8px;
  }

  .properties-metadata-code::-webkit-scrollbar-track {
    background: transparent;
  }

  .properties-metadata-code::-webkit-scrollbar-thumb {
    background: #333;
    border-radius: 3px;
  }

  .properties-metadata-code::-webkit-scrollbar-thumb:hover {
    background: #555;
  }

  @media (max-width: 860px) {
    .properties-modal {
      height: min(620px, calc(100vh - 2rem));
    }

    .properties-row {
      grid-template-columns: 1fr;
      gap: 0.25rem;
    }
  }
</style>
