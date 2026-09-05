import { afterEach, expect, test, vi } from "vitest";

import {
  detachedSurface,
  focusDetachedSurface,
  forgetDetachedSurfaces,
  openSurfaceWindow,
  surfaceIsDetached,
  surfaceWindowName,
  surfaceWindowUrl,
} from "./surfaceWindow";

afterEach(forgetDetachedSurfaces);

function fakeWindow() {
  return { focus: vi.fn(), closed: false } as unknown as Window & { focus: ReturnType<typeof vi.fn>; closed: boolean };
}

test("carries only the surface into the detached window's address", () => {
  expect(surfaceWindowUrl({ pathname: "/" }, "tasks")).toBe("/?surface=tasks&detached=1");
  expect(surfaceWindowName("tasks")).toBe("swarm-next-tasks");
});

test("a window opened for a surface is remembered as holding it", () => {
  // Two copies of one surface is the outcome to avoid, so the opener has to
  // know what it has already detached.
  const opened = fakeWindow();
  expect(surfaceIsDetached("tasks")).toBe(false);

  expect(openSurfaceWindow("tasks", () => opened, { pathname: "/" })).toBe(true);

  expect(surfaceIsDetached("tasks")).toBe(true);
  expect(surfaceIsDetached("workers")).toBe(false);
});

test("brings an already-detached surface forward instead of drawing it twice", () => {
  const opened = fakeWindow();
  openSurfaceWindow("tasks", () => opened, { pathname: "/" });
  opened.focus.mockClear();

  expect(focusDetachedSurface("tasks")).toBe(true);
  expect(opened.focus).toHaveBeenCalledOnce();
  // Nothing to bring forward for a surface that was never detached.
  expect(focusDetachedSurface("workers")).toBe(false);
});

test("forgets a window the operator closed", () => {
  // Browsers report closure only when asked and never announce it, so a closed
  // window must not keep a surface reserved forever.
  const opened = fakeWindow();
  openSurfaceWindow("tasks", () => opened, { pathname: "/" });

  opened.closed = true;

  expect(surfaceIsDetached("tasks")).toBe(false);
  expect(focusDetachedSurface("tasks")).toBe(false);
});

test("a blocked popup reserves nothing", () => {
  expect(openSurfaceWindow("tasks", () => null, { pathname: "/" })).toBe(false);
  expect(surfaceIsDetached("tasks")).toBe(false);
});

test("reads which surface this window was detached to show", () => {
  expect(detachedSurface({ search: "?surface=workers&detached=1" })).toBe("workers");
  expect(detachedSurface({ search: "" })).toBeUndefined();
  expect(detachedSurface({ search: "?surface=nonsense&detached=1" })).toBeUndefined();
});

test.each(["decisions", "queues", "tasks", "workers", "apiary", "settings"] as const)(
  "round trips the detached %s address",
  (surface) => {
    const url = new URL(surfaceWindowUrl({ pathname: "/" }, surface), "https://example.test");
    expect(detachedSurface(url)).toBe(surface);
  },
);

test("a notification deep link opens the whole control room, not a detached window", () => {
  // `surface` on its own already means "open Swarm here", which is what a
  // notification carries. Treating that as a detach request would land the
  // operator in a window with no navigation and no way out of it.
  expect(detachedSurface({ search: "?surface=decisions" })).toBeUndefined();
});
