<script lang="ts">
  import { onDestroy } from 'svelte';
  import { settings } from '$lib/stores/settings';
  import { annotationStore } from '$lib/stores/annotations';
  import { authStore } from '$lib/stores/auth';
  import type { Annotation, AnnotationKind, PointGeometry, EllipseGeometry, PolygonGeometry, MaskGeometry } from '$lib/api/annotations';

  interface Props {
    /** Viewport state */
    viewportX: number;
    viewportY: number;
    viewportZoom: number;
    /** Container dimensions */
    containerWidth: number;
    containerHeight: number;
    /** Callback when an annotation is clicked */
    onAnnotationClick?: (annotation: Annotation, screenX: number, screenY: number) => void;
  }

  let { viewportX, viewportY, viewportZoom, containerWidth, containerHeight, onAnnotationClick }: Props = $props();

  // Settings: global annotation visibility
  let globalVisible = $state(true);
  const unsubSettings = settings.subscribe((s) => {
    globalVisible = s.annotations.visible;
  });

  // Annotation store state
  let annotationSets = $state<typeof $annotationStore.annotationSets>([]);
  let annotationsBySet = $state<Map<string, Annotation[]>>(new Map());
  let layerVisibility = $state<Map<string, boolean>>(new Map());
  let highlightedId = $state<string | null>(null);
  let selectedId = $state<string | null>(null);

  const unsubAnnotations = annotationStore.subscribe((state) => {
    annotationSets = state.annotationSets;
    annotationsBySet = state.annotationsBySet;
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

  // Get all visible annotations
  let visibleAnnotations = $derived.by(() => {
    if (!globalVisible) return [];
    
    const result: Array<{ annotation: Annotation; setId: string; color: string }> = [];
    
    for (const set of annotationSets) {
      if (!layerVisibility.get(set.id)) continue;
      
      const annotations = annotationsBySet.get(set.id) ?? [];
      const color = getLayerColor(set.id);
      
      for (const annotation of annotations) {
        result.push({ annotation, setId: set.id, color });
      }
    }
    
    return result;
  });

  // Color palette for layers
  const layerColors = [
    '#0099ff', // blue
    '#ff6b6b', // red
    '#51cf66', // green
    '#ffd43b', // yellow
    '#cc5de8', // purple
    '#ff922b', // orange
    '#20c997', // teal
    '#f06595', // pink
  ];

  function getLayerColor(setId: string): string {
    const idx = annotationSets.findIndex((s) => s.id === setId);
    return layerColors[idx % layerColors.length];
  }

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

    {#each visibleAnnotations as { annotation, color } (annotation.id)}
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
            onmouseenter={() => handleMouseEnter(annotation)}
            onmouseleave={handleMouseLeave}
          >
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
            onmouseenter={() => handleMouseEnter(annotation)}
            onmouseleave={handleMouseLeave}
          >
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
            onmouseenter={() => handleMouseEnter(annotation)}
            onmouseleave={handleMouseLeave}
          >
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
            onmouseenter={() => handleMouseEnter(annotation)}
            onmouseleave={handleMouseLeave}
          >
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
            onmouseenter={() => handleMouseEnter(annotation)}
            onmouseleave={handleMouseLeave}
          >
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
</style>
