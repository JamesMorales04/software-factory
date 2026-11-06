// Network-only worker: Factory does not cache Core responses or authorize work offline.
self.addEventListener("fetch", (event) => {
  if (event.request.method !== "GET") return;
  event.respondWith(fetch(event.request));
});
