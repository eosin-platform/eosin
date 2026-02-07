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
    /** Whether "Close Others" should be disabled */
    disableCloseOthers: boolean;
    /** Whether "Close Others to the Right" should be disabled */
    disableCloseRight: boolean;
    /** Whether "Split Right" should be disabled (e.g. only one tab and no split yet) */
    disableSplitRight: boolean;
    /** Callback for "Split Right" action */
    onSplitRight: () => void;
    /** Callback for "Copy Permalink" action */
    onCopyPermalink: () => void;
    /** Callback for "Close Tab" action */
    onCloseTab: () => void;
    /** Callback for "Close Others" action */
    onCloseOthers: () => void;
    /** Callback for "Close Others to the Right" action */
    onCloseRight: () => void;
    /** Callback for "Close All" action */
    onCloseAll: () => void;
    /** Callback to dismiss the menu */
    onClose: () => void;
  }

  let {
    x,
    y,
    visible,
    disableCloseOthers,
    disableCloseRight,
    disableSplitRight,
    onSplitRight,
    onCopyPermalink,
    onCloseTab,
    onCloseOthers,
    onCloseRight,
    onCloseAll,
    onClose,
  }: Props = $props();

  let menuEl = $state<HTMLDivElement>();

  function handleSplitRight() {
    if (disableSplitRight) return;
    onSplitRight();
    onClose();
  }

  function handleCopyPermalink() {
    onCopyPermalink();
    onClose();
  }

  function handleCloseTab() {
    onCloseTab();
    onClose();
  }

  function handleCloseOthers() {
    if (disableCloseOthers) return;
    onCloseOthers();
    onClose();
  }

  function handleCloseRight() {
    if (disableCloseRight) return;
    onCloseRight();
    onClose();
  }

  function handleCloseAll() {
    onCloseAll();
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

  let adjustedX = $derived(Math.min(x, (browser ? window.innerWidth : 9999) - 220));
  let adjustedY = $derived(Math.min(y, (browser ? window.innerHeight : 9999) - 160));
</script>

{#if visible}
  <div
    class="context-menu"
    bind:this={menuEl}
    style="left: {adjustedX}px; top: {adjustedY}px;"
    role="menu"
  >
    <button
      class="context-menu-item"
      class:disabled={disableSplitRight}
      role="menuitem"
      aria-disabled={disableSplitRight}
      onclick={handleSplitRight}
    >
      Split Right
    </button>
    <button class="context-menu-item" role="menuitem" onclick={handleCopyPermalink}>
      Copy Permalink
    </button>
    <div class="separator"></div>
    <button class="context-menu-item" role="menuitem" onclick={handleCloseTab}>
      Close Tab
    </button>
    <button
      class="context-menu-item"
      class:disabled={disableCloseOthers}
      role="menuitem"
      aria-disabled={disableCloseOthers}
      onclick={handleCloseOthers}
    >
      Close Others
    </button>
    <button
      class="context-menu-item"
      class:disabled={disableCloseRight}
      role="menuitem"
      aria-disabled={disableCloseRight}
      onclick={handleCloseRight}
    >
      Close Others to the Right
    </button>
    <div class="separator"></div>
    <button class="context-menu-item" role="menuitem" onclick={handleCloseAll}>
      Close All
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
    min-width: 200px;
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
    color: #555;
    cursor: default;
  }

  .context-menu-item:first-child {
    border-radius: 5px 5px 0 0;
  }

  .context-menu-item:last-child {
    border-radius: 0 0 5px 5px;
  }

  .separator {
    height: 1px;
    background: #444;
    margin: 4px 0;
  }

  /* Touch device adaptations - larger touch targets */
  @media (pointer: coarse) {
    .context-menu-item {
      padding: 0.875rem 1rem;
      font-size: 1rem;
      min-height: 48px;
    }

    .separator {
      margin: 6px 0;
    }
  }
</style>
