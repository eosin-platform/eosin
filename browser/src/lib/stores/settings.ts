/**
 * Settings store for the WSI viewer application.
 * 
 * This store manages all user-configurable settings across three tiers:
 * 1. Frequent (HUD): brightness, contrast, stain mode, zoom, scale bar, annotations
 * 2. Medium (HUD More menu): gamma, measurement units, navigation, minimap, theme
 * 3. Rare (Settings modal): performance, privacy, advanced rendering options
 * 
 * Settings are persisted to localStorage and loaded on startup.
 */

import { writable, get } from 'svelte/store';
import { browser } from '$app/environment';

// ============================================================================
// Types
// ============================================================================

export type StainMode = 
  | 'he' 
  | 'ihc_dab' 
  | 'ihc_hema' 
  | 'fluorescence' 
  | 'gram' 
  | 'zn_afb' 
  | 'gms';

/** Stain enhancement post-processing modes */
export type StainEnhancementMode = 'none' | 'gram' | 'afb' | 'gms';

export type SensitivityLevel = 'low' | 'medium' | 'high';
export type PrefetchLevel = 'low' | 'medium' | 'high' | 'ludicrous';
export type StreamingQuality = 'auto' | 'full_res' | 'low_res';
export type MeasurementUnit = 'um' | 'mm' | 'in';
export type ThemeMode = 'light' | 'dark' | 'high_contrast';
export type ColorProfile = 'srgb' | 'scanner_native' | 'he_clinical';
export type StainNormalization = 'none' | 'macenko' | 'vahadane';

// Configurable defaults for the Reset to Defaults button
export interface ImageDefaults {
  brightness: number;
  contrast: number;
  gamma: number;
  sharpeningIntensity: number;
  stainEnhancement: StainEnhancementMode;
  stainNormalization: StainNormalization;
}

export interface ImageSettings {
  brightness: number;       // -100 to 100, default 0
  contrast: number;         // -100 to 100, default 0
  gamma: number;            // 0.1 to 3.0, default 1.0
  stainMode: StainMode;
  stainEnhancement: StainEnhancementMode;  // Post-processing stain enhancement
  scaleBarVisible: boolean;
  colorProfile: ColorProfile;
  sharpeningEnabled: boolean;
  sharpeningIntensity: number;  // 0 to 100, default 50
  stainNormalization: StainNormalization;
}

export interface NavigationSettings {
  zoomSensitivity: SensitivityLevel;
  panSensitivity: SensitivityLevel;
  minimapVisible: boolean;
}

export interface AnnotationSettings {
  visible: boolean;
  showLabels: boolean;
  autoClosePolygons: boolean;
  defaultColorPalette: string[];
}

export interface MeasurementSettings {
  units: MeasurementUnit;
}

export interface PerformanceSettings {
  tileCacheSizeMb: number;
  prefetchLevel: PrefetchLevel;
  streamingQuality: StreamingQuality;
  hardwareAccelerationEnabled: boolean;
  undoBufferSize: number; // Number of undo steps to keep (default 50)
}

export interface PrivacySettings {
  phiMaskingEnabled: boolean;
  screenshotsDisabled: boolean;
  autoLogoutMinutes: number;
}

export interface BehaviorSettings {
  smoothNavigation: boolean;
}

export interface UISettings {
  theme: ThemeMode;
  language: string;
}

export interface Settings {
  image: ImageSettings;
  navigation: NavigationSettings;
  annotations: AnnotationSettings;
  measurements: MeasurementSettings;
  performance: PerformanceSettings;
  privacy: PrivacySettings;
  behavior: BehaviorSettings;
  ui: UISettings;
  defaults: ImageDefaults;  // Configurable defaults for Reset to Defaults
}

// ============================================================================
// Default values
// ============================================================================

export const DEFAULT_COLOR_PALETTE = [
  '#ef4444', // red
  '#f97316', // orange
  '#eab308', // yellow
  '#22c55e', // green
  '#06b6d4', // cyan
  '#5E4AEF', // purple (primary)
  '#8b5cf6', // violet
  '#ec4899', // pink
];

