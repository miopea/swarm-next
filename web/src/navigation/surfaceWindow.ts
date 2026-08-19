export type PoppableSurface = "decisions" | "tasks" | "workers" | "apiary" | "settings";

/**
 * The address a detached surface opens at.
 *
 * Built from the current location so the detached window keeps the origin and
 * path it was opened from, and carries only the surface. Any other query the
 * opener happened to hold — a Jira deep link, a settings section — belongs to
 * that window's navigation, not to a fresh one.
 */
export function surfaceWindowUrl(location: { pathname: string }, surface: PoppableSurface): string {
  return `${location.pathname}?surface=${surface}`;
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
  opened?.focus();
  return opened !== null;
}
