<script lang="ts">
  import { onDestroy, tick } from 'svelte';
  import { settings } from '$lib/stores/settings';
  import { annotationStore, getLayerColor } from '$lib/stores/annotations';
  import { authStore } from '$lib/stores/auth';
  import type { Annotation, AnnotationKind, AnnotationSet, PointGeometry, EllipseGeometry, PolygonGeometry, MaskGeometry } from '$lib/api/annotations';

  // Cache for decoded mask data - avoids re-decoding base64 every frame
  const maskDataCache = new Map<string, Uint8Array>();
  
  // Cache for pre-rendered mask ImageBitmaps (keyed by annotationId:color)
  // This is the key optimization - render once to ImageBitmap, then just drawImage
  const maskBitmapCache = new Map<string, ImageBitmap>();

  // Track which mask tile is being hovered (only when cursor is over a painted pixel)
  let hoveredMaskId = $state<string | null>(null);
  // Hover highlight color (light cyan for distinction)
  const MASK_HOVER_COLOR = '#00e5ff';

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
    /** Annotation IDs being edited in mask-paint mode (to hide from normal rendering) */
    maskEditingAnnotationIds?: Set<string>;
    /** Whether the viewer is actively panning/zooming (skip expensive mask re-renders) */
    isInteracting?: boolean;
  }

  let { 
    slideId,
    viewportX, viewportY, viewportZoom, containerWidth, containerHeight, 
    onAnnotationClick, onAnnotationRightClick,
    modifyPhase = 'idle', modifyAnnotationId = null, modifyCenter = null, modifyRadii = null, modifyMousePos = null, modifyAngleOffset = 0, modifyRotation = 0, modifyCenterOffset = null, modifyIsCreating = true, modifyOriginalRadii = null, modifyDragStartPos = null,
    modifyPolygonVertices = null, modifyFreehandPath = null, modifyEditingVertexIndex = null,
    maskPaintData = null, maskTileOrigin = null, maskAllTiles = [], maskBrushSize = 20,
    maskEditingAnnotationIds = new Set(),
    isInteracting = false
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
    // Clear caches on destroy
    maskDataCache.clear();
    pendingBitmaps.clear();
    // Close all ImageBitmaps to free GPU memory
    for (const bitmap of maskBitmapCache.values()) {
      bitmap.close();
    }
    maskBitmapCache.clear();
    // Clear preview canvas references
    previewCanvas = null;
    previewImageData = null;
    if (_maskRenderRaf !== null) cancelAnimationFrame(_maskRenderRaf);
  });

  // Canvas for mask rendering (much faster than SVG for pixel-based graphics)
  let maskCanvas: HTMLCanvasElement | null = $state(null);
  let maskCanvasTransform = $state('none');
  let lastMaskRenderViewportX = 0;
  let lastMaskRenderViewportY = 0;
  let lastMaskRenderViewportZoom = 1;
  let hasMaskRenderViewport = false;
  
  // Offscreen canvas for creating mask bitmaps (reused for efficiency)
  let offscreenCanvas: OffscreenCanvas | null = null;
  
  // Get or create an ImageBitmap for a mask annotation (cached by id:color)
  // This is the key optimization - we render the mask to a bitmap ONCE, then just use drawImage
  async function getOrCreateMaskBitmap(
    annotationId: string,
    maskData: Uint8Array,
    width: number,
    height: number,
    colorHex: string,
    opacity: number
  ): Promise<ImageBitmap | null> {
    const cacheKey = `${annotationId}:${colorHex}`;
    
    // Return cached bitmap if available
    const cached = maskBitmapCache.get(cacheKey);
    if (cached) return cached;
    
    // Create offscreen canvas if needed
    if (!offscreenCanvas || offscreenCanvas.width !== width || offscreenCanvas.height !== height) {
      offscreenCanvas = new OffscreenCanvas(width, height);
    }
    
    const ctx = offscreenCanvas.getContext('2d', { alpha: true });
    if (!ctx) return null;
    
    // Clear and render mask to offscreen canvas
    ctx.clearRect(0, 0, width, height);
    
    // Parse color
    const r = parseInt(colorHex.slice(1, 3), 16) || 0;
    const g = parseInt(colorHex.slice(3, 5), 16) || 0;
    const b = parseInt(colorHex.slice(5, 7), 16) || 0;
    const a = Math.round(opacity * 255);
    
    // Create ImageData and fill directly - much faster than fillRect per pixel
    const imageData = ctx.createImageData(width, height);
    const data = imageData.data;
    
    for (let row = 0; row < height; row++) {
      for (let col = 0; col < width; col++) {
        const bitIndex = row * width + col;
        const byteIndex = Math.floor(bitIndex / 8);
        const bitOffset = bitIndex % 8;
        
        if (byteIndex < maskData.length && (maskData[byteIndex] & (1 << bitOffset)) !== 0) {
          const pixelIndex = (row * width + col) * 4;
          data[pixelIndex] = r;
          data[pixelIndex + 1] = g;
          data[pixelIndex + 2] = b;
          data[pixelIndex + 3] = a;
        }
      }
    }
    
    ctx.putImageData(imageData, 0, 0);
    
    // Create ImageBitmap from the canvas content
    try {
      const bitmap = await createImageBitmap(offscreenCanvas);
      maskBitmapCache.set(cacheKey, bitmap);
      return bitmap;
    } catch (e) {
      console.warn('Failed to create mask bitmap:', e);
      return null;
    }
  }
  
  // Synchronous bitmap lookup (returns null if not yet created)
  function getMaskBitmap(annotationId: string, colorHex: string): ImageBitmap | null {
    return maskBitmapCache.get(`${annotationId}:${colorHex}`) ?? null;
  }
  
  // Track which bitmaps are being created to avoid duplicate work
  const pendingBitmaps = new Set<string>();
  
  // Pre-create bitmaps for visible masks (async, non-blocking)
  $effect(() => {
    if (!globalVisible || !Array.isArray(visibleAnnotations)) return;
    
    const masks = visibleAnnotations.filter(a => 
      a && a.annotation && a.annotation.kind === 'mask_patch' && isInView(a.annotation)
    );
    
    for (const { annotation, color } of masks) {
      const geo = annotation.geometry as MaskGeometry;
      if (!geo || !geo.data_base64) continue;
      
      const cacheKey = `${annotation.id}:${color}`;
      if (maskBitmapCache.has(cacheKey) || pendingBitmaps.has(cacheKey)) continue;
      
      const maskData = getCachedMaskData(annotation.id, geo.data_base64);
      if (!maskData) continue;
      
      // Mark as pending and start async creation
      pendingBitmaps.add(cacheKey);
      getOrCreateMaskBitmap(annotation.id, maskData, geo.width, geo.height, color, 0.5)
        .then(() => {
          pendingBitmaps.delete(cacheKey);
          // Trigger re-render by touching renderTrigger
          if (maskCanvas) {
            maskCanvas.dispatchEvent(new Event('bitmapReady'));
          }
        });
    }
  });
  
  // Pre-create hover bitmap when a mask is hovered or highlighted (so color change is instant)
  $effect(() => {
    const activeId = hoveredMaskId || highlightedId;
    if (!activeId || !globalVisible || !Array.isArray(visibleAnnotations)) return;
    
    const maskEntry = visibleAnnotations.find(a => 
      a && a.annotation && a.annotation.id === activeId && a.annotation.kind === 'mask_patch'
    );
    if (!maskEntry) return;
    
    const annotation = maskEntry.annotation;
    const geo = annotation.geometry as MaskGeometry;
    if (!geo || !geo.data_base64) return;
    
    const cacheKey = `${annotation.id}:${MASK_HOVER_COLOR}`;
    if (maskBitmapCache.has(cacheKey) || pendingBitmaps.has(cacheKey)) return;
    
    const maskData = getCachedMaskData(annotation.id, geo.data_base64);
    if (!maskData) return;
    
    // Create hover bitmap
    pendingBitmaps.add(cacheKey);
    getOrCreateMaskBitmap(annotation.id, maskData, geo.width, geo.height, MASK_HOVER_COLOR, 0.5)
      .then(() => {
        pendingBitmaps.delete(cacheKey);
        if (maskCanvas) {
          maskCanvas.dispatchEvent(new Event('bitmapReady'));
        }
      });
  });
  
  // Listen for bitmap ready events to trigger re-render
  let bitmapReadyTrigger = $state(0);
  $effect(() => {
    if (!maskCanvas) return;
    const handler = () => { bitmapReadyTrigger++; };
    maskCanvas.addEventListener('bitmapReady', handler);
    return () => maskCanvas?.removeEventListener('bitmapReady', handler);
  });
  
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

  // Extract mask-canvas rendering into a callable function
  function renderMaskCanvas() {
    if (!maskCanvas || !globalVisible) return;
    
    const ctx = maskCanvas.getContext('2d', { alpha: true });
    if (!ctx) return;
    
    // Clear the canvas
    ctx.clearRect(0, 0, containerWidth, containerHeight);
    
    // Guard against invalid visible annotations
    if (!Array.isArray(visibleAnnotations)) return;
    
    // Get visible mask annotations (excluding those being edited in mask-paint mode)
    const masks = visibleAnnotations.filter(a => a && a.annotation && a.annotation.kind === 'mask_patch' && isInView(a.annotation) && !maskEditingAnnotationIds.has(a.annotation.id));
    
    // Render stored masks using cached ImageBitmaps
    for (const { annotation, color } of masks) {
      const geo = annotation.geometry as MaskGeometry;
      if (!geo || !geo.data_base64) continue;
      
      const isHovered = hoveredMaskId === annotation.id || highlightedId === annotation.id;
      // Use hover color if hovered, otherwise normal layer color
      const renderColor = isHovered ? MASK_HOVER_COLOR : color;
      const bitmap = getMaskBitmap(annotation.id, renderColor);
      
      if (bitmap) {
        // Fast path: use pre-rendered bitmap
        const screenX = (geo.x0_level0 - viewportX) * viewportZoom;
        const screenY = (geo.y0_level0 - viewportY) * viewportZoom;
        const screenWidth = geo.width * viewportZoom;
        const screenHeight = geo.height * viewportZoom;
        ctx.drawImage(bitmap, screenX, screenY, screenWidth, screenHeight);
      } else if (isHovered) {
        // Hover bitmap not ready yet - trigger creation and fall back to normal color
        const normalBitmap = getMaskBitmap(annotation.id, color);
        if (normalBitmap) {
          const screenX = (geo.x0_level0 - viewportX) * viewportZoom;
          const screenY = (geo.y0_level0 - viewportY) * viewportZoom;
          const screenWidth = geo.width * viewportZoom;
          const screenHeight = geo.height * viewportZoom;
          ctx.drawImage(normalBitmap, screenX, screenY, screenWidth, screenHeight);
        }
      }
      // If bitmap not ready yet, it will be rendered on next trigger
    }
    
    // Render painting preview tiles (these change constantly, so render directly)
    if (modifyPhase === 'mask-paint' && maskAllTiles && Array.isArray(maskAllTiles) && maskAllTiles.length > 0) {
      const previewColor = '#FE0E94'; // secondary color
      for (const tile of maskAllTiles) {
        if (!tile || !tile.origin || !tile.data) continue;
        renderMaskToCanvasDirect(ctx, tile.data, tile.origin.x, tile.origin.y, 512, 512, previewColor, 0.5);
      }
    }

    // Snapshot viewport used for this rasterized frame and clear any temporary transform.
    lastMaskRenderViewportX = viewportX;
    lastMaskRenderViewportY = viewportY;
    lastMaskRenderViewportZoom = Math.max(viewportZoom, 1e-6);
    hasMaskRenderViewport = true;
    maskCanvasTransform = 'none';
  }

  function updateMaskCanvasTransformForInteraction() {
    if (!maskCanvas || !hasMaskRenderViewport || !isInteracting) {
      maskCanvasTransform = 'none';
      return;
    }

    const currentZoom = Math.max(viewportZoom, 1e-6);
    const scale = currentZoom / lastMaskRenderViewportZoom;
    const translateX = (lastMaskRenderViewportX - viewportX) * currentZoom;
    const translateY = (lastMaskRenderViewportY - viewportY) * currentZoom;

    const isNearlyIdentity =
      Math.abs(scale - 1) < 1e-4 &&
      Math.abs(translateX) < 0.01 &&
      Math.abs(translateY) < 0.01;

    maskCanvasTransform = isNearlyIdentity
      ? 'none'
      : `matrix(${scale}, 0, 0, ${scale}, ${translateX}, ${translateY})`;
  }

  // Render all masks to canvas - uses pre-rendered ImageBitmaps for speed
  // During interaction (panning/zooming), defer mask re-renders to avoid
  // blocking the main thread every frame.  Tile rendering (TileRenderer)
  // already handles viewport paint; masks just need to catch up once the
  // user stops moving.
  let _maskRenderRaf: number | null = null;
  let _maskDirtyDuringInteraction = false;

  $effect(() => {
    // Touch reactive deps so Svelte tracks them
    void bitmapReadyTrigger;
    void viewportX; void viewportY; void viewportZoom;
    void containerWidth; void containerHeight;
    void visibleAnnotations; void maskEditingAnnotationIds;
    void modifyPhase; void maskAllTiles; void hoveredMaskId; void highlightedId;

    if (!maskCanvas || !globalVisible) return;

    // Fast path: while actively panning/zooming, avoid re-rasterizing masks but
    // keep them visually aligned using a cheap canvas CSS transform.
    if (isInteracting) {
      updateMaskCanvasTransformForInteraction();
      _maskDirtyDuringInteraction = true;
      return;
    }

    // Not interacting — render immediately (catches catch-up after interaction)
    renderMaskCanvas();
    _maskDirtyDuringInteraction = false;
  });

  // When interaction ends, catch up with a single deferred render
  $effect(() => {
    if (!isInteracting && _maskDirtyDuringInteraction) {
      if (_maskRenderRaf !== null) cancelAnimationFrame(_maskRenderRaf);
      _maskRenderRaf = requestAnimationFrame(() => {
        _maskRenderRaf = null;
        _maskDirtyDuringInteraction = false;
        renderMaskCanvas();
      });
    }
  });
  
  // Reusable canvas for painting preview (avoids creating new canvas every frame)
  let previewCanvas: HTMLCanvasElement | null = null;
  let previewImageData: ImageData | null = null;
  
  // Direct mask rendering for painting preview (not cached, used for live updates)
  function renderMaskToCanvasDirect(
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
    if (!Number.isFinite(x0) || !Number.isFinite(y0)) return;
    if (!Number.isFinite(width) || !Number.isFinite(height)) return;
    if (width <= 0 || height <= 0) return;
    
    // Reuse preview canvas if size matches, otherwise create new
    if (!previewCanvas || previewCanvas.width !== width || previewCanvas.height !== height) {
      previewCanvas = document.createElement('canvas');
      previewCanvas.width = width;
      previewCanvas.height = height;
      previewImageData = null; // Force recreate ImageData
    }
    
    const tempCtx = previewCanvas.getContext('2d', { alpha: true });
    if (!tempCtx) return;
    
    // Reuse or create ImageData
    if (!previewImageData || previewImageData.width !== width || previewImageData.height !== height) {
      previewImageData = tempCtx.createImageData(width, height);
    }
    
    // Clear the data buffer (important since we reuse it)
    const data = previewImageData.data;
    data.fill(0);
    
    // Parse color
    const r = parseInt(colorHex.slice(1, 3), 16) || 0;
    const g = parseInt(colorHex.slice(3, 5), 16) || 0;
    const b = parseInt(colorHex.slice(5, 7), 16) || 0;
    const a = Math.round(opacity * 255);
    
    // Fill set pixels
    for (let row = 0; row < height; row++) {
      for (let col = 0; col < width; col++) {
        const bitIndex = row * width + col;
        const byteIndex = Math.floor(bitIndex / 8);
        const bitOffset = bitIndex % 8;
        
        if (byteIndex < maskData.length && (maskData[byteIndex] & (1 << bitOffset)) !== 0) {
          const pixelIndex = (row * width + col) * 4;
          data[pixelIndex] = r;
          data[pixelIndex + 1] = g;
          data[pixelIndex + 2] = b;
          data[pixelIndex + 3] = a;
        }
      }
    }
    
    tempCtx.putImageData(previewImageData, 0, 0);
    
    // Draw to main canvas with proper transform
    const screenX = (x0 - viewportX) * viewportZoom;
    const screenY = (y0 - viewportY) * viewportZoom;
    const screenWidth = width * viewportZoom;
    const screenHeight = height * viewportZoom;
    ctx.drawImage(previewCanvas, screenX, screenY, screenWidth, screenHeight);
  }

  // Get all visible annotations for this slide
  let visibleAnnotations = $derived.by(() => {
    if (!globalVisible || !slideId) return [];
    
    const result: Array<{ annotation: Annotation; setId: string; setName: string; color: string }> = [];
    
    // Get annotation sets for this specific slide
    const annotationSets = annotationSetsBySlide.get(slideId);
    if (!annotationSets || !Array.isArray(annotationSets)) return [];
    
    const annotationsBySet = annotationsBySlide.get(slideId);
    if (!annotationsBySet) return [];
    
    for (const set of annotationSets) {
      if (!set || !set.id) continue;
      if (!layerVisibility.get(set.id)) continue;
      
      const annotations = annotationsBySet.get(set.id);
      if (!annotations || !Array.isArray(annotations)) continue;
      
      const color = getLayerColor(set.id, slideId);
      
      for (const annotation of annotations) {
        if (!annotation || !annotation.id) continue;
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

  // Handle annotation touch for mobile - track touch start position
  let touchStartPos: { x: number; y: number; time: number } | null = null;
  let touchAnnotation: Annotation | null = null;
  const TOUCH_TAP_THRESHOLD = 15; // max movement to count as a tap
  const TOUCH_TAP_MAX_TIME = 300; // max time for a tap

  function handleAnnotationTouchStart(e: TouchEvent, annotation: Annotation) {
    // Don't stop propagation - allow parent to handle panning
    // But track this touch for potential tap detection
    if (e.touches.length === 1) {
      touchStartPos = { x: e.touches[0].clientX, y: e.touches[0].clientY, time: Date.now() };
      touchAnnotation = annotation;
    }
  }

  function handleAnnotationTouchEnd(e: TouchEvent, annotation: Annotation) {
    if (!touchStartPos || touchAnnotation?.id !== annotation.id) {
      touchStartPos = null;
      touchAnnotation = null;
      return;
    }
    
    const touch = e.changedTouches[0];
    if (!touch) return;
    
    const dx = Math.abs(touch.clientX - touchStartPos.x);
    const dy = Math.abs(touch.clientY - touchStartPos.y);
    const elapsed = Date.now() - touchStartPos.time;
    
    // Only trigger if this was a quick tap (minimal movement and short duration)
    if (dx < TOUCH_TAP_THRESHOLD && dy < TOUCH_TAP_THRESHOLD && elapsed < TOUCH_TAP_MAX_TIME) {
      // For mask annotations, only highlight if the tap is on a painted pixel
      if (annotation.kind === 'mask_patch') {
        const geo = annotation.geometry as MaskGeometry;
        if (!geo || !geo.data_base64) {
          touchStartPos = null;
          touchAnnotation = null;
          return;
        }
        
        // Get mask data (cached)
        const maskData = getCachedMaskData(annotation.id, geo.data_base64);
        if (!maskData) {
          touchStartPos = null;
          touchAnnotation = null;
          return;
        }
        
        // Convert screen position to image coordinates
        const target = e.currentTarget as SVGElement;
        const svg = target.ownerSVGElement;
        if (!svg) {
          touchStartPos = null;
          touchAnnotation = null;
          return;
        }
        const rect = svg.getBoundingClientRect();
        const screenX = touch.clientX - rect.left;
        const screenY = touch.clientY - rect.top;
        const imgX = viewportX + screenX / viewportZoom;
        const imgY = viewportY + screenY / viewportZoom;
        
        // Convert to mask-local coordinates
        const maskX = Math.floor(imgX - geo.x0_level0);
        const maskY = Math.floor(imgY - geo.y0_level0);
        
        // Check bounds
        if (maskX < 0 || maskX >= geo.width || maskY < 0 || maskY >= geo.height) {
          touchStartPos = null;
          touchAnnotation = null;
          return;
        }
        
        // Check if pixel is painted
        const bitIndex = maskY * geo.width + maskX;
        const byteIndex = Math.floor(bitIndex / 8);
        const bitOffset = bitIndex % 8;
        const isPainted = byteIndex < maskData.length && (maskData[byteIndex] & (1 << bitOffset)) !== 0;
        
        // If pixel is not painted, don't highlight - let it clear any existing highlight
        if (!isPainted) {
          touchStartPos = null;
          touchAnnotation = null;
          return;
        }
        
        // Set hoveredMaskId for mask canvas rendering
        hoveredMaskId = annotation.id;
      } else {
        // Clear hoveredMaskId when selecting a non-mask annotation
        hoveredMaskId = null;
      }
      
      e.stopPropagation();
      e.preventDefault();
      // Highlight the annotation (like mouseenter does on desktop)
      annotationStore.setHighlightedAnnotation(annotation.id);
      // Also call the optional click callback
      if (onAnnotationClick) {
        onAnnotationClick(annotation, touch.clientX, touch.clientY);
      }
    }
    
    touchStartPos = null;
    touchAnnotation = null;
  }

  // Handle annotation right-click (mousedown with right button)
  // This records the start of a potential right-click; the menu is shown on mouseup
  function handleRightMouseDown(e: MouseEvent, annotation: Annotation) {
    // Only handle right mouse button
    if (e.button !== 2) return;
    
    // For mask annotations, only trigger if the pixel under cursor is painted
    if (annotation.kind === 'mask_patch') {
      const geo = annotation.geometry as MaskGeometry;
      if (!geo || !geo.data_base64) return;
      
      // Get mask data (cached)
      const maskData = getCachedMaskData(annotation.id, geo.data_base64);
      if (!maskData) return;
      
      // Convert screen position to image coordinates
      const imageX = viewportX + e.clientX / viewportZoom;
      const imageY = viewportY + e.clientY / viewportZoom;
      
      // Check bounds - get the element's bounding rect to get proper offset
      const target = e.currentTarget as SVGElement;
      const svg = target.ownerSVGElement;
      if (!svg) return;
      const rect = svg.getBoundingClientRect();
      const screenX = e.clientX - rect.left;
      const screenY = e.clientY - rect.top;
      const imgX = viewportX + screenX / viewportZoom;
      const imgY = viewportY + screenY / viewportZoom;
      
      // Convert to mask-local coordinates
      const maskX = Math.floor(imgX - geo.x0_level0);
      const maskY = Math.floor(imgY - geo.y0_level0);
      
      // Check bounds
      if (maskX < 0 || maskX >= geo.width || maskY < 0 || maskY >= geo.height) return;
      
      // Check if pixel is painted
      const bitIndex = maskY * geo.width + maskX;
      const byteIndex = Math.floor(bitIndex / 8);
      const bitOffset = bitIndex % 8;
      const isPainted = byteIndex < maskData.length && (maskData[byteIndex] & (1 << bitOffset)) !== 0;
      
      // If pixel is not painted, don't handle the event - let it propagate to viewport
      if (!isPainted) return;
    }
    
    e.preventDefault();
    e.stopPropagation();
    if (onAnnotationRightClick) {
      onAnnotationRightClick(annotation, e.clientX, e.clientY);
    }
  }

  // Prevent native context menu on annotations
  function handleContextMenu(e: MouseEvent) {
    e.preventDefault();
    e.stopPropagation();
  }

  // Handle annotation hover
  function handleMouseEnter(annotation: Annotation) {
    annotationStore.setHighlightedAnnotation(annotation.id);
  }

  function handleMouseLeave() {
    annotationStore.setHighlightedAnnotation(null);
  }

  // Handle mask-specific mousemove to check if cursor is over a painted pixel
  function handleMaskMouseMove(e: MouseEvent, annotation: Annotation) {
    const geo = annotation.geometry as MaskGeometry;
    if (!geo || !geo.data_base64) {
      if (hoveredMaskId === annotation.id) hoveredMaskId = null;
      return;
    }

    // Get mask data (cached)
    const maskData = getCachedMaskData(annotation.id, geo.data_base64);
    if (!maskData) {
      if (hoveredMaskId === annotation.id) hoveredMaskId = null;
      return;
    }

    // Convert screen position to image coordinates
    const target = e.currentTarget as SVGElement;
    const svg = target.ownerSVGElement;
    if (!svg) return;
    const rect = svg.getBoundingClientRect();
    const screenX = e.clientX - rect.left;
    const screenY = e.clientY - rect.top;
    const imgX = viewportX + screenX / viewportZoom;
    const imgY = viewportY + screenY / viewportZoom;

    // Convert to mask-local coordinates
    const maskX = Math.floor(imgX - geo.x0_level0);
    const maskY = Math.floor(imgY - geo.y0_level0);

    // Check bounds
    if (maskX < 0 || maskX >= geo.width || maskY < 0 || maskY >= geo.height) {
      if (hoveredMaskId === annotation.id) hoveredMaskId = null;
      return;
    }

    // Check if pixel is painted
    const bitIndex = maskY * geo.width + maskX;
    const byteIndex = Math.floor(bitIndex / 8);
    const bitOffset = bitIndex % 8;
    const isPainted = byteIndex < maskData.length && (maskData[byteIndex] & (1 << bitOffset)) !== 0;

    if (isPainted) {
      hoveredMaskId = annotation.id;
      annotationStore.setHighlightedAnnotation(annotation.id);
    } else {
      if (hoveredMaskId === annotation.id) hoveredMaskId = null;
      annotationStore.setHighlightedAnnotation(null);
    }
  }

  // Handle mask mouse leave
  function handleMaskMouseLeave(annotation: Annotation) {
    if (hoveredMaskId === annotation.id) hoveredMaskId = null;
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
  const POINT_TOUCH_RADIUS = 20; // Larger invisible hit area for touch devices
  
  // Ellipse stroke width
  const ELLIPSE_STROKE_WIDTH = 2;

  // Decode base64 mask data to Uint8Array
  function decodeMaskData(base64: string): Uint8Array | null {
    try {
      if (!base64 || typeof base64 !== 'string' || base64.length === 0) {
        return null;
      }
      const binaryString = atob(base64);
      if (binaryString.length === 0 || binaryString.length > 100000000) {
        // Guard against empty or unreasonably large data
        return null;
      }
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
    style="transform: {maskCanvasTransform}; transform-origin: 0 0;"
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
            onmousedown={(e) => handleRightMouseDown(e, annotation)}
            oncontextmenu={handleContextMenu}
            onmouseenter={() => handleMouseEnter(annotation)}
            onmouseleave={handleMouseLeave}
            ontouchstart={(e) => handleAnnotationTouchStart(e, annotation)}
            ontouchend={(e) => handleAnnotationTouchEnd(e, annotation)}
          >
            <title>{setName}</title>
            <!-- Invisible larger touch target -->
            <circle 
              cx={screen.x} 
              cy={screen.y} 
              r={POINT_TOUCH_RADIUS}
              fill="transparent"
              stroke="none"
            />
            <!-- Highlight ring (visible when highlighted/selected) -->
            {#if isHighlighted || isSelected}
              <circle 
                cx={screen.x} 
                cy={screen.y} 
                r={POINT_RADIUS + 6}
                fill="none"
                stroke="white"
                stroke-width="2"
                stroke-opacity="0.8"
              />
            {/if}
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
            onmousedown={(e) => handleRightMouseDown(e, annotation)}
            oncontextmenu={handleContextMenu}
            onmouseenter={() => handleMouseEnter(annotation)}
            onmouseleave={handleMouseLeave}
            ontouchstart={(e) => handleAnnotationTouchStart(e, annotation)}
            ontouchend={(e) => handleAnnotationTouchEnd(e, annotation)}
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
            onmousedown={(e) => handleRightMouseDown(e, annotation)}
            oncontextmenu={handleContextMenu}
            onmouseenter={() => handleMouseEnter(annotation)}
            onmouseleave={handleMouseLeave}
            ontouchstart={(e) => handleAnnotationTouchStart(e, annotation)}
            ontouchend={(e) => handleAnnotationTouchEnd(e, annotation)}
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
            onmousedown={(e) => handleRightMouseDown(e, annotation)}
            oncontextmenu={handleContextMenu}
            onmouseenter={() => handleMouseEnter(annotation)}
            onmouseleave={handleMouseLeave}
            ontouchstart={(e) => handleAnnotationTouchStart(e, annotation)}
            ontouchend={(e) => handleAnnotationTouchEnd(e, annotation)}
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
            onmousedown={(e) => handleRightMouseDown(e, annotation)}
            oncontextmenu={handleContextMenu}
            onmousemove={(e) => handleMaskMouseMove(e, annotation)}
            onmouseleave={() => handleMaskMouseLeave(annotation)}
            ontouchstart={(e) => handleAnnotationTouchStart(e, annotation)}
            ontouchend={(e) => handleAnnotationTouchEnd(e, annotation)}
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
      {@const previewColor = '#FE0E94'}
      
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
    /* Prevent selection on touch devices */
    -webkit-touch-callout: none;
    -webkit-user-select: none;
    user-select: none;
  }

  .annotation-overlay {
    position: absolute;
    top: 0;
    left: 0;
    pointer-events: none;
    overflow: visible;
    /* Prevent selection on touch devices */
    -webkit-touch-callout: none;
    -webkit-user-select: none;
    user-select: none;
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
