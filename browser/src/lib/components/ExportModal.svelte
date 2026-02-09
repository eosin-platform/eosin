<script lang="ts">
	import { onMount, onDestroy } from 'svelte';
	import { browser } from '$app/environment';
	import { exportStore, type ExportOptions, type ImageFilters } from '$lib/stores/export';

	// Store state
	let isOpen = $state(false);
	let viewportContainer = $state<HTMLElement | null>(null);
	let filename = $state('export');
	let filters = $state<ImageFilters>({
		brightness: 0,
		contrast: 0,
		gamma: 1,
	});
	let viewportWidth = $state(0);
	let viewportHeight = $state(0);
	let options = $state<ExportOptions>({
		includeAnnotations: true,
		format: 'png',
		quality: 0.92,
		dpi: 96,
	});

	// Preview state
	let previewDataUrl = $state<string | null>(null);
	let isGeneratingPreview = $state(false);
	let isExporting = $state(false);

	// Estimated file size
	let estimatedSize = $state<string>('');

	// DPI input for text field
	let dpiInputValue = $state('96');

	// Computed export dimensions based on DPI
	// Base DPI is 96 (standard screen), so the scale factor is dpi / 96
	let exportWidth = $derived(Math.round(viewportWidth * (options.dpi / 96)));
	let exportHeight = $derived(Math.round(viewportHeight * (options.dpi / 96)));

	const unsubExport = exportStore.subscribe((state) => {
		isOpen = state.open;
		viewportContainer = state.viewportContainer;
		filename = state.filename;
		filters = state.filters;
		viewportWidth = state.viewportWidth;
		viewportHeight = state.viewportHeight;
		options = state.options;
		dpiInputValue = String(state.options.dpi);
	});

	onDestroy(() => {
		unsubExport();
	});

	// Generate preview when modal opens or options change
	$effect(() => {
		if (isOpen && viewportContainer) {
			generatePreview();
		}
	});

	async function generatePreview() {
		if (!viewportContainer || !browser) return;

		isGeneratingPreview = true;
		try {
			const compositeCanvas = await createCompositeCanvas();
			if (!compositeCanvas) return;

			// Generate preview at reduced size for performance
			const maxPreviewSize = 400;
			const scale = Math.min(
				maxPreviewSize / compositeCanvas.width,
				maxPreviewSize / compositeCanvas.height,
				1
			);

			const previewWidth = Math.round(compositeCanvas.width * scale);
			const previewHeight = Math.round(compositeCanvas.height * scale);

			const preview = document.createElement('canvas');
			preview.width = previewWidth;
			preview.height = previewHeight;
			const ctx = preview.getContext('2d');
			if (!ctx) return;

			// Draw with white background for JPEG
			if (options.format === 'jpeg') {
				ctx.fillStyle = '#ffffff';
				ctx.fillRect(0, 0, previewWidth, previewHeight);
			}

			ctx.drawImage(compositeCanvas, 0, 0, previewWidth, previewHeight);
			previewDataUrl = preview.toDataURL('image/png');

			// Estimate file size
			const mimeType = options.format === 'jpeg' ? 'image/jpeg' : 'image/png';
			const quality = options.format === 'jpeg' ? options.quality : undefined;
			const blob = await new Promise<Blob | null>((resolve) => {
				compositeCanvas.toBlob(resolve, mimeType, quality);
			});
			if (blob) {
				estimatedSize = formatFileSize(blob.size);
			}
		} finally {
			isGeneratingPreview = false;
		}
	}

	/**
	 * Apply brightness, contrast, and gamma adjustments to image data.
	 * This replicates the CSS filter behavior for accurate export.
	 */
	function applyFilters(imageData: ImageData, filters: ImageFilters): void {
		const data = imageData.data;
		const { brightness, contrast, gamma } = filters;

		// Convert settings to multipliers (matching CSS filter behavior)
		const brightnessMultiplier = 1 + (brightness / 100);
		const contrastMultiplier = 1 + (contrast / 100);
		// Gamma applied as power function to normalized values
		const gammaExp = gamma !== 1 ? 1 / gamma : 1;
		// Additional brightness from gamma approximation (matching ViewerPane)
		const gammaBrightness = gamma !== 1 ? Math.pow(0.5, gamma - 1) : 1;
		const totalBrightness = brightnessMultiplier * gammaBrightness;

		// Pre-calculate lookup table for performance
		const lut = new Uint8ClampedArray(256);
		for (let i = 0; i < 256; i++) {
			let value = i / 255;
			
			// Apply contrast (centered around 0.5)
			value = (value - 0.5) * contrastMultiplier + 0.5;
			
			// Apply gamma correction
			if (gammaExp !== 1 && value > 0) {
				value = Math.pow(value, gammaExp);
			}
			
			// Apply brightness
			value = value * totalBrightness;
			
			// Clamp and convert back to 0-255
			lut[i] = Math.round(Math.max(0, Math.min(255, value * 255)));
		}

		// Apply lookup table to all pixels
		for (let i = 0; i < data.length; i += 4) {
			data[i] = lut[data[i]];       // R
			data[i + 1] = lut[data[i + 1]]; // G
			data[i + 2] = lut[data[i + 2]]; // B
			// Alpha channel (i + 3) is not modified
		}
	}

	async function createCompositeCanvas(): Promise<HTMLCanvasElement | null> {
		if (!viewportContainer || viewportWidth === 0 || viewportHeight === 0) return null;

		// Find the image layer canvas (the main tile renderer canvas)
		const imageLayer = viewportContainer.querySelector('.image-layer');
		const imageCanvas = imageLayer?.querySelector('canvas') as HTMLCanvasElement | null;
		if (!imageCanvas) return null;

		// Calculate source dimensions (the canvas may have different size due to device pixel ratio)
		const sourceWidth = imageCanvas.width;
		const sourceHeight = imageCanvas.height;
		
		// Calculate export dimensions based on viewport size and DPI
		// Base DPI is 96, so scale factor is dpi / 96
		const scaleFactor = options.dpi / 96;
		const outputWidth = Math.round(viewportWidth * scaleFactor);
		const outputHeight = Math.round(viewportHeight * scaleFactor);

		const compositeCanvas = document.createElement('canvas');
		compositeCanvas.width = outputWidth;
		compositeCanvas.height = outputHeight;
		const ctx = compositeCanvas.getContext('2d');
		if (!ctx) return null;

		// Enable high-quality image scaling
		ctx.imageSmoothingEnabled = true;
		ctx.imageSmoothingQuality = 'high';

		// Draw the main image canvas scaled to the output size
		ctx.drawImage(imageCanvas, 0, 0, sourceWidth, sourceHeight, 0, 0, outputWidth, outputHeight);

		// Apply brightness/contrast/gamma filters to the image data
		const hasFilters = filters.brightness !== 0 || filters.contrast !== 0 || filters.gamma !== 1;
		if (hasFilters) {
			const imageData = ctx.getImageData(0, 0, outputWidth, outputHeight);
			applyFilters(imageData, filters);
			ctx.putImageData(imageData, 0, 0);
		}

		// Overlay annotations if enabled
		if (options.includeAnnotations) {
			// Find specifically the annotation overlay elements by their known classes
			// These are positioned to match the viewport, so we need to scale them appropriately
			
			// Mask canvas has class "mask-canvas"
			const maskCanvas = viewportContainer.querySelector('canvas.mask-canvas') as HTMLCanvasElement | null;
			if (maskCanvas) {
				try {
					// Scale the mask canvas to match output dimensions
					ctx.drawImage(maskCanvas, 0, 0, maskCanvas.width, maskCanvas.height, 0, 0, outputWidth, outputHeight);
				} catch (e) {
					console.warn('Failed to draw mask canvas:', e);
				}
			}
			
			// Annotation SVG overlay has class "annotation-overlay"
			const annotationSvg = viewportContainer.querySelector('svg.annotation-overlay') as SVGSVGElement | null;
			if (annotationSvg) {
				await renderSvgToCanvas(annotationSvg, ctx, outputWidth, outputHeight);
			}
		}

		return compositeCanvas;
	}

	async function renderSvgToCanvas(
		svg: SVGSVGElement,
		ctx: CanvasRenderingContext2D,
		width: number,
		height: number
	) {
		// Get the SVG's bounding box to check if it's visible
		const rect = svg.getBoundingClientRect();
		if (rect.width === 0 || rect.height === 0) return;

		// Clone the SVG to avoid modifying the original
		const svgClone = svg.cloneNode(true) as SVGSVGElement;
		
		// Set explicit dimensions matching the canvas
		svgClone.setAttribute('width', String(width));
		svgClone.setAttribute('height', String(height));
		
		// Ensure the viewBox matches the canvas dimensions
		svgClone.setAttribute('viewBox', `0 0 ${width} ${height}`);
		
		// Remove any transforms that might be applied at the container level
		svgClone.style.transform = 'none';
		svgClone.style.position = 'static';

		// Serialize to string
		const serializer = new XMLSerializer();
		let svgString = serializer.serializeToString(svgClone);
		
		// Ensure proper XML namespace
		if (!svgString.includes('xmlns=')) {
			svgString = svgString.replace('<svg', '<svg xmlns="http://www.w3.org/2000/svg"');
		}

		// Create a blob URL
		const svgBlob = new Blob([svgString], { type: 'image/svg+xml;charset=utf-8' });
		const url = URL.createObjectURL(svgBlob);

		try {
			// Load as image
			const img = new Image();
			await new Promise<void>((resolve, reject) => {
				img.onload = () => resolve();
				img.onerror = reject;
				img.src = url;
			});
			ctx.drawImage(img, 0, 0);
		} catch (e) {
			console.warn('Failed to render SVG to canvas:', e);
		} finally {
			URL.revokeObjectURL(url);
		}
	}

	function formatFileSize(bytes: number): string {
		if (bytes < 1024) return `${bytes} B`;
		if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
		return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
	}

	async function handleExport() {
		if (!viewportContainer) return;

		isExporting = true;
		try {
			const compositeCanvas = await createCompositeCanvas();
			if (!compositeCanvas) return;

			const mimeType = options.format === 'jpeg' ? 'image/jpeg' : 'image/png';
			const quality = options.format === 'jpeg' ? options.quality : undefined;
			const extension = options.format === 'jpeg' ? 'jpg' : 'png';

			const blob = await new Promise<Blob | null>((resolve) => {
				compositeCanvas.toBlob(resolve, mimeType, quality);
			});
			if (!blob) return;

			// Download the file
			const url = URL.createObjectURL(blob);
			const link = document.createElement('a');
			link.href = url;
			link.download = `${filename}.${extension}`;
			document.body.appendChild(link);
			link.click();
			document.body.removeChild(link);
			URL.revokeObjectURL(url);

			closeModal();
		} finally {
			isExporting = false;
		}
	}

	function closeModal() {
		exportStore.close();
	}

	function handleKeydown(event: KeyboardEvent) {
		if (event.key === 'Escape') {
			closeModal();
		}
	}

	function handleFormatChange(format: 'png' | 'jpeg') {
		exportStore.updateOptions({ format });
	}

	function handleAnnotationsToggle() {
		exportStore.updateOptions({ includeAnnotations: !options.includeAnnotations });
	}

	function handleQualityChange(event: Event) {
		const target = event.target as HTMLInputElement;
		exportStore.updateOptions({ quality: parseFloat(target.value) });
	}

	function handleDpiSliderChange(event: Event) {
		const target = event.target as HTMLInputElement;
		const dpi = parseInt(target.value, 10);
		dpiInputValue = String(dpi);
		exportStore.updateOptions({ dpi });
	}

	function handleDpiInputChange(event: Event) {
		const target = event.target as HTMLInputElement;
		const value = target.value.trim();
		dpiInputValue = value;
		
		const dpi = parseInt(value, 10);
		if (!isNaN(dpi) && dpi >= 72 && dpi <= 600) {
			exportStore.updateOptions({ dpi });
		}
	}

	function handleDpiInputBlur() {
		// Clamp value on blur
		let dpi = parseInt(dpiInputValue, 10);
		if (isNaN(dpi) || dpi < 72) dpi = 72;
		if (dpi > 600) dpi = 600;
		dpiInputValue = String(dpi);
		exportStore.updateOptions({ dpi });
	}
