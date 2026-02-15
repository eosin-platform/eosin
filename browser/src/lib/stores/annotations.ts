/**
 * Annotation store for the WSI viewer application.
 * 
 * Manages annotation sets and annotations per slide, including:
 * - Annotation set (layer) list per slide
 * - Active annotation set selection
 * - Annotations for each set
 * - Layer visibility states
 * - Highlighted annotation for hover effects
 * - Selected annotation for editing
 */

import { writable, derived, get } from 'svelte/store';
import { browser } from '$app/environment';
import {
  fetchAnnotationSets,
  fetchAnnotations,
  createAnnotationSet as apiCreateAnnotationSet,
  updateAnnotationSet as apiUpdateAnnotationSet,
  deleteAnnotationSet as apiDeleteAnnotationSet,
  createAnnotation as apiCreateAnnotation,
  updateAnnotation as apiUpdateAnnotation,
  deleteAnnotation as apiDeleteAnnotation,
  type AnnotationSet,
  type Annotation,
  type CreateAnnotationSetRequest,
  type UpdateAnnotationSetRequest,
  type CreateAnnotationRequest,
  type UpdateAnnotationRequest,
} from '$lib/api/annotations';

// ============================================================================
// Types
// ============================================================================

export interface AnnotationStoreState {
  /** Current slide ID being viewed (for sidebar panel) */
  currentSlideId: string | null;
  /** Annotation sets keyed by slide ID */
  annotationSetsBySlide: Map<string, AnnotationSet[]>;
  /** Active annotation set ID per slide */
  activeAnnotationSetBySlide: Map<string, string | null>;
  /** Map of slide ID -> (annotation set ID -> annotations) */
  annotationsBySlide: Map<string, Map<string, Annotation[]>>;
  /** Map of annotation set ID -> visibility (global across slides) */
  layerVisibility: Map<string, boolean>;
  /** Currently highlighted annotation ID (for hover effects) */
  highlightedAnnotationId: string | null;
  /** Currently selected annotation ID (for editing) */
  selectedAnnotationId: string | null;
  /** Loading state per slide */
  loadingSlides: Set<string>;
  /** Error message */
  error: string | null;
}

export interface SidebarLayoutState {
  /** Whether the slides section is collapsed */
  slidesSectionCollapsed: boolean;
  /** Whether the annotations section is collapsed */
  annotationsSectionCollapsed: boolean;
  /** Height of the annotations section in pixels (when both sections expanded) */
  annotationsSectionHeight: number;
}

// ============================================================================
// Constants
// ============================================================================

const STORAGE_KEY = 'eosin-annotation-state';
const SIDEBAR_STORAGE_KEY = 'eosin-sidebar-layout';
const DEFAULT_ANNOTATIONS_HEIGHT = 300;
const MIN_SECTION_HEIGHT = 100;

// ============================================================================
// Initial State
// ============================================================================

const initialState: AnnotationStoreState = {
  currentSlideId: null,
  annotationSetsBySlide: new Map(),
  activeAnnotationSetBySlide: new Map(),
  annotationsBySlide: new Map(),
  layerVisibility: new Map(),
  highlightedAnnotationId: null,
  selectedAnnotationId: null,
  loadingSlides: new Set(),
  error: null,
};

const initialSidebarLayout: SidebarLayoutState = {
  slidesSectionCollapsed: false,
  annotationsSectionCollapsed: false,
  annotationsSectionHeight: DEFAULT_ANNOTATIONS_HEIGHT,
};

// ============================================================================
// Persistence
// ============================================================================

function loadLayerVisibility(): Map<string, boolean> {
  if (!browser) return new Map();
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) return new Map();
    const parsed = JSON.parse(raw);
    if (parsed.layerVisibility) {
      return new Map(Object.entries(parsed.layerVisibility));
    }
    return new Map();
  } catch {
    return new Map();
  }
}

function saveLayerVisibility(visibility: Map<string, boolean>): void {
  if (!browser) return;
  try {
    const existing = localStorage.getItem(STORAGE_KEY);
    const parsed = existing ? JSON.parse(existing) : {};
    parsed.layerVisibility = Object.fromEntries(visibility);
    localStorage.setItem(STORAGE_KEY, JSON.stringify(parsed));
  } catch {
    // Ignore storage errors
  }
}

