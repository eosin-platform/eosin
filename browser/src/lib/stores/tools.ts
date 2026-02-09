/**
 * Tool state store for sharing tool state between AppHeader and ViewerPane.
 * 
 * The focused ViewerPane publishes its current tool state to this store,
 * and AppHeader can read it to display active tools.
 * AppHeader can also dispatch tool commands through this store.
 */

import { writable, derived } from 'svelte/store';

// Tool types that can be active
export type AnnotationTool = 'point' | 'ellipse' | 'polygon' | 'lasso' | 'mask' | null;
export type MeasurementMode = 'drag' | 'pending' | 'placing' | 'toggle' | 'confirmed' | null;
export type RoiMode = 'pending' | 'placing' | 'toggle' | 'confirmed' | null;

export interface ToolState {
  /** Currently active annotation tool */
  annotationTool: AnnotationTool;
  /** Whether measurement mode is active */
  measurementActive: boolean;
  measurementMode: MeasurementMode;
  /** Whether ROI (Region of Interest) mode is active */
  roiActive: boolean;
  roiMode: RoiMode;
  /** Whether undo is available */
  canUndo: boolean;
  /** Whether redo is available */
  canRedo: boolean;
}

export const defaultToolState: ToolState = {
  annotationTool: null,
  measurementActive: false,
  measurementMode: null,
  roiActive: false,
  roiMode: null,
  canUndo: false,
  canRedo: false,
};

// Current tool state from the focused pane
export const toolState = writable<ToolState>({ ...defaultToolState });

// Tool commands that ViewerPane listens to
export type ToolCommand = 
  | { type: 'undo' }
  | { type: 'redo' }
  | { type: 'measure' }
  | { type: 'roi' }
  | { type: 'annotation'; tool: AnnotationTool }
  | null;

// Command dispatch - AppHeader writes, ViewerPane reads and clears
export const toolCommand = writable<ToolCommand>(null);

// Helper to dispatch a command
export function dispatchToolCommand(cmd: ToolCommand) {
  toolCommand.set(cmd);
}

// Helper to clear the command after handling
export function clearToolCommand() {
  toolCommand.set(null);
}

// Helper to update tool state
export function updateToolState(state: Partial<ToolState>) {
  toolState.update(s => ({ ...s, ...state }));
}

// Reset tool state (called when pane loses focus)
export function resetToolState() {
  toolState.set({ ...defaultToolState });
}
