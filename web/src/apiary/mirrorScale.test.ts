import { describe, expect, test } from "vitest";
import { fitMirroredTerminal, type MirrorFont } from "./mirrorScale";

/**
 * A mirrored terminal belongs to the Hive being watched or held. These pin the
 * one thing that must never happen — reflowing it — and what the operator
 * asked for: that it uses the room the window has, in legible text.
 */
function frame(width: number, height: number): HTMLElement {
  const outer = document.createElement("div");
  outer.getBoundingClientRect = () => ({ width, height, top: 0, left: 0, right: width, bottom: height, x: 0, y: 0, toJSON: () => ({}) });
  return outer;
}

/** An owner's grid whose drawn size follows the font, as xterm's does. */
function mirror(columns: number, rows: number, startingSize = 14) {
  let size = startingSize;
  const inner = document.createElement("div");
  const screen = document.createElement("div");
  screen.className = "xterm-screen";
  Object.defineProperty(screen, "offsetWidth", { get: () => Math.round(columns * 0.6 * size) });
  Object.defineProperty(screen, "offsetHeight", { get: () => Math.round(rows * 1.2 * size) });
  inner.appendChild(screen);
  const font: MirrorFont = { size: () => size, setSize: (next) => { size = next; } };
  return { inner, font, width: () => screen.offsetWidth, height: () => screen.offsetHeight };
}

describe("fitting a mirrored terminal", () => {
  test("a terminal wider than its window gets smaller text so all of it stays visible", () => {
    const outer = frame(400, 1000);
    const owner = mirror(80, 24);
    fitMirroredTerminal(outer, owner.inner, owner.font);
    expect(owner.width()).toBeLessThanOrEqual(400);
    expect(owner.font.size()).toBeLessThan(14);
  });

  test("the tighter of the two dimensions decides, so nothing is cut off", () => {
    const outer = frame(2000, 200);
    const owner = mirror(80, 24);
    fitMirroredTerminal(outer, owner.inner, owner.font);
    expect(owner.height()).toBeLessThanOrEqual(200);
  });

  /**
   * ⚠️ THE OPERATOR'S REPORT: "the font is weird when watching". A narrow
   * terminal in a wide window was blown up two and a half times. It may grow a
   * little, never to poster size.
   */
  test("a small terminal in a large window grows only a little past normal size", () => {
    const outer = frame(4000, 4000);
    const owner = mirror(80, 24);
    fitMirroredTerminal(outer, owner.inner, owner.font);
    expect(owner.font.size()).toBe(16);
  });

  test("fitting again changes nothing once it fits, so it cannot flip back and forth", () => {
    const outer = frame(700, 700);
    const owner = mirror(80, 24);
    fitMirroredTerminal(outer, owner.inner, owner.font);
    const settled = owner.font.size();
    for (let pass = 0; pass < 5; pass += 1) fitMirroredTerminal(outer, owner.inner, owner.font);
    expect(owner.font.size()).toBe(settled);
    expect(owner.width()).toBeLessThanOrEqual(700);
  });

  test("a window with no room yet is left alone rather than shrunk to nothing", () => {
    const outer = frame(0, 0);
    const owner = mirror(80, 24);
    fitMirroredTerminal(outer, owner.inner, owner.font);
    expect(owner.font.size()).toBe(14);
  });

  /** Scaling enlarged pixels and blurred the text; fitting is by font size only. */
  test("the terminal is never stretched with a CSS transform", () => {
    const outer = frame(4000, 4000);
    const owner = mirror(40, 10);
    fitMirroredTerminal(outer, owner.inner, owner.font);
    expect(owner.inner.style.transform).toBe("");
  });
});
