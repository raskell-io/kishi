// Offline play: serve the app from the cache, refresh it from the network in the background.
const CACHE = 'kishi-v1';
const SHELL = [
  './', 'index.html', 'style.css', 'app.js', 'board.js', 'engine.js', 'worker.js', 'helper.js',
  'kishi.wasm', 'kishi-mt.wasm',
  'icon.svg', 'icon-180.png', 'manifest.webmanifest',
  ...['K', 'Q', 'R', 'B', 'N', 'P'].flatMap((p) => [`pieces/w${p}.svg`, `pieces/b${p}.svg`]),
];

self.addEventListener('install', (e) => {
  e.waitUntil(caches.open(CACHE).then((c) => c.addAll(SHELL)).then(() => self.skipWaiting()));
});

self.addEventListener('activate', (e) => {
  e.waitUntil(
    caches.keys()
      .then((keys) => Promise.all(keys.filter((k) => k !== CACHE).map((k) => caches.delete(k))))
      .then(() => self.clients.claim()),
  );
});

self.addEventListener('fetch', (e) => {
  if (e.request.method !== 'GET' || new URL(e.request.url).origin !== location.origin) return;
  e.respondWith(
    caches.open(CACHE).then(async (cache) => {
      const cached = await cache.match(e.request, { ignoreSearch: true });
      const fresh = fetch(e.request)
        .then((r) => {
          if (r.ok) cache.put(e.request, r.clone());
          return r;
        })
        .catch(() => cached);
      return cached ?? fresh;
    }),
  );
});
