<script lang="ts">
  import { onDestroy } from 'svelte';
  import { settings } from '$lib/stores/settings';
  import { annotationStore, getLayerColor } from '$lib/stores/annotations';
  import { authStore } from '$lib/stores/auth';
  import type { Annotation, AnnotationKind, AnnotationSet, PointGeometry, EllipseGeometry, PolygonGeometry, MaskGeometry } from '$lib/api/annotations';

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
    /** Mask painting state */
    maskPaintData?: Uint8Array | null;
    maskTileOrigin?: { x: number; y: number } | null;
    maskBrushSize?: number;
  }

  let { 
    slideId,
    viewportX, viewportY, viewportZoom, containerWidth, containerHeight, 
    onAnnotationClick, onAnnotationRightClick,
    modifyPhase = 'idle', modifyAnnotationId = null, modifyCenter = null, modifyRadii = null, modifyMousePos = null, modifyAngleOffset = 0, modifyRotation = 0, modifyCenterOffset = null, modifyIsCreating = true, modifyOriginalRadii = null, modifyDragStartPos = null,
    modifyPolygonVertices = null, modifyFreehandPath = null, modifyEditingVertexIndex = null,
    maskPaintData = null, maskTileOrigin = null, maskBrushSize = 20
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
  });

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

  // Convert bitmask to run-length encoded runs for efficient rendering
  // Returns array of { row, startCol, length } for each horizontal run of set bits
  function getMaskRuns(data: Uint8Array): Array<{ row: number; startCol: number; length: number }> {
    const runs: Array<{ row: number; startCol: number; length: number }> = [];
    const TILE_SIZE = 512;
    
    // Limit to visible area for performance - subsample when zoomed out
    const step = viewportZoom < 0.5 ? Math.ceil(1 / viewportZoom) : 1;
    
    for (let row = 0; row < TILE_SIZE; row += step) {
      let runStart: number | null = null;
      
      for (let col = 0; col < TILE_SIZE; col += step) {
        const bitIndex = row * TILE_SIZE + col;
        const byteIndex = Math.floor(bitIndex / 8);
        const bitOffset = bitIndex % 8;
        const isSet = byteIndex < data.length && (data[byteIndex] & (1 << bitOffset)) !== 0;
        
        if (isSet) {
          if (runStart === null) {
            runStart = col;
          }
        } else {
          if (runStart !== null) {
            runs.push({ row, startCol: runStart, length: col - runStart });
            runStart = null;
          }
        }
      }
      
      // Close run at end of row
      if (runStart !== null) {
        runs.push({ row, startCol: runStart, length: TILE_SIZE - runStart });
      }
      
      // Limit total runs for performance
      if (runs.length > 10000) break;
    }
    
    return runs;
  }

  // Get pixelated brush outline for cursor display
  // Returns an SVG path string for the outline of pixels that would be painted
  function getPixelatedBrushPath(centerX: number, centerY: number, brushSize: number): string {
    const radius = brushSize / 2;
    const minPx = Math.floor(centerX - radius);
    const maxPx = Math.ceil(centerX + radius);
    const minPy = Math.floor(centerY - radius);
    const maxPy = Math.ceil(centerY + radius);
    
    // Build a set of pixel coordinates that are inside the brush
    const pixels = new Set<string>();
    for (let py = minPy; py <= maxPy; py++) {
      for (let px = minPx; px <= maxPx; px++) {
        // Check if pixel center is within brush radius
        const dx = (px + 0.5) - centerX;
        const dy = (py + 0.5) - centerY;
        if (dx * dx + dy * dy <= radius * radius) {
          pixels.add(`${px},${py}`);
        }
      }
    }
    
    if (pixels.size === 0) return '';
    
    // Build outline path by checking each pixel's edges
    const segments: string[] = [];
    for (const key of pixels) {
      const [px, py] = key.split(',').map(Number);
      const topLeft = imageToScreen(px, py);
      const size = viewportZoom;
      
      // Check each edge - if neighbor is not in set, draw that edge
      // Top edge
      if (!pixels.has(`${px},${py - 1}`)) {
        segments.push(`M${topLeft.x},${topLeft.y} L${topLeft.x + size},${topLeft.y}`);
      }
      // Bottom edge
      if (!pixels.has(`${px},${py + 1}`)) {
        segments.push(`M${topLeft.x},${topLeft.y + size} L${topLeft.x + size},${topLeft.y + size}`);
      }
      // Left edge
      if (!pixels.has(`${px - 1},${py}`)) {
        segments.push(`M${topLeft.x},${topLeft.y} L${topLeft.x},${topLeft.y + size}`);
      }
      // Right edge
      if (!pixels.has(`${px + 1},${py}`)) {
        segments.push(`M${topLeft.x + size},${topLeft.y} L${topLeft.x + size},${topLeft.y + size}`);
      }
    }
    
    return segments.join(' ');
  }
