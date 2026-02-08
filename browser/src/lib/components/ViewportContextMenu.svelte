<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import { browser } from '$app/environment';
  import { authStore } from '$lib/stores/auth';
  import { annotationStore, activeAnnotationSet } from '$lib/stores/annotations';

  interface Props {
    /** X position in viewport pixels */
    x: number;
    /** Y position in viewport pixels */
    y: number;
    /** Whether the menu is visible */
    visible: boolean;
    /** Current position in image coordinates (level 0) */
    imageX?: number;
    imageY?: number;
    /** Callback for "Save Image As..." action */
    onSaveImage: () => void;
    /** Callback for "Copy Image" action */
    onCopyImage: () => void;
    /** Callback to dismiss the menu */
    onClose: () => void;
    /** Callback after creating an annotation */
    onAnnotationCreated?: () => void;
    /** Callback to start interactive ellipse creation at given center */
    onStartEllipseCreation?: (centerX: number, centerY: number) => void;
  }

  let { x, y, visible, imageX, imageY, onSaveImage, onCopyImage, onClose, onAnnotationCreated, onStartEllipseCreation }: Props = $props();

  let menuEl = $state<HTMLDivElement>();
  let showAnnotationSubmenu = $state(false);

  // Auth state
  let isLoggedIn = $state(false);
  const unsubAuth = authStore.subscribe((state) => {
    isLoggedIn = state.user !== null;
  });

  // Active annotation set
  let currentActiveSet = $state<typeof $activeAnnotationSet>(null);
  const unsubActiveSet = activeAnnotationSet.subscribe((v) => {
    currentActiveSet = v;
  });

  onDestroy(() => {
    unsubAuth();
    unsubActiveSet();
  });

  // Computed: can create annotations
  let canCreate = $derived(isLoggedIn && currentActiveSet !== null && !currentActiveSet.locked);

  function handleSaveImage() {
    onSaveImage();
    onClose();
  }

  function handleCopyImage() {
    onCopyImage();
    onClose();
  }

  async function handleCreatePoint() {
    if (!canCreate || imageX === undefined || imageY === undefined) return;
    
    try {
      await annotationStore.createAnnotation({
        kind: 'point',
        label_id: 'unlabeled',
        geometry: {
          x_level0: imageX,
          y_level0: imageY,
        },
      });
      onAnnotationCreated?.();
    } catch (err) {
      console.error('Failed to create point annotation:', err);
    }
    onClose();
  }

  async function handleCreateEllipse() {
    if (!canCreate || imageX === undefined || imageY === undefined) return;
    
    // Start interactive ellipse creation flow
    if (onStartEllipseCreation) {
      onStartEllipseCreation(imageX, imageY);
      onClose();
      return;
    }
    
    // Fallback: create default ellipse if no interactive mode available
    const defaultRadius = 50;
    
    try {
      await annotationStore.createAnnotation({
        kind: 'ellipse',
        label_id: 'unlabeled',
        geometry: {
          cx_level0: imageX,
          cy_level0: imageY,
          radius_x: defaultRadius,
          radius_y: defaultRadius,
          rotation_radians: 0,
        },
      });
      onAnnotationCreated?.();
    } catch (err) {
      console.error('Failed to create ellipse annotation:', err);
    }
    onClose();
  }

  function handleClickOutside(e: MouseEvent) {
    if (menuEl && !menuEl.contains(e.target as Node)) {
      onClose();
    }
  }

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === 'Escape') {
      onClose();
    }
  }

  onMount(() => {
    if (browser) {
      // Delay to avoid the same click event closing the menu
      requestAnimationFrame(() => {
        document.addEventListener('click', handleClickOutside, true);
        document.addEventListener('keydown', handleKeydown);
      });
    }
  });

  onDestroy(() => {
    if (browser) {
      document.removeEventListener('click', handleClickOutside, true);
      document.removeEventListener('keydown', handleKeydown);
    }
  });

  // Adjust position to stay within viewport
  let adjustedX = $derived(Math.min(x, (browser ? window.innerWidth : 9999) - 220));
  let adjustedY = $derived(Math.min(y, (browser ? window.innerHeight : 9999) - 200));

  // Get tooltip for disabled create button
  function getCreateTooltip(): string {
    if (!isLoggedIn) return 'Log in to create annotations';
    if (!currentActiveSet) return 'Select an annotation layer first';
    if (currentActiveSet.locked) return 'Layer is locked';
    return '';
  }
</script>

