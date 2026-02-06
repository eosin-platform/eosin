<script lang="ts">
  import { settingsModalOpen } from '$lib/stores/settings';
  import SettingsModal from '$lib/components/settings/SettingsModal.svelte';

  interface Props {
    /** Show menu button (for mobile) */
    showMenuButton?: boolean;
    /** Callback when menu button is clicked */
    onMenuClick?: () => void;
    /** Title to display */
    title?: string;
  }

  let { showMenuButton = false, onMenuClick, title = 'Histion' }: Props = $props();

  function openSettings() {
    settingsModalOpen.set(true);
  }
</script>

<header class="app-header">
  <div class="header-left">
    {#if showMenuButton}
      <button class="menu-btn" onclick={onMenuClick} aria-label="Open menu">
        <svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
          <line x1="3" y1="12" x2="21" y2="12"></line>
          <line x1="3" y1="6" x2="21" y2="6"></line>
          <line x1="3" y1="18" x2="21" y2="18"></line>
        </svg>
      </button>
    {/if}
    <span class="header-title">{title}</span>
  </div>

  <div class="header-right">
    <button class="settings-btn" onclick={openSettings} title="Settings" aria-label="Open settings">
      <!-- Gear/Cog icon -->
      <svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 20 20" fill="currentColor" class="icon">
        <path fill-rule="evenodd" d="M7.84 1.804A1 1 0 018.82 1h2.36a1 1 0 01.98.804l.331 1.652a6.993 6.993 0 011.929 1.115l1.598-.54a1 1 0 011.186.447l1.18 2.044a1 1 0 01-.205 1.251l-1.267 1.113a7.047 7.047 0 010 2.228l1.267 1.113a1 1 0 01.206 1.25l-1.18 2.045a1 1 0 01-1.187.447l-1.598-.54a6.993 6.993 0 01-1.929 1.115l-.33 1.652a1 1 0 01-.98.804H8.82a1 1 0 01-.98-.804l-.331-1.652a6.993 6.993 0 01-1.929-1.115l-1.598.54a1 1 0 01-1.186-.447l-1.18-2.044a1 1 0 01.205-1.251l1.267-1.114a7.05 7.05 0 010-2.227L1.821 7.773a1 1 0 01-.206-1.25l1.18-2.045a1 1 0 011.187-.447l1.598.54A6.993 6.993 0 017.51 3.456l.33-1.652zM10 13a3 3 0 100-6 3 3 0 000 6z" clip-rule="evenodd" />
      </svg>
    </button>
  </div>
</header>

{#if $settingsModalOpen}
  <SettingsModal />
{/if}

<style>
  .app-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 0.5rem 1rem;
    background: #141414;
    border-bottom: 1px solid #333;
    flex-shrink: 0;
    height: 48px;
  }

  .header-left {
    display: flex;
    align-items: center;
    gap: 0.75rem;
  }

  .header-title {
    font-size: 1rem;
    font-weight: 600;
    color: #eee;
  }

  .header-right {
    display: flex;
    align-items: center;
    gap: 0.5rem;
  }

  .menu-btn,
  .settings-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 36px;
    height: 36px;
    padding: 0;
    background: transparent;
    border: none;
    color: #9ca3af;
    cursor: pointer;
    border-radius: 0.5rem;
    transition: background-color 0.15s, color 0.15s;
  }

  .menu-btn:hover,
  .settings-btn:hover {
    background: #333;
    color: #fff;
  }

  .icon {
    width: 1.25rem;
    height: 1.25rem;
  }

  /* Hide header title on very small screens if menu button is present */
  @media (max-width: 360px) {
    .header-title {
      display: none;
    }
  }
</style>
