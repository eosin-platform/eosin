<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import { browser } from '$app/environment';
  import {
    settings,
    hudMoreMenuOpen,
    type SensitivityLevel,
    type MeasurementUnit,
    type StainEnhancementMode,
    type StainNormalization,
  } from '$lib/stores/settings';

  // Local state bound to settings
  let gamma = $state($settings.image.gamma);
  let measurementUnits = $state<MeasurementUnit>($settings.measurements.units);
  let zoomSensitivity = $state<SensitivityLevel>($settings.navigation.zoomSensitivity);
  let panSensitivity = $state<SensitivityLevel>($settings.navigation.panSensitivity);
  let minimapVisible = $state($settings.navigation.minimapVisible);
  let stainEnhancement = $state<StainEnhancementMode>($settings.image.stainEnhancement);
  let sharpeningIntensity = $state($settings.image.sharpeningIntensity);
  let stainNormalization = $state<StainNormalization>($settings.image.stainNormalization);

  // Keep local state in sync with store
  $effect(() => {
    gamma = $settings.image.gamma;
    measurementUnits = $settings.measurements.units;
    zoomSensitivity = $settings.navigation.zoomSensitivity;
    panSensitivity = $settings.navigation.panSensitivity;
    minimapVisible = $settings.navigation.minimapVisible;
    stainEnhancement = $settings.image.stainEnhancement;
    sharpeningIntensity = $settings.image.sharpeningIntensity;
    stainNormalization = $settings.image.stainNormalization;
  });

  let menuElement: HTMLDivElement;

  // Close menu on outside click or Escape
  function handleClickOutside(e: MouseEvent) {
    if (menuElement && !menuElement.contains(e.target as Node)) {
      // Check if click was on the "More" button (which handles its own toggle)
      const target = e.target as HTMLElement;
      if (!target.closest('[title="More Settings"]')) {
        hudMoreMenuOpen.set(false);
      }
    }
  }

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === 'Escape') {
      hudMoreMenuOpen.set(false);
    }
  }

  onMount(() => {
    if (browser) {
      document.addEventListener('click', handleClickOutside, true);
      document.addEventListener('keydown', handleKeydown);
    }
  });

  onDestroy(() => {
    if (browser) {
      document.removeEventListener('click', handleClickOutside, true);
      document.removeEventListener('keydown', handleKeydown);
    }
  });

  // Debounce for gamma slider
  let gammaTimeout: ReturnType<typeof setTimeout> | null = null;

  function handleGammaChange(e: Event) {
    const target = e.target as HTMLInputElement;
    gamma = parseFloat(target.value);
    
    if (gammaTimeout) clearTimeout(gammaTimeout);
    gammaTimeout = setTimeout(() => {
      settings.setSetting('image', 'gamma', gamma);
    }, 50);
  }

  function resetGamma() {
    gamma = 1.0;
    settings.setSetting('image', 'gamma', gamma);
  }

  function resetToDefaults() {
    // Get configurable defaults from settings
    const defaults = $settings.defaults;
    
    // Reset normalization
    stainNormalization = defaults.stainNormalization;
    settings.setSetting('image', 'stainNormalization', defaults.stainNormalization);
    
    // Reset stain enhancement
    stainEnhancement = defaults.stainEnhancement;
    settings.setSetting('image', 'stainEnhancement', defaults.stainEnhancement);
    
    // Reset sharpening
    sharpeningIntensity = defaults.sharpeningIntensity;
    settings.setSetting('image', 'sharpeningIntensity', defaults.sharpeningIntensity);
    settings.setSetting('image', 'sharpeningEnabled', defaults.sharpeningIntensity > 0);
    
    // Reset gamma
    gamma = defaults.gamma;
    settings.setSetting('image', 'gamma', defaults.gamma);
    
    // Reset brightness
    settings.setSetting('image', 'brightness', defaults.brightness);
    
    // Reset contrast
    settings.setSetting('image', 'contrast', defaults.contrast);
  }

  function handleUnitsChange(e: Event) {
    const target = e.target as HTMLSelectElement;
    measurementUnits = target.value as MeasurementUnit;
    settings.setSetting('measurements', 'units', measurementUnits);
  }

  function handleZoomSensitivityChange(value: SensitivityLevel) {
    zoomSensitivity = value;
    settings.setSetting('navigation', 'zoomSensitivity', value);
  }

  function handlePanSensitivityChange(value: SensitivityLevel) {
    panSensitivity = value;
    settings.setSetting('navigation', 'panSensitivity', value);
  }

  function toggleMinimap() {
    minimapVisible = !minimapVisible;
    settings.setSetting('navigation', 'minimapVisible', minimapVisible);
  }

  // Handle stain enhancement mode change
  function handleStainEnhancementChange(value: StainEnhancementMode) {
    stainEnhancement = value;
    settings.setSetting('image', 'stainEnhancement', value);
  }

  // Debounce for sharpening slider
  let sharpeningTimeout: ReturnType<typeof setTimeout> | null = null;

  function handleSharpeningChange(e: Event) {
    const target = e.target as HTMLInputElement;
    sharpeningIntensity = parseInt(target.value, 10);
    
    // Update sharpeningEnabled based on intensity
    const enabled = sharpeningIntensity > 0;
    
    if (sharpeningTimeout) clearTimeout(sharpeningTimeout);
    sharpeningTimeout = setTimeout(() => {
      settings.setSetting('image', 'sharpeningIntensity', sharpeningIntensity);
      settings.setSetting('image', 'sharpeningEnabled', enabled);
    }, 50);
  }

  // Handle stain normalization mode change
  function handleStainNormalizationChange(value: StainNormalization) {
    stainNormalization = value;
    settings.setSetting('image', 'stainNormalization', value);
  }

  const sensitivityOptions: SensitivityLevel[] = ['low', 'medium', 'high'];

  // Stain enhancement options for the segmented control
  const stainEnhancementOptions: { value: StainEnhancementMode; label: string; title: string }[] = [
    { value: 'none', label: 'None', title: 'No enhancement' },
    { value: 'gram', label: 'Gram', title: 'Enhance Gram stain (purple/pink bacteria)' },
    { value: 'afb', label: 'AFB', title: 'Enhance AFB/Ziehl-Neelsen (red bacilli)' },
    { value: 'gms', label: 'GMS', title: 'Enhance GMS (dark fungal elements)' },
  ];

  const unitOptions: { value: MeasurementUnit; label: string }[] = [
    { value: 'um', label: 'µm' },
    { value: 'mm', label: 'mm' },
    { value: 'in', label: 'in' },
  ];

  // Stain normalization options
  const stainNormalizationOptions: { value: StainNormalization; label: string; title: string }[] = [
    { value: 'none', label: 'None', title: 'No stain normalization' },
    { value: 'macenko', label: 'Macenko', title: 'Macenko stain normalization' },
    { value: 'vahadane', label: 'Vahadane', title: 'Vahadane stain normalization' },
  ];

  // Stop mouse/touch events from propagating to the viewer (prevents panning)
  function stopPropagation(e: Event) {
    e.stopPropagation();
  }

  function closeMenu() {
    hudMoreMenuOpen.set(false);
  }
