/**
 * Export modal store for managing export dialog state.
 */

import { writable } from 'svelte/store';

/** RGBA color with components 0-255 for RGB and 0-1 for alpha */
export interface RgbaColor {
  r: number;
  g: number;
  b: number;
  a: number;
}

export type LineStyle = 'solid' | 'dashed' | 'dotted';

export interface RoiOutlineOptions {
  enabled: boolean;
  color: RgbaColor;
  thickness: number;
  lineStyle: LineStyle;
}

export interface RoiOverlayOptions {
  enabled: boolean;
  color: RgbaColor;
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
};

const defaultRoiOverlay: RoiOverlayOptions = {
  enabled: false,
  color: { r: 0, g: 0, b: 0, a: 0.2 },
};

const defaultOptions: ExportOptions = {
  includeAnnotations: true,
  format: 'png',
  quality: 0.92,
  dpi: 96,
  showMeasurement: true,
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
          // Only enable measurement/ROI options if they are active
          showMeasurement: measurement.active && measurement.startImage !== null && measurement.endImage !== null,
          roiOutline: {
            ...defaultRoiOutline,
            enabled: roi.active && roi.startImage !== null && roi.endImage !== null,
          },
          roiOverlay: {
            ...defaultRoiOverlay,
            enabled: false,
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
      update((state) => ({
        ...state,
        options: { ...state.options, ...options },
      }));
    },
  };
}

export const exportStore = createExportStore();
