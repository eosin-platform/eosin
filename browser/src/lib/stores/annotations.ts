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
  /** Current slide ID being viewed */
  currentSlideId: string | null;
  /** Annotation sets for the current slide */
  annotationSets: AnnotationSet[];
  /** Currently active (selected) annotation set ID */
  activeAnnotationSetId: string | null;
  /** Map of annotation set ID -> annotations */
  annotationsBySet: Map<string, Annotation[]>;
  /** Map of annotation set ID -> visibility */
  layerVisibility: Map<string, boolean>;
  /** Currently highlighted annotation ID (for hover effects) */
  highlightedAnnotationId: string | null;
  /** Currently selected annotation ID (for editing) */
  selectedAnnotationId: string | null;
  /** Loading state */
  isLoading: boolean;
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
  annotationSets: [],
  activeAnnotationSetId: null,
  annotationsBySet: new Map(),
  layerVisibility: new Map(),
  highlightedAnnotationId: null,
  selectedAnnotationId: null,
  isLoading: false,
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
     */
    async loadForSlide(slideId: string): Promise<void> {
      update((s) => ({
        ...s,
        currentSlideId: slideId,
        isLoading: true,
        error: null,
      }));

      try {
        const sets = await fetchAnnotationSets(slideId);
        
        // Initialize visibility for new sets
        const visibility = new Map(get({ subscribe }).layerVisibility);
        for (const set of sets) {
          if (!visibility.has(set.id)) {
            visibility.set(set.id, true);
          }
        }

        // Set first set as active if none selected
        const currentActive = get({ subscribe }).activeAnnotationSetId;
        const activeId = sets.find((set: AnnotationSet) => set.id === currentActive)?.id ?? sets[0]?.id ?? null;

        update((s) => ({
          ...s,
          annotationSets: sets,
          activeAnnotationSetId: activeId,
          layerVisibility: visibility,
          isLoading: false,
        }));

        schedulePersist();

        // Load annotations for the active set
        if (activeId) {
          await annotationStore.loadAnnotationsForSet(activeId);
        }
      } catch (err) {
        update((s) => ({
          ...s,
          isLoading: false,
          error: err instanceof Error ? err.message : 'Failed to load annotation sets',
        }));
      }
    },

    /**
     * Load annotations for an annotation set.
     */
    async loadAnnotationsForSet(annotationSetId: string): Promise<void> {
      try {
        const annotations = await fetchAnnotations(annotationSetId);
        update((s) => {
          const newMap = new Map(s.annotationsBySet);
          newMap.set(annotationSetId, annotations);
          return { ...s, annotationsBySet: newMap };
        });
      } catch (err) {
        console.error('Failed to load annotations:', err);
      }
    },

    /**
     * Set the active annotation set.
     */
    async setActiveSet(annotationSetId: string | null): Promise<void> {
      update((s) => ({ ...s, activeAnnotationSetId: annotationSetId }));
      
      if (annotationSetId && !get({ subscribe }).annotationsBySet.has(annotationSetId)) {
        await annotationStore.loadAnnotationsForSet(annotationSetId);
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

      const created = await apiCreateAnnotationSet(state.currentSlideId, data);
      
      update((s) => {
        const newSets = [...s.annotationSets, created];
        const visibility = new Map(s.layerVisibility);
        visibility.set(created.id, true);
        return {
          ...s,
          annotationSets: newSets,
          activeAnnotationSetId: created.id,
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
      
      update((s) => ({
        ...s,
        annotationSets: s.annotationSets.map((set) => (set.id === id ? updated : set)),
      }));

      return updated;
    },

    /**
     * Delete an annotation set.
     */
    async deleteSet(id: string): Promise<void> {
      await apiDeleteAnnotationSet(id);
      
      update((s) => {
        const newSets = s.annotationSets.filter((set) => set.id !== id);
        const visibility = new Map(s.layerVisibility);
        visibility.delete(id);
        const annotationsBySet = new Map(s.annotationsBySet);
        annotationsBySet.delete(id);
        
        // If deleted set was active, select next one
        let activeId = s.activeAnnotationSetId;
        if (activeId === id) {
          activeId = newSets[0]?.id ?? null;
        }

        return {
          ...s,
          annotationSets: newSets,
          activeAnnotationSetId: activeId,
          layerVisibility: visibility,
          annotationsBySet,
        };
      });

      schedulePersist();
    },

    /**
     * Create a new annotation.
     */
    async createAnnotation(data: CreateAnnotationRequest): Promise<Annotation> {
      const state = get({ subscribe });
      if (!state.activeAnnotationSetId) {
        throw new Error('No annotation set selected');
      }

      const created = await apiCreateAnnotation(state.activeAnnotationSetId, data);
      
      update((s) => {
        const annotationsBySet = new Map(s.annotationsBySet);
        const existing = annotationsBySet.get(state.activeAnnotationSetId!) ?? [];
        annotationsBySet.set(state.activeAnnotationSetId!, [...existing, created]);
        return { ...s, annotationsBySet };
      });

      return created;
    },

    /**
     * Update an annotation.
     */
    async updateAnnotation(id: string, data: UpdateAnnotationRequest): Promise<Annotation> {
      const updated = await apiUpdateAnnotation(id, data);
      
      update((s) => {
        const annotationsBySet = new Map(s.annotationsBySet);
        for (const [setId, annotations] of annotationsBySet) {
          const idx = annotations.findIndex((a) => a.id === id);
          if (idx >= 0) {
            const newAnnotations = [...annotations];
            newAnnotations[idx] = updated;
            annotationsBySet.set(setId, newAnnotations);
            break;
          }
        }
        return { ...s, annotationsBySet };
      });

      return updated;
    },

    /**
     * Delete an annotation.
     */
    async deleteAnnotation(id: string): Promise<void> {
      await apiDeleteAnnotation(id);
      
      update((s) => {
        const annotationsBySet = new Map(s.annotationsBySet);
        for (const [setId, annotations] of annotationsBySet) {
          const filtered = annotations.filter((a) => a.id !== id);
          if (filtered.length !== annotations.length) {
            annotationsBySet.set(setId, filtered);
            break;
          }
        }
        
        // Clear selection if deleted annotation was selected
        let selectedId = s.selectedAnnotationId;
        let highlightedId = s.highlightedAnnotationId;
        if (selectedId === id) selectedId = null;
        if (highlightedId === id) highlightedId = null;

        return {
          ...s,
          annotationsBySet,
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
        annotationSets: [],
        activeAnnotationSetId: null,
        annotationsBySet: new Map(),
        highlightedAnnotationId: null,
        selectedAnnotationId: null,
        isLoading: false,
        error: null,
      }));
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

// Derived stores for convenience
export const annotationSets = derived(annotationStore, ($s) => $s.annotationSets);
export const activeAnnotationSet = derived(annotationStore, ($s) =>
  $s.annotationSets.find((set) => set.id === $s.activeAnnotationSetId) ?? null
);
export const activeAnnotations = derived(annotationStore, ($s) =>
  $s.activeAnnotationSetId ? $s.annotationsBySet.get($s.activeAnnotationSetId) ?? [] : []
);
export const isAnnotationLoading = derived(annotationStore, ($s) => $s.isLoading);
export const annotationError = derived(annotationStore, ($s) => $s.error);

// Visibility helpers
export const getLayerVisibility = (annotationSetId: string): boolean => {
  const state = get(annotationStore);
  return state.layerVisibility.get(annotationSetId) ?? true;
};
