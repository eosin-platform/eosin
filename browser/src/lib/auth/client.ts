/**
 * Auth API client for client-side authentication operations.
 * Uses IAM_PUBLIC_ENDPOINT for browser-side requests.
 */

import { authStore, type UserCredentials } from '$lib/stores/auth';

export interface LoginRequest {
	username: string;
	password: string;
}

export interface RefreshRequest {
	refresh_token: string;
}

/**
 * Login with username and password
 */
export async function login(request: LoginRequest): Promise<UserCredentials> {
	authStore.setLoading(true);
	authStore.setError(null);

	try {
		const response = await fetch('/api/auth/login', {
			method: 'POST',
			headers: {
				'Content-Type': 'application/json'
			},
			body: JSON.stringify(request)
		});

		if (!response.ok) {
			const errorData = await response.json().catch(() => ({}));
			const errorMessage = errorData.message || errorData.error || 'Login failed';
			throw new Error(errorMessage);
		}

		const credentials: UserCredentials = await response.json();
		authStore.setCredentials(credentials);
		return credentials;
	} catch (error) {
		const message = error instanceof Error ? error.message : 'Login failed';
		authStore.setError(message);
		throw error;
	}
}

/**
 * Refresh the access token using the refresh token
 */
export async function refreshToken(refreshTokenValue?: string): Promise<UserCredentials> {
	const token = refreshTokenValue ?? authStore.getRefreshToken();
	if (!token) {
		throw new Error('No refresh token available');
	}

	try {
		const response = await fetch('/api/auth/refresh', {
			method: 'POST',
			headers: {
				'Content-Type': 'application/json'
			},
			body: JSON.stringify({ refresh_token: token })
		});

		if (!response.ok) {
			// If refresh fails, logout the user
			authStore.logout();
			const errorData = await response.json().catch(() => ({}));
			throw new Error(errorData.message || 'Token refresh failed');
		}

		const credentials: UserCredentials = await response.json();
		authStore.setCredentials(credentials);
		return credentials;
	} catch (error) {
		authStore.logout();
		throw error;
	}
}

/**
 * Logout the current user
 */
export function logout(): void {
	authStore.logout();
}

/**
 * Ensure we have a valid access token, refreshing if necessary.
 * Returns the valid access token or null if not authenticated.
 */
export async function ensureValidToken(): Promise<string | null> {
	if (!authStore.canRefresh() && authStore.needsRefresh()) {
		// Token expired and can't refresh
		authStore.logout();
		return null;
	}

	if (authStore.needsRefresh()) {
		try {
			await refreshToken();
		} catch {
			return null;
		}
	}

	return authStore.getAccessToken();
}

/**
 * Make an authenticated fetch request.
 * Automatically refreshes token if needed.
 */
export async function authenticatedFetch(
	url: string,
	options: RequestInit = {}
): Promise<Response> {
	const token = await ensureValidToken();
	if (!token) {
		throw new Error('Not authenticated');
	}

	const headers = new Headers(options.headers);
	headers.set('Authorization', `Bearer ${token}`);

	return fetch(url, {
		...options,
		headers
	});
}
