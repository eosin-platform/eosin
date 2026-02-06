<script lang="ts">
  import { tabStore, type Tab } from '$lib/stores/tabs';

  let tabs = $state<Tab[]>([]);
  let activeTabId = $state<string | null>(null);

  const unsubTabs = tabStore.tabs.subscribe((v) => (tabs = v));
  const unsubActive = tabStore.activeTabId.subscribe((v) => (activeTabId = v));

  import { onDestroy } from 'svelte';
  onDestroy(() => {
    unsubTabs();
    unsubActive();
  });

  function handleTabClick(tabId: string) {
    tabStore.setActive(tabId);
  }

  function handleCloseTab(e: MouseEvent, tabId: string) {
    e.stopPropagation();
    tabStore.closeTab(tabId);
  }

  function handleCloseKeydown(e: KeyboardEvent, tabId: string) {
    if (e.key === 'Enter' || e.key === ' ') {
      e.stopPropagation();
      tabStore.closeTab(tabId);
    }
  }
</script>

<div class="tab-bar" role="tablist">
  {#if tabs.length === 0}
    <div class="tab-bar-empty">No slides open</div>
  {:else}
    {#each tabs as tab (tab.tabId)}
      <button
        class="tab"
        class:active={tab.tabId === activeTabId}
        role="tab"
        aria-selected={tab.tabId === activeTabId}
        onclick={() => handleTabClick(tab.tabId)}
      >
        <span class="tab-label" title={tab.label}>{tab.label}</span>
        <span
          class="tab-close"
          role="button"
          tabindex="0"
          aria-label="Close tab"
          onclick={(e) => handleCloseTab(e, tab.tabId)}
          onkeydown={(e) => handleCloseKeydown(e, tab.tabId)}
        >×</span>
      </button>
    {/each}
  {/if}
</div>

<style>
  .tab-bar {
    display: flex;
    align-items: stretch;
    background: #111;
    border-bottom: 1px solid #333;
    overflow-x: auto;
    overflow-y: hidden;
    flex-shrink: 0;
    min-height: 36px;
    scrollbar-width: thin;
    scrollbar-color: #444 transparent;
  }

  .tab-bar::-webkit-scrollbar {
    height: 3px;
  }

  .tab-bar::-webkit-scrollbar-thumb {
    background: #444;
    border-radius: 2px;
  }

  .tab-bar-empty {
    display: flex;
    align-items: center;
    padding: 0 1rem;
    color: #555;
    font-size: 0.8125rem;
    white-space: nowrap;
  }

  .tab {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    padding: 0 0.375rem 0 0.875rem;
    background: transparent;
    border: none;
    border-right: 1px solid #222;
    color: #888;
    font-size: 0.8125rem;
    cursor: pointer;
    white-space: nowrap;
    max-width: 200px;
    min-width: 0;
    transition: background-color 0.1s, color 0.1s;
    position: relative;
  }

  .tab:hover {
    background: #1a1a1a;
    color: #ccc;
  }

  .tab.active {
    background: #1a1a1a;
    color: #eee;
    border-bottom: 2px solid #0066cc;
  }

  .tab-label {
    overflow: hidden;
    text-overflow: ellipsis;
    flex: 1;
    min-width: 0;
    text-align: left;
  }

  .tab-close {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 20px;
    height: 20px;
    border-radius: 4px;
    font-size: 1rem;
    line-height: 1;
    color: #666;
    flex-shrink: 0;
    transition: background-color 0.1s, color 0.1s;
  }

  .tab-close:hover {
    background: #333;
    color: #fff;
  }

  .tab.active .tab-close {
    color: #999;
  }

  .tab.active .tab-close:hover {
    color: #fff;
    background: #444;
  }
</style>
