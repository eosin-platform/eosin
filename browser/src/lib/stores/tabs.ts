import { writable, derived, get } from 'svelte/store';

/** Viewport position/zoom saved when switching away from a tab. */
export interface SavedViewport {
  x: number;
  y: number;
  zoom: number;
}

export interface Tab {
  /** Unique tab identifier */
  tabId: string;
  /** Slide UUID */
  slideId: string;
  /** Display label (filename) */
  label: string;
  /** Slide width */
  width: number;
  /** Slide height */
  height: number;
  /** Viewport state saved when the tab was last deactivated */
  savedViewport: SavedViewport | null;
}

export interface Pane {
  /** Unique pane identifier */
  paneId: string;
  /** Tabs within this pane */
  tabs: Tab[];
  /** Active tab ID within this pane */
  activeTabId: string | null;
}

export interface SplitState {
  /** All panes (1 = no split, 2 = split view) */
  panes: Pane[];
  /** Which pane currently has focus */
  focusedPaneId: string;
  /** Divider position as fraction (0–1), default 0.5 */
  splitRatio: number;
}

// ---- localStorage persistence ----
const STORAGE_KEY = 'histion-tab-state';
const PERSIST_DEBOUNCE_MS = 300;

/**
 * Extract the highest numeric suffix from all tab-N / pane-N identifiers
 * in a restored SplitState so the counter can resume without collisions.
 */
function extractMaxId(state: SplitState): number {
  let max = 0;
  for (const pane of state.panes) {
    if (!pane?.paneId) continue;
    const pm = pane.paneId.match(/^(?:tab|pane)-(\d+)$/);
    if (pm) max = Math.max(max, parseInt(pm[1], 10));
    for (const tab of pane.tabs ?? []) {
      if (!tab?.tabId) continue;
      const tm = tab.tabId.match(/^(?:tab|pane)-(\d+)$/);
      if (tm) max = Math.max(max, parseInt(tm[1], 10));
    }
  }
  return max;
}

/**
 * Validate that a value parsed from JSON has the shape of SplitState.
 * Returns the value cast to SplitState, or null if invalid.
 */
function validateSplitState(v: unknown): SplitState | null {
  if (!v || typeof v !== 'object') return null;
  const obj = v as Record<string, unknown>;
  if (!Array.isArray(obj.panes) || obj.panes.length === 0) return null;
  if (typeof obj.focusedPaneId !== 'string') return null;
  if (typeof obj.splitRatio !== 'number') return null;
  for (const pane of obj.panes as unknown[]) {
    if (!pane || typeof pane !== 'object') return null;
    const p = pane as Record<string, unknown>;
    if (typeof p.paneId !== 'string') return null;
    if (!Array.isArray(p.tabs)) return null;
    for (const tab of p.tabs as unknown[]) {
      if (!tab || typeof tab !== 'object') return null;
      const t = tab as Record<string, unknown>;
      if (typeof t.tabId !== 'string' || typeof t.slideId !== 'string') return null;
      if (typeof t.width !== 'number' || typeof t.height !== 'number') return null;
    }
  }
  return v as SplitState;
}

function loadState(): SplitState | null {
  if (typeof window === 'undefined') return null;
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) return null;
    return validateSplitState(JSON.parse(raw));
  } catch {
    return null;
  }
}

function saveState(state: SplitState): void {
  if (typeof window === 'undefined') return;
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(state));
  } catch {
    // Quota exceeded or unavailable — silently ignore.
  }
}