// Factory defaults for image settings (used when resetting user defaults)
export const FACTORY_IMAGE_DEFAULTS: ImageDefaults = {
  brightness: 0,
  contrast: 0,
  gamma: 1.0,
  sharpeningIntensity: 0,
  stainEnhancement: 'none',
  stainNormalization: 'none',
};

export const DEFAULT_SETTINGS: Settings = {
  image: {
    brightness: 0,
    contrast: 0,
    gamma: 1.0,
    stainMode: 'he',
    stainEnhancement: 'none',  // No post-processing enhancement by default
    scaleBarVisible: true,
    colorProfile: 'srgb',
    sharpeningEnabled: false,
    sharpeningIntensity: 50,
    stainNormalization: 'none',
  },
  navigation: {
    zoomSensitivity: 'medium',
    panSensitivity: 'medium',
    minimapVisible: true,
  },
  annotations: {
    visible: true,
    showLabels: true,
    autoClosePolygons: true,
    defaultColorPalette: [...DEFAULT_COLOR_PALETTE],
  },
  measurements: {
    units: 'um',
  },
  performance: {
    tileCacheSizeMb: 512,
    prefetchLevel: 'medium',
    streamingQuality: 'auto',
    hardwareAccelerationEnabled: false,
    undoBufferSize: 50,
  },
  privacy: {
    phiMaskingEnabled: false,
    screenshotsDisabled: false,
    autoLogoutMinutes: 30,
  },
  behavior: {
    smoothNavigation: true,
  },
  ui: {
    theme: 'dark',
    language: 'en',
  },
  defaults: { ...FACTORY_IMAGE_DEFAULTS },
};

// ============================================================================
// Persistence
// ============================================================================

const STORAGE_KEY = 'eosin-settings';
const PERSIST_DEBOUNCE_MS = 500;

let persistTimeout: ReturnType<typeof setTimeout> | null = null;

/**
 * Deep merge two objects, preferring values from the source.
 * This ensures new default settings are added when the schema evolves.
 */
function deepMerge<T extends Record<string, unknown>>(
  target: T,
  source: Partial<T>
): T {
  const result = { ...target };
  for (const key in source) {
    if (Object.prototype.hasOwnProperty.call(source, key)) {
      const sourceVal = source[key];
      const targetVal = target[key];
      if (
        sourceVal !== null &&
        typeof sourceVal === 'object' &&
        !Array.isArray(sourceVal) &&
        targetVal !== null &&
        typeof targetVal === 'object' &&
        !Array.isArray(targetVal)
      ) {
        result[key] = deepMerge(
          targetVal as Record<string, unknown>,
          sourceVal as Record<string, unknown>
        ) as T[Extract<keyof T, string>];
      } else if (sourceVal !== undefined) {
        result[key] = sourceVal as T[Extract<keyof T, string>];
      }
    }
  }
  return result;
}

/**
 * Load settings from localStorage, merged with defaults.
 */
function loadSettings(): Settings {
  if (!browser) return { ...DEFAULT_SETTINGS };
  
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) return { ...DEFAULT_SETTINGS };
    
    const parsed = JSON.parse(raw) as Partial<Settings>;
    // Deep merge to handle schema evolution - manually merge each section
    return {
      image: { ...DEFAULT_SETTINGS.image, ...parsed.image },
      navigation: { ...DEFAULT_SETTINGS.navigation, ...parsed.navigation },
      annotations: { ...DEFAULT_SETTINGS.annotations, ...parsed.annotations },
      measurements: { ...DEFAULT_SETTINGS.measurements, ...parsed.measurements },
      performance: { ...DEFAULT_SETTINGS.performance, ...parsed.performance },
      privacy: { ...DEFAULT_SETTINGS.privacy, ...parsed.privacy },
      behavior: { ...DEFAULT_SETTINGS.behavior, ...parsed.behavior },
      ui: { ...DEFAULT_SETTINGS.ui, ...parsed.ui },
      defaults: { ...DEFAULT_SETTINGS.defaults, ...parsed.defaults },
    };
  } catch {
    console.warn('Failed to load settings from localStorage');
    return { ...DEFAULT_SETTINGS };
  }
}

