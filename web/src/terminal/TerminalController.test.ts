import { expect, test, vi } from "vitest";

import { TerminalController, TerminalControllerRegistry, type TerminalSurface } from "./TerminalController";

function fakeSurface(): TerminalSurface {
  return { open: vi.fn(), dispose: vi.fn() };
}

test("view detach does not dispose or reopen a terminal", () => {
  const surface = fakeSurface();
  const controller = new TerminalController(() => surface);
  const firstMount = document.createElement("div");
  const secondMount = document.createElement("div");

  controller.attach(firstMount);
  controller.detach();
  controller.attach(secondMount);

  expect(surface.open).toHaveBeenCalledTimes(1);
  expect(surface.dispose).not.toHaveBeenCalled();
  expect(secondMount.children).toHaveLength(1);
});

test("only explicit session close disposes the controller", () => {
  const surface = fakeSurface();
  const registry = new TerminalControllerRegistry();
  const first = registry.getOrCreate("session-1", () => surface);
  const second = registry.getOrCreate("session-1", fakeSurface);

  expect(first).toBe(second);
  expect(registry.size).toBe(1);
  registry.closeSession("session-1");
  expect(surface.dispose).toHaveBeenCalledTimes(1);
  expect(registry.size).toBe(0);
});
