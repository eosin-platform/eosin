<script lang="ts">
  import { onDestroy, tick } from 'svelte';
  import { settings } from '$lib/stores/settings';
  import { annotationStore, getLayerColor } from '$lib/stores/annotations';
  import { authStore } from '$lib/stores/auth';
  import type { Annotation, AnnotationKind, AnnotationSet, PointGeometry, EllipseGeometry, PolygonGeometry, MaskGeometry } from '$lib/api/annotations';

  // Cache for decoded mask data - avoids re-decoding base64 every frame
  const maskDataCache = new Map<string, Uint8Array>();

  interface Props {
    /** Slide ID for this overlay - filters annotations to this slide */
    slideId: string | null;
    /** Viewport state */
    viewportX: number;
    viewportY: number;
    viewportZoom: number;
    /** Container dimensions */
    containerWidth: number;
    containerHeight: number;
    /** Callback when an annotation is clicked */
    onAnnotationClick?: (annotation: Annotation, screenX: number, screenY: number) => void;
    /** Callback when an annotation is right-clicked */
    onAnnotationRightClick?: (annotation: Annotation, screenX: number, screenY: number) => void;
    /** Modification mode state */
    modifyPhase?: 'idle' | 'point-position' | 'multi-point' | 'ellipse-center' | 'ellipse-radii' | 'ellipse-angle' | 'polygon-vertices' | 'polygon-freehand' | 'polygon-edit' | 'mask-paint';
    modifyAnnotationId?: string | null;
    modifyCenter?: { x: number; y: number } | null;
    modifyRadii?: { rx: number; ry: number } | null;
    modifyMousePos?: { x: number; y: number } | null;
    modifyAngleOffset?: number;
    modifyRotation?: number;
    modifyCenterOffset?: { x: number; y: number } | null;
    modifyIsCreating?: boolean;
    modifyOriginalRadii?: { rx: number; ry: number } | null;
    modifyDragStartPos?: { x: number; y: number } | null;
    /** Polygon modification state */
    modifyPolygonVertices?: Array<{ x: number; y: number }> | null;
    modifyFreehandPath?: Array<{ x: number; y: number }> | null;
    modifyEditingVertexIndex?: number | null;
    /** Mask painting state - multi-tile support */
    maskPaintData?: Uint8Array | null;
    maskTileOrigin?: { x: number; y: number } | null;
    maskAllTiles?: Array<{ origin: { x: number; y: number }; data: Uint8Array }>;
    maskBrushSize?: number;
  }

  let { 
    slideId,
    viewportX, viewportY, viewportZoom, containerWidth, containerHeight, 
    onAnnotationClick, onAnnotationRightClick,
    modifyPhase = 'idle', modifyAnnotationId = null, modifyCenter = null, modifyRadii = null, modifyMousePos = null, modifyAngleOffset = 0, modifyRotation = 0, modifyCenterOffset = null, modifyIsCreating = true, modifyOriginalRadii = null, modifyDragStartPos = null,
    modifyPolygonVertices = null, modifyFreehandPath = null, modifyEditingVertexIndex = null,
    maskPaintData = null, maskTileOrigin = null, maskAllTiles = [], maskBrushSize = 20
  }: Props = $props();

  // Settings: global annotation visibility
  let globalVisible = $state(true);
  const unsubSettings = settings.subscribe((s) => {
    globalVisible = s.annotations.visible;
  });

  // Annotation store state - now per-slide
  let annotationSetsBySlide = $state<Map<string, AnnotationSet[]>>(new Map());
  let annotationsBySlide = $state<Map<string, Map<string, Annotation[]>>>(new Map());
  let layerVisibility = $state<Map<string, boolean>>(new Map());
  let highlightedId = $state<string | null>(null);
  let selectedId = $state<string | null>(null);

  const unsubAnnotations = annotationStore.subscribe((state) => {
    annotationSetsBySlide = state.annotationSetsBySlide;
    annotationsBySlide = state.annotationsBySlide;
    layerVisibility = state.layerVisibility;
    highlightedId = state.highlightedAnnotationId;
    selectedId = state.selectedAnnotationId;
  });

  // Auth state for edit modes
  let isLoggedIn = $state(false);
  const unsubAuth = authStore.subscribe((state) => {
    isLoggedIn = state.user !== null;
  });

  onDestroy(() => {
    unsubSettings();
    unsubAnnotations();
    unsubAuth();
    // Clear cache on destroy
    maskDataCache.clear();
  });

  // Canvas for mask rendering (much faster than SVG for pixel-based graphics)
  let maskCanvas: HTMLCanvasElement | null = $state(null);
  
  // Cached decoded mask data keyed by annotation ID
  function getCachedMaskData(annotationId: string, base64: string): Uint8Array | null {
    const cached = maskDataCache.get(annotationId);
    if (cached) return cached;
    
    const decoded = decodeMaskData(base64);
    if (decoded) {
      maskDataCache.set(annotationId, decoded);
    }
    return decoded;
  }

  // Render all masks to canvas - only recomputes when inputs change
  $effect(() => {
    if (!maskCanvas || !globalVisible) return;
    
    const ctx = maskCanvas.getContext('2d', { alpha: true });
    if (!ctx) return;
    
    // Clear the canvas
    ctx.clearRect(0, 0, containerWidth, containerHeight);
    
    // Get visible mask annotations
    const masks = visibleAnnotations.filter(a => a.annotation.kind === 'mask_patch' && isInView(a.annotation));
    
    // Render stored masks
    for (const { annotation, color } of masks) {
      const geo = annotation.geometry as MaskGeometry;
      if (!geo.data_base64) continue;
      
      const maskData = getCachedMaskData(annotation.id, geo.data_base64);
      if (!maskData) continue;
      
      renderMaskToCanvas(ctx, maskData, geo.x0_level0, geo.y0_level0, geo.width, geo.height, color, 0.5);
    }
    
    // Render painting preview tiles
    if (modifyPhase === 'mask-paint' && maskAllTiles && maskAllTiles.length > 0) {
      const previewColor = '#3b82f6';
      for (const tile of maskAllTiles) {
        if (!tile || !tile.origin || !tile.data) continue;
        renderMaskToCanvas(ctx, tile.data, tile.origin.x, tile.origin.y, 512, 512, previewColor, 0.5);
      }
    }
  });
  
  // Efficient mask rendering to canvas using run-length encoding and fillRect
  function renderMaskToCanvas(
    ctx: CanvasRenderingContext2D,
    maskData: Uint8Array,
    x0: number,
    y0: number,
    width: number,
    height: number,
    colorHex: string,
    opacity: number
  ) {
    if (!maskData || maskData.length === 0) return;
    
    // Parse color once
    const r = parseInt(colorHex.slice(1, 3), 16) || 0;
    const g = parseInt(colorHex.slice(3, 5), 16) || 0;
    const b = parseInt(colorHex.slice(5, 7), 16) || 0;
    ctx.fillStyle = `rgba(${r}, ${g}, ${b}, ${opacity})`;
    
    // Determine effective mask dimensions
    const effectiveWidth = maskData.length === 32768 ? 512 : Math.min(width, Math.floor(maskData.length * 8 / height));
    const effectiveHeight = maskData.length === 32768 ? 512 : height;
    
    if (effectiveWidth <= 0 || effectiveHeight <= 0) return;
    
    // Calculate viewport bounds in mask coordinates  
    const viewLeft = viewportX;
    const viewTop = viewportY;
    const viewRight = viewportX + containerWidth / viewportZoom;
    const viewBottom = viewportY + containerHeight / viewportZoom;
    
    // Clamp to mask bounds
    const startRow = Math.max(0, Math.floor(viewTop - y0));
    const endRow = Math.min(effectiveHeight, Math.ceil(viewBottom - y0));
    const startCol = Math.max(0, Math.floor(viewLeft - x0));
    const endCol = Math.min(effectiveWidth, Math.ceil(viewRight - x0));
    
    if (startRow >= endRow || startCol >= endCol) return;
    
    // Subsample when zoomed out for performance
    const step = viewportZoom < 0.25 ? Math.ceil(1 / (viewportZoom * 4)) : 1;
    const pixelSize = viewportZoom * step;
    
    // Limit total operations for performance
    let opCount = 0;
    const maxOps = 50000;
    
    for (let row = startRow; row < endRow && opCount < maxOps; row += step) {
      let runStart: number | null = null;
      
      for (let col = startCol; col <= endCol && opCount < maxOps; col += step) {
        const inBounds = col < endCol;
        let isSet = false;
        
        if (inBounds) {
          const bitIndex = row * effectiveWidth + col;
          const byteIndex = Math.floor(bitIndex / 8);
          const bitOffset = bitIndex % 8;
          isSet = byteIndex < maskData.length && (maskData[byteIndex] & (1 << bitOffset)) !== 0;
        }
        
        if (isSet) {
          if (runStart === null) runStart = col;
        } else {
          if (runStart !== null) {
            // Draw the run
            const screenX = (x0 + runStart - viewportX) * viewportZoom;
            const screenY = (y0 + row - viewportY) * viewportZoom;
            const runWidth = (col - runStart) * viewportZoom;
            ctx.fillRect(screenX, screenY, runWidth, pixelSize);
            runStart = null;
            opCount++;
          }
        }
      }
      
      // Close run at end of row
      if (runStart !== null) {
        const screenX = (x0 + runStart - viewportX) * viewportZoom;
        const screenY = (y0 + row - viewportY) * viewportZoom;
        const runWidth = (endCol - runStart) * viewportZoom;
        ctx.fillRect(screenX, screenY, runWidth, pixelSize);
        opCount++;
      }
    }
  }

  // Get all visible annotations for this slide
  let visibleAnnotations = $derived.by(() => {
    if (!globalVisible || !slideId) return [];
    
    const result: Array<{ annotation: Annotation; setId: string; setName: string; color: string }> = [];
    
    // Get annotation sets for this specific slide
    const annotationSets = annotationSetsBySlide.get(slideId) ?? [];
    const annotationsBySet = annotationsBySlide.get(slideId) ?? new Map();
    
    for (const set of annotationSets) {
      if (!layerVisibility.get(set.id)) continue;
      
      const annotations = annotationsBySet.get(set.id) ?? [];
      const color = getLayerColor(set.id, slideId);
      
      for (const annotation of annotations) {
        result.push({ annotation, setId: set.id, setName: set.name, color });
      }
    }
    
    return result;
  });

  // Convert image coordinates to screen coordinates
  function imageToScreen(imageX: number, imageY: number): { x: number; y: number } {
    const screenX = (imageX - viewportX) * viewportZoom;
    const screenY = (imageY - viewportY) * viewportZoom;
    return { x: screenX, y: screenY };
  }

  // Scale factor for rendering
  function getScreenRadius(imageRadius: number): number {
    return imageRadius * viewportZoom;
  }

  // Check if annotation is in view (basic bounding box check)
  function isInView(annotation: Annotation): boolean {
    const bounds = getAnnotationBounds(annotation);
    if (!bounds) return true; // If we can't determine bounds, render anyway
    
    const viewRight = viewportX + containerWidth / viewportZoom;
    const viewBottom = viewportY + containerHeight / viewportZoom;
    
    return bounds.maxX >= viewportX && bounds.minX <= viewRight &&
           bounds.maxY >= viewportY && bounds.minY <= viewBottom;
  }

  function getAnnotationBounds(annotation: Annotation): { minX: number; minY: number; maxX: number; maxY: number } | null {
    const geo = annotation.geometry;
    switch (annotation.kind) {
      case 'point': {
        const p = geo as PointGeometry;
        return { minX: p.x_level0 - 10, minY: p.y_level0 - 10, maxX: p.x_level0 + 10, maxY: p.y_level0 + 10 };
      }
      case 'ellipse': {
        const e = geo as EllipseGeometry;
        const maxR = Math.max(e.radius_x, e.radius_y);
        return { minX: e.cx_level0 - maxR, minY: e.cy_level0 - maxR, maxX: e.cx_level0 + maxR, maxY: e.cy_level0 + maxR };
      }
      case 'polygon':
      case 'polyline': {
        const poly = geo as PolygonGeometry;
        if (poly.path.length === 0) return null;
        let minX = Infinity, minY = Infinity, maxX = -Infinity, maxY = -Infinity;
        for (const [x, y] of poly.path) {
          minX = Math.min(minX, x);
          minY = Math.min(minY, y);
          maxX = Math.max(maxX, x);
          maxY = Math.max(maxY, y);
        }
        return { minX, minY, maxX, maxY };
      }
      case 'mask_patch': {
        const m = geo as MaskGeometry;
        return { minX: m.x0_level0, minY: m.y0_level0, maxX: m.x0_level0 + m.width, maxY: m.y0_level0 + m.height };
      }
      default:
        return null;
    }
  }

  // Handle annotation click
  function handleClick(e: MouseEvent, annotation: Annotation) {
    e.stopPropagation();
    if (onAnnotationClick) {
      onAnnotationClick(annotation, e.clientX, e.clientY);
    }
  }

  // Handle annotation right-click
  function handleContextMenu(e: MouseEvent, annotation: Annotation) {
    e.preventDefault();
    e.stopPropagation();
    if (onAnnotationRightClick) {
      onAnnotationRightClick(annotation, e.clientX, e.clientY);
    }
  }

  // Handle annotation hover
  function handleMouseEnter(annotation: Annotation) {
    annotationStore.setHighlightedAnnotation(annotation.id);
  }

  function handleMouseLeave() {
    annotationStore.setHighlightedAnnotation(null);
  }

  // Build SVG path for polygon
  function buildPolygonPath(path: [number, number][]): string {
    if (path.length === 0) return '';
    const points = path.map(([x, y]) => {
      const screen = imageToScreen(x, y);
      return `${screen.x},${screen.y}`;
    });
    return `M${points.join(' L')} Z`;
  }

  // Build SVG path for polyline
  function buildPolylinePath(path: [number, number][]): string {
    if (path.length === 0) return '';
    const points = path.map(([x, y]) => {
      const screen = imageToScreen(x, y);
      return `${screen.x},${screen.y}`;
    });
    return `M${points.join(' L')}`;
  }

  // Point rendering parameters
  const POINT_RADIUS = 6;
  const POINT_STROKE_WIDTH = 2;
  
  // Ellipse stroke width
  const ELLIPSE_STROKE_WIDTH = 2;

  // Decode base64 mask data to Uint8Array
  function decodeMaskData(base64: string): Uint8Array | null {
    try {
      const binaryString = atob(base64);
      const bytes = new Uint8Array(binaryString.length);
      for (let i = 0; i < binaryString.length; i++) {
        bytes[i] = binaryString.charCodeAt(i);
      }
      return bytes;
    } catch (e) {
      console.error('Failed to decode mask data:', e);
      return null;
    }
  }
