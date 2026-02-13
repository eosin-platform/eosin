/**
 * SvelteKit server hooks for Prometheus metrics instrumentation.
 * 
 * This module instruments all server-side requests with metrics:
 * - HTTP request counts and durations
 * - Page render metrics
 * - API endpoint metrics
 * - Active request gauges
 */

import type { Handle, HandleServerError } from '@sveltejs/kit';
import {
	startMetricsServer,
	httpRequestsTotal,
	httpRequestDuration,
	pageRendersTotal,
	pageRenderDuration,
	apiCallsTotal,
	apiCallDuration,
	activeRequests,
	normalizeRoute
} from '$lib/server/metrics';

// Start the metrics server when the hooks module is loaded
startMetricsServer();

/**
 * Main request handler hook.
 * Instruments all incoming requests with Prometheus metrics.
 */
export const handle: Handle = async ({ event, resolve }) => {
	const startTime = performance.now();
	const method = event.request.method;
	const route = event.route.id ?? event.url.pathname;
	const normalizedRoute = normalizeRoute(route);
	const isApiRoute = route?.startsWith('/api/') ?? event.url.pathname.startsWith('/api/');

	// Track active requests
	activeRequests.inc();

	let statusCode = 200;

	try {
		const response = await resolve(event);
		statusCode = response.status;
		return response;
	} catch (err) {
		// Let SvelteKit handle the error, but track it as 500
		statusCode = 500;
		throw err;
	} finally {
		const durationSeconds = (performance.now() - startTime) / 1000;

		// Decrement active requests
		activeRequests.dec();

		// Record HTTP request metrics
		httpRequestsTotal.inc({
			method,
			route: normalizedRoute,
			status_code: statusCode.toString()
		});

		httpRequestDuration.observe(
			{
				method,
				route: normalizedRoute,
				status_code: statusCode.toString()
			},
			durationSeconds
		);

		// Record route-specific metrics
		if (isApiRoute) {
			// API endpoint metrics
			apiCallsTotal.inc({
				method,
				endpoint: normalizedRoute,
				status_code: statusCode.toString()
			});

			apiCallDuration.observe(
				{
					method,
					endpoint: normalizedRoute
				},
				durationSeconds
			);
		} else if (method === 'GET' && statusCode >= 200 && statusCode < 400) {
			// Page render metrics (only successful GETs that aren't API routes)
			pageRendersTotal.inc({ route: normalizedRoute });
			pageRenderDuration.observe({ route: normalizedRoute }, durationSeconds);
		}
	}
};

/**
 * Server error handler.
 * Logs errors but relies on the main handle hook for metrics.
 */
export const handleServerError: HandleServerError = async ({ error, event, status, message }) => {
	// Log the error for debugging
	console.error(`[Server Error] ${status} - ${event.url.pathname}:`, error);

	// Return a client-safe error message
	return {
		message: message ?? 'An unexpected error occurred'
	};
};
