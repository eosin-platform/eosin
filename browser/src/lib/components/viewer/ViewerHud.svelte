<script lang="ts">
  import { settings, hudMoreMenuOpen, type StainEnhancementMode } from '$lib/stores/settings';
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
  }

  let { zoom, onZoomChange, onFitView, magnification }: Props = $props();

  // Bind to settings store
  let brightness = $state($settings.image.brightness);
  let contrast = $state($settings.image.contrast);
  let stainEnhancement = $state<StainEnhancementMode>($settings.image.stainEnhancement);
  let scaleBarVisible = $state($settings.image.scaleBarVisible);
  let annotationsVisible = $state($settings.annotations.visible);

  // Keep local state in sync with store
  $effect(() => {
    brightness = $settings.image.brightness;
    contrast = $settings.image.contrast;
    stainEnhancement = $settings.image.stainEnhancement;
    scaleBarVisible = $settings.image.scaleBarVisible;
    annotationsVisible = $settings.annotations.visible;
  });

  // Debounce for sliders
  let brightnessTimeout: ReturnType<typeof setTimeout> | null = null;
  let contrastTimeout: ReturnType<typeof setTimeout> | null = null;

  // Tooltip visibility state
  let brightnessTooltipVisible = $state(false);
  let contrastTooltipVisible = $state(false);
  let brightnessAdjusting = $state(false);
  let contrastAdjusting = $state(false);

  function handleBrightnessChange(e: Event) {
    const target = e.target as HTMLInputElement;
    brightness = parseFloat(target.value);
    
    if (brightnessTimeout) clearTimeout(brightnessTimeout);
    brightnessTimeout = setTimeout(() => {
      settings.setSetting('image', 'brightness', brightness);
    }, 50);
  }

  function resetBrightness() {
    brightness = 0;
    settings.setSetting('image', 'brightness', 0);
  }

  function handleContrastChange(e: Event) {
    const target = e.target as HTMLInputElement;
    contrast = parseFloat(target.value);
    
    if (contrastTimeout) clearTimeout(contrastTimeout);
    contrastTimeout = setTimeout(() => {
      settings.setSetting('image', 'contrast', contrast);
    }, 50);
  }

  function resetContrast() {
    contrast = 0;
    settings.setSetting('image', 'contrast', 0);
  }

  function handleStainEnhancementChange(e: Event) {
    const target = e.target as HTMLSelectElement;
    stainEnhancement = target.value as StainEnhancementMode;
    settings.setSetting('image', 'stainEnhancement', stainEnhancement);
  }

  function toggleScaleBar() {
    scaleBarVisible = !scaleBarVisible;
    settings.setSetting('image', 'scaleBarVisible', scaleBarVisible);
  }

  function toggleAnnotations() {
    annotationsVisible = !annotationsVisible;
    settings.setSetting('annotations', 'visible', annotationsVisible);
  }

  function toggleMoreMenu() {
    hudMoreMenuOpen.update((v) => !v);
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

  // Stain enhancement options
  const stainEnhancementOptions: { value: StainEnhancementMode; label: string }[] = [
    { value: 'none', label: 'None' },
    { value: 'gram', label: 'Gram' },
    { value: 'afb', label: 'AFB' },
    { value: 'gms', label: 'GMS' },
  ];
  // Stop mouse/touch events from propagating to the viewer (prevents panning)
  function stopPropagation(e: Event) {
    e.stopPropagation();
  }
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div 
  class="viewer-hud"
  onmousedown={stopPropagation}
  ontouchstart={stopPropagation}
  onwheel={stopPropagation}
>
  <!-- Brightness slider -->
  <div class="slider-control">
    <span class="slider-label" aria-hidden="true">
      <!-- Sun icon for brightness -->
      <svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 20 20" fill="currentColor" class="icon">
        <path d="M10 2a.75.75 0 01.75.75v1.5a.75.75 0 01-1.5 0v-1.5A.75.75 0 0110 2zM10 15a.75.75 0 01.75.75v1.5a.75.75 0 01-1.5 0v-1.5A.75.75 0 0110 15zM10 7a3 3 0 100 6 3 3 0 000-6zM15.657 5.404a.75.75 0 10-1.06-1.06l-1.061 1.06a.75.75 0 001.06 1.061l1.061-1.06zM6.464 14.596a.75.75 0 10-1.06-1.06l-1.06 1.06a.75.75 0 001.06 1.06l1.06-1.06zM18 10a.75.75 0 01-.75.75h-1.5a.75.75 0 010-1.5h1.5A.75.75 0 0118 10zM5 10a.75.75 0 01-.75.75h-1.5a.75.75 0 010-1.5h1.5A.75.75 0 015 10zM14.596 15.657a.75.75 0 001.06-1.06l-1.06-1.061a.75.75 0 10-1.06 1.06l1.06 1.06zM5.404 6.464a.75.75 0 001.06-1.06l-1.06-1.06a.75.75 0 10-1.061 1.06l1.06 1.06z" />
      </svg>
    </span>
    <div class="slider-wrapper">
      <input
        type="range"
        min="-100"
        max="100"
        step="1"
        value={brightness}
        oninput={handleBrightnessChange}
        ondblclick={resetBrightness}
        onmouseenter={() => brightnessTooltipVisible = true}
        onmouseleave={() => { if (!brightnessAdjusting) brightnessTooltipVisible = false; }}
        onmousedown={() => brightnessAdjusting = true}
        onmouseup={() => { brightnessAdjusting = false; brightnessTooltipVisible = false; }}
        class="slider"
        aria-label="Brightness (double-click to reset)"
      />
      {#if brightnessTooltipVisible || brightnessAdjusting}
        <div class="slider-tooltip">{brightness}</div>
      {/if}
    </div>
  </div>
  
  <!-- Contrast slider -->
  <div class="slider-control">
    <span class="slider-label" aria-hidden="true">
      <!-- Contrast icon (circle half-filled) -->
      <svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 20 20" fill="currentColor" class="icon">
        <path fill-rule="evenodd" d="M10 18a8 8 0 100-16 8 8 0 000 16zm0-2a6 6 0 100-12v12z" clip-rule="evenodd" />
      </svg>
    </span>
    <div class="slider-wrapper">
      <input
        type="range"
        min="-100"
        max="100"
        step="1"
        value={contrast}
        oninput={handleContrastChange}
        ondblclick={resetContrast}
        onmouseenter={() => contrastTooltipVisible = true}
        onmouseleave={() => { if (!contrastAdjusting) contrastTooltipVisible = false; }}
        onmousedown={() => contrastAdjusting = true}
        onmouseup={() => { contrastAdjusting = false; contrastTooltipVisible = false; }}
        class="slider"
        aria-label="Contrast (double-click to reset)"
      />
      {#if contrastTooltipVisible || contrastAdjusting}
        <div class="slider-tooltip">{contrast}</div>
      {/if}
    </div>
  </div>

  <!-- Divider -->
  <div class="hud-divider"></div>

  <!-- Stain enhancement selector -->
  <select
    value={stainEnhancement}
    onchange={handleStainEnhancementChange}
    class="stain-select"
    title="Stain Enhancement"
  >
    {#each stainEnhancementOptions as mode}
      <option value={mode.value}>{mode.label}</option>
    {/each}
  </select>

  <!-- Divider -->
  <div class="hud-divider"></div>

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
  
  <button
    onclick={toggleAnnotations}
    class="icon-btn"
    class:active={annotationsVisible}
    title={annotationsVisible ? 'Hide Annotations' : 'Show Annotations'}
  >
    <!-- Eye icon -->
    <svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 20 20" fill="currentColor" class="icon">
      {#if annotationsVisible}
        <path d="M10 12.5a2.5 2.5 0 100-5 2.5 2.5 0 000 5z" />
        <path fill-rule="evenodd" d="M.664 10.59a1.651 1.651 0 010-1.186A10.004 10.004 0 0110 3c4.257 0 7.893 2.66 9.336 6.41.147.381.146.804 0 1.186A10.004 10.004 0 0110 17c-4.257 0-7.893-2.66-9.336-6.41zM14 10a4 4 0 11-8 0 4 4 0 018 0z" clip-rule="evenodd" />
      {:else}
        <path fill-rule="evenodd" d="M3.28 2.22a.75.75 0 00-1.06 1.06l14.5 14.5a.75.75 0 101.06-1.06l-1.745-1.745a10.029 10.029 0 003.3-4.38 1.651 1.651 0 000-1.185A10.004 10.004 0 009.999 3a9.956 9.956 0 00-4.744 1.194L3.28 2.22zM7.752 6.69l1.092 1.092a2.5 2.5 0 013.374 3.373l1.091 1.092a4 4 0 00-5.557-5.557z" clip-rule="evenodd" />
        <path d="M10.748 13.93l2.523 2.523a9.987 9.987 0 01-3.27.547c-4.258 0-7.894-2.66-9.337-6.41a1.651 1.651 0 010-1.186A10.007 10.007 0 012.839 6.02L6.07 9.252a4 4 0 004.678 4.678z" />
      {/if}
    </svg>
  </button>
  
  <!-- More menu button -->
  <button
    onclick={toggleMoreMenu}
    class="icon-btn"
    class:active={$hudMoreMenuOpen}
    title="More Settings"
  >
    <!-- Sliders/adjustments icon -->
    <svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 20 20" fill="currentColor" class="icon">
      <path d="M17 4.25a.75.75 0 01-.75.75h-5.5a.75.75 0 010-1.5h5.5a.75.75 0 01.75.75zM17 10a.75.75 0 01-.75.75h-10.5a.75.75 0 010-1.5h10.5a.75.75 0 01.75.75zM17 15.75a.75.75 0 01-.75.75h-5.5a.75.75 0 010-1.5h5.5a.75.75 0 01.75.75zM4.25 5.5a1.25 1.25 0 100-2.5 1.25 1.25 0 000 2.5zM4.25 11.25a1.25 1.25 0 100-2.5 1.25 1.25 0 000 2.5zM4.25 17a1.25 1.25 0 100-2.5 1.25 1.25 0 000 2.5z" />
    </svg>
  </button>
</div>

<!-- More menu popover - rendered outside .viewer-hud to avoid backdrop-filter containing block issue -->
{#if $hudMoreMenuOpen}
  <ViewerHudMoreMenu />
{/if}

<style>
  .viewer-hud {
    position: absolute;
    top: 1rem;
    left: 1rem;
    z-index: 30;
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
    max-width: calc(100vw - 2rem);
  }

  .hud-divider {
    width: 1px;
    height: 1.25rem;
    background: rgba(255, 255, 255, 0.15);
    flex-shrink: 0;
  }

  .slider-control {
    display: flex;
    align-items: center;
    gap: 0.375rem;
    min-width: 0;
  }

  .slider-label {
    display: flex;
    align-items: center;
    color: #9ca3af;
    flex-shrink: 0;
  }

  .icon {
    width: 1rem;
    height: 1rem;
  }

  .slider {
    flex: 1;
    height: 4px;
    background: #374151;
    border-radius: 2px;
    appearance: none;
    min-width: 0;
    cursor: pointer;
    min-width: 80px;
  }

  .slider::-webkit-slider-thumb {
    appearance: none;
    width: 12px;
    height: 12px;
    background: #3b82f6;
    border-radius: 50%;
    cursor: pointer;
    transition: transform 0.1s;
  }

  .slider::-webkit-slider-thumb:hover {
    transform: scale(1.2);
  }

  .slider::-moz-range-thumb {
    width: 12px;
    height: 12px;
    background: #3b82f6;
    border: none;
    border-radius: 50%;
    cursor: pointer;
  }

  .slider-wrapper {
    position: relative;
    flex: 1;
    min-width: 0;
    display: flex;
    align-items: center;
  }

  .slider-tooltip {
    position: absolute;
    top: -28px;
    left: 50%;
    transform: translateX(-50%);
    background: #1f2937;
    color: #e5e7eb;
    padding: 0.25rem 0.5rem;
    border-radius: 0.25rem;
    font-size: 0.6875rem;
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
    pointer-events: none;
    box-shadow: 0 2px 8px rgba(0, 0, 0, 0.4);
    z-index: 10;
  }

  .slider-tooltip::after {
    content: '';
    position: absolute;
    top: 100%;
    left: 50%;
    transform: translateX(-50%);
    border: 4px solid transparent;
    border-top-color: #1f2937;
  }

  .stain-select {
    padding: 0.375rem 0.5rem;
    background: #374151;
    border: 1px solid #4b5563;
    border-radius: 0.375rem;
    color: #e5e7eb;
    font-size: 0.75rem;
    cursor: pointer;
    outline: none;
  }

  .stain-select:hover {
    border-color: #6b7280;
  }

  .stain-select:focus {
    border-color: #3b82f6;
    box-shadow: 0 0 0 2px rgba(59, 130, 246, 0.3);
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

    .slider {
      min-width: 60px;
    }
  }
</style>
