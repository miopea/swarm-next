import { afterEach, beforeEach, expect, test, vi } from "vitest";

import type { OperatorPresence } from "../api";
import { PresenceController } from "./PresenceController";

const atHive: OperatorPresence = { mode: "at_hive", manual_mode: null, source: "active_device" };
const originalUserAgent = navigator.userAgent;

beforeEach(() => {
  vi.useFakeTimers();
  window.localStorage.clear();
  Object.defineProperty(document, "visibilityState", { configurable: true, value: "visible" });
});

afterEach(() => {
  vi.useRealTimers();
  vi.unstubAllGlobals();
  Object.defineProperty(navigator, "userAgent", { configurable: true, value: originalUserAgent });
});

test("owns one heartbeat and one listener set across repeated starts", async () => {
  const observe = vi.fn().mockResolvedValue(atHive);
  const onPresence = vi.fn();
  const setIntervalSpy = vi.spyOn(window, "setInterval");
  const clearIntervalSpy = vi.spyOn(window, "clearInterval");
  const controller = new PresenceController(observe, () => Date.now());

  controller.start("first", onPresence, vi.fn());
  controller.start("second", onPresence, vi.fn());
  await vi.runAllTicks();

  expect(setIntervalSpy).toHaveBeenCalledTimes(2);
  expect(clearIntervalSpy).toHaveBeenCalledTimes(1);
  expect(observe).toHaveBeenCalledTimes(2);
  window.dispatchEvent(new KeyboardEvent("keydown", { key: "a" }));
  await vi.runAllTicks();
  expect(observe).toHaveBeenCalledTimes(2);

  await vi.advanceTimersByTimeAsync(60_000);
  expect(observe).toHaveBeenCalledTimes(3);
  controller.stop();
  expect(clearIntervalSpy).toHaveBeenCalledTimes(2);
});

test("visibility changes replace pending state without concurrent writes", async () => {
  let release: ((value: OperatorPresence) => void) | undefined;
  const observe = vi.fn().mockImplementation(() => new Promise<OperatorPresence>((resolve) => { release = resolve; }));
  const controller = new PresenceController(observe);
  controller.start("secret", vi.fn(), vi.fn());
  expect(observe).toHaveBeenCalledTimes(1);

  Object.defineProperty(document, "visibilityState", { configurable: true, value: "hidden" });
  document.dispatchEvent(new Event("visibilitychange"));
  expect(observe).toHaveBeenCalledTimes(1);
  release?.(atHive);
  await vi.runAllTicks();
  expect(observe).toHaveBeenCalledTimes(2);
  expect(observe.mock.calls[1]?.[3]).toBe("hidden");
  controller.stop();
});

test("unsupported lock detection stays an optional enhancement", async () => {
  const controller = new PresenceController(vi.fn().mockResolvedValue(atHive));
  const onLockState = vi.fn();
  controller.start("secret", vi.fn(), onLockState);
  expect(onLockState).toHaveBeenCalledWith("unsupported");
  await expect(controller.enableLockDetection()).resolves.toBe(false);
  controller.stop();
});

test("mobile devices never request desktop lock-detection permission", async () => {
  Object.defineProperty(navigator, "userAgent", { configurable: true, value: "Mozilla/5.0 Android Mobile" });
  vi.stubGlobal("IdleDetector", class IdleDetector {});
  const controller = new PresenceController(vi.fn().mockResolvedValue(atHive));
  const onLockState = vi.fn();

  controller.start("secret", vi.fn(), onLockState);

  expect(onLockState).toHaveBeenCalledWith("unsupported");
  await expect(controller.enableLockDetection()).resolves.toBe(false);
  controller.stop();
});

test("granted desktop lock detection becomes enabled and reports locked presence", async () => {
  class FakeIdleDetector extends EventTarget {
    static requestPermission = vi.fn().mockResolvedValue("granted");
    screenState: "locked" | "unlocked" = "locked";
    userState: "active" | "idle" = "active";
    start = vi.fn().mockResolvedValue(undefined);
  }
  vi.stubGlobal("IdleDetector", FakeIdleDetector);
  const observe = vi.fn().mockResolvedValue({ mode: "away", manual_mode: null, source: "screen_locked" });
  const states: string[] = [];
  const controller = new PresenceController(observe);
  controller.start("secret", vi.fn(), (state) => states.push(state));
  await vi.runAllTicks();

  await expect(controller.enableLockDetection()).resolves.toBe(true);
  await vi.runAllTicks();

  expect(FakeIdleDetector.requestPermission).toHaveBeenCalledOnce();
  expect(states).toContain("enabling");
  expect(states.at(-1)).toBe("enabled");
  expect(observe.mock.calls.some((call) => call[3] === "locked")).toBe(true);
  controller.stop();
});
