/**
 * Authentication store for the WSI viewer application.
 *
 * Manages user credentials, token refresh, and session state.
 * Tokens are stored in cookies for SSR access.
 */

import { writable, get } from 'svelte/store';
import { browser } from '$app/environment';

// ============================================================================
// Types
// ============================================================================

export interface JwtLike {
	access_token: string;
	refresh_token?: string;
	token_type?: string;
	expires_in?: number;
	refresh_expires_in?: number;
	id_token?: string;
	scope?: string;
	session_state?: string;
}

export interface UserCredentials {
	id: string;
	username: string;
	first_name: string;
	last_name: string;
	email: string;
	jwt: JwtLike;
}

export interface AuthState {
	user: UserCredentials | null;
	isLoading: boolean;
	error: string | null;
	/** Timestamp when the access token expires (epoch ms) */
	accessTokenExpiry: number | null;
	/** Timestamp when the refresh token expires (epoch ms) */
	refreshTokenExpiry: number | null;
}

// ============================================================================
// Constants
// ============================================================================

const AUTH_COOKIE_NAME = 'eosin_refresh_token';
const AUTH_EXPIRY_COOKIE_NAME = 'eosin_refresh_expiry';
/** Buffer time (in ms) before token expiry to trigger refresh (30 seconds) */
const TOKEN_REFRESH_BUFFER_MS = 30_000;

// ============================================================================
// Helpers
// ============================================================================

function isSecureContext(): boolean {
	if (!browser) return false;
	return window.location.protocol === 'https:';
}

function setCookie(name: string, value: string, expiryMs: number): void {
	if (!browser) return;
	const expires = new Date(expiryMs).toUTCString();
	const secure = isSecureContext();
	let cookieStr = `${name}=${encodeURIComponent(value)}; expires=${expires}; path=/; SameSite=Lax`;
	if (secure) {
		cookieStr += '; Secure';
	}
	console.log('[Auth Cookie] Setting cookie:', { name, valueLen: value.length, expires, expiryMs, secure });
	document.cookie = cookieStr;
	
	// Immediately verify it was set
	const verifyMatch = document.cookie.match(new RegExp(`(^| )${name}=([^;]+)`));
	if (verifyMatch) {
		console.log('[Auth Cookie] Cookie verified, length:', decodeURIComponent(verifyMatch[2]).length);
	} else {
		console.error('[Auth Cookie] Cookie NOT found after setting! Full cookie string:', document.cookie.substring(0, 100));
	}
}

function getCookie(name: string): string | null {
	if (!browser) return null;
	const match = document.cookie.match(new RegExp(`(^| )${name}=([^;]+)`));
	return match ? decodeURIComponent(match[2]) : null;
}

function deleteCookie(name: string): void {
	if (!browser) return;
	const secure = isSecureContext();
	let cookieStr = `${name}=; expires=Thu, 01 Jan 1970 00:00:00 UTC; path=/; SameSite=Lax`;
	if (secure) {
		cookieStr += '; Secure';
	}
	document.cookie = cookieStr;
}

function calculateExpiry(expiresInSeconds: number | undefined): number | null {
	if (!expiresInSeconds) return null;
	return Date.now() + expiresInSeconds * 1000;
}

/** Default refresh token expiry: 30 days */
const DEFAULT_REFRESH_EXPIRY_SECONDS = 30 * 24 * 60 * 60;

function calculateRefreshExpiry(expiresInSeconds: number | undefined): number {
	// If no expiry provided OR is 0 (offline token), default to 30 days
	const seconds = expiresInSeconds && expiresInSeconds > 0 ? expiresInSeconds : DEFAULT_REFRESH_EXPIRY_SECONDS;
	return Date.now() + seconds * 1000;
}

// ============================================================================
// Store
// ============================================================================

const initialState: AuthState = {
	user: null,
	isLoading: false,
	error: null,
	accessTokenExpiry: null,
	refreshTokenExpiry: null
};

