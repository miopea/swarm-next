import { expect, test, vi } from "vitest";

import type { TerminalConnectionLike, TerminalSurface } from "./TerminalController";
import { TerminalWorkspace } from "./TerminalWorkspace";
import { terminalDraft } from "./TerminalDraft";

test("renderer reload preserves the one draft while logout clears it", () => {
  const workspace = new TerminalWorkspace();
  workspace.authenticate("operator");
  terminalDraft.clear();
  terminalDraft.update("session-a", "Unsent thought");
  workspace.resetSessionRenderer("session-a");
  expect(terminalDraft.snapshot().draft?.text).toBe("Unsent thought");
  workspace.logout();
  expect(terminalDraft.snapshot().draft).toBeUndefined();
});

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

/**
 * "When I click 'Work here' it should refresh the terminal so the size is
 * updated correctly."
 *
 * Taking a worker moves the geometry claim to this device, but the terminal
 * keeps whatever size the device you took it from had set until something
 * re-fits. On a phone that made the button look like it had done nothing.
 */
test("taking a worker refits its terminal to this device", async () => {
  const workspace = new TerminalWorkspace();
  const surface = fakeSurface();
  const connection = fakeConnection();
  workspace.authenticate("browser-session-cookie");
  const controller = workspace.controllerFor("queen-session", () => surface, () => connection);
  controller.attach(document.createElement("div"));
  await vi.waitFor(() => expect(connection.start).toHaveBeenCalledTimes(1));
  vi.mocked(surface.fit).mockClear();
  vi.mocked(connection.resize).mockClear();
  vi.mocked(surface.fit).mockResolvedValue({ rows: 50, columns: 40 });

  workspace.redrawSession("queen-session");

  // Taking a worker is an operator action, so this one claims.
  await vi.waitFor(() => expect(connection.resize).toHaveBeenCalledWith(50, 40, "operator"));
});

test("taking a worker whose terminal is not mounted here is harmless", () => {
  const workspace = new TerminalWorkspace();
  workspace.authenticate("browser-session-cookie");

  expect(() => workspace.redrawSession("never-opened")).not.toThrow();
});

test("authoritative session replacement releases only inactive old browser resources", () => {
  const workspace = new TerminalWorkspace();
  workspace.authenticate("operator");
  const oldSurface = fakeSurface();
  const oldConnection = fakeConnection();
  workspace.controllerFor("old", () => oldSurface, () => oldConnection);
  const current = workspace.controllerFor("current", fakeSurface, fakeConnection);
  workspace.reconcileSessions(["current"]);
  expect(oldSurface.dispose).toHaveBeenCalledOnce();
  expect(oldConnection.dispose).toHaveBeenCalledOnce();
  expect(workspace.controllerFor("current", fakeSurface, fakeConnection)).toBe(current);
  expect(workspace.rendererRetention.retained).toBe(1);
  workspace.logout();
});

test("ended terminal remains readable while attached and retires on detach", () => {
  const workspace = new TerminalWorkspace();
  workspace.authenticate("operator");
  const surface = fakeSurface();
  let renderable: ((visible: boolean) => void) | undefined;
  surface.onRenderable = (listener) => { renderable = listener; return { dispose: vi.fn() }; };
  const connection = fakeConnection();
  const controller = workspace.controllerFor("ended", () => surface, () => connection);
  controller.attach(document.createElement("div"));
  terminalDraft.update("ended", "Keep my unsent text");
  workspace.reconcileSessions([]);
  expect(surface.dispose).not.toHaveBeenCalled();
  renderable?.(false);
  expect(surface.dispose).not.toHaveBeenCalled();
  renderable?.(true);
  controller.detach();
  expect(surface.dispose).toHaveBeenCalledOnce();
  expect(connection.dispose).toHaveBeenCalledOnce();
  expect(workspace.rendererRetention.retained).toBe(0);
  expect(terminalDraft.snapshot().draft?.text).toBe("Keep my unsent text");
  workspace.logout();
});

test("one hundred provider incarnations do not accumulate inactive renderers", () => {
  const workspace = new TerminalWorkspace();
  workspace.authenticate("operator");
  for (let index = 0; index < 100; index += 1) {
    const id = `session-${index}`;
    workspace.controllerFor(id, fakeSurface, fakeConnection);
    workspace.reconcileSessions([id]);
    expect(workspace.rendererRetention.retained).toBe(1);
  }
  workspace.logout();
});
