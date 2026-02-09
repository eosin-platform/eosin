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
}

export interface ExportModalState {
  /** Whether the export modal is open */
  open: boolean;
  /** Canvas element to export from */
  canvas: HTMLCanvasElement | null;
  /** Annotation overlay element to composite */
  annotationLayer: HTMLElement | null;
  /** Suggested filename (without extension) */
  filename: string;
  /** Export options */
  options: ExportOptions;
}

const defaultOptions: ExportOptions = {
  includeAnnotations: true,
  format: 'png',
  quality: 0.92,
};

const initialState: ExportModalState = {
  open: false,
  canvas: null,
  annotationLayer: null,
  filename: 'export',
  options: { ...defaultOptions },
};

function createExportStore() {
  const { subscribe, set, update } = writable<ExportModalState>(initialState);

  return {
    subscribe,
    
    /**
     * Open the export modal with the given canvas and options.
     */
    open(canvas: HTMLCanvasElement, annotationLayer: HTMLElement | null, filename: string) {
      update((state) => ({
        ...state,
        open: true,
        canvas,
        annotationLayer,
        filename,
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
