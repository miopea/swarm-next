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
 * What to show, given what shipped, what this operator has already seen, and
 * what this Hive was running before.
 *
 * A FIRST RUN SHOWS NOTHING AND RECORDS THE VERSION. Someone installing Swarm
 * for the first time has not missed anything, and greeting them with five
 * releases of history reads as a changelog — which is the thing the operator
 * asked for an alternative to.
 *
 * BUT AN EMPTY BROWSER IS NOT A FIRST INSTALL. `seen` lives in local storage,
 * so it is empty on a second machine, in a private window, and after cleared
 * site data — none of which mean the operator has missed nothing. `previous`
 * is the version the installer replaced, and it distinguishes the two: absent
 * with no previous release is a genuine first install and shows nothing; absent
 * on a Hive that HAS been updated falls back to the release it came from, so
 * everything since then is shown.
 *
 * The floor is the OLDER of the two when both exist, because each can be stale
 * in a way the other is not — `seen` lags when the panel was dismissed on
 * another device, `previous` lags when several updates ran between two visits —
 * and showing a note twice is a much smaller failure than never showing it.
 */
export function whatsNewFor(
  releases: ReleaseVersionNotes[],
  runningVersion: string,
  seen: string | null,
  previous?: string | null,
): { show: ReleaseVersionNotes[]; recordAs: string | null; truncated: boolean; earlier: ReleaseVersionNotes[] } {
  const floor = olderOf(seen, previous ?? null);
  if (floor === null) {
    // A FIRST RUN STILL GETS THE HISTORY, it just is not shown one. Nothing
    // opens on its own here; `earlier` is what the operator can ask to read.
    return { show: [], recordAs: runningVersion, truncated: false, earlier: newestFirst(releases) };
  }
  const show = releasesNewerThan(releases, floor);
  return {
    show,
    recordAs: show.length > 0 ? runningVersion : null,
    truncated: reachesPastTheNotes(releases, floor),
    earlier: olderThan(releases, show),
  };
}

/**
 * Everything the artifact carries that this panel is not already showing.
 *
 * The whole history has always been in the bundle and the panel only ever
 * rendered the slice since the operator's last version, so the rest was
 * present and unreachable. This is what the panel offers to open.
 */
export function olderThan(releases: ReleaseVersionNotes[], shown: ReleaseVersionNotes[]): ReleaseVersionNotes[] {
  const already = new Set(shown.map((entry) => entry.version));
  return newestFirst(releases.filter((entry) => !already.has(entry.version)));
}

function newestFirst(releases: ReleaseVersionNotes[]): ReleaseVersionNotes[] {
  return releases
    .filter((entry) => parseVersion(entry.version) !== null)
    .sort((left, right) => compareVersions(parseVersion(right.version)!, parseVersion(left.version)!));
}

/**
 * Whether the operator's gap is deeper than the artifact carries.
 *
 * The notes bundle holds a bounded number of releases, so an operator who was
 * away long enough is shown a list that starts partway through what they
 * missed. Presenting that as "what's new since you were last here" is a quiet
 * false claim about completeness, and the panel would look identical either
 * way — so it is measured here and said out loud rather than left to look
 * complete.
 */
function reachesPastTheNotes(releases: ReleaseVersionNotes[], floor: string): boolean {
  const floorParsed = parseVersion(floor);
  if (floorParsed === null) return false;
  const parsed = releases
    .map((entry) => parseVersion(entry.version))
    .filter((version): version is number[] => version !== null);
  if (parsed.length === 0) return false;
  const oldest = parsed.reduce((left, right) => (compareVersions(left, right) <= 0 ? left : right));
  return compareVersions(floorParsed, oldest) < 0;
}

/** Whichever of the two anchors reaches further back; null when neither parses. */
function olderOf(left: string | null, right: string | null): string | null {
  const leftParsed = parseVersion(left === null || left.trim() === "" ? null : left);
  const rightParsed = parseVersion(right === null || right.trim() === "" ? null : right);
  if (leftParsed === null) return rightParsed === null ? null : right;
  if (rightParsed === null) return left;
  return compareVersions(leftParsed, rightParsed) <= 0 ? left : right;
}

/** Whether anything shown is installed but not yet in effect. */
export function anyAwaitingWorkerEngine(releases: ReleaseVersionNotes[]): boolean {
  return releases.some((entry) => entry.notes.some((note: ReleaseNote) => note.needs_worker_engine_update));
}
