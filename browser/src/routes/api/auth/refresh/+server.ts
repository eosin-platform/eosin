import { json, error } from '@sveltejs/kit';
import type { RequestHandler } from './$types';
import { serverRefreshToken } from '$lib/auth/server';

export const POST: RequestHandler = async ({ request }) => {
	const body = await request.json();
	const { refresh_token } = body;

	if (!refresh_token) {
		throw error(400, { message: 'Refresh token is required' });
	}

	const credentials = await serverRefreshToken(refresh_token);

	if (!credentials) {
		throw error(401, { message: 'Invalid or expired refresh token' });
	}

	return json(credentials);
};
