import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import type { HiveIdentity } from "../api";
import ApiarySettings from "./ApiarySettings";

afterEach(() => { cleanup(); vi.useRealTimers(); vi.unstubAllGlobals(); });

test.each(["keeper", "member"] as const)("%s management cancels superseded reads and times out stalled reads without losing edits", async role => {
  vi.useFakeTimers();
  const requests: { url: string; signal: AbortSignal }[] = [];
  const owned = new Set(["members", "collapse-readiness", "jira-projects", "bindings", "shared-work", "stewardships", "sync-health", "catalog-readiness"]);
  vi.stubGlobal("fetch", vi.fn((input: RequestInfo | URL, init?: RequestInit) => {
    const url = String(input);
    if (url.endsWith("/departure-readiness")) return Promise.reject(new Error("Departure check unavailable in this read-ownership fixture"));
    if (owned.has(url.split("/").at(-1)!)) {
      expect(init?.signal).toBeInstanceOf(AbortSignal);
      const signal = init!.signal!;
      requests.push({ url, signal });
      return new Promise<Response>((_resolve, reject) => signal.addEventListener("abort", () => reject(signal.reason), { once: true }));
    }
    return Promise.resolve({ ok: true, status: 200, json: async () => [] } as Response);
  }));
  const identity: HiveIdentity = { operator: { id: "operator", display_name: "Bea" }, hive: { id: "hive", name: "Test Hive", operator_id: "operator", apiary_id: "garden" }, apiary_context: { mode: "federated", local_role: role, apiary: { id: "garden", name: "Garden", keeper_operator_id: "keeper", shared_work_backend: "jira" } } };
  const props = { busy: false, hiveIdentity: identity, operatorToken: "fictional", onHiveIdentityChange: vi.fn() };
  const view = render(<ApiarySettings {...props} refreshKey="0" />);
  const initial = [...requests];
  expect(initial).toHaveLength(role === "keeper" ? 6 : 3);
  fireEvent.click(screen.getByRole("button", { name: "Edit names" }));
  const name = screen.getByRole("textbox", { name: "Hive name" });
  fireEvent.change(name, { target: { value: "Unsaved name" } });
  await act(async () => { view.rerender(<ApiarySettings {...props} refreshKey="1" />); });
  expect(initial.every(request => request.signal.aborted)).toBe(true);
  const replacement = requests.slice(initial.length);
  expect(replacement).toHaveLength(initial.length);
  expect(replacement.every(request => !request.signal.aborted)).toBe(true);
  expect(name).toHaveValue("Unsaved name");
  await act(async () => { await vi.advanceTimersByTimeAsync(8_000); });
  expect(replacement.every(request => request.signal.aborted && request.signal.reason.name === "TimeoutError")).toBe(true);
  expect(name).toHaveValue("Unsaved name");
  await act(async () => { view.rerender(<ApiarySettings {...props} refreshKey="2" />); });
  const retry = requests.slice(initial.length * 2);
  expect(retry).toHaveLength(initial.length);
  view.unmount();
  expect(retry.every(request => request.signal.aborted)).toBe(true);
  expect(vi.getTimerCount()).toBe(0);
});