function loadSidebarLayout(): SidebarLayoutState {
  if (!browser) return { ...initialSidebarLayout };
  try {
    const raw = localStorage.getItem(SIDEBAR_STORAGE_KEY);
    if (!raw) return { ...initialSidebarLayout };
    const parsed = JSON.parse(raw);
    return {
      slidesSectionCollapsed: parsed.slidesSectionCollapsed ?? false,
      annotationsSectionCollapsed: parsed.annotationsSectionCollapsed ?? false,
      annotationsSectionHeight: Math.max(
        MIN_SECTION_HEIGHT,
        parsed.annotationsSectionHeight ?? DEFAULT_ANNOTATIONS_HEIGHT
      ),
    };
  } catch {
    return { ...initialSidebarLayout };
  }
}

function saveSidebarLayout(state: SidebarLayoutState): void {
  if (!browser) return;
  try {
    localStorage.setItem(SIDEBAR_STORAGE_KEY, JSON.stringify(state));
  } catch {
    // Ignore storage errors
  }
}

// ============================================================================
// Store Creation
// ============================================================================

function createAnnotationStore() {
  const { subscribe, set, update } = writable<AnnotationStoreState>({
    ...initialState,
    layerVisibility: loadLayerVisibility(),
  });

  let persistTimeout: ReturnType<typeof setTimeout> | null = null;

  // Debounced persistence for layer visibility
  function schedulePersist() {
    if (persistTimeout) clearTimeout(persistTimeout);
    persistTimeout = setTimeout(() => {
      const state = get({ subscribe });
      saveLayerVisibility(state.layerVisibility);
      persistTimeout = null;
    }, 300);
  }

  return {
    subscribe,

    /**
     * Load annotation sets for a slide.
     * Does not clear data for other slides - supports multi-slide view.
     */
    async loadForSlide(slideId: string): Promise<void> {
      // Check if already loaded
      const state = get({ subscribe });
      if (state.annotationSetsBySlide.has(slideId)) {
        // Just update currentSlideId for the sidebar panel
        update((s) => ({ ...s, currentSlideId: slideId }));
        return;
      }

      update((s) => {
        const loadingSlides = new Set(s.loadingSlides);
        loadingSlides.add(slideId);
        return {
          ...s,
          currentSlideId: slideId,
          loadingSlides,
          error: null,
        };
      });

      try {
        const sets = await fetchAnnotationSets(slideId);
        
        // Initialize visibility for new sets
        const visibility = new Map(get({ subscribe }).layerVisibility);
        for (const set of sets) {
          if (!visibility.has(set.id)) {
            visibility.set(set.id, true);
          }
        }

        // Set first set as active for this slide if none selected
        const currentState = get({ subscribe });
        const currentActive = currentState.activeAnnotationSetBySlide.get(slideId);
        const activeId = sets.find((set: AnnotationSet) => set.id === currentActive)?.id ?? sets[0]?.id ?? null;

        update((s) => {
          const annotationSetsBySlide = new Map(s.annotationSetsBySlide);
          annotationSetsBySlide.set(slideId, sets);
          
          const activeAnnotationSetBySlide = new Map(s.activeAnnotationSetBySlide);
          activeAnnotationSetBySlide.set(slideId, activeId);
          
          // Initialize empty annotations map for this slide
          const annotationsBySlide = new Map(s.annotationsBySlide);
          if (!annotationsBySlide.has(slideId)) {
            annotationsBySlide.set(slideId, new Map());
          }
          
          const loadingSlides = new Set(s.loadingSlides);
          loadingSlides.delete(slideId);
          
          return {
            ...s,
            annotationSetsBySlide,
            activeAnnotationSetBySlide,
            annotationsBySlide,
            layerVisibility: visibility,
            loadingSlides,
          };
        });

        schedulePersist();

        // Load annotations for all sets
        for (const set of sets) {
          await annotationStore.loadAnnotationsForSet(slideId, set.id);
        }
      } catch (err) {
        update((s) => {
          const loadingSlides = new Set(s.loadingSlides);
          loadingSlides.delete(slideId);
          return {
            ...s,
            loadingSlides,
            error: err instanceof Error ? err.message : 'Failed to load annotation sets',
          };
        });
      }
    },

    /**
     * Load annotations for an annotation set.
     */
    async loadAnnotationsForSet(slideId: string, annotationSetId: string): Promise<void> {
      try {
        const annotations = await fetchAnnotations(annotationSetId);
        update((s) => {
          const annotationsBySlide = new Map(s.annotationsBySlide);
          const slideAnnotations = new Map(annotationsBySlide.get(slideId) ?? new Map());
          slideAnnotations.set(annotationSetId, annotations);
          annotationsBySlide.set(slideId, slideAnnotations);
          return { ...s, annotationsBySlide };
        });
      } catch (err) {
        console.error('Failed to load annotations:', err);
      }
    },

    /**
     * Set the active annotation set for the current slide.
     */
    async setActiveSet(annotationSetId: string | null): Promise<void> {
      const currentSlideId = get({ subscribe }).currentSlideId;
      if (!currentSlideId) return;
      
      update((s) => {
        const activeAnnotationSetBySlide = new Map(s.activeAnnotationSetBySlide);
        activeAnnotationSetBySlide.set(currentSlideId, annotationSetId);
        return { ...s, activeAnnotationSetBySlide };
      });
      
      const state = get({ subscribe });
      const slideAnnotations = state.annotationsBySlide.get(currentSlideId);
      if (annotationSetId && (!slideAnnotations || !slideAnnotations.has(annotationSetId))) {
        await annotationStore.loadAnnotationsForSet(currentSlideId, annotationSetId);
      }
    },

    /**
     * Toggle layer visibility.
     */
    toggleLayerVisibility(annotationSetId: string): void {
      update((s) => {
        const visibility = new Map(s.layerVisibility);
        visibility.set(annotationSetId, !visibility.get(annotationSetId));
        return { ...s, layerVisibility: visibility };
      });
      schedulePersist();
    },

    /**
     * Set layer visibility.
     */
    setLayerVisibility(annotationSetId: string, visible: boolean): void {
      update((s) => {
        const visibility = new Map(s.layerVisibility);
        visibility.set(annotationSetId, visible);
        return { ...s, layerVisibility: visibility };
      });
      schedulePersist();
    },

    /**
     * Set the highlighted annotation (for hover).
     */
    setHighlightedAnnotation(annotationId: string | null): void {
      update((s) => ({ ...s, highlightedAnnotationId: annotationId }));
    },

    /**
     * Set the selected annotation (for editing).
     */
    setSelectedAnnotation(annotationId: string | null): void {
      update((s) => ({ ...s, selectedAnnotationId: annotationId }));
    },

    /**
     * Create a new annotation set.
     */
    async createSet(data: CreateAnnotationSetRequest): Promise<AnnotationSet> {
      const state = get({ subscribe });
      if (!state.currentSlideId) {
        throw new Error('No slide selected');
      }

      const slideId = state.currentSlideId;
      const created = await apiCreateAnnotationSet(slideId, data);
      
      update((s) => {
        const annotationSetsBySlide = new Map(s.annotationSetsBySlide);
        const existingSets = annotationSetsBySlide.get(slideId) ?? [];
        annotationSetsBySlide.set(slideId, [...existingSets, created]);
        
        const activeAnnotationSetBySlide = new Map(s.activeAnnotationSetBySlide);
        activeAnnotationSetBySlide.set(slideId, created.id);
        
        const visibility = new Map(s.layerVisibility);
        visibility.set(created.id, true);
        
        // Initialize empty annotations for the new set
        const annotationsBySlide = new Map(s.annotationsBySlide);
        const slideAnnotations = new Map(annotationsBySlide.get(slideId) ?? new Map());
        slideAnnotations.set(created.id, []);
        annotationsBySlide.set(slideId, slideAnnotations);
        
        return {
          ...s,
          annotationSetsBySlide,
          activeAnnotationSetBySlide,
          annotationsBySlide,
          layerVisibility: visibility,
        };
      });

      schedulePersist();
      return created;
    },

    /**
     * Update an annotation set.
     */
    async updateSet(id: string, data: UpdateAnnotationSetRequest): Promise<AnnotationSet> {
      const updated = await apiUpdateAnnotationSet(id, data);
      
      update((s) => {
        const annotationSetsBySlide = new Map(s.annotationSetsBySlide);
        // Update in all slides that might have this set
        for (const [slideId, sets] of annotationSetsBySlide) {
          const idx = sets.findIndex((set) => set.id === id);
          if (idx >= 0) {
            const newSets = [...sets];
            newSets[idx] = updated;
            annotationSetsBySlide.set(slideId, newSets);
          }
        }
        return { ...s, annotationSetsBySlide };
      });

      return updated;
    },

    /**
     * Delete an annotation set.
     */
    async deleteSet(id: string): Promise<void> {
      await apiDeleteAnnotationSet(id);
      
      update((s) => {
        const annotationSetsBySlide = new Map(s.annotationSetsBySlide);
        const activeAnnotationSetBySlide = new Map(s.activeAnnotationSetBySlide);
        const annotationsBySlide = new Map(s.annotationsBySlide);
        const visibility = new Map(s.layerVisibility);
        visibility.delete(id);
        
        // Remove from all slides
        for (const [slideId, sets] of annotationSetsBySlide) {
          const newSets = sets.filter((set) => set.id !== id);
          annotationSetsBySlide.set(slideId, newSets);
          
          // Update active set if needed
          if (activeAnnotationSetBySlide.get(slideId) === id) {
            activeAnnotationSetBySlide.set(slideId, newSets[0]?.id ?? null);
          }
          
          // Remove annotations for this set
          const slideAnnotations = annotationsBySlide.get(slideId);
          if (slideAnnotations) {
            const newSlideAnnotations = new Map(slideAnnotations);
            newSlideAnnotations.delete(id);
            annotationsBySlide.set(slideId, newSlideAnnotations);
          }
        }

        return {
          ...s,
          annotationSetsBySlide,
          activeAnnotationSetBySlide,
          annotationsBySlide,
          layerVisibility: visibility,
        };
      });

      schedulePersist();
    },

    /**
     * Create a new annotation.
     */
    async createAnnotation(data: CreateAnnotationRequest): Promise<Annotation> {
      const state = get({ subscribe });
      const slideId = state.currentSlideId;
      const activeSetId = slideId ? state.activeAnnotationSetBySlide.get(slideId) : null;
      
      if (!slideId || !activeSetId) {
        throw new Error('No annotation set selected');
      }

      const isOptimisticPoint = data.kind === 'point';
      const optimisticId = `temp-point-${Date.now()}-${Math.random().toString(36).slice(2)}`;
      const now = new Date().toISOString();

      if (isOptimisticPoint) {
        const optimisticAnnotation: Annotation = {
          id: optimisticId,
          annotation_set_id: activeSetId,
          kind: 'point',
          geometry: data.geometry,
          label: data.label,
          label_id: data.label_id,
          description: data.description,
          properties: data.properties,
          created_at: now,
          updated_at: now,
        };

        update((s) => {
          const annotationsBySlide = new Map(s.annotationsBySlide);
          const slideAnnotations = new Map(annotationsBySlide.get(slideId) ?? new Map());
          const existing = slideAnnotations.get(activeSetId) ?? [];
          slideAnnotations.set(activeSetId, [...existing, optimisticAnnotation]);
          annotationsBySlide.set(slideId, slideAnnotations);
          return { ...s, annotationsBySlide };
        });
      }

      try {
        const created = await apiCreateAnnotation(activeSetId, data);

        update((s) => {
          const annotationsBySlide = new Map(s.annotationsBySlide);
          const slideAnnotations = new Map(annotationsBySlide.get(slideId) ?? new Map());
          const existing = slideAnnotations.get(activeSetId) ?? [];

          if (isOptimisticPoint) {
            const replaced = existing.map((annotation: Annotation) =>
              annotation.id === optimisticId ? created : annotation
            );
            slideAnnotations.set(activeSetId, replaced);
          } else {
            slideAnnotations.set(activeSetId, [...existing, created]);
          }

          annotationsBySlide.set(slideId, slideAnnotations);
          return { ...s, annotationsBySlide };
        });

        return created;
      } catch (error) {
        if (isOptimisticPoint) {
          update((s) => {
            const annotationsBySlide = new Map(s.annotationsBySlide);
            const slideAnnotations = new Map(annotationsBySlide.get(slideId) ?? new Map());
            const existing = slideAnnotations.get(activeSetId) ?? [];
            slideAnnotations.set(
              activeSetId,
              existing.filter((annotation: Annotation) => annotation.id !== optimisticId)
            );
            annotationsBySlide.set(slideId, slideAnnotations);
            return { ...s, annotationsBySlide };
          });
        }

        throw error;
      }
    },

    /**
     * Update an annotation.
     */
    async updateAnnotation(id: string, data: UpdateAnnotationRequest): Promise<Annotation> {
      const updated = await apiUpdateAnnotation(id, data);
      
      update((s) => {
        const annotationsBySlide = new Map(s.annotationsBySlide);
        // Search all slides for the annotation
        outer: for (const [slideId, slideAnnotations] of annotationsBySlide) {
          const newSlideAnnotations = new Map(slideAnnotations);
          for (const [setId, annotations] of newSlideAnnotations) {
            const idx = annotations.findIndex((a) => a.id === id);
            if (idx >= 0) {
              const newAnnotations = [...annotations];
              newAnnotations[idx] = updated;
              newSlideAnnotations.set(setId, newAnnotations);
              annotationsBySlide.set(slideId, newSlideAnnotations);
              break outer;
            }
          }
        }
        return { ...s, annotationsBySlide };
      });

      return updated;
    },

    /**
     * Delete an annotation.
     */
    async deleteAnnotation(id: string): Promise<void> {
      await apiDeleteAnnotation(id);
      
      update((s) => {
        const annotationsBySlide = new Map(s.annotationsBySlide);
        // Search all slides for the annotation
        outer: for (const [slideId, slideAnnotations] of annotationsBySlide) {
          const newSlideAnnotations = new Map(slideAnnotations);
          for (const [setId, annotations] of newSlideAnnotations) {
            const filtered = annotations.filter((a) => a.id !== id);
            if (filtered.length !== annotations.length) {
              newSlideAnnotations.set(setId, filtered);
              annotationsBySlide.set(slideId, newSlideAnnotations);
              break outer;
            }
          }
        }
        
        // Clear selection if deleted annotation was selected
        let selectedId = s.selectedAnnotationId;
        let highlightedId = s.highlightedAnnotationId;
        if (selectedId === id) selectedId = null;
        if (highlightedId === id) highlightedId = null;

        return {
          ...s,
          annotationsBySlide,
          selectedAnnotationId: selectedId,
          highlightedAnnotationId: highlightedId,
        };
      });
    },

    /**
     * Clear all annotation data.
     */
    clear(): void {
      update((s) => ({
        ...s,
        currentSlideId: null,
        annotationSetsBySlide: new Map(),
        activeAnnotationSetBySlide: new Map(),
        annotationsBySlide: new Map(),
        highlightedAnnotationId: null,
        selectedAnnotationId: null,
        loadingSlides: new Set(),
        error: null,
      }));
    },

    /**
     * Clear annotation data for a specific slide (e.g., when closing a tab).
     */
    clearSlide(slideId: string): void {
      update((s) => {
        const annotationSetsBySlide = new Map(s.annotationSetsBySlide);
        const activeAnnotationSetBySlide = new Map(s.activeAnnotationSetBySlide);
        const annotationsBySlide = new Map(s.annotationsBySlide);
        const loadingSlides = new Set(s.loadingSlides);
        
        annotationSetsBySlide.delete(slideId);
        activeAnnotationSetBySlide.delete(slideId);
        annotationsBySlide.delete(slideId);
        loadingSlides.delete(slideId);
        
        return {
          ...s,
          annotationSetsBySlide,
          activeAnnotationSetBySlide,
          annotationsBySlide,
          loadingSlides,
          currentSlideId: s.currentSlideId === slideId ? null : s.currentSlideId,
        };
      });
    },

    /**
     * Get annotation sets for a specific slide.
     */
    getAnnotationSetsForSlide(slideId: string): AnnotationSet[] {
      return get({ subscribe }).annotationSetsBySlide.get(slideId) ?? [];
    },

    /**
     * Get annotations for a specific slide.
     */
    getAnnotationsForSlide(slideId: string): Map<string, Annotation[]> {
      return get({ subscribe }).annotationsBySlide.get(slideId) ?? new Map();
    },

    /**
     * Get the active annotation set ID for a specific slide.
     */
    getActiveSetIdForSlide(slideId: string): string | null {
      return get({ subscribe }).activeAnnotationSetBySlide.get(slideId) ?? null;
    },
  };
}

