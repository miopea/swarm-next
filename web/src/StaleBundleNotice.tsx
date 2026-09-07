/**
 * Says the page is running code older than the Hive serving it, and stops
 * there.
 *
 * A developer reported a bug that had been fixed and released days earlier.
 * The operator said "that should be fixed in the newest version"; he answered
 * "because I'm still running into it". Both were right — the fix was on his
 * machine and his browser was still running the bundle it loaded before the
 * upgrade — and nothing anywhere could tell either of them that. The next
 * guess was to reload the workers, which is reasonable and wrong, because
 * workers do not handle browser keystrokes. With no signal to read, guessing
 * what to restart is the only move left.
 *
 * IT DOES NOT RELOAD BY ITSELF, and that is the main design decision. This app
 * is mostly a terminal and a set of half-written prompts; throwing that away to
 * apply a version change would be a worse defect than the one it fixes. It
 * says what happened and leaves the moment to the operator.
 *
 * DISMISSAL IS PER VERSION rather than permanent. Dismissing tells it "not
 * now, for this one" — the next upgrade is a new fact and says so again.
 * A single permanent dismissal would silence the signal for good on the first
 * inconvenient day, which is how a warning becomes decoration.
 */
export default function StaleBundleNotice({ stale, serverVersion, dismissed, onDismiss, onReload = reloadBrowser }: {
  stale: boolean;
  serverVersion: string | null;
  dismissed: string | null;
  onDismiss: (version: string) => void;
  onReload?: (version: string) => void;
}) {
  if (!stale || !serverVersion || dismissed === serverVersion) return null;
  return (
    <div className="stale-bundle-notice" role="status">
      <span>
        <strong>Browser update ready</strong>
        <small>This tab differs from the running Hive. Finish or save any unsent forms before reloading. Workers keep running.</small>
      </span>
      <span className="stale-bundle-actions">
        {/* A VERSION-STAMPED RELOAD, not a reload of the old URL.
            This said a plain reload was "all that is needed: static responses
            are served no-cache". That reasoning is sound about our headers and
            was still wrong in practice — the operator pressed this button on
            2026-08-28, stayed on the old bundle, and had to use ctrl-shift-F5
            to get the new one. A reload revalidates through whatever sits
            between the browser and the API, and this Hive can be published
            through a tunnel with its own edge cache.

            Navigating to a URL the browser has never seen cannot be answered
            from a cache entry, so the version it is stamped with is the thing
            that forces a fetch for the new URL. Replace the current history
            entry before reloading so navigation timing still identifies this
            as a reload and preserves the operator's current surface. */}
        <button
          type="button"
          className="primary-action"
          onClick={() => onReload(serverVersion)}
        >Reload this tab</button>
        <button type="button" className="text-button" onClick={() => onDismiss(serverVersion)}>Not now</button>
      </span>
    </div>
  );
}

export function reloadBrowser(version: string, reload: () => void = () => window.location.reload()) {
  const fresh = new URL(window.location.href);
  fresh.searchParams.set("v", version);
  window.history.replaceState(window.history.state, "", fresh.toString());
  reload();
}
