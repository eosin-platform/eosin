import { writable } from 'svelte/store';

export interface SlideProgress {
  slideId: string;
  progressSteps: number;
  progressTotal: number;
  /** Timestamp of the last progress update, used to trigger blink animations */
  lastUpdate: number;
}

/**
 * A store that tracks live progress updates for all slides.
 * Updated by +page.svelte when WebSocket progress events arrive,
 * consumed by the Sidebar and header to display live progress + activity indicators.
 * Keyed by slide UUID string.
 */
function createProgressStore() {
  const { subscribe, update } = writable<Map<string, SlideProgress>>(new Map());

  return {
    subscribe,
    /** Set progress for a specific slide */
    set(progress: SlideProgress) {
      update((map) => {
        map.set(progress.slideId, progress);
        return map;
      });
    },
    /** Get progress for a specific slide (returns null if not found) */
    get(slideId: string, map: Map<string, SlideProgress>): SlideProgress | null {
      return map.get(slideId) ?? null;
    },
    /** Clear all progress */
    clear() {
      update((map) => {
        map.clear();
        return map;
      });
    },
  };
}

export const liveProgress = createProgressStore();
