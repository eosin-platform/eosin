import { writable } from 'svelte/store';

/**
 * Navigation target for centering the viewport on a specific point.
 */
export interface NavigationTarget {
  /** X coordinate in image space (level 0) */
  x: number;
  /** Y coordinate in image space (level 0) */
  y: number;
  /** Optional annotation ID that triggered the navigation */
  annotationId?: string;
  /** Timestamp to trigger navigation even if coordinates are the same */
  timestamp: number;
}

/**
 * Store for triggering viewport navigation from other components.
 * ViewerPane subscribes to this and centers the viewport on the target.
 */
export const navigationTarget = writable<NavigationTarget | null>(null);

/**
 * Navigate to a specific point in image coordinates.
 */
export function navigateToPoint(x: number, y: number, annotationId?: string) {
  navigationTarget.set({
    x,
    y,
    annotationId,
    timestamp: Date.now(),
  });
}
