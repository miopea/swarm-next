import type { ITerminalOptions, Terminal } from "@xterm/xterm";

import { documentColorTheme, terminalTheme } from "../brand/terminalTheme";
import { TERMINAL_FONT_FAMILY, TERMINAL_FONT_SIZE } from "../terminal/XtermSurface";

/**
 * The font size a mirrored terminal is drawn at, read and set through xterm.
 */
export interface MirrorFont {
  size(): number;
  setSize(size: number): void;
}

/** The xterm font size, as the fit reads and sets it. */
export function mirrorFont(terminal: Terminal): MirrorFont {
  return {
    size: () => terminal.options.fontSize ?? TERMINAL_FONT_SIZE,
    setSize: (size) => { terminal.options.fontSize = size; },
  };
}

/**
 * Drawn the way this app draws its own terminals, so another Hive's screen
 * reads like one. The watch and takeover windows used xterm's defaults, which
 * is a different typeface from every other terminal here — part of what the
 * operator called "the font is weird when watching".
 */
export function mirrorTerminalOptions(): ITerminalOptions {
  return {
    fontFamily: TERMINAL_FONT_FAMILY,
    fontSize: TERMINAL_FONT_SIZE,
    minimumContrastRatio: 4.5,
    theme: terminalTheme(documentColorTheme()),
  };
}

/**
 * Fits a MIRRORED terminal into whatever room its window has, by font size.
 *
 * ⚠️ NEVER `fit()`. A watcher and a Keeper holding a takeover do not own the
 * geometry — the Hive being watched or held does. `TerminalController` states
 * the rule and the incident behind it: a device that does not own the geometry
 * must never mutate its own grid, because `fit()` reflows the owner's wide
 * content at this width and the next frame from the owner puts it back. That
 * alternation is what an operator reported as "terminal is unstable, keeps
 * jumping back ... I am required to do redraws".
 *
 * ⚠️ AND NOT A CSS `scale()` EITHER. That was the previous answer, and it
 * enlarged pixels: a narrow terminal in a wide window was stretched up to two
 * and a half times, blurred, and read as broken. xterm draws text crisply at
 * any font size and a font size never changes the grid, so the owner's rows
 * and columns stay exactly theirs and only the letters change size.
 */
export function fitMirroredTerminal(outer: HTMLElement, inner: HTMLElement, font: MirrorFont): void {
  const rendered = inner.querySelector<HTMLElement>(".xterm-screen") ?? inner;
  const width = rendered.offsetWidth;
  const height = rendered.offsetHeight;
  const room = outer.getBoundingClientRect();
  if (!width || !height || !room.width || !room.height) return;
  const current = font.size();
  // The rendered size is close to proportional to the font size but not
  // exactly, because cells round to whole pixels. The margin keeps a size
  // predicted to just fit from landing a pixel over, and then flipping back.
  const scale = Math.min(room.width / width, room.height / height) * FIT_MARGIN;
  const next = Math.min(MAX_MIRROR_FONT_SIZE, Math.max(MIN_MIRROR_FONT_SIZE, Math.floor(current * scale * 2) / 2));
  if (next !== current) font.setSize(next);
}

const FIT_MARGIN = 0.97;
const MIN_MIRROR_FONT_SIZE = 6;
/**
 * Growing is capped a little above this app's own terminal size: a mirror in a
 * large window is not a reason to draw someone's screen at poster size.
 */
const MAX_MIRROR_FONT_SIZE = 16;

/**
 * Keeps a mirrored terminal fitted while its window and the owner's grid change.
 *
 * Returns the teardown. Both are watched because either can move: the viewer
 * resizes their own window, and the owner resizes their terminal, which arrives
 * as a frame rather than as a layout change here.
 */
export function observeMirrorFit(outer: HTMLElement, inner: HTMLElement, font: MirrorFont): () => void {
  const refit = () => fitMirroredTerminal(outer, inner, font);
  refit();
  if (typeof ResizeObserver === "undefined") return () => {};
  const observer = new ResizeObserver(refit);
  observer.observe(outer);
  observer.observe(inner);
  return () => observer.disconnect();
}
