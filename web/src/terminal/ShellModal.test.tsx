import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";

import ShellModal from "./ShellModal";

afterEach(cleanup);

/**
 * jsdom has no PointerEvent, so testing-library falls back to a plain Event and
 * every mouse property is dropped — a drag fired without this computes NaN
 * offsets and moves nothing, which looks exactly like a broken component.
 *
 * MouseEvent already carries clientX/clientY/button; the only thing it lacks is
 * pointerId. Defined through defineProperty rather than assignment so no type
 * suppression is needed.
 */
class TestPointerEvent extends MouseEvent {
  readonly pointerId: number;

  constructor(type: string, init: PointerEventInit = {}) {
    super(type, init);
    this.pointerId = init.pointerId ?? 1;
  }
}

if (!("PointerEvent" in window)) {
  Object.defineProperty(window, "PointerEvent", { value: TestPointerEvent, configurable: true });
}

/** jsdom implements neither pointer capture method. */
function stubPointerCapture(element: HTMLElement) {
  element.setPointerCapture = vi.fn();
  element.releasePointerCapture = vi.fn();
  element.hasPointerCapture = vi.fn(() => true);
}

function header() {
  return screen.getByText("Shell").parentElement as HTMLElement;
}

test("the window closes from its own button", () => {
  const onClose = vi.fn();
  render(<ShellModal title="Shell" subtitle="Petal's workspace" onClose={onClose}><div /></ShellModal>);

  fireEvent.click(screen.getByRole("button", { name: "Close" }));

  expect(onClose).toHaveBeenCalledTimes(1);
});

/**
 * Dragging the header moves the window, which is what the operator asked for:
 * "It should be resizeable and moveable."
 *
 * Resizing is deliberately not asserted here — it is the browser's own
 * `resize: both`, so there is no code of ours to test and a test would only be
 * checking that jsdom applies a stylesheet.
 */
test("dragging the header moves the window", () => {
  render(<ShellModal title="Shell" subtitle="Petal's workspace" onClose={vi.fn()}><div /></ShellModal>);
  const bar = header();
  stubPointerCapture(bar);
  const dialog = screen.getByRole("dialog");
  expect(dialog.style.left).toBe("");

  fireEvent.pointerDown(bar, { button: 0, clientX: 500, clientY: 300, pointerId: 1 });
  fireEvent.pointerMove(bar, { button: 0, clientX: 540, clientY: 340, pointerId: 1 });

  expect(dialog.style.position).toBe("fixed");
  expect(dialog.style.left).not.toBe("");
});

/** A window dragged off screen could not be reached to close it again. */
test("the header cannot be dragged out of reach", () => {
  render(<ShellModal title="Shell" subtitle="Petal's workspace" onClose={vi.fn()}><div /></ShellModal>);
  const bar = header();
  stubPointerCapture(bar);
  const dialog = screen.getByRole("dialog");

  fireEvent.pointerDown(bar, { button: 0, clientX: 500, clientY: 300, pointerId: 1 });
  fireEvent.pointerMove(bar, { button: 0, clientX: -100_000, clientY: -100_000, pointerId: 1 });

  expect(Number.parseFloat(dialog.style.top)).toBeGreaterThanOrEqual(0);
  expect(Number.parseFloat(dialog.style.left)).toBeGreaterThan(-100_000);
});

/** A press that starts on the close button is a click, not a drag. */
test("pressing Close does not start a drag", () => {
  const onClose = vi.fn();
  render(<ShellModal title="Shell" subtitle="Petal's workspace" onClose={onClose}><div /></ShellModal>);
  const bar = header();
  stubPointerCapture(bar);
  const close = screen.getByRole("button", { name: "Close" });

  fireEvent.pointerDown(close, { button: 0, clientX: 500, clientY: 300, pointerId: 1 });
  fireEvent.pointerMove(bar, { button: 0, clientX: 700, clientY: 500, pointerId: 1 });

  expect(screen.getByRole("dialog").style.left).toBe("");
});

/**
 * The window must escape its parent, and this is the test that would have caught
 * both defects the operator reported.
 *
 * Rendered in place it lands inside `.workspace`, which carries
 * `contain: layout paint` and `isolation: isolate` — added deliberately to stop
 * xterm's accelerated canvas painting over a newly mounted view. Both bite a
 * floating window: `isolation` traps its z-index below the rail beside it, and
 * `contain: layout` makes the workspace the containing block for fixed
 * descendants, so `left` is measured from the workspace edge while the drag
 * clamp measures from the viewport. They disagree by the rail's width, which is
 * why the window threw itself sideways rather than drifting.
 *
 * jsdom does no layout, so neither symptom can be reproduced here. What CAN be
 * asserted is the structural fact underneath both: the dialog is not a
 * descendant of whatever rendered it. Raising the z-index would have satisfied
 * neither.
 */
test("the window renders outside the container that mounted it", () => {
  const host = document.createElement("div");
  host.id = "a-container-with-containment";
  document.body.append(host);

  render(<ShellModal title="Shell" subtitle="Petal's workspace" onClose={vi.fn()}><div /></ShellModal>, {
    container: host,
  });

  const dialog = screen.getByRole("dialog");
  expect(host.contains(dialog)).toBe(false);
  expect(document.body.contains(dialog)).toBe(true);

  host.remove();
});
