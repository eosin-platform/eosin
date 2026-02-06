<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import { browser } from '$app/environment';
  import {
    settings,
    hudMoreMenuOpen,
    type SensitivityLevel,
    type MeasurementUnit,
    type ThemeMode,
    type StainMode,
    type StainEnhancementMode,
  } from '$lib/stores/settings';

  // Local state bound to settings
  let gamma = $state($settings.image.gamma);
  let measurementUnits = $state<MeasurementUnit>($settings.measurements.units);
  let zoomSensitivity = $state<SensitivityLevel>($settings.navigation.zoomSensitivity);
  let panSensitivity = $state<SensitivityLevel>($settings.navigation.panSensitivity);
  let minimapVisible = $state($settings.navigation.minimapVisible);
  let theme = $state<ThemeMode>($settings.ui.theme);
  let stainEnhancement = $state<StainEnhancementMode>($settings.image.stainEnhancement);

  // Keep local state in sync with store
  $effect(() => {
    gamma = $settings.image.gamma;
    measurementUnits = $settings.measurements.units;
    zoomSensitivity = $settings.navigation.zoomSensitivity;
    panSensitivity = $settings.navigation.panSensitivity;
    minimapVisible = $settings.navigation.minimapVisible;
    theme = $settings.ui.theme;
    stainEnhancement = $settings.image.stainEnhancement;
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

  function handleThemeChange(value: ThemeMode) {
    theme = value;
    settings.setSetting('ui', 'theme', value);
  }

  // Handle stain enhancement mode change
  function handleStainEnhancementChange(value: StainEnhancementMode) {
    stainEnhancement = value;
    settings.setSetting('image', 'stainEnhancement', value);
  }

  // Apply stain enhancement preset
  function applyStainPreset(preset: StainMode) {
    settings.setSetting('image', 'stainMode', preset);
    hudMoreMenuOpen.set(false);
  }

  const sensitivityOptions: SensitivityLevel[] = ['low', 'medium', 'high'];
  const themeOptions: { value: ThemeMode; label: string }[] = [
    { value: 'light', label: '☀️' },
    { value: 'dark', label: '🌙' },
    { value: 'high_contrast', label: '◐' },
  ];

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

  // Stop mouse/touch events from propagating to the viewer (prevents panning)
  function stopPropagation(e: Event) {
    e.stopPropagation();
  }
</script>

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
  <div class="menu-header">
    <span>More Settings</span>
  </div>

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
        class="slider"
        aria-labelledby="gamma-label"
      />
      <span class="slider-value">{gamma.toFixed(1)}</span>
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

  <!-- Stain enhancement presets -->
  <div class="menu-section">
    <span class="menu-label" id="stain-presets-label">Stain Enhancements</span>
    <div class="preset-grid" role="group" aria-labelledby="stain-presets-label">
      <button class="preset-btn" onclick={() => applyStainPreset('gram')} title="Gram Stain Enhancement">
        Gram
      </button>
      <button class="preset-btn" onclick={() => applyStainPreset('zn_afb')} title="AFB/ZN Enhancement">
        AFB
      </button>
      <button class="preset-btn" onclick={() => applyStainPreset('gms')} title="GMS/Fungal Enhancement">
        GMS
      </button>
    </div>
  </div>

  <!-- Stain Enhancement Mode (post-processing) -->
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

  <!-- Theme toggle -->
  <div class="menu-section">
    <span class="menu-label" id="theme-label">Theme</span>
    <div class="segmented-control theme-control" role="group" aria-labelledby="theme-label">
      {#each themeOptions as opt}
        <button
          class="segment"
          class:active={theme === opt.value}
          onclick={() => handleThemeChange(opt.value)}
          title={opt.value.replace('_', ' ')}
        >
          {opt.label}
        </button>
      {/each}
    </div>
  </div>
</div>

<style>
  .more-menu {
    position: absolute;
    top: 100%;
    left: 0;
    margin-top: 0.5rem;
    min-width: 240px;
    background: rgba(20, 20, 20, 0.95);
    backdrop-filter: blur(12px);
    border-radius: 0.75rem;
    border: 1px solid rgba(255, 255, 255, 0.1);
    box-shadow: 0 8px 32px rgba(0, 0, 0, 0.4);
    padding: 0.5rem;
    z-index: 40;
  }

  .menu-header {
    padding: 0.5rem;
    font-size: 0.75rem;
    font-weight: 600;
    color: #9ca3af;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    border-bottom: 1px solid rgba(255, 255, 255, 0.1);
    margin-bottom: 0.5rem;
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

  .preset-grid {
    display: grid;
    grid-template-columns: repeat(3, 1fr);
    gap: 0.375rem;
  }

  .preset-btn {
    padding: 0.5rem 0.375rem;
    background: #374151;
    border: 1px solid #4b5563;
    border-radius: 0.375rem;
    color: #e5e7eb;
    font-size: 0.6875rem;
    font-weight: 500;
    cursor: pointer;
    transition: all 0.15s;
  }

  .preset-btn:hover {
    background: #4b5563;
    border-color: #6b7280;
  }

  /* Ensure menu doesn't overlap minimap (bottom-right corner) */
  @media (max-width: 480px) {
    .more-menu {
      position: fixed;
      top: auto;
      bottom: 1rem;
      left: 0.5rem;
      right: 0.5rem;
      max-height: 60vh;
      overflow-y: auto;
    }
  }
</style>
