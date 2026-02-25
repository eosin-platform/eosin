<script lang="ts">
	let mobileMenuOpen = $state(false);

	const softwareApplicationSchema = {
		'@context': 'https://schema.org',
		'@type': 'SoftwareApplication',
		name: 'Eosin',
		description:
			'Open-source whole-slide imaging (WSI) viewer for digital pathology and pathology education, with WSI annotation and shareable slide links for students and educators.',
		applicationCategory: 'EducationalApplication',
		operatingSystem: 'Web',
		url: 'https://eosin.ai',
		downloadUrl: 'https://github.com/eosin-platform/eosin',
		publisher: {
			'@type': 'Organization',
			name: 'Sable Crest Labs',
			url: 'https://eosin.ai'
		},
		offers: {
			'@type': 'Offer',
			price: '0',
			priceCurrency: 'USD'
		},
		sameAs: ['https://github.com/eosin-platform/eosin']
	};

	function scrollToSection(id: string) {
		const element = document.getElementById(id);
		if (element) {
			element.scrollIntoView({ behavior: 'smooth' });
		}
		mobileMenuOpen = false;
	}
</script>

<svelte:head>
	<title>Eosin – Whole-Slide Imaging (WSI) Viewer for Pathology Education</title>
	<link rel="icon" href="/favicon.png" />
	<meta
		name="description"
		content="Eosin is an open-source whole-slide imaging (WSI) viewer for digital pathology and pathology education. Use WSI annotation and shareable links to teach and study from whole-slide images."
	/>
	<script type="application/ld+json">
{@html JSON.stringify(softwareApplicationSchema)}
	</script>
	<link
		rel="stylesheet"
		href="https://fonts.googleapis.com/css2?family=Inter:wght@500&display=swap"
	/>
</svelte:head>

<!-- Header -->
<header
	class="fixed top-0 right-0 left-0 z-50 border-b border-gray-200 bg-white/95 backdrop-blur-sm"
