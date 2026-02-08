<script lang="ts">
  import { onMount } from 'svelte';
  import { settingsModalOpen, helpMenuOpen } from '$lib/stores/settings';
  import { authStore, loginModalOpen } from '$lib/stores/auth';
  import { logout } from '$lib/auth/client';
  import SettingsModal from '$lib/components/settings/SettingsModal.svelte';
  import LoginModal from '$lib/components/LoginModal.svelte';

  interface Props {
    /** Show menu button (for mobile) */
    showMenuButton?: boolean;
    /** Callback when menu button is clicked */
    onMenuClick?: () => void;
  }

  let { showMenuButton = false, onMenuClick }: Props = $props();

  // Help button pulse animation state (plays on mount for 1500ms)
  let helpButtonPulsing = $state(true);

  onMount(() => {
    // Stop the pulse animation after 1500ms
    const timer = setTimeout(() => {
      helpButtonPulsing = false;
    }, 1500);
    return () => clearTimeout(timer);
  });

  function openSettings() {
    settingsModalOpen.set(true);
  }

  function toggleHelp() {
    helpMenuOpen.update(v => !v);
  }

  function closeHelp() {
    helpMenuOpen.set(false);
  }

  function openLogin() {
    loginModalOpen.set(true);
  }

  function handleLogout() {
    logout();
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
    <span class="dim">(real-time analysis tools will go here soon)</span>
  </div>

  <div class="header-right">
    <!-- Login/Logout button -->
    {#if $authStore.user}
      <button 
        class="auth-btn logout-btn" 
        onclick={handleLogout} 
        title="Logout" 
        aria-label="Logout"
      >
        <svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 20 20" fill="currentColor" class="icon">
          <path fill-rule="evenodd" d="M3 4.25A2.25 2.25 0 015.25 2h5.5A2.25 2.25 0 0113 4.25v2a.75.75 0 01-1.5 0v-2a.75.75 0 00-.75-.75h-5.5a.75.75 0 00-.75.75v11.5c0 .414.336.75.75.75h5.5a.75.75 0 00.75-.75v-2a.75.75 0 011.5 0v2A2.25 2.25 0 0110.75 18h-5.5A2.25 2.25 0 013 15.75V4.25z" clip-rule="evenodd" />
          <path fill-rule="evenodd" d="M19 10a.75.75 0 00-.75-.75H8.704l1.048-.943a.75.75 0 10-1.004-1.114l-2.5 2.25a.75.75 0 000 1.114l2.5 2.25a.75.75 0 101.004-1.114l-1.048-.943h9.546A.75.75 0 0019 10z" clip-rule="evenodd" />
        </svg>
        <span class="auth-label">LOGOUT</span>
      </button>
    {:else}
      <button 
        class="auth-btn login-btn" 
        onclick={openLogin} 
        title="Login" 
        aria-label="Login"
      >
        <svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 20 20" fill="currentColor" class="icon">
          <path fill-rule="evenodd" d="M3 4.25A2.25 2.25 0 015.25 2h5.5A2.25 2.25 0 0113 4.25v2a.75.75 0 01-1.5 0v-2a.75.75 0 00-.75-.75h-5.5a.75.75 0 00-.75.75v11.5c0 .414.336.75.75.75h5.5a.75.75 0 00.75-.75v-2a.75.75 0 011.5 0v2A2.25 2.25 0 0110.75 18h-5.5A2.25 2.25 0 013 15.75V4.25z" clip-rule="evenodd" />
          <path fill-rule="evenodd" d="M6 10a.75.75 0 01.75-.75h9.546l-1.048-.943a.75.75 0 111.004-1.114l2.5 2.25a.75.75 0 010 1.114l-2.5 2.25a.75.75 0 11-1.004-1.114l1.048-.943H6.75A.75.75 0 016 10z" clip-rule="evenodd" />
        </svg>
        <span class="auth-label">LOGIN</span>
      </button>
    {/if}

    <!-- Help button -->
    <button 
      class="help-btn" 
      class:active={$helpMenuOpen}
      class:pulsing={helpButtonPulsing && !$helpMenuOpen}
      onclick={toggleHelp} 
      title="Help (H)" 
      aria-label="Open help"
    >
      <!-- Question mark icon -->
      <svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="currentColor" class="icon">
        <path d="M11.07 12.85c.77-1.39 2.25-2.21 3.11-3.44.91-1.29.4-3.7-2.18-3.7-1.69 0-2.52 1.28-2.87 2.34L6.54 6.96C7.25 4.83 9.18 3 11.99 3c2.35 0 3.96.95 4.87 2.17.9 1.21 1.14 2.72.72 4.13-.52 1.71-1.9 2.94-2.93 4.15-.73.86-.68 1.55-.68 2.55H11c0-1.15-.08-2.29.07-3.15zM14 20c0 1.1-.9 2-2 2s-2-.9-2-2 .9-2 2-2 2 .9 2 2z"/>
      </svg>
    </button>
    
    <!-- Settings button -->
    <button class="settings-btn" onclick={openSettings} title="Settings" aria-label="Open settings">
      <!-- Gear/Cog icon -->
      <svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 20 20" fill="currentColor" class="icon">
        <path fill-rule="evenodd" d="M7.84 1.804A1 1 0 018.82 1h2.36a1 1 0 01.98.804l.331 1.652a6.993 6.993 0 011.929 1.115l1.598-.54a1 1 0 011.186.447l1.18 2.044a1 1 0 01-.205 1.251l-1.267 1.113a7.047 7.047 0 010 2.228l1.267 1.113a1 1 0 01.206 1.25l-1.18 2.045a1 1 0 01-1.187.447l-1.598-.54a6.993 6.993 0 01-1.929 1.115l-.33 1.652a1 1 0 01-.98.804H8.82a1 1 0 01-.98-.804l-.331-1.652a6.993 6.993 0 01-1.929-1.115l-1.598.54a1 1 0 01-1.186-.447l-1.18-2.044a1 1 0 01.205-1.251l1.267-1.114a7.05 7.05 0 010-2.227L1.821 7.773a1 1 0 01-.206-1.25l1.18-2.045a1 1 0 011.187-.447l1.598.54A6.993 6.993 0 017.51 3.456l.33-1.652zM10 13a3 3 0 100-6 3 3 0 000 6z" clip-rule="evenodd" />
      </svg>
    </button>
  </div>
</header>

<!-- Global Help Menu (centered over all content) -->
{#if $helpMenuOpen}
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div class="help-overlay" onclick={closeHelp}>
    <div class="help-modal" onclick={(e) => e.stopPropagation()}>
      <div class="help-header">
        <h2>Help</h2>
        <button class="help-close" onclick={closeHelp} aria-label="Close help">
          <svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 20 20" fill="currentColor">
            <path d="M6.28 5.22a.75.75 0 00-1.06 1.06L8.94 10l-3.72 3.72a.75.75 0 101.06 1.06L10 11.06l3.72 3.72a.75.75 0 101.06-1.06L11.06 10l3.72-3.72a.75.75 0 00-1.06-1.06L10 8.94 6.28 5.22z" />
          </svg>
        </button>
      </div>
      <div class="help-grid">
        <div class="help-card">
          <h3>Viewport Navigation</h3>
          <div class="help-row"><kbd>Click + Drag</kbd><span>Pan the viewport</span></div>
          <div class="help-row"><kbd>Scroll Wheel</kbd><span>Zoom in/out at cursor</span></div>
          <div class="help-row"><kbd>Pinch</kbd><span>Zoom on touch devices</span></div>
          <div class="help-row"><kbd>Dbl-click Gamma</kbd><span>Reset to 1.0</span></div>
        </div>

        <div class="help-card">
          <h3>Keyboard Shortcuts</h3>
          <div class="help-row"><kbd>H</kbd><span>Toggle this help</span></div>
          <div class="help-row"><kbd>N</kbd><span>Cycle stain normalization</span></div>
          <div class="help-row"><kbd>E</kbd><span>Cycle stain enhancement</span></div>
          <div class="help-row"><kbd>D</kbd><span>Toggle measurement mode</span></div>
          <div class="help-row"><kbd>Esc</kbd><span>Close help / Cancel</span></div>
        </div>

        <div class="help-card">
          <h3>Measurement Tool</h3>
          <div class="help-row"><kbd>Middle Drag</kbd><span>Measure while dragging</span></div>
          <div class="help-row"><kbd>D</kbd><span>Toggle measurement mode</span></div>
          <div class="help-row"><span class="help-note">Click or pan to dismiss measurement</span></div>
        </div>

        <div class="help-card">
          <h3>Stain Normalization</h3>
          <div class="help-row"><strong>None</strong><span>Original colors</span></div>
          <div class="help-row"><strong>Macenko</strong><span>SVD-based separation</span></div>
          <div class="help-row"><strong>Vahadane</strong><span>Sparse NMF-based</span></div>
        </div>

        <div class="help-card">
          <h3>Stain Enhancement</h3>
          <div class="help-row"><strong>None</strong><span>No enhancement</span></div>
          <div class="help-row"><strong>Gram</strong><span>Gram+/Gram− bacteria</span></div>
          <div class="help-row"><strong>AFB</strong><span>Acid-Fast Bacilli</span></div>
          <div class="help-row"><strong>GMS</strong><span>Grocott Silver stain</span></div>
        </div>

        <div class="help-card citation-card">
          <h3>Data provided by <a href="https://camelyon17.grand-challenge.org/" target="_blank" rel="noopener noreferrer" class="citation-link">CAMELYON17</a></h3>
          <p class="citation-text">
            Geert Litjens, Peter Bandi, Babak Ehteshami Bejnordi, Oscar Geessink, Maschenka Balkenhol, Peter Bult, Altuna Halilovic, Meyke Hermsen, Rob van de Loo, Rob Vogels, Quirine F Manson, Nikolas Stathonikos, Alexi Baidoshvili, Paul van Diest, Carla Wauters, Marcory van Dijk, Jeroen van der Laak. 1399 H&amp;E-stained sentinel lymph node sections of breast cancer patients: the CAMELYON dataset. <em>GigaScience</em>, giy065, 2018.
          </p>
          <a href="https://doi.org/10.1093/gigascience/giy065" target="_blank" rel="noopener noreferrer" class="citation-link">
            DOI: 10.1093/gigascience/giy065
          </a>
        </div>

        <div class="help-card creator-card">
          <h3>Creator</h3>
          <p class="citation-text">
            Made by Thomas Havlik in 2026.
            <a href="https://thavlik.dev" target="_blank" rel="noopener noreferrer" class="citation-link">https://thavlik.dev</a>
          </p>
        </div>
      </div>
    </div>
  </div>
{/if}

{#if $settingsModalOpen}
  <SettingsModal />
{/if}

<LoginModal />

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

  /* Auth buttons (Login/Logout) */
  .auth-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 6px;
    height: 36px;
    padding: 0 12px;
    background: transparent;
    border: 1px solid rgba(255, 255, 255, 0.2);
    color: #9ca3af;
    cursor: pointer;
    border-radius: 0.5rem;
    font-size: 12px;
    font-weight: 600;
    letter-spacing: 0.5px;
    transition: background-color 0.15s, color 0.15s, border-color 0.15s;
  }

  .auth-btn .icon {
    width: 16px;
    height: 16px;
  }

  .auth-btn:hover {
    background: #333;
    color: #fff;
    border-color: rgba(255, 255, 255, 0.3);
  }

  .login-btn:hover {
    background: rgba(59, 130, 246, 0.2);
    border-color: #3b82f6;
    color: #60a5fa;
  }

  .logout-btn:hover {
    background: rgba(239, 68, 68, 0.2);
    border-color: #ef4444;
    color: #fca5a5;
  }

  .auth-label {
    display: block;
  }

  /* Hide label on small screens */
  @media (max-width: 500px) {
    .auth-label {
      display: none;
    }
    .auth-btn {
      padding: 0;
      width: 36px;
    }
  }

  .menu-btn,
  .settings-btn,
  .help-btn {
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
    transition: background-color 0.15s, color 0.15s, transform 0.15s, box-shadow 0.15s;
  }

  .menu-btn:hover,
  .settings-btn:hover,
  .help-btn:hover {
    background: #333;
    color: #fff;
  }

  .help-btn.active {
    background: #3b82f6;
    color: white;
  }

  /* Eye-catching pulse/breathing animation for help button on page load */
  @keyframes help-pulse {
    0% {
      transform: scale(1);
      box-shadow: 
        0 0 0 0 rgba(59, 130, 246, 0.7),
        0 0 0 0 rgba(59, 130, 246, 0.4);
    }
    50% {
      transform: scale(1.1);
      box-shadow: 
        0 0 16px 4px rgba(59, 130, 246, 0.6),
        0 0 32px 8px rgba(59, 130, 246, 0.3);
    }
    100% {
      transform: scale(1);
      box-shadow: 
        0 0 0 0 rgba(59, 130, 246, 0.7),
        0 0 0 0 rgba(59, 130, 246, 0.4);
    }
  }

  .help-btn.pulsing {
    animation: help-pulse 0.75s ease-in-out 2;
    background: linear-gradient(135deg, rgba(59, 130, 246, 0.9), rgba(99, 102, 241, 0.9));
    color: white;
  }

  .help-btn.pulsing .icon {
    filter: drop-shadow(0 0 3px rgba(255, 255, 255, 0.6));
  }

  .icon {
    width: 1.25rem;
    height: 1.25rem;
  }

  /* Help overlay - fills entire viewport */
  .help-overlay {
    position: fixed;
    inset: 0;
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 24px;
    background: rgba(0, 0, 0, 0.6);
    backdrop-filter: blur(4px);
    z-index: 1000;
  }

  /* Help modal container */
  .help-modal {
    display: flex;
    flex-direction: column;
    width: 100%;
    max-width: 900px;
    max-height: calc(100vh - 48px);
    background: rgba(20, 20, 20, 0.95);
    border: 1px solid rgba(255, 255, 255, 0.15);
    border-radius: 12px;
    overflow: hidden;
  }

  .help-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 16px 20px;
    border-bottom: 1px solid rgba(255, 255, 255, 0.1);
    flex-shrink: 0;
  }

  .help-header h2 {
    margin: 0;
    font-size: 18px;
    font-weight: 600;
    color: #fff;
  }

  .help-close {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 32px;
    height: 32px;
    background: rgba(255, 255, 255, 0.1);
    border: none;
    border-radius: 6px;
    cursor: pointer;
    color: rgba(255, 255, 255, 0.7);
    transition: background 0.15s, color 0.15s;
  }

  .help-close:hover {
    background: rgba(255, 255, 255, 0.2);
    color: #fff;
  }

  .help-close svg {
    width: 18px;
    height: 18px;
  }

  /* Responsive card grid */
  .help-grid {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(220px, 1fr));
    gap: 16px;
    padding: 20px;
    overflow-y: auto;
    flex: 1;
  }

  .help-card {
    background: rgba(255, 255, 255, 0.05);
    border: 1px solid rgba(255, 255, 255, 0.1);
    border-radius: 10px;
    padding: 16px 18px;
  }

  .help-card h3 {
    margin: 0 0 12px 0;
    font-size: 12px;
    font-weight: 600;
    color: rgba(255, 255, 255, 0.7);
    text-transform: uppercase;
    letter-spacing: 0.5px;
    border-bottom: 1px solid rgba(255, 255, 255, 0.08);
    padding-bottom: 8px;
  }

  .help-row {
    display: flex;
    align-items: baseline;
    gap: 8px;
    padding: 4px 0;
    font-size: 13px;
    color: rgba(255, 255, 255, 0.9);
    flex-wrap: wrap;
  }

  .help-row kbd {
    display: inline-block;
    padding: 2px 6px;
    background: rgba(255, 255, 255, 0.1);
    border: 1px solid rgba(255, 255, 255, 0.2);
    border-radius: 4px;
    font-family: 'SF Mono', Monaco, 'Cascadia Code', monospace;
    font-size: 11px;
    text-align: center;
    color: #fff;
    white-space: nowrap;
  }

  .help-row strong {
    min-width: 70px;
    color: #fff;
    font-weight: 600;
  }

  .help-row span {
    color: rgba(255, 255, 255, 0.6);
    line-height: 1.4;
  }

  .help-note {
    font-size: 0.75rem;
    font-style: italic;
    color: rgba(255, 255, 255, 0.5);
  }

  /* Citation card styling */
  .citation-card {
    grid-column: 1 / -1;
    background: rgba(59, 130, 246, 0.08);
    border-color: rgba(59, 130, 246, 0.25);
  }

  .creator-card {
    grid-column: 1 / -1;
  }

  .citation-text {
    margin: 0 0 12px 0;
    font-size: 12px;
    line-height: 1.6;
    color: rgba(255, 255, 255, 0.8);
  }

  .citation-text em {
    font-style: italic;
  }

  .citation-link {
    display: inline-block;
    font-size: 12px;
    color: #60a5fa;
    text-decoration: none;
    transition: color 0.15s;
  }

  .citation-link:hover {
    color: #93c5fd;
    text-decoration: underline;
  }

  /* Hide header title on very small screens if menu button is present */
  @media (max-width: 360px) {
    .header-title {
      display: none;
    }
  }

  /* Mobile: full-screen modal */
  @media (max-width: 600px) {
    .help-overlay {
      padding: 0;
    }

    .help-modal {
      max-width: 100%;
      max-height: 100%;
      height: 100%;
      border-radius: 0;
      border: none;
    }

    .help-grid {
      grid-template-columns: 1fr;
      gap: 12px;
      padding: 16px;
    }

    .help-card {
      padding: 14px 16px;
    }
  }

  .dim {
    font-size: 0.75rem;
    color: rgba(255, 255, 255, 0.35);
  }

  /* Touch device adaptations - larger touch targets */
  @media (pointer: coarse) {
    .menu-btn,
    .settings-btn,
    .help-btn {
      width: 44px;
      height: 44px;
    }

    .menu-btn .icon,
    .settings-btn .icon,
    .help-btn .icon {
      width: 1.5rem;
      height: 1.5rem;
    }
  }
</style>
