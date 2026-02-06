<script lang="ts">
  import { settings, hudMoreMenuOpen, type StainMode } from '$lib/stores/settings';
  import ViewerHudMoreMenu from './ViewerHudMoreMenu.svelte';

  interface Props {
    /** Current zoom level (0-1 range typically, displayed as percentage) */
    zoom: number;
    /** Callback to zoom in */
    onZoomIn: () => void;
    /** Callback to zoom out */
    onZoomOut: () => void;
    /** Callback to reset/fit view */
    onFitView: () => void;
    /** Current magnification (e.g., "10x", "40x") - optional */
    magnification?: string;
  }

  let { zoom, onZoomIn, onZoomOut, onFitView, magnification }: Props = $props();

  // Bind to settings store
  let brightness = $state($settings.image.brightness);
  let contrast = $state($settings.image.contrast);
  let stainMode = $state<StainMode>($settings.image.stainMode);
  let scaleBarVisible = $state($settings.image.scaleBarVisible);
  let annotationsVisible = $state($settings.annotations.visible);

  // Keep local state in sync with store
  $effect(() => {
    brightness = $settings.image.brightness;
    contrast = $settings.image.contrast;
    stainMode = $settings.image.stainMode;
    scaleBarVisible = $settings.image.scaleBarVisible;
    annotationsVisible = $settings.annotations.visible;
  });

  // Debounce for sliders
  let brightnessTimeout: ReturnType<typeof setTimeout> | null = null;
  let contrastTimeout: ReturnType<typeof setTimeout> | null = null;

  function handleBrightnessChange(e: Event) {
    const target = e.target as HTMLInputElement;
    brightness = parseFloat(target.value);
    
    if (brightnessTimeout) clearTimeout(brightnessTimeout);
    brightnessTimeout = setTimeout(() => {
      settings.setSetting('image', 'brightness', brightness);
    }, 50);
  }

  function handleContrastChange(e: Event) {
    const target = e.target as HTMLInputElement;
    contrast = parseFloat(target.value);
    
    if (contrastTimeout) clearTimeout(contrastTimeout);
    contrastTimeout = setTimeout(() => {
      settings.setSetting('image', 'contrast', contrast);
    }, 50);
  }

  function handleStainModeChange(e: Event) {
    const target = e.target as HTMLSelectElement;
    stainMode = target.value as StainMode;
    settings.setSetting('image', 'stainMode', stainMode);
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

  // Stain mode options
  const stainModes: { value: StainMode; label: string }[] = [
    { value: 'he', label: 'H&E' },
    { value: 'ihc_dab', label: 'IHC-DAB' },
    { value: 'ihc_hema', label: 'IHC-Hema' },
    { value: 'fluorescence', label: 'Fluor' },
    { value: 'gram', label: 'Gram' },
    { value: 'zn_afb', label: 'ZN/AFB' },
    { value: 'gms', label: 'GMS' },
  ];
</script>

<div class="viewer-hud">
  <!-- Image adjustments section -->
  <div class="hud-section">
    <div class="slider-control">
      <span class="slider-label" aria-hidden="true">
        <!-- Sun icon for brightness -->
        <svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 20 20" fill="currentColor" class="icon">
          <path d="M10 2a.75.75 0 01.75.75v1.5a.75.75 0 01-1.5 0v-1.5A.75.75 0 0110 2zM10 15a.75.75 0 01.75.75v1.5a.75.75 0 01-1.5 0v-1.5A.75.75 0 0110 15zM10 7a3 3 0 100 6 3 3 0 000-6zM15.657 5.404a.75.75 0 10-1.06-1.06l-1.061 1.06a.75.75 0 001.06 1.061l1.061-1.06zM6.464 14.596a.75.75 0 10-1.06-1.06l-1.06 1.06a.75.75 0 001.06 1.06l1.06-1.06zM18 10a.75.75 0 01-.75.75h-1.5a.75.75 0 010-1.5h1.5A.75.75 0 0118 10zM5 10a.75.75 0 01-.75.75h-1.5a.75.75 0 010-1.5h1.5A.75.75 0 015 10zM14.596 15.657a.75.75 0 001.06-1.06l-1.06-1.061a.75.75 0 10-1.06 1.06l1.06 1.06zM5.404 6.464a.75.75 0 001.06-1.06l-1.06-1.06a.75.75 0 10-1.061 1.06l1.06 1.06z" />
        </svg>
      </span>
      <input
        type="range"
        min="-100"
        max="100"
        step="1"
        value={brightness}
        oninput={handleBrightnessChange}
        class="slider"
        title="Brightness: {brightness}"
        aria-label="Brightness"
      />
      <span class="slider-value">{brightness}</span>
    </div>
    
    <div class="slider-control">
      <span class="slider-label" aria-hidden="true">
        <!-- Contrast icon (circle half-filled) -->
        <svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 20 20" fill="currentColor" class="icon">
          <path fill-rule="evenodd" d="M10 18a8 8 0 100-16 8 8 0 000 16zm0-2a6 6 0 100-12v12z" clip-rule="evenodd" />
        </svg>
      </span>
      <input
        type="range"
        min="-100"
        max="100"
        step="1"
        value={contrast}
        oninput={handleContrastChange}
        class="slider"
        title="Contrast: {contrast}"
        aria-label="Contrast"
      />
      <span class="slider-value">{contrast}</span>
    </div>
  </div>

  <!-- Stain mode selector -->
  <div class="hud-section">
    <select
      value={stainMode}
      onchange={handleStainModeChange}
      class="stain-select"
      title="Stain Mode"
    >
      {#each stainModes as mode}
        <option value={mode.value}>{mode.label}</option>
      {/each}
    </select>
  </div>

  <!-- Zoom controls -->
  <div class="hud-section zoom-controls">
    <button onclick={onZoomOut} class="icon-btn" title="Zoom Out">
      <svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 20 20" fill="currentColor" class="icon">
        <path fill-rule="evenodd" d="M3 10a.75.75 0 01.75-.75h12.5a.75.75 0 010 1.5H3.75A.75.75 0 013 10z" clip-rule="evenodd" />
      </svg>
    </button>
    <button onclick={onFitView} class="zoom-display" title="Fit to View">
      {zoomDisplay()}
      {#if magnification}
        <span class="magnification">({magnification})</span>
      {/if}
    </button>
    <button onclick={onZoomIn} class="icon-btn" title="Zoom In">
      <svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 20 20" fill="currentColor" class="icon">
        <path d="M10.75 4.75a.75.75 0 00-1.5 0v4.5h-4.5a.75.75 0 000 1.5h4.5v4.5a.75.75 0 001.5 0v-4.5h4.5a.75.75 0 000-1.5h-4.5v-4.5z" />
      </svg>
    </button>
  </div>

  <!-- Toggle buttons -->
  <div class="hud-section toggle-buttons">
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

  <!-- More menu popover -->
  {#if $hudMoreMenuOpen}
    <ViewerHudMoreMenu />
  {/if}
</div>

<style>
  .viewer-hud {
    position: absolute;
    top: 1rem;
    left: 1rem;
    z-index: 30;
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
    padding: 0.75rem;
    background: rgba(20, 20, 20, 0.85);
    backdrop-filter: blur(8px);
    border-radius: 0.75rem;
    border: 1px solid rgba(255, 255, 255, 0.1);
    box-shadow: 0 4px 16px rgba(0, 0, 0, 0.3);
    max-width: 280px;
  }

  .hud-section {
    display: flex;
    align-items: center;
    gap: 0.5rem;
  }

  .slider-control {
    display: flex;
    align-items: center;
    gap: 0.375rem;
    width: 100%;
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

  .slider-value {
    font-size: 0.6875rem;
    color: #9ca3af;
    min-width: 2rem;
    text-align: right;
    font-variant-numeric: tabular-nums;
  }

  .stain-select {
    flex: 1;
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

  .zoom-controls {
    justify-content: center;
    gap: 0.25rem;
  }

  .zoom-display {
    display: flex;
    align-items: center;
    gap: 0.25rem;
    padding: 0.375rem 0.625rem;
    background: #374151;
    border: 1px solid #4b5563;
    border-radius: 0.375rem;
    color: #e5e7eb;
    font-size: 0.75rem;
    font-weight: 500;
    cursor: pointer;
    transition: background-color 0.15s;
  }

  .zoom-display:hover {
    background: #4b5563;
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

  .toggle-buttons {
    justify-content: center;
  }

  /* Mobile adjustments */
  @media (max-width: 480px) {
    .viewer-hud {
      top: 0.5rem;
      left: 0.5rem;
      right: 0.5rem;
      max-width: none;
      padding: 0.5rem;
    }

    .slider {
      min-width: 60px;
    }
  }
</style>