function createTabStore() {
  // Try to restore from localStorage first.
  const restored = loadState();
  let nextId = restored ? extractMaxId(restored) + 1 : 1;

  function generateTabId(): string {
    return `tab-${nextId++}`;
  }

  function generatePaneId(): string {
    return `pane-${nextId++}`;
  }

  // ---- Core state ----
  let initialState: SplitState;
  if (restored) {
    initialState = restored;
  } else {
    const initialPaneId = generatePaneId();
    initialState = {
      panes: [{ paneId: initialPaneId, tabs: [], activeTabId: null }],
      focusedPaneId: initialPaneId,
      splitRatio: 0.5,
    };
  }

  const splitState = writable<SplitState>(initialState);

  // Debounced persistence: save to localStorage on every change.
  let persistTimeout: ReturnType<typeof setTimeout> | null = null;
  splitState.subscribe((state) => {
    if (typeof window === 'undefined') return;
    if (persistTimeout) clearTimeout(persistTimeout);
    persistTimeout = setTimeout(() => {
      saveState(state);
      persistTimeout = null;
    }, PERSIST_DEBOUNCE_MS);
  });

  // ---- Derived convenience stores (backwards-compatible) ----

  /** Focused pane's tabs */
  const tabs = derived(splitState, ($s) => {
    const focused = $s.panes.find((p) => p?.paneId === $s.focusedPaneId);
    return focused?.tabs ?? [];
  });

  /** Focused pane's active tab ID */
  const activeTabId = derived(splitState, ($s) => {
    const focused = $s.panes.find((p) => p?.paneId === $s.focusedPaneId);
    return focused?.activeTabId ?? null;
  });

  /** Focused pane's active tab */
  const activeTab = derived(splitState, ($s) => {
    const focused = $s.panes.find((p) => p?.paneId === $s.focusedPaneId);
    if (!focused || !focused.activeTabId) return null;
    return focused.tabs.find((t) => t?.tabId === focused.activeTabId) ?? null;
  });

  /** Whether the view is currently split */
  const isSplit = derived(splitState, ($s) => $s.panes.length > 1);

  // ---- Pane helpers ----

  function getPaneForTab(state: SplitState, tabId: string): Pane | undefined {
    return state.panes.find((p) => p?.tabs?.some((t) => t?.tabId === tabId));
  }

  function getFocusedPane(state: SplitState): Pane {
    return state.panes.find((p) => p?.paneId === state.focusedPaneId) ?? state.panes[0];
  }

  function updatePane(state: SplitState, paneId: string, updater: (p: Pane) => Pane): SplitState {
    return {
      ...state,
      panes: state.panes.filter((p) => p != null).map((p) => (p.paneId === paneId ? updater(p) : p)),
    };
  }

  // ---- Focus ----

  function setFocusedPane(paneId: string) {
    splitState.update((s) => ({ ...s, focusedPaneId: paneId }));
  }

  // ---- Tab operations (operate on focused pane by default) ----

  /**
   * Open a slide in the current tab (replaces active tab content),
   * or creates a new tab if none exists.
   */
  function open(slideId: string, label: string, width: number, height: number, initialViewport?: SavedViewport | null) {
    const vp = initialViewport ?? null;
    splitState.update((s) => {
      const pane = getFocusedPane(s);
      if (pane.activeTabId) {
        return updatePane(s, pane.paneId, (p) => ({
          ...p,
          tabs: p.tabs.map((tab) =>
            tab.tabId === p.activeTabId
              ? { ...tab, slideId, label, width, height, savedViewport: vp }
              : tab,
          ),
        }));
      } else {
        const tabId = generateTabId();
        return updatePane(s, pane.paneId, (p) => ({
          ...p,
          tabs: [...p.tabs, { tabId, slideId, label, width, height, savedViewport: vp }],
          activeTabId: tabId,
        }));
      }
    });
  }

  /**
   * Open a slide in a new tab in the focused pane.
   */
  function openInNewTab(slideId: string, label: string, width: number, height: number) {
    splitState.update((s) => {
      const pane = getFocusedPane(s);
      const tabId = generateTabId();
      return updatePane(s, pane.paneId, (p) => ({
        ...p,
        tabs: [...p.tabs, { tabId, slideId, label, width, height, savedViewport: null }],
        activeTabId: tabId,
      }));
    });
  }

  /**
   * Open a slide in a new tab in a specific pane.
   */
  function openInNewTabInPane(
    paneId: string,
    slideId: string,
    label: string,
    width: number,
    height: number,
  ) {
    splitState.update((s) => {
      const tabId = generateTabId();
      return updatePane(s, paneId, (p) => ({
        ...p,
        tabs: [...p.tabs, { tabId, slideId, label, width, height, savedViewport: null }],
        activeTabId: tabId,
      }));
    });
  }

  /**
   * Close a tab by its tabId (auto-detects which pane it's in).
   */
  function closeTab(tabId: string) {
    console.debug('[tabs] closeTab called:', tabId);
    splitState.update((s) => {
      const pane = getPaneForTab(s, tabId);
      if (!pane) return s;

      const tabs = pane.tabs ?? [];
      const idx = tabs.findIndex((t) => t?.tabId === tabId);
      if (idx === -1) return s;

      const newTabs = tabs.filter((t) => t?.tabId !== tabId);
      let newActiveTabId = pane.activeTabId;

      if (pane.activeTabId === tabId) {
        if (newTabs.length === 0) {
          newActiveTabId = null;
        } else {
          const newIdx = Math.min(idx, newTabs.length - 1);
          newActiveTabId = newTabs[newIdx]?.tabId ?? null;
        }
      }

      let newState = updatePane(s, pane.paneId, () => ({
        ...pane,
        tabs: newTabs,
        activeTabId: newActiveTabId,
      }));

      // If this pane is now empty AND there are other panes, collapse it
      if (newTabs.length === 0 && newState.panes.length > 1) {
        newState = {
          ...newState,
          panes: newState.panes.filter((p) => p?.paneId !== pane.paneId),
        };
        // If focused pane was removed, focus the remaining one
        if (newState.focusedPaneId === pane.paneId && newState.panes[0]) {
          newState.focusedPaneId = newState.panes[0]?.paneId ?? '';
        }
      }

      console.debug('[tabs] closeTab result:', { 
        paneCount: newState.panes.length, 
        totalTabs: newState.panes.reduce((sum, p) => sum + (p?.tabs?.length ?? 0), 0) 
      });
      return newState;
    });
  }

  /**
   * Switch to a specific tab (auto-detects which pane and focuses it).
   */
  function setActive(tabId: string) {
    splitState.update((s) => {
      const pane = getPaneForTab(s, tabId);
      if (!pane) return s;
      return {
        ...updatePane(s, pane.paneId, (p) => ({ ...p, activeTabId: tabId })),
        focusedPaneId: pane.paneId,
      };
    });
  }

  /**
   * Set active tab within a specific pane and focus that pane.
   */
  function setActiveInPane(paneId: string, tabId: string) {
    splitState.update((s) => ({
      ...updatePane(s, paneId, (p) => ({ ...p, activeTabId: tabId })),
      focusedPaneId: paneId,
    }));
  }

  /**
   * Save viewport position/zoom for a specific tab.
   */
  function saveViewport(tabId: string, vp: SavedViewport) {
    splitState.update((s) => {
      const pane = getPaneForTab(s, tabId);
      if (!pane) return s;
      return updatePane(s, pane.paneId, (p) => ({
        ...p,
        tabs: p.tabs.map((tab) => (tab.tabId === tabId ? { ...tab, savedViewport: vp } : tab)),
      }));
    });
  }

  /**
   * Move a tab from one index to another within a specific pane (for drag-and-drop reordering).
   */
  function reorder(paneId: string, fromIndex: number, toIndex: number) {
    if (fromIndex === toIndex) return;
    splitState.update((s) =>
      updatePane(s, paneId, (p) => {
        const updated = [...p.tabs];
        const [moved] = updated.splice(fromIndex, 1);
        updated.splice(toIndex, 0, moved);
        return { ...p, tabs: updated };
      }),
    );
  }

  /**
   * Close all tabs except the one with the given tabId (within its pane).
   */
  function closeOtherTabs(tabId: string) {
    splitState.update((s) => {
      const pane = getPaneForTab(s, tabId);
      if (!pane) return s;
      return updatePane(s, pane.paneId, (p) => ({
        ...p,
        tabs: (p.tabs ?? []).filter((t) => t?.tabId === tabId),
        activeTabId: tabId,
      }));
    });
  }

  /**
   * Close all tabs to the right of the given tabId (within its pane).
   */
  function closeTabsToRight(tabId: string) {
    splitState.update((s) => {
      const pane = getPaneForTab(s, tabId);
      if (!pane) return s;
      const tabs = pane.tabs ?? [];
      const idx = tabs.findIndex((t) => t?.tabId === tabId);
      if (idx === -1) return s;
      const newTabs = tabs.slice(0, idx + 1);
      let newActiveTabId = pane.activeTabId;
      if (newActiveTabId && !newTabs.find((t) => t?.tabId === newActiveTabId)) {
        newActiveTabId = tabId;
      }
      return updatePane(s, pane.paneId, () => ({
        ...pane,
        tabs: newTabs,
        activeTabId: newActiveTabId,
      }));
    });
  }

  /**
   * Close all tabs (in all panes), collapsing to a single empty pane.
   */
  function closeAllTabs() {
    const paneId = generatePaneId();
    splitState.set({
      panes: [{ paneId, tabs: [], activeTabId: null }],
      focusedPaneId: paneId,
      splitRatio: 0.5,
    });
  }

  /**
   * Close all tabs in a specific pane.
   * If other panes exist, the empty pane is removed.
   */
  function closeAllTabsInPane(paneId: string) {
    splitState.update((s) => {
      if (s.panes.length > 1) {
        const newPanes = s.panes.filter((p) => p.paneId !== paneId);
        let focusedPaneId = s.focusedPaneId;
        if (focusedPaneId === paneId) {
          focusedPaneId = newPanes[0].paneId;
        }
        return { ...s, panes: newPanes, focusedPaneId };
      } else {
        return updatePane(s, paneId, (p) => ({ ...p, tabs: [], activeTabId: null }));
      }
    });
  }

  // ---- Split operations ----

  /**
   * Split the given tab to the right, creating a new pane.
   * The tab is moved from its current pane into a new pane on the right.
   */
  function splitRight(tabId: string) {
    splitState.update((s) => {
      // Already at max panes (2)
      if (s.panes.length >= 2) {
        // Move the tab to the other pane instead
        const srcPane = getPaneForTab(s, tabId);
        if (!srcPane) return s;
        const tab = srcPane.tabs?.find((t) => t?.tabId === tabId);
        if (!tab) return s;

        const dstPane = s.panes.find((p) => p?.paneId !== srcPane.paneId);
        if (!dstPane) return s;

        // Remove from source
        const srcTabs = (srcPane.tabs ?? []).filter((t) => t?.tabId !== tabId);

        // If source pane would be empty, duplicate the tab instead of moving it
        if (srcTabs.length === 0) {
          const duplicateTabId = generateTabId();
          const duplicateTab: Tab = { ...tab, tabId: duplicateTabId, savedViewport: tab.savedViewport ? { ...tab.savedViewport } : null };
          return {
            ...s,
            panes: s.panes.map((p) => {
              if (p.paneId === dstPane.paneId) {
                return { ...p, tabs: [...p.tabs, duplicateTab], activeTabId: duplicateTabId };
              }
              return p;
            }),
            focusedPaneId: dstPane.paneId,
          };
        }

        let srcActive = srcPane.activeTabId;
        if (srcActive === tabId) {
          const idx = srcPane.tabs.findIndex((t) => t.tabId === tabId);
          srcActive =
            srcTabs.length > 0
              ? srcTabs[Math.min(idx, srcTabs.length - 1)].tabId
              : null;
        }

        return {
          ...s,
          panes: s.panes.map((p) => {
            if (p.paneId === srcPane.paneId) {
              return { ...p, tabs: srcTabs, activeTabId: srcActive };
            }
            if (p.paneId === dstPane.paneId) {
              return { ...p, tabs: [...p.tabs, tab], activeTabId: tabId };
            }
            return p;
          }),
          focusedPaneId: dstPane.paneId,
        };
      }

      // Create a new pane to the right
      const srcPane = getPaneForTab(s, tabId);
      if (!srcPane) return s;

      const tab = srcPane.tabs.find((t) => t.tabId === tabId);
      if (!tab) return s;

      const newPaneId = generatePaneId();

      // Remove tab from source pane
      const srcTabs = srcPane.tabs.filter((t) => t.tabId !== tabId);
      let srcActive = srcPane.activeTabId;
      if (srcActive === tabId) {
        const idx = srcPane.tabs.findIndex((t) => t.tabId === tabId);
        srcActive =
          srcTabs.length > 0
            ? srcTabs[Math.min(idx, srcTabs.length - 1)].tabId
            : null;
      }

      // If source pane would be empty, duplicate the tab instead of moving it
      if (srcTabs.length === 0) {
        const duplicateTabId = generateTabId();
        const duplicateTab: Tab = { ...tab, tabId: duplicateTabId, savedViewport: tab.savedViewport ? { ...tab.savedViewport } : null };
        return {
          ...s,
          panes: [
            { ...srcPane, tabs: [tab], activeTabId: tabId },
            { paneId: newPaneId, tabs: [duplicateTab], activeTabId: duplicateTabId },
          ],
          focusedPaneId: newPaneId,
          splitRatio: 0.5,
        };
      }

      return {
        ...s,
        panes: [
          { ...srcPane, tabs: srcTabs, activeTabId: srcActive },
          { paneId: newPaneId, tabs: [tab], activeTabId: tabId },
        ],
        focusedPaneId: newPaneId,
        splitRatio: 0.5,
      };
    });
  }

  /**
   * Move a tab from one pane to another (for cross-pane drag-and-drop).
   */
  function moveTabToPane(tabId: string, targetPaneId: string, targetIndex?: number) {
    splitState.update((s) => {
      const srcPane = getPaneForTab(s, tabId);
      if (!srcPane || srcPane.paneId === targetPaneId) {
        // Same pane — just reorder if index given
        if (srcPane && targetIndex !== undefined) {
          const fromIdx = srcPane.tabs.findIndex((t) => t.tabId === tabId);
          if (fromIdx !== -1 && fromIdx !== targetIndex) {
            return updatePane(s, srcPane.paneId, (p) => {
              const updated = [...p.tabs];
              const [moved] = updated.splice(fromIdx, 1);
              updated.splice(targetIndex, 0, moved);
              return { ...p, tabs: updated };
            });
          }
        }
        return s;
      }

      const tab = srcPane.tabs.find((t) => t.tabId === tabId);
      if (!tab) return s;

      // Remove from source
      const srcTabs = srcPane.tabs.filter((t) => t.tabId !== tabId);
      let srcActive = srcPane.activeTabId;
      if (srcActive === tabId) {
        const idx = srcPane.tabs.findIndex((t) => t.tabId === tabId);
        srcActive =
          srcTabs.length > 0
            ? srcTabs[Math.min(idx, srcTabs.length - 1)].tabId
            : null;
      }

      let newState: SplitState = {
        ...s,
        panes: s.panes.map((p) => {
          if (p.paneId === srcPane.paneId) {
            return { ...p, tabs: srcTabs, activeTabId: srcActive };
          }
          if (p.paneId === targetPaneId) {
            const newTabs = [...p.tabs];
            if (targetIndex !== undefined) {
              newTabs.splice(targetIndex, 0, tab);
            } else {
              newTabs.push(tab);
            }
            return { ...p, tabs: newTabs, activeTabId: tabId };
          }
          return p;
        }),
        focusedPaneId: targetPaneId,
      };

      // If source pane is now empty, remove it
      if (srcTabs.length === 0 && newState.panes.length > 1) {
        newState = {
          ...newState,
          panes: newState.panes.filter((p) => p.paneId !== srcPane.paneId),
          focusedPaneId: targetPaneId,
        };
      }

      return newState;
    });
  }

  /**
   * Update the split ratio (divider position).
   */
  function setSplitRatio(ratio: number) {
    splitState.update((s) => ({ ...s, splitRatio: Math.max(0.15, Math.min(0.85, ratio)) }));
  }

  /**
   * Restore session state from a URL-decoded session object.
   * This is used when loading from ?v= URL parameter.
   */
  function restoreFromSession(session: {
    p: Array<{
      t: Array<{
        s: string;
        l?: string;
        w: number;
        h: number;
        v?: [number, number, number];
      }>;
      a: number;
    }>;
    f: number;
    r: number;
  }) {
    // Build panes from session state
    const panes: Pane[] = session.p.map((paneState) => {
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
        activeTabId:
          tabs.length > paneState.a
            ? tabs[paneState.a]?.tabId
            : tabs[0]?.tabId || null,
      };
    });

    // Ensure focusedPaneId is valid
    const focusedPaneIndex = Math.max(0, Math.min(session.f, panes.length - 1));

    splitState.set({
      panes,
      focusedPaneId: panes[focusedPaneIndex]?.paneId || panes[0]?.paneId || '',
      splitRatio: session.r ?? 0.5,
    });
  }

  return {
    // Core state
    splitState,

    // Derived (backwards-compatible)
    tabs,
    activeTabId,
    activeTab,
    isSplit,

    // Tab operations
    open,
    openInNewTab,
    openInNewTabInPane,
    closeTab,
    closeOtherTabs,
    closeTabsToRight,
    closeAllTabs,
    closeAllTabsInPane,
    setActive,
    setActiveInPane,
    saveViewport,
    reorder,

    // Pane operations
    setFocusedPane,
    splitRight,
    moveTabToPane,
    setSplitRatio,
    restoreFromSession,
  };
}

export const tabStore = createTabStore();
