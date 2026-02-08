<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import { browser } from '$app/environment';
  import {
    type ConnectionState,
    type TileData,
    type ImageDesc,
    type ViewportState,
    type TileCache,
    type RenderMetrics,
    TileRenderer,
    toProtocolViewport,
    zoomAround,
    pan,
    clampViewport,
    centerViewport,
    TILE_SIZE,
    computeIdealLevel,
    visibleTilesForLevel,
    MIN_ZOOM,
    MAX_ZOOM,
  } from '$lib/frusta';
  import Minimap from '$lib/components/Minimap.svelte';
  import ActivityIndicator from '$lib/components/ActivityIndicator.svelte';
  import ViewerHud from '$lib/components/viewer/ViewerHud.svelte';
  import ScaleBar from '$lib/components/viewer/ScaleBar.svelte';
  import MeasurementOverlay from '$lib/components/viewer/MeasurementOverlay.svelte';
  import AnnotationOverlay from '$lib/components/viewer/AnnotationOverlay.svelte';
  import ViewportContextMenu from '$lib/components/ViewportContextMenu.svelte';
  import AnnotationContextMenu from '$lib/components/AnnotationContextMenu.svelte';
  import { annotationStore, activeAnnotationSet } from '$lib/stores/annotations';
  import { authStore } from '$lib/stores/auth';
  import { navigationTarget } from '$lib/stores/navigation';
  import type { Annotation, PointGeometry, EllipseGeometry, PolygonGeometry, MaskGeometry } from '$lib/api/annotations';
  import { tabStore, type Tab } from '$lib/stores/tabs';
  import { acquireCache, releaseCache } from '$lib/stores/slideCache';
  import { updatePerformanceMetrics } from '$lib/stores/metrics';
  import { settings, navigationSettings, imageSettings, performanceSettings, helpMenuOpen, type StainNormalization, type StainEnhancementMode } from '$lib/stores/settings';
  import { toolState, toolCommand, clearToolCommand, updateToolState, resetToolState, type ToolCommand } from '$lib/stores/tools';

  interface Props {
    /** The pane ID this viewer belongs to */
    paneId: string;
    /** The shared frusta WebSocket client */
    client: any;
    /** Current connection state */
    connectionState: ConnectionState;
    /** Map of slideId -> progress info for activity indicators */
    progressInfo: Map<string, { steps: number; total: number; trigger: number }>;
    /** Callback to register this pane's tile handler with the parent */
    onRegisterTileHandler: (paneId: string, handler: { getSlot: () => number | null; handleTile: (tile: TileData) => void }) => void;
    /** Callback to unregister this pane's tile handler */
    onUnregisterTileHandler: (paneId: string) => void;
  }

  let { paneId, client, connectionState, progressInfo, onRegisterTileHandler, onUnregisterTileHandler }: Props = $props();

  // Image state
  let imageDesc = $state<ImageDesc | null>(null);
  let currentSlot = $state<number | null>(null);
  let loadError = $state<string | null>(null);

  // Track the currently active tab handle (tabId) for the viewer
  let activeTabHandle = $state<string | null>(null);
  // The slide ID of the currently displayed slide
  let activeSlideId = $state<string | null>(null);

  // Viewport state
  let viewport = $state<ViewportState>({
    x: 0,
    y: 0,
    width: 800,
    height: 600,
    zoom: 0.1,
  });

  // Tile cache and render trigger
  let cache = $state<TileCache | null>(null);
  let cacheSize = $state(0);
  let tilesReceived = $state(0);
  let renderTrigger = $state(0);

  // Performance metrics
  let renderMetrics = $state<RenderMetrics | null>(null);
  let cacheMemoryBytes = $state(0);
  let pendingDecodes = $state(0);

  // Container ref for sizing
  let container: HTMLDivElement;

  // Debounce timer for viewport updates
  let viewportUpdateTimeout: ReturnType<typeof setTimeout> | null = null;
  const VIEWPORT_UPDATE_DEBOUNCE_MS = 16;

  // Mouse interaction state
  let isDragging = $state(false);
  let lastMouseX = 0;
  let lastMouseY = 0;
  // Track right-click start position for context menu threshold
  let rightClickStart = $state<{ x: number; y: number } | null>(null);
  const RIGHT_CLICK_THRESHOLD = 5; // Pixels - if moved more than this, don't show context menu

  // Measurement tool state
  interface MeasurementState {
    active: boolean;
    mode: 'drag' | 'toggle' | 'pending' | null;
    startScreen: { x: number; y: number } | null;
    endScreen: { x: number; y: number } | null;
    startImage: { x: number; y: number } | null;
    endImage: { x: number; y: number } | null;
  }
  
  let measurement = $state<MeasurementState>({
    active: false,
    mode: null,
    startScreen: null,
    endScreen: null,
    startImage: null,
    endImage: null,
  });

  // Progress
  let progressSteps = $state(0);
  let progressTotal = $state(0);
  let progressUpdateTrigger = $state(0);

  // Context menu state for viewport
  let contextMenuVisible = $state(false);
  let contextMenuX = $state(0);
  let contextMenuY = $state(0);

  // Annotation context menu state
  let annotationMenuVisible = $state(false);
  let annotationMenuX = $state(0);
  let annotationMenuY = $state(0);
  let annotationMenuTarget = $state<Annotation | null>(null);
  // Track annotation right-click start for threshold detection (same as viewport context menu)
  let annotationRightClickStart = $state<{ annotation: Annotation; x: number; y: number } | null>(null);

  // Annotation modification mode state
  type ModifyPhase = 'idle' | 'point-position' | 'multi-point' | 'ellipse-center' | 'ellipse-radii' | 'ellipse-angle' | 'polygon-vertices' | 'polygon-freehand' | 'polygon-edit' | 'mask-paint';
  let modifyMode = $state<{
    pointsCreated?: number; // Track count for multi-point mode
    phase: ModifyPhase;
    annotation: Annotation | null;
    isCreating: boolean;
    tempCenter?: { x: number; y: number };
    tempRadii?: { rx: number; ry: number };
    tempAngleOffset?: number; // Initial angle when entering ellipse-angle phase (to avoid jank)
    tempRotation?: number; // Stored rotation value (used when going back to center/radii phases)
    tempCenterOffset?: { x: number; y: number }; // Offset between cursor and center when re-entering center phase
    // For modification mode: track original values to make edits relative
    originalCenter?: { x: number; y: number };
    originalRadii?: { rx: number; ry: number };
    originalRotation?: number;
    dragStartPos?: { x: number; y: number }; // Where mouse was when entering current phase
    // Polygon-specific state
    polygonVertices?: Array<{ x: number; y: number }>; // Current vertices during creation/editing
    freehandPath?: Array<{ x: number; y: number }>; // Freehand drawing points
    editingVertexIndex?: number | null; // Index of vertex being dragged
    isDraggingPolygon?: boolean; // Whether dragging the entire polygon
    polygonDragStart?: { x: number; y: number }; // Mouse position when polygon drag started
  }>({ phase: 'idle', annotation: null, isCreating: false });

  // Mask painting state
  const MASK_TILE_SIZE = 512;
  const MASK_BYTES = (MASK_TILE_SIZE * MASK_TILE_SIZE) / 8; // 32768 bytes
  let maskBrushSize = $state(20); // Brush size in image pixels
  
  // Multi-tile mask state: Map from "x,y" key to tile state
  interface MaskTileState {
    origin: { x: number; y: number };
    data: Uint8Array;
    annotationId: string | null;
    dirty: boolean;
  }
  let maskTiles = $state<Map<string, MaskTileState>>(new Map());
  
  // Helper to generate tile key from origin
  function getTileKey(x: number, y: number): string {
    return `${x},${y}`;
  }
  
  let isMaskPainting = $state(false); // Whether actively painting (mouse held down)
  let maskSyncTimeout = $state<ReturnType<typeof setTimeout> | null>(null); // Debounce timer
  let maskBrushDragStart = $state<{ y: number } | null>(null); // Y position where middle-mouse drag started
  let maskBrushDragStartSize = $state<number>(20); // Brush size when middle-mouse drag started
  
  // Derived state for AnnotationOverlay (first tile for backward compatibility + all tiles)
  let maskPaintData = $derived.by(() => {
    if (maskTiles.size === 0) return null;
    return maskTiles.values().next().value?.data ?? null;
  });
  let maskTileOrigin = $derived.by(() => {
    if (maskTiles.size === 0) return null;
    return maskTiles.values().next().value?.origin ?? null;
  });
  let maskAllTiles = $derived.by(() => {
    return Array.from(maskTiles.values());
  });

  // Undo/Redo system for annotations
  // Each undo step stores a snapshot of all tiles before a brush stroke
  interface UndoTileSnapshot {
    origin: { x: number; y: number };
    data: Uint8Array;
    annotationId: string | null;
  }
  interface UndoStep {
    type: 'mask-stroke';
    tiles: UndoTileSnapshot[]; // Snapshot of all tiles before the stroke
    description: string; // e.g., "Brush stroke" or "Erase stroke"
  }
  let undoStack = $state<UndoStep[]>([]);
  let redoStack = $state<UndoStep[]>([]);
  let undoBufferSize = $derived($performanceSettings.undoBufferSize);
  let tilesBeforeStroke = $state<Map<string, UndoTileSnapshot>>(new Map()); // Snapshot before current stroke
  let strokeWasErase = $state(false); // Track if current stroke is erasing

  // Track if '1' key is being held for multi-point mode
  let oneKeyHeld = $state(false);

  // Track if '3' key is being held for freehand lasso mode
  let threeKeyHeld = $state(false);

  // Mouse position in image coordinates during modify mode
  let modifyMouseImagePos = $state<{ x: number; y: number } | null>(null);

  // Long press state for mobile context menu
  let longPressTimer: ReturnType<typeof setTimeout> | null = null;
  const LONG_PRESS_MS = 500;
  const LONG_PRESS_MOVE_THRESHOLD = 10; // Pixels of movement allowed before canceling long press
  let longPressStartX = 0;
  let longPressStartY = 0;

  // Settings-derived values for zoom/pan sensitivity
  const sensitivityMap = { low: 0.5, medium: 1.0, high: 2.0 };
  let zoomSensitivityFactor = $derived(sensitivityMap[$navigationSettings.zoomSensitivity] || 1.0);
  let panSensitivityFactor = $derived(sensitivityMap[$navigationSettings.panSensitivity] || 1.0);
  let minimapVisible = $derived($navigationSettings.minimapVisible);
  
  // Stain enhancement mode from image settings
  let stainEnhancement = $derived($imageSettings.stainEnhancement);
  
  // Stain normalization mode from image settings
  let stainNormalization = $derived($imageSettings.stainNormalization);

  // HUD notification state for keyboard shortcut feedback
  let hudNotification = $state<string | null>(null);
  let hudNotificationTimeout: ReturnType<typeof setTimeout> | null = null;
  let hudNotificationFading = $state(false);

  // Normalization modes for cycling with 'n' key
  const normalizationModes: StainNormalization[] = ['none', 'macenko', 'vahadane'];

  // Enhancement modes for cycling with 'e' key
  const enhancementModes: StainEnhancementMode[] = ['none', 'gram', 'afb', 'gms'];
  const enhancementModeNames: Record<StainEnhancementMode, string> = {
    none: 'None',
    gram: 'Gram Stain',
    afb: 'AFB <span class="dim">(Acid-Fast Bacilli)</span>',
    gms: 'GMS <span class="dim">(Grocott Methenamine Silver)</span>',
  };

  function showHudNotification(message: string) {
    // Clear any existing timeout
    if (hudNotificationTimeout) {
      clearTimeout(hudNotificationTimeout);
    }
    
    // Show notification
    hudNotification = message;
    hudNotificationFading = false;
    
    // After 800ms, start fade out
    hudNotificationTimeout = setTimeout(() => {
      hudNotificationFading = true;
      // After 600ms fade, hide completely
      hudNotificationTimeout = setTimeout(() => {
        hudNotification = null;
        hudNotificationFading = false;
        hudNotificationTimeout = null;
      }, 600);
    }, 800);
  }

  function cycleNormalization() {
    const currentIndex = normalizationModes.indexOf($imageSettings.stainNormalization);
    const nextIndex = (currentIndex + 1) % normalizationModes.length;
    const nextMode = normalizationModes[nextIndex];
    
    settings.setSetting('image', 'stainNormalization', nextMode);
    
    // Show notification
    if (nextMode === 'none') {
      showHudNotification('Normalization disabled');
    } else {
      const modeName = nextMode.charAt(0).toUpperCase() + nextMode.slice(1);
      showHudNotification(`Normalization: ${modeName}`);
    }
  }

  function cycleEnhancement() {
    const currentIndex = enhancementModes.indexOf($imageSettings.stainEnhancement);
    const nextIndex = (currentIndex + 1) % enhancementModes.length;
    const nextMode = enhancementModes[nextIndex];
    
    settings.setSetting('image', 'stainEnhancement', nextMode);
    
    // Show notification with full name
    if (nextMode === 'none') {
      showHudNotification('Enhancement disabled');
    } else {
      showHudNotification(`Enhancement: ${enhancementModeNames[nextMode]}`);
    }
  }

  function handleKeyDown(e: KeyboardEvent) {
    // Ignore if user is typing in an input field
    const target = e.target as HTMLElement;
    if (target.tagName === 'INPUT' || target.tagName === 'TEXTAREA' || target.isContentEditable) {
      return;
    }
    
    // Undo: Ctrl+Z (and Cmd+Z on Mac)
    if ((e.ctrlKey || e.metaKey) && e.key === 'z' && !e.shiftKey) {
      e.preventDefault();
      performUndo();
      return;
    }
    
    // Redo: Ctrl+Y or Ctrl+Shift+Z (and Cmd+Shift+Z on Mac)
    if ((e.ctrlKey || e.metaKey) && (e.key === 'y' || (e.key === 'z' && e.shiftKey) || (e.key === 'Z' && e.shiftKey))) {
      e.preventDefault();
      performRedo();
      return;
    }
    
    if (e.key === 'n' || e.key === 'N') {
      if (isPaneFocused) cycleNormalization();
    }
    if (e.key === 'm' || e.key === 'M') {
      if (isPaneFocused) cycleEnhancement();
    }
    if (e.key === 'h' || e.key === 'H') {
      e.preventDefault();
      helpMenuOpen.update(v => !v);
    }
    // 'd' key toggles measurement mode
    if (e.key === 'd' || e.key === 'D') {
      if (!imageDesc || !container) return;
      
      if (measurement.active && measurement.mode === 'toggle') {
        // Cancel measurement if already in toggle mode
        cancelMeasurement();
      } else {
        // Start toggle measurement at current mouse position
        // We'll use the center of the container as default if no mouse position available
        const rect = container.getBoundingClientRect();
        
        // Get current mouse position from last known position or use center
        const screenX = lastMouseX || (rect.left + rect.width / 2);
        const screenY = lastMouseY || (rect.top + rect.height / 2);
        const imagePos = screenToImage(screenX, screenY);
        
        measurement = {
          active: true,
          mode: 'toggle',
          startScreen: { x: screenX, y: screenY },
          endScreen: { x: screenX, y: screenY },
          startImage: imagePos,
          endImage: imagePos,
        };
      }
    }
    // '1' key: hold for multi-point, tap for single point
    if (e.key === '1') {
      if (e.repeat) return; // Ignore key repeat events
      if (!canCreate) {
        if (!isLoggedIn) {
          showHudNotification('Log in to create annotations');
        } else if (!currentActiveSet) {
          showHudNotification('Select an annotation layer first');
        } else if (currentActiveSet.locked) {
          showHudNotification('Layer is locked');
        }
        return;
      }
      oneKeyHeld = true;
      handleStartMultiPointCreation();
    }
    // '2' key starts ellipse creation
    if (e.key === '2') {
      if (!canCreate) {
        if (!isLoggedIn) {
          showHudNotification('Log in to create annotations');
        } else if (!currentActiveSet) {
          showHudNotification('Select an annotation layer first');
        } else if (currentActiveSet.locked) {
          showHudNotification('Layer is locked');
        }
        return;
      }
      handleStartEllipseCreation();
    }
    // '3' key: tap for polygon vertices, hold for freehand lasso
    if (e.key === '3') {
      if (e.repeat) return; // Ignore key repeat events
      if (!canCreate) {
        if (!isLoggedIn) {
          showHudNotification('Log in to create annotations');
        } else if (!currentActiveSet) {
          showHudNotification('Select an annotation layer first');
        } else if (currentActiveSet.locked) {
          showHudNotification('Layer is locked');
        }
        return;
      }
      threeKeyHeld = true;
      handleStartFreehandLasso();
    }
    // '4' key starts mask painting
    if (e.key === '4') {
      if (!canCreate) {
        if (!isLoggedIn) {
          showHudNotification('Log in to create annotations');
        } else if (!currentActiveSet) {
          showHudNotification('Select an annotation layer first');
        } else if (currentActiveSet.locked) {
          showHudNotification('Layer is locked');
        }
        return;
      }
      handleStartMaskPainting();
    }
    // Enter finishes multi-point mode, polygon creation, or mask painting
    if (e.key === 'Enter') {
      if (modifyMode.phase === 'multi-point') {
        cancelModifyMode();
      } else if (modifyMode.phase === 'polygon-vertices' || modifyMode.phase === 'polygon-edit') {
        // Complete polygon if we have enough vertices
        handleCompletePolygon();
      } else if (modifyMode.phase === 'mask-paint') {
        // Confirm and save mask painting
        confirmMaskPainting();
        cancelModifyMode();
        showHudNotification('Mask saved');
      }
    }
    // Q/W/E keys switch ellipse modification phases
    if (e.key === 'q' || e.key === 'Q') {
      // Switch to center/position phase
      if (modifyMode.phase === 'ellipse-radii' || modifyMode.phase === 'ellipse-angle') {
        // If in angle phase, capture current rotation before switching
        let currentRotation = modifyMode.tempRotation;
        if (modifyMode.phase === 'ellipse-angle' && modifyMouseImagePos && modifyMode.tempCenter) {
          if (!modifyMode.isCreating && modifyMode.dragStartPos && modifyMode.tempRotation !== undefined) {
            // Modification mode: compute rotation from delta
            const rawAngle = Math.atan2(modifyMouseImagePos.y - modifyMode.tempCenter.y, modifyMouseImagePos.x - modifyMode.tempCenter.x);
            const dragAngle = Math.atan2(modifyMode.dragStartPos.y - modifyMode.tempCenter.y, modifyMode.dragStartPos.x - modifyMode.tempCenter.x);
            currentRotation = modifyMode.tempRotation + (rawAngle - dragAngle);
          } else {
            // Creation mode
            const rawAngle = Math.atan2(modifyMouseImagePos.y - modifyMode.tempCenter.y, modifyMouseImagePos.x - modifyMode.tempCenter.x);
            currentRotation = rawAngle - (modifyMode.tempAngleOffset ?? 0);
          }
        }
        // If in radii phase in modification mode, capture current radii before switching
        let currentRadii = modifyMode.tempRadii;
        if (modifyMode.phase === 'ellipse-radii' && modifyMouseImagePos && modifyMode.tempCenter) {
          const dx = modifyMouseImagePos.x - modifyMode.tempCenter.x;
          const dy = modifyMouseImagePos.y - modifyMode.tempCenter.y;
          const rot = modifyMode.tempRotation ?? 0;
          const cosR = Math.cos(-rot);
          const sinR = Math.sin(-rot);
          const localX = dx * cosR - dy * sinR;
          const localY = dx * sinR + dy * cosR;
          
          // Use tempRadii as base (current working value), fallback to originalRadii
          const baseRadii = modifyMode.tempRadii ?? modifyMode.originalRadii;
          if (!modifyMode.isCreating && baseRadii && modifyMode.dragStartPos) {
            // Modification mode: compute delta from drag start
            const dragDx = modifyMode.dragStartPos.x - modifyMode.tempCenter.x;
            const dragDy = modifyMode.dragStartPos.y - modifyMode.tempCenter.y;
            const dragLocalX = dragDx * cosR - dragDy * sinR;
            const dragLocalY = dragDx * sinR + dragDy * cosR;
            currentRadii = {
              rx: Math.max(baseRadii.rx + (Math.abs(localX) - Math.abs(dragLocalX)), 1),
              ry: Math.max(baseRadii.ry + (Math.abs(localY) - Math.abs(dragLocalY)), 1),
            };
          } else if (currentRadii) {
            // Creation mode with existing radii
            currentRadii = {
              rx: Math.max(Math.abs(localX), 1),
              ry: Math.max(Math.abs(localY), 1),
            };
          }
        }
        // Store offset between current mouse position and center so ellipse doesn't snap to cursor
        const centerOffset = modifyMouseImagePos && modifyMode.tempCenter
          ? { x: modifyMouseImagePos.x - modifyMode.tempCenter.x, y: modifyMouseImagePos.y - modifyMode.tempCenter.y }
          : { x: 0, y: 0 };
        modifyMode = {
          ...modifyMode,
          phase: 'ellipse-center',
          tempRotation: currentRotation,
          tempRadii: currentRadii,
          tempCenterOffset: centerOffset,
          dragStartPos: undefined, // Reset for center phase
        };
        showHudNotification('Adjusting position (W=size, E=rotation)');
      } else if (modifyMode.phase === 'ellipse-center') {
        showHudNotification('Already adjusting position');
      }
    }
    if (e.key === 'w' || e.key === 'W') {
      // Switch to radii/size phase
      if (modifyMode.phase === 'ellipse-center' && modifyMode.tempCenter) {
        modifyMode = {
          ...modifyMode,
          phase: 'ellipse-radii',
          tempCenterOffset: undefined, // Clear center offset
          // In modification mode, set dragStartPos to current mouse so size doesn't jump
          dragStartPos: !modifyMode.isCreating && modifyMouseImagePos ? modifyMouseImagePos : undefined,
        };
        showHudNotification('Adjusting size (Q=position, E=rotation)');
      } else if (modifyMode.phase === 'ellipse-angle' && modifyMode.tempCenter && modifyMouseImagePos) {
        // Capture current rotation before switching to radii
        let currentRotation = modifyMode.tempRotation;
        if (!modifyMode.isCreating && modifyMode.dragStartPos && modifyMode.tempRotation !== undefined) {
          // Modification mode: compute rotation from delta
          const rawAngle = Math.atan2(modifyMouseImagePos.y - modifyMode.tempCenter.y, modifyMouseImagePos.x - modifyMode.tempCenter.x);
          const dragAngle = Math.atan2(modifyMode.dragStartPos.y - modifyMode.tempCenter.y, modifyMode.dragStartPos.x - modifyMode.tempCenter.x);
          currentRotation = modifyMode.tempRotation + (rawAngle - dragAngle);
        } else {
          // Creation mode
          const rawAngle = Math.atan2(modifyMouseImagePos.y - modifyMode.tempCenter.y, modifyMouseImagePos.x - modifyMode.tempCenter.x);
          currentRotation = rawAngle - (modifyMode.tempAngleOffset ?? 0);
        }
        modifyMode = {
          ...modifyMode,
          phase: 'ellipse-radii',
          tempRotation: currentRotation,
          // In modification mode, set dragStartPos to current mouse so size doesn't jump
          dragStartPos: !modifyMode.isCreating ? modifyMouseImagePos : undefined,
        };
        showHudNotification('Adjusting size (Q=position, E=rotation)');
      } else if (modifyMode.phase === 'ellipse-radii') {
        showHudNotification('Already adjusting size');
      }
    }
    if (e.key === 'e' || e.key === 'E') {
      // Switch to angle/rotation phase
      if (modifyMode.phase === 'ellipse-center' && modifyMode.tempCenter) {
        // Need radii before rotation - use existing or current mouse position for radii
        if (modifyMode.tempRadii) {
          // Already have radii, just switch to angle phase
          if (modifyMouseImagePos) {
            // Compute offset so that current rotation is preserved
            const rawAngle = Math.atan2(modifyMouseImagePos.y - modifyMode.tempCenter.y, modifyMouseImagePos.x - modifyMode.tempCenter.x);
            const desiredRotation = modifyMode.tempRotation ?? 0;
            modifyMode = {
              ...modifyMode,
              phase: 'ellipse-angle',
              tempAngleOffset: rawAngle - desiredRotation,
              tempCenterOffset: undefined,
              // In modification mode, set dragStartPos to current mouse so rotation doesn't jump
              dragStartPos: !modifyMode.isCreating ? modifyMouseImagePos : undefined,
            };
            showHudNotification('Adjusting rotation (Q=position, W=size)');
          }
        } else if (modifyMouseImagePos) {
          // No radii yet - compute from mouse position, accounting for any existing rotation
          const dx = modifyMouseImagePos.x - modifyMode.tempCenter.x;
          const dy = modifyMouseImagePos.y - modifyMode.tempCenter.y;
          const currentRotation = modifyMode.tempRotation ?? 0;
          const cosR = Math.cos(-currentRotation);
          const sinR = Math.sin(-currentRotation);
          const localX = dx * cosR - dy * sinR;
          const localY = dx * sinR + dy * cosR;
          const rx = Math.abs(localX);
          const ry = Math.abs(localY);
          const initialAngle = Math.atan2(dy, dx);
          modifyMode = {
            ...modifyMode,
            phase: 'ellipse-angle',
            tempRadii: { rx: Math.max(rx, 1), ry: Math.max(ry, 1) },
            tempAngleOffset: initialAngle,
            tempCenterOffset: undefined,
            // In modification mode, set dragStartPos to current mouse so rotation doesn't jump
            dragStartPos: !modifyMode.isCreating ? modifyMouseImagePos : undefined,
          };
          showHudNotification('Adjusting rotation (Q=position, W=size)');
        }
      } else if (modifyMode.phase === 'ellipse-radii' && modifyMode.tempCenter && modifyMouseImagePos) {
        // Capture current radii from mouse position and switch to angle
        // Account for existing rotation when computing radii
        const dx = modifyMouseImagePos.x - modifyMode.tempCenter.x;
        const dy = modifyMouseImagePos.y - modifyMode.tempCenter.y;
        const currentRotation = modifyMode.tempRotation ?? 0;
        const cosR = Math.cos(-currentRotation);
        const sinR = Math.sin(-currentRotation);
        const localX = dx * cosR - dy * sinR;
        const localY = dx * sinR + dy * cosR;
        
        let rx: number, ry: number;
        // Use tempRadii as base (current working value), fallback to originalRadii
        const baseRadii = modifyMode.tempRadii ?? modifyMode.originalRadii;
        if (!modifyMode.isCreating && baseRadii && modifyMode.dragStartPos) {
          // Modification mode: compute delta from drag start, apply to base radii
          const dragDx = modifyMode.dragStartPos.x - modifyMode.tempCenter.x;
          const dragDy = modifyMode.dragStartPos.y - modifyMode.tempCenter.y;
          const dragLocalX = dragDx * cosR - dragDy * sinR;
          const dragLocalY = dragDx * sinR + dragDy * cosR;
          rx = Math.max(baseRadii.rx + (Math.abs(localX) - Math.abs(dragLocalX)), 1);
          ry = Math.max(baseRadii.ry + (Math.abs(localY) - Math.abs(dragLocalY)), 1);
        } else {
          // Creation mode
          rx = Math.max(Math.abs(localX), 1);
          ry = Math.max(Math.abs(localY), 1);
        }
        
        // Compute angle offset to preserve existing rotation if any
        const rawAngle = Math.atan2(dy, dx);
        const desiredRotation = modifyMode.tempRotation ?? 0;
        modifyMode = {
          ...modifyMode,
          phase: 'ellipse-angle',
          tempRadii: { rx, ry },
          tempAngleOffset: rawAngle - desiredRotation,
          // In modification mode, set dragStartPos to current mouse so rotation doesn't jump
          dragStartPos: !modifyMode.isCreating ? modifyMouseImagePos : undefined,
        };
        showHudNotification('Adjusting rotation (Q=position, W=size)');
      } else if (modifyMode.phase === 'ellipse-angle') {
        showHudNotification('Already adjusting rotation');
      }
    }
    // Escape closes help and cancels measurement and modify mode
    if (e.key === 'Escape') {
      if ($helpMenuOpen) {
        helpMenuOpen.set(false);
      }
      if (measurement.active) {
        cancelMeasurement();
      }
      if (modifyMode.phase !== 'idle') {
        const wasCreating = modifyMode.isCreating;
        const wasMultiPoint = modifyMode.phase === 'multi-point';
        const wasMaskPaint = modifyMode.phase === 'mask-paint';
        if (wasMaskPaint) {
          // Confirm and save mask painting on Escape
          (async () => {
            await confirmMaskPainting();
            cancelModifyMode();
            showHudNotification('Mask saved');
          })();
        } else {
          cancelModifyMode();
          if (!wasMultiPoint) {
            showHudNotification(wasCreating ? 'Creation cancelled' : 'Modification cancelled');
          }
        }
      }
    }
  }

  function handleKeyUp(e: KeyboardEvent) {
    // Handle '1' key release - exit multi-point mode if active
    if (e.key === '1') {
      oneKeyHeld = false;
      if (modifyMode.phase === 'multi-point') {
        cancelModifyMode();
      }
    }
    // Handle '3' key release - switch to polygon vertices mode if user hasn't started drawing
    if (e.key === '3') {
      threeKeyHeld = false;
      if (modifyMode.phase === 'polygon-freehand') {
        // If user hasn't started drawing yet, switch to polygon vertex mode
        if (!modifyMode.freehandPath || modifyMode.freehandPath.length === 0) {
          handleStartPolygonCreation();
        }
        // If already drawing, keep the freehand mode active
      }
    }
  }

  // Zoom slider: convert linear slider value to logarithmic zoom
  // Slider value 0-100 maps to MIN_ZOOM to MAX_ZOOM logarithmically
  let zoomSliderValue = $derived({
    get value() {
      // Convert zoom to slider position (0-100)
      const logMin = Math.log(MIN_ZOOM);
      const logMax = Math.log(MAX_ZOOM);
      const logZoom = Math.log(viewport.zoom);
      return ((logZoom - logMin) / (logMax - logMin)) * 100;
    }
  });

  function handleZoomSliderChange(e: Event) {
    if (!imageDesc || !container) return;
    const target = e.target as HTMLInputElement;
    const sliderValue = parseFloat(target.value);
    
    // Convert slider position (0-100) to zoom level (logarithmic)
    const logMin = Math.log(MIN_ZOOM);
    const logMax = Math.log(MAX_ZOOM);
    const logZoom = logMin + (sliderValue / 100) * (logMax - logMin);
    const newZoom = Math.exp(logZoom);
    
    // Apply zoom centered on viewport
    const rect = container.getBoundingClientRect();
    const centerX = rect.width / 2;
    const centerY = rect.height / 2;
    
    // Calculate zoom delta from current to new
    const zoomDelta = newZoom / viewport.zoom;
    viewport = zoomAround(viewport, centerX, centerY, zoomDelta, imageDesc.width, imageDesc.height);
    scheduleViewportUpdate();
  }

  function stopSliderPropagation(e: Event) {
    e.stopPropagation();
  }

  // Image adjustment settings - compute CSS filter string
  // Brightness: -100 to 100 maps to CSS brightness 0 to 2 (0 = black, 1 = normal, 2 = double)
  // Contrast: -100 to 100 maps to CSS contrast 0 to 2
  // Gamma: applied via a combination of brightness adjustment (approximation)
  let imageFilter = $derived(() => {
    const b = $imageSettings.brightness;
    const c = $imageSettings.contrast;
    const g = $imageSettings.gamma;
    
    // Map -100..100 to 0..2 for brightness and contrast
    const brightness = 1 + (b / 100);
    const contrast = 1 + (c / 100);
    
    // Gamma is approximated using brightness adjustment
    // gamma < 1 = brighter midtones, gamma > 1 = darker midtones
    // We'll use a subtle additional brightness shift
    const gammaBrightness = g !== 1 ? Math.pow(0.5, g - 1) : 1;
    
    const filters: string[] = [];
    if (brightness !== 1) filters.push(`brightness(${brightness.toFixed(2)})`);
    if (contrast !== 1) filters.push(`contrast(${contrast.toFixed(2)})`);
    if (gammaBrightness !== 1) filters.push(`brightness(${gammaBrightness.toFixed(2)})`);
    
    return filters.length > 0 ? filters.join(' ') : 'none';
  });

  // React to progressInfo changes for our slide
  $effect(() => {
    if (activeSlideId && progressInfo.has(activeSlideId)) {
      const info = progressInfo.get(activeSlideId)!;
      progressSteps = info.steps;
      progressTotal = info.total;
      progressUpdateTrigger = info.trigger;
    }
  });

  /**
   * Convert slide info to ImageDesc for the frusta protocol.
   */
  function slideInfoToImageDesc(tab: Tab): ImageDesc | null {
    const hex = tab.slideId.replace(/-/g, '');
    if (hex.length !== 32) return null;

    const bytes = new Uint8Array(16);
    for (let i = 0; i < 16; i++) {
      bytes[i] = parseInt(hex.substr(i * 2, 2), 16);
    }

    const maxDim = Math.max(tab.width, tab.height);
    const levels = Math.ceil(Math.log2(maxDim / TILE_SIZE)) + 1;

    return {
      id: bytes,
      width: tab.width,
      height: tab.height,
      levels,
    };
  }

  /**
   * Format bytes to human readable string (KB, MB, etc.)
   */
  function formatBytes(bytes: number): string {
    if (bytes < 1024) return `${bytes} B`;
    if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
    if (bytes < 1024 * 1024 * 1024) return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
    return `${(bytes / (1024 * 1024 * 1024)).toFixed(2)} GB`;
  }

  /**
   * Center the viewport on the current image.
   */
  function centerOnImage() {
    if (!imageDesc || !container) return;
    const rect = container.getBoundingClientRect();
    viewport = centerViewport(rect.width, rect.height, imageDesc.width, imageDesc.height);
  }

  /**
   * Center the viewport on a specific point in image coordinates.
   */
  function centerOnPoint(imageX: number, imageY: number) {
    if (!container) return;
    const rect = container.getBoundingClientRect();
    const visibleWidth = rect.width / viewport.zoom;
    const visibleHeight = rect.height / viewport.zoom;
    viewport = {
      ...viewport,
      x: imageX - visibleWidth / 2,
      y: imageY - visibleHeight / 2,
      width: rect.width,
      height: rect.height,
    };
  }

  /**
   * Close the currently open slide over the WebSocket, freeing the slot.
   */
  function closeCurrentSlide() {
    if (currentSlot !== null && client) {
      client.closeSlide(currentSlot);
    }
    currentSlot = null;
  }

  /**
   * Activate a tab: save the previous tab's viewport, close its slot,
   * then set up the new tab's slide for viewing.
   */
  function activateTab(tab: Tab) {
    const newImageDesc = slideInfoToImageDesc(tab);
    if (!newImageDesc) {
      loadError = 'Failed to parse slide info';
      return;
    }

    // Save the current tab's viewport before switching away
    if (activeTabHandle && activeTabHandle !== tab.tabId) {
      tabStore.saveViewport(activeTabHandle, {
        x: viewport.x,
        y: viewport.y,
        zoom: viewport.zoom,
      });
    }

    // Close the previous tab's slot
    closeCurrentSlide();

    const prevSlideId = activeSlideId;

    imageDesc = newImageDesc;
    activeTabHandle = tab.tabId;
    activeSlideId = tab.slideId;
    loadError = null;

    // Hide help when changing slides
    helpMenuOpen.set(false);

    // Reset progress state for new slide
    progressSteps = 0;
    progressTotal = 0;

    // Swap to the shared cache for the new slide
    if (prevSlideId && prevSlideId !== tab.slideId) {
      releaseCache(prevSlideId);
    }
    if (prevSlideId !== tab.slideId) {
      cache = acquireCache(tab.slideId);
      cacheSize = cache.size;
      tilesReceived = cache.size;
    }

    // Restore saved viewport or center on the image
    if (tab.savedViewport) {
      viewport = { ...viewport, x: tab.savedViewport.x, y: tab.savedViewport.y, zoom: tab.savedViewport.zoom };
      if (container) {
        viewport = clampViewport(viewport, newImageDesc.width, newImageDesc.height);
      }
    } else if (container) {
      const rect = container.getBoundingClientRect();
      viewport = centerViewport(rect.width, rect.height, newImageDesc.width, newImageDesc.height);
    }

    // Open the slide on the WebSocket if connected
    openSlide();
  }

  // Subscribe to this pane's active tab and focus state
  let paneActiveTab = $state<Tab | null>(null);
  let isPaneFocused = $state(false);

  const unsubSplit = tabStore.splitState.subscribe((s) => {
    const pane = s.panes.find((p) => p.paneId === paneId);
    if (!pane || !pane.activeTabId) {
      paneActiveTab = null;
    } else {
      paneActiveTab = pane.tabs.find((t) => t.tabId === pane.activeTabId) ?? null;
    }
    isPaneFocused = s.focusedPaneId === paneId;
  });

  // Subscribe to navigation requests from other components (e.g., Sidebar annotation clicks)
  // Only navigate if this pane is displaying the target slide
  const unsubNavigation = navigationTarget.subscribe((target) => {
    if (target && target.slideId === activeSlideId) {
      centerOnPoint(target.x, target.y);
    }
  });

  // Subscribe to auth state for annotation creation permission
  let isLoggedIn = $state(false);
  const unsubAuth = authStore.subscribe((state) => {
    isLoggedIn = state.user !== null;
  });

  // Subscribe to active annotation set for creation permission
  let currentActiveSet = $state<typeof $activeAnnotationSet>(null);
  const unsubActiveSet = activeAnnotationSet.subscribe((v) => {
    currentActiveSet = v;
  });

  // Subscribe to annotation store to access annotations per slide
  let annotationsBySlide = $state<Map<string, Map<string, import('$lib/api/annotations').Annotation[]>>>(new Map());
  const unsubAnnotationsStore = annotationStore.subscribe((state) => {
    annotationsBySlide = state.annotationsBySlide;
  });

  // Can create annotations if logged in and have an unlocked active set
  let canCreate = $derived(isLoggedIn && currentActiveSet !== null && !currentActiveSet.locked);

  // Subscribe to tool commands from AppHeader toolbar
  const unsubToolCommand = toolCommand.subscribe((cmd) => {
    if (!cmd || !isPaneFocused) return;
    
    switch (cmd.type) {
      case 'undo':
        performUndo();
        break;
      case 'redo':
        performRedo();
        break;
      case 'measure':
        // Toggle measurement mode - use "pending" mode when activated from toolbar
        // so the first click sets the start point
        if (!imageDesc || !container) break;
        if (measurement.active) {
          cancelMeasurement();
        } else {
          measurement = {
            active: true,
            mode: 'pending',
            startScreen: null,
            endScreen: null,
            startImage: null,
            endImage: null,
          };
          showHudNotification('Click to start measuring');
        }
        break;
      case 'annotation':
        if (cmd.tool === null) {
          // Deactivate current tool
          if (modifyMode.phase === 'mask-paint') {
            confirmMaskPainting();
            cancelModifyMode();
            showHudNotification('Mask saved');
          } else if (modifyMode.phase !== 'idle') {
            cancelModifyMode();
          }
        } else if (cmd.tool === 'point') {
          if (canCreate) handleStartMultiPointCreation();
        } else if (cmd.tool === 'ellipse') {
          if (canCreate) handleStartEllipseCreation();
        } else if (cmd.tool === 'lasso') {
          if (canCreate) handleStartFreehandLasso();
        } else if (cmd.tool === 'polygon') {
          if (canCreate) handleStartPolygonCreation();
        } else if (cmd.tool === 'mask') {
          if (canCreate) handleStartMaskPainting();
        }
        break;
    }
    clearToolCommand();
  });

  // Update tool state when this pane is focused
  $effect(() => {
    if (isPaneFocused) {
      updateToolState({
        annotationTool: 
          modifyMode.phase === 'multi-point' ? 'point' :
          modifyMode.phase === 'ellipse-center' || modifyMode.phase === 'ellipse-radii' || modifyMode.phase === 'ellipse-angle' ? 'ellipse' :
          modifyMode.phase === 'polygon-freehand' ? 'lasso' :
          modifyMode.phase === 'polygon-vertices' || modifyMode.phase === 'polygon-edit' ? 'polygon' :
          modifyMode.phase === 'mask-paint' ? 'mask' : null,
        measurementActive: measurement.active,
        measurementMode: measurement.mode,
        canUndo: undoStack.length > 0,
        canRedo: redoStack.length > 0,
      });
    } else {
      resetToolState();
    }
  });

  $effect(() => {
    if (!paneActiveTab) {
      closeCurrentSlide();
      if (activeSlideId) {
        releaseCache(activeSlideId);
        cache = null;
        cacheSize = 0;
        tilesReceived = 0;
      }
      imageDesc = null;
      activeTabHandle = null;
      activeSlideId = null;
      return;
    }
    if (paneActiveTab.tabId !== activeTabHandle || paneActiveTab.slideId !== activeSlideId) {
      activateTab(paneActiveTab);
    }
  });

  // Load annotations for this pane's slide (independent of focused pane)
  $effect(() => {
    if (activeSlideId) {
      annotationStore.loadForSlide(activeSlideId);
    }
  });

  // Reactive trigger: when the WebSocket connects (or reconnects), ensure the
  // slide is open and a viewport update is sent so the backend starts streaming
  // tiles.  This covers the permalink-load case where `activateTab` allocates a
  // slot before the socket is ready — the open message is replayed by the
  // client's `reopenTrackedSlides`, but the viewport update was lost.
  //
  // Use scheduleViewportUpdate (debounced) rather than sendViewportUpdate
  // (immediate) to coalesce with other rapid-fire viewport updates during
  // initial layout (resize, center, etc.).  Without this, the server receives
  // many back-to-back updates that cancel each other's tile dispatches.
  $effect(() => {
    if (connectionState === 'connected' && imageDesc && activeTabHandle) {
      if (currentSlot === null) {
        // Slot not yet allocated — full open + viewport update
        openSlide();
      } else {
        // Slot was allocated before the connection was ready.  The client
        // already replayed the open message; we just need to push the
        // current viewport so the server knows which tiles to send.
        scheduleViewportUpdate();
      }
    }
  });

  function openSlide() {
    if (!client || !imageDesc) return;

    const dpi = window.devicePixelRatio * 96;
    const slot = client.openSlide(dpi, imageDesc);
    if (slot === -1) {
      loadError = 'No free slots available';
      return;
    }
    currentSlot = slot;
    // Re-register with updated slot
    registerHandler();
    // Use debounced update instead of immediate — setting currentSlot
    // (a $state variable) will re-trigger the $effect above, which also
    // calls scheduleViewportUpdate().  The two calls coalesce via the
    // shared timeout, so the server receives exactly one Update message
    // instead of two back-to-back (which would cause the second to cancel
    // the first's tile dispatches).
    scheduleViewportUpdate();
  }

  function handleTileReceived(tile: TileData) {
    if (!cache) return;
    const { bitmapReady } = cache.set(tile.meta, tile.data);
    cacheSize = cache.size;
    tilesReceived++;
    // Update memory metrics
    cacheMemoryBytes = cache.getMemoryUsage();
    pendingDecodes = cache.getPendingDecodeCount();
    // Update global store
    updatePerformanceMetrics({
      cacheMemoryBytes,
      pendingDecodes,
      tilesReceived,
      cacheSize,
    });
    // Trigger an immediate render so coarse fallbacks are displayed.
    renderTrigger++;
    // When the bitmap finishes decoding, trigger another render so the
    // crisp version replaces the blurry fallback (progressive loading).
    bitmapReady.then(() => {
      renderTrigger++;
      // Update pending decodes after decode completes
      if (cache) {
        pendingDecodes = cache.getPendingDecodeCount();
        cacheMemoryBytes = cache.getMemoryUsage();
        updatePerformanceMetrics({
          pendingDecodes,
          cacheMemoryBytes,
        });
      }
    });
  }

  function handleRenderMetrics(metrics: RenderMetrics) {
    renderMetrics = metrics;
    // Update global store with render metrics
    updatePerformanceMetrics({
      renderTimeMs: metrics.renderTimeMs,
      fps: metrics.fps,
      visibleTiles: metrics.visibleTiles,
      renderedTiles: metrics.renderedTiles,
      fallbackTiles: metrics.fallbackTiles,
      placeholderTiles: metrics.placeholderTiles,
    });
  }

  function registerHandler() {
    onRegisterTileHandler(paneId, {
      getSlot: () => currentSlot,
      handleTile: handleTileReceived,
    });
  }

  function sendViewportUpdate() {
    if (!client || currentSlot === null) return;
    client.updateViewport(currentSlot, toProtocolViewport(viewport));
  }

  /**
   * Cancel pending decodes for tiles that are no longer visible.
   * This is called when the viewport changes to avoid wasting CPU time
   * decoding tiles that have scrolled out of view.
   */
  function cancelNonVisibleDecodes() {
    if (!cache || !imageDesc) return;

    // Compute visible tiles at the ideal level and one level finer (for 2x DPI)
    const dpi = window.devicePixelRatio * 96;
    const idealLevel = computeIdealLevel(viewport.zoom, imageDesc.levels, dpi);
    const finerLevel = Math.max(0, idealLevel - 1);

    const idealTiles = visibleTilesForLevel(viewport, imageDesc, idealLevel);
    const finerTiles = finerLevel < idealLevel
      ? visibleTilesForLevel(viewport, imageDesc, finerLevel)
      : [];

    // Also include coarser levels as they're used for fallback rendering
    const coarserTiles = [];
    for (let level = idealLevel + 1; level < imageDesc.levels; level++) {
      coarserTiles.push(...visibleTilesForLevel(viewport, imageDesc, level));
    }

    const allVisibleTiles = [...finerTiles, ...idealTiles, ...coarserTiles];
    cache.cancelDecodesNotIn(allVisibleTiles);
  }

  function scheduleViewportUpdate() {
    if (viewportUpdateTimeout) {
      clearTimeout(viewportUpdateTimeout);
    }
    
    // Cancel decodes for tiles that are no longer visible IMMEDIATELY
    // (don't wait for the debounce) to free up decode capacity ASAP
    cancelNonVisibleDecodes();
    
    viewportUpdateTimeout = setTimeout(() => {
      sendViewportUpdate();
      // Keep the tab store's savedViewport in sync so that Copy Permalink
      // (and other consumers) always have the latest viewport.
      if (activeTabHandle) {
        tabStore.saveViewport(activeTabHandle, {
          x: viewport.x,
          y: viewport.y,
          zoom: viewport.zoom,
        });
      }
      viewportUpdateTimeout = null;
    }, VIEWPORT_UPDATE_DEBOUNCE_MS);
  }

  // Handler for minimap viewport changes
  function handleMinimapViewportChange(newViewport: ViewportState) {
    if (!imageDesc) return;
    viewport = clampViewport(newViewport, imageDesc.width, imageDesc.height);
    scheduleViewportUpdate();
  }

  // Convert screen coordinates to image coordinates (level 0 pixels)
  function screenToImage(screenX: number, screenY: number): { x: number; y: number } {
    const rect = container.getBoundingClientRect();
    const relX = screenX - rect.left;
    const relY = screenY - rect.top;
    const imageX = viewport.x + relX / viewport.zoom;
    const imageY = viewport.y + relY / viewport.zoom;
    return { x: imageX, y: imageY };
  }

  // Cancel any active measurement
  function cancelMeasurement() {
    measurement = {
      active: false,
      mode: null,
      startScreen: null,
      endScreen: null,
      startImage: null,
      endImage: null,
    };
  }

  // Mouse event handlers
  function handleMouseDown(e: MouseEvent) {
    // Handle annotation modification mode clicks
    if (modifyMode.phase !== 'idle' && e.button === 0) {
      // For polygon-edit mode, check if we're clicking on a vertex or inside polygon
      if (modifyMode.phase === 'polygon-edit' && modifyMode.polygonVertices) {
        e.preventDefault();
        e.stopPropagation();
        const imagePos = screenToImage(e.clientX, e.clientY);
        
        // Check if clicking on a vertex (within 10px screen distance)
        const clickRadius = 10 / viewport.zoom; // Convert screen pixels to image pixels
        const vertexIndex = modifyMode.polygonVertices.findIndex(v => {
          const dist = Math.sqrt((v.x - imagePos.x) ** 2 + (v.y - imagePos.y) ** 2);
          return dist < clickRadius;
        });
        
        if (vertexIndex >= 0) {
          // Start dragging this vertex
          modifyMode = {
            ...modifyMode,
            editingVertexIndex: vertexIndex,
            isDraggingPolygon: false,
          };
          return;
        }
        
        // Check if clicking inside the polygon
        if (isPointInPolygon(imagePos, modifyMode.polygonVertices)) {
          // Start dragging the entire polygon
          modifyMode = {
            ...modifyMode,
            isDraggingPolygon: true,
            polygonDragStart: imagePos,
            editingVertexIndex: null,
          };
          return;
        }
        
        // Clicking outside - just update mouse position
        return;
      }
      
      // For polygon-vertices mode, clicking adds a vertex
      if (modifyMode.phase === 'polygon-vertices') {
        e.preventDefault();
        e.stopPropagation();
        handlePolygonVertexClick(e);
        return;
      }
      
      // For polygon-freehand mode, start/continue freehand drawing
      if (modifyMode.phase === 'polygon-freehand') {
        e.preventDefault();
        e.stopPropagation();
        const imagePos = screenToImage(e.clientX, e.clientY);
        modifyMode = {
          ...modifyMode,
          freehandPath: [imagePos],
        };
        return;
      }
      
      // For mask-paint mode, start painting
      if (modifyMode.phase === 'mask-paint') {
        e.preventDefault();
        e.stopPropagation();
        const imagePos = screenToImage(e.clientX, e.clientY);
        // Capture mask state before stroke for undo
        captureUndoState();
        strokeWasErase = e.altKey;
        isMaskPainting = true;
        paintMaskBrush(imagePos.x, imagePos.y, e.altKey);
        return;
      }
      
      e.preventDefault();
      e.stopPropagation();
      handleModifyClick(e);
      return;
    }

    // Middle mouse button (button 1) - in mask-paint mode, adjust brush size
    if (e.button === 1 && modifyMode.phase === 'mask-paint') {
      e.preventDefault();
      e.stopPropagation();
      maskBrushDragStart = { y: e.clientY };
      maskBrushDragStartSize = maskBrushSize;
      return;
    }

    // Middle mouse button (button 1) - momentary measurement tool
    if (e.button === 1) {
      e.preventDefault();
      e.stopPropagation();
      const imagePos = screenToImage(e.clientX, e.clientY);
      measurement = {
        active: true,
        mode: 'drag',
        startScreen: { x: e.clientX, y: e.clientY },
        endScreen: { x: e.clientX, y: e.clientY },
        startImage: imagePos,
        endImage: imagePos,
      };
      return;
    }

    // Right mouse button (button 2) - pan viewport, show context menu on release if no drag
    if (e.button === 2) {
      e.preventDefault();
      isDragging = true;
      lastMouseX = e.clientX;
      lastMouseY = e.clientY;
      rightClickStart = { x: e.clientX, y: e.clientY };
      return;
    }

    // Left mouse button - regular pan, but also cancel toggle measurement
    if (e.button === 0) {
      // If measurement is in 'pending' mode (from toolbar), start measuring from this click
      if (measurement.active && measurement.mode === 'pending') {
        const imagePos = screenToImage(e.clientX, e.clientY);
        measurement = {
          active: true,
          mode: 'toggle',
          startScreen: { x: e.clientX, y: e.clientY },
          endScreen: { x: e.clientX, y: e.clientY },
          startImage: imagePos,
          endImage: imagePos,
        };
        e.preventDefault();
        return;
      }
      // Cancel toggle measurement mode on click
      if (measurement.active && measurement.mode === 'toggle') {
        cancelMeasurement();
      }
      isDragging = true;
      lastMouseX = e.clientX;
      lastMouseY = e.clientY;
      tabStore.setFocusedPane(paneId);
      helpMenuOpen.set(false);
      e.preventDefault();
    }
  }

  function handleMouseMove(e: MouseEvent) {
    // Handle mask brush size adjustment via middle-mouse drag
    if (modifyMode.phase === 'mask-paint' && maskBrushDragStart !== null) {
      const deltaY = maskBrushDragStart.y - e.clientY; // Up = increase, down = decrease
      const newSize = Math.max(1, Math.min(200, maskBrushDragStartSize + deltaY));
      maskBrushSize = Math.round(newSize);
      return;
    }
    
    // Handle continuous mask painting - use e.buttons to check if left button is held
    // e.buttons is a bitmask: 1 = left button, 2 = right button, 4 = middle button
    if (modifyMode.phase === 'mask-paint' && (e.buttons & 1)) {
      const imagePos = screenToImage(e.clientX, e.clientY);
      modifyMouseImagePos = imagePos; // Update for brush cursor preview
      isMaskPainting = true; // Ensure flag is set
      paintMaskBrush(imagePos.x, imagePos.y, e.altKey);
      return;
    }
    
    // Track mouse position for modify mode preview (including mask-paint for brush cursor)
    if (modifyMode.phase !== 'idle') {
      const newMousePos = screenToImage(e.clientX, e.clientY);
      modifyMouseImagePos = newMousePos;
      
      // Handle polygon vertex dragging
      if (modifyMode.phase === 'polygon-edit' && modifyMode.editingVertexIndex !== null && modifyMode.editingVertexIndex !== undefined) {
        const vertices = [...(modifyMode.polygonVertices ?? [])];
        vertices[modifyMode.editingVertexIndex] = newMousePos;
        modifyMode = {
          ...modifyMode,
          polygonVertices: vertices,
        };
        return;
      }
      
      // Handle polygon dragging (moving entire shape)
      if (modifyMode.phase === 'polygon-edit' && modifyMode.isDraggingPolygon && modifyMode.polygonDragStart) {
        const dx = newMousePos.x - modifyMode.polygonDragStart.x;
        const dy = newMousePos.y - modifyMode.polygonDragStart.y;
        const vertices = (modifyMode.polygonVertices ?? []).map(v => ({
          x: v.x + dx,
          y: v.y + dy,
        }));
        modifyMode = {
          ...modifyMode,
          polygonVertices: vertices,
          polygonDragStart: newMousePos,
        };
        return;
      }
      
      // Handle freehand lasso drawing - only add points if already started (has at least 1 point)
      if (modifyMode.phase === 'polygon-freehand' && modifyMode.freehandPath && modifyMode.freehandPath.length > 0) {
        modifyMode = {
          ...modifyMode,
          freehandPath: [...modifyMode.freehandPath, newMousePos],
        };
        return;
      }
      
      // For modification mode (not creation): set initial offsets/dragStartPos on first mouse move
      if (!modifyMode.isCreating && modifyMode.originalCenter) {
        // Set centerOffset if not yet set (for ellipse-center phase)
        if (modifyMode.phase === 'ellipse-center' && !modifyMode.tempCenterOffset) {
          modifyMode = {
            ...modifyMode,
            tempCenterOffset: { x: newMousePos.x - modifyMode.originalCenter.x, y: newMousePos.y - modifyMode.originalCenter.y },
          };
        }
        // Set dragStartPos if not yet set (for radii/angle phases)
        if ((modifyMode.phase === 'ellipse-radii' || modifyMode.phase === 'ellipse-angle') && !modifyMode.dragStartPos) {
          modifyMode = {
            ...modifyMode,
            dragStartPos: newMousePos,
          };
        }
      }
    }

    // Handle measurement mode (toggle or drag mode)
    if (measurement.active && measurement.startImage) {
      const imagePos = screenToImage(e.clientX, e.clientY);
      measurement = {
        ...measurement,
        endScreen: { x: e.clientX, y: e.clientY },
        endImage: imagePos,
      };
    }

    // Regular pan handling
    if (!isDragging || !imageDesc) return;

    const deltaX = e.clientX - lastMouseX;
    const deltaY = e.clientY - lastMouseY;
    lastMouseX = e.clientX;
    lastMouseY = e.clientY;

    // Close context menu when panning starts
    if (contextMenuVisible) {
      contextMenuVisible = false;
    }

    // If panning during toggle measurement, cancel the measurement
    if (measurement.active && measurement.mode === 'toggle') {
      cancelMeasurement();
    }

    // Apply pan sensitivity from settings
    viewport = pan(viewport, deltaX * panSensitivityFactor, deltaY * panSensitivityFactor, imageDesc.width, imageDesc.height);
    scheduleViewportUpdate();
  }

  function handleMouseUp(e?: MouseEvent) {
    // End mask brush size adjustment
    if (e && e.button === 1 && maskBrushDragStart !== null) {
      maskBrushDragStart = null;
      return;
    }
    
    // Stop mask painting on left button release
    if (e && e.button === 0 && modifyMode.phase === 'mask-paint' && isMaskPainting) {
      isMaskPainting = false;
      // Commit the brush stroke to the undo stack
      commitUndoStep();
      // Sync to backend after stroke completes
      scheduleMaskSync();
      return;
    }
    
    // Middle mouse button released - stop brush size adjustment or measurement
    if (e && e.button === 1) {
      if (maskBrushDragStart !== null) {
        maskBrushDragStart = null;
      } else if (measurement.active && measurement.mode === 'drag') {
        // End momentary measurement
        cancelMeasurement();
      }
      return;
    }
    
    // Right mouse button released - stop panning, show context menu if no drag
    if (e && e.button === 2) {
      isDragging = false;
      
      // First check if we started a right-click on an annotation
      if (annotationRightClickStart) {
        const dx = e.clientX - annotationRightClickStart.x;
        const dy = e.clientY - annotationRightClickStart.y;
        const dist = Math.sqrt(dx * dx + dy * dy);
        if (dist < RIGHT_CLICK_THRESHOLD) {
          // Didn't move much, show annotation context menu
          showAnnotationMenu(annotationRightClickStart.annotation, e.clientX, e.clientY);
        }
        annotationRightClickStart = null;
        return;
      }
      
      // Otherwise check for viewport right-click
      if (rightClickStart) {
        const dx = e.clientX - rightClickStart.x;
        const dy = e.clientY - rightClickStart.y;
        const dist = Math.sqrt(dx * dx + dy * dy);
        if (dist < RIGHT_CLICK_THRESHOLD) {
          // Didn't move much, show context menu
          showContextMenu(e.clientX, e.clientY);
        }
        rightClickStart = null;
      }
      return;
    }
    
    // Handle polygon vertex drag end
    if (modifyMode.phase === 'polygon-edit' && modifyMode.editingVertexIndex !== null) {
      modifyMode = {
        ...modifyMode,
        editingVertexIndex: null,
      };
      return;
    }
    
    // Handle polygon drag end
    if (modifyMode.phase === 'polygon-edit' && modifyMode.isDraggingPolygon) {
      modifyMode = {
        ...modifyMode,
        isDraggingPolygon: false,
        polygonDragStart: undefined,
      };
      return;
    }
    
    // Handle freehand lasso completion
    if (modifyMode.phase === 'polygon-freehand' && modifyMode.freehandPath && modifyMode.freehandPath.length >= 3) {
      // Discretize the freehand path into polygon vertices
      const tolerance = 5 / viewport.zoom; // Tolerance in image coordinates
      const discretizedVertices = discretizeFreehandPath(modifyMode.freehandPath, tolerance);
      
      if (discretizedVertices.length >= 3) {
        modifyMode = {
          ...modifyMode,
          phase: 'polygon-edit',
          polygonVertices: discretizedVertices,
          freehandPath: undefined,
          editingVertexIndex: null,
          isDraggingPolygon: false,
        };
        showHudNotification(`${discretizedVertices.length} vertices created, Enter to confirm`);
      } else {
        showHudNotification('Draw a larger shape');
        modifyMode = {
          ...modifyMode,
          freehandPath: [],
        };
      }
      return;
    }

    isDragging = false;
  }

  // Window event handlers (with event parameter)
  function handleWindowMouseUp(e: MouseEvent) {
    handleMouseUp(e);
  }

  function handleWindowMouseMove(e: MouseEvent) {
    // Track mouse position globally for 'd' key to use current mouse position
    lastMouseX = e.clientX;
    lastMouseY = e.clientY;
  }

  function handleWheel(e: WheelEvent) {
    if (!imageDesc) return;
    e.preventDefault();
    helpMenuOpen.set(false);

    const rect = container.getBoundingClientRect();
    const mouseX = e.clientX - rect.left;
    const mouseY = e.clientY - rect.top;

    // Apply zoom sensitivity from settings
    const baseZoom = 1.15;
    const sensitiveZoom = 1 + (baseZoom - 1) * zoomSensitivityFactor;
    const zoomFactor = e.deltaY < 0 ? sensitiveZoom : 1 / sensitiveZoom;
    viewport = zoomAround(viewport, mouseX, mouseY, zoomFactor, imageDesc.width, imageDesc.height);
    scheduleViewportUpdate();
  }

  // HUD zoom change - set zoom to specific level centered on viewport
  function handleHudZoomChange(newZoom: number) {
    if (!imageDesc || !container) return;
    const rect = container.getBoundingClientRect();
    const centerX = rect.width / 2;
    const centerY = rect.height / 2;
    
    // Clamp zoom to valid range
    const clampedZoom = Math.max(MIN_ZOOM, Math.min(MAX_ZOOM, newZoom));
    const zoomDelta = clampedZoom / viewport.zoom;
    viewport = zoomAround(viewport, centerX, centerY, zoomDelta, imageDesc.width, imageDesc.height);
    scheduleViewportUpdate();
  }

  function handleHudFitView() {
    centerOnImage();
    scheduleViewportUpdate();
  }

  // Touch event handlers for mobile
  let lastTouchDistance = 0;
  let lastTouchCenter = { x: 0, y: 0 };

  function handleTouchStart(e: TouchEvent) {
    cancelLongPress();
    
    if (e.touches.length === 1) {
      lastMouseX = e.touches[0].clientX;
      lastMouseY = e.touches[0].clientY;
      
      // Start longpress timer for context menu (only when viewing an image)
      if (imageDesc) {
        const touch = e.touches[0];
        longPressStartX = touch.clientX;
        longPressStartY = touch.clientY;
        // Don't set isDragging yet - wait to see if it's a long press or pan
        isDragging = false;
        longPressTimer = setTimeout(() => {
          longPressTimer = null;
          showContextMenu(touch.clientX, touch.clientY);
        }, LONG_PRESS_MS);
      } else {
        isDragging = true;
      }
    } else if (e.touches.length === 2) {
      isDragging = false;
      lastTouchDistance = getTouchDistance(e.touches);
      lastTouchCenter = getTouchCenter(e.touches);
    }
    tabStore.setFocusedPane(paneId);
    helpMenuOpen.set(false);
    e.preventDefault();
  }

  function handleTouchMove(e: TouchEvent) {
    // Check if we should cancel long press and start panning
    if (longPressTimer && e.touches.length === 1) {
      const dx = Math.abs(e.touches[0].clientX - longPressStartX);
      const dy = Math.abs(e.touches[0].clientY - longPressStartY);
      if (dx > LONG_PRESS_MOVE_THRESHOLD || dy > LONG_PRESS_MOVE_THRESHOLD) {
        cancelLongPress();
        // Now start panning
        isDragging = true;
        lastMouseX = e.touches[0].clientX;
        lastMouseY = e.touches[0].clientY;
      }
    }
    
    if (!imageDesc) return;

    // Only pan if we've cancelled long press and are now dragging
    if (e.touches.length === 1 && isDragging) {
      const deltaX = e.touches[0].clientX - lastMouseX;
      const deltaY = e.touches[0].clientY - lastMouseY;
      lastMouseX = e.touches[0].clientX;
      lastMouseY = e.touches[0].clientY;

      viewport = pan(viewport, deltaX, deltaY, imageDesc.width, imageDesc.height);
      scheduleViewportUpdate();
    } else if (e.touches.length === 2) {
      const distance = getTouchDistance(e.touches);
      const center = getTouchCenter(e.touches);

      if (lastTouchDistance > 0) {
        const rect = container.getBoundingClientRect();
        const zoomFactor = distance / lastTouchDistance;
        const centerX = center.x - rect.left;
        const centerY = center.y - rect.top;

        viewport = zoomAround(viewport, centerX, centerY, zoomFactor, imageDesc.width, imageDesc.height);
        scheduleViewportUpdate();
      }

      lastTouchDistance = distance;
      lastTouchCenter = center;
    }

    e.preventDefault();
  }

  function handleTouchEnd(e: TouchEvent) {
    cancelLongPress();
    
    if (e.touches.length === 0) {
      isDragging = false;
      lastTouchDistance = 0;
    } else if (e.touches.length === 1) {
      isDragging = true;
      lastMouseX = e.touches[0].clientX;
      lastMouseY = e.touches[0].clientY;
      lastTouchDistance = 0;
    }
  }

  function getTouchDistance(touches: TouchList): number {
    const dx = touches[0].clientX - touches[1].clientX;
    const dy = touches[0].clientY - touches[1].clientY;
    return Math.sqrt(dx * dx + dy * dy);
  }

  function getTouchCenter(touches: TouchList): { x: number; y: number } {
    return {
      x: (touches[0].clientX + touches[1].clientX) / 2,
      y: (touches[0].clientY + touches[1].clientY) / 2,
    };
  }

  // Context menu handlers
  let contextMenuImageX = $state<number | undefined>(undefined);
  let contextMenuImageY = $state<number | undefined>(undefined);

  function showContextMenu(x: number, y: number) {
    if (!imageDesc) return;
    contextMenuX = x;
    contextMenuY = y;
    // Convert to image coordinates
    if (container && viewport) {
      const imagePos = screenToImage(x, y);
      contextMenuImageX = imagePos.x;
      contextMenuImageY = imagePos.y;
    }
    contextMenuVisible = true;
  }

  function handleContextMenu(e: MouseEvent) {
    // Prevent native context menu - we handle it via mouseup
    e.preventDefault();
    e.stopPropagation();
  }

  function handleContextMenuClose() {
    contextMenuVisible = false;
    contextMenuImageX = undefined;
    contextMenuImageY = undefined;
  }

  // Annotation context menu handlers
  function handleAnnotationRightClick(annotation: Annotation, screenX: number, screenY: number) {
    // Track right-click start on annotation - menu will be shown on mouseup if threshold is met
    // Clear rightClickStart so viewport context menu doesn't also appear
    rightClickStart = null;
    annotationRightClickStart = { annotation, x: screenX, y: screenY };
    // Also enable panning - RMB should always pan
    isDragging = true;
    lastMouseX = screenX;
    lastMouseY = screenY;
  }

  function showAnnotationMenu(annotation: Annotation, x: number, y: number) {
    annotationMenuX = x;
    annotationMenuY = y;
    annotationMenuTarget = annotation;
    annotationMenuVisible = true;
  }

  function handleAnnotationMenuClose() {
    annotationMenuVisible = false;
    annotationMenuTarget = null;
  }

  function handleAnnotationModify(annotation: Annotation) {
    // Start modification mode based on annotation kind
    if (annotation.kind === 'point') {
      modifyMode = { phase: 'point-position', annotation, isCreating: false };
      showHudNotification('Click to set new position');
    } else if (annotation.kind === 'ellipse') {
      // For modification, store original values so edits are relative
      const geo = annotation.geometry as EllipseGeometry;
      const originalCenter = { x: geo.cx_level0, y: geo.cy_level0 };
      const originalRadii = { rx: geo.radius_x, ry: geo.radius_y };
      const originalRotation = geo.rotation_radians;
      // Initialize tempCenter/tempRadii/tempRotation to original values
      // dragStartPos will be set when we know the mouse position
      modifyMode = {
        phase: 'ellipse-center',
        annotation,
        isCreating: false,
        tempCenter: originalCenter,
        tempRadii: originalRadii,
        tempRotation: originalRotation,
        originalCenter,
        originalRadii,
        originalRotation,
        // tempCenterOffset will be computed from first mouse position
      };
      showHudNotification('Drag to adjust position (W=size, E=rotation)');
    } else if (annotation.kind === 'polygon') {
      // For polygon modification, load existing vertices
      const geo = annotation.geometry as PolygonGeometry;
      const vertices = geo.path.map(([x, y]) => ({ x, y }));
      modifyMode = {
        phase: 'polygon-edit',
        annotation,
        isCreating: false,
        polygonVertices: vertices,
        editingVertexIndex: null,
        isDraggingPolygon: false,
      };
      showHudNotification('Drag vertices to move, drag inside to reposition, Enter to confirm');
    }
  }

  function handleStartPolygonCreation() {
    // Start interactive polygon creation - clicking adds vertices
    modifyMode = {
      phase: 'polygon-vertices',
      annotation: null,
      isCreating: true,
      polygonVertices: [],
    };
    showHudNotification('Click to add vertices, Enter to finish (min 3)');
  }

  async function handleCompletePolygon() {
    const vertices = modifyMode.polygonVertices;
    if (!vertices || vertices.length < 3) {
      showHudNotification('Need at least 3 vertices');
      return;
    }

    const path: [number, number][] = vertices.map(v => [v.x, v.y]);
    const geometry: PolygonGeometry = { path };

    try {
      if (modifyMode.isCreating) {
        await annotationStore.createAnnotation({
          kind: 'polygon',
          geometry,
          label_id: 'unlabeled',
        });
        showHudNotification('Polygon created');
      } else if (modifyMode.annotation) {
        await annotationStore.updateAnnotation(modifyMode.annotation.id, { geometry });
        showHudNotification('Polygon updated');
      }
    } catch (err) {
      console.error('Failed to save polygon:', err);
      showHudNotification('Failed to save polygon');
    }
    cancelModifyMode();
  }

  function handleStartEllipseCreation() {
    // Start interactive ellipse creation at center selection phase
    modifyMode = {
      phase: 'ellipse-center',
      annotation: null,
      isCreating: true,
    };
    showHudNotification('Click to set center');
  }

  function handleStartPointCreation() {
    // Start interactive point creation
    modifyMode = {
      phase: 'point-position',
      annotation: null,
      isCreating: true,
    };
    showHudNotification('Click to place point');
  }

  function handleStartMultiPointCreation() {
    // Start multi-point creation mode
    modifyMode = {
      phase: 'multi-point',
      annotation: null,
      isCreating: true,
      pointsCreated: 0,
    };
  }

  // Helper: check if point is inside polygon using ray casting algorithm
  function isPointInPolygon(point: { x: number; y: number }, vertices: Array<{ x: number; y: number }>): boolean {
    if (vertices.length < 3) return false;
    
    let inside = false;
    for (let i = 0, j = vertices.length - 1; i < vertices.length; j = i++) {
      const xi = vertices[i].x, yi = vertices[i].y;
      const xj = vertices[j].x, yj = vertices[j].y;
      
      if (((yi > point.y) !== (yj > point.y)) &&
          (point.x < (xj - xi) * (point.y - yi) / (yj - yi) + xi)) {
        inside = !inside;
      }
    }
    return inside;
  }

  // Helper: discretize freehand path into polygon vertices using Douglas-Peucker algorithm
  function discretizeFreehandPath(path: Array<{ x: number; y: number }>, tolerance: number): Array<{ x: number; y: number }> {
    if (path.length < 3) return path;
    
    // Douglas-Peucker algorithm implementation
    function perpendicularDistance(point: { x: number; y: number }, lineStart: { x: number; y: number }, lineEnd: { x: number; y: number }): number {
      const dx = lineEnd.x - lineStart.x;
      const dy = lineEnd.y - lineStart.y;
      const mag = Math.sqrt(dx * dx + dy * dy);
      if (mag === 0) return Math.sqrt((point.x - lineStart.x) ** 2 + (point.y - lineStart.y) ** 2);
      
      const u = ((point.x - lineStart.x) * dx + (point.y - lineStart.y) * dy) / (mag * mag);
      const closestX = lineStart.x + u * dx;
      const closestY = lineStart.y + u * dy;
      return Math.sqrt((point.x - closestX) ** 2 + (point.y - closestY) ** 2);
    }
    
    function douglasPeucker(points: Array<{ x: number; y: number }>, epsilon: number): Array<{ x: number; y: number }> {
      if (points.length < 3) return points;
      
      let maxDist = 0;
      let maxIndex = 0;
      const first = points[0];
      const last = points[points.length - 1];
      
      for (let i = 1; i < points.length - 1; i++) {
        const dist = perpendicularDistance(points[i], first, last);
        if (dist > maxDist) {
          maxDist = dist;
          maxIndex = i;
        }
      }
      
      if (maxDist > epsilon) {
        const left = douglasPeucker(points.slice(0, maxIndex + 1), epsilon);
        const right = douglasPeucker(points.slice(maxIndex), epsilon);
        return [...left.slice(0, -1), ...right];
      }
      
      return [first, last];
    }
    
    return douglasPeucker(path, tolerance);
  }

  function handlePolygonVertexClick(e: MouseEvent) {
    const imagePos = screenToImage(e.clientX, e.clientY);
    const vertices = modifyMode.polygonVertices ?? [];
    
    // Add the new vertex
    const newVertices = [...vertices, imagePos];
    modifyMode = {
      ...modifyMode,
      polygonVertices: newVertices,
    };
    
    // Auto-close notification when we have 3+ vertices
    if (newVertices.length >= 3) {
      showHudNotification(`${newVertices.length} vertices (Enter to finish, Esc to cancel)`);
    } else {
      showHudNotification(`${newVertices.length}/3 vertices (need at least 3)`);
    }
  }

  function handleStartFreehandLasso() {
    // Start freehand lasso mode - click and drag to draw
    // freehandPath starts undefined until user clicks
    modifyMode = {
      phase: 'polygon-freehand',
      annotation: null,
      isCreating: true,
      freehandPath: undefined,
    };
    showHudNotification('Click and drag to draw, release to finish');
  }

  function handleStartMaskPainting() {
    // Start mask painting mode
    // Initialize empty tile map
    maskTiles = new Map();
    isMaskPainting = false;
    
    // Clear undo/redo stacks for new mask session
    undoStack = [];
    redoStack = [];
    tilesBeforeStroke = new Map();
    
    modifyMode = {
      phase: 'mask-paint',
      annotation: null,
      isCreating: true,
    };
    showHudNotification('Paint mask, Alt = erase, Esc/Enter = save');
  }

  // Helper: Set a pixel in the mask data
  function setMaskPixel(data: Uint8Array, x: number, y: number, value: boolean) {
    if (x < 0 || x >= MASK_TILE_SIZE || y < 0 || y >= MASK_TILE_SIZE) return;
    const byteIndex = Math.floor((y * MASK_TILE_SIZE + x) / 8);
    const bitIndex = (y * MASK_TILE_SIZE + x) % 8;
    if (value) {
      data[byteIndex] |= (1 << bitIndex);
    } else {
      data[byteIndex] &= ~(1 << bitIndex);
    }
  }

  // Helper: Get a pixel from the mask data
  function getMaskPixel(data: Uint8Array, x: number, y: number): boolean {
    if (x < 0 || x >= MASK_TILE_SIZE || y < 0 || y >= MASK_TILE_SIZE) return false;
    const byteIndex = Math.floor((y * MASK_TILE_SIZE + x) / 8);
    const bitIndex = (y * MASK_TILE_SIZE + x) % 8;
    return (data[byteIndex] & (1 << bitIndex)) !== 0;
  }

  // Helper: Decode base64 mask data to Uint8Array
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

  // Helper: Find existing mask annotation at a tile location
  function findExistingMaskAtTile(tileX: number, tileY: number): { id: string; data: Uint8Array } | null {
    if (!activeSlideId || !currentActiveSet) return null;
    
    const slideAnnotations = annotationsBySlide.get(activeSlideId);
    if (!slideAnnotations) return null;
    
    const setAnnotations = slideAnnotations.get(currentActiveSet.id);
    if (!setAnnotations) return null;
    
    for (const annotation of setAnnotations) {
      if (annotation.kind !== 'mask_patch') continue;
      const geo = annotation.geometry as MaskGeometry;
      // Check if this mask is at the same tile origin
      if (geo.x0_level0 === tileX && geo.y0_level0 === tileY) {
        if (geo.data_base64) {
          const data = decodeMaskData(geo.data_base64);
          if (data) {
            return { id: annotation.id, data };
          }
        }
      }
    }
    return null;
  }

  // Undo/Redo functions
  function pushUndoStep(step: UndoStep) {
    // Limit buffer size
    while (undoStack.length >= undoBufferSize) {
      undoStack.shift();
    }
    undoStack = [...undoStack, step];
    // Clear redo stack on new action
    redoStack = [];
  }

  function captureUndoState() {
    // Capture snapshot of all current tiles before stroke
    tilesBeforeStroke = new Map();
    for (const [key, tile] of maskTiles) {
      tilesBeforeStroke.set(key, {
        origin: { ...tile.origin },
        data: tile.data.slice(),
        annotationId: tile.annotationId,
      });
    }
  }

  function commitUndoStep() {
    if (tilesBeforeStroke.size === 0 && maskTiles.size === 0) {
      strokeWasErase = false;
      return;
    }
    
    // Check if any tile actually changed
    let hasChanges = false;
    
    // Check tiles that existed before
    for (const [key, before] of tilesBeforeStroke) {
      const after = maskTiles.get(key);
      if (!after) {
        hasChanges = true; // tile was removed
        break;
      }
      for (let i = 0; i < before.data.length; i++) {
        if (before.data[i] !== after.data[i]) {
          hasChanges = true;
          break;
        }
      }
      if (hasChanges) break;
    }
    
    // Check for new tiles
    if (!hasChanges) {
      for (const key of maskTiles.keys()) {
        if (!tilesBeforeStroke.has(key)) {
          hasChanges = true;
          break;
        }
      }
    }
    
    if (hasChanges) {
      pushUndoStep({
        type: 'mask-stroke',
        tiles: Array.from(tilesBeforeStroke.values()),
        description: strokeWasErase ? 'Erase stroke' : 'Brush stroke',
      });
    }
    
    tilesBeforeStroke = new Map();
    strokeWasErase = false;
  }

  function performUndo() {
    if (undoStack.length === 0) {
      showHudNotification('No Undo Available');
      return;
    }
    
    const step = undoStack[undoStack.length - 1];
    undoStack = undoStack.slice(0, -1);
    
    if (step.type === 'mask-stroke') {
      // Save current state to redo stack before restoring
      const currentTiles: UndoTileSnapshot[] = [];
      for (const tile of maskTiles.values()) {
        currentTiles.push({
          origin: { ...tile.origin },
          data: tile.data.slice(),
          annotationId: tile.annotationId,
        });
      }
      const currentStep: UndoStep = {
        type: 'mask-stroke',
        tiles: currentTiles,
        description: step.description,
      };
      redoStack = [...redoStack, currentStep];
      
      // Restore the mask state from step
      const newTiles = new Map<string, MaskTileState>();
      for (const tileSn of step.tiles) {
        const key = getTileKey(tileSn.origin.x, tileSn.origin.y);
        newTiles.set(key, {
          origin: { ...tileSn.origin },
          data: tileSn.data.slice(),
          annotationId: tileSn.annotationId,
          dirty: true,
        });
      }
      maskTiles = newTiles;
      scheduleMaskSync();
      
      showHudNotification(`Undo: ${step.description}`);
    }
  }

  function performRedo() {
    if (redoStack.length === 0) {
      showHudNotification('No Redo Available');
      return;
    }
    
    const step = redoStack[redoStack.length - 1];
    redoStack = redoStack.slice(0, -1);
    
    if (step.type === 'mask-stroke') {
      // Save current state to undo stack before restoring
      const currentTiles: UndoTileSnapshot[] = [];
      for (const tile of maskTiles.values()) {
        currentTiles.push({
          origin: { ...tile.origin },
          data: tile.data.slice(),
          annotationId: tile.annotationId,
        });
      }
      const currentStep: UndoStep = {
        type: 'mask-stroke',
        tiles: currentTiles,
        description: step.description,
      };
      undoStack = [...undoStack, currentStep];
      
      // Restore the mask state from step
      const newTiles = new Map<string, MaskTileState>();
      for (const tileSn of step.tiles) {
        const key = getTileKey(tileSn.origin.x, tileSn.origin.y);
        newTiles.set(key, {
          origin: { ...tileSn.origin },
          data: tileSn.data.slice(),
          annotationId: tileSn.annotationId,
          dirty: true,
        });
      }
      maskTiles = newTiles;
      scheduleMaskSync();
      
      showHudNotification(`Redo: ${step.description}`);
    }
  }

  // Helper: Get or create a mask tile at given origin
  function getOrCreateTile(tileX: number, tileY: number): MaskTileState {
    const key = getTileKey(tileX, tileY);
    let tile = maskTiles.get(key);
    if (!tile) {
      // Create new tile
      tile = {
        origin: { x: tileX, y: tileY },
        data: new Uint8Array(MASK_BYTES),
        annotationId: null,
        dirty: false,
      };
      
      // Check for existing mask annotation at this tile location
      const existing = findExistingMaskAtTile(tileX, tileY);
      if (existing) {
        tile.data.set(existing.data);
        tile.annotationId = existing.id;
      }
      
      // Capture snapshot for undo before modifying
      if (!tilesBeforeStroke.has(key)) {
        tilesBeforeStroke.set(key, {
          origin: { x: tileX, y: tileY },
          data: tile.data.slice(), // copy before modification
          annotationId: tile.annotationId,
        });
      }
      
      maskTiles.set(key, tile);
    }
    return tile;
  }

  // Paint a circle brush stroke at given image coordinates - supports multiple tiles
  function paintMaskBrush(imageX: number, imageY: number, erase: boolean) {
    const radius = maskBrushSize / 2;
    
    // Calculate bounding box of the brush in image coords
    const minImageX = Math.floor(imageX - radius);
    const maxImageX = Math.ceil(imageX + radius);
    const minImageY = Math.floor(imageY - radius);
    const maxImageY = Math.ceil(imageY + radius);
    
    // Determine which tiles are affected
    const minTileX = Math.floor(minImageX / MASK_TILE_SIZE) * MASK_TILE_SIZE;
    const maxTileX = Math.floor(maxImageX / MASK_TILE_SIZE) * MASK_TILE_SIZE;
    const minTileY = Math.floor(minImageY / MASK_TILE_SIZE) * MASK_TILE_SIZE;
    const maxTileY = Math.floor(maxImageY / MASK_TILE_SIZE) * MASK_TILE_SIZE;
    
    let anyPixelsPainted = false;
    
    // Iterate over all affected tiles
    for (let tileX = minTileX; tileX <= maxTileX; tileX += MASK_TILE_SIZE) {
      for (let tileY = minTileY; tileY <= maxTileY; tileY += MASK_TILE_SIZE) {
        const tile = getOrCreateTile(tileX, tileY);
        
        // Convert to tile-local coordinates
        const localCenterX = imageX - tileX;
        const localCenterY = imageY - tileY;
        
        // Clamp to this tile's bounds
        const localMinX = Math.max(0, Math.floor(localCenterX - radius));
        const localMaxX = Math.min(MASK_TILE_SIZE - 1, Math.ceil(localCenterX + radius));
        const localMinY = Math.max(0, Math.floor(localCenterY - radius));
        const localMaxY = Math.min(MASK_TILE_SIZE - 1, Math.ceil(localCenterY + radius));
        
        // Skip if brush doesn't overlap this tile
        if (localMinX > MASK_TILE_SIZE - 1 || localMaxX < 0 || localMinY > MASK_TILE_SIZE - 1 || localMaxY < 0) {
          continue;
        }
        
        let pixelsPainted = 0;
        for (let py = localMinY; py <= localMaxY; py++) {
          for (let px = localMinX; px <= localMaxX; px++) {
            const dx = px - localCenterX;
            const dy = py - localCenterY;
            if (dx * dx + dy * dy <= radius * radius) {
              setMaskPixel(tile.data, px, py, !erase);
              pixelsPainted++;
            }
          }
        }
        
        if (pixelsPainted > 0) {
          tile.dirty = true;
          anyPixelsPainted = true;
          // Update the tile in the map with a new object to trigger reactivity
          const key = getTileKey(tileX, tileY);
          // Use slice() for a safer copy that doesn't depend on length property
          const dataCopy = tile.data.slice();
          maskTiles.set(key, { ...tile, data: dataCopy });
        }
      }
    }
    
    // Force reactivity by creating a new Map reference
    if (anyPixelsPainted) {
      maskTiles = new Map(maskTiles);
    }
  }

  // Debounced sync to backend
  function scheduleMaskSync() {
    if (maskSyncTimeout) {
      clearTimeout(maskSyncTimeout);
    }
    maskSyncTimeout = setTimeout(() => {
      syncMaskToBackend();
    }, 350);
  }

  // Sync all dirty mask tiles to backend
  async function syncMaskToBackend() {
    const dirtyTiles = Array.from(maskTiles.values()).filter(t => t.dirty);
    if (dirtyTiles.length === 0) return;
    
    for (const tile of dirtyTiles) {
      // Encode mask to base64 - build binary string byte by byte
      let binaryString = '';
      for (let i = 0; i < tile.data.length; i++) {
        binaryString += String.fromCharCode(tile.data[i]);
      }
      const base64 = btoa(binaryString);
      
      const geometry: MaskGeometry = {
        x0_level0: tile.origin.x,
        y0_level0: tile.origin.y,
        width: MASK_TILE_SIZE,
        height: MASK_TILE_SIZE,
        data_base64: base64,
      };
      
      try {
        if (tile.annotationId) {
          // Update existing annotation
          await annotationStore.updateAnnotation(tile.annotationId, { geometry });
        } else {
          // Create new annotation
          const result = await annotationStore.createAnnotation({
            kind: 'mask_patch',
            geometry,
            label_id: 'unlabeled',
          });
          tile.annotationId = result.id;
        }
        tile.dirty = false;
      } catch (err) {
        console.error('Failed to sync mask tile:', err, tile.origin);
      }
    }
  }

  function cancelMaskPainting() {
    // Cancel any pending sync
    if (maskSyncTimeout) {
      clearTimeout(maskSyncTimeout);
      maskSyncTimeout = null;
    }
    
    // Reset mask state
    maskTiles = new Map();
    isMaskPainting = false;
  }

  async function confirmMaskPainting() {
    // Cancel any pending debounced sync
    if (maskSyncTimeout) {
      clearTimeout(maskSyncTimeout);
      maskSyncTimeout = null;
    }
    
    // Immediately sync any unsaved changes
    const hasDirty = Array.from(maskTiles.values()).some(t => t.dirty);
    if (hasDirty) {
      await syncMaskToBackend();
    }
    
    // Reset mask state
    maskTiles = new Map();
    isMaskPainting = false;
    
    // Clear undo/redo stacks
    undoStack = [];
    redoStack = [];
  }

  function cancelModifyMode() {
    modifyMode = { phase: 'idle', annotation: null, isCreating: false };
    modifyMouseImagePos = null;
  }

  async function handleModifyClick(e: MouseEvent) {
    // For creation mode, annotation is null but we still proceed
    if (!modifyMode.annotation && !modifyMode.isCreating) return;
    
    const imagePos = screenToImage(e.clientX, e.clientY);
    const annotation = modifyMode.annotation;

    if (modifyMode.phase === 'point-position') {
      // Create or update point position
      try {
        if (modifyMode.isCreating) {
          // Creating new point
          await annotationStore.createAnnotation({
            kind: 'point',
            geometry: {
              x_level0: imagePos.x,
              y_level0: imagePos.y,
            },
            label_id: 'unlabeled',
          });
          showHudNotification('Point created');
        } else {
          // Updating existing point
          if (!annotation) return;
          await annotationStore.updateAnnotation(annotation.id, {
            geometry: {
              x_level0: imagePos.x,
              y_level0: imagePos.y,
            },
          });
          showHudNotification('Point updated');
        }
      } catch (err) {
        console.error('Failed to save point:', err);
        showHudNotification('Failed to save point');
      }
      cancelModifyMode();
    } else if (modifyMode.phase === 'multi-point') {
      // Create point and stay in multi-point mode
      try {
        await annotationStore.createAnnotation({
          kind: 'point',
          geometry: {
            x_level0: imagePos.x,
            y_level0: imagePos.y,
          },
          label_id: 'unlabeled',
        });
        const count = (modifyMode.pointsCreated || 0) + 1;
        modifyMode = { ...modifyMode, pointsCreated: count };
      } catch (err) {
        console.error('Failed to save point:', err);
        showHudNotification('Failed to save point');
      }
      // Stay in multi-point mode, don't call cancelModifyMode()
    } else if (modifyMode.phase === 'ellipse-center') {
      // Store center and move to next phase
      // If we have a centerOffset (user went back from radii/angle), apply it
      const offset = modifyMode.tempCenterOffset ?? { x: 0, y: 0 };
      const newCenter = { x: imagePos.x - offset.x, y: imagePos.y - offset.y };
      // If we already have radii (user went back to adjust), go to radii phase preserving existing values
      // If we already have angle too, go straight to angle phase
      if (modifyMode.tempRadii && modifyMode.tempAngleOffset !== undefined) {
        modifyMode = {
          ...modifyMode,
          phase: 'ellipse-angle',
          tempCenter: newCenter,
          tempCenterOffset: undefined, // Clear offset after applying
          // In modification mode, set dragStartPos to current click so rotation doesn't jump
          dragStartPos: !modifyMode.isCreating ? imagePos : undefined,
        };
        showHudNotification('Adjust rotation, then click to confirm');
      } else if (modifyMode.tempRadii) {
        modifyMode = {
          ...modifyMode,
          phase: 'ellipse-radii',
          tempCenter: newCenter,
          tempCenterOffset: undefined, // Clear offset after applying
          // In modification mode, set dragStartPos to current click so size doesn't jump
          dragStartPos: !modifyMode.isCreating ? imagePos : undefined,
        };
        showHudNotification('Adjust size, then click (W=size, E=rotation)');
      } else {
        // For creation mode (no original values), start fresh radii phase
        // For modification mode, preserve original values and set dragStartPos
        modifyMode = {
          ...modifyMode,
          phase: 'ellipse-radii',
          tempCenter: newCenter,
          tempCenterOffset: undefined,
          // In modification mode, set dragStartPos to current click so size doesn't jump
          dragStartPos: !modifyMode.isCreating ? imagePos : undefined,
        };
        showHudNotification('Move mouse to set width & height, then click');
      }
    } else if (modifyMode.phase === 'ellipse-radii') {
      // Store radii based on mouse offset from center, accounting for existing rotation
      const center = modifyMode.tempCenter!;
      const dx = imagePos.x - center.x;
      const dy = imagePos.y - center.y;
      // Transform mouse offset into ellipse's local coordinate system
      const currentRotation = modifyMode.tempRotation ?? 0;
      const cosR = Math.cos(-currentRotation);
      const sinR = Math.sin(-currentRotation);
      const localX = dx * cosR - dy * sinR;
      const localY = dx * sinR + dy * cosR;
      
      let rx: number, ry: number;
      // Use tempRadii as base (current working value), fallback to originalRadii
      const baseRadii = modifyMode.tempRadii ?? modifyMode.originalRadii;
      if (!modifyMode.isCreating && baseRadii && modifyMode.dragStartPos) {
        // Modification mode: compute delta from drag start, apply to base radii
        const dragDx = modifyMode.dragStartPos.x - center.x;
        const dragDy = modifyMode.dragStartPos.y - center.y;
        const dragLocalX = dragDx * cosR - dragDy * sinR;
        const dragLocalY = dragDx * sinR + dragDy * cosR;
        rx = Math.max(baseRadii.rx + (Math.abs(localX) - Math.abs(dragLocalX)), 1);
        ry = Math.max(baseRadii.ry + (Math.abs(localY) - Math.abs(dragLocalY)), 1);
      } else {
        // Creation mode: use absolute mouse distance as radii
        rx = Math.max(Math.abs(localX), 1);
        ry = Math.max(Math.abs(localY), 1);
      }
      
      // If we already have an angle offset (user went back to adjust size), preserve it
      // Otherwise, create a new one based on current mouse position
      const initialAngle = modifyMode.tempAngleOffset ?? Math.atan2(dy, dx);
      modifyMode = {
        ...modifyMode,
        phase: 'ellipse-angle',
        tempRadii: { rx, ry },
        tempAngleOffset: initialAngle,
        // In modification mode, set dragStartPos to current click so rotation doesn't jump
        dragStartPos: !modifyMode.isCreating ? imagePos : undefined,
      };
      showHudNotification('Move mouse to set rotation, then click');
    } else if (modifyMode.phase === 'ellipse-angle') {
      // Calculate final angle and create/update
      const center = modifyMode.tempCenter!;
      const dx = imagePos.x - center.x;
      const dy = imagePos.y - center.y;
      const rawAngle = Math.atan2(dy, dx);
      
      let angle: number;
      if (!modifyMode.isCreating && modifyMode.dragStartPos && modifyMode.tempRotation !== undefined) {
        // Modification mode: compute angle delta from drag start, add to current rotation
        const dragDx = modifyMode.dragStartPos.x - center.x;
        const dragDy = modifyMode.dragStartPos.y - center.y;
        const dragAngle = Math.atan2(dragDy, dragDx);
        angle = modifyMode.tempRotation + (rawAngle - dragAngle);
      } else {
        // Creation mode: use angle offset from when entering this phase
        angle = rawAngle - (modifyMode.tempAngleOffset ?? 0);
      }
      
      const geometry = {
        cx_level0: center.x,
        cy_level0: center.y,
        radius_x: modifyMode.tempRadii!.rx,
        radius_y: modifyMode.tempRadii!.ry,
        rotation_radians: angle,
      };
      
      try {
        if (modifyMode.isCreating) {
          // Creating new ellipse
          await annotationStore.createAnnotation({
            kind: 'ellipse',
            geometry,
            label_id: 'unlabeled',
          });
          showHudNotification('Ellipse created');
        } else {
          // Updating existing ellipse
          await annotationStore.updateAnnotation(annotation!.id, { geometry });
          showHudNotification('Ellipse updated');
        }
      } catch (err) {
        console.error('Failed to save ellipse:', err);
        showHudNotification('Failed to save ellipse');
      }
      cancelModifyMode();
    }
  }

  // Get current modify preview position (for rendering in overlay)
  function getModifyPreview(): { x: number; y: number; rx?: number; ry?: number; rotation?: number } | null {
    if (modifyMode.phase === 'idle') return null;
    return null; // Preview is handled in overlay via props
  }

  function cancelLongPress() {
    if (longPressTimer) {
      clearTimeout(longPressTimer);
      longPressTimer = null;
    }
  }

  async function handleSaveImage() {
    const canvas = container?.querySelector('canvas') as HTMLCanvasElement | null;
    if (!canvas) return;

    try {
      const blob = await new Promise<Blob | null>((resolve) => {
        canvas.toBlob(resolve, 'image/png');
      });
      if (!blob) return;

      const url = URL.createObjectURL(blob);
      const link = document.createElement('a');
      link.href = url;
      link.download = `${activeSlideId || 'viewport'}.png`;
      document.body.appendChild(link);
      link.click();
      document.body.removeChild(link);
      URL.revokeObjectURL(url);
    } catch (err) {
      console.error('Failed to save image:', err);
    }
  }

  async function handleCopyImage() {
    const canvas = container?.querySelector('canvas') as HTMLCanvasElement | null;
    if (!canvas) return;

    try {
      const blob = await new Promise<Blob | null>((resolve) => {
        canvas.toBlob(resolve, 'image/png');
      });
      if (!blob) return;

      await navigator.clipboard.write([
        new ClipboardItem({ 'image/png': blob })
      ]);
      
      showHudNotification('Image copied to clipboard');
    } catch (err) {
      console.error('Failed to copy image:', err);
      showHudNotification('Failed to copy image');
    }
  }

  // Update viewport size on resize
  function updateViewportSize() {
    if (!container) return;
    const rect = container.getBoundingClientRect();
    viewport = { ...viewport, width: rect.width, height: rect.height };

    if (imageDesc) {
      viewport = clampViewport(viewport, imageDesc.width, imageDesc.height);
    }

    scheduleViewportUpdate();
  }

  let resizeObserver: ResizeObserver | null = null;

  onMount(() => {
    if (container) {
      const rect = container.getBoundingClientRect();
      viewport = { ...viewport, width: rect.width, height: rect.height };

      // Use ResizeObserver to handle pane resizing (from divider drag)
      resizeObserver = new ResizeObserver(() => {
        updateViewportSize();
      });
      resizeObserver.observe(container);
    }

    // Register tile handler
    registerHandler();

    window.addEventListener('mouseup', handleWindowMouseUp);
    window.addEventListener('keydown', handleKeyDown, true);
    window.addEventListener('keyup', handleKeyUp, true);
    window.addEventListener('mousemove', handleWindowMouseMove);
  });

  onDestroy(() => {
    unsubSplit();
    unsubNavigation();
    unsubAuth();
    unsubActiveSet();
    unsubAnnotationsStore();
    unsubToolCommand();
    onUnregisterTileHandler(paneId);
    closeCurrentSlide();
    if (activeSlideId) {
      releaseCache(activeSlideId);
    }
    resizeObserver?.disconnect();
    if (viewportUpdateTimeout) {
      clearTimeout(viewportUpdateTimeout);
    }
    if (hudNotificationTimeout) {
      clearTimeout(hudNotificationTimeout);
    }
    cancelLongPress();
    if (browser) {
      window.removeEventListener('mouseup', handleWindowMouseUp);
      window.removeEventListener('keydown', handleKeyDown, true);
      window.removeEventListener('keyup', handleKeyUp, true);
      window.removeEventListener('mousemove', handleWindowMouseMove);
    }
  });
