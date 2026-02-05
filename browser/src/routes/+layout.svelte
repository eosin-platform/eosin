<script lang="ts">
	import './layout.css';
	import favicon from '$lib/assets/favicon.svg';
	import Sidebar from '$lib/components/Sidebar.svelte';
	import type { Snippet } from 'svelte';

	interface SlideListItem {
		id: string;
		width: number;
		height: number;
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
</script>

<svelte:head><link rel="icon" href={favicon} /></svelte:head>
<div class="app-layout">
	<Sidebar 
		initialSlides={data.slides} 
		totalCount={data.totalCount} 
		hasMore={data.hasMore}
		pageSize={data.pageSize}
	/>
	<div class="main-content">
		{@render children()}
	</div>
</div>

<style>
	.app-layout {
		display: flex;
		height: 100vh;
		overflow: hidden;
	}

	.main-content {
		flex: 1;
		overflow: hidden;
		display: flex;
		flex-direction: column;
	}
</style>
