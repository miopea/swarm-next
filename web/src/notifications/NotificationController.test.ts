import { afterEach, beforeEach, expect, test, vi } from "vitest";

import { NotificationController } from "./NotificationController";

const settings = { policy: "important_only", subscription_count: 0, vapid_public_key: "BAM" } as const;

beforeEach(() => {
  window.localStorage.clear();
  vi.stubGlobal("Notification", { permission: "default", requestPermission: vi.fn().mockResolvedValue("granted") });
  vi.stubGlobal("PushManager", class PushManager {});
});

afterEach(() => vi.unstubAllGlobals());

test("does not register a service worker or request permission during startup", async () => {
  const getRegistration = vi.fn().mockResolvedValue(undefined);
  const register = vi.fn();
  Object.defineProperty(navigator, "serviceWorker", { configurable: true, value: { getRegistration, register } });
  vi.stubGlobal("fetch", vi.fn().mockResolvedValue(ok(settings)));
  const states: string[] = [];

  await new NotificationController().start("token", vi.fn(), (state) => states.push(state));

  expect(getRegistration).toHaveBeenCalledWith("/");
  expect(register).not.toHaveBeenCalled();
  expect(Notification.requestPermission).not.toHaveBeenCalled();
  expect(states).toContain("available");
});

test("explicit enable registers the push-only worker and persists a browser subscription", async () => {
  const subscription = {
    toJSON: () => ({ endpoint: "https://fcm.googleapis.com/push/one", keys: { p256dh: "key", auth: "auth" } }),
  } as unknown as PushSubscription;
  const pushManager = { getSubscription: vi.fn().mockResolvedValue(null), subscribe: vi.fn().mockResolvedValue(subscription) };
  const register = vi.fn().mockResolvedValue({ pushManager });
  Object.defineProperty(navigator, "serviceWorker", { configurable: true, value: { getRegistration: vi.fn().mockResolvedValue(undefined), register } });
  const fetchMock = vi.fn()
    .mockResolvedValueOnce(ok(settings))
    .mockResolvedValueOnce(ok({ ...settings, subscription_count: 1 }));
  vi.stubGlobal("fetch", fetchMock);
  const controller = new NotificationController();
  const states: string[] = [];
  await controller.start("token", vi.fn(), (state) => states.push(state));

  expect(await controller.enable()).toBe(true);
  expect(register).toHaveBeenCalledWith("/sw.js", { scope: "/" });
  expect(pushManager.subscribe).toHaveBeenCalledWith(expect.objectContaining({ userVisibleOnly: true }));
  expect(fetchMock).toHaveBeenLastCalledWith(
    expect.stringContaining("/notifications/subscriptions/"),
    expect.objectContaining({ method: "PUT", cache: "no-store" }),
  );
  expect(states.at(-1)).toBe("enabled");
});

test("a denied browser permission does not register or persist anything", async () => {
  vi.stubGlobal("Notification", { permission: "default", requestPermission: vi.fn().mockResolvedValue("denied") });
  const register = vi.fn();
  Object.defineProperty(navigator, "serviceWorker", { configurable: true, value: { getRegistration: vi.fn().mockResolvedValue(undefined), register } });
  vi.stubGlobal("fetch", vi.fn().mockResolvedValue(ok(settings)));
  const controller = new NotificationController();
  const states: string[] = [];
  await controller.start("token", vi.fn(), (state) => states.push(state));

  expect(await controller.enable()).toBe(false);
  expect(register).not.toHaveBeenCalled();
  expect(states.at(-1)).toBe("denied");
});

function ok(body: unknown) {
  return new Response(JSON.stringify(body), { status: 200, headers: { "Content-Type": "application/json" } });
}