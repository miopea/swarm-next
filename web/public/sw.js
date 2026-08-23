self.addEventListener("install", () => self.skipWaiting());
self.addEventListener("activate", (event) => event.waitUntil(clients.claim()));

self.addEventListener("push", (event) => {
  let payload = {
    title: "Your Hive needs you",
    body: "Open Swarm to review a decision.",
    tag: "swarm-attention",
    url: "/?surface=decisions",
    urgency: "normal",
  };
  try {
    if (event.data) payload = { ...payload, ...event.data.json() };
  } catch {
    // Encrypted payload could not be decoded; the generic alert remains content-free.
  }
  event.waitUntil(self.registration.showNotification(payload.title, {
    body: payload.body,
    tag: payload.tag,
    icon: "/swarm-app-icon-192.png?v=queen-20260812",
    badge: "/swarm-app-icon-192.png?v=queen-20260812",
    renotify: payload.urgency === "time_sensitive",
    data: { url: payload.url },
  }));
});

self.addEventListener("notificationclick", (event) => {
  event.notification.close();
  const target = new URL(event.notification.data?.url || "/?surface=decisions", self.location.origin).href;
  const surface = new URL(target).searchParams.get("surface") || "decisions";
  event.waitUntil((async () => {
    const windows = await clients.matchAll({ type: "window", includeUncontrolled: true });
    const client = windows.find((candidate) => candidate.visibilityState === "visible") || windows[0];
    if (!client) return clients.openWindow(target);
    // Tell the page where to go before trying to navigate it. In an installed
    // PWA client.navigate() is frequently a no-op — the window is not
    // controlled, or the origin already matches — and the only visible effect
    // was focus(): the app came up on whatever surface it was already showing,
    // which read as "the notification takes me to my default page".
    client.postMessage({ type: "swarm-show-surface", surface });
    if ("navigate" in client) {
      try {
        await client.navigate(target);
      } catch {
        // Already handled by the message above.
      }
    }
    return client.focus();
  })());
});
