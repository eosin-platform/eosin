<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import { browser } from '$app/environment';
  import { annotationStore } from '$lib/stores/annotations';
  import type { Annotation } from '$lib/api/annotations';

  interface Props {
    /** X position in viewport pixels */
    x: number;
    /** Y position in viewport pixels */
    y: number;
    /** Whether the menu is visible */
    visible: boolean;
    /** The annotation this context menu is for */
    annotation: Annotation | null;
    /** Callback to dismiss the menu */
    onClose: () => void;
    /** Callback to start modifying the annotation */
    onModify: (annotation: Annotation) => void;
  }

  let { x, y, visible, annotation, onClose, onModify }: Props = $props();

  let menuEl = $state<HTMLDivElement>();
  let isDeleting = $state(false);

  function handleModify() {
    if (!annotation) return;
    onModify(annotation);
    onClose();
  }

  async function handleDelete() {
    if (!annotation || isDeleting) return;
    
    isDeleting = true;
    try {
      await annotationStore.deleteAnnotation(annotation.id);
    } catch (err) {
      console.error('Failed to delete annotation:', err);
    } finally {
      isDeleting = false;
    }
    onClose();
  }

  function handleClickOutside(e: MouseEvent) {
    if (menuEl && !menuEl.contains(e.target as Node)) {
      onClose();
    }
  }

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === 'Escape') {
      onClose();
    }
  }

  onMount(() => {
    if (browser) {
      // Delay to avoid the same click event closing the menu
      requestAnimationFrame(() => {
        document.addEventListener('click', handleClickOutside, true);
        document.addEventListener('keydown', handleKeydown);
      });
    }
  });

  onDestroy(() => {
    if (browser) {
      document.removeEventListener('click', handleClickOutside, true);
      document.removeEventListener('keydown', handleKeydown);
    }
  });

  // Adjust position to stay within viewport
  let adjustedX = $derived(Math.min(x, (browser ? window.innerWidth : 9999) - 180));
  let adjustedY = $derived(Math.min(y, (browser ? window.innerHeight : 9999) - 100));

  // Check if annotation kind supports modification
  let canModify = $derived(annotation?.kind === 'point' || annotation?.kind === 'ellipse');
</script>

{#if visible && annotation}
  <div
    class="context-menu"
    bind:this={menuEl}
    style="left: {adjustedX}px; top: {adjustedY}px;"
    role="menu"
  >
    <button 
      class="context-menu-item" 
      class:disabled={!canModify}
      role="menuitem" 
      onclick={handleModify}
      disabled={!canModify}
      title={!canModify ? 'Modification only supported for points and ellipses' : ''}
    >
      <svg xmlns="http://www.w3.org/2000/svg" width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
        <path d="M11 4H4a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2v-7"></path>
        <path d="M18.5 2.5a2.121 2.121 0 0 1 3 3L12 15l-4 1 1-4 9.5-9.5z"></path>
      </svg>
      <span>Modify</span>
    </button>

    <button 
      class="context-menu-item delete" 
      role="menuitem" 
      onclick={handleDelete}
      disabled={isDeleting}
    >
      <svg xmlns="http://www.w3.org/2000/svg" width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
        <polyline points="3 6 5 6 21 6"></polyline>
        <path d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6m3 0V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2"></path>
        <line x1="10" y1="11" x2="10" y2="17"></line>
        <line x1="14" y1="11" x2="14" y2="17"></line>
      </svg>
      <span>{isDeleting ? 'Deleting...' : 'Delete'}</span>
    </button>
  </div>
{/if}

<style>
  .context-menu {
    position: fixed;
    z-index: 10000;
    background: #222;
    border: 1px solid #444;
    border-radius: 6px;
    box-shadow: 0 4px 16px rgba(0, 0, 0, 0.5);
    padding: 4px 0;
    min-width: 140px;
    animation: fadeIn 0.1s ease-out;
  }

  @keyframes fadeIn {
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

  .context-menu-item:hover:not(.disabled) {
    background: #0066cc;
    color: #fff;
  }

  .context-menu-item.disabled {
    color: #666;
    cursor: not-allowed;
  }

  .context-menu-item.disabled:hover {
    background: transparent;
    color: #666;
  }

  .context-menu-item.delete:hover:not(.disabled) {
    background: #cc3333;
  }

  .context-menu-item:first-child {
    border-radius: 5px 5px 0 0;
  }

  .context-menu-item:last-child {
    border-radius: 0 0 5px 5px;
  }

  /* Touch device adaptations */
  @media (pointer: coarse) {
    .context-menu-item {
      padding: 0.875rem 1rem;
      font-size: 1rem;
      min-height: 48px;
      gap: 0.75rem;
    }

    .context-menu-item svg {
      width: 18px;
      height: 18px;
    }
  }
</style>
