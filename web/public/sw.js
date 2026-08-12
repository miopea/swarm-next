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
    icon: "/bee-notification.svg",
    badge: "/bee-notification.svg",
    renotify: payload.urgency === "time_sensitive",
    data: { url: payload.url },
  }));
});

self.addEventListener("notificationclick", (event) => {
  event.notification.close();
  const target = new URL(event.notification.data?.url || "/?surface=decisions", self.location.origin).href;
  event.waitUntil((async () => {
    const windows = await clients.matchAll({ type: "window", includeUncontrolled: true });
    for (const client of windows) {
      if ("navigate" in client) await client.navigate(target);
      return client.focus();
    }
    return clients.openWindow(target);
  })());
});