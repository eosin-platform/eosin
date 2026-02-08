<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import { browser } from '$app/environment';

  interface Props {
    /** X position in viewport pixels */
    x: number;
    /** Y position in viewport pixels */
    y: number;
    /** Whether the menu is visible */
    visible: boolean;
    /** Callback for "Save Image As..." action */
    onSaveImage: () => void;
    /** Callback for "Copy Image" action */
    onCopyImage: () => void;
    /** Callback to dismiss the menu */
    onClose: () => void;
  }

  let { x, y, visible, onSaveImage, onCopyImage, onClose }: Props = $props();

  let menuEl = $state<HTMLDivElement>();

  function handleSaveImage() {
    onSaveImage();
    onClose();
  }

  function handleCopyImage() {
    onCopyImage();
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
  let adjustedX = $derived(Math.min(x, (browser ? window.innerWidth : 9999) - 200));
  let adjustedY = $derived(Math.min(y, (browser ? window.innerHeight : 9999) - 100));
</script>

{#if visible}
  <div
    class="context-menu"
    bind:this={menuEl}
    style="left: {adjustedX}px; top: {adjustedY}px;"
    role="menu"
  >
    <button class="context-menu-item" role="menuitem" onclick={handleSaveImage}>
      <svg xmlns="http://www.w3.org/2000/svg" width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
        <path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"></path>
        <polyline points="7 10 12 15 17 10"></polyline>
        <line x1="12" y1="15" x2="12" y2="3"></line>
      </svg>
      Save Image As...
    </button>
    <button class="context-menu-item" role="menuitem" onclick={handleCopyImage}>
      <svg xmlns="http://www.w3.org/2000/svg" width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
        <rect x="9" y="9" width="13" height="13" rx="2" ry="2"></rect>
        <path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1"></path>
      </svg>
      Copy Image
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
    min-width: 180px;
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

  .context-menu-item:hover {
    background: #0066cc;
    color: #fff;
  }

  .context-menu-item:first-child {
    border-radius: 5px 5px 0 0;
  }

  .context-menu-item:last-child {
    border-radius: 0 0 5px 5px;
  }

  /* Touch device adaptations - larger touch targets */
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
