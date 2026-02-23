import { redirect, type Handle } from '@sveltejs/kit';

export const handle: Handle = async ({ event, resolve }) => {
	const url = event.url;

	// Redirect to app.eosin.ai if v= or slide= query params are present
	if (url.searchParams.has('v') || url.searchParams.has('slide')) {
		throw redirect(302, `https://app.eosin.ai${url.pathname}${url.search}`);
	}

	return resolve(event);
};
