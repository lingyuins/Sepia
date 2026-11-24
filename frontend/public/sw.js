// Sepia console service worker.
// Caches only the static app shell (same-origin GET assets); /api and /v1
// always hit the gateway so live data, auth and SSE are never served stale.
const CACHE = 'sepia-shell-v2'
// Replaced at build time by vite-pwa-shell.ts with the hashed entry assets.
const PRECACHE_ASSETS = []
const SHELL = ['/', '/index.html', '/manifest.webmanifest', '/favicon.svg', '/icons/icon-192.png', '/icons/icon-512.png', '/icons/icon-maskable-512.png', '/icons/apple-touch-icon.png', ...PRECACHE_ASSETS]
self.addEventListener('install', (event) => {
  event.waitUntil(caches.open(CACHE).then((cache) => cache.addAll(SHELL)).then(() => self.skipWaiting()))
})

self.addEventListener('activate', (event) => {
  event.waitUntil(
    caches.keys()
      .then((keys) => Promise.all(keys.filter((key) => key !== CACHE).map((key) => caches.delete(key))))
      .then(() => self.clients.claim()),
  )
})

self.addEventListener('fetch', (event) => {
  const { request } = event
  if (request.method !== 'GET') return
  const url = new URL(request.url)
  if (url.origin !== self.location.origin) return
  if (url.pathname.startsWith('/api/') || url.pathname.startsWith('/v1/') || url.pathname.startsWith('/v1beta/') || url.pathname.startsWith('/suno/')) return
  if (request.mode === 'navigate') {
    // Network-first so a freshly deployed gateway wins; cached shell when offline.
    event.respondWith(fetch(request).catch(() => caches.match('/index.html').then((cached) => cached || Response.error())))
    return
  }
  // Static assets are content-hashed: cache-first, fill the cache on first use.
  event.respondWith(
    caches.match(request).then((cached) => cached || fetch(request).then((response) => {
      if (response.ok && response.type === 'basic') {
        const copy = response.clone()
        caches.open(CACHE).then((cache) => cache.put(request, copy))
      }
      return response
    })),
  )
})