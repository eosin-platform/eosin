/**
 * Export modal store for managing export dialog state.
 */

import { writable, get } from 'svelte/store';
import { browser } from '$app/environment';

/** RGBA color with components 0-255 for RGB and 0-1 for alpha */
export interface RgbaColor {
  r: number;
  g: number;
  b: number;
  a: number;
}

export type LineStyle = 'solid' | 'dashed' | 'dotted';
export type LineCap = 'round' | 'square' | 'butt';

export interface RoiOutlineOptions {
  enabled: boolean;
  color: RgbaColor;
  thickness: number;
  lineStyle: LineStyle;
  lineCap: LineCap;
  dashLength: number;
  dashSpacing: number;
  dotSpacing: number;
}

export interface RoiOverlayOptions {
  enabled: boolean;
  color: RgbaColor;
}

export interface MeasurementOptions {
  color: RgbaColor;
  thickness: number;
  lineStyle: LineStyle;
  lineCap: LineCap;
  dashLength: number;
  dashSpacing: number;
  dotSpacing: number;
  fontSize: number;
}

export interface ExportOptions {
  /** Include annotations in the export */
  includeAnnotations: boolean;
  /** Image format */
  format: 'png' | 'jpeg';
  /** JPEG quality (0-1), only used when format is 'jpeg' */
  quality: number;
  /** Export DPI (dots per inch) - affects output resolution */
  dpi: number;
  /** Show measurement overlay */
  showMeasurement: boolean;
  /** Measurement style options */
  measurementOptions: MeasurementOptions;
  /** ROI outline options */
  roiOutline: RoiOutlineOptions;
  /** ROI outside overlay options */
  roiOverlay: RoiOverlayOptions;
}

export interface ImageFilters {
  /** Brightness adjustment (-100 to 100) */
  brightness: number;
  /** Contrast adjustment (-100 to 100) */
  contrast: number;
  /** Gamma adjustment */
  gamma: number;
}

/** Measurement state for export */
export interface MeasurementExportState {
  active: boolean;
  startImage: { x: number; y: number } | null;
  endImage: { x: number; y: number } | null;
}

/** ROI state for export */
export interface RoiExportState {
  active: boolean;
  startImage: { x: number; y: number } | null;
  endImage: { x: number; y: number } | null;
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
  /** Microns per pixel for measurement display */
  micronsPerPixel: number;
  /** Viewport zoom and position for coordinate conversion */
  viewportState: { x: number; y: number; zoom: number } | null;
  /** Export options */
  options: ExportOptions;
  /** Measurement state */
  measurement: MeasurementExportState;
  /** ROI state */
  roi: RoiExportState;
}

const defaultRoiOutline: RoiOutlineOptions = {
  enabled: true,
  color: { r: 251, g: 191, b: 36, a: 1 }, // Yellow (#fbbf24)
  thickness: 2,
  lineStyle: 'dashed',
  lineCap: 'round',
  dashLength: 8,
  dashSpacing: 4,
  dotSpacing: 4,
};

const defaultRoiOverlay: RoiOverlayOptions = {
  enabled: false,
  color: { r: 0, g: 0, b: 0, a: 0.4 },
};

const defaultMeasurementOptions: MeasurementOptions = {
  color: { r: 59, g: 130, b: 246, a: 1 }, // Blue (#3b82f6)
  thickness: 2,
  lineStyle: 'solid',
  lineCap: 'round',
  dashLength: 8,
  dashSpacing: 4,
  dotSpacing: 4,
  fontSize: 20,
};

const defaultOptions: ExportOptions = {
  includeAnnotations: true,
  format: 'jpeg',
  quality: 0.92,
  dpi: 96,
  showMeasurement: true,
  measurementOptions: { ...defaultMeasurementOptions },
  roiOutline: { ...defaultRoiOutline },
  roiOverlay: { ...defaultRoiOverlay },
};

const defaultFilters: ImageFilters = {
  brightness: 0,
  contrast: 0,
  gamma: 1,
};

const defaultMeasurement: MeasurementExportState = {
  active: false,
  startImage: null,
  endImage: null,
};

const defaultRoi: RoiExportState = {
  active: false,
  startImage: null,
  endImage: null,
};

