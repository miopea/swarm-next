import { act, cleanup, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import type { HiveIdentity } from "../api";
import KeeperControlRoom from "./KeeperControlRoom";
import MemberControlRoom from "./MemberControlRoom";

afterEach(() => { cleanup(); vi.unstubAllGlobals(); vi.restoreAllMocks(); });

test.each(["keeper", "member"] as const)("%s refreshes renamed Hives on return without periodic polling", async (role) => {
  let visibility: DocumentVisibilityState = "visible";
  vi.spyOn(document, "visibilityState", "get").mockImplementation(() => visibility);
  const intervals = vi.spyOn(window, "setInterval");
  let hiveName = "Before rename";
  let stalled = false;
  let cancelled = false;
  const fetchMock = vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
    const url = String(input);
    if (url.endsWith("/members")) {
      if (stalled) return new Promise<Response>((_resolve, reject) => {
        init?.signal?.addEventListener("abort", () => { cancelled = true; reject(new DOMException("Cancelled", "AbortError")); }, { once: true });
      });
      return ok([{ hive_id: "other", hive_name: hiveName, operator_id: "other", operator_display_name: "Bea", role: "keeper", is_local: false }]);
    }
    if (url.endsWith("/sync-health")) return ok({ condition: "current", last_attempt_at: 1, last_success_at: 1, consecutive_failures: 0, next_attempt_at: null });
    if (url.endsWith("/catalog-readiness")) return ok({ acknowledgement: null, jira_connection: "not_connected", projects: [], blockers: [] });
    if (url.endsWith("/task-sync-status")) return ok({ cursor: 0, task_count: 0, last_applied_at: 1 });
    if (url.endsWith("/task-outbox-status")) return ok({ queued_count: 0, conflict_count: 0, rejected_count: 0, last_attempt_at: null });
    if (url.endsWith("/my-stewardship")) return ok(null);
    if (url.endsWith("/steward/assists")) return ok({ incoming: [], outbox: [] });
    return ok([]);
  });
  vi.stubGlobal("fetch", fetchMock);
  const identity: HiveIdentity = { operator: { id: "local", display_name: "Cora" }, hive: { id: "local", name: "Local", operator_id: "local", apiary_id: "apiary" }, apiary_context: { mode: "federated", local_role: role, apiary: { id: "apiary", name: "Garden", keeper_operator_id: "other", shared_work_backend: "jira" } } };
  const View = role === "keeper" ? KeeperControlRoom : MemberControlRoom;
  let view!: ReturnType<typeof render>;
  await act(async () => { view = render(<View identity={identity} operatorToken="fictional" onManage={vi.fn()} onInvite={vi.fn()} onOpenTasks={vi.fn()} />); });
  expect(screen.getAllByText("Before rename").length).toBeGreaterThan(0);
  expect(intervals).not.toHaveBeenCalled();
  hiveName = "Explicit refresh rename";
  await act(async () => { view.rerender(<View identity={identity} operatorToken="fictional" onManage={vi.fn()} onInvite={vi.fn()} onOpenTasks={vi.fn()} refreshKey="1" />); });
  expect(screen.getAllByText("Explicit refresh rename").length).toBeGreaterThan(0);
  expect(screen.queryByText("Before rename")).not.toBeInTheDocument();
  visibility = "hidden";
  await act(async () => { document.dispatchEvent(new Event("visibilitychange")); });
  const reads = fetchMock.mock.calls.length;
  await act(async () => { view.rerender(<View identity={identity} operatorToken="fictional" onManage={vi.fn()} onInvite={vi.fn()} onOpenTasks={vi.fn()} refreshKey="2" />); });
  expect(fetchMock).toHaveBeenCalledTimes(reads);
  await act(async () => { document.dispatchEvent(new Event("visibilitychange")); });
  expect(fetchMock).toHaveBeenCalledTimes(reads);
  hiveName = "After rename";
  visibility = "visible";
  await act(async () => { document.dispatchEvent(new Event("visibilitychange")); });
  await screen.findAllByText("After rename");
  expect(screen.queryByText("Before rename")).not.toBeInTheDocument();
  stalled = true;
  await act(async () => { document.dispatchEvent(new Event("visibilitychange")); });
  const pendingReads = fetchMock.mock.calls.length;
  await act(async () => { document.dispatchEvent(new Event("visibilitychange")); });
  expect(fetchMock).toHaveBeenCalledTimes(pendingReads);
  view.unmount();
  await waitFor(() => expect(cancelled).toBe(true));
});

function ok(value: unknown) { return { ok: true, status: 200, json: async () => value } as Response; }
