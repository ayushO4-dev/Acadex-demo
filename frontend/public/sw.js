// Acadex's service worker: just enough to be an installable app that says
// so when it's offline. It never caches pages or API responses: everything
// in Acadex is someone's private data, and a shared device mustn't keep it.
const OFFLINE = "/offline.html";
const CACHE = "acadex-offline-v1";

self.addEventListener("install", (event) => {
  event.waitUntil(caches.open(CACHE).then((cache) => cache.addAll([OFFLINE, "/icons/icon-192.png"])));
  self.skipWaiting();
});

self.addEventListener("activate", (event) => {
  event.waitUntil(
    caches.keys().then((keys) => Promise.all(keys.filter((k) => k !== CACHE).map((k) => caches.delete(k)))),
  );
  self.clients.claim();
});

// Page loads go to the network; only when that fails, the offline page.
self.addEventListener("fetch", (event) => {
  if (event.request.mode !== "navigate") return;
  event.respondWith(fetch(event.request).catch(() => caches.match(OFFLINE)));
});
