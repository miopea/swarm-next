import type { ReleaseNote, ReleaseVersionNotes } from "../api";

const STORAGE_KEY = "swarm.whats-new.seen";

/**
 * Which releases are new to this operator, newest first.
 *
 * Compared as version tuples rather than strings, because "0.8.9" sorts after
 * "0.8.10" lexically and that is exactly the pair a Hive updating across a
 * rollover would hit.
 */
export function releasesNewerThan(releases: ReleaseVersionNotes[], seen: string | null): ReleaseVersionNotes[] {
  const floor = parseVersion(seen);
  if (floor === null) return [];
  return releases
    .filter((entry) => {
      const version = parseVersion(entry.version);
      return version !== null && compareVersions(version, floor) > 0;
    })
    .sort((left, right) => compareVersions(parseVersion(right.version)!, parseVersion(left.version)!));
}

function parseVersion(value: string | null): number[] | null {
  if (value === null || value.trim() === "") return null;
  const parts = value.trim().replace(/^v/, "").split("-")[0].split(".");
  if (parts.length === 0) return null;
  const numbers = parts.map(Number);
  return numbers.every((part) => Number.isInteger(part) && part >= 0) ? numbers : null;
}

function compareVersions(left: number[], right: number[]): number {
  for (let index = 0; index < Math.max(left.length, right.length); index += 1) {
    const difference = (left[index] ?? 0) - (right[index] ?? 0);
    if (difference !== 0) return difference;
  }
  return 0;
}

/**
 * The version this operator was last shown notes for.
 *
 * READING STORAGE CAN THROW — a private window, blocked site data, a policy
 * change — and this is read during render. A "what's new" panel must never be
 * able to take the control room down, so the throw becomes "nothing seen".
 */
export function readSeenVersion(): string | null {
  try {
    return localStorage.getItem(STORAGE_KEY);
  } catch {
    return null;
  }
}

export function storeSeenVersion(version: string) {
  try {
    localStorage.setItem(STORAGE_KEY, version);
  } catch {
    // Forgetting means the operator may see one panel twice. That is a smaller
    // problem than anything worth reporting to them.
  }
}

/**
 * What to show, given what shipped and what this operator has already seen.
 *
 * A FIRST RUN SHOWS NOTHING AND RECORDS THE VERSION. Someone installing Swarm
 * for the first time has not missed anything, and greeting them with five
 * releases of history reads as a changelog — which is the thing the operator
 * asked for an alternative to. `seen` being absent is that case, and it is
 * deliberately distinguished from `seen` being an older version.
 */
export function whatsNewFor(
  releases: ReleaseVersionNotes[],
  runningVersion: string,
  seen: string | null,
): { show: ReleaseVersionNotes[]; recordAs: string | null } {
  if (seen === null || seen.trim() === "") {
    return { show: [], recordAs: runningVersion };
  }
  const show = releasesNewerThan(releases, seen);
  return { show, recordAs: show.length > 0 ? runningVersion : null };
}

/** Whether anything shown is installed but not yet in effect. */
export function anyAwaitingWorkerEngine(releases: ReleaseVersionNotes[]): boolean {
  return releases.some((entry) => entry.notes.some((note: ReleaseNote) => note.needs_worker_engine_update));
}