// ============================================================================
// Sidebar Layout Store
// ============================================================================

function createSidebarLayoutStore() {
  const { subscribe, set, update } = writable<SidebarLayoutState>(loadSidebarLayout());

  let persistTimeout: ReturnType<typeof setTimeout> | null = null;

  function schedulePersist() {
    if (persistTimeout) clearTimeout(persistTimeout);
    persistTimeout = setTimeout(() => {
      saveSidebarLayout(get({ subscribe }));
      persistTimeout = null;
    }, 300);
  }

  return {
    subscribe,

    toggleSlidesSection(): void {
      update((s) => ({ ...s, slidesSectionCollapsed: !s.slidesSectionCollapsed }));
      schedulePersist();
    },

    toggleAnnotationsSection(): void {
      update((s) => ({ ...s, annotationsSectionCollapsed: !s.annotationsSectionCollapsed }));
      schedulePersist();
    },

    setAnnotationsSectionHeight(height: number): void {
      update((s) => ({
        ...s,
        annotationsSectionHeight: Math.max(MIN_SECTION_HEIGHT, height),
      }));
      schedulePersist();
    },

    setSlidesCollapsed(collapsed: boolean): void {
      update((s) => ({ ...s, slidesSectionCollapsed: collapsed }));
      schedulePersist();
    },

    setAnnotationsCollapsed(collapsed: boolean): void {
      update((s) => ({ ...s, annotationsSectionCollapsed: collapsed }));
      schedulePersist();
    },
  };
}