function createAuthStore() {
	const { subscribe, set, update } = writable<AuthState>(initialState);

	return {
		subscribe,

		/**
		 * Initialize auth state from SSR data (for client-side hydration).
		 * Also updates cookies with the new refresh token from the server refresh.
		 */
		initialize(credentials: UserCredentials | null, _refreshExpiry: number | null) {
			if (credentials) {
				const accessExpiry = calculateExpiry(credentials.jwt.expires_in);
				// Use the new refresh token expiry from credentials, not the old cookie value
				const refreshExpiry = calculateRefreshExpiry(credentials.jwt.refresh_expires_in);

				// Update cookies with the NEW refresh token from server refresh
				if (credentials.jwt.refresh_token) {
					setCookie(AUTH_COOKIE_NAME, credentials.jwt.refresh_token, refreshExpiry);
					setCookie(AUTH_EXPIRY_COOKIE_NAME, refreshExpiry.toString(), refreshExpiry);
				}

				update((state) => ({
					...state,
					user: credentials,
					accessTokenExpiry: accessExpiry,
					refreshTokenExpiry: refreshExpiry,
					isLoading: false,
					error: null
				}));
			}
		},

		/**
		 * Set credentials after successful login/refresh
		 */
		setCredentials(credentials: UserCredentials) {
			const accessExpiry = calculateExpiry(credentials.jwt.expires_in);
			const refreshExpiry = calculateRefreshExpiry(credentials.jwt.refresh_expires_in);

			console.log('[Auth] setCredentials called:', {
				hasRefreshToken: !!credentials.jwt.refresh_token,
				refreshTokenLength: credentials.jwt.refresh_token?.length,
				refreshExpiry,
				expires_in: credentials.jwt.expires_in,
				refresh_expires_in: credentials.jwt.refresh_expires_in
			});

			// Store refresh token in cookie for SSR
			if (credentials.jwt.refresh_token) {
				setCookie(AUTH_COOKIE_NAME, credentials.jwt.refresh_token, refreshExpiry);
				setCookie(AUTH_EXPIRY_COOKIE_NAME, refreshExpiry.toString(), refreshExpiry);
				console.log('[Auth] Cookies set. Verifying:', {
					tokenCookie: getCookie(AUTH_COOKIE_NAME)?.length,
					expiryCookie: getCookie(AUTH_EXPIRY_COOKIE_NAME)
				});
			}

			update((state) => ({
				...state,
				user: credentials,
				accessTokenExpiry: accessExpiry,
				refreshTokenExpiry: refreshExpiry,
				isLoading: false,
				error: null
			}));
		},

		/**
		 * Clear auth state and cookies
		 */
		logout() {
			deleteCookie(AUTH_COOKIE_NAME);
			deleteCookie(AUTH_EXPIRY_COOKIE_NAME);
			set(initialState);
		},

		/**
		 * Set loading state
		 */
		setLoading(isLoading: boolean) {
			update((state) => ({ ...state, isLoading }));
		},

		/**
		 * Set error state
		 */
		setError(error: string | null) {
			update((state) => ({ ...state, error, isLoading: false }));
		},

		/**
		 * Check if access token needs refresh
		 */
		needsRefresh(): boolean {
			const state = get({ subscribe });
			if (!state.user || !state.accessTokenExpiry) return false;
			return Date.now() + TOKEN_REFRESH_BUFFER_MS >= state.accessTokenExpiry;
		},

		/**
		 * Check if refresh token is still valid
		 */
		canRefresh(): boolean {
			const state = get({ subscribe });
			if (!state.user?.jwt.refresh_token || !state.refreshTokenExpiry) return false;
			return Date.now() < state.refreshTokenExpiry;
		},

		/**
		 * Get current access token (for API calls)
		 */
		getAccessToken(): string | null {
			const state = get({ subscribe });
			return state.user?.jwt.access_token ?? null;
		},

		/**
		 * Get refresh token
		 */
		getRefreshToken(): string | null {
			const state = get({ subscribe });
			return state.user?.jwt.refresh_token ?? null;
		}
	};
}

export const authStore = createAuthStore();

// Also export the login modal state
export const loginModalOpen = writable<boolean>(false);
