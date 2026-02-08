<script lang="ts">
	import { loginModalOpen, authStore } from '$lib/stores/auth';
	import { login } from '$lib/auth/client';

	let username = $state('');
	let password = $state('');
	let isSubmitting = $state(false);
	let errorMessage = $state<string | null>(null);
	let toastMessage = $state<string | null>(null);

	function closeModal() {
		loginModalOpen.set(false);
		// Reset form state
		username = '';
		password = '';
		errorMessage = null;
	}

	function showToast(message: string) {
		toastMessage = message;
		setTimeout(() => {
			toastMessage = null;
		}, 3000);
	}

	function handleCreateAccount() {
		showToast('This feature is coming soon!');
	}

	function handleForgotPassword() {
		showToast('This feature is coming soon!');
	}

	async function handleSubmit(event: Event) {
		event.preventDefault();
		if (!username.trim() || !password.trim()) {
			errorMessage = 'Please enter both username and password';
			return;
		}

		isSubmitting = true;
		errorMessage = null;

		try {
			await login({ username: username.trim(), password });
			closeModal();
		} catch (error) {
			errorMessage = error instanceof Error ? error.message : 'Login failed';
		} finally {
			isSubmitting = false;
		}
	}

	function handleKeydown(event: KeyboardEvent) {
		if (event.key === 'Escape') {
			closeModal();
		}
	}
</script>

<svelte:window onkeydown={handleKeydown} />

