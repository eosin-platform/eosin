<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import { authStore } from '$lib/stores/auth';
  import {
    annotationStore,
    annotationSets,
    activeAnnotationSet,
    activeAnnotations,
    isAnnotationLoading,
    annotationError,
    getLayerColor,
  } from '$lib/stores/annotations';
  import { getAnnotationLabel, type AnnotationKind } from '$lib/api/annotations';

  interface Props {
    /** Current slide ID */
    slideId: string | null;
    /** Whether the panel is collapsed */
    collapsed?: boolean;
    /** Callback when an annotation is clicked (to center viewport) */
    onAnnotationClick?: (annotationId: string, x: number, y: number) => void;
    /** Whether to show the new layer dialog (controlled externally) */
    showNewLayerDialogProp?: boolean;
    /** Callback when new layer dialog should close */
    onNewLayerDialogClose?: () => void;
  }

  let { slideId, collapsed = false, onAnnotationClick, showNewLayerDialogProp = false, onNewLayerDialogClose }: Props = $props();

  // Auth state
  let isLoggedIn = $state(false);
  const unsubAuth = authStore.subscribe((state) => {
    isLoggedIn = state.user !== null;
  });

  onDestroy(() => {
    unsubAuth();
  });

  // Store state
  let sets = $state<typeof $annotationSets>([]);
  let activeSet = $state<typeof $activeAnnotationSet>(null);
  let annotations = $state<typeof $activeAnnotations>([]);
  let loading = $state(false);
  let error = $state<string | null>(null);

  const unsubSets = annotationSets.subscribe((v) => (sets = v));
  const unsubActiveSet = activeAnnotationSet.subscribe((v) => (activeSet = v));
  const unsubAnnotations = activeAnnotations.subscribe((v) => (annotations = v));
  const unsubLoading = isAnnotationLoading.subscribe((v) => (loading = v));
  const unsubError = annotationError.subscribe((v) => (error = v));

  onDestroy(() => {
    unsubSets();
    unsubActiveSet();
    unsubAnnotations();
    unsubLoading();
    unsubError();
  });

  // Load annotations when slide changes
  $effect(() => {
    if (slideId) {
      annotationStore.loadForSlide(slideId);
    }
  });

  // Layer visibility state (local for reactive UI)
  let layerVisibility = $state<Map<string, boolean>>(new Map());
  const unsubStore = annotationStore.subscribe((state) => {
    layerVisibility = state.layerVisibility;
  });
  onDestroy(() => unsubStore());

  // UI state
  let showNewLayerDialog = $state(false);
  let newLayerName = $state('');
  let newLayerTaskType = $state('other');
  let editingLayerId = $state<string | null>(null);
  let editingLayerName = $state('');

  // Edit layer dialog state
  let showEditLayerDialog = $state(false);
  let editLayerTarget = $state<typeof sets[0] | null>(null);
  let editLayerName = $state('');
  let editLayerTaskType = $state('other');

  // Layer context menu state
  let layerContextMenu = $state<{ visible: boolean; x: number; y: number; target: typeof sets[0] | null }>({
    visible: false,
    x: 0,
    y: 0,
    target: null,
  });

  // Sync dialog state with prop
  $effect(() => {
    if (showNewLayerDialogProp) {
      showNewLayerDialog = true;
      newLayerName = '';
      newLayerTaskType = 'other';
    }
  });

  // New layer dialog
  function openNewLayerDialog() {
    newLayerName = '';
    newLayerTaskType = 'other';
    showNewLayerDialog = true;
  }

  function closeNewLayerDialog() {
    showNewLayerDialog = false;
    if (onNewLayerDialogClose) {
      onNewLayerDialogClose();
    }
  }

  async function handleCreateLayer() {
    if (!newLayerName.trim()) return;
    try {
      await annotationStore.createSet({
        name: newLayerName.trim(),
        task_type: newLayerTaskType,
      });
      closeNewLayerDialog();
    } catch (err) {
      console.error('Failed to create layer:', err);
    }
  }

  // Layer actions
  function handleSelectLayer(setId: string) {
    annotationStore.setActiveSet(setId);
  }

  function handleToggleVisibility(e: MouseEvent, setId: string) {
    e.stopPropagation();
    annotationStore.toggleLayerVisibility(setId);
  }

  function startEditingLayer(e: MouseEvent, set: typeof sets[0]) {
    e.stopPropagation();
    editingLayerId = set.id;
    editingLayerName = set.name;
  }

  async function finishEditingLayer() {
    if (editingLayerId && editingLayerName.trim()) {
      try {
        await annotationStore.updateSet(editingLayerId, { name: editingLayerName.trim() });
      } catch (err) {
        console.error('Failed to rename layer:', err);
      }
    }
    editingLayerId = null;
    editingLayerName = '';
  }

  function cancelEditingLayer() {
    editingLayerId = null;
    editingLayerName = '';
  }

  // Layer context menu handlers
  function handleLayerContextMenu(e: MouseEvent, set: typeof sets[0]) {
    e.preventDefault();
    e.stopPropagation();
    layerContextMenu = {
      visible: true,
      x: e.clientX,
      y: e.clientY,
      target: set,
    };
  }

  function closeLayerContextMenu() {
    layerContextMenu = { visible: false, x: 0, y: 0, target: null };
  }

  // Edit layer dialog handlers
  function openEditLayerDialog(set: typeof sets[0]) {
    editLayerTarget = set;
    editLayerName = set.name;
    editLayerTaskType = set.task_type ?? 'other';
    showEditLayerDialog = true;
    closeLayerContextMenu();
  }

  function closeEditLayerDialog() {
    showEditLayerDialog = false;
    editLayerTarget = null;
    editLayerName = '';
    editLayerTaskType = 'other';
  }

  async function handleSaveEditLayer() {
    if (!editLayerTarget || !editLayerName.trim()) return;
    try {
      await annotationStore.updateSet(editLayerTarget.id, {
        name: editLayerName.trim(),
        task_type: editLayerTaskType,
      });
      closeEditLayerDialog();
    } catch (err) {
      console.error('Failed to update layer:', err);
    }
  }

  function handleContextMenuDelete() {
    if (!layerContextMenu.target) return;
    const setId = layerContextMenu.target.id;
    closeLayerContextMenu();
    if (confirm('Delete this annotation layer? This will delete all annotations in it.')) {
      annotationStore.deleteSet(setId).catch((err) => {
        console.error('Failed to delete layer:', err);
      });
    }
  }

  async function handleDeleteLayer(e: MouseEvent, setId: string) {
    e.stopPropagation();
    if (confirm('Delete this annotation layer? This will delete all annotations in it.')) {
      try {
        await annotationStore.deleteSet(setId);
      } catch (err) {
        console.error('Failed to delete layer:', err);
      }
    }
  }

  async function handleToggleLock(e: MouseEvent, set: typeof sets[0]) {
    e.stopPropagation();
    try {
      await annotationStore.updateSet(set.id, { locked: !set.locked });
    } catch (err) {
      console.error('Failed to toggle lock:', err);
    }
  }

  // Annotation actions
  function handleAnnotationClick(annotation: typeof annotations[0]) {
    if (onAnnotationClick) {
      const geo = annotation.geometry as any;
      let x = 0, y = 0;
      if (annotation.kind === 'point') {
        x = geo.x_level0;
        y = geo.y_level0;
      } else if (annotation.kind === 'ellipse') {
        x = geo.cx_level0;
        y = geo.cy_level0;
      } else if (annotation.kind === 'polygon' || annotation.kind === 'polyline') {
        if (geo.path?.length > 0) {
          x = geo.path.reduce((s: number, p: [number, number]) => s + p[0], 0) / geo.path.length;
          y = geo.path.reduce((s: number, p: [number, number]) => s + p[1], 0) / geo.path.length;
        }
      } else if (annotation.kind === 'mask_patch') {
        x = geo.x0_level0 + geo.width / 2;
        y = geo.y0_level0 + geo.height / 2;
      }
      onAnnotationClick(annotation.id, x, y);
    }
  }

  function handleAnnotationHover(annotationId: string | null) {
    annotationStore.setHighlightedAnnotation(annotationId);
  }

  async function handleDeleteAnnotation(e: MouseEvent, annotationId: string) {
    e.stopPropagation();
    if (confirm('Delete this annotation?')) {
      try {
        await annotationStore.deleteAnnotation(annotationId);
      } catch (err) {
        console.error('Failed to delete annotation:', err);
      }
    }
  }

  // Get annotation kind icon
  function getKindIcon(kind: AnnotationKind): string {
    switch (kind) {
      case 'point': return '●';
      case 'ellipse': return '○';
      case 'polygon': return '⬡';
      case 'polyline': return '⎯';
      case 'mask_patch': return '▦';
      default: return '◇';
    }
  }

  // Task type options
  const taskTypes = [
    { value: 'classification', label: 'Classification' },
    { value: 'segmentation', label: 'Segmentation' },
    { value: 'detection', label: 'Detection' },
    { value: 'qa', label: 'QA' },
    { value: 'measurement', label: 'Measurement' },
    { value: 'other', label: 'Other' },
  ];
