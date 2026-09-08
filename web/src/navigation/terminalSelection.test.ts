import { afterEach, expect, test, vi } from "vitest";
import type { Worker, SessionSummary } from "../api";
import { reconcileTerminalSelection, restoreTerminalSelection, saveTerminalSelection, selectTerminal } from "./terminalSelection";

const worker = (id: string, session: string | null): Worker => ({ id, name: id, hive_id: "hive", role: id === "queen" ? "queen" : "worker", provider: "claude_code", workspace: `/${id}`, autostart: false, position: 0, active_session_id: session, running: session !== null, attention_state: "resting", created_at: 1, updated_at: 1 });
const sessions = (...ids: string[]): SessionSummary[] => ids.map((session_id) => ({ session_id, running: true }));
afterEach(() => { vi.restoreAllMocks(); window.localStorage.clear(); window.sessionStorage.clear(); });

test("another tab's shared choice cannot redirect this tab on reload", () => {
  const workers = [worker("queen", "q"), worker("daisy", "new")];
  saveTerminalSelection({ workerId: "daisy", sessionId: "old" });
  window.localStorage.setItem("swarm-next.terminal-selection.v2", JSON.stringify({ workerId: "queen", sessionId: "q" }));
  expect(restoreTerminalSelection(workers, sessions("q", "new"))).toEqual({ workerId: "daisy", sessionId: "new" });
});

test("a new tab seeds from the shared preference once, then owns its restoration", () => {
  const workers = [worker("queen", "q"), worker("daisy", "d")];
  window.localStorage.setItem("swarm-next.terminal-selection.v2", JSON.stringify({ workerId: "daisy" }));
  expect(restoreTerminalSelection(workers, sessions("q", "d"))).toEqual({ workerId: "daisy", sessionId: "d" });
  window.localStorage.setItem("swarm-next.terminal-selection.v2", JSON.stringify({ workerId: "queen" }));
  expect(restoreTerminalSelection(workers, sessions("q", "d"))).toEqual({ workerId: "daisy", sessionId: "d" });
});

test("corrupt tab state falls back without replacing a valid preference with garbage", () => {
  window.sessionStorage.setItem("swarm-next.terminal-selection.v2", "{invalid");
  window.localStorage.setItem("swarm-next.terminal-selection.v2", JSON.stringify({ workerId: "daisy" }));
  expect(restoreTerminalSelection([worker("daisy", "d")], sessions("d"))).toEqual({ workerId: "daisy", sessionId: "d" });
});

test("unavailable shared storage does not break this tab's preference", () => {
  saveTerminalSelection({ workerId: "daisy" });
  vi.spyOn(window, "localStorage", "get").mockImplementation(() => { throw new Error("denied"); });
  expect(restoreTerminalSelection([worker("queen", "q"), worker("daisy", "d")], sessions("q", "d"))).toEqual({ workerId: "daisy", sessionId: "d" });
});

test("worker selection survives an empty engine and follows only its replacement", () => {
  let selection = selectTerminal("old", [worker("daisy", "old")]);
  selection = reconcileTerminalSelection(selection, [worker("queen", null), worker("daisy", null)], []);
  expect(selection).toEqual({ workerId: "daisy", sessionId: undefined });
  selection = reconcileTerminalSelection(selection, [worker("queen", "q"), worker("daisy", null)], sessions("q"));
  expect(selection.sessionId).toBeUndefined();
  selection = reconcileTerminalSelection(selection, [worker("queen", "q"), worker("daisy", "new")], sessions("q", "new"));
  expect(selection).toEqual({ workerId: "daisy", sessionId: "new" });
});

test("session metadata alone cannot restore input until its session exists", () => {
  const selection = { workerId: "daisy", sessionId: "old" };
  expect(reconcileTerminalSelection(selection, [worker("daisy", "new")], [])).toEqual({ workerId: "daisy", sessionId: undefined });
});

test("an explicit different selection wins over a late worker return", () => {
  const workers = [worker("queen", "q"), worker("daisy", "new")];
  expect(reconcileTerminalSelection(selectTerminal("q", workers), workers, sessions("q", "new"))).toEqual({ workerId: "queen", sessionId: "q" });
});

test("removal falls back while an unconfigured live session remains selectable", () => {
  const workers = [worker("queen", "q")];
  expect(reconcileTerminalSelection({ workerId: "removed" }, workers, sessions("q"))).toEqual({ workerId: "queen", sessionId: "q" });
  expect(reconcileTerminalSelection({ sessionId: "legacy" }, workers, sessions("q", "legacy"))).toEqual({ sessionId: "legacy" });
});

test("reload during a gap preserves worker identity and restores a replacement", () => {
  saveTerminalSelection({ workerId: "daisy" });
  expect(restoreTerminalSelection([worker("queen", "q"), worker("daisy", null)], sessions("q"))).toEqual({ workerId: "daisy", sessionId: undefined });
  expect(restoreTerminalSelection([worker("daisy", "new")], sessions("new"))).toEqual({ workerId: "daisy", sessionId: "new" });
});

test("legacy session preferences migrate only through current worker bindings", () => {
  window.localStorage.setItem("swarm-next.active-session.v1", "old");
  expect(restoreTerminalSelection([worker("daisy", "old")], sessions("old"))).toEqual({ workerId: "daisy", sessionId: "old" });
});

test("unchanged observations preserve identity and optional storage may fail", () => {
  const selection = { workerId: "daisy", sessionId: "live" };
  expect(reconcileTerminalSelection(selection, [worker("daisy", "live")], sessions("live"))).toBe(selection);
  vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => { throw new Error("denied"); });
  expect(() => saveTerminalSelection(selection)).not.toThrow();
});
