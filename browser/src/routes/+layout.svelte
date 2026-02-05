<script lang="ts">
	import './layout.css';
	import favicon from '$lib/assets/favicon.svg';
	import Sidebar from '$lib/components/Sidebar.svelte';
	import type { Snippet } from 'svelte';
	import { onMount } from 'svelte';
	import { browser } from '$app/environment';

	interface SlideListItem {
		id: string;
		width: number;
		height: number;
		/** Full size of the original slide file in bytes */
		full_size: number;
		/** Current processing progress in steps of 10,000 tiles */
		progress_steps: number;
		/** Total tiles to process */
		progress_total: number;
		url: string;
	}

	interface LayoutData {
		slides: SlideListItem[];
		totalCount: number;
		hasMore: boolean;
		pageSize: number;
		error: string | null;
	}

	let { children, data }: { children: Snippet; data: LayoutData } = $props();

	// Breakpoint for mobile/tablet
	const MOBILE_BREAKPOINT = 768;

	let sidebarCollapsed = $state(true);
	let isMobile = $state(false);

	function checkMobile() {
		if (browser) {
			isMobile = window.innerWidth < MOBILE_BREAKPOINT;
			// Auto-collapse on mobile
			if (isMobile) {
				sidebarCollapsed = true;
			}
		}
	}

	function toggleSidebar() {
		sidebarCollapsed = !sidebarCollapsed;
	}

	function handleOverlayClick() {
		if (isMobile && !sidebarCollapsed) {
			sidebarCollapsed = true;
		}
	}

	onMount(() => {
		checkMobile();
		// Auto-collapse on small screens initially
		sidebarCollapsed = window.innerWidth < 1024;
		
		window.addEventListener('resize', checkMobile);
		return () => {
			window.removeEventListener('resize', checkMobile);
		};
	});
</script>

<svelte:head><link rel="icon" href={favicon} /></svelte:head>
<div class="app-layout" class:mobile={isMobile}>
	<!-- Mobile overlay when sidebar is open -->
	{#if isMobile && !sidebarCollapsed}
		<button class="sidebar-overlay" onclick={handleOverlayClick} aria-label="Close sidebar"></button>
	{/if}
	
	<div class="sidebar-container" class:collapsed={sidebarCollapsed} class:mobile-open={isMobile && !sidebarCollapsed}>
		<Sidebar 
			initialSlides={data.slides} 
			totalCount={data.totalCount} 
			hasMore={data.hasMore}
			pageSize={data.pageSize}
			collapsed={sidebarCollapsed}
			onToggle={toggleSidebar}
		/>
	</div>
	
	<div class="main-content">
		<!-- Mobile header with menu button -->
		{#if isMobile && sidebarCollapsed}
			<header class="mobile-header">
				<button class="menu-btn" onclick={toggleSidebar} aria-label="Open menu">
					<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
						<line x1="3" y1="12" x2="21" y2="12"></line>
						<line x1="3" y1="6" x2="21" y2="6"></line>
						<line x1="3" y1="18" x2="21" y2="18"></line>
					</svg>
				</button>
				<span class="mobile-title">Slides</span>
			</header>
		{/if}
		{@render children()}
	</div>
</div>

<style>
	.app-layout {
		display: flex;
		height: 100vh;
		overflow: hidden;
		position: relative;
	}

	.sidebar-container {
		flex-shrink: 0;
		transition: margin-left 0.2s ease;
	}

	.main-content {
		flex: 1;
		overflow: hidden;
		display: flex;
		flex-direction: column;
	}

	/* Mobile overlay */
	.sidebar-overlay {
		display: none;
	}

	.app-layout.mobile .sidebar-overlay {
		display: block;
		position: fixed;
		inset: 0;
		background: rgba(0, 0, 0, 0.5);
		z-index: 40;
		border: none;
		cursor: pointer;
	}

	/* Mobile sidebar positioning */
	.app-layout.mobile .sidebar-container {
		position: fixed;
		left: 0;
		top: 0;
		height: 100vh;
		z-index: 50;
		transform: translateX(-100%);
		transition: transform 0.2s ease;
	}

	.app-layout.mobile .sidebar-container.mobile-open {
		transform: translateX(0);
	}

	.app-layout.mobile .sidebar-container.mobile-open :global(.sidebar) {
		width: 280px;
		min-width: 280px;
	}

	/* Mobile header */
	.mobile-header {
		display: flex;
		align-items: center;
		gap: 0.75rem;
		padding: 0.75rem 1rem;
		background: #141414;
		border-bottom: 1px solid #333;
		flex-shrink: 0;
	}

	.menu-btn {
		display: flex;
		align-items: center;
		justify-content: center;
		width: 40px;
		height: 40px;
		padding: 0;
		background: transparent;
		border: none;
		color: #aaa;
		cursor: pointer;
		border-radius: 8px;
		transition: background-color 0.15s, color 0.15s;
	}

	.menu-btn:hover {
		background: #333;
		color: #fff;
	}

	.mobile-title {
		font-size: 1.125rem;
		font-weight: 600;
		color: #eee;
	}

	/* Responsive adjustments */
	@media (max-width: 768px) {
		.sidebar-container:not(.mobile-open) {
			position: fixed;
			transform: translateX(-100%);
		}
	}
</style>
