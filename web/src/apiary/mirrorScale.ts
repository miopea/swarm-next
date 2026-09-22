/**
 * Fits a MIRRORED terminal into whatever room its window has, by scaling.
 *
 * ⚠️ SCALING, NEVER `fit()`. A watcher and a Keeper holding a takeover do not
 * own the geometry — the Hive being watched or held does. `TerminalController`
 * states the rule and the incident behind it: a device that does not own the
 * geometry must never mutate its own grid, because `fit()` reflows the owner's
 * wide content at this width and the next frame from the owner puts it back.
 * That alternation is what an operator reported as "terminal is unstable, keeps
 * jumping back ... I am required to do redraws". Reflowing here would rebuild
 * exactly that, one window over.
 *
 * So the grid stays the owner's and only its PRESENTATION changes. The whole of
 * their screen stays on screen, at the largest size that fits.
 */
export function fitMirroredTerminal(outer: HTMLElement, inner: HTMLElement): void {
  inner.style.transformOrigin = "top left";
  // Measured unscaled, or each pass would compound the previous one's scale.
  inner.style.transform = "none";
  const rendered = inner.querySelector<HTMLElement>(".xterm-screen") ?? inner;
  const width = rendered.offsetWidth;
  const height = rendered.offsetHeight;
  const room = outer.getBoundingClientRect();
  if (!width || !height || !room.width || !room.height) return;
  // Growing is capped: xterm draws at a fixed cell size, so past a point this
  // is enlarging pixels rather than showing more, and a blurry terminal reads
  // as broken. Shrinking is not capped, because content that does not fit is
  // content the watcher cannot see at all.
  const scale = Math.min(room.width / width, room.height / height, MAX_MIRROR_SCALE);
  inner.style.transform = `scale(${scale})`;
  // The host keeps the SCALED footprint so the surrounding layout does not
  // reserve room for a size nothing is drawn at.
  inner.style.width = `${width}px`;
  inner.style.height = `${height}px`;
  outer.style.setProperty("--mirror-scaled-height", `${Math.round(height * scale)}px`);
}

const MAX_MIRROR_SCALE = 2.5;

/**
 * Keeps a mirrored terminal fitted while its window and the owner's grid change.
 *
 * Returns the teardown. Both are watched because either can move: the viewer
 * resizes their own window, and the owner resizes their terminal, which arrives
 * as a frame rather than as a layout change here.
 */
export function observeMirrorFit(outer: HTMLElement, inner: HTMLElement): () => void {
  const refit = () => fitMirroredTerminal(outer, inner);
  refit();
  if (typeof ResizeObserver === "undefined") return () => {};
  const observer = new ResizeObserver(refit);
  observer.observe(outer);
  observer.observe(inner);
  return () => observer.disconnect();
}
