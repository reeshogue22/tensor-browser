// tensor-browser service worker
// Intercepts all fetch requests and proxies cross-origin ones through /api/resource

const PROXY = '/api/resource?url=';
const NAV_PROXY = '/api/proxy?url=';
const SELF_ORIGIN = self.location.origin;

self.addEventListener('install', () => self.skipWaiting());
self.addEventListener('activate', (e) => e.waitUntil(self.clients.claim()));

self.addEventListener('fetch', (e) => {
  const url = e.request.url;

  // Skip same-origin and non-http requests
  if (url.startsWith(SELF_ORIGIN) || (!url.startsWith('http://') && !url.startsWith('https://'))) {
    return;
  }

  // Navigate requests (iframe loading a page) — use HTML proxy
  if (e.request.mode === 'navigate') {
    e.respondWith(
      fetch(NAV_PROXY + encodeURIComponent(url))
        .catch(() => new Response('error loading page', { status: 502 }))
    );
    return;
  }

  // All other cross-origin requests (JS, CSS, images, API calls) — use resource proxy
  e.respondWith(
    fetch(PROXY + encodeURIComponent(url))
      .catch(() => new Response('', { status: 502 }))
  );
});
