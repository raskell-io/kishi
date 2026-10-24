// Offline play: serve the app from the cache, refresh it from the network in the background.
// Also makes the page cross-origin isolated where the host cannot send the
// headers itself (GitHub Pages), so the threaded engine can share memory.
const CACHE = 'kishi-v11';
const SHELL = [
  './', 'index.html', 'style.css', 'app.js', 'board.js', 'shogi-board.js', 'engine.js', 'worker.js', 'helper.js',
  'games/chess.js', 'games/shogi.js', 'games/go.js', 'go-board.js', 'kishi.wasm', 'kishi-mt.wasm', 'shogi.wasm', 'go.wasm',
  'icon.svg', 'icon-180.png', 'manifest.webmanifest', 'fonts/Geist-Variable.woff2', 'fonts/GeistMono-Variable.woff2', 'fonts/kishi-koma.woff',
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

function isolated(response) {
  if (!response || response.status === 0 || response.type === 'opaque') return response;
  const headers = new Headers(response.headers);
  headers.set('Cross-Origin-Opener-Policy', 'same-origin');
  headers.set('Cross-Origin-Embedder-Policy', 'require-corp');
  headers.set('Cross-Origin-Resource-Policy', 'same-origin');
  return new Response(response.body, { status: response.status, statusText: response.statusText, headers });
}

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
      return isolated(cached ?? (await fresh));
    }),
  );
});
