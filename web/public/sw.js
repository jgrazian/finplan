/*
 * FinPlan's service worker (spec 19, phase 5): the app opens offline once it
 * has been loaded, because plans and the engine live in the browser.
 *
 * Deliberately conservative:
 *  - never touches `/api/*`, and never anything that is not a GET from this
 *    origin: plan data and the session are not cached here;
 *  - navigations are network-first, so a deploy is seen at once, and fall back
 *    to the last page cached (any tab path is the same client app);
 *  - `/_next/static/*` is content-hashed and immutable (the JS, CSS, fonts,
 *    workers and the WASM engine all live there): cache-first.
 * It is registered only by a production build with local mode on.
 */
const VERSION = "v1";
const SHELL = `finplan-shell-${VERSION}`;
const STATIC = `finplan-static-${VERSION}`;

self.addEventListener("install", (event) => {
  // The shell is cached with the first visit's navigation too; this makes the
  // very first load of the page available offline as soon as the worker runs.
  event.waitUntil(
    caches
      .open(SHELL)
      .then((cache) => cache.add(new Request("/", { cache: "reload" })))
      .catch(() => undefined)
      .then(() => self.skipWaiting()),
  );
});

self.addEventListener("activate", (event) => {
  event.waitUntil(
    caches
      .keys()
      .then((keys) =>
        Promise.all(
          keys
            .filter((key) => key.startsWith("finplan-") && key !== SHELL && key !== STATIC)
            .map((key) => caches.delete(key)),
        ),
      )
      .then(() => self.clients.claim()),
  );
});

self.addEventListener("fetch", (event) => {
  const request = event.request;
  if (request.method !== "GET") return;
  const url = new URL(request.url);
  if (url.origin !== self.location.origin) return;
  if (url.pathname.startsWith("/api/")) return;

  if (url.pathname.startsWith("/_next/static/")) {
    event.respondWith(cacheFirst(request));
    return;
  }
  if (request.mode === "navigate") {
    event.respondWith(networkFirstPage(request));
  }
});

async function cacheFirst(request) {
  const cache = await caches.open(STATIC);
  const hit = await cache.match(request);
  if (hit) return hit;
  const response = await fetch(request);
  if (response.ok) cache.put(request, response.clone());
  return response;
}

async function networkFirstPage(request) {
  const cache = await caches.open(SHELL);
  try {
    const response = await fetch(request);
    if (response.ok) cache.put(request, response.clone());
    return response;
  } catch (error) {
    // Offline: this exact page if it was seen, else any cached tab of the same app.
    const hit = (await cache.match(request)) ?? (await cache.match("/"));
    if (hit) return hit;
    throw error;
  }
}
