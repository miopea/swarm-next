import { useState, type KeyboardEvent, type PointerEvent } from "react";

const STORAGE_KEY = "swarm-next.rail-width.v1";
const MIN_WIDTH = 220;
const MAX_WIDTH = 480;
const DEFAULT_WIDTH = 286;

function clampWidth(value: number) {
  return Number.isFinite(value)
    ? Math.round(Math.max(MIN_WIDTH, Math.min(MAX_WIDTH, value)))
    : DEFAULT_WIDTH;
}

/**
 * The saved rail width, or the default — never the floor by accident.
 *
 * Two bugs lived in one line here, and both were reproduced in the running app
 * rather than reasoned about.
 *
 * READING STORAGE CAN THROW, and this is inside a useState initialiser, so the
 * throw took the whole control room down: not a wide rail, not a narrow rail,
 * but "Swarm hit a problem drawing this view" and no shell at all. A browser
 * with site data blocked, a private window, or a storage policy change is
 * enough. A rail width is a convenience and must never be able to do that.
 *
 * A MISSING KEY IS NOT A ZERO. localStorage.getItem returns null when unset,
 * Number(null) is 0, and Number.isFinite(0) is true — so the old guard accepted
 * it and clamped 0 up to the 220px MINIMUM. Every browser that had never
 * dragged the rail got the narrowest rail the app allows while --rail-width
 * still read 286px, which is why it looked like a styling problem. Measured in
 * the live app at eleven viewport widths from 600 to 1440: the rail was 220px
 * at every single one. Number("") is 0 too, so an empty string did the same,
 * while "abc" correctly fell through to the default — inconsistent handling of
 * two spellings of the same absence.
 */
function readSavedWidth() {
  const saved = readStoredValue();
  if (saved === null || saved.trim() === "") return DEFAULT_WIDTH;
  const parsed = Number(saved);
  return Number.isFinite(parsed) ? clampWidth(parsed) : DEFAULT_WIDTH;
}

function readStoredValue(): string | null {
  try {
    return localStorage.getItem(STORAGE_KEY);
  } catch {
    return null;
  }
}

function storeWidth(width: number) {
  try {
    localStorage.setItem(STORAGE_KEY, String(width));
  } catch {
    // Not being able to remember the width is a smaller problem than not
    // drawing the page, and there is nothing useful to tell the operator about
    // it — the rail still works for this session.
  }
}

export function useWorkerRailWidth() {
  const [width, setWidth] = useState(readSavedWidth);
  const [resizing, setResizing] = useState(false);

  function start(event: PointerEvent<HTMLDivElement>) {
    setResizing(true);
    event.currentTarget.setPointerCapture?.(event.pointerId);
  }

  function move(event: PointerEvent<HTMLDivElement>) {
    if (resizing) setWidth(clampWidth(event.clientX));
  }

  function finish(event: PointerEvent<HTMLDivElement>) {
    if (!resizing) return;
    setResizing(false);
    if (event.currentTarget.hasPointerCapture?.(event.pointerId)) {
      event.currentTarget.releasePointerCapture(event.pointerId);
    }
    // A CANCELLED GESTURE IS NOT A CHOSEN WIDTH. This handler is wired to
    // pointercancel as well as pointerup, and a cancelled pointer event carries
    // a clientX of 0 — which clamped to the 220px minimum and was then written
    // to storage as though the operator had dragged it there. Losing a pointer
    // to a system gesture or a lost window silently narrowed the rail and made
    // it stick.
    if (event.type === "pointercancel" || !Number.isFinite(event.clientX) || event.clientX <= 0) {
      return;
    }
    const next = clampWidth(event.clientX);
    setWidth(next);
    storeWidth(next);
  }

  function resizeWithKeyboard(event: KeyboardEvent<HTMLDivElement>) {
    if (event.key !== "ArrowLeft" && event.key !== "ArrowRight") return;
    event.preventDefault();
    const next = clampWidth(width + (event.key === "ArrowLeft" ? -16 : 16));
    setWidth(next);
    storeWidth(next);
  }

  return { width, resizing, start, move, finish, resizeWithKeyboard };
}
