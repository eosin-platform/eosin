import { writable } from 'svelte/store';

export interface NewSlide {
  id: string;
  dataset?: string;
  width: number;
  height: number;
  filename: string;
  full_size: number;
  url: string;
  /** Timestamp when the event was received */
  receivedAt: number;
}

/**
 * A store that receives "slide created" events from the WebSocket.
 * The Sidebar subscribes and prepends new slides to its list in real time.
 * Each push increments a counter so subscribers can react.
 */
function createNewSlidesStore() {
  const { subscribe, update } = writable<NewSlide[]>([]);

  return {
    subscribe,
    /** Push a newly-created slide */
    push(slide: NewSlide) {
      update((list) => [slide, ...list]);
    },
    /** Clear all buffered slides (e.g. after a full refresh) */
    clear() {
      update(() => []);
    },
  };
}

export const newSlides = createNewSlidesStore();
