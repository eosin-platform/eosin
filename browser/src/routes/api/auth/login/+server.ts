import { json, error } from '@sveltejs/kit';
import type { RequestHandler } from './$types';
import { serverLogin } from '$lib/auth/server';

export const POST: RequestHandler = async ({ request }) => {
	const body = await request.json();
	const { username, password } = body;

	if (!username || !password) {
		throw error(400, { message: 'Username and password are required' });
	}

	const credentials = await serverLogin(username, password);

	if (!credentials) {
		throw error(401, { message: 'Invalid username or password' });
	}

	return json(credentials);
};
