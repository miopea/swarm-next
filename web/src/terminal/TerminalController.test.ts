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
    write: vi.fn(),
    onData: vi.fn(() => ({ dispose: vi.fn() })),
    onResize: vi.fn(() => ({ dispose: vi.fn() })),
    dispose: vi.fn(),
  };
}

function fakeConnection(): TerminalConnectionLike {
  return { start: vi.fn(), sendInput: vi.fn(), resize: vi.fn(), dispose: vi.fn() };
}

test("view detach does not dispose, reopen, or reconnect a terminal", () => {
  const surface = fakeSurface();
  const connection = fakeConnection();
  const controller = new TerminalController(() => surface, () => connection);
  const firstMount = document.createElement("div");
  const secondMount = document.createElement("div");

  controller.attach(firstMount);
  controller.detach();
  controller.attach(secondMount);

  expect(surface.open).toHaveBeenCalledTimes(1);
  expect(surface.dispose).not.toHaveBeenCalled();
  expect(connection.start).toHaveBeenCalledTimes(1);
  expect(connection.dispose).not.toHaveBeenCalled();
  expect(secondMount.children).toHaveLength(1);
});

test("switching between sessions keeps both transports attached", () => {
  const registry = new TerminalControllerRegistry();
  const firstConnection = fakeConnection();
  const secondConnection = fakeConnection();
  const first = registry.getOrCreate("session-1", fakeSurface, () => firstConnection);
  const second = registry.getOrCreate("session-2", fakeSurface, () => secondConnection);
  const mount = document.createElement("div");

  first.attach(mount);
  first.detach();
  second.attach(mount);
  second.detach();
  first.attach(mount);

  expect(firstConnection.start).toHaveBeenCalledTimes(1);
  expect(secondConnection.start).toHaveBeenCalledTimes(1);
  expect(firstConnection.dispose).not.toHaveBeenCalled();
  expect(secondConnection.dispose).not.toHaveBeenCalled();
  expect(registry.size).toBe(2);
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
