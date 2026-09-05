export type PoppableSurface = "decisions" | "queues" | "tasks" | "workers" | "apiary" | "settings";

/**
 * The address a detached surface opens at.
 *
 * Built from the current location so the detached window keeps the origin and
 * path it was opened from, and carries only the surface. Any other query the
 * opener happened to hold — a Jira deep link, a settings section — belongs to
 * that window's navigation, not to a fresh one.
 */
export function surfaceWindowUrl(location: { pathname: string }, surface: PoppableSurface): string {
  // `surface` alone already means "open Swarm here", which is what a
  // notification link carries. Detaching is a different request — show this and
  // nothing else — so it needs its own word rather than a second meaning for
  // that one.
  return `${location.pathname}?surface=${surface}&detached=1`;
}

/**
 * The window name a surface detaches into.
 *
 * Naming it per surface means asking again focuses the window that is already
 * open instead of stacking duplicates of the same view, which is what a browser
 * does for a repeated `window.open` with the same name.
 */
export function surfaceWindowName(surface: PoppableSurface): string {
  return `swarm-next-${surface}`;
}

export const SURFACE_WINDOW_FEATURES = "noopener=false,popup=yes,width=1280,height=900";

/**
 * Detaching is a convenience, and a blocked popup is the common failure. Report
 * whether a window actually opened so the caller can say so rather than leaving
 * the operator waiting for a window that a blocker silently refused.
 */
/**
 * The windows this one has detached, so a surface is never shown twice.
 *
 * Held here rather than in component state because a detached window outlives
 * any one render, and the only way to learn it was closed is to ask it.
 */
const detached = new Map<PoppableSurface, Window>();

export function openSurfaceWindow(
  surface: PoppableSurface,
  open: (url: string, name: string, features: string) => Window | null,
  location: { pathname: string } = window.location,
): boolean {
  const opened = open(
    surfaceWindowUrl(location, surface),
    surfaceWindowName(surface),
    SURFACE_WINDOW_FEATURES,
  );
  if (opened) detached.set(surface, opened);
  opened?.focus();
  return opened !== null;
}

/**
 * Whether this surface is already open in a window of its own.
 *
 * A closed window is forgotten on the way past, since browsers report closure
 * only when asked and never announce it.
 */
export function surfaceIsDetached(surface: PoppableSurface): boolean {
  const window = detached.get(surface);
  if (!window || window.closed) {
    detached.delete(surface);
    return false;
  }
  return true;
}

/** Brings an already-detached surface forward instead of drawing it twice. */
export function focusDetachedSurface(surface: PoppableSurface): boolean {
  if (!surfaceIsDetached(surface)) return false;
  detached.get(surface)?.focus();
  return true;
}

/**
 * Which surface this window was detached to show, if it is one.
 *
 * Requires the explicit `detached` flag. A notification deep link carries
 * `surface` on its own and must open the whole control room, not a window with
 * no way out of it.
 */
export function detachedSurface(
  location: { search: string } = window.location,
): PoppableSurface | undefined {
  const query = new URLSearchParams(location.search);
  if (query.get("detached") !== "1") return undefined;
  const asked = query.get("surface");
  return asked === "decisions" || asked === "queues" || asked === "tasks" || asked === "workers"
    || asked === "apiary" || asked === "settings"
    ? asked
    : undefined;
}

/** Test seam: forgets every detached window. */
export function forgetDetachedSurfaces(): void {
  detached.clear();
}
