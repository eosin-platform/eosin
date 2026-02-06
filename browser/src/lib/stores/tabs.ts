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

function createTabStore() {
  const tabs = writable<Tab[]>([]);
  const activeTabId = writable<string | null>(null);

  let nextId = 1;

  function generateTabId(): string {
    return `tab-${nextId++}`;
  }

  /**
   * Open a slide in the current tab (replaces active tab content),
   * or creates a new tab if none exists.
   */
  function open(slideId: string, label: string, width: number, height: number) {
    const currentTabs = get(tabs);
    const currentActive = get(activeTabId);

    // If there's an active tab, replace it
    if (currentActive) {
      tabs.update((t) =>
        t.map((tab) =>
          tab.tabId === currentActive
            ? { ...tab, slideId, label, width, height, savedViewport: null }
            : tab,
        ),
      );
    } else {
      // No tabs, create one
      const tabId = generateTabId();
      tabs.update((t) => [...t, { tabId, slideId, label, width, height, savedViewport: null }]);
      activeTabId.set(tabId);
    }
  }

  /**
   * Open a slide in a new tab.
   */
  function openInNewTab(slideId: string, label: string, width: number, height: number) {
    const tabId = generateTabId();
    tabs.update((t) => [...t, { tabId, slideId, label, width, height, savedViewport: null }]);
    activeTabId.set(tabId);
  }

  /**
   * Close a tab by its tabId.
   */
  function closeTab(tabId: string) {
    const currentTabs = get(tabs);
    const currentActive = get(activeTabId);
    const idx = currentTabs.findIndex((t) => t.tabId === tabId);
    if (idx === -1) return;

    const newTabs = currentTabs.filter((t) => t.tabId !== tabId);
    tabs.set(newTabs);

    // If we closed the active tab, activate an adjacent one
    if (currentActive === tabId) {
      if (newTabs.length === 0) {
        activeTabId.set(null);
      } else {
        // Prefer the tab to the left, or the first tab
        const newIdx = Math.min(idx, newTabs.length - 1);
        activeTabId.set(newTabs[newIdx].tabId);
      }
    }
  }

  /**
   * Switch to a specific tab.
   */
  function setActive(tabId: string) {
    activeTabId.set(tabId);
  }

  const activeTab = derived([tabs, activeTabId], ([$tabs, $activeTabId]) => {
    if (!$activeTabId) return null;
    return $tabs.find((t) => t.tabId === $activeTabId) ?? null;
  });

  /**
   * Save viewport position/zoom for a specific tab.
   */
  function saveViewport(tabId: string, vp: SavedViewport) {
    tabs.update((t) =>
      t.map((tab) =>
        tab.tabId === tabId ? { ...tab, savedViewport: vp } : tab,
      ),
    );
  }

  return {
    tabs,
    activeTabId,
    activeTab,
    open,
    openInNewTab,
    closeTab,
    setActive,
    saveViewport,
  };
}

export const tabStore = createTabStore();