// ============================================================================
// Store Exports
// ============================================================================

export const annotationStore = createAnnotationStore();
export const sidebarLayoutStore = createSidebarLayoutStore();

// Derived stores for convenience (for the current/focused slide)
export const annotationSets = derived(annotationStore, ($s) => 
  $s.currentSlideId ? $s.annotationSetsBySlide.get($s.currentSlideId) ?? [] : []
);
export const activeAnnotationSet = derived(annotationStore, ($s) => {
  if (!$s.currentSlideId) return null;
  const sets = $s.annotationSetsBySlide.get($s.currentSlideId) ?? [];
  const activeId = $s.activeAnnotationSetBySlide.get($s.currentSlideId);
  return sets.find((set) => set.id === activeId) ?? null;
});
export const activeAnnotations = derived(annotationStore, ($s) => {
  if (!$s.currentSlideId) return [];
  const activeId = $s.activeAnnotationSetBySlide.get($s.currentSlideId);
  if (!activeId) return [];
  const slideAnnotations = $s.annotationsBySlide.get($s.currentSlideId);
  return slideAnnotations?.get(activeId) ?? [];
});
export const isAnnotationLoading = derived(annotationStore, ($s) => 
  $s.currentSlideId ? $s.loadingSlides.has($s.currentSlideId) : false
);
export const annotationError = derived(annotationStore, ($s) => $s.error);