>
	<nav class="mx-auto flex max-w-6xl items-center justify-between px-4 py-3 sm:px-6 lg:px-8">
		<a href="/" class="flex items-center gap-2">
			<img src="/logo_half.png" alt="Eosin logo" class="h-7 w-auto sm:hidden" />
			<img src="/logo_full.png" alt="Eosin logo" class="hidden h-7 w-auto sm:block" />
			<span
				class="text-base text-gray-900"
				style="font-family: 'Inter', sans-serif; font-weight: 500;">EOSIN</span
			>
		</a>

		<!-- Desktop Navigation -->
		<div class="hidden items-center gap-6 md:flex">
			<button
				onclick={() => scrollToSection('features')}
				class="text-sm text-gray-600 transition-colors hover:text-gray-900"
			>
				Features
			</button>
			<button
				onclick={() => scrollToSection('for-educators')}
				class="text-sm text-gray-600 transition-colors hover:text-gray-900"
			>
				For Educators
			</button>
			<button
				onclick={() => scrollToSection('for-students')}
				class="text-sm text-gray-600 transition-colors hover:text-gray-900"
			>
				For Students
			</button>
			<a
				href="https://github.com/eosin-platform/eosin"
				target="_blank"
				rel="noopener noreferrer"
				class="text-sm text-gray-600 transition-colors hover:text-gray-900"
			>
				GitHub
			</a>
			<a
				href="https://app.eosin.ai"
				target="_blank"
				rel="noopener noreferrer"
				class="rounded-md bg-[#5E4AEF] px-4 py-2 text-sm font-medium text-white transition-colors hover:bg-[#4e3cd0]"
			>
				Open Demo
			</a>
		</div>

		<!-- Mobile menu button -->
		<button
			onclick={() => (mobileMenuOpen = !mobileMenuOpen)}
			class="flex h-10 w-10 items-center justify-center rounded-md text-gray-600 transition-colors hover:bg-gray-100 md:hidden"
			aria-label="Toggle menu"
		>
			{#if mobileMenuOpen}
				<svg class="h-5 w-5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
					<path
						stroke-linecap="round"
						stroke-linejoin="round"
						stroke-width="2"
						d="M6 18L18 6M6 6l12 12"
					/>
				</svg>
			{:else}
				<svg class="h-5 w-5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
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
		<div class="border-t border-gray-200 bg-white px-4 py-4 md:hidden">
			<div class="flex flex-col gap-2">
				<button
					onclick={() => scrollToSection('features')}
					class="rounded-md px-3 py-2 text-left text-gray-600 transition-colors hover:bg-gray-50 hover:text-gray-900"
				>
					Features
				</button>
				<button
					onclick={() => scrollToSection('for-educators')}
					class="rounded-md px-3 py-2 text-left text-gray-600 transition-colors hover:bg-gray-50 hover:text-gray-900"
				>
					For Educators
				</button>
				<button
					onclick={() => scrollToSection('for-students')}
					class="rounded-md px-3 py-2 text-left text-gray-600 transition-colors hover:bg-gray-50 hover:text-gray-900"
				>
					For Students
				</button>
				<a
					href="https://github.com/eosin-platform/eosin"
					target="_blank"
					rel="noopener noreferrer"
					class="rounded-md px-3 py-2 text-left text-gray-600 transition-colors hover:bg-gray-50 hover:text-gray-900"
				>
					GitHub
				</a>
				<a
					href="https://app.eosin.ai"
					target="_blank"
					rel="noopener noreferrer"
					class="rounded-md bg-[#5E4AEF] px-3 py-2 text-center font-medium text-white transition-colors hover:bg-[#4e3cd0]"
				>
					Open Demo
				</a>
			</div>
		</div>
	{/if}
</header>

<main class="min-h-screen bg-white">
	<!-- Hero Section -->
	<section class="bg-white pt-20">
		<div class="mx-auto w-full max-w-6xl px-4 py-16 sm:px-6 sm:py-20 lg:px-8">
			<div class="grid items-center gap-12 lg:grid-cols-2">
				<!-- Text Content -->
				<div class="order-2 lg:order-1">
					<h1 class="mb-4 text-3xl font-semibold tracking-tight text-gray-900 sm:text-4xl">
						Whole-slide imaging (WSI) for pathology education
					</h1>
					<div class="mb-8 max-w-lg space-y-3 text-base leading-relaxed text-gray-600">
						<p>
							Eosin is an open-source whole-slide imaging viewer for digital pathology under Sable
							Crest Labs. It’s built for pathology students and educators who need consistent,
							browser-based access to whole-slide images.
						</p>
						<p>
							Use Eosin to navigate WSI quickly, add structured WSI annotation, and share permalinks
							that open to a specific field of view for teaching and guided study.
						</p>
					</div>

					<div class="flex flex-col gap-3 sm:flex-row">
						<a
							href="https://app.eosin.ai"
							target="_blank"
							rel="noopener noreferrer"
							class="inline-flex items-center justify-center gap-2 rounded-md bg-[#5E4AEF] px-5 py-2.5 text-sm font-medium text-white transition-colors hover:bg-[#4e3cd0]"
						>
							Open Demo
							<svg class="h-4 w-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
								<path
									stroke-linecap="round"
									stroke-linejoin="round"
									stroke-width="2"
									d="M10 6H6a2 2 0 00-2 2v10a2 2 0 002 2h10a2 2 0 002-2v-4M14 4h6m0 0v6m0-6L10 14"
								/>
							</svg>
						</a>
						<button
							onclick={() => scrollToSection('features')}
							class="inline-flex items-center justify-center gap-2 rounded-md px-5 py-2.5 text-sm font-medium text-[#5E4AEF] transition-colors hover:bg-gray-50"
						>
							Learn more
							<svg class="h-4 w-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
								<path
									stroke-linecap="round"
									stroke-linejoin="round"
									stroke-width="2"
									d="M19 9l-7 7-7-7"
								/>
							</svg>
						</button>
					</div>
				</div>

				<!-- Hero Screenshot -->
				<div class="order-1 lg:order-2">
					<div class="overflow-hidden rounded-lg border border-gray-200 shadow-sm">
						<video
							autoplay
							loop
							muted
							playsinline
							aria-label="Screen recording of the Eosin whole-slide imaging (WSI) viewer showing zoom, pan, and WSI annotation"
							title="Eosin whole-slide imaging (WSI) viewer and annotation UI"
							class="h-auto w-full"
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

	<!-- Features Grid -->
	<section id="features" class="bg-gray-50 py-16 sm:py-20">
		<div class="mx-auto max-w-6xl px-4 sm:px-6 lg:px-8">
			<div class="mb-10 text-center">
				<h2 class="mb-3 text-2xl font-semibold text-gray-900">
					Whole-slide imaging viewer features
				</h2>
			</div>

			<div class="grid gap-6 md:grid-cols-3">
				<!-- Feature 1 -->
				<div class="rounded-lg border border-gray-200 bg-white p-6">
					<div class="mb-3 flex h-10 w-10 items-center justify-center rounded-md bg-[#5E4AEF]/10">
						<svg
							class="h-5 w-5 text-[#5E4AEF]"
							fill="none"
							stroke="currentColor"
							viewBox="0 0 24 24"
						>
							<path
								stroke-linecap="round"
								stroke-linejoin="round"
								stroke-width="2"
								d="M21 21l-6-6m2-5a7 7 0 11-14 0 7 7 0 0114 0zM10 7v3m0 0v3m0-3h3m-3 0H7"
							/>
						</svg>
					</div>
					<h3 class="mb-2 text-base font-medium text-gray-900">Fast whole-slide navigation</h3>
					<p class="text-sm leading-relaxed text-gray-600">
						Move from overview to 40× magnification quickly. Smooth zooming and panning across
						gigapixel whole-slide images in the browser.
					</p>
				</div>

				<!-- Feature 2 -->
				<div class="rounded-lg border border-gray-200 bg-white p-6">
					<div class="mb-3 flex h-10 w-10 items-center justify-center rounded-md bg-[#5E4AEF]/10">
						<svg
							class="h-5 w-5 text-[#5E4AEF]"
							fill="none"
							stroke="currentColor"
							viewBox="0 0 24 24"
						>
							<path
								stroke-linecap="round"
								stroke-linejoin="round"
								stroke-width="2"
								d="M11 5H6a2 2 0 00-2 2v11a2 2 0 002 2h11a2 2 0 002-2v-5m-1.414-9.414a2 2 0 112.828 2.828L11.828 15H9v-2.828l8.586-8.586z"
							/>
						</svg>
					</div>
					<h3 class="mb-2 text-base font-medium text-gray-900">WSI annotation for teaching</h3>
					<p class="text-sm leading-relaxed text-gray-600">
						Define regions of interest, attach notes, and share precise fields of view for pathology
						education and review.
					</p>
				</div>

				<!-- Feature 3 -->
				<div class="rounded-lg border border-gray-200 bg-white p-6">
					<div class="mb-3 flex h-10 w-10 items-center justify-center rounded-md bg-[#FE0E94]/10">
						<svg
							class="h-5 w-5 text-[#FE0E94]"
							fill="none"
							stroke="currentColor"
							viewBox="0 0 24 24"
						>
							<path
								stroke-linecap="round"
								stroke-linejoin="round"
								stroke-width="2"
								d="M9.663 17h4.673M12 3v1m6.364 1.636l-.707.707M21 12h-1M4 12H3m3.343-5.657l-.707-.707m2.828 9.9a5 5 0 117.072 0l-.548.547A3.374 3.374 0 0014 18.469V19a2 2 0 11-4 0v-.531c0-.895-.356-1.754-.988-2.386l-.548-.547z"
							/>
						</svg>
					</div>
					<h3 class="mb-2 text-base font-medium text-gray-900">Similarity search (experimental)</h3>
					<p class="text-sm leading-relaxed text-gray-600">
						Optionally find visually similar regions across a slide set to support teaching and
						study.
					</p>
				</div>
			</div>
		</div>
	</section>

	<!-- For Educators -->
	<section id="for-educators" class="bg-white py-16 sm:py-20">
		<div class="mx-auto max-w-6xl px-4 sm:px-6 lg:px-8">
			<div class="grid items-start gap-12 lg:grid-cols-2">
				<div>
					<div
						class="mb-4 inline-flex items-center gap-2 rounded-full bg-[#FE0E94]/10 px-3 py-1 text-sm font-medium text-[#FE0E94]"
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
					<h2 class="mb-4 text-2xl font-semibold text-gray-900">
						WSI annotation tools for teaching pathology
					</h2>
					<p class="mb-6 text-gray-600">
						Use whole-slide imaging in your teaching materials. Share curated slide collections and
						WSI annotations, and link students to an exact field of view for labs, lectures, and
						assessments.
					</p>
					<ul class="space-y-3">
						<li class="flex items-start gap-3 text-gray-600">
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
							<span class="text-sm"
								>Create permalinks to exact fields of view in whole-slide images</span
							>
						</li>
						<li class="flex items-start gap-3 text-gray-600">
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
							<span class="text-sm"
								>Provide consistent access to digital pathology slides across cohorts</span
							>
						</li>
						<li class="flex items-start gap-3 text-gray-600">
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
							<span class="text-sm">Browser-based viewer for classroom, lab, and at-home study</span
							>
						</li>
					</ul>
				</div>

				<div>
					<div
						id="for-students"
						class="mb-4 inline-flex items-center gap-2 rounded-full bg-[#5E4AEF]/10 px-3 py-1 text-sm font-medium text-[#5E4AEF]"
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
					<h2 class="mb-4 text-2xl font-semibold text-gray-900">Digital pathology study tools</h2>
					<p class="mb-6 text-gray-600">
						Open curated slide collections anytime. Follow instructor-shared permalinks to the exact
						field of view, then explore the whole-slide image at your own pace.
					</p>
					<ul class="space-y-3">
						<li class="flex items-start gap-3 text-gray-600">
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
							<span class="text-sm">Study whole-slide images from laptop, tablet, or phone</span>
						</li>
						<li class="flex items-start gap-3 text-gray-600">
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
							<span class="text-sm"
								>Jump directly to instructor-highlighted regions and annotations</span
							>
						</li>
						<li class="flex items-start gap-3 text-gray-600">
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
							<span class="text-sm"
								>Practice making your own notes and WSI annotations (when enabled)</span
							>
						</li>
					</ul>
				</div>
			</div>
		</div>
	</section>

	<!-- CTA Section -->
	<section class="bg-gray-50 py-16 sm:py-20">
		<div class="mx-auto max-w-3xl px-4 text-center sm:px-6 lg:px-8">
			<h2 class="mb-4 text-2xl font-semibold text-gray-900">Try the whole-slide imaging demo</h2>
			<p class="mx-auto mb-6 max-w-2xl text-gray-600">
				Viewer access is unrestricted. Free registration required for annotation/analysis tools.
			</p>
			<a
				href="https://app.eosin.ai"
				target="_blank"
				rel="noopener noreferrer"
				class="inline-flex items-center justify-center gap-2 rounded-md bg-[#5E4AEF] px-6 py-3 text-sm font-medium text-white transition-colors hover:bg-[#4e3cd0]"
			>
				Open Demo
				<svg class="h-4 w-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
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
<footer class="border-t border-gray-200 bg-white">
	<div class="mx-auto max-w-6xl px-4 py-8 sm:px-6 lg:px-8">
		<div class="flex flex-col items-center justify-between gap-4 sm:flex-row">
			<div class="flex items-center gap-4">
				<img src="/logo_half.png" alt="Eosin logo" class="h-5 w-auto opacity-60" />
			</div>
			<div class="flex items-center gap-6 text-sm text-gray-500">
				<a
					href="https://app.eosin.ai"
					target="_blank"
					rel="noopener noreferrer"
					class="transition-colors hover:text-gray-900"
				>
					Viewer
				</a>
				<a
					href="https://github.com/eosin-platform/eosin"
					target="_blank"
					rel="noopener noreferrer"
					class="transition-colors hover:text-gray-900"
				>
					GitHub
				</a>
				<a href="mailto:sablecrestlabs@protonmail.com" class="transition-colors hover:text-gray-900"
					>Contact</a
				>
			</div>
		</div>
		<div class="mt-6 border-t border-gray-100 pt-6 text-center text-sm text-gray-400">
			© {new Date().getFullYear()}
			<a
				class="font-medium text-gray-500 transition-colors hover:text-gray-900"
				href="https://thavlik.dev"
				target="_blank"
				rel="noopener noreferrer">Thomas Havlik</a
			>
		</div>
	</div>
</footer>

<style>
	/* Smooth scrolling for the page */
	:global(html) {
		scroll-behavior: smooth;
	}
</style>
