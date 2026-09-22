import { describe, expect, test } from "vitest";
import { fitMirroredTerminal } from "./mirrorScale";

/**
 * A mirrored terminal belongs to the Hive being watched or held. These pin the
 * one thing that must never happen — reflowing it — and the thing the operator
 * asked for: that it uses the room the window has.
 */
function frame(width: number, height: number): HTMLElement {
  const outer = document.createElement("div");
  outer.getBoundingClientRect = () => ({ width, height, top: 0, left: 0, right: width, bottom: height, x: 0, y: 0, toJSON: () => ({}) });
  return outer;
}

function mirror(width: number, height: number): HTMLElement {
  const inner = document.createElement("div");
  const screen = document.createElement("div");
  screen.className = "xterm-screen";
  Object.defineProperty(screen, "offsetWidth", { value: width });
  Object.defineProperty(screen, "offsetHeight", { value: height });
  inner.appendChild(screen);
  return inner;
}

function scaleOf(element: HTMLElement): number {
  return Number(/scale\(([\d.]+)\)/.exec(element.style.transform)?.[1] ?? NaN);
}

describe("fitting a mirrored terminal", () => {
  test("a terminal wider than its window is shrunk so all of it stays visible", () => {
    const outer = frame(400, 1000);
    const inner = mirror(800, 400);
    fitMirroredTerminal(outer, inner);
    // Content that does not fit is content the watcher cannot see at all, so
    // shrinking is never capped.
    expect(scaleOf(inner)).toBeCloseTo(0.5);
  });

  test("the tighter of the two dimensions decides, so nothing is cut off", () => {
    const outer = frame(800, 200);
    const inner = mirror(800, 400);
    fitMirroredTerminal(outer, inner);
    expect(scaleOf(inner)).toBeCloseTo(0.5);
  });

  test("a small terminal grows into a large window, but only so far", () => {
    const outer = frame(4000, 4000);
    const inner = mirror(400, 200);
    fitMirroredTerminal(outer, inner);
    // Past a point this enlarges pixels rather than showing more, and a blurry
    // terminal reads as broken.
    expect(scaleOf(inner)).toBeCloseTo(2.5);
  });

  test("measuring is unscaled, so repeated fits do not compound", () => {
    const outer = frame(400, 1000);
    const inner = mirror(800, 400);
    fitMirroredTerminal(outer, inner);
    fitMirroredTerminal(outer, inner);
    fitMirroredTerminal(outer, inner);
    expect(scaleOf(inner)).toBeCloseTo(0.5);
  });

  test("a window with no room yet is left alone rather than scaled to nothing", () => {
    const outer = frame(0, 0);
    const inner = mirror(800, 400);
    fitMirroredTerminal(outer, inner);
    expect(inner.style.transform).toBe("none");
  });
});
