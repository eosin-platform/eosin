<script lang="ts">
	import { onMount, onDestroy } from 'svelte';
	import { browser } from '$app/environment';
	import {
		exportStore,
		type ExportOptions,
		type ImageFilters,
		type MeasurementExportState,
		type RoiExportState,
		type RgbaColor,
		type LineStyle,
		type LineCap,
		type RoiOutlineOptions,
		type RoiOverlayOptions,
		type MeasurementOptions
	} from '$lib/stores/export';
	import { settings, type MeasurementUnit } from '$lib/stores/settings';

	// Store state
	let isOpen = $state(false);
	let viewportContainer = $state<HTMLElement | null>(null);
	let filename = $state('export');
	let filters = $state<ImageFilters>({
		brightness: 0,
		contrast: 0,
		gamma: 1
	});
	let viewportWidth = $state(0);
	let viewportHeight = $state(0);
	let micronsPerPixel = $state(0.25);
	let viewportState = $state<{ x: number; y: number; zoom: number } | null>(null);
	let measurement = $state<MeasurementExportState>({
		active: false,
		startImage: null,
		endImage: null
	});
	let roi = $state<RoiExportState>({
		active: false,
		startImage: null,
		endImage: null
	});
	let options = $state<ExportOptions>({
		includeAnnotations: true,
		format: 'png',
		quality: 0.92,
		dpi: 96,
		showMeasurement: true,
		measurementOptions: {
			color: { r: 59, g: 130, b: 246, a: 1 },
			thickness: 2,
			lineStyle: 'solid',
			lineCap: 'round',
			dashLength: 8,
			dashSpacing: 4,
			dotSpacing: 4,
			fontSize: 20
		},
		roiOutline: {
			enabled: true,
			color: { r: 251, g: 191, b: 36, a: 1 },
			thickness: 2,
			lineStyle: 'dashed',
			lineCap: 'round',
			dashLength: 8,
			dashSpacing: 4,
			dotSpacing: 4
		},
		roiOverlay: {
			enabled: false,
			color: { r: 0, g: 0, b: 0, a: 0.4 }
		}
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

	// Check if measurement/ROI are available
	let hasMeasurement = $derived(
		measurement.active && measurement.startImage !== null && measurement.endImage !== null
	);
	let hasRoi = $derived(roi.active && roi.startImage !== null && roi.endImage !== null);

	const unsubExport = exportStore.subscribe((state) => {
		isOpen = state.open;
		viewportContainer = state.viewportContainer;
		filename = state.filename;
		filters = state.filters;
		viewportWidth = state.viewportWidth;
		viewportHeight = state.viewportHeight;
		micronsPerPixel = state.micronsPerPixel;
		viewportState = state.viewportState;
		measurement = state.measurement;
		roi = state.roi;
		options = state.options;
		dpiInputValue = String(state.options.dpi);
	});

	onDestroy(() => {
		unsubExport();
	});

	// Generate preview when modal opens or options change
	$effect(() => {
		// Track all options that affect the preview
		const _trackOptions = JSON.stringify(options);
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

			// Generate preview at high quality
			const maxPreviewSize = 1600;
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
		const brightnessMultiplier = 1 + brightness / 100;
		const contrastMultiplier = 1 + contrast / 100;
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
			data[i] = lut[data[i]]; // R
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
			const maskCanvas = viewportContainer.querySelector(
				'canvas.mask-canvas'
			) as HTMLCanvasElement | null;
			if (maskCanvas) {
				try {
					// Scale the mask canvas to match output dimensions
					ctx.drawImage(
						maskCanvas,
						0,
						0,
						maskCanvas.width,
						maskCanvas.height,
						0,
						0,
						outputWidth,
						outputHeight
					);
				} catch (e) {
					console.warn('Failed to draw mask canvas:', e);
				}
			}

			// Annotation SVG overlay has class "annotation-overlay"
			const annotationSvg = viewportContainer.querySelector(
				'svg.annotation-overlay'
			) as SVGSVGElement | null;
			if (annotationSvg) {
				await renderSvgToCanvas(annotationSvg, ctx, outputWidth, outputHeight);
			}
		}

		// Render ROI overlays if ROI is active
		if (hasRoi && viewportState && roi.startImage && roi.endImage) {
			// Convert image coordinates to output canvas coordinates
			const scaleX = outputWidth / viewportWidth;
			const scaleY = outputHeight / viewportHeight;

			// Calculate screen coordinates (same as in RoiOverlay.svelte)
			const screenStartX = (roi.startImage.x - viewportState.x) * viewportState.zoom;
			const screenStartY = (roi.startImage.y - viewportState.y) * viewportState.zoom;
			const screenEndX = (roi.endImage.x - viewportState.x) * viewportState.zoom;
			const screenEndY = (roi.endImage.y - viewportState.y) * viewportState.zoom;

			// Scale to output dimensions
			const roiX = Math.min(screenStartX, screenEndX) * scaleX;
			const roiY = Math.min(screenStartY, screenEndY) * scaleY;
			const roiWidth = Math.abs(screenEndX - screenStartX) * scaleX;
			const roiHeight = Math.abs(screenEndY - screenStartY) * scaleY;

			// Draw outside overlay first (so outline goes on top)
			if (options.roiOverlay.enabled) {
				const overlayColor = rgbaToCss(options.roiOverlay.color);
				ctx.fillStyle = overlayColor;

				// Fill everything outside the ROI
				// Top region
				ctx.fillRect(0, 0, outputWidth, roiY);
				// Bottom region
				ctx.fillRect(0, roiY + roiHeight, outputWidth, outputHeight - roiY - roiHeight);
				// Left region
				ctx.fillRect(0, roiY, roiX, roiHeight);
				// Right region
				ctx.fillRect(roiX + roiWidth, roiY, outputWidth - roiX - roiWidth, roiHeight);
			}

			// Draw ROI outline
			if (options.roiOutline.enabled) {
				ctx.strokeStyle = rgbaToCss(options.roiOutline.color);
				ctx.lineWidth = options.roiOutline.thickness * scaleX; // Scale line thickness
				ctx.lineCap = options.roiOutline.lineCap;
				ctx.lineJoin = options.roiOutline.lineCap === 'round' ? 'round' : 'miter';

				// Set line style
				if (options.roiOutline.lineStyle === 'dashed') {
					ctx.setLineDash([
						options.roiOutline.dashLength * scaleX,
						options.roiOutline.dashSpacing * scaleX
					]);
				} else if (options.roiOutline.lineStyle === 'dotted') {
					ctx.setLineDash([
						options.roiOutline.thickness * scaleX,
						options.roiOutline.dotSpacing * scaleX
					]);
				} else {
					ctx.setLineDash([]);
				}

				ctx.strokeRect(roiX, roiY, roiWidth, roiHeight);
				ctx.setLineDash([]); // Reset
			}
		}

		// Render measurement overlay if enabled and active
		if (
			options.showMeasurement &&
			hasMeasurement &&
			viewportState &&
			measurement.startImage &&
			measurement.endImage
		) {
			// Convert image coordinates to output canvas coordinates
			const scaleX = outputWidth / viewportWidth;
			const scaleY = outputHeight / viewportHeight;

			// Calculate screen coordinates
			const startX = (measurement.startImage.x - viewportState.x) * viewportState.zoom * scaleX;
			const startY = (measurement.startImage.y - viewportState.y) * viewportState.zoom * scaleY;
			const endX = (measurement.endImage.x - viewportState.x) * viewportState.zoom * scaleX;
			const endY = (measurement.endImage.y - viewportState.y) * viewportState.zoom * scaleY;

			// Draw measurement line
			const measureColor = rgbaToCss(options.measurementOptions.color);
			ctx.strokeStyle = measureColor;
			ctx.lineWidth = options.measurementOptions.thickness * scaleX;
			ctx.lineCap = options.measurementOptions.lineCap;

			// Set line style
			if (options.measurementOptions.lineStyle === 'dashed') {
				ctx.setLineDash([
					options.measurementOptions.dashLength * scaleX,
					options.measurementOptions.dashSpacing * scaleX
				]);
			} else if (options.measurementOptions.lineStyle === 'dotted') {
				ctx.setLineDash([
					options.measurementOptions.thickness * scaleX,
					options.measurementOptions.dotSpacing * scaleX
				]);
			} else {
				ctx.setLineDash([]);
			}

			ctx.beginPath();
			ctx.moveTo(startX, startY);
			ctx.lineTo(endX, endY);
			ctx.stroke();
			ctx.setLineDash([]);

			// Draw end points
			ctx.fillStyle = measureColor;
			const pointRadius = 4 * scaleX;

			ctx.beginPath();
			ctx.arc(startX, startY, pointRadius, 0, Math.PI * 2);
			ctx.fill();

			ctx.beginPath();
			ctx.arc(endX, endY, pointRadius, 0, Math.PI * 2);
			ctx.fill();

			// Calculate distance and draw label
			const dx = measurement.endImage.x - measurement.startImage.x;
			const dy = measurement.endImage.y - measurement.startImage.y;
			const distancePixels = Math.sqrt(dx * dx + dy * dy);
			const distanceMicrons = distancePixels * micronsPerPixel;
			const displayText = formatMeasurementDistance(distanceMicrons);

			// Label position (midpoint)
			const midX = (startX + endX) / 2;
			const midY = (startY + endY) / 2;

			// Draw label background
			const fontSize = options.measurementOptions.fontSize * scaleX;
			ctx.font = `${fontSize}px system-ui, -apple-system, sans-serif`;
			const textMetrics = ctx.measureText(displayText);
			const textWidth = textMetrics.width + 12 * scaleX;
			const textHeight = fontSize * 1.5 + 4 * scaleX;

			ctx.fillStyle = 'rgba(0, 0, 0, 0.75)';
			ctx.beginPath();
			ctx.roundRect(
				midX - textWidth / 2,
				midY - textHeight / 2 - 15 * scaleY,
				textWidth,
				textHeight,
				4 * scaleX
			);
			ctx.fill();

			// Draw label text
			ctx.fillStyle = 'white';
			ctx.textAlign = 'center';
			ctx.textBaseline = 'middle';
			ctx.fillText(displayText, midX, midY - 15 * scaleY);
		}

		return compositeCanvas;
	}

	// Format measurement distance (similar to MeasurementOverlay)
	function formatMeasurementDistance(microns: number): string {
		const units = $settings.measurements?.units ?? 'um';
		switch (units) {
			case 'um':
				if (microns >= 1000) {
					return `${(microns / 1000).toFixed(microns >= 10000 ? 0 : 1)} mm`;
				}
				return `${microns.toFixed(1)} µm`;
			case 'mm':
				return `${(microns / 1000).toFixed(microns >= 1000 ? 1 : 3)} mm`;
			case 'in':
				const inches = microns / 25400;
				if (inches >= 0.1) {
					return `${inches.toFixed(2)} in`;
				}
				return `${(inches * 1000).toFixed(1)} mil`;
			default:
				return `${microns.toFixed(1)} µm`;
		}
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

	function handleReset() {
		exportStore.resetOptions();
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

	// Measurement toggle
	function handleMeasurementToggle() {
		exportStore.updateOptions({ showMeasurement: !options.showMeasurement });
	}

	// Measurement option handlers
	function handleMeasurementColorChange(event: Event) {
		const target = event.target as HTMLInputElement;
		const hex = target.value;
		const r = parseInt(hex.slice(1, 3), 16);
		const g = parseInt(hex.slice(3, 5), 16);
		const b = parseInt(hex.slice(5, 7), 16);
		exportStore.updateOptions({
			measurementOptions: {
				...options.measurementOptions,
				color: { ...options.measurementOptions.color, r, g, b }
			}
		});
	}

	function handleMeasurementAlphaChange(event: Event) {
		const target = event.target as HTMLInputElement;
		const a = Math.max(0, Math.min(100, parseInt(target.value, 10) || 0)) / 100;
		exportStore.updateOptions({
			measurementOptions: {
				...options.measurementOptions,
				color: { ...options.measurementOptions.color, a }
			}
		});
	}

	function handleMeasurementThicknessChange(event: Event) {
		const target = event.target as HTMLInputElement;
		const thickness = Math.max(1, Math.min(10, parseInt(target.value, 10) || 2));
		exportStore.updateOptions({
			measurementOptions: { ...options.measurementOptions, thickness }
		});
	}

	function handleMeasurementStyleChange(event: Event) {
		const target = event.target as HTMLSelectElement;
		const lineStyle = target.value as LineStyle;
		exportStore.updateOptions({
			measurementOptions: { ...options.measurementOptions, lineStyle }
		});
	}

	function handleMeasurementCapChange(event: Event) {
		const target = event.target as HTMLSelectElement;
		const lineCap = target.value as LineCap;
		exportStore.updateOptions({
			measurementOptions: { ...options.measurementOptions, lineCap }
		});
	}

	function handleMeasurementDashLengthChange(event: Event) {
		const target = event.target as HTMLInputElement;
		const dashLength = Math.max(1, Math.min(50, parseInt(target.value, 10) || 8));
		exportStore.updateOptions({
			measurementOptions: { ...options.measurementOptions, dashLength }
		});
	}

	function handleMeasurementDashSpacingChange(event: Event) {
		const target = event.target as HTMLInputElement;
		const dashSpacing = Math.max(1, Math.min(50, parseInt(target.value, 10) || 4));
		exportStore.updateOptions({
			measurementOptions: { ...options.measurementOptions, dashSpacing }
		});
	}

	function handleMeasurementDotSpacingChange(event: Event) {
		const target = event.target as HTMLInputElement;
		const dotSpacing = Math.max(1, Math.min(50, parseInt(target.value, 10) || 4));
		exportStore.updateOptions({
			measurementOptions: { ...options.measurementOptions, dotSpacing }
		});
	}

	function handleMeasurementFontSizeChange(event: Event) {
		const target = event.target as HTMLInputElement;
		const value = target.value.trim();
		if (value === '') {
			// Allow empty during editing, will be filled with default on blur
			return;
		}
		const parsed = parseInt(value, 10);
		if (!isNaN(parsed)) {
			const fontSize = Math.max(6, Math.min(128, parsed));
			exportStore.updateOptions({
				measurementOptions: { ...options.measurementOptions, fontSize }
			});
		}
	}

	function handleMeasurementFontSizeBlur(event: Event) {
		const target = event.target as HTMLInputElement;
		const value = target.value.trim();
		if (value === '') {
			// Default to 20 when empty
			exportStore.updateOptions({
				measurementOptions: { ...options.measurementOptions, fontSize: 20 }
			});
		} else {
			const parsed = parseInt(value, 10);
			const fontSize = Math.max(6, Math.min(128, isNaN(parsed) ? 20 : parsed));
			exportStore.updateOptions({
				measurementOptions: { ...options.measurementOptions, fontSize }
			});
		}
	}

	// ROI outline handlers
	function handleRoiOutlineToggle() {
		exportStore.updateOptions({
			roiOutline: { ...options.roiOutline, enabled: !options.roiOutline.enabled }
		});
	}

	function handleRoiOutlineColorChange(event: Event) {
		const target = event.target as HTMLInputElement;
		const hex = target.value;
		const r = parseInt(hex.slice(1, 3), 16);
		const g = parseInt(hex.slice(3, 5), 16);
		const b = parseInt(hex.slice(5, 7), 16);
		exportStore.updateOptions({
			roiOutline: { ...options.roiOutline, color: { ...options.roiOutline.color, r, g, b } }
		});
	}

	function handleRoiOutlineAlphaChange(event: Event) {
		const target = event.target as HTMLInputElement;
		const a = Math.max(0, Math.min(100, parseInt(target.value, 10) || 0)) / 100;
		exportStore.updateOptions({
			roiOutline: { ...options.roiOutline, color: { ...options.roiOutline.color, a } }
		});
	}

	function handleRoiOutlineThicknessChange(event: Event) {
		const target = event.target as HTMLInputElement;
		const thickness = Math.max(1, Math.min(10, parseInt(target.value, 10) || 2));
		exportStore.updateOptions({
			roiOutline: { ...options.roiOutline, thickness }
		});
	}

	function handleRoiOutlineStyleChange(event: Event) {
		const target = event.target as HTMLSelectElement;
		const lineStyle = target.value as LineStyle;
		exportStore.updateOptions({
			roiOutline: { ...options.roiOutline, lineStyle }
		});
	}

	function handleRoiOutlineCapChange(event: Event) {
		const target = event.target as HTMLSelectElement;
		const lineCap = target.value as LineCap;
		exportStore.updateOptions({
			roiOutline: { ...options.roiOutline, lineCap }
		});
	}

	function handleRoiOutlineDashLengthChange(event: Event) {
		const target = event.target as HTMLInputElement;
		const dashLength = Math.max(1, Math.min(50, parseInt(target.value, 10) || 8));
		exportStore.updateOptions({
			roiOutline: { ...options.roiOutline, dashLength }
		});
	}

	function handleRoiOutlineDashSpacingChange(event: Event) {
		const target = event.target as HTMLInputElement;
		const dashSpacing = Math.max(1, Math.min(50, parseInt(target.value, 10) || 4));
		exportStore.updateOptions({
			roiOutline: { ...options.roiOutline, dashSpacing }
		});
	}

	function handleRoiOutlineDotSpacingChange(event: Event) {
		const target = event.target as HTMLInputElement;
		const dotSpacing = Math.max(1, Math.min(50, parseInt(target.value, 10) || 4));
		exportStore.updateOptions({
			roiOutline: { ...options.roiOutline, dotSpacing }
		});
	}

	// ROI overlay handlers
	function handleRoiOverlayToggle() {
		exportStore.updateOptions({
			roiOverlay: { ...options.roiOverlay, enabled: !options.roiOverlay.enabled }
		});
	}

	function handleRoiOverlayColorChange(event: Event) {
		const target = event.target as HTMLInputElement;
		const hex = target.value;
		const r = parseInt(hex.slice(1, 3), 16);
		const g = parseInt(hex.slice(3, 5), 16);
		const b = parseInt(hex.slice(5, 7), 16);
		exportStore.updateOptions({
			roiOverlay: { ...options.roiOverlay, color: { ...options.roiOverlay.color, r, g, b } }
		});
	}

	function handleRoiOverlayAlphaChange(event: Event) {
		const target = event.target as HTMLInputElement;
		const a = Math.max(0, Math.min(100, parseInt(target.value, 10) || 0)) / 100;
		exportStore.updateOptions({
			roiOverlay: { ...options.roiOverlay, color: { ...options.roiOverlay.color, a } }
		});
	}

	// Helper to convert RGBA to hex (for color picker)
	function rgbaToHex(color: RgbaColor): string {
		const r = color.r.toString(16).padStart(2, '0');
		const g = color.g.toString(16).padStart(2, '0');
		const b = color.b.toString(16).padStart(2, '0');
		return `#${r}${g}${b}`;
	}

	// Helper to convert RGBA to CSS string
	function rgbaToCss(color: RgbaColor): string {
		return `rgba(${color.r}, ${color.g}, ${color.b}, ${color.a})`;
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

			<!-- Preview Section (fixed, no scroll) -->
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
							<svg
								xmlns="http://www.w3.org/2000/svg"
								width="48"
								height="48"
								viewBox="0 0 24 24"
								fill="none"
								stroke="currentColor"
								stroke-width="1.5"
							>
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
						<svg
							xmlns="http://www.w3.org/2000/svg"
							width="14"
							height="14"
							viewBox="0 0 24 24"
							fill="none"
							stroke="currentColor"
							stroke-width="2"
						>
							<rect x="3" y="3" width="18" height="18" rx="2" ry="2"></rect>
						</svg>
					</span>
					<span class="dimensions-text">{exportWidth} × {exportHeight} px</span>
				</div>
			</div>

			<!-- Settings Section (scrollable) -->
			<div class="settings-scrollable">
				<div class="options-section">
					<div class="section-label">Export Options</div>

					<!-- DPI Control -->
					<div class="option-group">
						<div class="option-label-standalone">
							<svg
								xmlns="http://www.w3.org/2000/svg"
								width="16"
								height="16"
								viewBox="0 0 24 24"
								fill="none"
								stroke="currentColor"
								stroke-width="2"
							>
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
								onclick={() => {
									dpiInputValue = '72';
									exportStore.updateOptions({ dpi: 72 });
								}}
								type="button">72</button
							>
							<button
								class="preset-btn"
								class:active={options.dpi === 96}
								onclick={() => {
									dpiInputValue = '96';
									exportStore.updateOptions({ dpi: 96 });
								}}
								type="button">96</button
							>
							<button
								class="preset-btn"
								class:active={options.dpi === 150}
								onclick={() => {
									dpiInputValue = '150';
									exportStore.updateOptions({ dpi: 150 });
								}}
								type="button">150</button
							>
							<button
								class="preset-btn"
								class:active={options.dpi === 300}
								onclick={() => {
									dpiInputValue = '300';
									exportStore.updateOptions({ dpi: 300 });
								}}
								type="button">300</button
							>
						</div>
					</div>

					<!-- Annotations Toggle -->
					<div class="option-group">
						<label class="option-row toggle-option">
							<div class="option-label">
								<svg
									xmlns="http://www.w3.org/2000/svg"
									width="16"
									height="16"
									viewBox="0 0 24 24"
									fill="none"
									stroke="currentColor"
									stroke-width="2"
								>
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

					<!-- Measurement Section -->
					<div class="section-label" style="margin-top: 8px;">Measurement</div>
					<div
						class="option-group measurement-section"
						class:section-disabled={!hasMeasurement}
						title={!hasMeasurement
							? 'No measurement set. Use the measurement tool to add one.'
							: ''}
					>
						<label class="option-row toggle-option">
							<div class="option-label">
								<svg
									xmlns="http://www.w3.org/2000/svg"
									width="16"
									height="16"
									viewBox="0 0 24 24"
									fill="none"
									stroke="currentColor"
									stroke-width="2"
								>
									<path
										d="M21.3 8.7 8.7 21.3c-1 1-2.5 1-3.4 0l-2.6-2.6c-1-1-1-2.5 0-3.4L15.3 2.7c1-1 2.5-1 3.4 0l2.6 2.6c1 1 1 2.5 0 3.4Z"
									></path>
									<path d="m7.5 10.5 2 2"></path>
									<path d="m10.5 7.5 2 2"></path>
								</svg>
								<span>Show Measurement</span>
							</div>
							<button
								class="toggle-switch"
								class:active={options.showMeasurement && hasMeasurement}
								onclick={handleMeasurementToggle}
								aria-pressed={options.showMeasurement}
								aria-label="Toggle show measurement"
								type="button"
								disabled={!hasMeasurement}
							>
								<span class="toggle-track">
									<span class="toggle-thumb"></span>
								</span>
							</button>
						</label>

						<!-- Measurement sub-options -->
						<div class="sub-options" class:disabled={!options.showMeasurement || !hasMeasurement}>
							<!-- Color picker -->
							<div class="sub-option-row">
								<span class="sub-option-label">Color</span>
								<div class="color-picker-wrapper">
									<input
										type="color"
										class="color-picker"
										value={rgbaToHex(options.measurementOptions.color)}
										oninput={(e) => handleMeasurementColorChange(e)}
										disabled={!options.showMeasurement || !hasMeasurement}
									/>
									<input
										type="number"
										class="alpha-input"
										min="0"
										max="100"
										step="5"
										value={Math.round(options.measurementOptions.color.a * 100)}
										oninput={(e) => handleMeasurementAlphaChange(e)}
										disabled={!options.showMeasurement || !hasMeasurement}
										title="Opacity %"
									/>
									<span class="alpha-label">%</span>
								</div>
							</div>

							<!-- Thickness -->
							<div class="sub-option-row">
								<span class="sub-option-label">Thickness</span>
								<input
									type="number"
									class="thickness-input"
									min="1"
									max="10"
									step="1"
									value={options.measurementOptions.thickness}
									oninput={(e) => handleMeasurementThicknessChange(e)}
									disabled={!options.showMeasurement || !hasMeasurement}
								/>
							</div>

							<!-- Stroke style -->
							<div class="sub-option-row">
								<span class="sub-option-label">Stroke Style</span>
								<div class="stroke-style-wrapper">
									<select
										class="line-style-select"
										value={options.measurementOptions.lineStyle}
										onchange={(e) => handleMeasurementStyleChange(e)}
										disabled={!options.showMeasurement || !hasMeasurement}
									>
										<option value="solid">Solid</option>
										<option value="dashed">Dashed</option>
										<option value="dotted">Dotted</option>
									</select>
									{#if options.measurementOptions.lineStyle === 'dashed'}
										<input
											type="number"
											class="stroke-param-input"
											min="1"
											max="50"
											step="1"
											value={options.measurementOptions.dashLength}
											oninput={(e) => handleMeasurementDashLengthChange(e)}
											disabled={!options.showMeasurement || !hasMeasurement}
											title="Dash length"
										/>
										<input
											type="number"
											class="stroke-param-input"
											min="1"
											max="50"
											step="1"
											value={options.measurementOptions.dashSpacing}
											oninput={(e) => handleMeasurementDashSpacingChange(e)}
											disabled={!options.showMeasurement || !hasMeasurement}
											title="Dash spacing"
										/>
									{:else if options.measurementOptions.lineStyle === 'dotted'}
										<input
											type="number"
											class="stroke-param-input"
											min="1"
											max="50"
											step="1"
											value={options.measurementOptions.dotSpacing}
											oninput={(e) => handleMeasurementDotSpacingChange(e)}
											disabled={!options.showMeasurement || !hasMeasurement}
											title="Dot spacing"
										/>
									{/if}
								</div>
							</div>

							<!-- Cap style -->
							<div class="sub-option-row">
								<span class="sub-option-label">Cap Style</span>
								<select
									class="line-style-select"
									value={options.measurementOptions.lineCap}
									onchange={(e) => handleMeasurementCapChange(e)}
									disabled={!options.showMeasurement || !hasMeasurement}
								>
									<option value="round">Round</option>
									<option value="square">Square</option>
									<option value="butt">Flat</option>
								</select>
							</div>

							<!-- Font size -->
							<div class="sub-option-row">
								<span class="sub-option-label">Font Size</span>
								<input
									type="number"
									class="thickness-input"
									min="6"
									max="128"
									step="1"
									value={options.measurementOptions.fontSize}
									oninput={(e) => handleMeasurementFontSizeChange(e)}
									onblur={(e) => handleMeasurementFontSizeBlur(e)}
									disabled={!options.showMeasurement || !hasMeasurement}
								/>
							</div>
						</div>
					</div>

					<!-- Region of Interest Section -->
					<div class="section-label" style="margin-top: 8px;">Region of Interest</div>

					<!-- ROI Outline -->
					<div
						class="option-group roi-section"
						class:section-disabled={!hasRoi}
						title={!hasRoi ? 'No ROI set. Use the ROI tool to define a region.' : ''}
					>
						<label class="option-row toggle-option">
							<div class="option-label">
								<svg
									xmlns="http://www.w3.org/2000/svg"
									width="16"
									height="16"
									viewBox="0 0 24 24"
									fill="none"
									stroke="currentColor"
									stroke-width="2"
								>
									<rect
										x="3"
										y="3"
										width="18"
										height="18"
										rx="2"
										ry="2"
										stroke="yellow"
										stroke-dasharray="1 4"
									></rect>
								</svg>
								<span>Show Outline</span>
							</div>
							<button
								class="toggle-switch"
								class:active={options.roiOutline.enabled && hasRoi}
								onclick={handleRoiOutlineToggle}
								aria-pressed={options.roiOutline.enabled}
								aria-label="Toggle ROI outline"
								type="button"
								disabled={!hasRoi}
							>
								<span class="toggle-track">
									<span class="toggle-thumb"></span>
								</span>
							</button>
						</label>

						<!-- Outline sub-options -->
						<div class="sub-options" class:disabled={!options.roiOutline.enabled || !hasRoi}>
							<!-- Color picker -->
							<div class="sub-option-row">
								<span class="sub-option-label">Color</span>
								<div class="color-picker-wrapper">
									<input
										type="color"
										class="color-picker"
										value={rgbaToHex(options.roiOutline.color)}
										oninput={(e) => handleRoiOutlineColorChange(e)}
										disabled={!options.roiOutline.enabled || !hasRoi}
									/>
									<input
										type="number"
										class="alpha-input"
										min="0"
										max="100"
										step="5"
										value={Math.round(options.roiOutline.color.a * 100)}
										oninput={(e) => handleRoiOutlineAlphaChange(e)}
										disabled={!options.roiOutline.enabled || !hasRoi}
										title="Opacity %"
									/>
									<span class="alpha-label">%</span>
								</div>
							</div>

							<!-- Thickness -->
							<div class="sub-option-row">
								<span class="sub-option-label">Thickness</span>
								<input
									type="number"
									class="thickness-input"
									min="1"
									max="10"
									step="1"
									value={options.roiOutline.thickness}
									oninput={(e) => handleRoiOutlineThicknessChange(e)}
									disabled={!options.roiOutline.enabled || !hasRoi}
								/>
							</div>

							<!-- Line style -->
							<div class="sub-option-row">
								<span class="sub-option-label">Stroke Style</span>
								<div class="stroke-style-wrapper">
									<select
										class="line-style-select"
										value={options.roiOutline.lineStyle}
										onchange={(e) => handleRoiOutlineStyleChange(e)}
										disabled={!options.roiOutline.enabled || !hasRoi}
									>
										<option value="solid">Solid</option>
										<option value="dashed">Dashed</option>
										<option value="dotted">Dotted</option>
									</select>
									{#if options.roiOutline.lineStyle === 'dashed'}
										<input
											type="number"
											class="stroke-param-input"
											min="1"
											max="50"
											step="1"
											value={options.roiOutline.dashLength}
											oninput={(e) => handleRoiOutlineDashLengthChange(e)}
											disabled={!options.roiOutline.enabled || !hasRoi}
											title="Dash length"
										/>
										<input
											type="number"
											class="stroke-param-input"
											min="1"
											max="50"
											step="1"
											value={options.roiOutline.dashSpacing}
											oninput={(e) => handleRoiOutlineDashSpacingChange(e)}
											disabled={!options.roiOutline.enabled || !hasRoi}
											title="Dash spacing"
										/>
									{:else if options.roiOutline.lineStyle === 'dotted'}
										<input
											type="number"
											class="stroke-param-input"
											min="1"
											max="50"
											step="1"
											value={options.roiOutline.dotSpacing}
											oninput={(e) => handleRoiOutlineDotSpacingChange(e)}
											disabled={!options.roiOutline.enabled || !hasRoi}
											title="Dot spacing"
										/>
									{/if}
								</div>
							</div>

							<!-- Cap style -->
							<div class="sub-option-row">
								<span class="sub-option-label">Cap Style</span>
								<select
									class="line-style-select"
									value={options.roiOutline.lineCap}
									onchange={(e) => handleRoiOutlineCapChange(e)}
									disabled={!options.roiOutline.enabled || !hasRoi}
								>
									<option value="round">Round</option>
									<option value="square">Square</option>
									<option value="butt">Flat</option>
								</select>
							</div>
						</div>
					</div>

					<!-- Outside Overlay -->
					<div
						class="option-group roi-section"
						class:section-disabled={!hasRoi}
						title={!hasRoi ? 'No ROI set. Use the ROI tool to define a region.' : ''}
					>
						<label class="option-row toggle-option">
							<div class="option-label">
								<svg
									xmlns="http://www.w3.org/2000/svg"
									width="16"
									height="16"
									viewBox="0 0 24 24"
									fill="none"
									stroke="currentColor"
									stroke-width="2"
								>
									<rect x="3" y="3" width="18" height="18" rx="2" ry="2"></rect>
									<path d="M3 3l18 18"></path>
								</svg>
								<span>Outside Overlay</span>
							</div>
							<button
								class="toggle-switch"
								class:active={options.roiOverlay.enabled && hasRoi}
								onclick={handleRoiOverlayToggle}
								aria-pressed={options.roiOverlay.enabled}
								aria-label="Toggle outside overlay"
								type="button"
								disabled={!hasRoi}
							>
								<span class="toggle-track">
									<span class="toggle-thumb"></span>
								</span>
							</button>
						</label>

						<!-- Overlay color -->
						<div class="sub-options" class:disabled={!options.roiOverlay.enabled || !hasRoi}>
							<div class="sub-option-row">
								<span class="sub-option-label">Color</span>
								<div class="color-picker-wrapper">
									<input
										type="color"
										class="color-picker"
										value={rgbaToHex(options.roiOverlay.color)}
										oninput={(e) => handleRoiOverlayColorChange(e)}
										disabled={!options.roiOverlay.enabled || !hasRoi}
									/>
									<input
										type="number"
										class="alpha-input"
										min="0"
										max="100"
										step="5"
										value={Math.round(options.roiOverlay.color.a * 100)}
										oninput={(e) => handleRoiOverlayAlphaChange(e)}
										disabled={!options.roiOverlay.enabled || !hasRoi}
										title="Opacity %"
									/>
									<span class="alpha-label">%</span>
								</div>
							</div>
						</div>
					</div>

					<!-- Format Selection -->
					<div class="option-group">
						<div class="option-label-standalone">
							<svg
								xmlns="http://www.w3.org/2000/svg"
								width="16"
								height="16"
								viewBox="0 0 24 24"
								fill="none"
								stroke="currentColor"
								stroke-width="2"
							>
								<path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z"></path>
								<polyline points="14 2 14 8 20 8"></polyline>
							</svg>
							<span>Format</span>
						</div>
						<div class="format-buttons">
							<button
								class="format-btn"
								class:active={options.format === 'jpeg'}
								onclick={() => handleFormatChange('jpeg')}
								type="button"
							>
								<span class="format-name">JPEG</span>
								<span class="format-desc">Smaller file size</span>
							</button>
							<button
								class="format-btn"
								class:active={options.format === 'png'}
								onclick={() => handleFormatChange('png')}
								type="button"
							>
								<span class="format-name">PNG</span>
								<span class="format-desc">Lossless, transparent</span>
							</button>
						</div>
					</div>

					<!-- Quality Slider (JPEG only) -->
					{#if options.format === 'jpeg'}
						<div class="option-group quality-group">
							<label class="option-label-standalone" for="quality-slider">
								<svg
									xmlns="http://www.w3.org/2000/svg"
									width="16"
									height="16"
									viewBox="0 0 24 24"
									fill="none"
									stroke="currentColor"
									stroke-width="2"
								>
									<circle cx="12" cy="12" r="3"></circle>
									<path
										d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 0 1 0 2.83 2 2 0 0 1-2.83 0l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 0 1-2 2 2 2 0 0 1-2-2v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 0 1-2.83 0 2 2 0 0 1 0-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 0 1-2-2 2 2 0 0 1 2-2h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 0 1 0-2.83 2 2 0 0 1 2.83 0l.06.06a1.65 1.65 0 0 0 1.82.33H9a1.65 1.65 0 0 0 1-1.51V3a2 2 0 0 1 2-2 2 2 0 0 1 2 2v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 0 1 2.83 0 2 2 0 0 1 0 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82V9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 0 1 2 2 2 2 0 0 1-2 2h-.09a1.65 1.65 0 0 0-1.51 1z"
									></path>
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
				<button class="btn btn-tertiary" onclick={handleReset} type="button"> Reset </button>
				<div class="footer-right">
					<button class="btn btn-secondary" onclick={closeModal} type="button"> Cancel </button>
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
							<svg
								xmlns="http://www.w3.org/2000/svg"
								width="16"
								height="16"
								viewBox="0 0 24 24"
								fill="none"
								stroke="currentColor"
								stroke-width="2"
							>
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
		cursor: default;
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
		cursor: default;
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

	/* Preview Section (fixed, outside scroll) */
	.preview-section {
		flex: 0 0 auto;
		display: flex;
		flex-direction: column;
		padding: 16px 20px;
		border-bottom: 1px solid rgba(255, 255, 255, 0.08);
	}

	/* Settings scrollable area */
	.settings-scrollable {
		display: flex;
		flex-direction: column;
		gap: 16px;
		padding: 16px 20px;
		overflow-y: auto;
		flex: 1;
		min-height: 0;
		scrollbar-width: thin;
		scrollbar-color: #444 transparent;
		background: rgba(0, 0, 0, 0.5);
	}

	.settings-scrollable::-webkit-scrollbar {
		width: 9px;
	}

	.settings-scrollable::-webkit-scrollbar-track {
		background: transparent;
	}

	.settings-scrollable::-webkit-scrollbar-thumb {
		background: #444;
		border-radius: 3px;
	}

	.settings-scrollable::-webkit-scrollbar-thumb:hover {
		background: #666;
	}

	.section-label {
		font-size: 11px;
		font-weight: 600;
		text-transform: uppercase;
		letter-spacing: 0.05em;
		color: rgba(255, 255, 255, 0.45);
		margin-bottom: 10px;
	}

	/* Preview Container */

	.preview-container {
		display: flex;
		align-items: center;
		justify-content: center;
		height: 200px;
		/* Crosshatch pattern to indicate non-export area */
		background-color: #1a1a1a;
		background-image:
			repeating-linear-gradient(
				45deg,
				transparent,
				transparent 8px,
				rgba(255, 255, 255, 0.03) 8px,
				rgba(255, 255, 255, 0.03) 9px
			),
			repeating-linear-gradient(
				-45deg,
				transparent,
				transparent 8px,
				rgba(255, 255, 255, 0.03) 8px,
				rgba(255, 255, 255, 0.03) 9px
			);
		border: 1px solid rgba(255, 255, 255, 0.08);
		border-radius: 8px;
		overflow: hidden;
	}

	.preview-image {
		max-width: 100%;
		max-height: 200px;
		object-fit: contain;
		/* Add subtle shadow to separate from crosshatch background */
		box-shadow: 0 2px 8px rgba(0, 0, 0, 0.4);
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
		background: var(--secondary-muted);
		border-color: var(--secondary-hex);
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
		background: var(--secondary-hex);
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
		background: var(--secondary-muted);
		border-color: var(--secondary-hex);
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
		justify-content: space-between;
		align-items: center;
		gap: 10px;
		padding: 16px 20px;
		border-top: 1px solid rgba(255, 255, 255, 0.08);
		background: rgba(255, 255, 255, 0.02);
	}

	.footer-right {
		display: flex;
		gap: 10px;
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
		background: var(--primary-hex);
		color: #fff;
	}

	.btn-primary:hover:not(:disabled) {
		background: var(--primary-hover);
	}

	.btn-primary:active:not(:disabled) {
		transform: scale(0.98);
	}

	.btn-primary:disabled {
		opacity: 0.6;
		cursor: not-allowed;
	}

	.btn-tertiary {
		background: transparent;
		color: rgba(255, 255, 255, 0.5);
		border: 1px solid rgba(255, 255, 255, 0.15);
	}

	.btn-tertiary:hover {
		background: rgba(255, 255, 255, 0.05);
		color: rgba(255, 255, 255, 0.7);
		border-color: rgba(255, 255, 255, 0.25);
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

		.preview-section {
			padding: 12px 16px;
		}

		.settings-scrollable {
			padding: 12px 16px;
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

	/* ROI Section Styles */
	.roi-section {
		margin-left: 0;
	}

	.sub-options {
		display: flex;
		flex-direction: column;
		gap: 8px;
		margin-top: 10px;
		padding-left: 28px;
		transition: opacity 0.2s;
	}

	.sub-options.disabled {
		opacity: 0.4;
		pointer-events: none;
	}

	.section-disabled {
		opacity: 0.5;
	}

	.section-disabled .toggle-switch {
		cursor: not-allowed;
	}

	.stroke-style-wrapper {
		display: flex;
		align-items: center;
		gap: 6px;
	}

	.stroke-param-input {
		width: 48px;
		padding: 4px 6px;
		border: 1px solid rgba(255, 255, 255, 0.15);
		border-radius: 4px;
		background: rgba(0, 0, 0, 0.3);
		color: white;
		font-size: 12px;
		font-family: 'SF Mono', 'Monaco', 'Inconsolata', 'Fira Code', monospace;
		text-align: center;
		appearance: textfield;
		-moz-appearance: textfield;
	}

	.stroke-param-input::-webkit-outer-spin-button,
	.stroke-param-input::-webkit-inner-spin-button {
		-webkit-appearance: none;
		margin: 0;
	}

	.stroke-param-input:hover:not(:disabled) {
		border-color: rgba(255, 255, 255, 0.25);
	}

	.stroke-param-input:focus {
		outline: none;
		border-color: var(--secondary-hex);
	}

	.stroke-param-input:disabled {
		opacity: 0.5;
		cursor: not-allowed;
	}

	.sub-option-row {
		display: flex;
		align-items: center;
		gap: 10px;
	}

	.sub-option-label {
		font-size: 12px;
		color: rgba(255, 255, 255, 0.7);
		min-width: 65px;
	}

	.color-picker-wrapper {
		display: flex;
		align-items: center;
		gap: 6px;
	}

	.color-picker {
		width: 28px;
		height: 24px;
		padding: 0;
		border: 1px solid rgba(255, 255, 255, 0.15);
		border-radius: 4px;
		background: transparent;
		cursor: pointer;
	}

	.color-picker::-webkit-color-swatch-wrapper {
		padding: 2px;
	}

	.color-picker::-webkit-color-swatch {
		border-radius: 2px;
		border: none;
	}

	.color-picker:disabled {
		opacity: 0.5;
		cursor: not-allowed;
	}

	.alpha-input {
		width: 56px;
		padding: 6px 8px;
		font-size: 13px;
		font-family: ui-monospace, 'SF Mono', 'Cascadia Code', 'Segoe UI Mono', monospace;
		background: rgba(255, 255, 255, 0.06);
		border: 1px solid rgba(255, 255, 255, 0.12);
		border-radius: 6px;
		color: #fff;
		text-align: center;
		outline: none;
		transition:
			border-color 0.15s,
			background-color 0.15s;
	}

	.alpha-input:hover:not(:disabled) {
		background: rgba(255, 255, 255, 0.08);
		border-color: rgba(255, 255, 255, 0.2);
	}

	.alpha-input:focus {
		border-color: var(--secondary-hex);
		background: var(--secondary-muted);
	}

	.alpha-input:disabled {
		opacity: 0.5;
		cursor: not-allowed;
	}

	/* Hide spinner arrows for number inputs */
	.alpha-input::-webkit-outer-spin-button,
	.alpha-input::-webkit-inner-spin-button {
		-webkit-appearance: none;
		margin: 0;
	}

	.alpha-input[type='number'] {
		appearance: textfield;
		-moz-appearance: textfield;
	}

	.alpha-label {
		font-size: 12px;
		color: rgba(255, 255, 255, 0.5);
	}

	.thickness-input {
		width: 54px;
		padding: 6px 8px;
		font-size: 13px;
		font-family: ui-monospace, 'SF Mono', 'Cascadia Code', 'Segoe UI Mono', monospace;
		background: rgba(255, 255, 255, 0.06);
		border: 1px solid rgba(255, 255, 255, 0.12);
		border-radius: 6px;
		color: #fff;
		text-align: center;
		outline: none;
		transition:
			border-color 0.15s,
			background-color 0.15s;
	}

	.thickness-input:hover:not(:disabled) {
		background: rgba(255, 255, 255, 0.08);
		border-color: rgba(255, 255, 255, 0.2);
	}

	.thickness-input:focus {
		border-color: var(--secondary-hex);
		background: var(--secondary-muted);
	}

	.thickness-input:disabled {
		opacity: 0.5;
		cursor: not-allowed;
	}

	/* Hide spinner arrows for number inputs */
	.thickness-input::-webkit-outer-spin-button,
	.thickness-input::-webkit-inner-spin-button {
		-webkit-appearance: none;
		margin: 0;
	}

	.thickness-input[type='number'] {
		appearance: textfield;
		-moz-appearance: textfield;
	}

	.line-style-select {
		padding: 6px 10px;
		font-size: 13px;
		background: rgba(255, 255, 255, 0.06);
		border: 1px solid rgba(255, 255, 255, 0.12);
		border-radius: 6px;
		color: #fff;
		cursor: pointer;
		min-width: 90px;
		outline: none;
		transition:
			border-color 0.15s,
			background-color 0.15s;
	}

	.line-style-select:hover:not(:disabled) {
		background: rgba(255, 255, 255, 0.08);
		border-color: rgba(255, 255, 255, 0.2);
	}

	.line-style-select:focus {
		border-color: var(--primary-hex);
	}

	.line-style-select:disabled {
		opacity: 0.5;
		cursor: not-allowed;
	}

	.line-style-select option {
		background: #1a1a1a;
		color: white;
	}
</style>