{#if $loginModalOpen}
	<!-- svelte-ignore a11y_click_events_have_key_events -->
	<!-- svelte-ignore a11y_no_static_element_interactions -->
	<div class="login-overlay" onclick={closeModal}>
		<div class="login-modal" onclick={(e) => e.stopPropagation()}>
			<div class="login-header">
				<h2>Login</h2>
				<button class="login-close" onclick={closeModal} aria-label="Close login">
					<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 20 20" fill="currentColor">
						<path
							d="M6.28 5.22a.75.75 0 00-1.06 1.06L8.94 10l-3.72 3.72a.75.75 0 101.06 1.06L10 11.06l3.72 3.72a.75.75 0 101.06-1.06L11.06 10l3.72-3.72a.75.75 0 00-1.06-1.06L10 8.94 6.28 5.22z"
						/>
					</svg>
				</button>
			</div>

			<form class="login-form" onsubmit={handleSubmit}>
				{#if errorMessage}
					<div class="error-message">
						<svg
							xmlns="http://www.w3.org/2000/svg"
							viewBox="0 0 20 20"
							fill="currentColor"
							class="error-icon"
						>
							<path
								fill-rule="evenodd"
								d="M18 10a8 8 0 11-16 0 8 8 0 0116 0zm-8-5a.75.75 0 01.75.75v4.5a.75.75 0 01-1.5 0v-4.5A.75.75 0 0110 5zm0 10a1 1 0 100-2 1 1 0 000 2z"
								clip-rule="evenodd"
							/>
						</svg>
						<span>{errorMessage}</span>
					</div>
				{/if}

				<div class="form-group">
					<label for="username">Username or Email</label>
					<input
						type="text"
						id="username"
						bind:value={username}
						placeholder="Enter your username or email"
						autocomplete="username"
						disabled={isSubmitting}
					/>
				</div>

				<div class="form-group">
					<label for="password">Password</label>
					<input
						type="password"
						id="password"
						bind:value={password}
						placeholder="Enter your password"
						autocomplete="current-password"
						disabled={isSubmitting}
					/>
				</div>

				<button type="submit" class="submit-btn" disabled={isSubmitting}>
					{#if isSubmitting}
						<span class="spinner"></span>
						Logging in...
					{:else}
						Login
					{/if}
				</button>

				<div class="login-links">
					<button type="button" class="link-btn" onclick={handleForgotPassword}>
						Forgot your password?
					</button>
					<span class="link-divider">•</span>
					<button type="button" class="link-btn" onclick={handleCreateAccount}>
						Create Account
					</button>
				</div>
			</form>
		</div>
	</div>
{/if}

{#if toastMessage}
	<div class="toast">
		{toastMessage}
	</div>
{/if}

<style>
	.login-overlay {
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

	.login-modal {
		display: flex;
		flex-direction: column;
		width: 100%;
		max-width: 400px;
		background: rgba(20, 20, 20, 0.95);
		border: 1px solid rgba(255, 255, 255, 0.15);
		border-radius: 12px;
		overflow: hidden;
	}

	.login-header {
		display: flex;
		align-items: center;
		justify-content: space-between;
		padding: 16px 20px;
		border-bottom: 1px solid rgba(255, 255, 255, 0.1);
	}

	.login-header h2 {
		margin: 0;
		font-size: 18px;
		font-weight: 600;
		color: #fff;
	}

	.login-close {
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
		transition:
			background 0.15s,
			color 0.15s;
	}

	.login-close:hover {
		background: rgba(255, 255, 255, 0.2);
		color: #fff;
	}

	.login-close svg {
		width: 18px;
		height: 18px;
	}

	.login-form {
		display: flex;
		flex-direction: column;
		gap: 16px;
		padding: 20px;
	}

	.form-group {
		display: flex;
		flex-direction: column;
		gap: 6px;
	}

	.form-group label {
		font-size: 13px;
		font-weight: 500;
		color: rgba(255, 255, 255, 0.8);
	}

	.form-group input {
		padding: 10px 12px;
		background: rgba(255, 255, 255, 0.08);
		border: 1px solid rgba(255, 255, 255, 0.15);
		border-radius: 6px;
		font-size: 14px;
		color: #fff;
		transition:
			border-color 0.15s,
			background 0.15s;
	}

	.form-group input::placeholder {
		color: rgba(255, 255, 255, 0.4);
	}

	.form-group input:focus {
		outline: none;
		border-color: #3b82f6;
		background: rgba(255, 255, 255, 0.1);
	}

	.form-group input:disabled {
		opacity: 0.6;
		cursor: not-allowed;
	}

	.error-message {
		display: flex;
		align-items: center;
		gap: 8px;
		padding: 10px 12px;
		background: rgba(239, 68, 68, 0.15);
		border: 1px solid rgba(239, 68, 68, 0.3);
		border-radius: 6px;
		color: #fca5a5;
		font-size: 13px;
	}

	.error-icon {
		width: 16px;
		height: 16px;
		flex-shrink: 0;
	}

	.submit-btn {
		display: flex;
		align-items: center;
		justify-content: center;
		gap: 8px;
		padding: 12px 16px;
		background: #3b82f6;
		border: none;
		border-radius: 6px;
		font-size: 14px;
		font-weight: 600;
		color: #fff;
		cursor: pointer;
		transition:
			background 0.15s,
			transform 0.1s;
	}

	.submit-btn:hover:not(:disabled) {
		background: #2563eb;
	}

	.submit-btn:active:not(:disabled) {
		transform: scale(0.98);
	}

	.submit-btn:disabled {
		opacity: 0.7;
		cursor: not-allowed;
	}

	.spinner {
		width: 16px;
		height: 16px;
		border: 2px solid rgba(255, 255, 255, 0.3);
		border-top-color: #fff;
		border-radius: 50%;
		animation: spin 0.8s linear infinite;
	}

	@keyframes spin {
		to {
			transform: rotate(360deg);
		}
	}

	.login-links {
		display: flex;
		align-items: center;
		justify-content: center;
		gap: 8px;
		padding-top: 8px;
		border-top: 1px solid rgba(255, 255, 255, 0.08);
		flex-wrap: wrap;
	}

	.link-btn {
		background: none;
		border: none;
		padding: 4px 8px;
		font-size: 13px;
		color: #60a5fa;
		cursor: pointer;
		transition: color 0.15s;
	}

	.link-btn:hover {
		color: #93c5fd;
		text-decoration: underline;
	}

	.link-divider {
		color: rgba(255, 255, 255, 0.3);
		font-size: 12px;
	}

	/* Toast notification */
	.toast {
		position: fixed;
		bottom: 24px;
		left: 50%;
		transform: translateX(-50%);
		padding: 12px 24px;
		background: rgba(30, 30, 30, 0.95);
		border: 1px solid rgba(255, 255, 255, 0.15);
		border-radius: 8px;
		color: #fff;
		font-size: 14px;
		z-index: 2000;
		animation: toast-in 0.3s ease-out;
		box-shadow: 0 4px 20px rgba(0, 0, 0, 0.4);
	}

	@keyframes toast-in {
		from {
			opacity: 0;
			transform: translateX(-50%) translateY(20px);
		}
		to {
			opacity: 1;
			transform: translateX(-50%) translateY(0);
		}
	}

	/* Mobile: full-width modal */
	@media (max-width: 480px) {
		.login-overlay {
			padding: 16px;
		}

		.login-modal {
			max-width: 100%;
		}

		.login-links {
			flex-direction: column;
			gap: 4px;
		}

		.link-divider {
			display: none;
		}
	}
</style>
