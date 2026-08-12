import { expect, test, vi } from "vitest";

import {
  TerminalController,
  TerminalControllerRegistry,
  type TerminalConnectionLike,
  type TerminalSurface,
} from "./TerminalController";

function fakeSurface(): TerminalSurface {
  return {
    open: vi.fn(),
    focus: vi.fn(),
    fit: vi.fn().mockResolvedValue({ rows: 24, columns: 80 }),
    write: vi.fn().mockResolvedValue(undefined),
    restore: vi.fn().mockResolvedValue(undefined),
    onData: vi.fn(() => ({ dispose: vi.fn() })),
    onResize: vi.fn(() => ({ dispose: vi.fn() })),
    dispose: vi.fn(),
  };
}

test("a requested desktop focus follows the session into its mounted terminal", () => {
  const surface = fakeSurface();
  const controller = new TerminalController(() => surface, fakeConnection);

  controller.requestFocus(true);
  controller.attach(document.createElement("div"));

  expect(surface.focus).toHaveBeenCalledOnce();
});

test("a mobile worker selection focuses the terminal region without opening its keyboard", () => {
  const surface = fakeSurface();
  const controller = new TerminalController(() => surface, fakeConnection);
  const mount = document.createElement("div");
  document.body.append(mount);

  controller.requestFocus(false);
  controller.attach(mount);

  expect(surface.focus).not.toHaveBeenCalled();
  expect(document.activeElement).toBe(mount.querySelector(".terminal-surface"));
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
  expect(connection.resize).toHaveBeenCalledWith(24, 80);
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
  expect(connection.resize).toHaveBeenLastCalledWith(24, 80);

  controller.detach();
  controller.attach(secondMount);

  await vi.waitFor(() => expect(surface.fit).toHaveBeenCalledTimes(2));
  await vi.waitFor(() => expect(connection.resize).toHaveBeenLastCalledWith(38, 132));
  expect(connection.start).toHaveBeenCalledTimes(1);
  expect(connection.dispose).not.toHaveBeenCalled();
  expect(secondMount.children).toHaveLength(1);

  controller.dispose();
});

test("mobile controls use the same terminal input transport", () => {
  const connection = fakeConnection();
  const controller = new TerminalController(fakeSurface, () => connection);

  controller.sendInput("/status\r");

  expect(connection.sendInput).toHaveBeenCalledWith("/status\r");
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
    bytes: new TextEncoder().encode("canonical"),
  };

  await handlers.onSnapshot(snapshot);

  expect(surface.restore).toHaveBeenCalledWith(snapshot);
  expect(surface.fit).toHaveBeenCalledTimes(2);
  expect(connection.resize).toHaveBeenLastCalledWith(38, 132);
  expect(surface.write).not.toHaveBeenCalled();
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
  expect(connection.resize).toHaveBeenCalledWith(38, 132);
  expect(surface.onResize).toHaveBeenCalledTimes(1);
});