</script>

{#if globalVisible}
  <!-- Canvas layer for efficient mask rendering -->
  <canvas 
    bind:this={maskCanvas}
    class="mask-canvas"
    width={containerWidth}
    height={containerHeight}
  ></canvas>
  
  <svg 
    class="annotation-overlay"
    width={containerWidth}
    height={containerHeight}
    viewBox="0 0 {containerWidth} {containerHeight}"
  >
    <defs>
      <!-- Glow filter for highlighted annotations -->
      <filter id="annotation-glow" x="-50%" y="-50%" width="200%" height="200%">
        <feGaussianBlur stdDeviation="3" result="blur"/>
        <feMerge>
          <feMergeNode in="blur"/>
          <feMergeNode in="SourceGraphic"/>
        </feMerge>
      </filter>
    </defs>

    {#each visibleAnnotations as { annotation, color, setName } (annotation.id)}
      {#if isInView(annotation)}
        {@const isHighlighted = highlightedId === annotation.id}
        {@const isSelected = selectedId === annotation.id}

        {#if annotation.kind === 'point'}
          {@const geo = annotation.geometry as PointGeometry}
          {@const screen = imageToScreen(geo.x_level0, geo.y_level0)}
          <!-- svelte-ignore a11y_click_events_have_key_events -->
          <!-- svelte-ignore a11y_no_static_element_interactions -->
          <g 
            class="annotation-point"
            class:highlighted={isHighlighted}
            class:selected={isSelected}
            onclick={(e) => handleClick(e, annotation)}
            oncontextmenu={(e) => handleContextMenu(e, annotation)}
            onmouseenter={() => handleMouseEnter(annotation)}
            onmouseleave={handleMouseLeave}
          >
            <title>{setName}</title>
            <circle 
              cx={screen.x} 
              cy={screen.y} 
              r={POINT_RADIUS}
              fill={color}
              fill-opacity="0.8"
              stroke="white"
              stroke-width={POINT_STROKE_WIDTH}
              filter={isHighlighted || isSelected ? 'url(#annotation-glow)' : undefined}
            />
          </g>

        {:else if annotation.kind === 'ellipse'}
          {@const geo = annotation.geometry as EllipseGeometry}
          {@const screen = imageToScreen(geo.cx_level0, geo.cy_level0)}
          {@const rx = getScreenRadius(geo.radius_x)}
          {@const ry = getScreenRadius(geo.radius_y)}
          {@const rotation = geo.rotation_radians * (180 / Math.PI)}
          <!-- svelte-ignore a11y_click_events_have_key_events -->
          <!-- svelte-ignore a11y_no_static_element_interactions -->
          <g 
            class="annotation-ellipse"
            class:highlighted={isHighlighted}
            class:selected={isSelected}
            onclick={(e) => handleClick(e, annotation)}
            oncontextmenu={(e) => handleContextMenu(e, annotation)}
            onmouseenter={() => handleMouseEnter(annotation)}
            onmouseleave={handleMouseLeave}
          >
            <title>{setName}</title>
            <ellipse 
              cx={screen.x} 
              cy={screen.y} 
              rx={rx}
              ry={ry}
              transform="rotate({rotation} {screen.x} {screen.y})"
              fill={color}
              fill-opacity="0.2"
              stroke={color}
              stroke-width={ELLIPSE_STROKE_WIDTH}
              filter={isHighlighted || isSelected ? 'url(#annotation-glow)' : undefined}
            />
            {#if isSelected && isLoggedIn}
              <!-- Edit handles for selected ellipse -->
              <circle cx={screen.x} cy={screen.y} r="5" fill="white" stroke={color} stroke-width="2" class="handle center"/>
              <circle cx={screen.x + rx} cy={screen.y} r="4" fill="white" stroke={color} stroke-width="2" class="handle radius-x" transform="rotate({rotation} {screen.x} {screen.y})"/>
              <circle cx={screen.x} cy={screen.y + ry} r="4" fill="white" stroke={color} stroke-width="2" class="handle radius-y" transform="rotate({rotation} {screen.x} {screen.y})"/>
            {/if}
          </g>

        {:else if annotation.kind === 'polygon'}
          {@const geo = annotation.geometry as PolygonGeometry}
          {@const pathD = buildPolygonPath(geo.path)}
          <!-- svelte-ignore a11y_click_events_have_key_events -->
          <!-- svelte-ignore a11y_no_static_element_interactions -->
          <g 
            class="annotation-polygon"
            class:highlighted={isHighlighted}
            class:selected={isSelected}
            onclick={(e) => handleClick(e, annotation)}
            oncontextmenu={(e) => handleContextMenu(e, annotation)}
            onmouseenter={() => handleMouseEnter(annotation)}
            onmouseleave={handleMouseLeave}
          >
            <title>{setName}</title>
            <path 
              d={pathD}
              fill={color}
              fill-opacity="0.2"
              stroke={color}
              stroke-width="2"
              filter={isHighlighted || isSelected ? 'url(#annotation-glow)' : undefined}
            />
          </g>

        {:else if annotation.kind === 'polyline'}
          {@const geo = annotation.geometry as PolygonGeometry}
          {@const pathD = buildPolylinePath(geo.path)}
          <!-- svelte-ignore a11y_click_events_have_key_events -->
          <!-- svelte-ignore a11y_no_static_element_interactions -->
          <g 
            class="annotation-polyline"
            class:highlighted={isHighlighted}
            class:selected={isSelected}
            onclick={(e) => handleClick(e, annotation)}
            oncontextmenu={(e) => handleContextMenu(e, annotation)}
            onmouseenter={() => handleMouseEnter(annotation)}
            onmouseleave={handleMouseLeave}
          >
            <title>{setName}</title>
            <path 
              d={pathD}
              fill="none"
              stroke={color}
              stroke-width="2"
              stroke-linecap="round"
              stroke-linejoin="round"
              filter={isHighlighted || isSelected ? 'url(#annotation-glow)' : undefined}
            />
          </g>

        {:else if annotation.kind === 'mask_patch'}
          <!-- Masks are rendered on canvas; this is just a click/hover target -->
          {@const geo = annotation.geometry as MaskGeometry}
          {@const screen = imageToScreen(geo.x0_level0, geo.y0_level0)}
          {@const width = getScreenRadius(geo.width)}
          {@const height = getScreenRadius(geo.height)}
          <!-- svelte-ignore a11y_click_events_have_key_events -->
          <!-- svelte-ignore a11y_no_static_element_interactions -->
          <rect 
            class="annotation-mask-target"
            class:highlighted={isHighlighted}
            class:selected={isSelected}
            x={screen.x} 
            y={screen.y} 
            width={width}
            height={height}
            fill="transparent"
            onclick={(e) => handleClick(e, annotation)}
            oncontextmenu={(e) => handleContextMenu(e, annotation)}
            onmouseenter={() => handleMouseEnter(annotation)}
            onmouseleave={handleMouseLeave}
          />
        {/if}
      {/if}
    {/each}

    <!-- Ellipse modification preview -->
    {#if modifyMousePos && (modifyPhase === 'ellipse-center' || modifyPhase === 'ellipse-radii' || modifyPhase === 'ellipse-angle')}
      {@const previewColor = '#ffcc00'}
      
      {#if modifyPhase === 'ellipse-center'}
        <!-- Show crosshair at mouse position for center selection -->
        <!-- When modifyCenterOffset exists, offset the preview so ellipse doesn't snap to cursor -->
        {@const offset = modifyCenterOffset ?? { x: 0, y: 0 }}
        {@const previewPos = { x: modifyMousePos.x - offset.x, y: modifyMousePos.y - offset.y }}
        {@const screen = imageToScreen(previewPos.x, previewPos.y)}
        {#if modifyRadii}
          <!-- If radii already exist, show full ellipse preview centered on offset position -->
          {@const rx = getScreenRadius(modifyRadii.rx)}
          {@const ry = getScreenRadius(modifyRadii.ry)}
          {@const angleDeg = modifyRotation * (180 / Math.PI)}
          <g class="preview-ellipse-repositioning">
            <ellipse 
              cx={screen.x} 
              cy={screen.y} 
              rx={rx}
              ry={ry}
              transform="rotate({angleDeg} {screen.x} {screen.y})"
              fill={previewColor}
              fill-opacity="0.15"
              stroke={previewColor}
              stroke-width="2"
              stroke-dasharray="6 3"
            />
            <circle cx={screen.x} cy={screen.y} r="4" fill={previewColor} stroke="white" stroke-width="1"/>
          </g>
        {:else}
          <!-- No radii yet, just show crosshair -->
          <g class="preview-center">
            <circle cx={screen.x} cy={screen.y} r="6" fill={previewColor} fill-opacity="0.5" stroke={previewColor} stroke-width="2"/>
            <line x1={screen.x - 12} y1={screen.y} x2={screen.x + 12} y2={screen.y} stroke={previewColor} stroke-width="1"/>
            <line x1={screen.x} y1={screen.y - 12} x2={screen.x} y2={screen.y + 12} stroke={previewColor} stroke-width="1"/>
          </g>
        {/if}
      {:else if modifyPhase === 'ellipse-radii' && modifyCenter}
        <!-- Show ellipse preview with mouse offset transformed by rotation for rx/ry -->
        {@const centerScreen = imageToScreen(modifyCenter.x, modifyCenter.y)}
        {@const angleDeg = modifyRotation * (180 / Math.PI)}
        {@const cosA = Math.cos(modifyRotation)}
        {@const sinA = Math.sin(modifyRotation)}
        <!-- For modification mode: compute radii relative to current tempRadii, for creation: from mouse distance -->
        {@const dx = modifyMousePos.x - modifyCenter.x}
        {@const dy = modifyMousePos.y - modifyCenter.y}
        {@const cosR = Math.cos(-modifyRotation)}
        {@const sinR = Math.sin(-modifyRotation)}
        {@const localX = dx * cosR - dy * sinR}
        {@const localY = dx * sinR + dy * cosR}
        <!-- In modification mode with radii and drag start: compute delta from drag start, apply to current radii -->
        <!-- Use modifyRadii (tempRadii) as the base since it's the current working value -->
        {@const baseRadii = modifyRadii ?? modifyOriginalRadii}
        {@const rxImage = !modifyIsCreating && baseRadii && modifyDragStartPos
          ? Math.max(baseRadii.rx + (Math.abs(localX) - Math.abs((modifyDragStartPos.x - modifyCenter.x) * cosR - (modifyDragStartPos.y - modifyCenter.y) * sinR)), 1)
          : Math.max(Math.abs(localX), 1)}
        {@const ryImage = !modifyIsCreating && baseRadii && modifyDragStartPos
          ? Math.max(baseRadii.ry + (Math.abs(localY) - Math.abs((modifyDragStartPos.x - modifyCenter.x) * sinR + (modifyDragStartPos.y - modifyCenter.y) * cosR)), 1)
          : Math.max(Math.abs(localY), 1)}
        {@const rx = getScreenRadius(rxImage)}
        {@const ry = getScreenRadius(ryImage)}
        <g class="preview-ellipse">
          <ellipse 
            cx={centerScreen.x} 
            cy={centerScreen.y} 
            rx={rx}
            ry={ry}
            transform="rotate({angleDeg} {centerScreen.x} {centerScreen.y})"
            fill={previewColor}
            fill-opacity="0.15"
            stroke={previewColor}
            stroke-width="2"
            stroke-dasharray="6 3"
          />
          <circle cx={centerScreen.x} cy={centerScreen.y} r="4" fill={previewColor} stroke="white" stroke-width="1"/>
          <!-- Show rx and ry guidelines (rotated) -->
          <line x1={centerScreen.x} y1={centerScreen.y} x2={centerScreen.x + rx * cosA} y2={centerScreen.y + rx * sinA} stroke={previewColor} stroke-width="1" stroke-dasharray="3 2"/>
          <line x1={centerScreen.x} y1={centerScreen.y} x2={centerScreen.x - ry * sinA} y2={centerScreen.y + ry * cosA} stroke={previewColor} stroke-width="1" stroke-dasharray="3 2"/>
        </g>
      {:else if modifyPhase === 'ellipse-angle' && modifyCenter && modifyRadii}
        <!-- Show ellipse preview with rotation based on mouse angle -->
        {@const centerScreen = imageToScreen(modifyCenter.x, modifyCenter.y)}
        {@const dx = modifyMousePos.x - modifyCenter.x}
        {@const dy = modifyMousePos.y - modifyCenter.y}
        {@const rawAngleRad = Math.atan2(dy, dx)}
        <!-- For modification mode with dragStartPos: compute angle delta from drag start, add to current rotation -->
        {@const angleRad = !modifyIsCreating && modifyDragStartPos
          ? modifyRotation + (rawAngleRad - Math.atan2(modifyDragStartPos.y - modifyCenter.y, modifyDragStartPos.x - modifyCenter.x))
          : rawAngleRad - modifyAngleOffset}
        {@const angleDeg = angleRad * (180 / Math.PI)}
        {@const rx = getScreenRadius(modifyRadii.rx)}
        {@const ry = getScreenRadius(modifyRadii.ry)}
        {@const lineEndX = centerScreen.x + Math.cos(angleRad) * rx}
        {@const lineEndY = centerScreen.y + Math.sin(angleRad) * rx}
        <g class="preview-ellipse-rotated">
          <ellipse 
            cx={centerScreen.x} 
            cy={centerScreen.y} 
            rx={rx}
            ry={ry}
            transform="rotate({angleDeg} {centerScreen.x} {centerScreen.y})"
            fill={previewColor}
            fill-opacity="0.2"
            stroke={previewColor}
            stroke-width="2"
          />
          <circle cx={centerScreen.x} cy={centerScreen.y} r="4" fill={previewColor} stroke="white" stroke-width="1"/>
          <!-- Rotation line indicator -->
          <line x1={centerScreen.x} y1={centerScreen.y} x2={lineEndX} y2={lineEndY} stroke={previewColor} stroke-width="2"/>
        </g>
      {/if}
    {/if}

    <!-- Point modification preview -->
    {#if modifyMousePos && (modifyPhase === 'point-position' || modifyPhase === 'multi-point')}
      {@const screen = imageToScreen(modifyMousePos.x, modifyMousePos.y)}
      {@const previewColor = '#ffcc00'}
      <g class="preview-point">
        <circle cx={screen.x} cy={screen.y} r={POINT_RADIUS} fill={previewColor} fill-opacity="0.7" stroke="white" stroke-width={POINT_STROKE_WIDTH}/>
      </g>
    {/if}

    <!-- Polygon modification/creation preview -->
    {#if modifyPhase === 'polygon-vertices' || modifyPhase === 'polygon-edit' || modifyPhase === 'polygon-freehand'}
      {@const previewColor = '#ffcc00'}
      
      {#if modifyPolygonVertices && modifyPolygonVertices.length > 0}
        <!-- Draw polygon path -->
        {@const polygonPath = modifyPolygonVertices.map((v, i) => {
          const s = imageToScreen(v.x, v.y);
          return i === 0 ? `M${s.x},${s.y}` : `L${s.x},${s.y}`;
        }).join(' ') + (modifyPolygonVertices.length >= 3 ? ' Z' : '')}
        
        <g class="preview-polygon">
          <path 
            d={polygonPath}
            fill={previewColor}
            fill-opacity={modifyPolygonVertices.length >= 3 ? 0.15 : 0}
            stroke={previewColor}
            stroke-width="2"
            stroke-dasharray={modifyPolygonVertices.length >= 3 ? 'none' : '6 3'}
          />
          
          <!-- Preview line to current mouse position during vertex creation -->
          {#if modifyPhase === 'polygon-vertices' && modifyMousePos && modifyPolygonVertices.length > 0}
            {@const lastVertex = modifyPolygonVertices[modifyPolygonVertices.length - 1]}
            {@const lastScreen = imageToScreen(lastVertex.x, lastVertex.y)}
            {@const mouseScreen = imageToScreen(modifyMousePos.x, modifyMousePos.y)}
            <line 
              x1={lastScreen.x} y1={lastScreen.y} 
              x2={mouseScreen.x} y2={mouseScreen.y} 
              stroke={previewColor} 
              stroke-width="1"
              stroke-dasharray="4 2"
            />
            <!-- Close preview line if we have 3+ vertices -->
            {#if modifyPolygonVertices.length >= 2}
              {@const firstVertex = modifyPolygonVertices[0]}
              {@const firstScreen = imageToScreen(firstVertex.x, firstVertex.y)}
              <line 
                x1={mouseScreen.x} y1={mouseScreen.y} 
                x2={firstScreen.x} y2={firstScreen.y} 
                stroke={previewColor} 
                stroke-width="1"
                stroke-dasharray="4 2"
                opacity="0.5"
              />
            {/if}
          {/if}
          
          <!-- Draw vertex handles -->
          {#each modifyPolygonVertices as vertex, i}
            {@const screen = imageToScreen(vertex.x, vertex.y)}
            {@const isEditing = modifyEditingVertexIndex === i}
            <circle 
              cx={screen.x} 
              cy={screen.y} 
              r={isEditing ? 8 : 6} 
              fill={isEditing ? 'white' : previewColor} 
              stroke={isEditing ? previewColor : 'white'} 
              stroke-width="2"
              class="polygon-vertex-handle"
            />
            <!-- Vertex number label -->
            <text 
              x={screen.x} 
              y={screen.y - 12} 
              text-anchor="middle" 
              fill={previewColor}
              font-size="10"
              font-weight="bold"
            >{i + 1}</text>
          {/each}
        </g>
      {/if}
      
      <!-- Freehand lasso preview -->
      {#if modifyFreehandPath && modifyFreehandPath.length > 1}
        {@const freehandPathD = modifyFreehandPath.map((v, i) => {
          const s = imageToScreen(v.x, v.y);
          return i === 0 ? `M${s.x},${s.y}` : `L${s.x},${s.y}`;
        }).join(' ')}
        
        <g class="preview-freehand">
          <path 
            d={freehandPathD}
            fill="none"
            stroke={previewColor}
            stroke-width="2"
            stroke-linecap="round"
            stroke-linejoin="round"
          />
        </g>
      {/if}
    {/if}

    <!-- Mask painting mode: tile boundaries and brush cursor -->
    <!-- Mask pixels are rendered on canvas for performance -->
    {#if modifyPhase === 'mask-paint'}
      {@const previewColor = '#3b82f6'}
      
      <!-- Tile boundaries -->
      {#if maskAllTiles && maskAllTiles.length > 0}
        {@const tileSize = 512 * viewportZoom}
        {#each maskAllTiles.filter(t => t && t.origin) as tile (tile.origin.x + ',' + tile.origin.y)}
          {@const tileScreen = imageToScreen(tile.origin.x, tile.origin.y)}
          <rect 
            x={tileScreen.x} 
            y={tileScreen.y}
            width={tileSize} 
            height={tileSize}
            fill="none"
            stroke={previewColor}
            stroke-width="1"
            stroke-dasharray="4 2"
            opacity="0.5"
            style="pointer-events: none;"
          />
        {/each}
      {/if}
      
      <!-- Simple circle brush cursor (much faster than pixelated path) -->
      {#if modifyMousePos}
        {@const screen = imageToScreen(modifyMousePos.x, modifyMousePos.y)}
        {@const radius = (maskBrushSize / 2) * viewportZoom}
        <circle 
          cx={screen.x}
          cy={screen.y}
          r={radius}
          fill="none"
          stroke="white"
          stroke-width="2"
          opacity="0.9"
          style="pointer-events: none;"
        />
        <circle 
          cx={screen.x}
          cy={screen.y}
          r={radius}
          fill="none"
          stroke="black"
          stroke-width="1"
          opacity="0.5"
          style="pointer-events: none;"
        />
      {/if}
    {/if}
  </svg>
{/if}

<style>
  .mask-canvas {
    position: absolute;
    top: 0;
    left: 0;
    pointer-events: none;
  }

  .annotation-overlay {
    position: absolute;
    top: 0;
    left: 0;
    pointer-events: none;
    overflow: visible;
  }

  .annotation-point,
  .annotation-ellipse,
  .annotation-polygon,
  .annotation-polyline,
  .annotation-mask-target {
    pointer-events: auto;
    cursor: pointer;
    transition: opacity 0.15s;
  }

  .annotation-point:hover,
  .annotation-ellipse:hover,
  .annotation-polygon:hover,
  .annotation-polyline:hover,
  .annotation-mask-target:hover {
    opacity: 0.9;
  }

  .annotation-point.highlighted,
  .annotation-ellipse.highlighted,
  .annotation-polygon.highlighted,
  .annotation-polyline.highlighted,
  .annotation-mask-target.highlighted {
    opacity: 1;
  }

  .annotation-point.selected,
  .annotation-ellipse.selected,
  .annotation-polygon.selected,
  .annotation-polyline.selected,
  .annotation-mask-target.selected {
    opacity: 1;
  }

  .handle {
    cursor: move;
    transition: transform 0.1s;
  }

  .handle:hover {
    transform: scale(1.2);
  }

  .handle.center {
    cursor: move;
  }

  .handle.radius-x,
  .handle.radius-y {
    cursor: ew-resize;
  }

  /* Preview styling for modification mode */
  .preview-center,
  .preview-point,
  .preview-ellipse,
  .preview-ellipse-rotated,
  .preview-polygon,
  .preview-freehand {
    pointer-events: none;
    animation: previewPulse 1s ease-in-out infinite;
  }

  .polygon-vertex-handle {
    cursor: move;
    transition: r 0.15s, fill 0.15s;
  }

  .polygon-vertex-handle:hover {
    r: 9;
  }

  @keyframes previewPulse {
    0%, 100% {
      opacity: 1;
    }
    50% {
      opacity: 0.6;
    }
  }
</style>
