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
  expect(register).toHaveBeenCalledWith("/sw.js", { scope: "/", updateViaCache: "none" });
  expect(pushManager.subscribe).toHaveBeenCalledWith(expect.objectContaining({ userVisibleOnly: true }));
  expect(fetchMock).toHaveBeenLastCalledWith(
    expect.stringContaining("/notifications/subscriptions/"),
    expect.objectContaining({ method: "PUT", cache: "no-store" }),
  );
  expect(states.at(-1)).toBe("enabled");
});

test("startup refreshes an existing push worker without requesting permission again", async () => {
  const update = vi.fn().mockResolvedValue(undefined);
  const subscription = {
    toJSON: () => ({ endpoint: "https://fcm.googleapis.com/push/existing", keys: { p256dh: "key", auth: "auth" } }),
  } as unknown as PushSubscription;
  const registration = { update, pushManager: { getSubscription: vi.fn().mockResolvedValue(subscription) } };
  Object.defineProperty(navigator, "serviceWorker", { configurable: true, value: { getRegistration: vi.fn().mockResolvedValue(registration), register: vi.fn() } });
  const fetchMock = vi.fn()
    .mockResolvedValueOnce(ok(settings))
    .mockResolvedValueOnce(ok({ ...settings, subscription_count: 1 }));
  vi.stubGlobal("fetch", fetchMock);

  await new NotificationController().start("token", vi.fn(), vi.fn());

  expect(update).toHaveBeenCalledOnce();
  expect(Notification.requestPermission).not.toHaveBeenCalled();
});

test("startup repairs a missing subscription when this device was intentionally enabled", async () => {
  window.localStorage.setItem("swarm-next.notifications.enabled.v1", "true");
  vi.stubGlobal("Notification", { permission: "granted", requestPermission: vi.fn().mockResolvedValue("granted") });
  const subscription = {
    toJSON: () => ({ endpoint: "https://fcm.googleapis.com/push/repaired", keys: { p256dh: "key", auth: "auth" } }),
  } as unknown as PushSubscription;
  const pushManager = { getSubscription: vi.fn().mockResolvedValue(null), subscribe: vi.fn().mockResolvedValue(subscription) };
  const register = vi.fn().mockResolvedValue({ pushManager });
  Object.defineProperty(navigator, "serviceWorker", { configurable: true, value: { getRegistration: vi.fn().mockResolvedValue(undefined), register } });
  vi.stubGlobal("fetch", vi.fn().mockImplementation(() => Promise.resolve(ok({ ...settings, subscription_count: 1 }))));
  const states: string[] = [];

  await new NotificationController().start("token", vi.fn(), (state) => states.push(state));

  expect(register).toHaveBeenCalledWith("/sw.js", { scope: "/", updateViaCache: "none" });
  expect(pushManager.subscribe).toHaveBeenCalledOnce();
  expect(states.at(-1)).toBe("enabled");
});

test("disable removes delivery but keeps the service worker available for repair", async () => {
  window.localStorage.setItem("swarm-next.notifications.enabled.v1", "true");
  const unsubscribe = vi.fn().mockResolvedValue(true);
  const subscription = {
    unsubscribe,
    toJSON: () => ({ endpoint: "https://fcm.googleapis.com/push/existing", keys: { p256dh: "key", auth: "auth" } }),
  } as unknown as PushSubscription;
  const unregister = vi.fn();
  const registration = { update: vi.fn(), unregister, pushManager: { getSubscription: vi.fn().mockResolvedValue(subscription) } };
  Object.defineProperty(navigator, "serviceWorker", { configurable: true, value: { getRegistration: vi.fn().mockResolvedValue(registration), register: vi.fn() } });
  vi.stubGlobal("fetch", vi.fn().mockImplementation(() => Promise.resolve(ok({ ...settings, subscription_count: 0 }))));
  const controller = new NotificationController();
  await controller.start("token", vi.fn(), vi.fn());

  await controller.disable();

  expect(unsubscribe).toHaveBeenCalledOnce();
  expect(unregister).not.toHaveBeenCalled();
  expect(window.localStorage.getItem("swarm-next.notifications.enabled.v1")).toBeNull();
});

test("an existing browser subscription survives a transient API handoff while enabling", async () => {
  vi.useFakeTimers();
  const subscription = {
    toJSON: () => ({ endpoint: "https://fcm.googleapis.com/push/existing", keys: { p256dh: "key", auth: "auth" } }),
  } as unknown as PushSubscription;
  const registration = { pushManager: { getSubscription: vi.fn().mockResolvedValue(subscription) } };
  Object.defineProperty(navigator, "serviceWorker", { configurable: true, value: {
    getRegistration: vi.fn().mockResolvedValue(undefined),
    register: vi.fn().mockResolvedValue(registration),
  } });
  const fetchMock = vi.fn()
    .mockResolvedValueOnce(ok(settings))
    .mockRejectedValueOnce(new TypeError("gateway restarting"))
    .mockResolvedValueOnce(ok({ ...settings, subscription_count: 1 }));
  vi.stubGlobal("fetch", fetchMock);
  const controller = new NotificationController();
  const states: string[] = [];
  await controller.start("token", vi.fn(), (state) => states.push(state));

  const enabled = controller.enable();
  await vi.advanceTimersByTimeAsync(250);

  await expect(enabled).resolves.toBe(true);
  expect(states.at(-1)).toBe("enabled");
  vi.useRealTimers();
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

test("a test notification targets only the initiating browser device", async () => {
  window.localStorage.setItem("swarm-next.presence-device.v1", "019fedfc-1c30-70e1-a5e2-9a3c94268093");
  Object.defineProperty(navigator, "serviceWorker", { configurable: true, value: { getRegistration: vi.fn().mockResolvedValue(undefined) } });
  const fetchMock = vi.fn().mockImplementation(() => Promise.resolve(ok(settings)));
  vi.stubGlobal("fetch", fetchMock);
  const controller = new NotificationController();
  await controller.start("token", vi.fn(), vi.fn());

  await controller.test();

  expect(fetchMock).toHaveBeenLastCalledWith(
    "/api/v1/notifications/subscriptions/019fedfc-1c30-70e1-a5e2-9a3c94268093/test",
    expect.objectContaining({ method: "POST", cache: "no-store" }),
  );
});

function ok(body: unknown) {
  return new Response(JSON.stringify(body), { status: 200, headers: { "Content-Type": "application/json" } });
}
