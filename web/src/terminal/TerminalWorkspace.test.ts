import { expect, test, vi } from "vitest";

import type { TerminalConnectionLike, TerminalSurface } from "./TerminalController";
import { TerminalWorkspace } from "./TerminalWorkspace";

function fakeSurface(): TerminalSurface {
  return {
    open: vi.fn(),
    focus: vi.fn(),
    fit: vi.fn().mockResolvedValue({ rows: 24, columns: 80 }),
    write: vi.fn().mockResolvedValue(undefined),
    restore: vi.fn().mockResolvedValue(undefined),
    onData: vi.fn(() => ({ dispose: vi.fn() })),
  onResize: vi.fn(() => ({ dispose: vi.fn() })),
  onScroll: vi.fn((listener) => { listener(true); return { dispose: vi.fn() }; }),
  scrollToBottom: vi.fn(),
    dispose: vi.fn(),
  };
}

function fakeConnection(): TerminalConnectionLike {
  return { start: vi.fn(), sendInput: vi.fn(), resize: vi.fn(), dispose: vi.fn() };
}

test("renderer recovery reconnects the browser without touching the durable worker", () => {
  const workspace = new TerminalWorkspace();
  const firstSurface = fakeSurface();
  const firstConnection = fakeConnection();
  workspace.authenticate("browser-session-cookie");
  const first = workspace.controllerFor("queen-session", () => firstSurface, () => firstConnection);

  workspace.resetSessionRenderer("queen-session");

  expect(firstSurface.dispose).toHaveBeenCalledOnce();
  expect(firstConnection.dispose).toHaveBeenCalledOnce();
  const second = workspace.controllerFor("queen-session", fakeSurface, fakeConnection);
  expect(second).not.toBe(first);
});
