import { afterEach, beforeEach, expect, test, vi } from "vitest";

import {
  TerminalController,
  TerminalControllerRegistry,
  type TerminalConnectionLike,
  type TerminalSurface,
} from "./TerminalController";

// jsdom reports the document as unfocused. Every case here is the window the
// operator is acting in, except the one that says otherwise.
const documentHasFocus = document.hasFocus.bind(document);
beforeEach(() => {
  document.hasFocus = () => true;
});
afterEach(() => {
  document.hasFocus = documentHasFocus;
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

test("a requested desktop focus follows the session into its mounted terminal", async () => {
  const surface = fakeSurface();
  const controller = new TerminalController(() => surface, fakeConnection);

  controller.requestFocus(true);
  controller.attach(document.createElement("div"));

  await vi.waitFor(() => expect(surface.focus).toHaveBeenCalledOnce());
});

test("a new terminal reapplies operator focus after its transport is ready", async () => {
  const surface = fakeSurface();
  const connection = fakeConnection();
  const controller = new TerminalController(() => surface, () => connection);

  controller.requestFocus(true);
  controller.attach(document.createElement("div"));
  await vi.waitFor(() => expect(connection.start).toHaveBeenCalledOnce());
  expect(surface.focus).toHaveBeenCalledOnce();

  const handlers = vi.mocked(connection.start).mock.calls[0][0];
  handlers.onState("connected");
  expect(surface.focus).toHaveBeenCalledTimes(2);
});

test("a mobile worker selection focuses the terminal region without opening its keyboard", async () => {
  const surface = fakeSurface();
  const controller = new TerminalController(() => surface, fakeConnection);
  const mount = document.createElement("div");
  document.body.append(mount);

  controller.requestFocus(false);
  controller.attach(mount);

  await vi.waitFor(() => expect(document.activeElement).toBe(mount.querySelector(".terminal-surface")));
  expect(surface.focus).not.toHaveBeenCalled();
  mount.remove();
});

test("focus waits through a hidden canonical restore until the terminal is focusable", async () => {
  const surface = fakeSurface();
  const connection = fakeConnection();
  const controller = new TerminalController(() => surface, () => connection);
  const mount = document.createElement("div");
  document.body.append(mount);
  controller.attach(mount);
  await vi.waitFor(() => expect(connection.start).toHaveBeenCalledOnce());
  const host = mount.querySelector<HTMLElement>(".terminal-surface")!;
  host.style.visibility = "hidden";
  const focusWhenVisible = host.focus.bind(host);
  host.focus = vi.fn();

  controller.requestFocus(false);
  expect(document.activeElement).not.toBe(host);
  vi.mocked(surface.restore).mockImplementation(async () => {
    host.style.visibility = "";
    host.focus = focusWhenVisible;
  });
  const handlers = vi.mocked(connection.start).mock.calls[0][0];
  await handlers.onSnapshot({
    sequence: 1,
    rows: 24,
    columns: 80,
    truncated: false,
    reason: "attached" as const,
    bytes: new Uint8Array(),
  });

  expect(document.activeElement).toBe(host);
  mount.remove();
});

function fakeConnection(): TerminalConnectionLike {
  return { start: vi.fn(), sendInput: vi.fn(), resize: vi.fn(), dispose: vi.fn() };
}

test("view detach does not dispose, reopen, or reconnect a terminal", async () => {
  const surface = fakeSurface();
  const connection = fakeConnection();
  const controller = new TerminalController(() => surface, () => connection);
  const firstMount = document.createElement("div");
  const secondMount = document.createElement("div");

  expect(connection.start).not.toHaveBeenCalled();
  controller.attach(firstMount);
  controller.detach();
  controller.attach(secondMount);

  await vi.waitFor(() => expect(connection.start).toHaveBeenCalledTimes(1));

  expect(surface.open).toHaveBeenCalledTimes(1);
  expect(surface.fit).toHaveBeenCalledTimes(1);
  expect(connection.resize).toHaveBeenCalledWith(24, 80, "echo");
  expect(surface.dispose).not.toHaveBeenCalled();
  expect(connection.start).toHaveBeenCalledTimes(1);
  expect(connection.dispose).not.toHaveBeenCalled();
  expect(secondMount.children).toHaveLength(1);
});

test("switching between sessions keeps both transports attached", async () => {
  const registry = new TerminalControllerRegistry();
  const firstConnection = fakeConnection();
  const secondConnection = fakeConnection();
  const first = registry.getOrCreate("session-1", fakeSurface, () => firstConnection);
  const second = registry.getOrCreate("session-2", fakeSurface, () => secondConnection);
  const mount = document.createElement("div");

  first.attach(mount);
  await vi.waitFor(() => expect(firstConnection.start).toHaveBeenCalledTimes(1));
  first.detach();
  second.attach(mount);
  await vi.waitFor(() => expect(secondConnection.start).toHaveBeenCalledTimes(1));
  second.detach();
  first.attach(mount);

  expect(firstConnection.start).toHaveBeenCalledTimes(1);
  expect(secondConnection.start).toHaveBeenCalledTimes(1);
  expect(firstConnection.dispose).not.toHaveBeenCalled();
  expect(secondConnection.dispose).not.toHaveBeenCalled();
  expect(registry.size).toBe(2);
});

test("reattaching a started terminal refits after its new container layout", async () => {
  const surface = fakeSurface();
  vi.mocked(surface.fit)
    .mockResolvedValueOnce({ rows: 24, columns: 80 })
    .mockResolvedValueOnce({ rows: 38, columns: 132 });
  const connection = fakeConnection();
  const controller = new TerminalController(() => surface, () => connection);
  const firstMount = document.createElement("div");
  const secondMount = document.createElement("div");

  controller.attach(firstMount);
  await vi.waitFor(() => expect(connection.start).toHaveBeenCalledTimes(1));
  expect(connection.resize).toHaveBeenLastCalledWith(24, 80, "echo");

  controller.detach();
  controller.attach(secondMount);

  await vi.waitFor(() => expect(surface.fit).toHaveBeenCalledTimes(2));
  await vi.waitFor(() => expect(connection.resize).toHaveBeenLastCalledWith(38, 132, "echo"));
  expect(connection.start).toHaveBeenCalledTimes(1);
  expect(connection.dispose).not.toHaveBeenCalled();
  expect(secondMount.children).toHaveLength(1);

  controller.dispose();
});

test("a transient reattach measurement miss keeps the connected terminal healthy", async () => {
  const surface = fakeSurface();
  vi.mocked(surface.fit)
    .mockResolvedValueOnce({ rows: 24, columns: 80 })
    .mockRejectedValueOnce(new Error("renderer metrics are not ready"));
  const connection = fakeConnection();
  const controller = new TerminalController(() => surface, () => connection);
  const states: string[] = [];
  controller.subscribe((state) => states.push(state));

  controller.attach(document.createElement("div"));
  await vi.waitFor(() => expect(connection.start).toHaveBeenCalledTimes(1));
  vi.mocked(connection.start).mock.calls[0][0].onState("connected");
  controller.detach();
  controller.attach(document.createElement("div"));

  await vi.waitFor(() => expect(surface.fit).toHaveBeenCalledTimes(2));
  expect(states.at(-1)).toBe("connected");
  expect(connection.start).toHaveBeenCalledTimes(1);
  expect(connection.dispose).not.toHaveBeenCalled();
});

test("mobile controls use the same terminal input transport", () => {
  const connection = fakeConnection();
  const controller = new TerminalController(fakeSurface, () => connection);

  controller.sendInput("/status\r");

  expect(connection.sendInput).toHaveBeenCalledWith("/status\r");
});

test("manual redraw preserves the live transport while repainting its surface", async () => {
  const surface = fakeSurface();
  const connection = fakeConnection();
  const controller = new TerminalController(() => surface, () => connection);
  controller.attach(document.createElement("div"));
  await vi.waitFor(() => expect(connection.start).toHaveBeenCalledOnce());
  vi.mocked(surface.fit).mockClear();

  await controller.redraw();

  expect(surface.fit).toHaveBeenCalledOnce();
  expect(connection.start).toHaveBeenCalledOnce();
  expect(connection.dispose).not.toHaveBeenCalled();
});

test("only explicit session close disposes the controller", () => {
  const surface = fakeSurface();
  const connection = fakeConnection();
  const registry = new TerminalControllerRegistry();
  const first = registry.getOrCreate("session-1", () => surface, () => connection);
  const second = registry.getOrCreate("session-1", fakeSurface, fakeConnection);

  expect(first).toBe(second);
  registry.closeSession("session-1");
  expect(surface.dispose).toHaveBeenCalledTimes(1);
  expect(connection.dispose).toHaveBeenCalledTimes(1);
  expect(registry.size).toBe(0);
});

test("canonical snapshots reset the renderer through its controller", async () => {
  const surface = fakeSurface();
  vi.mocked(surface.fit)
    .mockResolvedValueOnce({ rows: 24, columns: 80 })
    .mockResolvedValueOnce({ rows: 38, columns: 132 });
  const connection = fakeConnection();
  const controller = new TerminalController(() => surface, () => connection);
  controller.attach(document.createElement("div"));
  await vi.waitFor(() => expect(connection.start).toHaveBeenCalledTimes(1));
  const handlers = vi.mocked(connection.start).mock.calls[0][0];
  const snapshot = {
    sequence: 9,
    rows: 30,
    columns: 100,
    truncated: false,
    reason: "attached" as const,
    bytes: new TextEncoder().encode("canonical"),
  };

  await handlers.onSnapshot(snapshot);

  expect(surface.restore).toHaveBeenCalledWith(snapshot);
  expect(surface.fit).toHaveBeenCalledTimes(2);
  expect(connection.resize).toHaveBeenLastCalledWith(38, 132, "echo");
  expect(surface.write).not.toHaveBeenCalled();
});

test("an unfocused window accepts the canonical size instead of arguing with it", async () => {
  // A popped-out worker window and the window it came from both restore each
  // other's snapshots, re-fit to their own viewport, and resize back — the
  // terminal adjusts forever. They share one device id, because it lives in
  // localStorage, so the server cannot tell them apart and applies both.
  // Focus picks exactly one window browser-wide.
  const surface = fakeSurface();
  vi.mocked(surface.fit).mockResolvedValue({ rows: 24, columns: 80 });
  const connection = fakeConnection();
  const controller = new TerminalController(() => surface, () => connection);
  controller.attach(document.createElement("div"));
  await vi.waitFor(() => expect(connection.start).toHaveBeenCalledTimes(1));
  const handlers = vi.mocked(connection.start).mock.calls[0][0];
  vi.mocked(connection.resize).mockClear();
  vi.mocked(surface.fit).mockClear();
  document.hasFocus = () => false;

  const snapshot = { sequence: 9, rows: 30, columns: 100, truncated: false, reason: "attached" as const, bytes: new Uint8Array() };
  await handlers.onSnapshot(snapshot);

  // Restored at the size the other window set, and nothing pushed back.
  expect(surface.restore).toHaveBeenCalledWith(snapshot);
  expect(surface.fit).not.toHaveBeenCalled();
  expect(connection.resize).not.toHaveBeenCalled();
});

test("a canonical restore survives transient responsive renderer metrics", async () => {
  const surface = fakeSurface();
  vi.mocked(surface.fit)
    .mockResolvedValueOnce({ rows: 24, columns: 80 })
    .mockRejectedValueOnce(new Error("renderer metrics are not ready"));
  const connection = fakeConnection();
  const controller = new TerminalController(() => surface, () => connection);
  controller.attach(document.createElement("div"));
  await vi.waitFor(() => expect(connection.start).toHaveBeenCalledOnce());
  const handlers = vi.mocked(connection.start).mock.calls[0][0];

  await expect(handlers.onSnapshot({
    sequence: 10,
    rows: 24,
    columns: 80,
    truncated: false,
    reason: "attached" as const,
    bytes: new Uint8Array(),
  })).resolves.toBeUndefined();

  expect(surface.restore).toHaveBeenCalledOnce();
  expect(connection.dispose).not.toHaveBeenCalled();
});

test("transport waits for a post-layout renderer fit", async () => {
  let resolveFit: ((size: { rows: number; columns: number }) => void) | undefined;
  const surface = fakeSurface();
  vi.mocked(surface.fit).mockImplementation(
    () => new Promise((resolve) => {
      resolveFit = resolve;
    }),
  );
  const connection = fakeConnection();
  const controller = new TerminalController(() => surface, () => connection);

  controller.attach(document.createElement("div"));
  expect(surface.open).toHaveBeenCalledTimes(1);
  expect(surface.onResize).not.toHaveBeenCalled();
  expect(connection.start).not.toHaveBeenCalled();

  resolveFit?.({ rows: 38, columns: 132 });
  await vi.waitFor(() => expect(connection.start).toHaveBeenCalledTimes(1));
  expect(connection.resize).toHaveBeenCalledWith(38, 132, "echo");
  expect(surface.onResize).toHaveBeenCalledTimes(1);
});

test("a focused device that has lost the geometry claim stops arguing", async () => {
  // "I had the terminal open on my computer... I went away, opened it on my
  // phone, and on my phone it's constantly jumping."
  //
  // The desktop holds the engagement, so the phone's claim is refused and the
  // PTY keeps the desktop's size. The phone has focus, so focus alone let it
  // re-fit, get refused, receive the canonical snapshot, and re-fit again.
  // Nothing bounded that loop.
  const surface = fakeSurface();
  vi.mocked(surface.fit).mockResolvedValue({ rows: 60, columns: 40 });
  const connection = { ...fakeConnection(), ownsGeometry: false };
  const controller = new TerminalController(() => surface, () => connection);
  controller.attach(document.createElement("div"));
  await vi.waitFor(() => expect(connection.start).toHaveBeenCalledTimes(1));
  const handlers = vi.mocked(connection.start).mock.calls[0][0];
  vi.mocked(connection.resize).mockClear();
  vi.mocked(surface.fit).mockClear();
  document.hasFocus = () => true;

  const snapshot = { sequence: 4, rows: 24, columns: 120, truncated: false, reason: "attached" as const, bytes: new Uint8Array() };
  await handlers.onSnapshot(snapshot);

  expect(surface.restore).toHaveBeenCalledWith(snapshot);
  expect(surface.fit).not.toHaveBeenCalled();
  expect(connection.resize).not.toHaveBeenCalled();
});

test("the device that owns the claim still asserts its own size", async () => {
  const surface = fakeSurface();
  vi.mocked(surface.fit).mockResolvedValue({ rows: 60, columns: 40 });
  const connection = { ...fakeConnection(), ownsGeometry: true };
  const controller = new TerminalController(() => surface, () => connection);
  controller.attach(document.createElement("div"));
  await vi.waitFor(() => expect(connection.start).toHaveBeenCalledTimes(1));
  const handlers = vi.mocked(connection.start).mock.calls[0][0];
  vi.mocked(connection.resize).mockClear();
  vi.mocked(surface.fit).mockClear();
  document.hasFocus = () => true;

  await handlers.onSnapshot({ sequence: 5, rows: 24, columns: 120, truncated: false, reason: "attached" as const, bytes: new Uint8Array() });

  expect(connection.resize).toHaveBeenCalledWith(60, 40, "echo");
});
