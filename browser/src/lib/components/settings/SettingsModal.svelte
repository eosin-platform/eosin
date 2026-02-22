<script lang="ts">
	import { onMount, onDestroy } from 'svelte';
	import { browser } from '$app/environment';
	import {
		settings,
		settingsModalOpen,
		DEFAULT_SETTINGS,
		DEFAULT_COLOR_PALETTE,
		FACTORY_IMAGE_DEFAULTS,
		type ColorProfile,
		type StainNormalization,
		type StainEnhancementMode,
		type ImageDefaults,
		type PrefetchLevel,
		type StreamingQuality
	} from '$lib/stores/settings';
	import { clearNormalizationCache } from '$lib/frusta';
	import { authStore } from '$lib/stores/auth';
	import { logout } from '$lib/auth/client';

	type TabId =
		| 'image'
		| 'performance'
		| 'annotations'
		| 'privacy'
		| 'behavior'
		| 'defaults'
		| 'account'
		| 'about';
	type ResettableTabId = Exclude<TabId, 'about' | 'account'>;

	let activeTab = $state<TabId>('image');
	let dialogElement: HTMLDivElement;

	// Image settings
	let colorProfile = $state<ColorProfile>($settings.image.colorProfile);
	let sharpeningEnabled = $state($settings.image.sharpeningEnabled);
	let sharpeningIntensity = $state($settings.image.sharpeningIntensity);
	let stainNormalization = $state<StainNormalization>($settings.image.stainNormalization);

	// Performance settings
	let tileCacheSizeMb = $state($settings.performance.tileCacheSizeMb);
	let prefetchLevel = $state<PrefetchLevel>($settings.performance.prefetchLevel);
	let streamingQuality = $state<StreamingQuality>($settings.performance.streamingQuality);
	let hardwareAccelerationEnabled = $state($settings.performance.hardwareAccelerationEnabled);
	let undoBufferSize = $state($settings.performance.undoBufferSize);

	// Annotation settings
	let showLabels = $state($settings.annotations.showLabels);
	let autoClosePolygons = $state($settings.annotations.autoClosePolygons);
	let defaultColorPalette = $state<string[]>([...$settings.annotations.defaultColorPalette]);

	// Privacy settings
	let phiMaskingEnabled = $state($settings.privacy.phiMaskingEnabled);
	let screenshotsDisabled = $state($settings.privacy.screenshotsDisabled);
	let autoLogoutMinutes = $state($settings.privacy.autoLogoutMinutes);

	// Behavior settings
	let smoothNavigation = $state($settings.behavior.smoothNavigation);

	// Defaults settings (configurable image defaults)
	let defaultBrightness = $state($settings.defaults.brightness);
	let defaultContrast = $state($settings.defaults.contrast);
	let defaultGamma = $state($settings.defaults.gamma);
	let defaultSharpeningIntensity = $state($settings.defaults.sharpeningIntensity);
	let defaultStainEnhancement = $state<StainEnhancementMode>($settings.defaults.stainEnhancement);
	let defaultStainNormalization = $state<StainNormalization>($settings.defaults.stainNormalization);

	// Keep local state in sync with store
	$effect(() => {
		colorProfile = $settings.image.colorProfile;
		sharpeningEnabled = $settings.image.sharpeningEnabled;
		sharpeningIntensity = $settings.image.sharpeningIntensity;
		stainNormalization = $settings.image.stainNormalization;
		tileCacheSizeMb = $settings.performance.tileCacheSizeMb;
		prefetchLevel = $settings.performance.prefetchLevel;
		streamingQuality = $settings.performance.streamingQuality;
		hardwareAccelerationEnabled = $settings.performance.hardwareAccelerationEnabled;
		undoBufferSize = $settings.performance.undoBufferSize;
		showLabels = $settings.annotations.showLabels;
		autoClosePolygons = $settings.annotations.autoClosePolygons;
		defaultColorPalette = [...$settings.annotations.defaultColorPalette];
		phiMaskingEnabled = $settings.privacy.phiMaskingEnabled;
		screenshotsDisabled = $settings.privacy.screenshotsDisabled;
		autoLogoutMinutes = $settings.privacy.autoLogoutMinutes;
		smoothNavigation = $settings.behavior.smoothNavigation;
		defaultBrightness = $settings.defaults.brightness;
		defaultContrast = $settings.defaults.contrast;
		defaultGamma = $settings.defaults.gamma;
		defaultSharpeningIntensity = $settings.defaults.sharpeningIntensity;
		defaultStainEnhancement = $settings.defaults.stainEnhancement;
		defaultStainNormalization = $settings.defaults.stainNormalization;
	});

	function handleKeydown(e: KeyboardEvent) {
		if (e.key === 'Escape') {
			closeModal();
		}
	}

	function handleBackdropClick(e: MouseEvent) {
		if (e.target === e.currentTarget) {
			closeModal();
		}
	}

	function closeModal() {
		settingsModalOpen.set(false);
	}

	function resetToDefaults() {
		if (confirm('Are you sure you want to reset all settings to defaults?')) {
			settings.resetToDefaults();
		}
	}

	function resetSection(section: ResettableTabId) {
		const sectionMap: Record<ResettableTabId, keyof typeof DEFAULT_SETTINGS> = {
			image: 'image',
			performance: 'performance',
			annotations: 'annotations',
			privacy: 'privacy',
			behavior: 'behavior',
			defaults: 'defaults'
		};
		settings.resetSection(sectionMap[section]);
	}

	onMount(() => {
		if (browser) {
			document.addEventListener('keydown', handleKeydown);
			// Trap focus
			dialogElement?.focus();
		}
	});

	onDestroy(() => {
		if (browser) {
			document.removeEventListener('keydown', handleKeydown);
		}
	});

	// --- Image tab handlers ---
	function handleColorProfileChange(e: Event) {
		colorProfile = (e.target as HTMLSelectElement).value as ColorProfile;
		settings.setSetting('image', 'colorProfile', colorProfile);
	}

	function toggleSharpening() {
		sharpeningEnabled = !sharpeningEnabled;
		settings.setSetting('image', 'sharpeningEnabled', sharpeningEnabled);
	}

	function handleSharpeningIntensityChange(e: Event) {
		sharpeningIntensity = parseInt((e.target as HTMLInputElement).value);
		settings.setSetting('image', 'sharpeningIntensity', sharpeningIntensity);
	}

	function handleStainNormalizationChange(value: StainNormalization) {
		// Clear cached parameters when changing modes to allow fresh computation
		// This helps recover from potentially bad parameters
		clearNormalizationCache();
		stainNormalization = value;
		settings.setSetting('image', 'stainNormalization', value);
	}

	// --- Performance tab handlers ---
	function handleCacheSizeChange(e: Event) {
		tileCacheSizeMb = parseInt((e.target as HTMLInputElement).value);
		settings.setSetting('performance', 'tileCacheSizeMb', tileCacheSizeMb);
	}

	function handlePrefetchLevelChange(e: Event) {
		prefetchLevel = (e.target as HTMLSelectElement).value as PrefetchLevel;
		settings.setSetting('performance', 'prefetchLevel', prefetchLevel);
	}

	function handleStreamingQualityChange(e: Event) {
		streamingQuality = (e.target as HTMLSelectElement).value as StreamingQuality;
		settings.setSetting('performance', 'streamingQuality', streamingQuality);
	}

	function toggleHardwareAcceleration() {
		hardwareAccelerationEnabled = !hardwareAccelerationEnabled;
		settings.setSetting('performance', 'hardwareAccelerationEnabled', hardwareAccelerationEnabled);
	}

	function handleUndoBufferSizeChange(e: Event) {
		undoBufferSize = parseInt((e.target as HTMLInputElement).value);
		settings.setSetting('performance', 'undoBufferSize', undoBufferSize);
	}

	// --- Annotation tab handlers ---
	function toggleShowLabels() {
		showLabels = !showLabels;
		settings.setSetting('annotations', 'showLabels', showLabels);
	}

	function toggleAutoClosePolygons() {
		autoClosePolygons = !autoClosePolygons;
		settings.setSetting('annotations', 'autoClosePolygons', autoClosePolygons);
	}

	function handleColorChange(index: number, e: Event) {
		const newColor = (e.target as HTMLInputElement).value;
		defaultColorPalette[index] = newColor;
		settings.setSetting('annotations', 'defaultColorPalette', [...defaultColorPalette]);
	}

	function resetColorPalette() {
		defaultColorPalette = [...DEFAULT_COLOR_PALETTE];
		settings.setSetting('annotations', 'defaultColorPalette', [...DEFAULT_COLOR_PALETTE]);
	}

	// --- Privacy tab handlers ---
	function togglePhiMasking() {
		phiMaskingEnabled = !phiMaskingEnabled;
		settings.setSetting('privacy', 'phiMaskingEnabled', phiMaskingEnabled);
	}

	function toggleScreenshotsDisabled() {
		screenshotsDisabled = !screenshotsDisabled;
		settings.setSetting('privacy', 'screenshotsDisabled', screenshotsDisabled);
	}

	function handleAutoLogoutChange(e: Event) {
		autoLogoutMinutes = parseInt((e.target as HTMLSelectElement).value);
		settings.setSetting('privacy', 'autoLogoutMinutes', autoLogoutMinutes);
	}

	// --- Defaults tab handlers ---
	function handleDefaultBrightnessChange(e: Event) {
		defaultBrightness = parseInt((e.target as HTMLInputElement).value);
		settings.setSetting('defaults', 'brightness', defaultBrightness);
	}

	function handleDefaultContrastChange(e: Event) {
		defaultContrast = parseInt((e.target as HTMLInputElement).value);
		settings.setSetting('defaults', 'contrast', defaultContrast);
	}

	function handleDefaultGammaChange(e: Event) {
		defaultGamma = parseFloat((e.target as HTMLInputElement).value);
		settings.setSetting('defaults', 'gamma', defaultGamma);
	}

	function handleDefaultSharpeningChange(e: Event) {
		defaultSharpeningIntensity = parseInt((e.target as HTMLInputElement).value);
		settings.setSetting('defaults', 'sharpeningIntensity', defaultSharpeningIntensity);
	}

	function handleDefaultEnhancementChange(value: StainEnhancementMode) {
		defaultStainEnhancement = value;
		settings.setSetting('defaults', 'stainEnhancement', value);
	}

	function handleDefaultNormalizationChange(value: StainNormalization) {
		defaultStainNormalization = value;
		settings.setSetting('defaults', 'stainNormalization', value);
	}

	function resetDefaultsToFactory() {
		defaultBrightness = FACTORY_IMAGE_DEFAULTS.brightness;
		defaultContrast = FACTORY_IMAGE_DEFAULTS.contrast;
		defaultGamma = FACTORY_IMAGE_DEFAULTS.gamma;
		defaultSharpeningIntensity = FACTORY_IMAGE_DEFAULTS.sharpeningIntensity;
		defaultStainEnhancement = FACTORY_IMAGE_DEFAULTS.stainEnhancement;
		defaultStainNormalization = FACTORY_IMAGE_DEFAULTS.stainNormalization;
		settings.updateSection('defaults', { ...FACTORY_IMAGE_DEFAULTS });
	}

	const baseTabs: { id: TabId; label: string; icon: string }[] = [
		{ id: 'image', label: 'Image', icon: '🎨' },
		{ id: 'performance', label: 'Performance', icon: '⚡' },
		{ id: 'annotations', label: 'Annotations', icon: '✏️' },
		{ id: 'privacy', label: 'Privacy', icon: '🔒' },
		{ id: 'behavior', label: 'Behavior', icon: '🎯' },
		{ id: 'defaults', label: 'Defaults', icon: '↺' },
		{ id: 'about', label: 'About', icon: 'ℹ️' }
	];

	const accountTab = { id: 'account' as TabId, label: 'Account', icon: '👤' };

	// Show Account tab only when logged in
	let tabs = $derived(
		$authStore.user
			? [...baseTabs.slice(0, -1), accountTab, baseTabs[baseTabs.length - 1]]
			: baseTabs
	);

	function handleLogout() {
		logout();
		closeModal();
	}

	const stainEnhancementOptions: { value: StainEnhancementMode; label: string }[] = [
		{ value: 'none', label: 'None' },
		{ value: 'gram', label: 'Gram' },
		{ value: 'afb', label: 'AFB' },
		{ value: 'gms', label: 'GMS' }
	];

	const colorProfileOptions: { value: ColorProfile; label: string }[] = [
		{ value: 'srgb', label: 'sRGB (Standard)' },
		{ value: 'scanner_native', label: 'Scanner Native' },
		{ value: 'he_clinical', label: 'H&E Clinical' }
	];

	const stainNormOptions: { value: StainNormalization; label: string }[] = [
		{ value: 'none', label: 'None' },
		{ value: 'macenko', label: 'Macenko' },
		{ value: 'vahadane', label: 'Vahadane' }
	];

	const prefetchOptions: { value: PrefetchLevel; label: string }[] = [
		{ value: 'low', label: 'Low' },
		{ value: 'medium', label: 'Medium' },
		{ value: 'high', label: 'High' },
		{ value: 'ludicrous', label: 'Ludicrous' }
	];

	const qualityOptions: { value: StreamingQuality; label: string }[] = [
		{ value: 'auto', label: 'Auto' },
		{ value: 'full_res', label: 'Full Resolution' },
		{ value: 'low_res', label: 'Low Resolution' }
	];

	const logoutOptions = [
		{ value: 5, label: '5 minutes' },
		{ value: 15, label: '15 minutes' },
		{ value: 30, label: '30 minutes' },
		{ value: 60, label: '1 hour' },
		{ value: 0, label: 'Never' }
	];