{#if visible}
  <div
    class="context-menu"
    bind:this={menuEl}
    style="left: {adjustedX}px; top: {adjustedY}px;"
    role="menu"
  >
    <!-- Annotation creation submenu -->
    <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
    <div 
      class="context-menu-item submenu-trigger"
      class:disabled={!canCreate}
      role="menuitem"
      tabindex="0"
      onmouseenter={() => showAnnotationSubmenu = true}
      onmouseleave={() => showAnnotationSubmenu = false}
      title={!canCreate ? getCreateTooltip() : ''}
    >
      <svg xmlns="http://www.w3.org/2000/svg" width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
        <polygon points="12 2 22 8.5 22 15.5 12 22 2 15.5 2 8.5 12 2"></polygon>
        <line x1="12" y1="22" x2="12" y2="15.5"></line>
        <polyline points="22 8.5 12 15.5 2 8.5"></polyline>
      </svg>
      <span>Create annotation</span>
      <svg class="chevron" xmlns="http://www.w3.org/2000/svg" width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
        <polyline points="9 18 15 12 9 6"></polyline>
      </svg>

      {#if showAnnotationSubmenu && canCreate}
        <div class="submenu">
          <button class="context-menu-item" role="menuitem" onclick={handleCreatePoint}>
            <svg xmlns="http://www.w3.org/2000/svg" width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
              <circle cx="12" cy="12" r="3"></circle>
            </svg>
            Point here
          </button>
          <button class="context-menu-item" role="menuitem" onclick={handleCreateEllipse}>
            <svg xmlns="http://www.w3.org/2000/svg" width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
              <ellipse cx="12" cy="12" rx="8" ry="5"></ellipse>
            </svg>
            Ellipse here
          </button>
        </div>
      {/if}
    </div>

    <div class="menu-divider"></div>

    <button class="context-menu-item" role="menuitem" onclick={handleSaveImage}>
      <svg xmlns="http://www.w3.org/2000/svg" width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
        <path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"></path>
        <polyline points="7 10 12 15 17 10"></polyline>
        <line x1="12" y1="15" x2="12" y2="3"></line>
      </svg>
      Save Image As...
    </button>
    <button class="context-menu-item" role="menuitem" onclick={handleCopyImage}>
      <svg xmlns="http://www.w3.org/2000/svg" width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
        <rect x="9" y="9" width="13" height="13" rx="2" ry="2"></rect>
        <path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1"></path>
      </svg>
      Copy Image
    </button>
  </div>
{/if}

<style>
  .context-menu {
    position: fixed;
    z-index: 10000;
    background: #222;
    border: 1px solid #444;
    border-radius: 6px;
    box-shadow: 0 4px 16px rgba(0, 0, 0, 0.5);
    padding: 4px 0;
    min-width: 200px;
    animation: fadeIn 0.1s ease-out;
  }

  @keyframes fadeIn {
    from {
      opacity: 0;
      transform: scale(0.95);
    }
    to {
      opacity: 1;
      transform: scale(1);
    }
  }

  .context-menu-item {
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
    position: relative;
  }

  .context-menu-item:hover {
    background: #0066cc;
    color: #fff;
  }

  .context-menu-item.disabled {
    color: #666;
    cursor: not-allowed;
  }

  .context-menu-item.disabled:hover {
    background: transparent;
    color: #666;
  }

  .context-menu-item:first-child {
    border-radius: 5px 5px 0 0;
  }

  .context-menu-item:last-child {
    border-radius: 0 0 5px 5px;
  }

  .menu-divider {
    height: 1px;
    background: #444;
    margin: 4px 0;
  }

  .submenu-trigger {
    position: relative;
  }

  .submenu-trigger span {
    flex: 1;
  }

  .submenu-trigger .chevron {
    margin-left: auto;
    opacity: 0.7;
  }

  .submenu {
    position: absolute;
    left: 100%;
    top: -4px;
    background: #222;
    border: 1px solid #444;
    border-radius: 6px;
    box-shadow: 0 4px 16px rgba(0, 0, 0, 0.5);
    padding: 4px 0;
    min-width: 160px;
    animation: fadeIn 0.1s ease-out;
  }

  .submenu .context-menu-item:first-child {
    border-radius: 5px 5px 0 0;
  }

  .submenu .context-menu-item:last-child {
    border-radius: 0 0 5px 5px;
  }

  /* Touch device adaptations - larger touch targets */
  @media (pointer: coarse) {
    .context-menu-item {
      padding: 0.875rem 1rem;
      font-size: 1rem;
      min-height: 48px;
      gap: 0.75rem;
    }

    .context-menu-item svg {
      width: 18px;
      height: 18px;
    }
  }

  /* Mobile: show submenu inline */
  @media (max-width: 600px) {
    .submenu {
      position: static;
      border: none;
      box-shadow: none;
      padding-left: 1.5rem;
      min-width: auto;
    }
  }
</style>
