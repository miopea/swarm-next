import { useState, type KeyboardEvent, type PointerEvent } from "react";

const STORAGE_KEY = "swarm-next.rail-width.v1";
const MIN_WIDTH = 220;
const MAX_WIDTH = 480;

function clampWidth(value: number) {
  return Number.isFinite(value) ? Math.round(Math.max(MIN_WIDTH, Math.min(MAX_WIDTH, value))) : 286;
}

function readSavedWidth() {
  const saved = Number(localStorage.getItem(STORAGE_KEY));
  return Number.isFinite(saved) ? clampWidth(saved) : 286;
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
    const next = clampWidth(event.clientX);
    setWidth(next);
    setResizing(false);
    if (event.currentTarget.hasPointerCapture?.(event.pointerId)) event.currentTarget.releasePointerCapture(event.pointerId);
    localStorage.setItem(STORAGE_KEY, String(next));
  }

  function resizeWithKeyboard(event: KeyboardEvent<HTMLDivElement>) {
    if (event.key !== "ArrowLeft" && event.key !== "ArrowRight") return;
    event.preventDefault();
    const next = clampWidth(width + (event.key === "ArrowLeft" ? -16 : 16));
    setWidth(next);
    localStorage.setItem(STORAGE_KEY, String(next));
  }

  return { width, resizing, start, move, finish, resizeWithKeyboard };
}
