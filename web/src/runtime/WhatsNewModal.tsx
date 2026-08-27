import type { ReleaseVersionNotes } from "../api";
import { anyAwaitingWorkerEngine } from "./whatsNew";

/**
 * What arrived while the operator was away.
 *
 * The operator's own words for this were "just an easy way to show them the new
 * features" — so it leads with what they can now do, one line each, rather than
 * with version headings and a scroll. Versions are present because a Hive that
 * skipped several needs to see everything it missed, but they are a quiet label
 * rather than the structure.
 */
export default function WhatsNewModal({ releases, onDismiss }: {
  releases: ReleaseVersionNotes[];
  onDismiss: () => void;
}) {
  if (releases.length === 0) return null;
  const awaitingEngine = anyAwaitingWorkerEngine(releases);
  const heading = releases.length === 1 ? `What's new in ${releases[0].version}` : "What's new since you were last here";
  return (
    <div className="dialog-backdrop" role="presentation" onClick={onDismiss}>
      <div
        className="dialog whats-new"
        role="dialog"
        aria-modal="true"
        aria-labelledby="whats-new-heading"
        onClick={(event) => event.stopPropagation()}
      >
        <p className="eyebrow">Updated</p>
        <h3 id="whats-new-heading">{heading}</h3>
        {releases.map((release) => (
          <section key={release.version} className="whats-new-release">
            {releases.length > 1 && <h4 className="whats-new-version">{release.version}</h4>}
            <ul className="whats-new-notes">
              {release.notes.map((note, index) => (
                <li key={`${release.version}-${index}`}>
                  <span className={`whats-new-kind ${note.kind}`}>{note.kind === "fix" ? "Fixed" : "New"}</span>
                  <span className="whats-new-summary">{note.summary}</span>
                  {/* Installed and NOT in effect. Saying "new" about something
                      the operator cannot use yet is worse than not listing it. */}
                  {note.needs_worker_engine_update && (
                    <span className="whats-new-pending"> · after the worker engine update</span>
                  )}
                </li>
              ))}
            </ul>
          </section>
        ))}
        {awaitingEngine && (
          <p className="whats-new-engine-note">
            Some of this is installed but not running yet. The terminal host keeps your workers alive across an
            update, so it swaps separately — run the worker engine update when your workers are idle.
          </p>
        )}
        <div className="dialog-actions">
          <button type="button" className="primary-button" onClick={onDismiss}>
            Got it
          </button>
        </div>
      </div>
    </div>
  );
}
