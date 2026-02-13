/**
 * Global toast notification store.
 * 
 * Provides a centralized way to show toast notifications from anywhere in the app.
 */

import { writable, get } from 'svelte/store';

export type ToastType = 'error' | 'success' | 'warning' | 'info';

export interface Toast {
  message: string;
  type: ToastType;
  duration: number;
  id: number;
}

interface ToastState {
  current: Toast | null;
  timeoutId: ReturnType<typeof setTimeout> | null;
}

function createToastStore() {
  const { subscribe, set, update } = writable<ToastState>({
    current: null,
    timeoutId: null,
  });

  let idCounter = 0;

  return {
    subscribe,
    
    /**
     * Show a toast notification.
     * @param message - The message to display
     * @param duration - How long to show the toast (default: 5000ms)
     * @param type - The type of toast (default: 'error')
     */
    show(message: string, duration = 5000, type: ToastType = 'error') {
      const state = get({ subscribe });
      
      // Clear any existing timeout
      if (state.timeoutId) {
        clearTimeout(state.timeoutId);
      }

      const id = ++idCounter;
      const toast: Toast = { message, type, duration, id };

      const timeoutId = setTimeout(() => {
        update(s => {
          // Only clear if it's still the same toast
          if (s.current?.id === id) {
            return { current: null, timeoutId: null };
          }
          return s;
        });
      }, duration);

      set({ current: toast, timeoutId });
    },

    /**
     * Dismiss the current toast immediately.
     */
    dismiss() {
      update(state => {
        if (state.timeoutId) {
          clearTimeout(state.timeoutId);
        }
        return { current: null, timeoutId: null };
      });
    },

    /**
     * Convenience method for error toasts.
     */
    error(message: string, duration = 5000) {
      this.show(message, duration, 'error');
    },

    /**
     * Convenience method for success toasts.
     */
    success(message: string, duration = 3000) {
      this.show(message, duration, 'success');
    },

    /**
     * Convenience method for warning toasts.
     */
    warning(message: string, duration = 5000) {
      this.show(message, duration, 'warning');
    },

    /**
     * Convenience method for info toasts.
     */
    info(message: string, duration = 4000) {
      this.show(message, duration, 'info');
    },
  };
}

export const toastStore = createToastStore();
