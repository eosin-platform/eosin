<script lang="ts">
	let mobileMenuOpen = $state(false);

	function scrollToSection(id: string) {
		const element = document.getElementById(id);
		if (element) {
			element.scrollIntoView({ behavior: 'smooth' });
		}
		mobileMenuOpen = false;
	}
</script>

<svelte:head>
	<title>Eosin – Modern Pathology Viewer</title>
	<link rel="icon" href="/favicon.png" />
	<meta
		name="description"
		content="Eosin is a modern web-based whole-slide image viewer for digital pathology. Explore slides instantly with no installation, featuring real-time zoom, annotation tools, and AI-assisted analysis."
	/>
</svelte:head>

<!-- Header -->
<header
	class="fixed top-0 right-0 left-0 z-50 border-b border-white/10 bg-[#0a0a1a]/90 backdrop-blur-md"
>
	<nav class="mx-auto flex max-w-7xl items-center justify-between px-4 py-3 sm:px-6 lg:px-8">
		<a href="/" class="flex items-center gap-2">
			<img src="/logo_half.png" alt="Eosin" class="h-8 w-auto sm:hidden" />
			<img src="/logo_full.png" alt="Eosin" class="hidden h-8 w-auto sm:block" />
		</a>

		<!-- Desktop Navigation -->
		<div class="hidden items-center gap-6 md:flex">
			<button
				onclick={() => scrollToSection('how-it-works')}
				class="text-sm text-gray-300 transition-colors hover:text-white"
			>
				Product
			</button>
			<button
				onclick={() => scrollToSection('for-students')}
				class="text-sm text-gray-300 transition-colors hover:text-white"
			>
				For Students
			</button>
			<button
				onclick={() => scrollToSection('for-educators')}
				class="text-sm text-gray-300 transition-colors hover:text-white"
			>
				For Educators
			</button>
			<a
				href="https://eosin.ai/app/"
				target="_blank"
				rel="noopener noreferrer"
				class="rounded-lg bg-[#5E4AEF] px-4 py-2 text-sm font-medium text-white transition-all hover:scale-105 hover:bg-[#4e3cd0]"
			>
				Demo
			</a>
		</div>

		<!-- Mobile menu button -->
		<button
			onclick={() => (mobileMenuOpen = !mobileMenuOpen)}
			class="flex h-10 w-10 items-center justify-center rounded-lg text-gray-300 transition-colors hover:bg-white/10 md:hidden"
			aria-label="Toggle menu"
		>
			{#if mobileMenuOpen}
				<svg class="h-6 w-6" fill="none" stroke="currentColor" viewBox="0 0 24 24">
					<path
						stroke-linecap="round"
						stroke-linejoin="round"
						stroke-width="2"
						d="M6 18L18 6M6 6l12 12"
					/>
				</svg>
			{:else}
				<svg class="h-6 w-6" fill="none" stroke="currentColor" viewBox="0 0 24 24">
					<path
						stroke-linecap="round"
						stroke-linejoin="round"
						stroke-width="2"
						d="M4 6h16M4 12h16M4 18h16"
					/>
				</svg>
			{/if}
		</button>
	</nav>

	<!-- Mobile Navigation -->
	{#if mobileMenuOpen}
		<div class="border-t border-white/10 bg-[#0a0a1a] px-4 py-4 md:hidden">
			<div class="flex flex-col gap-3">
				<button
					onclick={() => scrollToSection('how-it-works')}
					class="rounded-lg px-3 py-2 text-left text-gray-300 transition-colors hover:bg-white/10 hover:text-white"
				>
					Product
				</button>
				<button
					onclick={() => scrollToSection('for-students')}
					class="rounded-lg px-3 py-2 text-left text-gray-300 transition-colors hover:bg-white/10 hover:text-white"
				>
					For Students
				</button>
				<button
					onclick={() => scrollToSection('for-educators')}
					class="rounded-lg px-3 py-2 text-left text-gray-300 transition-colors hover:bg-white/10 hover:text-white"
				>
					For Educators
				</button>
				<a
					href="https://eosin.ai/app/"
					target="_blank"
					rel="noopener noreferrer"
					class="rounded-lg bg-[#5E4AEF] px-3 py-2 text-center font-medium text-white transition-all hover:bg-[#4e3cd0]"
				>
					Demo
				</a>
			</div>
		</div>
	{/if}
</header>

<main class="min-h-screen bg-[#0a0a1a]">
	<!-- Hero Section -->
	<section
		class="relative flex min-h-[90vh] items-center overflow-hidden pt-20"
		style="background: radial-gradient(ellipse at 30% 20%, rgba(94, 74, 239, 0.15) 0%, transparent 50%), radial-gradient(ellipse at 80% 80%, rgba(254, 14, 148, 0.08) 0%, transparent 40%), #0a0a1a;"
	>
		<div class="mx-auto w-full max-w-7xl px-4 py-12 sm:px-6 lg:px-8">
			<div class="grid items-center gap-8 lg:grid-cols-2 lg:gap-12">
				<!-- Text Content -->
				<div class="animate-fade-in order-2 lg:order-1">
					<h1
						class="mb-6 text-4xl leading-tight font-bold tracking-tight text-white sm:text-5xl lg:text-6xl"
					>
						A modern way to explore pathology slides.
					</h1>
					<p class="mb-8 max-w-xl text-lg leading-relaxed text-gray-300 sm:text-xl">
						Navigate whole-slide images instantly in your browser. Annotate, zoom, and learn with
						AI-assisted tools — no installation required.
					</p>

					<div class="mb-4 flex flex-col gap-4 sm:flex-row">
						<a
							href="https://eosin.ai/app/"
							target="_blank"
							rel="noopener noreferrer"
							class="inline-flex items-center justify-center gap-2 rounded-xl bg-[#5E4AEF] px-6 py-3 font-semibold text-white shadow-lg shadow-[#5E4AEF]/25 transition-all hover:scale-105 hover:bg-[#4e3cd0]"
						>
							Open Demo
							<svg class="h-5 w-5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
								<path
									stroke-linecap="round"
									stroke-linejoin="round"
									stroke-width="2"
									d="M10 6H6a2 2 0 00-2 2v10a2 2 0 002 2h10a2 2 0 002-2v-4M14 4h6m0 0v6m0-6L10 14"
								/>
							</svg>
						</a>
						<button
							onclick={() => scrollToSection('how-it-works')}
							class="inline-flex items-center justify-center gap-2 rounded-xl border-2 border-[#FE0E94] px-6 py-3 font-semibold text-[#FE0E94] transition-all hover:scale-105 hover:bg-[#FE0E94]/10"
						>
							Watch How It Works
							<svg class="h-5 w-5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
								<path
									stroke-linecap="round"
									stroke-linejoin="round"
									stroke-width="2"
									d="M19 9l-7 7-7-7"
								/>
							</svg>
						</button>
					</div>

					<p class="text-sm text-gray-500">No login required for the demo.</p>
				</div>

				<!-- Hero Video -->
				<div class="animate-fade-in-delayed order-1 lg:order-2">
					<div
						class="overflow-hidden rounded-2xl border border-white/10 shadow-2xl shadow-[#5E4AEF]/20"
					>
						<video
							autoplay
							loop
							muted
							playsinline
							class="aspect-video w-full object-cover"
							poster="/logo_full.png"
						>
							<source src="/hero.webm" type="video/webm" />
							Your browser does not support the video tag.
						</video>
					</div>
				</div>
			</div>
		</div>
	</section>

	<!-- How It Works -->
	<section id="how-it-works" class="bg-[#0d0d20] py-20 sm:py-28">
		<div class="mx-auto max-w-7xl px-4 sm:px-6 lg:px-8">
			<div class="mb-12 text-center sm:mb-16">
				<h2 class="mb-4 text-3xl font-bold text-white sm:text-4xl">How It Works</h2>
				<p class="mx-auto max-w-2xl text-gray-400">
					Get started in seconds — no downloads, no complex setup.
				</p>
			</div>

			<div class="grid gap-8 md:grid-cols-3">
				<!-- Step 1 -->
				<div
					class="group rounded-2xl border border-white/10 bg-white/5 p-6 transition-all hover:border-[#5E4AEF]/50 hover:bg-white/10 sm:p-8"
				>
					<div
						class="mb-4 flex h-12 w-12 items-center justify-center rounded-xl bg-[#5E4AEF]/20 text-2xl"
					>
						🔬
					</div>
					<div class="mb-2 text-sm font-medium text-[#5E4AEF]">Step 1</div>
					<h3 class="mb-3 text-xl font-semibold text-white">Open a slide instantly</h3>
					<p class="leading-relaxed text-gray-400">
						Click any slide link and start exploring immediately in your browser. No plugins, no
						registration.
					</p>
				</div>

				<!-- Step 2 -->
				<div
					class="group rounded-2xl border border-white/10 bg-white/5 p-6 transition-all hover:border-[#5E4AEF]/50 hover:bg-white/10 sm:p-8"
				>
					<div
						class="mb-4 flex h-12 w-12 items-center justify-center rounded-xl bg-[#5E4AEF]/20 text-2xl"
					>
						🔍
					</div>
					<div class="mb-2 text-sm font-medium text-[#5E4AEF]">Step 2</div>
					<h3 class="mb-3 text-xl font-semibold text-white">Explore at any magnification</h3>
					<p class="leading-relaxed text-gray-400">
						Zoom seamlessly from 1x to 40x. Pan effortlessly to find exactly what you're looking for
						with smooth, responsive controls.
					</p>
				</div>

				<!-- Step 3 -->
				<div
					class="group rounded-2xl border border-white/10 bg-white/5 p-6 transition-all hover:border-[#5E4AEF]/50 hover:bg-white/10 sm:p-8"
				>
					<div
						class="mb-4 flex h-12 w-12 items-center justify-center rounded-xl bg-[#FE0E94]/20 text-2xl"
					>
						🤖
					</div>
					<div class="mb-2 text-sm font-medium text-[#FE0E94]">Step 3</div>
					<h3 class="mb-3 text-xl font-semibold text-white">Learn with AI-assisted tools</h3>
					<p class="leading-relaxed text-gray-400">
						Coming soon: nuclei detection overlays, prototype embeddings, and intelligent analysis
						to enhance your learning.
					</p>
				</div>
			</div>
		</div>
	</section>

	<!-- For Students / For Educators -->
	<section id="for-students" class="py-20 sm:py-28">
		<div class="mx-auto max-w-7xl px-4 sm:px-6 lg:px-8">
			<div class="mb-12 text-center sm:mb-16">
				<h2 class="mb-4 text-3xl font-bold text-white sm:text-4xl">Built for Learning</h2>
				<p class="mx-auto max-w-2xl text-gray-400">
					Whether you're studying independently or teaching a class, Eosin adapts to your workflow.
				</p>
			</div>

			<div id="for-educators" class="grid gap-8 lg:grid-cols-2">
				<!-- For Students Card -->
				<div
					class="group rounded-2xl border border-[#5E4AEF]/30 bg-[#5E4AEF]/5 p-6 transition-all hover:scale-[1.02] hover:border-[#5E4AEF]/50 hover:bg-[#5E4AEF]/10 sm:p-8"
				>
					<div
						class="mb-4 inline-flex items-center gap-2 rounded-full bg-[#5E4AEF]/20 px-3 py-1 text-sm font-medium text-[#5E4AEF]"
					>
						<svg class="h-4 w-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
							<path
								stroke-linecap="round"
								stroke-linejoin="round"
								stroke-width="2"
								d="M12 6.253v13m0-13C10.832 5.477 9.246 5 7.5 5S4.168 5.477 3 6.253v13C4.168 18.477 5.754 18 7.5 18s3.332.477 4.5 1.253m0-13C13.168 5.477 14.754 5 16.5 5c1.747 0 3.332.477 4.5 1.253v13C19.832 18.477 18.247 18 16.5 18c-1.746 0-3.332.477-4.5 1.253"
							/>
						</svg>
						For Students
					</div>
					<h3 class="mb-4 text-2xl font-bold text-white">Learn pathology visually</h3>
					<ul class="space-y-4">
						<li class="flex items-start gap-3 text-gray-300">
							<svg
								class="mt-0.5 h-5 w-5 flex-shrink-0 text-[#5E4AEF]"
								fill="none"
								stroke="currentColor"
								viewBox="0 0 24 24"
							>
								<path
									stroke-linecap="round"
									stroke-linejoin="round"
									stroke-width="2"
									d="M5 13l4 4L19 7"
								/>
							</svg>
							<span
								>Access curated slide collections anytime — study from your laptop, tablet, or
								phone.</span
							>
						</li>
						<li class="flex items-start gap-3 text-gray-300">
							<svg
								class="mt-0.5 h-5 w-5 flex-shrink-0 text-[#5E4AEF]"
								fill="none"
								stroke="currentColor"
								viewBox="0 0 24 24"
							>
								<path
									stroke-linecap="round"
									stroke-linejoin="round"
									stroke-width="2"
									d="M5 13l4 4L19 7"
								/>
							</svg>
							<span
								>Follow instructor-shared permalinks to exact diagnostic regions for guided
								learning.</span
							>
						</li>
						<li class="flex items-start gap-3 text-gray-300">
							<svg
								class="mt-0.5 h-5 w-5 flex-shrink-0 text-[#5E4AEF]"
								fill="none"
								stroke="currentColor"
								viewBox="0 0 24 24"
							>
								<path
									stroke-linecap="round"
									stroke-linejoin="round"
									stroke-width="2"
									d="M5 13l4 4L19 7"
								/>
							</svg>
							<span>Practice identifying features with upcoming AI-assisted hints and overlays.</span>
						</li>
					</ul>
				</div>

				<!-- For Educators Card -->
				<div
					class="group rounded-2xl border border-[#FE0E94]/30 bg-[#FE0E94]/5 p-6 transition-all hover:scale-[1.02] hover:border-[#FE0E94]/50 hover:bg-[#FE0E94]/10 sm:p-8"
				>
					<div
						class="mb-4 inline-flex items-center gap-2 rounded-full bg-[#FE0E94]/20 px-3 py-1 text-sm font-medium text-[#FE0E94]"
					>
						<svg class="h-4 w-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
							<path
								stroke-linecap="round"
								stroke-linejoin="round"
								stroke-width="2"
								d="M17 20h5v-2a3 3 0 00-5.356-1.857M17 20H7m10 0v-2c0-.656-.126-1.283-.356-1.857M7 20H2v-2a3 3 0 015.356-1.857M7 20v-2c0-.656.126-1.283.356-1.857m0 0a5.002 5.002 0 019.288 0M15 7a3 3 0 11-6 0 3 3 0 016 0zm6 3a2 2 0 11-4 0 2 2 0 014 0zM7 10a2 2 0 11-4 0 2 2 0 014 0z"
							/>
						</svg>
						For Educators
					</div>
					<h3 class="mb-4 text-2xl font-bold text-white">Teach with precision</h3>
					<ul class="space-y-4">
						<li class="flex items-start gap-3 text-gray-300">
							<svg
								class="mt-0.5 h-5 w-5 flex-shrink-0 text-[#FE0E94]"
								fill="none"
								stroke="currentColor"
								viewBox="0 0 24 24"
							>
								<path
									stroke-linecap="round"
									stroke-linejoin="round"
									stroke-width="2"
									d="M5 13l4 4L19 7"
								/>
							</svg>
							<span
								>Create and share permalinks to specific fields-of-view for lectures and exams.</span
							>
						</li>
						<li class="flex items-start gap-3 text-gray-300">
							<svg
								class="mt-0.5 h-5 w-5 flex-shrink-0 text-[#FE0E94]"
								fill="none"
								stroke="currentColor"
								viewBox="0 0 24 24"
							>
								<path
									stroke-linecap="round"
									stroke-linejoin="round"
									stroke-width="2"
									d="M5 13l4 4L19 7"
								/>
							</svg>
							<span
								>Maintain consistent exemplar slides across cohorts — no more broken links or lost
								files.</span
							>
						</li>
						<li class="flex items-start gap-3 text-gray-300">
							<svg
								class="mt-0.5 h-5 w-5 flex-shrink-0 text-[#FE0E94]"
								fill="none"
								stroke="currentColor"
								viewBox="0 0 24 24"
							>
								<path
									stroke-linecap="round"
									stroke-linejoin="round"
									stroke-width="2"
									d="M5 13l4 4L19 7"
								/>
							</svg>
							<span
								>Web-based access means every student has the same experience, on any device.</span
							>
						</li>
					</ul>
				</div>
			</div>
		</div>
	</section>

	<!-- Feature Highlights Strip -->
	<section class="border-y border-white/10 bg-[#0d0d20] py-12 sm:py-16">
		<div class="mx-auto max-w-7xl px-4 sm:px-6 lg:px-8">
			<div class="flex flex-wrap items-center justify-center gap-4">
				<div
					class="inline-flex items-center gap-2 rounded-full border border-[#5E4AEF]/50 bg-[#5E4AEF]/10 px-4 py-2 text-sm font-medium text-white"
				>
					<span class="h-2 w-2 rounded-full bg-[#5E4AEF]"></span>
					Web-based WSI viewer
				</div>
				<div
					class="inline-flex items-center gap-2 rounded-full border border-[#FE0E94]/50 bg-[#FE0E94]/10 px-4 py-2 text-sm font-medium text-white"
				>
					<span class="h-2 w-2 rounded-full bg-[#FE0E94]"></span>
					Optimized for teaching
				</div>
				<div
					class="inline-flex items-center gap-2 rounded-full border border-[#5E4AEF]/50 bg-[#5E4AEF]/10 px-4 py-2 text-sm font-medium text-white"
				>
					<span class="h-2 w-2 rounded-full bg-[#5E4AEF]"></span>
					Fast navigation and zoom
				</div>
				<div
					class="inline-flex items-center gap-2 rounded-full border border-[#FE0E94]/50 bg-[#FE0E94]/10 px-4 py-2 text-sm font-medium text-white"
				>
					<span class="h-2 w-2 rounded-full bg-[#FE0E94]"></span>
					Built by a pathology student
				</div>
			</div>
		</div>
	</section>

	<!-- CTA Section -->
	<section class="py-20 sm:py-28">
		<div class="mx-auto max-w-4xl px-4 text-center sm:px-6 lg:px-8">
			<h2 class="mb-6 text-3xl font-bold text-white sm:text-4xl">Start exploring now</h2>
			<p class="mx-auto mb-8 max-w-2xl text-lg text-gray-400">
				Jump into the demo and see how Eosin transforms the way you study and teach pathology.
			</p>
			<a
				href="https://eosin.ai/app/"
				target="_blank"
				rel="noopener noreferrer"
				class="inline-flex items-center justify-center gap-2 rounded-xl bg-[#5E4AEF] px-8 py-4 text-lg font-semibold text-white shadow-lg shadow-[#5E4AEF]/25 transition-all hover:scale-105 hover:bg-[#4e3cd0]"
			>
				Open Demo
				<svg class="h-5 w-5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
					<path
						stroke-linecap="round"
						stroke-linejoin="round"
						stroke-width="2"
						d="M10 6H6a2 2 0 00-2 2v10a2 2 0 002 2h10a2 2 0 002-2v-4M14 4h6m0 0v6m0-6L10 14"
					/>
				</svg>
			</a>
		</div>
	</section>
</main>

<!-- Footer -->
<footer class="border-t border-white/10 bg-[#070710]">
	<div class="mx-auto max-w-7xl px-4 py-12 sm:px-6 lg:px-8">
		<div class="flex flex-col items-center justify-between gap-6 sm:flex-row">
			<div class="flex items-center gap-4">
				<img src="/logo_half.png" alt="Eosin" class="h-6 w-auto opacity-70" />
			</div>
			<div class="flex items-center gap-6 text-sm text-gray-400">
				<a
					href="https://eosin.ai/app/"
					target="_blank"
					rel="noopener noreferrer"
					class="transition-colors hover:text-white"
				>
					Demo
				</a>
				<a href="mailto:contact@eosin.ai" class="transition-colors hover:text-white"> Contact </a>
			</div>
		</div>
		<div class="mt-8 border-t border-white/5 pt-8 text-center text-sm text-gray-500">
			© {new Date().getFullYear()} Eosin.
		</div>
	</div>
</footer>

<style>
	@keyframes fade-in {
		from {
			opacity: 0;
			transform: translateY(20px);
		}
		to {
			opacity: 1;
			transform: translateY(0);
		}
	}

	.animate-fade-in {
		animation: fade-in 0.6s ease-out forwards;
	}

	.animate-fade-in-delayed {
		animation: fade-in 0.6s ease-out 0.2s forwards;
		opacity: 0;
	}

	/* Smooth scrolling for the page */
	:global(html) {
		scroll-behavior: smooth;
	}
</style>
