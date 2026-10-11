// Copyright (c) 2026 Witalis Domitrz <witekdomitrz@gmail.com>
// AGPL License
// Browser lifecycle/cache plumbing only; all clock and app logic remains Rust.
// No skipWaiting: an update takes over after old app tabs close, avoiding
// mixed-version wasm/bindings during an in-progress game.
// The fetch handler only ever answers for a URL inside this app's own
// directory, checked on every request rather than assumed from the scope.
// Scope is a registration's claim, not a promise, and these PWAs share one
// origin with pages that are not an app. An allowlist scoped to the directory
// is the guard that holds even if the scope is ever wrong. The manifest it
// never answers from the cache at all — it must always be a fresh read,
// because it decides what an install is.
const ROOT = new URL('./', self.location.href);
const CACHE = 'chess-clock-' + ROOT.pathname + '-__VERSION__';
const ASSETS = ['./', 'app.js', 'app_bg.wasm', 'manifest.webmanifest', 'icon-192.png', 'icon-512.png', 'icon.svg', 'index.html'].map(p => new URL(p, ROOT).href);
const IS_OWN = url => url.startsWith(ROOT.href);
// The manifest's URL on its own. It stays in ASSETS — a manifest that 404s
// should still fail the install, loudly — but it is the one precached URL the
// fetch handler will never answer out of the cache (see there).
const MANIFEST = new URL('manifest.webmanifest', ROOT).href;
self.addEventListener('install', event => {
  event.waitUntil(caches.open(CACHE).then(cache => cache.addAll(ASSETS)));
});
self.addEventListener('activate', event => {
  event.waitUntil((async () => {
    const prefix = 'chess-clock-' + ROOT.pathname + '-';
    for (const key of await caches.keys()) {
      if (key.startsWith(prefix) && key !== CACHE) await caches.delete(key);
    }
    await self.clients.claim();
  })());
});
self.addEventListener('fetch', event => {
  // Unrelated pages, API requests, files and blobs are left alone. `IS_OWN`
  // is the second half of that rule, checked per request: scope is a
  // registration's claim, not a promise, and a wrong scope must not let this
  // worker answer for pages it knows nothing about.
  //
  // The manifest is the one own-directory GET that always goes to the network.
  // It is what the browser reads to decide what app an install *is*: while a
  // cached copy answered, a fresh install of this app could read a manifest
  // from before any change to it, and derive the same stale identity the
  // family once shared — the very bug the identity fix was meant to retire.
  // The manifest is tiny; no offline entry point needs it cached.
  const url = event.request.url;
  if (url === MANIFEST) return;
  if (event.request.method !== 'GET' || !IS_OWN(url) || !ASSETS.includes(url)) return;
  event.respondWith((async () => {
    const cache = await caches.open(CACHE);
    const cached = await cache.match(event.request);
    return cached || fetch(event.request);
  })());
});
