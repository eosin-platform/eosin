import { json, error } from '@sveltejs/kit';
import type { RequestHandler } from './$types';
import { serverRefreshToken } from '$lib/auth/server';

const AUTH_COOKIE_NAME = 'eosin_refresh_token';
const AUTH_EXPIRY_COOKIE_NAME = 'eosin_refresh_expiry';
const DEFAULT_REFRESH_EXPIRY_SECONDS = 30 * 24 * 60 * 60; // 30 days

export const POST: RequestHandler = async ({ request, cookies, url }) => {
	const body = await request.json();
	const { refresh_token } = body;

	if (!refresh_token) {
		throw error(400, { message: 'Refresh token is required' });
	}

	const credentials = await serverRefreshToken(refresh_token);

	if (!credentials) {
		throw error(401, { message: 'Invalid or expired refresh token' });
	}

	// Update cookies with the new refresh token
	if (credentials.jwt.refresh_token) {
		// refresh_expires_in: 0 means offline token (never expires), use 30 days default
		const expirySeconds = credentials.jwt.refresh_expires_in && credentials.jwt.refresh_expires_in > 0
			? credentials.jwt.refresh_expires_in
			: DEFAULT_REFRESH_EXPIRY_SECONDS;
		const expiryMs = Date.now() + expirySeconds * 1000;
		const expiryDate = new Date(expiryMs);
		
		// Detect if we're behind HTTPS
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
	}

	return json(credentials);
};
