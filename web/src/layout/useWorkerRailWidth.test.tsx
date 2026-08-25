import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, expect, test } from "vitest";

import { useWorkerRailWidth } from "./useWorkerRailWidth";

beforeEach(() => localStorage.clear());
afterEach(cleanup);

function Harness() {
  const rail = useWorkerRailWidth();
  return <div role="separator" aria-valuenow={rail.width} tabIndex={0} onPointerDown={rail.start} onPointerMove={rail.move} onPointerUp={rail.finish} onPointerCancel={rail.finish} onKeyDown={rail.resizeWithKeyboard}>{rail.resizing ? "resizing" : "idle"}</div>;
}

test("persists pointer and keyboard resizing within the worker-rail bounds", () => {
  render(<Harness />);
  const separator = screen.getByRole("separator");

  fireEvent.pointerDown(separator, { pointerId: 1, clientX: 286 });
  fireEvent(separator, pointerEvent("pointermove", 390));
  expect(separator).toHaveAttribute("aria-valuenow", "390");
  fireEvent(separator, pointerEvent("pointerup", 390));
  expect(localStorage.getItem("swarm-next.rail-width.v1")).toBe("390");

  fireEvent.keyDown(separator, { key: "ArrowRight" });
  expect(separator).toHaveAttribute("aria-valuenow", "406");
  expect(localStorage.getItem("swarm-next.rail-width.v1")).toBe("406");
});

/**
 * A rail nobody has dragged is the DEFAULT width, not the minimum.
 *
 * localStorage.getItem returns null when unset, Number(null) is 0, and
 * Number.isFinite(0) is true — so the old guard accepted it and clamped 0 up to
 * the 220px floor. Measured in the running app at eleven viewport widths from
 * 600 to 1440: the rail was 220px at every one, while --rail-width still read
 * 286px, which is why it looked like a styling problem rather than a value one.
 */
test("a rail nobody has dragged opens at the default, not the minimum", () => {
  render(<Harness />);

  expect(screen.getByRole("separator")).toHaveAttribute("aria-valuenow", "286");
});

test("an empty stored value is an absence, not a zero", () => {
  // Number("") is 0 as well, so this took the same wrong branch, while "abc"
  // correctly fell through to the default — two spellings of the same absence
  // handled differently.
  localStorage.setItem("swarm-next.rail-width.v1", "");
  render(<Harness />);

  expect(screen.getByRole("separator")).toHaveAttribute("aria-valuenow", "286");
});

test("unreadable storage costs the width, never the page", () => {
  // This runs inside a useState initialiser, so a throw here took the whole
  // control room down — reproduced in the running app as "Swarm hit a problem
  // drawing this view" with no shell rendered at all. A blocked site-data
  // policy or a private window is enough. A rail width must never be able to
  // do that.
  const original = Object.getOwnPropertyDescriptor(window, "localStorage");
  Object.defineProperty(window, "localStorage", {
    configurable: true,
    get() { throw new DOMException("denied", "SecurityError"); },
  });
  try {
    expect(() => render(<Harness />)).not.toThrow();
    expect(screen.getByRole("separator")).toHaveAttribute("aria-valuenow", "286");
  } finally {
    if (original) Object.defineProperty(window, "localStorage", original);
  }
});

test("a cancelled drag is not a chosen width", () => {
  // finish() is wired to pointercancel as well as pointerup, and a cancelled
  // pointer carries clientX 0 — which clamped to the 220px minimum and was
  // written to storage as though it had been dragged there. Losing a pointer to
  // a system gesture silently narrowed the rail and made it stick.
  localStorage.setItem("swarm-next.rail-width.v1", "400");
  render(<Harness />);
  const separator = screen.getByRole("separator");

  fireEvent.pointerDown(separator, { pointerId: 1, clientX: 400 });
  fireEvent(separator, pointerEvent("pointercancel", 0));

  expect(localStorage.getItem("swarm-next.rail-width.v1")).toBe("400");
  expect(separator).toHaveAttribute("aria-valuenow", "400");
});

function pointerEvent(type: string, clientX: number) {
  const event = new Event(type, { bubbles: true });
  Object.defineProperties(event, {
    clientX: { value: clientX },
    pointerId: { value: 1 },
  });
  return event;
}
