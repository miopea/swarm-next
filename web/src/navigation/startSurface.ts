import { readSettingsSection } from "../settings/settingsNavigation";

export const SURFACE_STORAGE_KEY = "swarm-next.surface.v1";

export type Surface = "decisions" | "queues" | "tasks" | "workers" | "apiary" | "settings";

const SURFACES: readonly string[] = ["decisions", "queues", "tasks", "workers", "apiary", "settings"];

export function isSurface(value: unknown): value is Surface {
  return typeof value === "string" && SURFACES.includes(value);
}

/**
 * Whether something asked for a specific surface, as opposed to this tab simply
 * being where it was.
 *
 * A link, a settings deep link and a Jira hand-off are requests. The surface
 * remembered in sessionStorage is not: it is written on every navigation, so
 * counting it meant the configured opening screen applied once on a genuinely
 * fresh tab and never again. An installed PWA is one long-lived tab, so in
 * practice "start on Workers" did nothing and every reload came back to
 * whatever happened to be open last.
 */
export function surfaceWasRequested(search: string = window.location.search): boolean {
  try {
    const parameters = new URLSearchParams(search);
    return Boolean(readSettingsSection()) || parameters.has("jira") || parameters.has("surface");
  } catch {
    return false;
  }
}

/**
 * The surface to render before the configured opening screen has been fetched.
 *
 * Deliberately still reads the remembered surface: showing where you were and
 * then correcting to the configured screen is less jarring than a flash of the
 * board on every load.
 */
export function readSavedSurface(search: string = window.location.search): Surface {
  try {
    if (readSettingsSection()) return "settings";
    const linked = new URLSearchParams(search).get("surface");
    if (isSurface(linked)) return linked;
    const saved = window.sessionStorage.getItem(SURFACE_STORAGE_KEY);
    return isSurface(saved) && saved !== "tasks" ? saved : "tasks";
  } catch {
    return "tasks";
  }
}

export function saveSurface(surface: Surface) {
  try {
    window.sessionStorage.setItem(SURFACE_STORAGE_KEY, surface);
  } catch {
    /* Surface persistence is a non-critical convenience. */
  }
}
