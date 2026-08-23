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
function loadWorker(options: { focusFails?: boolean; noWindows?: boolean } = {}) {
  const listeners = new Map<string, (event: unknown) => void>();
  const posted: unknown[] = [];
  const opened: string[] = [];
  let focused = false;
  let navigated: string | undefined;

  const order: string[] = [];
  const client = {
    visibilityState: "visible",
    navigate: (url: string) => {
      order.push("navigate");
      navigated = url;
      // What a real installed PWA does: resolves without moving anywhere.
      return Promise.resolve(null);
    },
    postMessage: (message: unknown) => posted.push(message),
    focus: () => {
      order.push("focus");
      if (options.focusFails) return Promise.reject(new Error("not allowed to focus"));
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
      matchAll: () => Promise.resolve(options.noWindows ? [] : [client]),
      claim: () => Promise.resolve(),
      openWindow: (url: string) => {
        opened.push(url);
        return Promise.resolve(null);
      },
    },
  };

  // The handler reports what it did; the report is the subject of one test.
  const traced: { action: string; windows: number; visible: number }[] = [];
  const fetchStub = (_url: string, init: { body: string }) => {
    traced.push(JSON.parse(init.body) as { action: string; windows: number; visible: number });
    return Promise.resolve({ ok: true });
  };

  // eslint-disable-next-line no-new-func -- the shipped worker is the subject.
  new Function("self", "clients", "fetch", workerSource)(scope.self, scope.clients, fetchStub);
  return { listeners, posted, opened, traced, order, state: () => ({ focused, navigated }) };
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

/**
 * The regression the operator hit: "When I click on the notifications, nothing
 * happens. It doesn't open the app."
 *
 * A notification click grants transient user activation, and focus() needs it.
 * Awaiting navigate() first spent it, so focus() silently did nothing and the
 * app never came forward. Focus first, then navigate.
 */
test("focuses the window before trying to navigate it", async () => {
  const worker = loadWorker();
  let pending: Promise<unknown> = Promise.resolve();

  worker.listeners.get("notificationclick")?.({
    notification: { close: () => undefined, data: { url: "/?surface=decisions" } },
    waitUntil: (work: Promise<unknown>) => { pending = work; },
  });
  await pending;

  expect(worker.order).toEqual(["focus", "navigate"]);
  expect(worker.state().focused).toBe(true);
});

/** A window that cannot be focused is no better than no window. */
test("opens a window when the one it found cannot be focused", async () => {
  const worker = loadWorker({ focusFails: true });
  let pending: Promise<unknown> = Promise.resolve();

  worker.listeners.get("notificationclick")?.({
    notification: { close: () => undefined, data: { url: "/?surface=decisions" } },
    waitUntil: (work: Promise<unknown>) => { pending = work; },
  });
  await pending;

  expect(worker.opened).toEqual(["https://swarm.example/?surface=decisions"]);
});

test("opens a window when the app is not running at all", async () => {
  const worker = loadWorker({ noWindows: true });
  let pending: Promise<unknown> = Promise.resolve();

  worker.listeners.get("notificationclick")?.({
    notification: { close: () => undefined, data: { url: "/?surface=tasks" } },
    waitUntil: (work: Promise<unknown>) => { pending = work; },
  });
  await pending;

  expect(worker.opened).toEqual(["https://swarm.example/?surface=tasks"]);
  expect(worker.traced.at(-1)?.action).toBe("open");
});

test("reports what it did, because nothing in a service worker is visible", async () => {
  const worker = loadWorker();
  let pending: Promise<unknown> = Promise.resolve();

  worker.listeners.get("notificationclick")?.({
    notification: { close: () => undefined, data: { url: "/?surface=decisions" } },
    waitUntil: (work: Promise<unknown>) => { pending = work; },
  });
  await pending;

  expect(worker.traced.at(-1)).toMatchObject({ action: "focus", windows: 1, visible: 1 });
});