</script>

<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<div
	class="modal-backdrop"
	onclick={handleBackdropClick}
	onkeydown={handleKeydown}
	role="presentation"
>
	<div
		class="modal-dialog"
		bind:this={dialogElement}
		tabindex="-1"
		role="dialog"
		aria-modal="true"
		aria-labelledby="settings-title"
	>
		<!-- Header -->
		<header class="modal-header">
			<h2 id="settings-title">Settings</h2>
			<button class="close-btn" onclick={closeModal} title="Close" aria-label="Close settings">
				<svg
					xmlns="http://www.w3.org/2000/svg"
					viewBox="0 0 20 20"
					fill="currentColor"
					class="icon"
				>
					<path
						d="M6.28 5.22a.75.75 0 00-1.06 1.06L8.94 10l-3.72 3.72a.75.75 0 101.06 1.06L10 11.06l3.72 3.72a.75.75 0 101.06-1.06L11.06 10l3.72-3.72a.75.75 0 00-1.06-1.06L10 8.94 6.28 5.22z"
					/>
				</svg>
			</button>
		</header>

		<!-- Content -->
		<div class="modal-body">
			<!-- Tab navigation -->
			<div class="tab-nav" role="tablist">
				{#each tabs as tab}
					<button
						class="tab-btn"
						class:active={activeTab === tab.id}
						onclick={() => (activeTab = tab.id)}
						role="tab"
						aria-selected={activeTab === tab.id}
						aria-controls={`panel-${tab.id}`}
					>
						<span class="tab-icon">{tab.icon}</span>
						<span class="tab-label">{tab.label}</span>
					</button>
				{/each}
			</div>

			<!-- Tab panels -->
			<div class="tab-content">
				<!-- Image / Rendering -->
				{#if activeTab === 'image'}
					<div class="panel" id="panel-image" role="tabpanel">
						<div class="setting-group">
							<h3>Color Profile</h3>
							<select value={colorProfile} onchange={handleColorProfileChange} class="select-input">
								{#each colorProfileOptions as opt}
									<option value={opt.value}>{opt.label}</option>
								{/each}
							</select>
						</div>

						<div class="setting-group">
							<h3>Sharpening</h3>
							<div class="toggle-row">
								<span id="sharpening-label">Enable Sharpening</span>
								<button
									class="toggle-btn"
									class:active={sharpeningEnabled}
									onclick={toggleSharpening}
									role="switch"
									aria-checked={sharpeningEnabled}
									aria-labelledby="sharpening-label"
								>
									<span class="toggle-track">
										<span class="toggle-thumb"></span>
									</span>
								</button>
							</div>
							{#if sharpeningEnabled}
								<div class="slider-row">
									<span class="slider-label">Intensity</span>
									<input
										type="range"
										min="0"
										max="100"
										value={sharpeningIntensity}
										oninput={handleSharpeningIntensityChange}
										class="slider"
									/>
									<span class="slider-value">{sharpeningIntensity}%</span>
								</div>
							{/if}
						</div>

						<div class="setting-group">
							<h3>Stain Normalization</h3>
							<div class="radio-group">
								{#each stainNormOptions as opt}
									<label class="radio-label">
										<input
											type="radio"
											name="stainNorm"
											value={opt.value}
											checked={stainNormalization === opt.value}
											onchange={() => handleStainNormalizationChange(opt.value)}
										/>
										<span>{opt.label}</span>
									</label>
								{/each}
							</div>
						</div>

						<button class="reset-btn" onclick={() => resetSection('image')}>
							Reset Image Settings
						</button>
					</div>
				{/if}

				<!-- Performance -->
				{#if activeTab === 'performance'}
					<div class="panel" id="panel-performance" role="tabpanel">
						<div class="setting-group">
							<h3>Tile Cache Size</h3>
							<div class="slider-row">
								<input
									type="range"
									min="128"
									max="2048"
									step="128"
									value={tileCacheSizeMb}
									oninput={handleCacheSizeChange}
									class="slider"
								/>
								<span class="slider-value">{tileCacheSizeMb} MB</span>
							</div>
							<p class="setting-hint">
								Higher values use more memory but improve performance when navigating.
							</p>
						</div>

						<div class="setting-group">
							<h3>Prefetch Aggressiveness</h3>
							<select
								value={prefetchLevel}
								onchange={handlePrefetchLevelChange}
								class="select-input"
							>
								{#each prefetchOptions as opt}
									<option value={opt.value}>{opt.label}</option>
								{/each}
							</select>
							<p class="setting-hint">
								Higher values load more tiles ahead of time, using more bandwidth.
							</p>
						</div>

						<div class="setting-group">
							<h3>Streaming Quality</h3>
							<select
								value={streamingQuality}
								onchange={handleStreamingQualityChange}
								class="select-input"
							>
								{#each qualityOptions as opt}
									<option value={opt.value}>{opt.label}</option>
								{/each}
							</select>
						</div>

						<div class="setting-group">
							<div class="toggle-row">
								<span id="hw-accel-label">Hardware Acceleration</span>
								<button
									class="toggle-btn"
									class:active={hardwareAccelerationEnabled}
									onclick={toggleHardwareAcceleration}
									role="switch"
									aria-checked={hardwareAccelerationEnabled}
									aria-labelledby="hw-accel-label"
								>
									<span class="toggle-track">
										<span class="toggle-thumb"></span>
									</span>
								</button>
							</div>
							<p class="setting-hint">
								Uses GPU for rendering when available. Disable if you experience visual glitches.
							</p>
						</div>

						<div class="setting-group">
							<h3>Undo Buffer Size</h3>
							<div class="slider-row">
								<input
									type="range"
									min="10"
									max="200"
									step="10"
									value={undoBufferSize}
									oninput={handleUndoBufferSizeChange}
									class="slider"
								/>
								<span class="slider-value">{undoBufferSize} steps</span>
							</div>
							<p class="setting-hint">
								Maximum number of undo steps to keep. Higher values use more memory.
							</p>
						</div>

						<button class="reset-btn" onclick={() => resetSection('performance')}>
							Reset Performance Settings
						</button>
					</div>
				{/if}

				<!-- Annotations -->
				{#if activeTab === 'annotations'}
					<div class="panel" id="panel-annotations" role="tabpanel">
						<div class="setting-group">
							<div class="toggle-row">
								<span id="show-labels-label">Show Labels</span>
								<button
									class="toggle-btn"
									class:active={showLabels}
									onclick={toggleShowLabels}
									role="switch"
									aria-checked={showLabels}
									aria-labelledby="show-labels-label"
								>
									<span class="toggle-track">
										<span class="toggle-thumb"></span>
									</span>
								</button>
							</div>
						</div>

						<div class="setting-group">
							<div class="toggle-row">
								<span id="auto-close-label">Auto-close Polygons</span>
								<button
									class="toggle-btn"
									class:active={autoClosePolygons}
									onclick={toggleAutoClosePolygons}
									role="switch"
									aria-checked={autoClosePolygons}
									aria-labelledby="auto-close-label"
								>
									<span class="toggle-track">
										<span class="toggle-thumb"></span>
									</span>
								</button>
							</div>
							<p class="setting-hint">
								Automatically close polygon annotations when clicking near the starting point.
							</p>
						</div>

						<div class="setting-group">
							<h3>Default Color Palette</h3>
							<div class="color-palette">
								{#each defaultColorPalette as color, i}
									<label class="color-swatch">
										<input type="color" value={color} onchange={(e) => handleColorChange(i, e)} />
										<span class="swatch-preview" style="background-color: {color}"></span>
									</label>
								{/each}
							</div>
							<button class="text-btn" onclick={resetColorPalette}>
								Reset to default colors
							</button>
						</div>

						<button class="reset-btn" onclick={() => resetSection('annotations')}>
							Reset Annotation Settings
						</button>
					</div>
				{/if}

				<!-- Privacy / Compliance -->
				{#if activeTab === 'privacy'}
					<div class="panel" id="panel-privacy" role="tabpanel">
						<div class="setting-group">
							<div class="toggle-row">
								<span id="phi-masking-label">PHI Masking</span>
								<button
									class="toggle-btn"
									class:active={phiMaskingEnabled}
									onclick={togglePhiMasking}
									role="switch"
									aria-checked={phiMaskingEnabled}
									aria-labelledby="phi-masking-label"
								>
									<span class="toggle-track">
										<span class="toggle-thumb"></span>
									</span>
								</button>
							</div>
							<p class="setting-hint">
								Automatically mask protected health information in slide labels and metadata.
							</p>
						</div>

						<div class="setting-group">
							<div class="toggle-row">
								<span id="screenshots-label">Disable Screenshots</span>
								<button
									class="toggle-btn"
									class:active={screenshotsDisabled}
									onclick={toggleScreenshotsDisabled}
									role="switch"
									aria-checked={screenshotsDisabled}
									aria-labelledby="screenshots-label"
								>
									<span class="toggle-track">
										<span class="toggle-thumb"></span>
									</span>
								</button>
							</div>
							<p class="setting-hint">
								Prevents screenshot functionality to protect sensitive images.
							</p>
						</div>

						<div class="setting-group">
							<h3>Auto-Logout Timeout</h3>
							<select
								value={autoLogoutMinutes}
								onchange={handleAutoLogoutChange}
								class="select-input"
							>
								{#each logoutOptions as opt}
									<option value={opt.value}>{opt.label}</option>
								{/each}
							</select>
							<p class="setting-hint">Automatically log out after period of inactivity.</p>
						</div>

						<button class="reset-btn" onclick={() => resetSection('privacy')}>
							Reset Privacy Settings
						</button>
					</div>
				{/if}

				<!-- Behavior -->
				{#if activeTab === 'behavior'}
					<div class="panel" id="panel-behavior" role="tabpanel">
						<div class="setting-group">
							<div class="toggle-row">
								<span id="smooth-nav-label">Smooth Navigation</span>
								<button
									class="toggle-btn"
									class:active={smoothNavigation}
									onclick={() => {
										smoothNavigation = !smoothNavigation;
										settings.setSetting('behavior', 'smoothNavigation', smoothNavigation);
									}}
									role="switch"
									aria-checked={smoothNavigation}
									aria-labelledby="smooth-nav-label"
								>
									<span class="toggle-track">
										<span class="toggle-thumb"></span>
									</span>
								</button>
							</div>
							<p class="setting-hint">
								Animate viewport transitions when clicking on annotations with gradual deceleration.
							</p>
						</div>

						<button class="reset-btn" onclick={() => resetSection('behavior')}>
							Reset Behavior Settings
						</button>
					</div>
				{/if}

				<!-- Defaults (configurable image defaults) -->
				{#if activeTab === 'defaults'}
					<div class="panel" id="panel-defaults" role="tabpanel">
						<p class="panel-intro">
							Configure the default values used when clicking "Reset to Defaults" in the viewer.
						</p>

						<div class="setting-group">
							<h3>Default Brightness</h3>
							<div class="slider-row">
								<input
									type="range"
									min="-100"
									max="100"
									step="1"
									value={defaultBrightness}
									oninput={handleDefaultBrightnessChange}
									class="slider"
								/>
								<span class="slider-value">{defaultBrightness}</span>
							</div>
						</div>

						<div class="setting-group">
							<h3>Default Contrast</h3>
							<div class="slider-row">
								<input
									type="range"
									min="-100"
									max="100"
									step="1"
									value={defaultContrast}
									oninput={handleDefaultContrastChange}
									class="slider"
								/>
								<span class="slider-value">{defaultContrast}</span>
							</div>
						</div>

						<div class="setting-group">
							<h3>Default Gamma</h3>
							<div class="slider-row">
								<input
									type="range"
									min="0.1"
									max="3.0"
									step="0.05"
									value={defaultGamma}
									oninput={handleDefaultGammaChange}
									class="slider"
								/>
								<span class="slider-value">{defaultGamma.toFixed(2)}</span>
							</div>
						</div>

						<div class="setting-group">
							<h3>Default Sharpening</h3>
							<div class="slider-row">
								<input
									type="range"
									min="0"
									max="100"
									step="1"
									value={defaultSharpeningIntensity}
									oninput={handleDefaultSharpeningChange}
									class="slider"
								/>
								<span class="slider-value">{defaultSharpeningIntensity}</span>
							</div>
						</div>

						<div class="setting-group">
							<h3>Default Stain Enhancement</h3>
							<div class="segmented-control" role="group">
								{#each stainEnhancementOptions as opt}
									<button
										class="segment"
										class:active={defaultStainEnhancement === opt.value}
										onclick={() => handleDefaultEnhancementChange(opt.value)}
									>
										{opt.label}
									</button>
								{/each}
							</div>
						</div>

						<div class="setting-group">
							<h3>Default Stain Normalization</h3>
							<div class="segmented-control" role="group">
								{#each stainNormOptions as opt}
									<button
										class="segment"
										class:active={defaultStainNormalization === opt.value}
										onclick={() => handleDefaultNormalizationChange(opt.value)}
									>
										{opt.label}
									</button>
								{/each}
							</div>
						</div>

						<button class="reset-btn" onclick={resetDefaultsToFactory}>
							Reset to Factory Defaults
						</button>
					</div>
				{/if}

				<!-- Account -->
				{#if activeTab === 'account' && $authStore.user}
					<div class="panel" id="panel-account" role="tabpanel">
						<div class="setting-group">
							<h3>User Information</h3>
							<div class="account-info">
								<div class="info-row">
									<span class="info-label">Username</span>
									<span class="info-value">{$authStore.user.username}</span>
								</div>
								<div class="info-row">
									<span class="info-label">Name</span>
									<span class="info-value"
										>{$authStore.user.first_name} {$authStore.user.last_name}</span
									>
								</div>
								<div class="info-row">
									<span class="info-label">Email</span>
									<span class="info-value">{$authStore.user.email}</span>
								</div>
								<div class="info-row">
									<span class="info-label">User ID</span>
									<span class="info-value info-id">{$authStore.user.id}</span>
								</div>
							</div>
						</div>

						<div class="setting-group">
							<h3>Session</h3>
							<button class="logout-btn" onclick={handleLogout}> Log Out </button>
						</div>
					</div>
				{/if}

				<!-- About -->
				{#if activeTab === 'about'}
					<div class="panel" id="panel-about" role="tabpanel">
						<div class="setting-group">
							<h3>About Eosin</h3>
							<p class="setting-hint">
								Eosin is a Whole-Slide Imaging (WSI) and analysis platform built for computational
								pathology and microbiology. Gigapixel slide images become viewable within seconds of
								upload as an event-driven compiler processes them into read-optimized multiscale
								pyramids. A viewport-based tile service streams only the tiles relevant to the user
								over WebSocket, providing a fluid, microscope-like experience even on commodity
								hardware. Sharded NVMe-backed storage with read-only replicas maximizes throughput
								and availability, while horizontally scaling services orchestrate ingestion, tiling,
								caching, and delivery. Eosin is engineered for future expansion into machine vision,
								search, and large-scale analysis across the visual manifold of histopathology.
							</p>
							<p class="setting-hint">
								This project is open source under MIT / Apache 2.0 dual license. Source code, documentation, and issue tracker
								are available on GitHub: <a href="https://github.com/eosin-platform/eosin" target="_blank" rel="noopener noreferrer">GitHub</a>
							</p>
							<p class="setting-hint">
								© <a
									href="https://thavlik.dev"
									target="_blank"
									rel="noopener noreferrer"
									class="about-link"
								>
									Thomas Havlik
								</a> 2026
							</p>
						</div>
					</div>
				{/if}
			</div>
		</div>

		<!-- Footer -->
		<footer class="modal-footer">
			<button class="danger-btn" onclick={resetToDefaults}> Reset All to Defaults </button>
			<button class="primary-btn" onclick={closeModal}> Done </button>
		</footer>
	</div>
</div>

<style>
	.modal-backdrop {
		position: fixed;
		inset: 0;
		z-index: 100;
		display: flex;
		align-items: center;
		justify-content: center;
		background: rgba(0, 0, 0, 0.7);
		backdrop-filter: blur(4px);
		padding: 1rem;
	}

	.modal-dialog {
		display: flex;
		flex-direction: column;
		width: 100%;
		max-width: 640px;
		height: min(600px, calc(100vh - 2rem));
		background: #1f1f1f;
		border-radius: 1rem;
		box-shadow: 0 20px 60px rgba(0, 0, 0, 0.5);
		overflow: hidden;
		outline: none;
	}

	.modal-header {
		display: flex;
		align-items: center;
		justify-content: space-between;
		padding: 1rem 1.25rem;
		border-bottom: 1px solid #333;
	}

	.modal-header h2 {
		margin: 0;
		font-size: 1.25rem;
		font-weight: 600;
		color: #f3f4f6;
	}

	.close-btn {
		display: flex;
		align-items: center;
		justify-content: center;
		width: 2rem;
		height: 2rem;
		padding: 0;
		background: transparent;
		border: none;
		border-radius: 0.375rem;
		color: #9ca3af;
		cursor: pointer;
		transition: all 0.15s;
	}

	.close-btn:hover {
		background: #374151;
		color: #f3f4f6;
	}

	.icon {
		width: 1.25rem;
		height: 1.25rem;
	}

	.modal-body {
		display: flex;
		flex: 1;
		min-height: 0;
		overflow: hidden;
	}

	.tab-nav {
		display: flex;
		flex-direction: column;
		width: 160px;
		padding: 0.5rem;
		background: #171717;
		border-right: 1px solid #333;
		flex-shrink: 0;
	}

	.tab-btn {
		display: flex;
		align-items: center;
		gap: 0.5rem;
		padding: 0.625rem 0.75rem;
		background: transparent;
		border: none;
		border-radius: 0.5rem;
		color: #9ca3af;
		font-size: 0.875rem;
		text-align: left;
		cursor: pointer;
		transition: all 0.15s;
	}

	.tab-btn:hover {
		background: rgba(255, 255, 255, 0.05);
		color: #e5e7eb;
	}

	.tab-btn.active {
		background: var(--primary-hex);
		color: white;
	}

	.tab-icon {
		font-size: 1rem;
	}

	.tab-content {
		flex: 1;
		overflow-y: auto;
		padding: 1.25rem;
		scrollbar-width: thin;
		scrollbar-color: #333 transparent;
	}

	.tab-content::-webkit-scrollbar {
		width: 9px;
	}

	.tab-content::-webkit-scrollbar-track {
		background: transparent;
	}

	.tab-content::-webkit-scrollbar-thumb {
		background: #333;
		border-radius: 3px;
	}

	.tab-content::-webkit-scrollbar-thumb:hover {
		background: #555;
	}

	.panel {
		display: flex;
		flex-direction: column;
		gap: 1.5rem;
	}

	.setting-group {
		display: flex;
		flex-direction: column;
		gap: 0.5rem;
	}

	.setting-group h3 {
		margin: 0;
		font-size: 0.8125rem;
		font-weight: 600;
		color: #9ca3af;
		text-transform: uppercase;
		letter-spacing: 0.05em;
	}

	.setting-hint {
		margin: 0.25rem 0 0 0;
		font-size: 0.75rem;
		color: #6b7280;
		line-height: 1.4;
	}

	.about-link {
		color: var(--secondary-hex);
		text-decoration: none;
	}

	.about-link:hover {
		text-decoration: underline;
	}

	/* Account panel styles */
	.account-info {
		display: flex;
		flex-direction: column;
		gap: 0.75rem;
		padding: 0.75rem;
		background: #2a2a2a;
		border-radius: 0.5rem;
	}

	.info-row {
		display: flex;
		justify-content: space-between;
		align-items: center;
		padding: 0.5rem 0;
		border-bottom: 1px solid #3a3a3a;
	}

	.info-row:last-child {
		border-bottom: none;
	}

	.info-label {
		font-size: 0.875rem;
		color: #9ca3af;
		font-weight: 500;
	}

	.info-value {
		font-size: 0.875rem;
		color: #e5e7eb;
	}

	.info-id {
		font-family: monospace;
		font-size: 0.75rem;
		color: #6b7280;
	}

	.logout-btn {
		width: 100%;
		padding: 0.75rem 1rem;
		background: #dc2626;
		color: white;
		border: none;
		border-radius: 0.5rem;
		font-size: 0.875rem;
		font-weight: 500;
		cursor: pointer;
		transition: background 0.15s;
	}

	.logout-btn:hover {
		background: #b91c1c;
	}

	.toggle-row {
		display: flex;
		align-items: center;
		justify-content: space-between;
		padding: 0.5rem 0;
		color: #e5e7eb;
		font-size: 0.875rem;
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
		width: 44px;
		height: 24px;
		background: #374151;
		border-radius: 12px;
		transition: background-color 0.2s;
	}

	.toggle-btn.active .toggle-track {
		background: var(--secondary-hex);
	}

	.toggle-thumb {
		position: absolute;
		top: 2px;
		left: 2px;
		width: 20px;
		height: 20px;
		background: white;
		border-radius: 50%;
		transition: transform 0.2s;
	}

	.toggle-btn.active .toggle-thumb {
		transform: translateX(20px);
	}

	.slider-row {
		display: flex;
		align-items: center;
		gap: 0.75rem;
	}

	.slider-label {
		font-size: 0.875rem;
		color: #9ca3af;
		min-width: 60px;
	}

	.slider {
		flex: 1;
		height: 6px;
		background: #374151;
		border-radius: 3px;
		appearance: none;
		cursor: pointer;
	}

	.slider::-webkit-slider-thumb {
		appearance: none;
		width: 18px;
		height: 18px;
		background: var(--secondary-hex);
		border-radius: 50%;
		cursor: pointer;
	}

	.slider::-moz-range-thumb {
		width: 18px;
		height: 18px;
		background: var(--secondary-hex);
		border: none;
		border-radius: 50%;
		cursor: pointer;
	}

	.slider-value {
		font-size: 0.875rem;
		color: #e5e7eb;
		min-width: 60px;
		text-align: right;
		font-variant-numeric: tabular-nums;
	}

	.select-input {
		width: 100%;
		padding: 0.625rem 0.75rem;
		background: #374151;
		border: 1px solid #4b5563;
		border-radius: 0.5rem;
		color: #e5e7eb;
		font-size: 0.875rem;
		cursor: pointer;
		outline: none;
	}

	.select-input:hover {
		border-color: #6b7280;
	}

	.select-input:focus {
		border-color: var(--secondary-hex);
		box-shadow: 0 0 0 2px var(--secondary-muted);
	}

	.radio-group {
		display: flex;
		flex-direction: column;
		gap: 0.5rem;
	}

	.radio-label {
		display: flex;
		align-items: center;
		gap: 0.5rem;
		padding: 0.5rem;
		background: #374151;
		border-radius: 0.375rem;
		color: #e5e7eb;
		font-size: 0.875rem;
		cursor: pointer;
		transition: background-color 0.15s;
	}

	.radio-label:hover {
		background: #4b5563;
	}

	.radio-label input[type='radio'] {
		width: 1rem;
		height: 1rem;
		accent-color: var(--secondary-hex);
	}

	.color-palette {
		display: flex;
		gap: 0.5rem;
		flex-wrap: wrap;
	}

	.color-swatch {
		position: relative;
		width: 32px;
		height: 32px;
		cursor: pointer;
	}

	.color-swatch input[type='color'] {
		position: absolute;
		inset: 0;
		opacity: 0;
		cursor: pointer;
	}

	.swatch-preview {
		display: block;
		width: 100%;
		height: 100%;
		border-radius: 0.375rem;
		border: 2px solid rgba(255, 255, 255, 0.2);
	}

	.text-btn {
		background: none;
		border: none;
		color: var(--secondary-hex);
		font-size: 0.8125rem;
		cursor: pointer;
		padding: 0;
		text-decoration: underline;
	}

	.text-btn:hover {
		color: var(--secondary-hover);
	}

	.reset-btn {
		align-self: flex-start;
		padding: 0.5rem 0.75rem;
		background: #374151;
		border: 1px solid #4b5563;
		border-radius: 0.375rem;
		color: #e5e7eb;
		font-size: 0.75rem;
		cursor: pointer;
		transition: all 0.15s;
		margin-top: 0.5rem;
	}

	.reset-btn:hover {
		background: #4b5563;
		border-color: #6b7280;
	}

	.modal-footer {
		display: flex;
		justify-content: space-between;
		padding: 1rem 1.25rem;
		border-top: 1px solid #333;
	}

	.danger-btn {
		padding: 0.625rem 1rem;
		background: transparent;
		border: 1px solid #dc2626;
		border-radius: 0.5rem;
		color: #dc2626;
		font-size: 0.875rem;
		cursor: pointer;
		transition: all 0.15s;
	}

	.danger-btn:hover {
		background: #dc2626;
		color: white;
	}

	.panel-intro {
		font-size: 0.875rem;
		color: #9ca3af;
		margin-bottom: 1.5rem;
		line-height: 1.5;
	}

	.segmented-control {
		display: flex;
		gap: 0.25rem;
		background: #1f2937;
		padding: 0.25rem;
		border-radius: 0.5rem;
	}

	.segment {
		flex: 1;
		padding: 0.5rem 0.75rem;
		background: transparent;
		border: none;
		border-radius: 0.375rem;
		color: #9ca3af;
		font-size: 0.8125rem;
		font-weight: 500;
		cursor: pointer;
		transition: all 0.15s;
	}

	.segment:hover {
		color: #e5e7eb;
	}

	.segment.active {
		background: var(--primary-hex);
		color: white;
	}

	.primary-btn {
		padding: 0.625rem 1.5rem;
		background: var(--primary-hex);
		border: none;
		border-radius: 0.5rem;
		color: white;
		font-size: 0.875rem;
		font-weight: 500;
		cursor: pointer;
		transition: background-color 0.15s;
	}

	.primary-btn:hover {
		background: var(--primary-hover);
	}

	/* Responsive */
	@media (max-width: 640px) {
		.modal-dialog {
			max-height: 100vh;
			border-radius: 0;
		}

		.modal-body {
			flex-direction: column;
		}

		.tab-nav {
			width: 100%;
			flex-direction: row;
			overflow-x: auto;
			border-right: none;
			border-bottom: 1px solid #333;
		}

		.tab-btn {
			flex-direction: column;
			gap: 0.25rem;
			padding: 0.5rem 0.75rem;
			flex-shrink: 0;
		}

		.tab-label {
			font-size: 0.6875rem;
		}
	}

	/* Touch device adaptations - larger touch targets */
	@media (pointer: coarse) {
		.close-btn {
			width: 2.75rem;
			height: 2.75rem;
		}

		.close-btn .icon {
			width: 1.5rem;
			height: 1.5rem;
		}

		.toggle-track {
			width: 52px;
			height: 28px;
			border-radius: 14px;
		}

		.toggle-thumb {
			width: 24px;
			height: 24px;
		}

		.toggle-btn.active .toggle-thumb {
			transform: translateX(24px);
		}

		.tab-btn {
			padding: 0.75rem 1rem;
		}

		/* Larger segmented control buttons for touch */
		.segment {
			padding: 0.75rem 1rem;
			font-size: 0.9375rem;
			min-height: 44px;
		}

		/* Larger slider for touch */
		.slider-row input[type='range'] {
			height: 8px;
		}

		.slider-row input[type='range']::-webkit-slider-thumb {
			width: 28px;
			height: 28px;
		}

		.slider-row input[type='range']::-moz-range-thumb {
			width: 28px;
			height: 28px;
		}

		/* Larger reset button for touch */
		.reset-btn {
			padding: 0.75rem 1rem;
			font-size: 0.875rem;
			min-height: 44px;
		}

		/* Larger primary button for touch */
		.primary-btn {
			padding: 0.875rem 1.5rem;
			font-size: 1rem;
			min-height: 48px;
		}

		/* Larger danger button for touch */
		.danger-btn {
			padding: 0.875rem 1.25rem;
			font-size: 1rem;
			min-height: 48px;
		}

		/* Larger color swatch for touch */
		.color-swatch {
			width: 44px;
			height: 44px;
		}

		/* Larger text button for touch */
		.text-btn {
			font-size: 1rem;
			padding: 0.5rem;
		}
	}
</style>
