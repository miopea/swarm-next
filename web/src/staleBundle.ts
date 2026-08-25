/**
 * Whether the JavaScript now running predates the Hive serving it.
 *
 * WHY THIS EXISTS. A developer reported a bug that had been fixed and released
 * days earlier; the operator said "that should be fixed in the newest version",
 * and he answered "because I'm still running into it". Both were right. The fix
 * was on his machine and his browser was still executing the bundle it had
 * loaded before the upgrade, and nothing anywhere could tell either of them
 * that. The next guess was to reload the workers, which was reasonable and
 * wrong — workers do not handle browser keystrokes. That wrong guess is the
 * cost of a missing signal: with nothing to read, all you can do is guess what
 * to restart.
 *
 * IT IS NOT A CACHING BUG, which is worth stating because it looks exactly like
 * one and the remedy for a caching bug would be different. Static responses
 * already carry `Cache-Control: no-cache`, and the service worker handles push
 * notifications only — it has no fetch handler and caches no assets. The cause
 * is that this is a long-lived page: someone leaves a tab open, the Hive is
 * upgraded underneath it, and the already-loaded code keeps running because
 * nothing navigates. A hard refresh works purely because it IS a navigation.
 */

/**
 * What this bundle was built from, baked in at build time.
 *
 * Must be compiled in rather than fetched: a value the running page asks the
 * server for would describe the SERVER, which is the thing it is trying to
 * compare itself against.
 *
 * Absent under `vite dev` and in tests, where there is no release to be behind.
 */
function builtVersionNow(): string | undefined {
  // Read at the call site rather than captured at module load. Vite replaces
  // `import.meta.env.VITE_*` statically wherever it appears, so this is baked
  // in at build exactly the same way — and unlike a module-level const it can
  // be exercised by a test, which is how the two false-positive cases below
  // are actually covered rather than asserted.
  return import.meta.env.VITE_SWARM_BUILD_VERSION || undefined;
}

/**
 * True when the page should tell the operator to reload.
 *
 * Deliberately conservative — it stays silent unless it is certain, because a
 * banner that cries wolf is worse than the silence it replaces:
 *
 * - No baked version means a development server or a test. Nothing to compare.
 * - No server version yet means health has not answered. Not evidence.
 * - Equal versions are the overwhelmingly common case.
 *
 * It does NOT try to decide which version is newer. A page whose version merely
 * DIFFERS from the Hive is a page that should be reloaded either way, and
 * ordering two version strings is a guess this does not need to make.
 */
export function bundleIsStale(serverVersion: string | null | undefined): boolean {
  const built = builtVersionNow();
  if (!built || !serverVersion) return false;
  return built !== serverVersion;
}

/** What the running page was built from, for the diagnostic report. */
export function builtVersion(): string | undefined {
  return builtVersionNow();
}
