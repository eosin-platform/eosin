/**
 * URL synchronization store for sharing viewer state via URL.
 * 
 * Two modes:
 * 1. Single slide mode: Individual query params (?slide=X&x=Y&y=Z&zoom=W&enhance=E&...)
 * 2. Split view mode: Compact base64 JSON (?v=<base64>)
 * 
 * This allows users to share their exact view by copying the URL.
 */

import { derived, get } from 'svelte/store';
import { browser } from '$app/environment';
import { replaceState } from '$app/navigation';
import { tabStore, type SplitState, type Tab, type SavedViewport } from './tabs';
import { settings, type StainEnhancementMode, type StainNormalization } from './settings';

// ============================================================================
// Types
// ============================================================================

/** Viewport and rendering settings for a single slide view */
export interface SingleSlideUrlState {
  slideId: string;
  x: number;
  y: number;
  zoom: number;
  enhance: StainEnhancementMode;
  normalize: StainNormalization;
  sharpen: number;
  gamma: number;
  brightness: number;
  contrast: number;
}

/** Per-tab state for the encoded session */
export interface TabState {
  /** Slide UUID */
  s: string;
  /** Tab label (filename) - optional, can be reconstructed from metadata */
  l?: string;
  /** Slide width */
  w: number;
  /** Slide height */
  h: number;
  /** Viewport: x, y, zoom */
  v?: [number, number, number];
}

/** Per-pane state */
export interface PaneState {
  /** Tabs in this pane */
  t: TabState[];
  /** Active tab index (0-based) */
  a: number;
}

/** Full session state encoded in ?v= */
export interface SessionState {
  /** Panes */
  p: PaneState[];
  /** Focused pane index (0-based) */
  f: number;
  /** Split ratio (0-1) */
  r: number;
  /** Image settings (shared across all tabs) */
  i?: {
    e?: StainEnhancementMode;  // enhance
    n?: StainNormalization;    // normalize
    s?: number;                // sharpen
    g?: number;                // gamma
    b?: number;                // brightness
    c?: number;                // contrast
  };
}

// ============================================================================
// Constants
// ============================================================================

const URL_UPDATE_DEBOUNCE_MS = 300;

// ============================================================================
// Utility Functions
// ============================================================================

/**
 * Encode session state to URL-safe base64.
 */
