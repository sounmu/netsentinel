// Bump the suffix when the service worker cache strategy changes. The
// activate handler below drops every non-matching cache so users do not keep
// stale Next/Turbopack chunks across deploys.
const CACHE_NAME = "netsentinel-v2";

self.addEventListener("install", () => {
  // Skip pre-caching — Cloudflare Access may intercept and redirect
  self.skipWaiting();
});

self.addEventListener("activate", (event) => {
  event.waitUntil(
    caches.keys().then((keys) =>
      Promise.all(keys.filter((k) => k !== CACHE_NAME).map((k) => caches.delete(k)))
    )
  );
  self.clients.claim();
});

self.addEventListener("fetch", (event) => {
  const { request } = event;
  const url = new URL(request.url);

  // Only handle immutable same-origin static assets.
  // Never cache authenticated document/RSC responses.
  if (
    request.method !== "GET" ||
    !url.href.startsWith(self.location.origin) ||
    url.pathname.startsWith("/api/") ||
    request.destination === "document" ||
    url.searchParams.has("_rsc")
  ) {
    return;
  }

  const isStaticAsset =
    url.pathname.startsWith("/_next/static/") ||
    request.destination === "style" ||
    request.destination === "script" ||
    request.destination === "font" ||
    request.destination === "image" ||
    url.pathname === "/manifest.json";

  if (!isStaticAsset) {
    return;
  }

  if (url.pathname.startsWith("/_next/static/")) {
    event.respondWith(
      fetch(request)
        .then((response) => {
          if (response.ok && response.type === "basic") {
            const clone = response.clone();
            caches.open(CACHE_NAME).then((cache) => cache.put(request, clone));
          }
          return response;
        })
        .catch(() =>
          caches.match(request).then((cached) => {
            return cached || new Response("Offline", { status: 503, statusText: "Service Unavailable" });
          })
        )
    );
    return;
  }

  // Network-first strategy for non-hashed static assets: try network, fall back to cache.
  event.respondWith(
    fetch(request)
      .then((response) => {
        // Only cache successful same-origin responses
        if (response.ok && response.type === "basic") {
          const clone = response.clone();
          caches.open(CACHE_NAME).then((cache) => cache.put(request, clone));
        }
        return response;
      })
      .catch(() =>
        caches.match(request).then((cached) => {
          // Return cached response, or a minimal offline fallback
          return cached || new Response("Offline", { status: 503, statusText: "Service Unavailable" });
        })
      )
  );
});
