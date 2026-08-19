import { expect, test, vi } from "vitest";

import {
  openSurfaceWindow,
  surfaceWindowName,
  surfaceWindowUrl,
} from "./surfaceWindow";

test("opens a surface at the path it was detached from", () => {
  expect(surfaceWindowUrl({ pathname: "/" }, "workers")).toBe("/?surface=workers");
  expect(surfaceWindowUrl({ pathname: "/swarm/" }, "tasks")).toBe("/swarm/?surface=tasks");
});

test("carries only the surface, not the opener's other navigation", () => {
  // A Jira deep link or settings section belongs to the window that holds it.
  expect(surfaceWindowUrl({ pathname: "/" }, "settings")).toBe("/?surface=settings");
});

test("names a window per surface so asking twice focuses one window", () => {
  expect(surfaceWindowName("workers")).toBe("swarm-next-workers");
  expect(surfaceWindowName("workers")).toBe(surfaceWindowName("workers"));
  expect(surfaceWindowName("tasks")).not.toBe(surfaceWindowName("workers"));
});

test("focuses the detached window so a reused one comes forward", () => {
  const focus = vi.fn();
  const open = vi.fn(() => ({ focus }) as unknown as Window);

  expect(openSurfaceWindow("workers", open, { pathname: "/" })).toBe(true);
  expect(open).toHaveBeenCalledWith("/?surface=workers", "swarm-next-workers", expect.any(String));
  expect(focus).toHaveBeenCalledOnce();
});

test("reports a blocked popup rather than pretending a window opened", () => {
  const open = vi.fn(() => null);

  expect(openSurfaceWindow("tasks", open, { pathname: "/" })).toBe(false);
});
