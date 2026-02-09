/**
 * Export modal store for managing export dialog state.
 */

import { writable } from 'svelte/store';

export interface ExportOptions {
  /** Include annotations in the export */
  includeAnnotations: boolean;
  /** Image format */
  format: 'png' | 'jpeg';
  /** JPEG quality (0-1), only used when format is 'jpeg' */
  quality: number;
  /** Export DPI (dots per inch) - affects output resolution */
  dpi: number;
}

export interface ImageFilters {
  /** Brightness adjustment (-100 to 100) */
  brightness: number;
  /** Contrast adjustment (-100 to 100) */
  contrast: number;
  /** Gamma adjustment */
  gamma: number;
}

export interface ExportModalState {
  /** Whether the export modal is open */
  open: boolean;
  /** The viewport container element */
  viewportContainer: HTMLElement | null;
  /** Suggested filename (without extension) */
  filename: string;
  /** Image filter settings */
  filters: ImageFilters;
  /** Viewport dimensions (display pixels) */
  viewportWidth: number;
  viewportHeight: number;
  /** Export options */
  options: ExportOptions;
}

const defaultOptions: ExportOptions = {
  includeAnnotations: true,
  format: 'png',
  quality: 0.92,
  dpi: 96,
};

const defaultFilters: ImageFilters = {
  brightness: 0,
  contrast: 0,
  gamma: 1,
};

const initialState: ExportModalState = {
  open: false,
  viewportContainer: null,
  filename: 'export',
  filters: { ...defaultFilters },
  viewportWidth: 0,
  viewportHeight: 0,
  options: { ...defaultOptions },
};

function createExportStore() {
  const { subscribe, set, update } = writable<ExportModalState>(initialState);

  return {
    subscribe,
    
    /**
     * Open the export modal with the given viewport container.
     */
    open(viewportContainer: HTMLElement, filename: string, filters: ImageFilters, viewportWidth: number, viewportHeight: number) {
      update((state) => ({
        ...state,
        open: true,
        viewportContainer,
        filename,
        filters,
        viewportWidth,
        viewportHeight,
        options: { ...defaultOptions },
      }));
    },

    /**
     * Close the export modal and reset state.
     */
    close() {
      set(initialState);
    },

    /**
     * Update export options.
     */
    updateOptions(options: Partial<ExportOptions>) {
      update((state) => ({
        ...state,
        options: { ...state.options, ...options },
      }));
    },
  };
}

export const exportStore = createExportStore();
