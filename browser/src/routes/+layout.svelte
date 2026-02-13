<script lang="ts">
	import './layout.css';
	import Sidebar from '$lib/components/Sidebar.svelte';
	import AppHeader from '$lib/components/AppHeader.svelte';
	import type { Snippet } from 'svelte';
	import { onMount } from 'svelte';
	import { browser } from '$app/environment';
	import { authStore, type UserCredentials } from '$lib/stores/auth';

	interface SlideListItem {
		id: string;
		width: number;
		height: number;
		/** Original filename extracted from the S3 key */
		filename: string;
		/** Full size of the original slide file in bytes */
		full_size: number;
		/** Current processing progress in steps */
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
		auth: {
			user: UserCredentials;
			refreshExpiry: number | null;
		} | null;
	}

	let { children, data }: { children: Snippet; data: LayoutData } = $props();

	// Initialize auth store from SSR data on mount
	onMount(() => {
		if (data.auth) {
			authStore.initialize(data.auth.user, data.auth.refreshExpiry);
		}
	});

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

	// Fix for mobile viewport height (100vh issue)
	function setViewportHeight() {
		if (browser) {
			// Set CSS custom property to actual viewport height
			const vh = window.innerHeight * 0.01;
			document.documentElement.style.setProperty('--vh', `${vh}px`);
		}
	}

	onMount(() => {
		checkMobile();
		setViewportHeight();
		// Auto-collapse on small screens initially
		sidebarCollapsed = window.innerWidth < 1024;
		
		window.addEventListener('resize', checkMobile);
		window.addEventListener('resize', setViewportHeight);
		// Also update on orientation change
		window.addEventListener('orientationchange', () => {
			setTimeout(setViewportHeight, 100);
		});
		
		return () => {
			window.removeEventListener('resize', checkMobile);
			window.removeEventListener('resize', setViewportHeight);
		};
	});
</script>

<svelte:head>
	<title>Eosin — Next-Generation WSI Viewer</title>
	<link rel="icon" type="image/png" href="/favicon.png" />
</svelte:head>
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
		<!-- App header with settings button -->
		<AppHeader 
			showMenuButton={isMobile && sidebarCollapsed}
			onMenuClick={toggleSidebar}
		/>
		{@render children()}
	</div>
</div>

<style>
	.app-layout {
		display: flex;
		position: fixed;
		inset: 0;
		overflow: hidden;
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
		min-height: 0; /* Allow flex children to shrink */
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
		height: 100%;
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

	/* Responsive adjustments */
	@media (max-width: 768px) {
		.sidebar-container:not(.mobile-open) {
			position: fixed;
			transform: translateX(-100%);
		}
	}
</style>
