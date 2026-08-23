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

/**
 * Reports what this handler decided, because nothing here is observable.
 *
 * A notification click runs in the service worker: no console anyone reads, no
 * page to inspect, and the failure the operator reports is "nothing happens",
 * which looks exactly like the handler never running.
 */
async function trace(windows, visible, action, surface, detail) {
  try {
    await fetch("/api/v1/notifications/click-trace", {
      method: "POST",
      credentials: "same-origin",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ windows, visible, action, surface, detail: detail || null }),
    });
  } catch {
    // Tracing must never be the reason a notification fails to open anything.
  }
}

self.addEventListener("notificationclick", (event) => {
  event.notification.close();
  const target = new URL(event.notification.data?.url || "/?surface=decisions", self.location.origin).href;
  const surface = new URL(target).searchParams.get("surface") || "decisions";
  event.waitUntil((async () => {
    let windows = [];
    try {
      windows = await clients.matchAll({ type: "window", includeUncontrolled: true });
    } catch (error) {
      await trace(0, 0, "none", surface, String(error));
    }
    const visible = windows.filter((candidate) => candidate.visibilityState === "visible");
    const client = visible[0] || windows[0];

    if (client) {
      client.postMessage({ type: "swarm-show-surface", surface });
      try {
        // Focus first, while the click's activation is still fresh. Awaiting
        // navigate() before this spent it, and focus() then did nothing at all
        // — which is why the app stopped coming forward when the previous
        // version of this handler shipped.
        await client.focus();
        if ("navigate" in client) {
          try {
            await client.navigate(target);
          } catch {
            // The page was told where to go by the message above.
          }
        }
        await trace(windows.length, visible.length, "focus", surface);
        return;
      } catch (error) {
        // A window that cannot be focused is no better than no window.
        await trace(windows.length, visible.length, "open", surface, String(error));
      }
    }

    try {
      await clients.openWindow(target);
      if (client) return;
      await trace(windows.length, visible.length, "open", surface);
    } catch (error) {
      await trace(windows.length, visible.length, "none", surface, String(error));
    }
  })());
});
