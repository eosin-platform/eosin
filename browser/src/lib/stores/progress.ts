import { writable } from 'svelte/store';

export interface SlideProgress {
  slideId: string;
  progressSteps: number;
  progressTotal: number;
  /** Timestamp of the last progress update, used to trigger blink animations */
  lastUpdate: number;
}

/**
 * A store that tracks live progress updates for the currently viewed slide.
 * Updated by +page.svelte when WebSocket progress events arrive,
 * consumed by the Sidebar and header to display live progress + activity indicators.
 */
export const liveProgress = writable<SlideProgress | null>(null);