</script>

<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<!-- svelte-ignore a11y_no_noninteractive_tabindex -->
<div
  class="viewer-container"
  class:measuring={measurement.active}
  class:measuring-toggle={measurement.active && (measurement.mode === 'toggle' || measurement.mode === 'pending')}
  class:modifying={modifyMode.phase !== 'idle'}
  bind:this={container}
  onmousedown={handleMouseDown}
  onmousemove={handleMouseMove}
  onwheel={handleWheel}
  oncontextmenu={handleContextMenu}
  ontouchstart={handleTouchStart}
  ontouchmove={handleTouchMove}
  ontouchend={handleTouchEnd}
  role="application"
  tabindex="0"
  aria-label="Tile viewer - use mouse to pan, scroll to zoom"
>
  {#if imageDesc && cache}
    <!-- Image layer with brightness/contrast/gamma filters applied -->
    <div class="image-layer" style="filter: {imageFilter()}">
      <TileRenderer image={imageDesc} {viewport} {cache} {renderTrigger} {stainNormalization} {stainEnhancement} client={client ?? undefined} slot={currentSlot ?? undefined} onMetrics={handleRenderMetrics} />
    </div>
    
    <!-- Scale bar (bottom-left) - controlled by settings -->
    <ScaleBar {viewport} />
    
    <!-- Measurement overlay -->
    <MeasurementOverlay {viewport} {measurement} />
    
    <!-- Annotation overlay -->
    <AnnotationOverlay
      slideId={activeSlideId}
      viewportX={viewport.x}
      viewportY={viewport.y}
      viewportZoom={viewport.zoom}
      containerWidth={viewport.width}
      containerHeight={viewport.height}
      onAnnotationRightClick={handleAnnotationRightClick}
      modifyPhase={modifyMode.phase}
      modifyAnnotationId={modifyMode.annotation?.id ?? null}
      modifyCenter={modifyMode.tempCenter ?? null}
      modifyRadii={modifyMode.tempRadii ?? null}
      modifyMousePos={modifyMouseImagePos}
      modifyAngleOffset={modifyMode.tempAngleOffset ?? 0}
      modifyRotation={modifyMode.tempRotation ?? 0}
      modifyCenterOffset={modifyMode.tempCenterOffset ?? null}
      modifyIsCreating={modifyMode.isCreating}
      modifyOriginalRadii={modifyMode.originalRadii ?? null}
      modifyDragStartPos={modifyMode.dragStartPos ?? null}
      modifyPolygonVertices={modifyMode.polygonVertices ?? null}
      modifyFreehandPath={modifyMode.freehandPath ?? null}
      modifyEditingVertexIndex={modifyMode.editingVertexIndex ?? null}
      maskPaintData={maskPaintData}
      maskTileOrigin={maskTileOrigin}
      maskAllTiles={maskAllTiles}
      maskBrushSize={maskBrushSize}
    />
    
    <!-- Viewer HUD overlay (top-left) -->
    <ViewerHud
      zoom={viewport.zoom}
      onZoomChange={handleHudZoomChange}
      onFitView={handleHudFitView}
      isPanning={isDragging}
      isMaskPainting={modifyMode.phase === 'mask-paint' && isPaneFocused}
      maskBrushSize={maskBrushSize}
    />
    
    <!-- Keyboard shortcut notification (bottom center) - only show for focused pane -->
    {#if hudNotification && isPaneFocused}
      <div class="hud-notification" class:fading={hudNotificationFading}>
        {@html hudNotification}
      </div>
    {/if}
    
    <!-- Minimap (bottom-right) - controlled by settings -->
    {#if minimapVisible}
      <div class="bottom-right-controls">
        <!-- Vertical zoom slider -->
        <!-- svelte-ignore a11y_no_static_element_interactions -->
        <div 
          class="zoom-slider-container"
          onmousedown={stopSliderPropagation}
          ontouchstart={stopSliderPropagation}
          onwheel={stopSliderPropagation}
        >
          <span class="zoom-slider-label">+</span>
          <input
            type="range"
            min="0"
            max="100"
            step="0.5"
            value={zoomSliderValue.value}
            oninput={handleZoomSliderChange}
            class="zoom-slider"
            aria-label="Zoom level"
          />
          <span class="zoom-slider-label">−</span>
        </div>
        <Minimap
          image={imageDesc}
          {viewport}
          {cache}
          {renderTrigger}
          onViewportChange={handleMinimapViewportChange}
        />
      </div>
    {/if}
  {:else}
    <div class="welcome-screen">
      <div class="welcome-content">
        <img src="/logo_full.png" alt="Eosin Logo" class="welcome-logo" />
        <h2>Welcome to Eosin.</h2>
        <p class="welcome-subtitle">Multi-gigapixel histopathology at your fingertips.</p>
        <div class="getting-started">
          <h3>Getting Started</h3>
          <ul>
            <li><strong>Browse slides:</strong> Open the sidebar to browse available slides</li>
            <li><strong>Open a slide:</strong> Click on any slide in the sidebar to view it</li>
            <li><strong>Navigate:</strong> Drag to pan, scroll to zoom, or use the minimap</li>
            <li><strong>Keyboard shortcuts:</strong> Press <kbd>H</kbd> for help</li>
          </ul>
        </div>
      </div>
    </div>
  {/if}

  {#if imageDesc}
    <footer class="controls">
      <div class="stats">
        <span>Zoom: {(viewport.zoom * 100).toFixed(1)}%</span>
        <span>Image: {imageDesc.width}×{imageDesc.height} ({imageDesc.levels} levels)</span>
        {#if progressTotal > 0 && progressSteps < progressTotal}
          <span class="progress-indicator"><ActivityIndicator trigger={progressUpdateTrigger} />Processing: {((progressSteps / progressTotal) * 100).toPrecision(3)}%</span>
        {/if}
        {#if loadError}
          <span class="error">{loadError}</span>
        {/if}
      </div>
    </footer>
  {/if}

  <!-- Viewport context menu (right-click / longpress) -->
  <ViewportContextMenu
    x={contextMenuX}
    y={contextMenuY}
    visible={contextMenuVisible}
    imageX={contextMenuImageX}
    imageY={contextMenuImageY}
    onSaveImage={handleSaveImage}
    onCopyImage={handleCopyImage}
    onClose={handleContextMenuClose}
    onStartPointCreation={handleStartPointCreation}
    onStartEllipseCreation={handleStartEllipseCreation}
    onStartPolygonCreation={handleStartPolygonCreation}
    onStartFreehandLasso={handleStartFreehandLasso}
    onStartMaskPainting={handleStartMaskPainting}
  />

  <!-- Annotation context menu (right-click on annotation) -->
  <AnnotationContextMenu
    x={annotationMenuX}
    y={annotationMenuY}
    visible={annotationMenuVisible}
    annotation={annotationMenuTarget}
    onClose={handleAnnotationMenuClose}
    onModify={handleAnnotationModify}
  />
</div>

<style>
  .viewer-container {
    flex: 1;
    position: relative;
    overflow: hidden;
    cursor: grab;
    touch-action: none;
    background: white;
    display: flex;
    flex-direction: column;
  }

  .viewer-container:active {
    cursor: grabbing;
  }

  /* Measurement mode cursor */
  .viewer-container.measuring {
    cursor: crosshair;
  }

  .viewer-container.measuring:active {
    cursor: crosshair;
  }

  .viewer-container.measuring-toggle {
    cursor: crosshair;
  }

  /* Modify mode cursor */
  .viewer-container.modifying {
    cursor: crosshair;
  }

  .viewer-container.modifying:active {
    cursor: crosshair;
  }

  /* Image layer wrapper for applying CSS filters (brightness/contrast/gamma) */
  .image-layer {
    position: absolute;
    inset: 0;
    z-index: 0;
  }

  .welcome-screen {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    height: 100%;
    background: 
      linear-gradient(135deg, rgba(15, 15, 25, 0.92) 0%, rgba(20, 25, 40, 0.88) 50%, rgba(15, 20, 35, 0.92) 100%),
      url('/background.webp');
    background-size: cover, cover;
    background-position: center, center;
    background-repeat: no-repeat, no-repeat;
    padding: 2rem;
    box-sizing: border-box;
  }

  .welcome-content {
    display: flex;
    flex-direction: column;
    align-items: center;
    text-align: center;
    max-width: 500px;
  }

  .welcome-logo {
    max-width: 280px;
    width: 100%;
    height: auto;
    margin-bottom: 1.5rem;
    filter: drop-shadow(0 4px 12px rgba(0, 0, 0, 0.3));
  }

  .welcome-screen h2 {
    color: #e8e8e8;
    font-size: 1.75rem;
    font-weight: 600;
    margin: 0 0 0.5rem 0;
  }

  .welcome-subtitle {
    color: #94a3b8;
    font-size: 1rem;
    margin: 0 0 2rem 0;
  }

  .getting-started {
    background: rgba(255, 255, 255, 0.05);
    border: 1px solid rgba(255, 255, 255, 0.1);
    border-radius: 12px;
    padding: 1.25rem 1.5rem;
    width: 100%;
    text-align: left;
    margin-bottom: 1.5rem;
  }

  .getting-started h3 {
    color: #e2e8f0;
    font-size: 1rem;
    font-weight: 600;
    margin: 0 0 0.75rem 0;
  }

  .getting-started ul {
    list-style: none;
    padding: 0;
    margin: 0;
  }

  .getting-started li {
    color: #cbd5e1;
    font-size: 0.875rem;
    padding: 0.5rem 0;
    border-bottom: 1px solid rgba(255, 255, 255, 0.05);
  }

  .getting-started li:last-child {
    border-bottom: none;
    padding-bottom: 0;
  }

  .getting-started li strong {
    color: #60a5fa;
  }

  .getting-started kbd {
    display: inline-block;
    background: rgba(255, 255, 255, 0.1);
    border: 1px solid rgba(255, 255, 255, 0.2);
    border-radius: 4px;
    padding: 0.125rem 0.375rem;
    font-family: 'SF Mono', 'Monaco', 'Inconsolata', 'Fira Code', monospace;
    font-size: 0.75rem;
    color: #e2e8f0;
  }

  .controls {
    display: flex;
    flex-wrap: wrap;
    gap: 1rem;
    padding: 0.5rem 0.75rem;
    background: #1a1a1a;
    border-top: 1px solid #333;
    align-items: center;
    justify-content: flex-end;
    flex-shrink: 0;
  }

  .stats {
    display: flex;
    gap: 1rem;
    font-size: 0.8125rem;
    color: #aaa;
  }

  .progress-indicator {
    color: #f59e0b;
    font-weight: 500;
    display: inline-flex;
    align-items: center;
    gap: 0.375rem;
  }

  .error {
    color: #ef4444;
    margin: 0;
    font-size: 0.8125rem;
  }

  .bottom-right-controls {
    position: absolute;
    bottom: 1rem;
    right: 1rem;
    display: flex;
    align-items: center;
    gap: 0.75rem;
    z-index: 10;
  }

  .zoom-slider-container {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 0.25rem;
    padding: 0.5rem 0.25rem;
    background: rgba(20, 20, 20, 0.75);
    backdrop-filter: blur(12px);
    border-radius: 0.5rem;
    border: 1px solid rgba(255, 255, 255, 0.1);
  }

  .zoom-slider-label {
    font-size: 0.875rem;
    font-weight: 600;
    color: #9ca3af;
    user-select: none;
    line-height: 1;
  }

  .zoom-slider {
    writing-mode: vertical-lr;
    direction: rtl;
    width: 6px;
    height: 120px;
    appearance: none;
    background: #374151;
    border-radius: 3px;
    cursor: pointer;
    margin: 0.25rem 0;
  }

  .zoom-slider::-webkit-slider-thumb {
    appearance: none;
    width: 14px;
    height: 14px;
    background: #3b82f6;
    border-radius: 50%;
    cursor: pointer;
    transition: transform 0.1s;
  }

  .zoom-slider::-webkit-slider-thumb:hover {
    transform: scale(1.2);
  }

  .zoom-slider::-moz-range-thumb {
    width: 14px;
    height: 14px;
    background: #3b82f6;
    border: none;
    border-radius: 50%;
    cursor: pointer;
  }

  /* HUD notification for keyboard shortcuts */
  .hud-notification {
    position: absolute;
    bottom: 2rem;
    left: 50%;
    transform: translateX(-50%);
    background: rgba(0, 0, 0, 0.8);
    backdrop-filter: blur(8px);
    color: #fff;
    padding: 0.75rem 1.5rem;
    border-radius: 0.5rem;
    font-size: 1rem;
    font-weight: 500;
    pointer-events: none;
    z-index: 100;
    opacity: 1;
    transition: opacity 600ms ease-out;
  }

  .hud-notification.fading {
    opacity: 0;
  }

  .hud-notification :global(.dim) {
    opacity: 0.6;
  }

  /* Responsive styles for welcome screen - height constrained */
  @media (max-height: 600px) {
    .welcome-screen {
      padding: 1rem;
    }

    .welcome-logo {
      max-width: 160px;
      margin-bottom: 0.75rem;
    }

    .welcome-screen h2 {
      font-size: 1.25rem;
      margin-bottom: 0.25rem;
    }

    .welcome-subtitle {
      font-size: 0.875rem;
      margin-bottom: 1rem;
    }

    .getting-started {
      padding: 0.75rem 1rem;
      margin-bottom: 0.75rem;
    }

    .getting-started h3 {
      font-size: 0.875rem;
      margin-bottom: 0.5rem;
    }

    .getting-started li {
      font-size: 0.8125rem;
      padding: 0.375rem 0;
    }
  }

  /* Responsive styles for welcome screen - very height constrained */
  @media (max-height: 480px) {
    .welcome-screen {
      padding: 0.5rem;
      justify-content: flex-start;
      overflow: hidden;
    }

    .welcome-content {
      max-width: 100%;
    }

    .welcome-logo {
      max-width: 100px;
      margin-bottom: 0.5rem;
    }

    .welcome-screen h2 {
      font-size: 1rem;
    }

    .welcome-subtitle {
      font-size: 0.75rem;
      margin-bottom: 0.5rem;
    }

    .getting-started {
      padding: 0.5rem 0.75rem;
      margin-bottom: 0.5rem;
    }

    .getting-started h3 {
      font-size: 0.8125rem;
      margin-bottom: 0.375rem;
    }

    .getting-started li {
      font-size: 0.75rem;
      padding: 0.25rem 0;
    }
  }

  /* Responsive styles for welcome screen - width constrained */
  @media (max-width: 480px) {
    .welcome-screen {
      padding: 1rem;
    }

    .welcome-content {
      max-width: 100%;
      width: 100%;
    }

    .welcome-logo {
      max-width: 200px;
      margin-bottom: 1rem;
    }

    .welcome-screen h2 {
      font-size: 1.375rem;
    }

    .welcome-subtitle {
      font-size: 0.875rem;
      margin-bottom: 1rem;
    }

    .getting-started {
      padding: 1rem;
    }

    .getting-started li {
      font-size: 0.8125rem;
    }
  }

  /* Responsive styles for welcome screen - small mobile (both constrained) */
  @media (max-width: 380px), (max-height: 400px) {
    .welcome-screen {
      padding: 0.5rem;
    }

    .welcome-logo {
      max-width: 80px;
      margin-bottom: 0.5rem;
    }

    .welcome-screen h2 {
      font-size: 1rem;
    }

    .welcome-subtitle {
      font-size: 0.75rem;
      margin-bottom: 0.5rem;
    }

    .getting-started {
      padding: 0.5rem 0.75rem;
      margin-bottom: 0;
    }

    .getting-started h3 {
      font-size: 0.75rem;
      margin-bottom: 0.25rem;
    }

    .getting-started li {
      font-size: 0.6875rem;
      padding: 0.25rem 0;
    }

    .getting-started kbd {
      font-size: 0.625rem;
      padding: 0.0625rem 0.25rem;
    }
  }
</style>
