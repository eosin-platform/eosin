<script lang="ts">
  import { settings } from '$lib/stores/settings';
  import ViewerHudMoreMenu from './ViewerHudMoreMenu.svelte';

  interface Props {
    /** Current zoom level (0-1 range typically, displayed as percentage) */
    zoom: number;
    /** Callback when zoom level is changed via input */
    onZoomChange: (zoom: number) => void;
    /** Callback to reset/fit view */
    onFitView: () => void;
    /** Current magnification (e.g., "10x", "40x") - optional */
    magnification?: string;
    /** Whether panning is currently happening - closes menu immediately */
    isPanning?: boolean;
    /** Whether mask painting mode is active */
    isMaskPainting?: boolean;
    /** Current brush size in pixels */
    maskBrushSize?: number;
  }

  let { zoom, onZoomChange, onFitView, magnification, isPanning = false, isMaskPainting = false, maskBrushSize = 20 }: Props = $props();

  // Bind to settings store
  let scaleBarVisible = $state($settings.image.scaleBarVisible);

  // Keep local state in sync with store
  $effect(() => {
    scaleBarVisible = $settings.image.scaleBarVisible;
  });

  // Local state for the more menu (per-instance, not global)
  let moreMenuOpen = $state(false);

  // Close menu immediately when panning starts
  $effect(() => {
    if (isPanning && moreMenuOpen) {
      moreMenuOpen = false;
    }
  });

  function toggleScaleBar() {
    scaleBarVisible = !scaleBarVisible;
    settings.setSetting('image', 'scaleBarVisible', scaleBarVisible);
  }

  function toggleMoreMenu() {
    moreMenuOpen = !moreMenuOpen;
  }

  function closeMoreMenu() {
    moreMenuOpen = false;
  }

  // Format zoom for display
  const zoomDisplay = $derived(() => {
    if (zoom >= 1) {
      return `${zoom.toFixed(1)}x`;
    }
    return `${(zoom * 100).toFixed(0)}%`;
  });

  // Zoom input state
  let zoomInputValue = $state('');
  let zoomInputFocused = $state(false);

  // Keep input value synced with zoom when not focused
  $effect(() => {
    if (!zoomInputFocused) {
      zoomInputValue = zoomDisplay();
    }
  });

  function handleZoomInputFocus() {
    zoomInputFocused = true;
    // Strip the % or x suffix for easier editing
    if (zoom >= 1) {
      zoomInputValue = zoom.toFixed(1);
    } else {
      zoomInputValue = (zoom * 100).toFixed(0);
    }
  }

  function handleZoomInputBlur() {
    zoomInputFocused = false;
    applyZoomInput();
  }

  function handleZoomInputKeydown(e: KeyboardEvent) {
    if (e.key === 'Enter') {
      applyZoomInput();
      (e.target as HTMLInputElement).blur();
    } else if (e.key === 'Escape') {
      zoomInputFocused = false;
      zoomInputValue = zoomDisplay();
      (e.target as HTMLInputElement).blur();
    }
  }

  function applyZoomInput() {
    const value = zoomInputValue.trim();
    let newZoom: number;

    // Parse the input - handle both "50%" and "2x" formats
    if (value.endsWith('x')) {
      newZoom = parseFloat(value.slice(0, -1));
    } else if (value.endsWith('%')) {
      newZoom = parseFloat(value.slice(0, -1)) / 100;
    } else {
      // Assume it's a number - if >= 1, treat as multiplier, else as percentage
      const num = parseFloat(value);
      if (isNaN(num)) return;
      // If user types a small number like 0.5, treat as zoom level
      // If they type a larger number like 50, treat as percentage
      if (num > 10) {
        newZoom = num / 100;
      } else {
        newZoom = num;
      }
    }

    if (!isNaN(newZoom) && newZoom > 0) {
      onZoomChange(newZoom);
    }
  }

  // Stop mouse/touch events from propagating to the viewer (prevents panning)
  function stopPropagation(e: Event) {
    e.stopPropagation();
  }
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="hud-container">
  <div 
    class="viewer-hud"
    onmousedown={stopPropagation}
    ontouchstart={stopPropagation}
    onwheel={stopPropagation}
  >
  <!-- Zoom input -->
  <input
    type="text"
    class="zoom-input"
    value={zoomInputValue}
    oninput={(e) => zoomInputValue = (e.target as HTMLInputElement).value}
    onfocus={handleZoomInputFocus}
    onblur={handleZoomInputBlur}
    onkeydown={handleZoomInputKeydown}
    title="Zoom level (e.g., 50%, 2x, or 0.5)"
    aria-label="Zoom level"
  />
  {#if magnification}
    <span class="magnification">({magnification})</span>
  {/if}
  <button onclick={onFitView} class="icon-btn" title="Fit to View">
    <!-- Fit/expand icon -->
    <svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 20 20" fill="currentColor" class="icon">
      <path d="M13.28 7.78l3.22-3.22v2.69a.75.75 0 001.5 0v-4.5a.75.75 0 00-.75-.75h-4.5a.75.75 0 000 1.5h2.69l-3.22 3.22a.75.75 0 001.06 1.06zM2 17.25v-4.5a.75.75 0 011.5 0v2.69l3.22-3.22a.75.75 0 011.06 1.06L4.56 16.5h2.69a.75.75 0 010 1.5h-4.5a.75.75 0 01-.75-.75zM12.22 13.28l3.22 3.22h-2.69a.75.75 0 000 1.5h4.5a.75.75 0 00.75-.75v-4.5a.75.75 0 00-1.5 0v2.69l-3.22-3.22a.75.75 0 00-1.06 1.06zM3.5 4.56l3.22 3.22a.75.75 0 001.06-1.06L4.56 3.5h2.69a.75.75 0 000-1.5h-4.5a.75.75 0 00-.75.75v4.5a.75.75 0 001.5 0V4.56z" />
    </svg>
  </button>

  <!-- Divider -->
  <div class="hud-divider"></div>

  <!-- Toggle buttons -->
  <button
    onclick={toggleScaleBar}
    class="icon-btn"
    class:active={scaleBarVisible}
    title={scaleBarVisible ? 'Hide Scale Bar' : 'Show Scale Bar'}
  >
    <!-- Ruler icon -->
    <svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 20 20" fill="currentColor" class="icon">
      <path fill-rule="evenodd" d="M2 4.25A2.25 2.25 0 014.25 2h11.5A2.25 2.25 0 0118 4.25v2a.75.75 0 01-1.5 0V5.5h-2v.75a.75.75 0 01-1.5 0V5.5h-2v.75a.75.75 0 01-1.5 0V5.5h-2v.75a.75.75 0 01-1.5 0V5.5h-2v.75a.75.75 0 01-1.5 0v-2z" clip-rule="evenodd" />
    </svg>
  </button>
  
  <!-- More menu button -->
  <button
    onclick={toggleMoreMenu}
    class="icon-btn"
    class:active={moreMenuOpen}
    title="More Settings"
  >
    <!-- Sliders/adjustments icon -->
    <svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 20 20" fill="currentColor" class="icon">
      <path d="M17 4.25a.75.75 0 01-.75.75h-5.5a.75.75 0 010-1.5h5.5a.75.75 0 01.75.75zM17 10a.75.75 0 01-.75.75h-10.5a.75.75 0 010-1.5h10.5a.75.75 0 01.75.75zM17 15.75a.75.75 0 01-.75.75h-5.5a.75.75 0 010-1.5h5.5a.75.75 0 01.75.75zM4.25 5.5a1.25 1.25 0 100-2.5 1.25 1.25 0 000 2.5zM4.25 11.25a1.25 1.25 0 100-2.5 1.25 1.25 0 000 2.5zM4.25 17a1.25 1.25 0 100-2.5 1.25 1.25 0 000 2.5z" />
    </svg>
  </button>
  </div>

  <!-- Brush size HUD (separate visual island) -->
  {#if isMaskPainting}
    <div class="brush-hud">
      <div class="brush-preview" style="width: {Math.min(maskBrushSize * 0.5, 24)}px; height: {Math.min(maskBrushSize * 0.5, 24)}px;"></div>
      <span class="brush-size-label">{maskBrushSize}px</span>
    </div>
  {/if}

  <!-- More menu popover - rendered inside hud-container for proper positioning -->
  {#if moreMenuOpen}
    <div class="menu-wrapper">
      <ViewerHudMoreMenu onClose={closeMoreMenu} />
    </div>
  {/if}
</div>

<style>
  .hud-container {
    position: absolute;
    top: 1rem;
    left: 1rem;
    z-index: 30;
    max-width: calc(100vw - 2rem);
    display: flex;
    flex-direction: row;
    align-items: flex-start;
    gap: 0.5rem;
    /* Prevent selection on touch devices */
    -webkit-touch-callout: none;
    -webkit-user-select: none;
    user-select: none;
  }

  .viewer-hud {
    display: flex;
    flex-direction: row;
    flex-wrap: wrap;
    align-items: center;
    gap: 0.5rem;
    padding: 0.5rem 0.75rem;
    background: rgba(20, 20, 20, 0.75);
    backdrop-filter: blur(12px);
    border-radius: 0.75rem;
    border: 1px solid rgba(255, 255, 255, 0.1);
    overflow: visible;
    box-shadow: 0 4px 16px rgba(0, 0, 0, 0.3);
  }

  .hud-divider {
    width: 1px;
    height: 1.25rem;
    background: rgba(255, 255, 255, 0.15);
    flex-shrink: 0;
  }

  .icon {
    width: 1rem;
    height: 1rem;
  }

  .brush-hud {
    display: flex;
    flex-direction: row;
    align-items: center;
    gap: 0.375rem;
    padding: 0.375rem 0.5rem;
    background: rgba(20, 20, 20, 0.75);
    backdrop-filter: blur(12px);
    border-radius: 0.5rem;
    border: 1px solid rgba(255, 255, 255, 0.1);
    box-shadow: 0 4px 16px rgba(0, 0, 0, 0.3);
  }

  .brush-preview {
    border: 1.5px solid rgba(255, 255, 255, 0.8);
    border-radius: 50%;
    background: rgba(59, 130, 246, 0.4);
    min-width: 6px;
    min-height: 6px;
    max-width: 24px;
    max-height: 24px;
  }

  .brush-size-label {
    color: #fff;
    font-size: 0.75rem;
    font-weight: 500;
  }

  .zoom-input {
    width: 4.5rem;
    padding: 0.375rem 0.5rem;
    background: #374151;
    border: 1px solid #4b5563;
    border-radius: 0.375rem;
    color: #e5e7eb;
    font-size: 0.75rem;
    font-weight: 500;
    text-align: center;
    outline: none;
    transition: border-color 0.15s, box-shadow 0.15s;
  }

  .zoom-input:hover {
    border-color: #6b7280;
  }

  .zoom-input:focus {
    border-color: #3b82f6;
    box-shadow: 0 0 0 2px rgba(59, 130, 246, 0.3);
  }

  .magnification {
    color: #9ca3af;
    font-weight: 400;
  }

  .icon-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 2rem;
    height: 2rem;
    padding: 0;
    background: #374151;
    border: 1px solid #4b5563;
    border-radius: 0.375rem;
    color: #9ca3af;
    cursor: pointer;
    transition: all 0.15s;
  }

  .icon-btn:hover {
    background: #4b5563;
    color: #e5e7eb;
  }

  .icon-btn.active {
    background: #3b82f6;
    border-color: #3b82f6;
    color: white;
  }

  /* Mobile adjustments */
  @media (max-width: 480px) {
    .viewer-hud {
      top: 0.5rem;
      left: 0.5rem;
      max-width: none;
      padding: 0.5rem;
    }
  }

  /* Touch device adaptations - larger touch targets */
  @media (pointer: coarse) {
    .icon-btn {
      width: 2.75rem;
      height: 2.75rem;
    }

    .icon-btn svg {
      width: 1.25rem;
      height: 1.25rem;
    }

    /* Larger zoom input for touch */
    .zoom-input {
      padding: 0.625rem 0.75rem;
      font-size: 0.875rem;
      min-height: 44px;
      width: 5rem;
    }
  }

  /* Menu wrapper with same blur as hud toolbar */
  .menu-wrapper {
    position: absolute;
    top: calc(100% + 0.5rem);
    left: 0;
    min-width: 240px;
    background: rgba(20, 20, 20, 0.75);
    backdrop-filter: blur(12px);
    -webkit-backdrop-filter: blur(12px);
    border-radius: 0.75rem;
    border: 1px solid rgba(255, 255, 255, 0.1);
    box-shadow: 0 8px 32px rgba(0, 0, 0, 0.4);
    z-index: 40;
    /* overflow: hidden is needed for border-radius to clip content, but scrolling is handled by .more-menu inside */
  }

  /* On mobile, the menu handles its own full-screen styling */
  @media (max-width: 600px) {
    .menu-wrapper {
      position: static;
      min-width: unset;
      background: transparent;
      border: none;
      border-radius: 0;
      box-shadow: none;
    }
  }
</style>
