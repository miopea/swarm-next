import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, expect, test } from "vitest";

import { useWorkerRailWidth } from "./useWorkerRailWidth";

beforeEach(() => localStorage.clear());
afterEach(cleanup);

function Harness() {
  const rail = useWorkerRailWidth();
  return <div role="separator" aria-valuenow={rail.width} tabIndex={0} onPointerDown={rail.start} onPointerMove={rail.move} onPointerUp={rail.finish} onKeyDown={rail.resizeWithKeyboard}>{rail.resizing ? "resizing" : "idle"}</div>;
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

function pointerEvent(type: string, clientX: number) {
  const event = new Event(type, { bubbles: true });
  Object.defineProperties(event, {
    clientX: { value: clientX },
    pointerId: { value: 1 },
  });
  return event;
}