</script>

<div class="annotations-panel" class:collapsed>
  {#if collapsed}
    <div class="collapsed-label">
      <svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
        <polygon points="12 2 22 8.5 22 15.5 12 22 2 15.5 2 8.5 12 2"></polygon>
        <line x1="12" y1="22" x2="12" y2="15.5"></line>
        <polyline points="22 8.5 12 15.5 2 8.5"></polyline>
      </svg>
    </div>
  {:else}
    <!-- Layers list (no header - parent component provides header) -->
    <div class="section">
      <div class="layer-list">
        {#if loading}
          <div class="loading-state">
            <div class="spinner"></div>
            <span>Loading...</span>
          </div>
        {:else if error}
          <div class="error-state">{error}</div>
        {:else if sets.length === 0}
          <div class="empty-state">No annotation layers</div>
        {:else}
          {#each sets as set (set.id)}
            <div 
              class="layer-item"
              class:active={activeSet?.id === set.id}
              onclick={() => handleSelectLayer(set.id)}
              oncontextmenu={(e) => handleLayerContextMenu(e, set)}
              onkeydown={(e) => e.key === 'Enter' && handleSelectLayer(set.id)}
              role="button"
              tabindex="0"
            >
              <!-- Visibility toggle -->
              <button 
                class="visibility-btn"
                class:hidden={!layerVisibility.get(set.id)}
                onclick={(e) => handleToggleVisibility(e, set.id)}
                title={layerVisibility.get(set.id) ? 'Hide layer' : 'Show layer'}
              >
                {#if layerVisibility.get(set.id)}
                  <svg xmlns="http://www.w3.org/2000/svg" width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                    <path d="M1 12s4-8 11-8 11 8 11 8-4 8-11 8-11-8-11-8z"></path>
                    <circle cx="12" cy="12" r="3"></circle>
                  </svg>
                {:else}
                  <svg xmlns="http://www.w3.org/2000/svg" width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                    <path d="M17.94 17.94A10.07 10.07 0 0 1 12 20c-7 0-11-8-11-8a18.45 18.45 0 0 1 5.06-5.94M9.9 4.24A9.12 9.12 0 0 1 12 4c7 0 11 8 11 8a18.5 18.5 0 0 1-2.16 3.19m-6.72-1.07a3 3 0 1 1-4.24-4.24"></path>
                    <line x1="1" y1="1" x2="23" y2="23"></line>
                  </svg>
                {/if}
              </button>

              <!-- Layer name -->
              {#if editingLayerId === set.id}
                <input 
                  type="text"
                  class="layer-name-input"
                  bind:value={editingLayerName}
                  onblur={finishEditingLayer}
                  onkeydown={(e) => {
                    if (e.key === 'Enter') finishEditingLayer();
                    if (e.key === 'Escape') cancelEditingLayer();
                  }}
                  onclick={(e) => e.stopPropagation()}
                />
              {:else}
                <span class="layer-name" style="color: {getLayerColor(set.id)}">{set.name}</span>
              {/if}

              <!-- Lock indicator -->
              {#if set.locked}
                <span class="lock-icon" title="Locked">
                  <svg xmlns="http://www.w3.org/2000/svg" width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                    <rect x="3" y="11" width="18" height="11" rx="2" ry="2"></rect>
                    <path d="M7 11V7a5 5 0 0 1 10 0v4"></path>
                  </svg>
                </span>
              {/if}

              <!-- Layer actions (only for logged in users) -->
              {#if isLoggedIn && !set.locked}
                <div class="layer-actions">
                  <button 
                    class="action-btn" 
                    onclick={(e) => startEditingLayer(e, set)}
                    title="Rename"
                  >
                    <svg xmlns="http://www.w3.org/2000/svg" width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                      <path d="M11 4H4a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2v-7"></path>
                      <path d="M18.5 2.5a2.121 2.121 0 0 1 3 3L12 15l-4 1 1-4 9.5-9.5z"></path>
                    </svg>
                  </button>
                  <button 
                    class="action-btn"
                    onclick={(e) => handleToggleLock(e, set)}
                    title="Lock"
                  >
                    <svg xmlns="http://www.w3.org/2000/svg" width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                      <rect x="3" y="11" width="18" height="11" rx="2" ry="2"></rect>
                      <path d="M7 11V7a5 5 0 0 1 9.9-1"></path>
                    </svg>
                  </button>
                  <button 
                    class="action-btn delete" 
                    onclick={(e) => handleDeleteLayer(e, set.id)}
                    title="Delete"
                  >
                    <svg xmlns="http://www.w3.org/2000/svg" width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                      <polyline points="3 6 5 6 21 6"></polyline>
                      <path d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6m3 0V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2"></path>
                    </svg>
                  </button>
                </div>
              {:else if isLoggedIn && set.locked}
                <div class="layer-actions">
                  <button 
                    class="action-btn"
                    onclick={(e) => handleToggleLock(e, set)}
                    title="Unlock"
                  >
                    <svg xmlns="http://www.w3.org/2000/svg" width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                      <rect x="3" y="11" width="18" height="11" rx="2" ry="2"></rect>
                      <path d="M7 11V7a5 5 0 0 1 10 0v4"></path>
                    </svg>
                  </button>
                </div>
              {/if}
            </div>
          {/each}
        {/if}
      </div>
    </div>

    <!-- Annotations list for active layer -->
    {#if activeSet}
      <div class="section annotations-section">
        <div class="section-header">
          <span class="section-title">Annotations</span>
          <span class="count">({annotations.length})</span>
        </div>

        <div class="annotation-list">
          {#if annotations.length === 0}
            <div class="empty-state">No annotations in this layer</div>
          {:else}
            {#each annotations as annotation (annotation.id)}
              <div 
                class="annotation-item"
                onclick={() => handleAnnotationClick(annotation)}
                onmouseenter={() => handleAnnotationHover(annotation.id)}
                onmouseleave={() => handleAnnotationHover(null)}
                onkeydown={(e) => e.key === 'Enter' && handleAnnotationClick(annotation)}
                role="button"
                tabindex="0"
              >
                <span class="annotation-icon" title={annotation.kind} style="color: {activeSet ? getLayerColor(activeSet.id) : 'inherit'}">
                  {getKindIcon(annotation.kind as AnnotationKind)}
                </span>
                <span class="annotation-label">
                  {getAnnotationLabel(annotation.kind)}
                </span>
                {#if isLoggedIn && !activeSet.locked}
                  <button 
                    class="action-btn delete"
                    onclick={(e) => handleDeleteAnnotation(e, annotation.id)}
                    title="Delete"
                  >
                    <svg xmlns="http://www.w3.org/2000/svg" width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                      <line x1="18" y1="6" x2="6" y2="18"></line>
                      <line x1="6" y1="6" x2="18" y2="18"></line>
                    </svg>
                  </button>
                {/if}
              </div>
            {/each}
          {/if}
        </div>
      </div>
    {/if}
  {/if}

  <!-- New Layer Dialog -->
  {#if showNewLayerDialog}
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div class="dialog-overlay" onclick={closeNewLayerDialog} onkeydown={(e) => e.key === 'Escape' && closeNewLayerDialog()}>
      <!-- svelte-ignore a11y_click_events_have_key_events -->
      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <div class="dialog" onclick={(e) => e.stopPropagation()}>
        <h3>New Annotation Layer</h3>
        <div class="form-group">
          <label for="layer-name">Name</label>
          <input 
            id="layer-name"
            type="text" 
            bind:value={newLayerName}
            placeholder="Layer name"
          />
        </div>
        <div class="form-group">
          <label for="layer-task-type">Task Type</label>
          <select id="layer-task-type" bind:value={newLayerTaskType}>
            {#each taskTypes as tt}
              <option value={tt.value}>{tt.label}</option>
            {/each}
          </select>
        </div>
        <div class="dialog-actions">
          <button class="btn secondary" onclick={closeNewLayerDialog}>Cancel</button>
          <button class="btn primary" onclick={handleCreateLayer} disabled={!newLayerName.trim()}>
            Create
          </button>
        </div>
      </div>
    </div>
  {/if}

  <!-- Edit Layer Dialog -->
  {#if showEditLayerDialog && editLayerTarget}
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div class="dialog-overlay" onclick={closeEditLayerDialog} onkeydown={(e) => e.key === 'Escape' && closeEditLayerDialog()}>
      <!-- svelte-ignore a11y_click_events_have_key_events -->
      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <div class="dialog" onclick={(e) => e.stopPropagation()}>
        <h3>Edit Annotation Layer</h3>
        <div class="form-group">
          <label for="edit-layer-name">Name</label>
          <input 
            id="edit-layer-name"
            type="text" 
            bind:value={editLayerName}
            placeholder="Layer name"
          />
        </div>
        <div class="form-group">
          <label for="edit-layer-task-type">Task Type</label>
          <select id="edit-layer-task-type" bind:value={editLayerTaskType}>
            {#each taskTypes as tt}
              <option value={tt.value}>{tt.label}</option>
            {/each}
          </select>
        </div>
        <div class="dialog-actions">
          <button class="btn secondary" onclick={closeEditLayerDialog}>Cancel</button>
          <button class="btn primary" onclick={handleSaveEditLayer} disabled={!editLayerName.trim()}>
            Save
          </button>
        </div>
      </div>
    </div>
  {/if}

  <!-- Layer Context Menu -->
  {#if layerContextMenu.visible && layerContextMenu.target}
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div class="context-menu-overlay" onclick={closeLayerContextMenu} onkeydown={(e) => e.key === 'Escape' && closeLayerContextMenu()}>
      <!-- svelte-ignore a11y_click_events_have_key_events -->
      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <div 
        class="context-menu" 
        style="left: {layerContextMenu.x}px; top: {layerContextMenu.y}px;"
        onclick={(e) => e.stopPropagation()}
      >
        <button 
          class="context-menu-item" 
          class:disabled={!isLoggedIn}
          disabled={!isLoggedIn}
          onclick={() => isLoggedIn && openEditLayerDialog(layerContextMenu.target!)}
          title={isLoggedIn ? 'Edit layer' : 'Log in to edit layers'}
        >
          <svg xmlns="http://www.w3.org/2000/svg" width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
            <path d="M11 4H4a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2v-7"></path>
            <path d="M18.5 2.5a2.121 2.121 0 0 1 3 3L12 15l-4 1 1-4 9.5-9.5z"></path>
          </svg>
          <span>Edit</span>
        </button>
        <button 
          class="context-menu-item delete" 
          class:disabled={!isLoggedIn}
          disabled={!isLoggedIn}
          onclick={() => isLoggedIn && handleContextMenuDelete()}
          title={isLoggedIn ? 'Delete layer' : 'Log in to delete layers'}
        >
          <svg xmlns="http://www.w3.org/2000/svg" width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
            <polyline points="3 6 5 6 21 6"></polyline>
            <path d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6m3 0V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2"></path>
          </svg>
          <span>Delete</span>
        </button>
      </div>
    </div>
  {/if}
</div>

<style>
  .annotations-panel {
    display: flex;
    flex-direction: column;
    height: 100%;
    background: #141414;
    overflow: hidden;
  }

  .annotations-panel.collapsed {
    align-items: center;
    justify-content: center;
  }

  .collapsed-label {
    color: #666;
    writing-mode: vertical-rl;
    text-orientation: mixed;
    display: flex;
    align-items: center;
    gap: 0.5rem;
  }

  .section {
    display: flex;
    flex-direction: column;
    min-height: 0;
  }

  .section-header {
    display: flex;
    align-items: center;
    padding: 0.5rem 0.75rem;
    border-bottom: 1px solid #333;
    background: #1a1a1a;
  }

  .section-title {
    font-size: 0.75rem;
    font-weight: 600;
    color: #888;
    text-transform: uppercase;
    letter-spacing: 0.05em;
  }

  .count {
    font-size: 0.7rem;
    color: #666;
    margin-left: 0.5rem;
  }

  .layer-list,
  .annotation-list {
    flex: 1;
    overflow-y: auto;
    padding: 0.25rem;
  }

  .layer-item,
  .annotation-item {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    padding: 0.375rem 0.5rem;
    border-radius: 4px;
    cursor: pointer;
    transition: background-color 0.1s;
  }

  .layer-item:hover,
  .annotation-item:hover {
    background: #252525;
  }

  .layer-item.active {
    background: #0066cc33;
    border-left: 2px solid #0066cc;
    padding-left: calc(0.5rem - 2px);
  }

  .visibility-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 22px;
    height: 22px;
    padding: 0;
    background: transparent;
    border: none;
    color: #888;
    cursor: pointer;
    border-radius: 4px;
    flex-shrink: 0;
  }

  .visibility-btn:hover {
    background: #333;
    color: #fff;
  }

  .visibility-btn.hidden {
    color: #555;
  }

  .layer-name {
    flex: 1;
    min-width: 0;
    font-size: 0.8125rem;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .annotation-label {
    flex: 1;
    min-width: 0;
    font-size: 0.8125rem;
    color: #ddd;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .layer-name-input {
    flex: 1;
    min-width: 0;
    font-size: 0.8125rem;
    color: #fff;
    background: #333;
    border: 1px solid #0066cc;
    border-radius: 3px;
    padding: 0.125rem 0.25rem;
    outline: none;
  }

  .lock-icon {
    color: #666;
    flex-shrink: 0;
  }

  .layer-actions {
    display: flex;
    gap: 0.125rem;
    opacity: 0;
    transition: opacity 0.1s;
  }

  .layer-item:hover .layer-actions,
  .layer-item:hover .layer-actions .action-btn,
  .annotation-item:hover .action-btn {
    opacity: 1;
  }

  .action-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 20px;
    height: 20px;
    padding: 0;
    background: transparent;
    border: none;
    color: #888;
    cursor: pointer;
    border-radius: 3px;
    flex-shrink: 0;
    opacity: 0;
    transition: background-color 0.1s, opacity 0.1s;
  }

  .action-btn:hover {
    background: #333;
    color: #fff;
  }

  .action-btn.delete:hover {
    background: #dc2626;
    color: #fff;
  }

  .annotation-icon {
    font-size: 0.875rem;
    width: 18px;
    text-align: center;
    flex-shrink: 0;
  }

  .annotations-section {
    flex: 1;
    min-height: 0;
    border-top: 1px solid #333;
  }

  .loading-state,
  .error-state,
  .empty-state {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 0.5rem;
    padding: 1rem;
    color: #666;
    font-size: 0.8125rem;
  }

  .error-state {
    color: #f87171;
  }

  .spinner {
    width: 14px;
    height: 14px;
    border: 2px solid #333;
    border-top-color: #0066cc;
    border-radius: 50%;
    animation: spin 0.8s linear infinite;
  }

  @keyframes spin {
    to { transform: rotate(360deg); }
  }

  /* Dialog */
  .dialog-overlay {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.6);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 10000;
  }

  .dialog {
    background: #222;
    border: 1px solid #444;
    border-radius: 8px;
    padding: 1.25rem;
    min-width: 280px;
    max-width: 90vw;
    box-shadow: 0 8px 32px rgba(0, 0, 0, 0.5);
  }

  .dialog h3 {
    margin: 0 0 1rem;
    font-size: 1rem;
    font-weight: 600;
    color: #eee;
  }

  .form-group {
    margin-bottom: 0.75rem;
  }

  .form-group label {
    display: block;
    font-size: 0.75rem;
    color: #888;
    margin-bottom: 0.25rem;
  }

  .form-group input,
  .form-group select {
    width: 100%;
    padding: 0.5rem;
    font-size: 0.875rem;
    color: #fff;
    background: #333;
    border: 1px solid #444;
    border-radius: 4px;
    outline: none;
    box-sizing: border-box;
  }

  .form-group input:focus,
  .form-group select:focus {
    border-color: #0066cc;
  }

  .dialog-actions {
    display: flex;
    justify-content: flex-end;
    gap: 0.5rem;
    margin-top: 1rem;
  }

  .btn {
    padding: 0.5rem 1rem;
    font-size: 0.8125rem;
    font-weight: 500;
    border: none;
    border-radius: 4px;
    cursor: pointer;
    transition: background-color 0.15s;
  }

  .btn.primary {
    background: #0066cc;
    color: #fff;
  }

  .btn.primary:hover:not(:disabled) {
    background: #0077ee;
  }

  .btn.primary:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .btn.secondary {
    background: #444;
    color: #ddd;
  }

  .btn.secondary:hover {
    background: #555;
  }

  /* Scrollbar */
  .layer-list::-webkit-scrollbar,
  .annotation-list::-webkit-scrollbar {
    width: 6px;
  }

  .layer-list::-webkit-scrollbar-track,
  .annotation-list::-webkit-scrollbar-track {
    background: transparent;
  }

  .layer-list::-webkit-scrollbar-thumb,
  .annotation-list::-webkit-scrollbar-thumb {
    background: #333;
    border-radius: 3px;
  }

  .layer-list::-webkit-scrollbar-thumb:hover,
  .annotation-list::-webkit-scrollbar-thumb:hover {
    background: #444;
  }

  /* Context menu */
  .context-menu-overlay {
    position: fixed;
    inset: 0;
    z-index: 10000;
  }

  .context-menu {
    position: fixed;
    z-index: 10001;
    background: #222;
    border: 1px solid #444;
    border-radius: 6px;
    box-shadow: 0 4px 16px rgba(0, 0, 0, 0.5);
    padding: 4px 0;
    min-width: 140px;
    animation: contextFadeIn 0.1s ease-out;
  }

  @keyframes contextFadeIn {
    from {
      opacity: 0;
      transform: scale(0.95);
    }
    to {
      opacity: 1;
      transform: scale(1);
    }
  }

  .context-menu-item {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    width: 100%;
    padding: 0.5rem 0.75rem;
    background: transparent;
    border: none;
    color: #ddd;
    font-size: 0.8125rem;
    cursor: pointer;
    text-align: left;
    transition: background-color 0.1s;
  }

  .context-menu-item:hover {
    background: #0066cc;
    color: #fff;
  }

  .context-menu-item.delete:hover {
    background: #dc2626;
    color: #fff;
  }

  .context-menu-item:first-child {
    border-radius: 5px 5px 0 0;
  }

  .context-menu-item:last-child {
    border-radius: 0 0 5px 5px;
  }

  .context-menu-item.disabled {
    color: #666;
    cursor: not-allowed;
    opacity: 0.5;
  }

  .context-menu-item.disabled:hover {
    background: transparent;
    color: #666;
  }

  /* Mobile adaptations */
  @media (max-width: 600px) {
    .layer-actions {
      opacity: 1;
    }

    .action-btn {
      opacity: 1;
      width: 28px;
      height: 28px;
    }

    .layer-item,
    .annotation-item {
      padding: 0.5rem;
    }

    .dialog {
      margin: 1rem;
      min-width: auto;
      width: calc(100% - 2rem);
    }
  }
</style>