</script>

<!-- Mobile overlay backdrop -->
<!-- svelte-ignore a11y_click_events_have_key_events -->
<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="mobile-overlay" onclick={closeMenu}></div>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div 
  class="more-menu" 
  bind:this={menuElement} 
  role="menu" 
  tabindex="-1"
  onmousedown={stopPropagation}
  ontouchstart={stopPropagation}
  onwheel={stopPropagation}
>
  <!-- Mobile header with close button -->
  <div class="mobile-header">
    <h2>Settings</h2>
    <button class="mobile-close" onclick={closeMenu} aria-label="Close settings">
      <svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 20 20" fill="currentColor">
        <path d="M6.28 5.22a.75.75 0 00-1.06 1.06L8.94 10l-3.72 3.72a.75.75 0 101.06 1.06L10 11.06l3.72 3.72a.75.75 0 101.06-1.06L11.06 10l3.72-3.72a.75.75 0 00-1.06-1.06L10 8.94 6.28 5.22z" />
      </svg>
    </button>
  </div>
  
  <div class="menu-content">
  <!-- Gamma slider -->
  <div class="menu-section">
    <span class="menu-label" id="gamma-label">Gamma</span>
    <div class="slider-row">
      <input
        type="range"
        min="0.1"
        max="3.0"
        step="0.1"
        value={gamma}
        oninput={handleGammaChange}
        ondblclick={resetGamma}
        class="slider"
        aria-labelledby="gamma-label"
      />
      <span class="slider-value">{gamma.toFixed(1)}</span>
    </div>
  </div>

  <!-- Sharpening slider (0 = disabled) -->
  <div class="menu-section">
    <span class="menu-label" id="sharpening-label">Sharpness</span>
    <div class="slider-row">
      <input
        type="range"
        min="0"
        max="100"
        step="1"
        value={sharpeningIntensity}
        oninput={handleSharpeningChange}
        class="slider"
        aria-labelledby="sharpening-label"
      />
      <span class="slider-value">{sharpeningIntensity === 0 ? 'Off' : sharpeningIntensity}</span>
    </div>
  </div>

  <!-- Stain Normalization -->
  <div class="menu-section">
    <span class="menu-label" id="stain-normalization-label">Stain Normalization</span>
    <div class="segmented-control" role="group" aria-labelledby="stain-normalization-label">
      {#each stainNormalizationOptions as opt}
        <button
          class="segment"
          class:active={stainNormalization === opt.value}
          onclick={() => handleStainNormalizationChange(opt.value)}
          title={opt.title}
        >
          {opt.label}
        </button>
      {/each}
    </div>
  </div>

  <!-- Stain Enhancement (post-processing) -->
  <div class="menu-section">
    <span class="menu-label" id="stain-enhancement-label">Stain Enhancement</span>
    <div class="segmented-control stain-enhancement-control" role="group" aria-labelledby="stain-enhancement-label">
      {#each stainEnhancementOptions as opt}
        <button
          class="segment"
          class:active={stainEnhancement === opt.value}
          onclick={() => handleStainEnhancementChange(opt.value)}
          title={opt.title}
        >
          {opt.label}
        </button>
      {/each}
    </div>
  </div>

  <!-- Measurement units -->
  <div class="menu-section">
    <span class="menu-label" id="units-label">Measurement Units</span>
    <select value={measurementUnits} onchange={handleUnitsChange} class="select-input" aria-labelledby="units-label">
      {#each unitOptions as opt}
        <option value={opt.value}>{opt.label}</option>
      {/each}
    </select>
  </div>

  <!-- Navigation sensitivity -->
  <div class="menu-section">
    <span class="menu-label" id="zoom-sens-label">Zoom Sensitivity</span>
    <div class="segmented-control" role="group" aria-labelledby="zoom-sens-label">
      {#each sensitivityOptions as opt}
        <button
          class="segment"
          class:active={zoomSensitivity === opt}
          onclick={() => handleZoomSensitivityChange(opt)}
        >
          {opt.charAt(0).toUpperCase() + opt.slice(1)}
        </button>
      {/each}
    </div>
  </div>

  <div class="menu-section">
    <span class="menu-label" id="pan-sens-label">Pan Sensitivity</span>
    <div class="segmented-control" role="group" aria-labelledby="pan-sens-label">
      {#each sensitivityOptions as opt}
        <button
          class="segment"
          class:active={panSensitivity === opt}
          onclick={() => handlePanSensitivityChange(opt)}
        >
          {opt.charAt(0).toUpperCase() + opt.slice(1)}
        </button>
      {/each}
    </div>
  </div>

  <!-- Minimap toggle -->
  <div class="menu-section row">
    <span class="menu-label" id="minimap-label">Mini-map</span>
    <button
      class="toggle-btn"
      class:active={minimapVisible}
      onclick={toggleMinimap}
      role="switch"
      aria-checked={minimapVisible}
      aria-labelledby="minimap-label"
    >
      <span class="toggle-track">
        <span class="toggle-thumb"></span>
      </span>
    </button>
  </div>

  <!-- Reset to Defaults button -->
  <div class="menu-section reset-section">
    <button class="reset-all-btn" onclick={resetToDefaults}>
      Reset to Defaults
    </button>
  </div>
  </div>
</div>

<style>
  /* Mobile overlay - only visible on small screens */
  .mobile-overlay {
    display: none;
  }

  /* Mobile header - only visible on small screens */
  .mobile-header {
    display: none;
  }

  .more-menu {
    position: fixed;
    top: calc(1rem + 48px + 0.5rem);
    left: 1rem;
    min-width: 240px;
    max-height: calc(100vh - 120px);
    overflow-y: auto;
    background: rgba(20, 20, 20, 0.95);
    backdrop-filter: blur(12px);
    border-radius: 0.75rem;
    border: 1px solid rgba(255, 255, 255, 0.1);
    box-shadow: 0 8px 32px rgba(0, 0, 0, 0.4);
    padding: 0.5rem;
    z-index: 40;
  }

  .menu-content {
    display: contents;
  }

  .menu-section {
    padding: 0.5rem;
  }

  .menu-section.row {
    display: flex;
    align-items: center;
    justify-content: space-between;
  }

  .menu-label {
    display: block;
    font-size: 0.6875rem;
    font-weight: 500;
    color: #9ca3af;
    margin-bottom: 0.375rem;
    text-transform: uppercase;
    letter-spacing: 0.03em;
  }

  .menu-section.row .menu-label {
    margin-bottom: 0;
  }

  .slider-row {
    display: flex;
    align-items: center;
    gap: 0.5rem;
  }

  .slider {
    flex: 1;
    height: 4px;
    background: #374151;
    border-radius: 2px;
    appearance: none;
    cursor: pointer;
  }

  .slider::-webkit-slider-thumb {
    appearance: none;
    width: 14px;
    height: 14px;
    background: #3b82f6;
    border-radius: 50%;
    cursor: pointer;
  }

  .slider::-moz-range-thumb {
    width: 14px;
    height: 14px;
    background: #3b82f6;
    border: none;
    border-radius: 50%;
    cursor: pointer;
  }

  .slider-value {
    font-size: 0.75rem;
    color: #e5e7eb;
    min-width: 2rem;
    text-align: right;
    font-variant-numeric: tabular-nums;
  }

  .select-input {
    width: 100%;
    padding: 0.5rem;
    background: #374151;
    border: 1px solid #4b5563;
    border-radius: 0.375rem;
    color: #e5e7eb;
    font-size: 0.8125rem;
    cursor: pointer;
    outline: none;
  }

  .select-input:hover {
    border-color: #6b7280;
  }

  .select-input:focus {
    border-color: #3b82f6;
  }

  .segmented-control {
    display: flex;
    background: #1f2937;
    border-radius: 0.375rem;
    padding: 2px;
    gap: 2px;
  }

  .segment {
    flex: 1;
    padding: 0.375rem 0.5rem;
    background: transparent;
    border: none;
    border-radius: 0.25rem;
    color: #9ca3af;
    font-size: 0.6875rem;
    font-weight: 500;
    cursor: pointer;
    transition: all 0.15s;
  }

  .segment:hover {
    color: #e5e7eb;
    background: rgba(255, 255, 255, 0.05);
  }

  .segment.active {
    background: #3b82f6;
    color: white;
  }

  .theme-control .segment {
    font-size: 0.875rem;
  }

  /* Stain enhancement control - 4 buttons need slightly smaller padding */
  .stain-enhancement-control .segment {
    padding: 0.375rem 0.25rem;
    font-size: 0.625rem;
  }

  .toggle-btn {
    display: flex;
    align-items: center;
    padding: 0;
    background: none;
    border: none;
    cursor: pointer;
  }

  .toggle-track {
    position: relative;
    width: 36px;
    height: 20px;
    background: #374151;
    border-radius: 10px;
    transition: background-color 0.2s;
  }

  .toggle-btn.active .toggle-track {
    background: #3b82f6;
  }

  .toggle-thumb {
    position: absolute;
    top: 2px;
    left: 2px;
    width: 16px;
    height: 16px;
    background: white;
    border-radius: 50%;
    transition: transform 0.2s;
  }

  .toggle-btn.active .toggle-thumb {
    transform: translateX(16px);
  }

  /* Reset All button */
  .reset-section {
    border-top: 1px solid rgba(255, 255, 255, 0.1);
    margin-top: 0.25rem;
    padding-top: 0.75rem;
  }

  .reset-all-btn {
    width: 100%;
    padding: 0.5rem 1rem;
    background: rgba(239, 68, 68, 0.15);
    border: 1px solid rgba(239, 68, 68, 0.3);
    border-radius: 0.5rem;
    color: #fca5a5;
    font-size: 0.75rem;
    font-weight: 500;
    cursor: pointer;
    transition: all 0.15s;
  }

  .reset-all-btn:hover {
    background: rgba(239, 68, 68, 0.25);
    border-color: rgba(239, 68, 68, 0.5);
    color: #fecaca;
  }

  .reset-all-btn:active {
    background: rgba(239, 68, 68, 0.35);
  }

  /* Ensure menu doesn't overlap minimap (bottom-right corner) */
  @media (max-width: 600px) {
    /* Show overlay backdrop on mobile */
    .mobile-overlay {
      display: block;
      position: fixed;
      inset: 0;
      background: rgba(0, 0, 0, 0.6);
      backdrop-filter: blur(4px);
      z-index: 999;
    }

    /* Show mobile header */
    .mobile-header {
      display: flex;
      align-items: center;
      justify-content: space-between;
      padding: 16px 20px;
      border-bottom: 1px solid rgba(255, 255, 255, 0.1);
      flex-shrink: 0;
    }

    .mobile-header h2 {
      margin: 0;
      font-size: 18px;
      font-weight: 600;
      color: #fff;
    }

    .mobile-close {
      display: flex;
      align-items: center;
      justify-content: center;
      width: 32px;
      height: 32px;
      background: rgba(255, 255, 255, 0.1);
      border: none;
      border-radius: 6px;
      cursor: pointer;
      color: rgba(255, 255, 255, 0.7);
      transition: background 0.15s, color 0.15s;
    }

    .mobile-close:hover {
      background: rgba(255, 255, 255, 0.2);
      color: #fff;
    }

    .mobile-close svg {
      width: 18px;
      height: 18px;
    }

    /* Full-screen modal on mobile */
    .more-menu {
      position: fixed;
      top: 0;
      left: 0;
      right: 0;
      bottom: 0;
      width: 100%;
      height: 100%;
      height: 100dvh;
      margin: 0;
      min-width: unset;
      max-height: unset;
      border-radius: 0;
      border: none;
      z-index: 1000;
      display: flex;
      flex-direction: column;
      padding: 0;
      overflow: hidden;
      background: rgba(20, 20, 20, 0.98);
    }

    /* Scrollable content area */
    .menu-content {
      display: flex;
      flex-direction: column;
      flex: 1 1 0%;
      min-height: 0;
      overflow-y: auto;
      overflow-x: hidden;
      -webkit-overflow-scrolling: touch;
      padding: 0.5rem 0;
    }

    /* Menu sections need padding inside scrollable area */
    .menu-section {
      padding: 0.75rem 1rem;
      flex-shrink: 0;
    }

    .reset-section {
      margin-top: 0.5rem;
      padding: 1rem;
      border-top: 1px solid rgba(255, 255, 255, 0.1);
      flex-shrink: 0;
    }
  }
</style>