// Visibility helpers
export const getLayerVisibility = (annotationSetId: string): boolean => {
  const state = get(annotationStore);
  return state.layerVisibility.get(annotationSetId) ?? true;
};

// ============================================================================
// Layer Colors
// ============================================================================

/** Color palette for annotation layers */
export const LAYER_COLORS = [
  '#0099ff', // blue
  '#ff6b6b', // red
  '#51cf66', // green
  '#ffd43b', // yellow
  '#cc5de8', // purple
  '#ff922b', // orange
  '#20c997', // teal
  '#f06595', // pink
];

/** Get the color for an annotation layer by its index or set ID */
export function getLayerColor(setIdOrIndex: string | number, slideId?: string): string {
  if (typeof setIdOrIndex === 'number') {
    return LAYER_COLORS[setIdOrIndex % LAYER_COLORS.length];
  }
  const state = get(annotationStore);
  // Look up in the specified slide, or currentSlideId, or search all slides
  const targetSlideId = slideId ?? state.currentSlideId;
  if (targetSlideId) {
    const sets = state.annotationSetsBySlide.get(targetSlideId) ?? [];
    const idx = sets.findIndex((s) => s.id === setIdOrIndex);
    if (idx >= 0) {
      return LAYER_COLORS[idx % LAYER_COLORS.length];
    }
  }
  // Fallback: search all slides
  for (const [, sets] of state.annotationSetsBySlide) {
    const idx = sets.findIndex((s) => s.id === setIdOrIndex);
    if (idx >= 0) {
      return LAYER_COLORS[idx % LAYER_COLORS.length];
    }
  }
  return LAYER_COLORS[0];
}
