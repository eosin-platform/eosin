<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import { browser } from '$app/environment';
  import { settingsModalOpen, helpMenuOpen, settings, type StainEnhancementMode } from '$lib/stores/settings';
  import { authStore, loginModalOpen } from '$lib/stores/auth';
  import { logout } from '$lib/auth/client';
  import { toolState, dispatchToolCommand, type ToolState } from '$lib/stores/tools';
  import { toastStore } from '$lib/stores/toast';
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
  
  // Scroll area overflow detection
  let headerLeftEl: HTMLElement | undefined = $state();
  let isScrollable = $state(false);
  
  // Tool state from focused pane
  let tools = $state<ToolState>({ annotationTool: null, measurementActive: false, measurementMode: null, roiActive: false, roiMode: null, canUndo: false, canRedo: false });
  const unsubTools = toolState.subscribe(s => tools = s);
  
  // Auth state for annotation permission
  let isLoggedIn = $state(false);
  const unsubAuth = authStore.subscribe((state) => {
    isLoggedIn = state.user !== null;
  });
  
  // Settings state for stain enhancement and annotations
  let stainEnhancement = $state<StainEnhancementMode>($settings.image.stainEnhancement);
  let annotationsVisible = $state($settings.annotations.visible);
  
  // Keep local state in sync with store
  $effect(() => {
    stainEnhancement = $settings.image.stainEnhancement;
    annotationsVisible = $settings.annotations.visible;
  });

  onMount(() => {
    // Stop the pulse animation after 1500ms
    const timer = setTimeout(() => {
      helpButtonPulsing = false;
    }, 1500);
    
    // Check if scroll area has overflow
    function checkScrollable() {
      if (headerLeftEl) {
        isScrollable = headerLeftEl.scrollWidth > headerLeftEl.clientWidth;
      }
    }
    
    // Observe resize to detect overflow changes
    let resizeObserver: ResizeObserver | undefined;
    if (headerLeftEl) {
      resizeObserver = new ResizeObserver(checkScrollable);
      resizeObserver.observe(headerLeftEl);
      checkScrollable();
    }
    
    return () => {
      clearTimeout(timer);
      unsubTools();
      unsubAuth();
      resizeObserver?.disconnect();
    };
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
  
  // Tool actions
  function handleUndo() {
    dispatchToolCommand({ type: 'undo' });
  }
  
  function handleRedo() {
    dispatchToolCommand({ type: 'redo' });
  }
  
  function handleMeasure() {
    dispatchToolCommand({ type: 'measure' });
  }
  
  function handleRoi() {
    dispatchToolCommand({ type: 'roi' });
  }
  
  function handleAnnotationTool(tool: 'point' | 'ellipse' | 'polygon' | 'lasso' | 'mask') {
    // Check if user is logged in
    if (!isLoggedIn) {
      toastStore.error('Please log in to use annotation tools');
      return;
    }
    
    // Toggle off if already active, otherwise activate
    if (tools.annotationTool === tool) {
      dispatchToolCommand({ type: 'annotation', tool: null });
    } else {
      dispatchToolCommand({ type: 'annotation', tool });
    }
  }
  
  // Stain enhancement dropdown state
  let enhancementDropdownOpen = $state(false);
  let enhancementDropdownEl = $state<HTMLDivElement>();
  let enhancementTriggerEl = $state<HTMLButtonElement>();
  let dropdownPosition = $state({ top: 0, left: 0 });
  
  function toggleEnhancementDropdown() {
    if (!enhancementDropdownOpen && enhancementTriggerEl) {
      const rect = enhancementTriggerEl.getBoundingClientRect();
      dropdownPosition = { top: rect.bottom + 4, left: rect.left };
    }
    enhancementDropdownOpen = !enhancementDropdownOpen;
  }
  
  function selectEnhancement(mode: StainEnhancementMode) {
    stainEnhancement = mode;
    settings.setSetting('image', 'stainEnhancement', mode);
    enhancementDropdownOpen = false;
  }
  
  function handleEnhancementClickOutside(e: MouseEvent) {
    if (enhancementDropdownEl && !enhancementDropdownEl.contains(e.target as Node)) {
      enhancementDropdownOpen = false;
    }
  }
  
  // Set up click-outside handler for enhancement dropdown
  $effect(() => {
    if (browser && enhancementDropdownOpen) {
      document.addEventListener('click', handleEnhancementClickOutside, true);
      return () => {
        document.removeEventListener('click', handleEnhancementClickOutside, true);
      };
    }
  });
  
  function toggleAnnotations() {
    annotationsVisible = !annotationsVisible;
    settings.setSetting('annotations', 'visible', annotationsVisible);
  }
  
  // Stain enhancement options
  const stainEnhancementOptions: { value: StainEnhancementMode; label: string }[] = [
    { value: 'none', label: 'None' },
    { value: 'gram', label: 'Gram' },
    { value: 'afb', label: 'AFB' },
    { value: 'gms', label: 'GMS' },
  ];
</script>

<header class="app-header">
  {#if showMenuButton}
    <button class="menu-btn" onclick={onMenuClick} aria-label="Open menu">
      <svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
        <line x1="3" y1="12" x2="21" y2="12"></line>
        <line x1="3" y1="6" x2="21" y2="6"></line>
        <line x1="3" y1="18" x2="21" y2="18"></line>
      </svg>
    </button>
  {/if}

  <div class="header-left" class:scrollable={isScrollable} bind:this={headerLeftEl}>
    <!-- Tool toolbar -->
    <div class="tool-toolbar">
      <!-- Undo/Redo group -->
      <div class="tool-group">
        <button 
          class="tool-btn" 
          class:disabled={!tools.canUndo}
          onclick={handleUndo}
          title="Undo (Ctrl+Z)"
          aria-label="Undo"
        >
          <!-- Undo arrow icon -->
          <svg xmlns="http://www.w3.org/2000/svg" width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
            <path d="M3 7v6h6"></path>
            <path d="M21 17a9 9 0 0 0-9-9 9 9 0 0 0-6 2.3L3 13"></path>
          </svg>
        </button>
        <button 
          class="tool-btn"
          class:disabled={!tools.canRedo}
          onclick={handleRedo}
          title="Redo (Ctrl+Y)"
          aria-label="Redo"
        >
          <!-- Redo arrow icon -->
          <svg xmlns="http://www.w3.org/2000/svg" width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
            <path d="M21 7v6h-6"></path>
            <path d="M3 17a9 9 0 0 1 9-9 9 9 0 0 1 6 2.3l3 2.7"></path>
          </svg>
        </button>
      </div>
      
      <div class="tool-separator"></div>
      
      <!-- Measurement tool -->
      <button 
        class="tool-btn"
        class:active={tools.measurementActive}
        onclick={handleMeasure}
        title="Measure Distance (D)"
        aria-label="Measure distance"
      >
        <!-- Ruler icon -->
        <svg xmlns="http://www.w3.org/2000/svg" width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
          <path d="M21.3 8.7 8.7 21.3c-1 1-2.5 1-3.4 0l-2.6-2.6c-1-1-1-2.5 0-3.4L15.3 2.7c1-1 2.5-1 3.4 0l2.6 2.6c1 1 1 2.5 0 3.4Z"></path>
          <path d="m7.5 10.5 2 2"></path>
          <path d="m10.5 7.5 2 2"></path>
          <path d="m13.5 4.5 2 2"></path>
          <path d="m4.5 13.5 2 2"></path>
        </svg>
      </button>
      
      <!-- Region of Interest tool -->
      <button 
        class="tool-btn"
        class:active={tools.roiActive}
        onclick={handleRoi}
        title="Region of Interest (R)"
        aria-label="Region of interest"
      >
        <!-- Rectangle icon -->
        <svg xmlns="http://www.w3.org/2000/svg" width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
          <rect x="3" y="3" width="18" height="18" rx="2" ry="2" stroke="yellow" stroke-dasharray="1 4"></rect>
        </svg>
      </button>
      
      <div class="tool-separator"></div>
      
      <!-- Annotation tools group -->
      <div class="tool-group">
        <button 
          class="tool-btn"
          class:active={tools.annotationTool === 'point'}
          class:disabled={!isLoggedIn}
          onclick={() => handleAnnotationTool('point')}
          title={isLoggedIn ? 'Point Annotation (1)' : 'Log in to use annotation tools'}
          aria-label="Point annotation"
        >
          <!-- Point/dot icon -->
          <svg xmlns="http://www.w3.org/2000/svg" width="18" height="18" viewBox="0 0 24 24" fill="currentColor">
            <circle cx="12" cy="12" r="4"></circle>
          </svg>
        </button>
        <button 
          class="tool-btn"
          class:active={tools.annotationTool === 'ellipse'}
          class:disabled={!isLoggedIn}
          onclick={() => handleAnnotationTool('ellipse')}
          title={isLoggedIn ? 'Ellipse Annotation (2)' : 'Log in to use annotation tools'}
          aria-label="Ellipse annotation"
        >
          <!-- Ellipse/oval icon -->
          <svg xmlns="http://www.w3.org/2000/svg" width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
            <ellipse cx="12" cy="12" rx="9" ry="6"></ellipse>
          </svg>
        </button>
        <button 
          class="tool-btn"
          class:active={tools.annotationTool === 'lasso'}
          class:disabled={!isLoggedIn}
          onclick={() => handleAnnotationTool('lasso')}
          title={isLoggedIn ? 'Lasso Annotation (hold 3)' : 'Log in to use annotation tools'}
          aria-label="Lasso annotation"
        >
          <!-- Lasso/blob icon - Photoshop-style freehand selection -->
          <svg xmlns="http://www.w3.org/2000/svg" width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
            <path d="M4 12c0-4 3-8 8-8 4 0 7 2.5 8 6 .5 2-.5 4-2 5.5-1.5 1.5-3.5 2.5-6 2.5-3 0-5.5-1-7-3"></path>
            <path d="M4 12c-.5 2 0 4 2 5.5 1 .8 2.5 1.5 4 1.5"></path>
            <circle cx="10" cy="19" r="2"></circle>
          </svg>
        </button>
        <button 
          class="tool-btn"
          class:active={tools.annotationTool === 'polygon'}
          class:disabled={!isLoggedIn}
          onclick={() => handleAnnotationTool('polygon')}
          title={isLoggedIn ? 'Polygon Annotation (tap 3)' : 'Log in to use annotation tools'}
          aria-label="Polygon annotation"
        >
          <!-- Pentagon/polygon icon -->
          <svg xmlns="http://www.w3.org/2000/svg" width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linejoin="round">
            <polygon points="12 2 22 8.5 19 21 5 21 2 8.5"></polygon>
          </svg>
        </button>
        <button 
          class="tool-btn"
          class:active={tools.annotationTool === 'mask'}
          class:disabled={!isLoggedIn}
          onclick={() => handleAnnotationTool('mask')}
          title={isLoggedIn ? 'Mask Painting (4)' : 'Log in to use annotation tools'}
          aria-label="Mask painting"
        >
          <!-- Paintbrush icon -->
          <svg xmlns="http://www.w3.org/2000/svg" width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
            <path d="m9.06 11.9 8.07-8.06a2.85 2.85 0 1 1 4.03 4.03l-8.06 8.08"></path>
            <path d="M7.07 14.94c-1.66 0-3 1.35-3 3.02 0 1.33-2.5 1.52-2 2.02 1.08 1.1 2.49 2.02 4 2.02 2.2 0 4-1.8 4-4.04a3.01 3.01 0 0 0-3-3.02z"></path>
          </svg>
        </button>
      </div>
      
      <div class="tool-separator"></div>
      
      <!-- Stain enhancement selector (custom dropdown) -->
      <div class="enhancement-dropdown" bind:this={enhancementDropdownEl}>
        <button 
          class="enhancement-trigger"
          class:open={enhancementDropdownOpen}
          onclick={toggleEnhancementDropdown}
          bind:this={enhancementTriggerEl}
          title="Stain Enhancement"
          aria-haspopup="listbox"
          aria-expanded={enhancementDropdownOpen}
        >
          <span class="enhancement-label">{stainEnhancementOptions.find(o => o.value === stainEnhancement)?.label ?? 'None'}</span>
          <svg class="enhancement-chevron" xmlns="http://www.w3.org/2000/svg" width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
            <polyline points="6 9 12 15 18 9"></polyline>
          </svg>
        </button>
        {#if enhancementDropdownOpen}
          <div class="enhancement-menu" role="listbox" style="top: {dropdownPosition.top}px; left: {dropdownPosition.left}px;">
            {#each stainEnhancementOptions as mode}
              <button
                class="enhancement-option"
                class:selected={stainEnhancement === mode.value}
                onclick={() => selectEnhancement(mode.value)}
                role="option"
                aria-selected={stainEnhancement === mode.value}
              >
                {mode.label}
                {#if stainEnhancement === mode.value}
                  <svg class="check-icon" xmlns="http://www.w3.org/2000/svg" width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5">
                    <polyline points="20 6 9 17 4 12"></polyline>
                  </svg>
                {/if}
              </button>
            {/each}
          </div>
        {/if}
      </div>
      
      <!-- Toggle annotations visibility -->
      <button
        class="tool-btn"
        class:active={annotationsVisible}
        onclick={toggleAnnotations}
        title={annotationsVisible ? 'Hide Annotations' : 'Show Annotations'}
        aria-label={annotationsVisible ? 'Hide annotations' : 'Show annotations'}
      >
        <!-- Eye icon -->
        <svg xmlns="http://www.w3.org/2000/svg" width="18" height="18" viewBox="0 0 20 20" fill="currentColor">
          {#if annotationsVisible}
            <path d="M10 12.5a2.5 2.5 0 100-5 2.5 2.5 0 000 5z" />
            <path fill-rule="evenodd" d="M.664 10.59a1.651 1.651 0 010-1.186A10.004 10.004 0 0110 3c4.257 0 7.893 2.66 9.336 6.41.147.381.146.804 0 1.186A10.004 10.004 0 0110 17c-4.257 0-7.893-2.66-9.336-6.41zM14 10a4 4 0 11-8 0 4 4 0 018 0z" clip-rule="evenodd" />
          {:else}
            <path fill-rule="evenodd" d="M3.28 2.22a.75.75 0 00-1.06 1.06l14.5 14.5a.75.75 0 101.06-1.06l-1.745-1.745a10.029 10.029 0 003.3-4.38 1.651 1.651 0 000-1.185A10.004 10.004 0 009.999 3a9.956 9.956 0 00-4.744 1.194L3.28 2.22zM7.752 6.69l1.092 1.092a2.5 2.5 0 013.374 3.373l1.091 1.092a4 4 0 00-5.557-5.557z" clip-rule="evenodd" />
            <path d="M10.748 13.93l2.523 2.523a9.987 9.987 0 01-3.27.547c-4.258 0-7.894-2.66-9.337-6.41a1.651 1.651 0 010-1.186A10.007 10.007 0 012.839 6.02L6.07 9.252a4 4 0 004.678 4.678z" />
          {/if}
        </svg>
      </button>
    </div>
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
        </div>

        <div class="help-card">
          <h3>Keyboard Shortcuts</h3>
          <div class="help-row"><kbd>H</kbd><span>Toggle this help</span></div>
          <div class="help-row"><kbd>N</kbd><span>Cycle stain normalization</span></div>
          <div class="help-row"><kbd>E</kbd><span>Cycle stain enhancement</span></div>
          <div class="help-row"><kbd>D</kbd><span>Toggle measurement mode</span></div>
          <div class="help-row"><kbd>R</kbd><span>Toggle region of interest</span></div>
          <div class="help-row"><kbd>Esc</kbd><span>Close help / Cancel</span></div>
        </div>

        <div class="help-card">
          <h3>Measurement Tool</h3>
          <div class="help-row"><kbd>Middle Drag</kbd><span>Measure while dragging</span></div>
          <div class="help-row"><kbd>D</kbd><span>Toggle measurement mode</span></div>
          <div class="help-row"><span class="help-note">Click or pan to dismiss measurement</span></div>
        </div>

        <div class="help-card">
          <h3>Region of Interest</h3>
          <div class="help-row"><kbd>R</kbd><span>Toggle ROI mode</span></div>
          <div class="help-row"><span class="help-note">Click two corners or click+drag</span></div>
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
    padding: 0 1rem;
    background: #141414;
    border-bottom: 1px solid #333;
    flex-shrink: 0;
    height: 48px;
    box-sizing: border-box;
    position: relative;
    z-index: 100;
  }

  .header-left {
    display: flex;
    align-items: center;
    gap: 0.75rem;
    flex: 1;
    min-width: 0;
    overflow-x: auto;
    overflow-y: hidden;
    scrollbar-width: thin;
    scrollbar-color: #444 transparent;
    padding-right: 0.75rem;
  }

  .header-left::-webkit-scrollbar {
    height: 3px;
  }

  .header-left::-webkit-scrollbar-thumb {
    background: #444;
    border-radius: 2px;
  }

  .header-left::-webkit-scrollbar-track {
    background: transparent;
  }

  .header-left.scrollable {
    background: rgba(0, 0, 0, 0.5);
    padding-left: 0.5rem;
    margin-top: -0.5rem;
    margin-bottom: -0.5rem;
    padding-top: 0.5rem;
    padding-bottom: 0.5rem;
  }

  /* Tool toolbar */
  .tool-toolbar {
    display: flex;
    align-items: center;
    gap: 4px;
    flex-shrink: 0;
  }

  .tool-group {
    display: flex;
    align-items: center;
    gap: 2px;
    flex-shrink: 0;
  }

  .tool-separator {
    width: 1px;
    height: 20px;
    background: rgba(255, 255, 255, 0.15);
    margin: 0 4px;
    flex-shrink: 0;
  }

  /* Custom enhancement dropdown */
  .enhancement-dropdown {
    position: relative;
  }
  
  .enhancement-trigger {
    display: flex;
    align-items: center;
    gap: 6px;
    height: 32px;
    padding: 0 10px;
    background: rgba(255, 255, 255, 0.08);
    border: 1px solid rgba(255, 255, 255, 0.15);
    border-radius: 4px;
    color: #fff;
    font-size: 0.8rem;
    cursor: pointer;
    outline: none;
    transition: background-color 0.15s, border-color 0.15s;
  }
  
  .enhancement-trigger:hover {
    background: rgba(255, 255, 255, 0.12);
    border-color: rgba(255, 255, 255, 0.25);
  }
  
  .enhancement-trigger:focus,
  .enhancement-trigger.open {
    border-color: #0088ff;
  }
  
  .enhancement-label {
    min-width: 40px;
  }
  
  .enhancement-chevron {
    transition: transform 0.15s;
  }
  
  .enhancement-trigger.open .enhancement-chevron {
    transform: rotate(180deg);
  }
  
  .enhancement-menu {
    position: fixed;
    min-width: 120px;
    background: #222;
    border: 1px solid #444;
    border-radius: 6px;
    box-shadow: 0 4px 16px rgba(0, 0, 0, 0.4);
    padding: 4px 0;
    z-index: 1000;
    animation: dropdownFadeIn 0.1s ease-out;
  }
  
  @keyframes dropdownFadeIn {
    from {
      opacity: 0;
      transform: translateY(-4px);
    }
    to {
      opacity: 1;
      transform: translateY(0);
    }
  }
  
  .enhancement-option {
    display: flex;
    align-items: center;
    justify-content: space-between;
    width: 100%;
    padding: 8px 12px;
    background: transparent;
    border: none;
    color: #ccc;
    font-size: 0.85rem;
    text-align: left;
    cursor: pointer;
    transition: background-color 0.1s, color 0.1s;
  }
  
  .enhancement-option:hover {
    background: rgba(255, 255, 255, 0.1);
    color: #fff;
  }
  
  .enhancement-option.selected {
    color: #0088ff;
  }
  
  .check-icon {
    color: #0088ff;
  }

  .tool-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 32px;
    height: 32px;
    background: transparent;
    border: none;
    border-radius: 4px;
    color: #9ca3af;
    cursor: pointer;
    transition: background-color 0.15s, color 0.15s;
    flex-shrink: 0;
  }

  .tool-btn:hover:not(.disabled) {
    background: rgba(255, 255, 255, 0.1);
    color: #fff;
  }

  .tool-btn.active {
    background: #0088ff;
    color: #fff;
  }

  .tool-btn.active:hover {
    background: #0077e6;
  }

  .tool-btn.disabled {
    color: #555;
    cursor: not-allowed;
  }

  .tool-btn:focus {
    outline: none;
  }

  .tool-btn svg {
    flex-shrink: 0;
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
    background: var(--secondary-muted);
    border-color: var(--secondary-hex);
    color: var(--secondary-hex);
  }

  .logout-btn:hover {
    background: rgba(239, 68, 68, 0.2);
    border-color: #ef4444;
    color: #fca5a5;
  }

  .auth-label {
    display: block;
  }

  /* Hide auth buttons (login/logout) completely on small screens */
  @media (max-width: 600px) {
    .auth-btn {
      display: none !important;
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
    flex-shrink: 0;
  }

  .menu-btn:hover,
  .settings-btn:hover,
  .help-btn:hover {
    background: var(--secondary-muted);
    color: #fff;
  }

  .help-btn.active {
    background: var(--primary-hex);
    color: white;
  }

  /* Eye-catching pulse/breathing animation for help button on page load */
  @keyframes help-pulse {
    0% {
      transform: scale(1);
      box-shadow: 
        0 0 0 0 rgba(254, 14, 148, 0.7),
        0 0 0 0 rgba(254, 14, 148, 0.4);
    }
    50% {
      transform: scale(1.1);
      box-shadow: 
        0 0 16px 4px rgba(254, 14, 148, 0.6),
        0 0 32px 8px rgba(254, 14, 148, 0.3);
    }
    100% {
      transform: scale(1);
      box-shadow: 
        0 0 0 0 rgba(254, 14, 148, 0.7),
        0 0 0 0 rgba(254, 14, 148, 0.4);
    }
  }

  .help-btn.pulsing {
    animation: help-pulse 0.75s ease-in-out 2;
    background: linear-gradient(135deg, rgba(254, 14, 148, 0.9), rgba(94, 74, 239, 0.9));
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
    background: var(--secondary-muted);
    border-color: rgba(254, 14, 148, 0.25);
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
    color: var(--secondary-hex);
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

  /* Touch device adaptations - larger touch targets (1.5x size) */
  @media (pointer: coarse) {
    .tool-btn {
      width: 48px;
      height: 48px;
    }

    .tool-btn svg {
      width: 27px;
      height: 27px;
    }

    .tool-separator {
      height: 30px;
      margin: 0 6px;
    }

    .menu-btn,
    .settings-btn,
    .help-btn,
    .auth-btn {
      width: 54px;
      height: 54px;
    }

    .menu-btn .icon,
    .settings-btn .icon,
    .help-btn .icon,
    .auth-btn .icon {
      width: 1.875rem;
      height: 1.875rem;
    }

    .menu-btn svg,
    .settings-btn svg,
    .help-btn svg {
      width: 30px;
      height: 30px;
    }

    .app-header {
      height: 72px;
      padding: 0.5rem 0.75rem;
    }

    .enhancement-trigger {
      height: 48px;
      padding: 0 12px;
      font-size: 0.9rem;
    }

    .enhancement-label {
      min-width: 50px;
    }

    .enhancement-chevron {
      width: 16px;
      height: 16px;
    }
  }
</style>
