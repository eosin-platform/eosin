import { json, error } from '@sveltejs/kit';
import type { RequestHandler } from './$types';
import { serverLogin } from '$lib/auth/server';

const AUTH_COOKIE_NAME = 'eosin_refresh_token';
const AUTH_EXPIRY_COOKIE_NAME = 'eosin_refresh_expiry';
const DEFAULT_REFRESH_EXPIRY_SECONDS = 30 * 24 * 60 * 60; // 30 days

export const POST: RequestHandler = async ({ request, cookies, url }) => {
	const body = await request.json();
	const { username, password } = body;

	if (!username || !password) {
		throw error(400, { message: 'Username and password are required' });
	}

	const credentials = await serverLogin(username, password);

	if (!credentials) {
		throw error(401, { message: 'Invalid username or password' });
	}

	// Set cookies on server-side to ensure they're properly set for SSR
	if (credentials.jwt.refresh_token) {
		// refresh_expires_in: 0 means offline token (never expires), use 30 days default
		const expirySeconds = credentials.jwt.refresh_expires_in && credentials.jwt.refresh_expires_in > 0
			? credentials.jwt.refresh_expires_in
			: DEFAULT_REFRESH_EXPIRY_SECONDS;
		const expiryMs = Date.now() + expirySeconds * 1000;
		const expiryDate = new Date(expiryMs);
		
		// Detect if we're behind HTTPS (check X-Forwarded-Proto header or URL)
		const forwardedProto = request.headers.get('x-forwarded-proto');
		const isSecure = forwardedProto === 'https' || url.protocol === 'https:';
		
		cookies.set(AUTH_COOKIE_NAME, credentials.jwt.refresh_token, {
			path: '/',
			httpOnly: false,
			sameSite: 'lax',
			secure: isSecure,
			expires: expiryDate
		});
		cookies.set(AUTH_EXPIRY_COOKIE_NAME, expiryMs.toString(), {
			path: '/',
			httpOnly: false,
			sameSite: 'lax',
			secure: isSecure,
			expires: expiryDate
		});

		console.log('[SSR Auth] Login successful, cookies set for user:', credentials.username);
	}

	return json(credentials);
};
