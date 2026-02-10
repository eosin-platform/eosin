/**
 * Prometheus metrics instrumentation for the SvelteKit browser service.
 * 
 * This module provides:
 * - A separate HTTP server for /metrics, /healthz, and /readyz endpoints
 * - Counters and histograms for tracking request metrics
 * - Automatic Node.js default metrics collection
 */

import http from 'node:http';
import {
	Registry,
	collectDefaultMetrics,
	Counter,
	Histogram,
	Gauge
} from 'prom-client';

// Global registry for all metrics
const registry = new Registry();

// Collect default Node.js metrics (CPU, memory, event loop, etc.)
collectDefaultMetrics({ register: registry });

// Add default labels
const nodeId = process.env.NODE_ID ?? 'unknown';
registry.setDefaultLabels({ node_id: nodeId, service: 'browser' });

// ============================================================================
// Custom Metrics
// ============================================================================

/**
 * Total HTTP requests received by the SvelteKit server
 */
export const httpRequestsTotal = new Counter({
	name: 'sveltekit_http_requests_total',
	help: 'Total number of HTTP requests received',
	labelNames: ['method', 'route', 'status_code'] as const,
	registers: [registry]
});

/**
 * HTTP request duration in seconds
 */
export const httpRequestDuration = new Histogram({
	name: 'sveltekit_http_request_duration_seconds',
	help: 'HTTP request duration in seconds',
	labelNames: ['method', 'route', 'status_code'] as const,
	buckets: [0.001, 0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1, 2.5, 5, 10],
	registers: [registry]
});

/**
 * Server-side page render count
 */
export const pageRendersTotal = new Counter({
	name: 'sveltekit_page_renders_total',
	help: 'Total number of server-side page renders',
	labelNames: ['route'] as const,
	registers: [registry]
});

/**
 * Server-side page render duration in seconds
 */
export const pageRenderDuration = new Histogram({
	name: 'sveltekit_page_render_duration_seconds',
	help: 'Server-side page render duration in seconds',
	labelNames: ['route'] as const,
	buckets: [0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1, 2.5, 5],
	registers: [registry]
});

/**
 * API endpoint calls
 */
export const apiCallsTotal = new Counter({
	name: 'sveltekit_api_calls_total',
	help: 'Total number of API endpoint calls',
	labelNames: ['method', 'endpoint', 'status_code'] as const,
	registers: [registry]
});

/**
 * API endpoint call duration in seconds
 */
export const apiCallDuration = new Histogram({
	name: 'sveltekit_api_call_duration_seconds',
	help: 'API endpoint call duration in seconds',
	labelNames: ['method', 'endpoint'] as const,
	buckets: [0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1, 2.5, 5],
	registers: [registry]
});

/**
 * Authentication events
 */
export const authEventsTotal = new Counter({
	name: 'sveltekit_auth_events_total',
	help: 'Total number of authentication events',
	labelNames: ['event', 'success'] as const,
	registers: [registry]
});

/**
 * External service call duration (calls to meta, IAM, frusta, etc.)
 */
export const externalCallDuration = new Histogram({
	name: 'sveltekit_external_call_duration_seconds',
	help: 'Duration of calls to external services',
	labelNames: ['service', 'endpoint', 'status_code'] as const,
	buckets: [0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1, 2.5, 5, 10],
	registers: [registry]
});

/**
 * External service call errors
 */
export const externalCallErrorsTotal = new Counter({
	name: 'sveltekit_external_call_errors_total',
	help: 'Total number of external service call errors',
	labelNames: ['service', 'endpoint', 'error_type'] as const,
	registers: [registry]
});

/**
 * Active SSR (server-side rendering) requests in progress
 */
export const activeRequests = new Gauge({
	name: 'sveltekit_active_requests',
	help: 'Number of HTTP requests currently being processed',
	registers: [registry]
});

/**
 * Slide list fetches
 */
export const slideListFetchTotal = new Counter({
	name: 'sveltekit_slide_list_fetch_total',
	help: 'Total number of slide list fetches',
	labelNames: ['success'] as const,
	registers: [registry]
});

/**
 * Slide count returned from list operations
 */
export const slideListCount = new Histogram({
	name: 'sveltekit_slide_list_count',
	help: 'Number of slides returned per list operation',
	buckets: [0, 1, 5, 10, 25, 50, 100, 250, 500, 1000],
	registers: [registry]
});

// ============================================================================
// Metrics Server
// ============================================================================

let metricsServer: http.Server | null = null;

/**
 * Start the metrics HTTP server on the specified port.
 * Provides /metrics, /healthz, and /readyz endpoints.
 */
export function startMetricsServer(): void {
	const portStr = process.env.METRICS_PORT;
	if (!portStr) {
		console.log('[Metrics] METRICS_PORT not set, metrics server disabled');
		return;
	}

	const port = parseInt(portStr, 10);
	if (isNaN(port) || port <= 0) {
		console.error(`[Metrics] Invalid METRICS_PORT: ${portStr}`);
		return;
	}

	if (metricsServer) {
		console.log('[Metrics] Server already started');
		return;
	}

	metricsServer = http.createServer(async (req, res) => {
		const url = new URL(req.url ?? '/', `http://${req.headers.host}`);
		const path = url.pathname;

		if (path === '/metrics') {
			try {
				const metrics = await registry.metrics();
				res.writeHead(200, { 'Content-Type': registry.contentType });
				res.end(metrics);
			} catch (err) {
				console.error('[Metrics] Error generating metrics:', err);
				res.writeHead(500, { 'Content-Type': 'text/plain' });
				res.end('Error generating metrics');
			}
		} else if (path === '/healthz') {
			res.writeHead(200, { 'Content-Type': 'text/plain' });
			res.end('ok');
		} else if (path === '/readyz') {
			res.writeHead(200, { 'Content-Type': 'text/plain' });
			res.end('ok');
		} else {
			res.writeHead(404, { 'Content-Type': 'text/plain' });
			res.end('Not Found');
		}
	});

	metricsServer.listen(port, '0.0.0.0', () => {
		console.log(`[Metrics] 📈 Metrics server listening on 0.0.0.0:${port}`);
	});

	// Handle graceful shutdown
	const shutdown = () => {
		if (metricsServer) {
			metricsServer.close(() => {
				console.log('[Metrics] 🛑 Metrics server stopped');
			});
		}
	};

	process.on('SIGTERM', shutdown);
	process.on('SIGINT', shutdown);
}

/**
 * Helper to normalize route paths for metrics labels.
 * Replaces dynamic segments like [id] with :id for consistent labeling.
 */
export function normalizeRoute(route: string | null): string {
	if (!route) return 'unknown';
	// Replace SvelteKit dynamic params like [id] or [...rest] with normalized versions
	return route
		.replace(/\[\.\.\.([^\]]+)\]/g, ':$1*')
		.replace(/\[([^\]]+)\]/g, ':$1');
}
