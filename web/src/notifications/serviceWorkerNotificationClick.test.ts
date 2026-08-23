import { expect, test } from "vitest";

import workerSource from "../../public/sw.js?raw";

/**
 * Exercises the real `public/sw.js` notificationclick handler.
 *
 * "When I open the notification for something needing my attention, it just
 * takes me to whatever my default page is." The handler called
 * `client.navigate(target)` and then `client.focus()`. In an installed PWA
 * navigate() is routinely a no-op — the window is not controlled by the worker,
 * or the origin already matches — so focus() was the only thing that happened
 * and the app came up on whatever surface it was already showing.
 *
 * Loading the shipped file rather than a copy is the point: this broke without
 * anything failing, and a paraphrase of the handler would have kept passing.
 */
function loadWorker() {
  const listeners = new Map<string, (event: unknown) => void>();
  const posted: unknown[] = [];
  const opened: string[] = [];
  let focused = false;
  let navigated: string | undefined;

  const client = {
    visibilityState: "visible",
    navigate: (url: string) => {
      navigated = url;
      // What a real installed PWA does: resolves without moving anywhere.
      return Promise.resolve(null);
    },
    postMessage: (message: unknown) => posted.push(message),
    focus: () => {
      focused = true;
      return Promise.resolve(client);
    },
  };

  const scope = {
    self: {
      addEventListener: (name: string, handler: (event: unknown) => void) => listeners.set(name, handler),
      location: { origin: "https://swarm.example" },
      registration: { showNotification: () => Promise.resolve() },
    },
    clients: {
      matchAll: () => Promise.resolve([client]),
      claim: () => Promise.resolve(),
      openWindow: (url: string) => {
        opened.push(url);
        return Promise.resolve(null);
      },
    },
  };

  // eslint-disable-next-line no-new-func -- the shipped worker is the subject.
  new Function("self", "clients", workerSource)(scope.self, scope.clients);
  return { listeners, posted, opened, state: () => ({ focused, navigated }) };
}

test("a tapped notification tells the open window which surface to show", async () => {
  const worker = loadWorker();
  let pending: Promise<unknown> = Promise.resolve();

  worker.listeners.get("notificationclick")?.({
    notification: { close: () => undefined, data: { url: "/?surface=decisions" } },
    waitUntil: (work: Promise<unknown>) => { pending = work; },
  });
  await pending;

  expect(worker.posted).toEqual([{ type: "swarm-show-surface", surface: "decisions" }]);
  expect(worker.state().focused).toBe(true);
});

test("a notification with no surface still asks for the attention queue", async () => {
  const worker = loadWorker();
  let pending: Promise<unknown> = Promise.resolve();

  worker.listeners.get("notificationclick")?.({
    notification: { close: () => undefined, data: {} },
    waitUntil: (work: Promise<unknown>) => { pending = work; },
  });
  await pending;

  expect(worker.posted).toEqual([{ type: "swarm-show-surface", surface: "decisions" }]);
});
