// Service Worker for Eosin PWA
// Minimal service worker for PWA installation support
// No caching - app updates frequently

const CACHE_NAME = 'eosin-v1';

// Install event - skip waiting for immediate activation
self.addEventListener('install', (event) => {
  console.log('[SW] Installing service worker');
  self.skipWaiting();
});

// Activate event - claim all clients immediately
self.addEventListener('activate', (event) => {
  console.log('[SW] Activating service worker');
  event.waitUntil(
    Promise.all([
      // Claim all clients so the SW takes control immediately
      self.clients.claim(),
      // Clean up any old caches
      caches.keys().then((cacheNames) => {
        return Promise.all(
          cacheNames
            .filter((name) => name !== CACHE_NAME)
            .map((name) => caches.delete(name))
        );
      })
    ])
  );
});

// Fetch event - network-first strategy, no caching
// This ensures users always get the latest content
self.addEventListener('fetch', (event) => {
  // Only handle same-origin requests
  if (!event.request.url.startsWith(self.location.origin)) {
    return;
  }

  // Skip non-GET requests
  if (event.request.method !== 'GET') {
    return;
  }

  // Network-first: always try network, fall back to cache for offline support
  event.respondWith(
    fetch(event.request)
      .then((response) => {
        // Return network response directly without caching
        return response;
      })
      .catch(() => {
        // If network fails, try cache as fallback
        return caches.match(event.request);
      })
  );
});

// Handle messages from the client
self.addEventListener('message', (event) => {
  if (event.data && event.data.type === 'SKIP_WAITING') {
    self.skipWaiting();
  }
});