</script>

<svelte:window onkeydown={handleKeydown} />

{#if isOpen}
	<!-- svelte-ignore a11y_click_events_have_key_events -->
	<!-- svelte-ignore a11y_no_static_element_interactions -->
	<div class="export-overlay" onclick={closeModal}>
		<div 
			class="export-modal" 
			onclick={(e) => e.stopPropagation()}
			onmousedown={(e) => e.stopPropagation()}
			onmousemove={(e) => e.stopPropagation()}
			onwheel={(e) => e.stopPropagation()}
			ontouchstart={(e) => e.stopPropagation()}
			ontouchmove={(e) => e.stopPropagation()}
		>
			<!-- Header -->
			<div class="export-header">
				<h2>Export Image</h2>
				<button class="close-btn" onclick={closeModal} aria-label="Close export dialog">
					<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 20 20" fill="currentColor">
						<path
							d="M6.28 5.22a.75.75 0 00-1.06 1.06L8.94 10l-3.72 3.72a.75.75 0 101.06 1.06L10 11.06l3.72 3.72a.75.75 0 101.06-1.06L11.06 10l3.72-3.72a.75.75 0 00-1.06-1.06L10 8.94 6.28 5.22z"
						/>
					</svg>
				</button>
			</div>

			<div class="export-body">
				<!-- Preview Section -->
				<div class="preview-section">
					<div class="section-label">Preview</div>
					<div class="preview-container">
						{#if isGeneratingPreview}
							<div class="preview-loading">
								<div class="spinner"></div>
								<span>Generating preview...</span>
							</div>
						{:else if previewDataUrl}
							<img src={previewDataUrl} alt="Export preview" class="preview-image" />
						{:else}
							<div class="preview-placeholder">
								<svg xmlns="http://www.w3.org/2000/svg" width="48" height="48" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5">
									<rect x="3" y="3" width="18" height="18" rx="2" ry="2"></rect>
									<circle cx="8.5" cy="8.5" r="1.5"></circle>
									<polyline points="21 15 16 10 5 21"></polyline>
								</svg>
								<span>No preview available</span>
							</div>
						{/if}
					</div>
					{#if estimatedSize}
						<div class="size-estimate">
							Estimated size: <strong>{estimatedSize}</strong>
						</div>
					{/if}
					<div class="dimensions-display">
						<span class="dimensions-icon">
							<svg xmlns="http://www.w3.org/2000/svg" width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
								<rect x="3" y="3" width="18" height="18" rx="2" ry="2"></rect>
							</svg>
						</span>
						<span class="dimensions-text">{exportWidth} × {exportHeight} px</span>
					</div>
				</div>

				<!-- Options Section -->
				<div class="options-section">
					<div class="section-label">Export Options</div>

					<!-- DPI Control -->
					<div class="option-group">
						<div class="option-label-standalone">
							<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
								<rect x="2" y="2" width="20" height="20" rx="2.18" ry="2.18"></rect>
								<line x1="7" y1="2" x2="7" y2="22"></line>
								<line x1="17" y1="2" x2="17" y2="22"></line>
								<line x1="2" y1="12" x2="22" y2="12"></line>
								<line x1="2" y1="7" x2="7" y2="7"></line>
								<line x1="2" y1="17" x2="7" y2="17"></line>
								<line x1="17" y1="7" x2="22" y2="7"></line>
								<line x1="17" y1="17" x2="22" y2="17"></line>
							</svg>
							<span>Resolution (DPI)</span>
						</div>
						<div class="dpi-control">
							<input
								type="range"
								id="dpi-slider"
								min="72"
								max="600"
								step="1"
								value={options.dpi}
								oninput={handleDpiSliderChange}
								class="dpi-slider"
								aria-label="DPI slider"
							/>
							<div class="dpi-input-wrapper">
								<input
									type="text"
									id="dpi-input"
									value={dpiInputValue}
									oninput={handleDpiInputChange}
									onblur={handleDpiInputBlur}
									class="dpi-input"
									aria-label="DPI value"
								/>
								<span class="dpi-unit">dpi</span>
							</div>
						</div>
						<div class="dpi-presets">
							<button
								class="preset-btn"
								class:active={options.dpi === 72}
								onclick={() => { dpiInputValue = '72'; exportStore.updateOptions({ dpi: 72 }); }}
								type="button"
							>72</button>
							<button
								class="preset-btn"
								class:active={options.dpi === 96}
								onclick={() => { dpiInputValue = '96'; exportStore.updateOptions({ dpi: 96 }); }}
								type="button"
							>96</button>
							<button
								class="preset-btn"
								class:active={options.dpi === 150}
								onclick={() => { dpiInputValue = '150'; exportStore.updateOptions({ dpi: 150 }); }}
								type="button"
							>150</button>
							<button
								class="preset-btn"
								class:active={options.dpi === 300}
								onclick={() => { dpiInputValue = '300'; exportStore.updateOptions({ dpi: 300 }); }}
								type="button"
							>300</button>
						</div>
					</div>

					<!-- Annotations Toggle -->
					<div class="option-group">
						<label class="option-row toggle-option">
							<div class="option-label">
								<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
									<polygon points="12 2 22 8.5 22 15.5 12 22 2 15.5 2 8.5 12 2"></polygon>
									<line x1="12" y1="22" x2="12" y2="15.5"></line>
									<polyline points="22 8.5 12 15.5 2 8.5"></polyline>
								</svg>
								<span>Include Annotations</span>
							</div>
							<button
								class="toggle-switch"
								class:active={options.includeAnnotations}
								onclick={handleAnnotationsToggle}
								aria-pressed={options.includeAnnotations}
								aria-label="Toggle include annotations"
								type="button"
							>
								<span class="toggle-track">
									<span class="toggle-thumb"></span>
								</span>
							</button>
						</label>
					</div>

					<!-- Format Selection -->
					<div class="option-group">
						<div class="option-label-standalone">
							<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
								<path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z"></path>
								<polyline points="14 2 14 8 20 8"></polyline>
							</svg>
							<span>Format</span>
						</div>
						<div class="format-buttons">
							<button
								class="format-btn"
								class:active={options.format === 'png'}
								onclick={() => handleFormatChange('png')}
								type="button"
							>
								<span class="format-name">PNG</span>
								<span class="format-desc">Lossless, transparent</span>
							</button>
							<button
								class="format-btn"
								class:active={options.format === 'jpeg'}
								onclick={() => handleFormatChange('jpeg')}
								type="button"
							>
								<span class="format-name">JPEG</span>
								<span class="format-desc">Smaller file size</span>
							</button>
						</div>
					</div>

					<!-- Quality Slider (JPEG only) -->
					{#if options.format === 'jpeg'}
						<div class="option-group quality-group">
							<label class="option-label-standalone" for="quality-slider">
								<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
									<circle cx="12" cy="12" r="3"></circle>
									<path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 0 1 0 2.83 2 2 0 0 1-2.83 0l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 0 1-2 2 2 2 0 0 1-2-2v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 0 1-2.83 0 2 2 0 0 1 0-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 0 1-2-2 2 2 0 0 1 2-2h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 0 1 0-2.83 2 2 0 0 1 2.83 0l.06.06a1.65 1.65 0 0 0 1.82.33H9a1.65 1.65 0 0 0 1-1.51V3a2 2 0 0 1 2-2 2 2 0 0 1 2 2v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 0 1 2.83 0 2 2 0 0 1 0 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82V9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 0 1 2 2 2 2 0 0 1-2 2h-.09a1.65 1.65 0 0 0-1.51 1z"></path>
								</svg>
								<span>Quality: {Math.round(options.quality * 100)}%</span>
							</label>
							<input
								type="range"
								id="quality-slider"
								min="0.1"
								max="1"
								step="0.05"
								value={options.quality}
								oninput={handleQualityChange}
								class="quality-slider"
							/>
							<div class="quality-labels">
								<span>Smaller</span>
								<span>Higher</span>
							</div>
						</div>
					{/if}
				</div>
			</div>

			<!-- Footer -->
			<div class="export-footer">
				<button class="btn btn-secondary" onclick={closeModal} type="button">
					Cancel
				</button>
				<button
					class="btn btn-primary"
					onclick={handleExport}
					disabled={isExporting || isGeneratingPreview}
					type="button"
				>
					{#if isExporting}
						<span class="spinner small"></span>
						Exporting...
					{:else}
						<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
							<path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"></path>
							<polyline points="7 10 12 15 17 10"></polyline>
							<line x1="12" y1="15" x2="12" y2="3"></line>
						</svg>
						Export
					{/if}
				</button>
			</div>
		</div>
	</div>
{/if}

<style>
	.export-overlay {
		position: fixed;
		inset: 0;
		display: flex;
		align-items: center;
		justify-content: center;
		padding: 24px;
		background: rgba(0, 0, 0, 0.7);
		backdrop-filter: blur(4px);
		z-index: 10001;
		animation: fadeIn 0.15s ease-out;
	}

	@keyframes fadeIn {
		from {
			opacity: 0;
		}
		to {
			opacity: 1;
		}
	}

	.export-modal {
		display: flex;
		flex-direction: column;
		width: 100%;
		max-width: 560px;
		max-height: calc(100vh - 48px);
		background: #1a1a1a;
		border: 1px solid rgba(255, 255, 255, 0.12);
		border-radius: 12px;
		box-shadow: 0 24px 48px rgba(0, 0, 0, 0.5);
		overflow: hidden;
		animation: slideUp 0.2s ease-out;
	}

	@keyframes slideUp {
		from {
			opacity: 0;
			transform: translateY(20px) scale(0.98);
		}
		to {
			opacity: 1;
			transform: translateY(0) scale(1);
		}
	}

	.export-header {
		display: flex;
		align-items: center;
		justify-content: space-between;
		padding: 16px 20px;
		border-bottom: 1px solid rgba(255, 255, 255, 0.08);
		background: rgba(255, 255, 255, 0.02);
	}

	.export-header h2 {
		margin: 0;
		font-size: 17px;
		font-weight: 600;
		color: #fff;
		letter-spacing: -0.01em;
	}

	.close-btn {
		display: flex;
		align-items: center;
		justify-content: center;
		width: 32px;
		height: 32px;
		background: rgba(255, 255, 255, 0.06);
		border: none;
		border-radius: 6px;
		cursor: pointer;
		color: rgba(255, 255, 255, 0.6);
		transition: all 0.15s ease;
	}

	.close-btn:hover {
		background: rgba(255, 255, 255, 0.12);
		color: #fff;
	}

	.close-btn svg {
		width: 18px;
		height: 18px;
	}

	.export-body {
		display: flex;
		flex-direction: column;
		gap: 20px;
		padding: 20px;
		overflow-y: auto;
		scrollbar-width: thin;
		scrollbar-color: #333 transparent;
	}

	.export-body::-webkit-scrollbar {
		width: 9px;
	}

	.export-body::-webkit-scrollbar-track {
		background: transparent;
	}

	.export-body::-webkit-scrollbar-thumb {
		background: #333;
		border-radius: 3px;
	}

	.export-body::-webkit-scrollbar-thumb:hover {
		background: #555;
	}

	.section-label {
		font-size: 11px;
		font-weight: 600;
		text-transform: uppercase;
		letter-spacing: 0.05em;
		color: rgba(255, 255, 255, 0.45);
		margin-bottom: 10px;
	}

	/* Preview Section */
	.preview-section {
		display: flex;
		flex-direction: column;
	}

	.preview-container {
		display: flex;
		align-items: center;
		justify-content: center;
		height: 280px;
		background: #0d0d0d;
		border: 1px solid rgba(255, 255, 255, 0.08);
		border-radius: 8px;
		overflow: hidden;
	}

	.preview-image {
		max-width: 100%;
		max-height: 280px;
		object-fit: contain;
	}

	.preview-loading,
	.preview-placeholder {
		display: flex;
		flex-direction: column;
		align-items: center;
		gap: 12px;
		color: rgba(255, 255, 255, 0.4);
		font-size: 13px;
	}

	.preview-placeholder svg {
		opacity: 0.3;
	}

	.size-estimate {
		margin-top: 8px;
		font-size: 12px;
		color: rgba(255, 255, 255, 0.5);
		text-align: center;
	}

	.size-estimate strong {
		color: rgba(255, 255, 255, 0.8);
	}

	.dimensions-display {
		display: flex;
		align-items: center;
		justify-content: center;
		gap: 6px;
		margin-top: 6px;
		font-size: 13px;
		color: rgba(255, 255, 255, 0.6);
	}

	.dimensions-icon {
		color: rgba(255, 255, 255, 0.4);
		display: flex;
		align-items: center;
	}

	.dimensions-text {
		font-family: ui-monospace, 'SF Mono', 'Cascadia Code', 'Segoe UI Mono', monospace;
		letter-spacing: -0.02em;
	}

	/* DPI Control */
	.dpi-control {
		display: flex;
		align-items: center;
		gap: 12px;
	}

	.dpi-slider {
		flex: 1;
		height: 6px;
		background: rgba(255, 255, 255, 0.1);
		border-radius: 3px;
		outline: none;
		-webkit-appearance: none;
		appearance: none;
		cursor: pointer;
	}

	.dpi-slider::-webkit-slider-thumb {
		-webkit-appearance: none;
		width: 18px;
		height: 18px;
		background: #fff;
		border-radius: 50%;
		box-shadow: 0 1px 4px rgba(0, 0, 0, 0.3);
		cursor: pointer;
		transition: transform 0.1s ease;
	}

	.dpi-slider::-webkit-slider-thumb:hover {
		transform: scale(1.1);
	}

	.dpi-slider::-moz-range-thumb {
		width: 18px;
		height: 18px;
		background: #fff;
		border: none;
		border-radius: 50%;
		box-shadow: 0 1px 4px rgba(0, 0, 0, 0.3);
		cursor: pointer;
	}

	.dpi-input-wrapper {
		display: flex;
		align-items: center;
		gap: 4px;
		background: rgba(255, 255, 255, 0.06);
		border: 1px solid rgba(255, 255, 255, 0.12);
		border-radius: 6px;
		padding: 6px 10px;
	}

	.dpi-input {
		width: 48px;
		background: transparent;
		border: none;
		font-size: 14px;
		font-family: ui-monospace, 'SF Mono', 'Cascadia Code', 'Segoe UI Mono', monospace;
		color: #fff;
		text-align: right;
		outline: none;
	}

	.dpi-input:focus {
		outline: none;
	}

	.dpi-unit {
		font-size: 12px;
		color: rgba(255, 255, 255, 0.5);
	}

	.dpi-presets {
		display: flex;
		gap: 6px;
		margin-top: 8px;
	}

	.preset-btn {
		padding: 4px 10px;
		background: rgba(255, 255, 255, 0.06);
		border: 1px solid rgba(255, 255, 255, 0.1);
		border-radius: 4px;
		font-size: 12px;
		color: rgba(255, 255, 255, 0.7);
		cursor: pointer;
		transition: all 0.15s ease;
	}

	.preset-btn:hover {
		background: rgba(255, 255, 255, 0.1);
		border-color: rgba(255, 255, 255, 0.2);
	}

	.preset-btn.active {
		background: rgba(59, 130, 246, 0.2);
		border-color: #3b82f6;
		color: #fff;
	}

	/* Options Section */
	.options-section {
		display: flex;
		flex-direction: column;
	}

	.option-group {
		padding: 12px 0;
		border-bottom: 1px solid rgba(255, 255, 255, 0.06);
	}

	.option-group:last-child {
		border-bottom: none;
		padding-bottom: 0;
	}

	.option-row {
		display: flex;
		align-items: center;
		justify-content: space-between;
		cursor: pointer;
	}

	.option-label,
	.option-label-standalone {
		display: flex;
		align-items: center;
		gap: 10px;
		font-size: 14px;
		color: rgba(255, 255, 255, 0.9);
	}

	.option-label svg,
	.option-label-standalone svg {
		color: rgba(255, 255, 255, 0.5);
	}

	.option-label-standalone {
		margin-bottom: 10px;
	}

	/* Toggle Switch */
	.toggle-switch {
		background: none;
		border: none;
		padding: 0;
		cursor: pointer;
	}

	.toggle-track {
		display: block;
		width: 44px;
		height: 24px;
		background: rgba(255, 255, 255, 0.15);
		border-radius: 12px;
		position: relative;
		transition: background 0.2s ease;
	}

	.toggle-switch.active .toggle-track {
		background: #3b82f6;
	}

	.toggle-thumb {
		position: absolute;
		top: 2px;
		left: 2px;
		width: 20px;
		height: 20px;
		background: #fff;
		border-radius: 50%;
		box-shadow: 0 1px 3px rgba(0, 0, 0, 0.3);
		transition: transform 0.2s ease;
	}

	.toggle-switch.active .toggle-thumb {
		transform: translateX(20px);
	}

	/* Format Buttons */
	.format-buttons {
		display: grid;
		grid-template-columns: 1fr 1fr;
		gap: 10px;
	}

	.format-btn {
		display: flex;
		flex-direction: column;
		align-items: flex-start;
		padding: 12px 14px;
		background: rgba(255, 255, 255, 0.04);
		border: 1px solid rgba(255, 255, 255, 0.1);
		border-radius: 8px;
		color: rgba(255, 255, 255, 0.7);
		cursor: pointer;
		transition: all 0.15s ease;
	}

	.format-btn:hover {
		background: rgba(255, 255, 255, 0.08);
		border-color: rgba(255, 255, 255, 0.2);
	}

	.format-btn.active {
		background: rgba(59, 130, 246, 0.15);
		border-color: #3b82f6;
		color: #fff;
	}

	.format-name {
		font-size: 14px;
		font-weight: 600;
	}

	.format-desc {
		font-size: 11px;
		color: rgba(255, 255, 255, 0.4);
		margin-top: 2px;
	}

	.format-btn.active .format-desc {
		color: rgba(255, 255, 255, 0.6);
	}

	/* Quality Slider */
	.quality-group {
		padding-top: 8px;
	}

	.quality-slider {
		width: 100%;
		height: 6px;
		background: rgba(255, 255, 255, 0.1);
		border-radius: 3px;
		outline: none;
		-webkit-appearance: none;
		appearance: none;
		cursor: pointer;
	}

	.quality-slider::-webkit-slider-thumb {
		-webkit-appearance: none;
		width: 18px;
		height: 18px;
		background: #fff;
		border-radius: 50%;
		box-shadow: 0 1px 4px rgba(0, 0, 0, 0.3);
		cursor: pointer;
		transition: transform 0.1s ease;
	}

	.quality-slider::-webkit-slider-thumb:hover {
		transform: scale(1.1);
	}

	.quality-slider::-moz-range-thumb {
		width: 18px;
		height: 18px;
		background: #fff;
		border: none;
		border-radius: 50%;
		box-shadow: 0 1px 4px rgba(0, 0, 0, 0.3);
		cursor: pointer;
	}

	.quality-labels {
		display: flex;
		justify-content: space-between;
		margin-top: 6px;
		font-size: 11px;
		color: rgba(255, 255, 255, 0.35);
	}

	/* Footer */
	.export-footer {
		display: flex;
		justify-content: flex-end;
		gap: 10px;
		padding: 16px 20px;
		border-top: 1px solid rgba(255, 255, 255, 0.08);
		background: rgba(255, 255, 255, 0.02);
	}

	.btn {
		display: flex;
		align-items: center;
		justify-content: center;
		gap: 8px;
		padding: 10px 18px;
		border: none;
		border-radius: 6px;
		font-size: 14px;
		font-weight: 500;
		cursor: pointer;
		transition: all 0.15s ease;
	}

	.btn-secondary {
		background: rgba(255, 255, 255, 0.08);
		color: rgba(255, 255, 255, 0.8);
	}

	.btn-secondary:hover {
		background: rgba(255, 255, 255, 0.12);
		color: #fff;
	}

	.btn-primary {
		background: #3b82f6;
		color: #fff;
	}

	.btn-primary:hover:not(:disabled) {
		background: #2563eb;
	}

	.btn-primary:active:not(:disabled) {
		transform: scale(0.98);
	}

	.btn-primary:disabled {
		opacity: 0.6;
		cursor: not-allowed;
	}

	/* Spinner */
	.spinner {
		width: 20px;
		height: 20px;
		border: 2px solid rgba(255, 255, 255, 0.2);
		border-top-color: #fff;
		border-radius: 50%;
		animation: spin 0.8s linear infinite;
	}

	.spinner.small {
		width: 14px;
		height: 14px;
		border-width: 2px;
	}

	@keyframes spin {
		to {
			transform: rotate(360deg);
		}
	}

	/* Mobile responsiveness */
	@media (max-width: 480px) {
		.export-overlay {
			padding: 12px;
		}

		.export-modal {
			max-width: 100%;
		}

		.export-body {
			padding: 16px;
		}

		.format-buttons {
			grid-template-columns: 1fr;
		}

		.export-footer {
			flex-direction: column-reverse;
		}

		.btn {
			width: 100%;
		}
	}
</style>