</script>

{#if globalVisible}
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
          {@const geo = annotation.geometry as MaskGeometry}
          {@const screen = imageToScreen(geo.x0_level0, geo.y0_level0)}
          {@const width = getScreenRadius(geo.width)}
          {@const height = getScreenRadius(geo.height)}
          <!-- Mask patch - render as colored rectangle for now (full bitmask rendering would need canvas) -->
          <!-- svelte-ignore a11y_click_events_have_key_events -->
          <!-- svelte-ignore a11y_no_static_element_interactions -->
          <g 
            class="annotation-mask"
            class:highlighted={isHighlighted}
            class:selected={isSelected}
            onclick={(e) => handleClick(e, annotation)}
            oncontextmenu={(e) => handleContextMenu(e, annotation)}
            onmouseenter={() => handleMouseEnter(annotation)}
            onmouseleave={handleMouseLeave}
          >
            <title>{setName}</title>
            <rect 
              x={screen.x} 
              y={screen.y} 
              width={width}
              height={height}
              fill={color}
              fill-opacity="0.3"
              stroke={color}
              stroke-width="1"
              stroke-dasharray="4 2"
              filter={isHighlighted || isSelected ? 'url(#annotation-glow)' : undefined}
            />
          </g>
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

    <!-- Mask painting preview -->
    {#if modifyPhase === 'mask-paint' && maskPaintData && maskTileOrigin}
      {@const tileScreen = imageToScreen(maskTileOrigin.x, maskTileOrigin.y)}
      {@const tileSize = 512 * viewportZoom}
      {@const previewColor = '#3b82f6'}
      
      <!-- Tile boundary -->
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
      
      <!-- Render painted pixels - simplified approach using rects -->
      <!-- For performance, we skip rendering individual pixels and show a summary -->
      <g class="mask-preview">
        {#each getMaskRuns(maskPaintData) as run}
          {@const runScreen = imageToScreen(maskTileOrigin.x + run.startCol, maskTileOrigin.y + run.row)}
          <rect 
            x={runScreen.x} 
            y={runScreen.y}
            width={run.length * viewportZoom}
            height={viewportZoom}
            fill={previewColor}
            fill-opacity="0.5"
          />
        {/each}
      </g>
      
      <!-- Brush cursor (pixelated) -->
      {#if modifyMousePos}
        {@const brushPath = getPixelatedBrushPath(modifyMousePos.x, modifyMousePos.y, maskBrushSize)}
        <g class="brush-cursor">
          <path 
            d={brushPath}
            fill="none"
            stroke="white"
            stroke-width="2"
            opacity="0.9"
          />
          <path 
            d={brushPath}
            fill="none"
            stroke="black"
            stroke-width="1"
            opacity="0.5"
          />
        </g>
      {/if}
    {/if}

    <!-- Mask brush cursor (shows even before first paint) -->
    {#if modifyPhase === 'mask-paint' && modifyMousePos && !maskTileOrigin}
      {@const brushPath = getPixelatedBrushPath(modifyMousePos.x, modifyMousePos.y, maskBrushSize)}
      <g class="brush-cursor">
        <path 
          d={brushPath}
          fill="none"
          stroke="white"
          stroke-width="2"
          opacity="0.9"
        />
        <path 
          d={brushPath}
          fill="none"
          stroke="black"
          stroke-width="1"
          opacity="0.5"
        />
      </g>
    {/if}
  </svg>
{/if}

<style>
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
  .annotation-mask {
    pointer-events: auto;
    cursor: pointer;
    transition: opacity 0.15s;
  }

  .annotation-point:hover,
  .annotation-ellipse:hover,
  .annotation-polygon:hover,
  .annotation-polyline:hover,
  .annotation-mask:hover {
    opacity: 0.9;
  }

  .annotation-point.highlighted,
  .annotation-ellipse.highlighted,
  .annotation-polygon.highlighted,
  .annotation-polyline.highlighted,
  .annotation-mask.highlighted {
    opacity: 1;
  }

  .annotation-point.selected,
  .annotation-ellipse.selected,
  .annotation-polygon.selected,
  .annotation-polyline.selected,
  .annotation-mask.selected {
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
  .preview-freehand,
  .mask-preview,
  .brush-cursor {
    pointer-events: none;
    animation: previewPulse 1s ease-in-out infinite;
  }

  .mask-preview,
  .brush-cursor {
    animation: none;
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