export function encodeSessionState(state: SessionState): string {
  const json = JSON.stringify(state);
  // Use base64url encoding (replace + with -, / with _, remove padding =)
  const base64 = btoa(json)
    .replace(/\+/g, '-')
    .replace(/\//g, '_')
    .replace(/=+$/, '');
  return base64;
}

/**
 * Decode session state from URL-safe base64.
 */
export function decodeSessionState(encoded: string): SessionState | null {
  try {
    // Restore standard base64 from base64url
    let base64 = encoded
      .replace(/-/g, '+')
      .replace(/_/g, '/');
    // Add padding if needed
    while (base64.length % 4 !== 0) {
      base64 += '=';
    }
    const json = atob(base64);
    return JSON.parse(json) as SessionState;
  } catch {
    console.warn('Failed to decode session state from URL');
    return null;
  }
}

/**
 * Round a number to a reasonable precision for URL params.
 */
function roundForUrl(value: number, decimals: number = 2): number {
  const factor = Math.pow(10, decimals);
  return Math.round(value * factor) / factor;
}

/**
 * Check if a value differs from its default (for omitting defaults in URL).
 */
function isNonDefault<T>(value: T, defaultValue: T): boolean {
  return value !== defaultValue;
}

// ============================================================================
// URL Building
// ============================================================================

/**
 * Build URL search params for single slide mode.
 */
export function buildSingleSlideUrl(state: SingleSlideUrlState): URLSearchParams {
  const params = new URLSearchParams();
  
  params.set('slide', state.slideId);
  params.set('x', roundForUrl(state.x).toString());
  params.set('y', roundForUrl(state.y).toString());
  params.set('zoom', roundForUrl(state.zoom, 6).toString());
  
  // Only include non-default rendering settings
  if (isNonDefault(state.enhance, 'none')) {
    params.set('enhance', state.enhance);
  }
  if (isNonDefault(state.normalize, 'none')) {
    params.set('normalize', state.normalize);
  }
  if (isNonDefault(state.sharpen, 0)) {
    params.set('sharpen', state.sharpen.toString());
  }
  if (isNonDefault(state.gamma, 1.0)) {
    params.set('gamma', roundForUrl(state.gamma, 2).toString());
  }
  if (isNonDefault(state.brightness, 0)) {
    params.set('brightness', roundForUrl(state.brightness).toString());
  }
  if (isNonDefault(state.contrast, 0)) {
    params.set('contrast', roundForUrl(state.contrast).toString());
  }
  
  return params;
}

/**
 * Build URL search params for split/multi-tab mode.
 */
export function buildSessionUrl(splitState: SplitState, imageSettings: {
  stainEnhancement: StainEnhancementMode;
  stainNormalization: StainNormalization;
  sharpeningIntensity: number;
  gamma: number;
  brightness: number;
  contrast: number;
}): URLSearchParams {
  const params = new URLSearchParams();
  
  // Build session state
  const session: SessionState = {
    p: splitState.panes.map((pane) => {
      const activeIdx = pane.activeTabId 
        ? pane.tabs.findIndex(t => t.tabId === pane.activeTabId)
        : 0;
      
      return {
        t: pane.tabs.map((tab) => {
          const tabState: TabState = {
            s: tab.slideId,
            w: tab.width,
            h: tab.height,
          };
          // Only include label if it's not just the first 8 chars of the ID
          if (tab.label && tab.label !== tab.slideId.slice(0, 8)) {
            tabState.l = tab.label;
          }
          // Include viewport if saved
          if (tab.savedViewport) {
            tabState.v = [
              roundForUrl(tab.savedViewport.x),
              roundForUrl(tab.savedViewport.y),
              roundForUrl(tab.savedViewport.zoom, 6),
            ];
          }
          return tabState;
        }),
        a: Math.max(0, activeIdx),
      };
    }),
    f: splitState.panes.findIndex(p => p.paneId === splitState.focusedPaneId),
    r: roundForUrl(splitState.splitRatio, 2),
  };
  
  // Include image settings if any are non-default
  const hasNonDefaultSettings = 
    isNonDefault(imageSettings.stainEnhancement, 'none') ||
    isNonDefault(imageSettings.stainNormalization, 'none') ||
    isNonDefault(imageSettings.sharpeningIntensity, 0) ||
    isNonDefault(imageSettings.gamma, 1.0) ||
    isNonDefault(imageSettings.brightness, 0) ||
    isNonDefault(imageSettings.contrast, 0);
  
  if (hasNonDefaultSettings) {
    session.i = {};
    if (isNonDefault(imageSettings.stainEnhancement, 'none')) {
      session.i.e = imageSettings.stainEnhancement;
    }
    if (isNonDefault(imageSettings.stainNormalization, 'none')) {
      session.i.n = imageSettings.stainNormalization;
    }
    if (isNonDefault(imageSettings.sharpeningIntensity, 0)) {
      session.i.s = imageSettings.sharpeningIntensity;
    }
    if (isNonDefault(imageSettings.gamma, 1.0)) {
      session.i.g = roundForUrl(imageSettings.gamma, 2);
    }
    if (isNonDefault(imageSettings.brightness, 0)) {
      session.i.b = roundForUrl(imageSettings.brightness);
    }
    if (isNonDefault(imageSettings.contrast, 0)) {
      session.i.c = roundForUrl(imageSettings.contrast);
    }
  }
  
  params.set('v', encodeSessionState(session));
  return params;
}

// ============================================================================
// URL Sync Manager
// ============================================================================

/**
 * Create a URL sync manager that keeps the URL in sync with the viewer state.
 */
export function createUrlSyncManager() {
  let urlUpdateTimeout: ReturnType<typeof setTimeout> | null = null;
  let enabled = false;
  let unsubSplit: (() => void) | null = null;
  let unsubSettings: (() => void) | null = null;

  /**
   * Update the URL based on current state.
   * This is debounced to avoid excessive URL updates during rapid viewport changes.
   */
  function scheduleUrlUpdate() {
    if (!enabled || !browser) return;

    if (urlUpdateTimeout) {
      clearTimeout(urlUpdateTimeout);
    }

    urlUpdateTimeout = setTimeout(() => {
      updateUrl();
      urlUpdateTimeout = null;
    }, URL_UPDATE_DEBOUNCE_MS);
  }

  /**
   * Immediately update the URL based on current state.
   */
  function updateUrl() {
    if (!browser) return;

    const splitState = get(tabStore.splitState);
    const currentSettings = settings.get();
    
    // Count total tabs across all panes
    const totalTabs = splitState.panes.reduce((sum, pane) => sum + pane.tabs.length, 0);
    
    if (totalTabs === 0) {
      // No slides open - clear URL
      replaceState('/', {});
      return;
    }
    
    const isSplitView = splitState.panes.length > 1;
    const hasMultipleTabs = totalTabs > 1;
    
    if (!isSplitView && !hasMultipleTabs) {
      // Single slide mode - use individual query params
      const pane = splitState.panes[0];
      const activeTab = pane.tabs.find(t => t.tabId === pane.activeTabId);
      
      if (!activeTab) {
        replaceState('/', {});
        return;
      }
      
      const viewport = activeTab.savedViewport || { x: 0, y: 0, zoom: 0.1 };
      
      const state: SingleSlideUrlState = {
        slideId: activeTab.slideId,
        x: viewport.x,
        y: viewport.y,
        zoom: viewport.zoom,
        enhance: currentSettings.image.stainEnhancement,
        normalize: currentSettings.image.stainNormalization,
        sharpen: currentSettings.image.sharpeningEnabled ? currentSettings.image.sharpeningIntensity : 0,
        gamma: currentSettings.image.gamma,
        brightness: currentSettings.image.brightness,
        contrast: currentSettings.image.contrast,
      };
      
      const params = buildSingleSlideUrl(state);
      replaceState(`?${params.toString()}`, {});
    } else {
      // Split/multi-tab mode - use compact base64 encoding
      const params = buildSessionUrl(splitState, {
        stainEnhancement: currentSettings.image.stainEnhancement,
        stainNormalization: currentSettings.image.stainNormalization,
        sharpeningIntensity: currentSettings.image.sharpeningEnabled ? currentSettings.image.sharpeningIntensity : 0,
        gamma: currentSettings.image.gamma,
        brightness: currentSettings.image.brightness,
        contrast: currentSettings.image.contrast,
      });
      replaceState(`?${params.toString()}`, {});
    }
  }

  /**
   * Start syncing the URL with viewer state.
   */
  function start() {
    if (enabled) return;
    enabled = true;

    // Subscribe to split state changes
    unsubSplit = tabStore.splitState.subscribe(() => {
      scheduleUrlUpdate();
    });

    // Subscribe to settings changes
    unsubSettings = settings.subscribe(() => {
      scheduleUrlUpdate();
    });
  }

  /**
   * Stop syncing the URL.
   */
  function stop() {
    enabled = false;

    if (urlUpdateTimeout) {
      clearTimeout(urlUpdateTimeout);
      urlUpdateTimeout = null;
    }

    if (unsubSplit) {
      unsubSplit();
      unsubSplit = null;
    }

    if (unsubSettings) {
      unsubSettings();
      unsubSettings = null;
    }
  }

  /**
   * Force an immediate URL update (e.g., when viewport changes in ViewerPane).
   */
  function forceUpdate() {
    if (!enabled) return;
    if (urlUpdateTimeout) {
      clearTimeout(urlUpdateTimeout);
      urlUpdateTimeout = null;
    }
    updateUrl();
  }

  return {
    start,
    stop,
    scheduleUpdate: scheduleUrlUpdate,
    forceUpdate,
  };
}

// ============================================================================
// Singleton instance
// ============================================================================

let urlSyncManager: ReturnType<typeof createUrlSyncManager> | null = null;

export function getUrlSyncManager() {
  if (!urlSyncManager) {
    urlSyncManager = createUrlSyncManager();
  }
  return urlSyncManager;
}

// ============================================================================
// Restore state from URL
// ============================================================================

/**
 * Parse session state from URL ?v= parameter and restore it.
 * Returns true if a session was restored, false otherwise.
 */
export function restoreSessionFromUrl(url: URL): SessionState | null {
  const encoded = url.searchParams.get('v');
  if (!encoded) return null;
  
  return decodeSessionState(encoded);
}

/**
 * Convert decoded session state to a SplitState for the tab store.
 * This is used during initial load to restore the session.
 */
export function sessionStateToSplitState(
  session: SessionState, 
  generateTabId: () => string,
  generatePaneId: () => string
): SplitState {
  const panes = session.p.map((paneState) => {
    const paneId = generatePaneId();
    const tabs: Tab[] = paneState.t.map((tabState) => {
      const tabId = generateTabId();
      let savedViewport: SavedViewport | null = null;
      if (tabState.v) {
        savedViewport = {
          x: tabState.v[0],
          y: tabState.v[1],
          zoom: tabState.v[2],
        };
      }
      return {
        tabId,
        slideId: tabState.s,
        label: tabState.l || tabState.s.slice(0, 8),
        width: tabState.w,
        height: tabState.h,
        savedViewport,
      };
    });
    
    return {
      paneId,
      tabs,
      activeTabId: tabs.length > paneState.a ? tabs[paneState.a]?.tabId : (tabs[0]?.tabId || null),
    };
  });

  // Ensure focusedPaneId is valid
  const focusedPaneIndex = Math.max(0, Math.min(session.f, panes.length - 1));
  
  return {
    panes,
    focusedPaneId: panes[focusedPaneIndex]?.paneId || panes[0]?.paneId || '',
    splitRatio: session.r ?? 0.5,
  };
}