const initialState: ExportModalState = {
  open: false,
  viewportContainer: null,
  filename: 'export',
  filters: { ...defaultFilters },
  viewportWidth: 0,
  viewportHeight: 0,
  micronsPerPixel: 0.25,
  viewportState: null,
  options: { ...defaultOptions },
  measurement: { ...defaultMeasurement },
  roi: { ...defaultRoi },
};

// LocalStorage key for persisting export options
const EXPORT_OPTIONS_KEY = 'eosin-export-options';

// Load saved options from localStorage
function loadSavedOptions(): Partial<ExportOptions> {
  if (!browser) return {};
  try {
    const stored = localStorage.getItem(EXPORT_OPTIONS_KEY);
    if (stored) {
      return JSON.parse(stored);
    }
  } catch (e) {
    console.warn('Failed to load export options:', e);
  }
  return {};
}

// Save options to localStorage
function saveOptions(options: ExportOptions) {
  if (!browser) return;
  try {
    localStorage.setItem(EXPORT_OPTIONS_KEY, JSON.stringify(options));
  } catch (e) {
    console.warn('Failed to save export options:', e);
  }
}

// Clear saved options from localStorage
function clearSavedOptions() {
  if (!browser) return;
  try {
    localStorage.removeItem(EXPORT_OPTIONS_KEY);
  } catch (e) {
    // Ignore
  }
}

function createExportStore() {
  const { subscribe, set, update } = writable<ExportModalState>(initialState);

  return {
    subscribe,
    
    /**
     * Open the export modal with the given viewport container.
     */
    open(
      viewportContainer: HTMLElement,
      filename: string,
      filters: ImageFilters,
      viewportWidth: number,
      viewportHeight: number,
      viewportState: { x: number; y: number; zoom: number },
      micronsPerPixel: number,
      measurement: MeasurementExportState,
      roi: RoiExportState
    ) {
      const savedOptions = loadSavedOptions();
      const hasMeasurement = measurement.active && measurement.startImage !== null && measurement.endImage !== null;
      const hasRoi = roi.active && roi.startImage !== null && roi.endImage !== null;
      
      // Use saved enabled state if feature is available, otherwise disable
      const savedShowMeasurement = savedOptions.showMeasurement ?? defaultOptions.showMeasurement;
      const savedRoiOutlineEnabled = savedOptions.roiOutline?.enabled ?? defaultRoiOutline.enabled;
      const savedRoiOverlayEnabled = savedOptions.roiOverlay?.enabled ?? defaultRoiOverlay.enabled;
      
      update((state) => ({
        ...state,
        open: true,
        viewportContainer,
        filename,
        filters,
        viewportWidth,
        viewportHeight,
        viewportState,
        micronsPerPixel,
        measurement,
        roi,
        options: { 
          ...defaultOptions,
          ...savedOptions,
          // Use saved enabled state if feature is available, otherwise force disabled
          showMeasurement: hasMeasurement && savedShowMeasurement,
          roiOutline: {
            ...defaultRoiOutline,
            ...(savedOptions.roiOutline || {}),
            enabled: hasRoi && savedRoiOutlineEnabled,
          },
          roiOverlay: {
            ...defaultRoiOverlay,
            ...(savedOptions.roiOverlay || {}),
            enabled: hasRoi && savedRoiOverlayEnabled,
          },
        },
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
      update((state) => {
        const newOptions = { ...state.options, ...options };
        saveOptions(newOptions);
        return {
          ...state,
          options: newOptions,
        };
      });
    },

    /**
     * Reset export options to defaults.
     */
    resetOptions() {
      clearSavedOptions();
      update((state) => {
        const hasMeasurement = state.measurement.active && state.measurement.startImage !== null && state.measurement.endImage !== null;
        const hasRoi = state.roi.active && state.roi.startImage !== null && state.roi.endImage !== null;
        return {
          ...state,
          options: {
            ...defaultOptions,
            showMeasurement: hasMeasurement && defaultOptions.showMeasurement,
            roiOutline: {
              ...defaultRoiOutline,
              enabled: hasRoi && defaultRoiOutline.enabled,
            },
            roiOverlay: {
              ...defaultRoiOverlay,
              enabled: hasRoi && defaultRoiOverlay.enabled,
            },
          },
        };
      });
    },
  };
}

export const exportStore = createExportStore();