/**
 * Save settings to localStorage (debounced).
 */
function saveSettings(settings: Settings): void {
  if (!browser) return;
  
  if (persistTimeout) {
    clearTimeout(persistTimeout);
  }
  
  persistTimeout = setTimeout(() => {
    try {
      localStorage.setItem(STORAGE_KEY, JSON.stringify(settings));
    } catch (e) {
      console.warn('Failed to save settings to localStorage:', e);
    }
    persistTimeout = null;
  }, PERSIST_DEBOUNCE_MS);
}

// ============================================================================
// Store creation
// ============================================================================

function createSettingsStore() {
  const initialSettings = loadSettings();
  const { subscribe, set, update } = writable<Settings>(initialSettings);

  // Subscribe to persist changes
  subscribe((settings) => {
    saveSettings(settings);
  });

  return {
    subscribe,

    /**
     * Set a specific setting by path (e.g., "image.brightness").
     */
    setSetting<K extends keyof Settings>(
      section: K,
      key: keyof Settings[K],
      value: Settings[K][keyof Settings[K]]
    ): void {
      update((s) => ({
        ...s,
        [section]: {
          ...s[section],
          [key]: value,
        },
      }));
    },

    /**
     * Set a setting using a dot-notation path string.
     * Example: setSettingByPath('image.brightness', 50)
     */
    setSettingByPath(path: string, value: unknown): void {
      const parts = path.split('.');
      if (parts.length !== 2) {
        console.warn(`Invalid settings path: ${path}`);
        return;
      }
      const [section, key] = parts;
      update((s) => {
        const sectionKey = section as keyof Settings;
        if (!(sectionKey in s)) {
          console.warn(`Unknown settings section: ${section}`);
          return s;
        }
        return {
          ...s,
          [sectionKey]: {
            ...s[sectionKey],
            [key]: value,
          },
        };
      });
    },

    /**
     * Update an entire section at once.
     */
    updateSection<K extends keyof Settings>(
      section: K,
      updates: Partial<Settings[K]>
    ): void {
      update((s) => ({
        ...s,
        [section]: {
          ...s[section],
          ...updates,
        },
      }));
    },

    /**
     * Reset all settings to defaults.
     */
    resetToDefaults(): void {
      set({ ...DEFAULT_SETTINGS });
    },

    /**
     * Reset a specific section to defaults.
     */
    resetSection<K extends keyof Settings>(section: K): void {
      update((s) => ({
        ...s,
        [section]: { ...DEFAULT_SETTINGS[section] },
      }));
    },

    /**
     * Get the current settings value (non-reactive).
     */
    get(): Settings {
      return get({ subscribe });
    },
  };
}

export const settings = createSettingsStore();

// ============================================================================
// Derived stores for specific sections (for convenience)
// ============================================================================

import { derived } from 'svelte/store';

export const imageSettings = derived(settings, ($s) => $s.image);
export const navigationSettings = derived(settings, ($s) => $s.navigation);
export const annotationSettings = derived(settings, ($s) => $s.annotations);
export const measurementSettings = derived(settings, ($s) => $s.measurements);
export const performanceSettings = derived(settings, ($s) => $s.performance);
export const privacySettings = derived(settings, ($s) => $s.privacy);
export const behaviorSettings = derived(settings, ($s) => $s.behavior);
export const uiSettings = derived(settings, ($s) => $s.ui);
export const imageDefaults = derived(settings, ($s) => $s.defaults);

// ============================================================================
// UI state for modals/popups (not persisted)
// ============================================================================

export const settingsModalOpen = writable(false);
export const hudMoreMenuOpen = writable(false);
export const helpMenuOpen = writable(false);
