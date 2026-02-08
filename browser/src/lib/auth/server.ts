/**
 * Server-side auth utilities for SSR.
 * Uses IAM_ENDPOINT for internal server-to-server communication.
 */

import { env } from '$env/dynamic/private';
import type { UserCredentials } from '$lib/stores/auth';

const AUTH_COOKIE_NAME = 'eosin_refresh_token';
const AUTH_EXPIRY_COOKIE_NAME = 'eosin_refresh_expiry';

export interface RefreshRequest {
	refresh_token: string;
}

/**
 * Parse cookies from the Cookie header
 */
export function parseCookies(cookieHeader: string | null): Record<string, string> {
	if (!cookieHeader) return {};
	const cookies: Record<string, string> = {};
	cookieHeader.split(';').forEach((cookie) => {
		const [name, ...rest] = cookie.split('=');
		const value = rest.join('=').trim();
		if (name) {
			cookies[name.trim()] = decodeURIComponent(value);
		}
	});
	return cookies;
}

/**
 * Get refresh token from cookies
 */
export function getRefreshTokenFromCookies(cookies: Record<string, string>): string | null {
	return cookies[AUTH_COOKIE_NAME] ?? null;
}

/**
 * Get refresh token expiry from cookies
 */
export function getRefreshExpiryFromCookies(cookies: Record<string, string>): number | null {
	const expiry = cookies[AUTH_EXPIRY_COOKIE_NAME];
	if (!expiry) return null;
	const parsed = parseInt(expiry, 10);
	return isNaN(parsed) ? null : parsed;
}

/**
 * Refresh token on the server side using internal IAM endpoint
 */
export async function serverRefreshToken(refreshToken: string): Promise<UserCredentials | null> {
	const iamEndpoint = env.IAM_ENDPOINT;
	if (!iamEndpoint) {
		console.error('IAM_ENDPOINT environment variable is not set');
		return null;
	}

	try {
		const response = await fetch(`${iamEndpoint}/user/refresh`, {
			method: 'POST',
			headers: {
				'Content-Type': 'application/json'
			},
			body: JSON.stringify({ refresh_token: refreshToken } as RefreshRequest)
		});

		if (!response.ok) {
			console.error(`Token refresh failed: ${response.status}`);
			return null;
		}

		const credentials: UserCredentials = await response.json();
		return credentials;
	} catch (error) {
		console.error('Failed to refresh token on server:', error);
		return null;
	}
}

/**
 * Login on the server side using internal IAM endpoint
 */
export async function serverLogin(
	username: string,
	password: string
): Promise<UserCredentials | null> {
	const iamEndpoint = env.IAM_ENDPOINT;
	if (!iamEndpoint) {
		console.error('IAM_ENDPOINT environment variable is not set');
		return null;
	}

	try {
		const response = await fetch(`${iamEndpoint}/user/login`, {
			method: 'POST',
			headers: {
				'Content-Type': 'application/json'
			},
			body: JSON.stringify({ username, password })
		});

		if (!response.ok) {
			const text = await response.text();
			console.error(`Login failed: ${response.status} - ${text}`);
			return null;
		}

		const credentials: UserCredentials = await response.json();
		return credentials;
	} catch (error) {
		console.error('Failed to login on server:', error);
		return null;
	}
}
