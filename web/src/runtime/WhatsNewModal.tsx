import { useState } from "react";

import type { ReleaseVersionNotes } from "../api";
import { anyAwaitingWorkerEngine } from "./whatsNew";
import { renderInline } from "./inlineMarkdown";

/**
 * What arrived while the operator was away.
 *
 * The operator's own words for this were "just an easy way to show them the new
 * features" — so it leads with what they can now do, one line each, rather than
 * with version headings and a scroll. Versions are present because a Hive that
 * skipped several needs to see everything it missed, but they are a quiet label
 * rather than the structure.
 *
 * EARLIER RELEASES ARE CARRIED BUT NOT SHOWN. The artifact has always held the
 * whole history and the panel only ever rendered the slice since the operator's
 * last version, so the rest was present and unreachable. It opens behind a
 * button rather than inline because the reason this is not a changelog still
 * holds: someone who just updated wants what changed, not everything that ever
 * changed.
 */
export default function WhatsNewModal({
  releases,
  onDismiss,
  truncated = false,
  earlier = [],
  heading: headingOverride,
}: {
  releases: ReleaseVersionNotes[];
  onDismiss: () => void;
  /** The gap reaches further back than the notes this artifact carries. */
  truncated?: boolean;
  /** Older than anything in `releases`, newest first, opened on request. */
  earlier?: ReleaseVersionNotes[];
  /** Set when the panel was opened deliberately rather than by an update. */
  heading?: string;
}) {
  const [showEarlier, setShowEarlier] = useState(false);
  if (releases.length === 0) return null;
  const awaitingEngine = anyAwaitingWorkerEngine(releases);
  const heading =
    headingOverride ??
    (releases.length === 1 ? `What's new in ${releases[0].version}` : "What's new since you were last here");
  return (
    <div className="dialog-backdrop" role="presentation" onClick={onDismiss}>
      <div
        className="dialog whats-new"
        role="dialog"
        aria-modal="true"
        aria-labelledby="whats-new-heading"
        onClick={(event) => event.stopPropagation()}
      >
        <p className="eyebrow">{headingOverride ? "Releases" : "Updated"}</p>
        <h3 id="whats-new-heading">{heading}</h3>
        {releases.map((release) => (
          <ReleaseSection key={release.version} release={release} showVersion={releases.length > 1} />
        ))}
        {truncated && (
          // SAID RATHER THAN IMPLIED. Without this the list looks like the whole
          // story, and the operator has no way to tell a complete list from one
          // that starts partway through what they missed.
          <p className="whats-new-pending">
            You were away longer than this list reaches — older releases are not shown.
          </p>
        )}
        {earlier.length > 0 &&
          (showEarlier ? (
            <>
              <h4 className="whats-new-earlier-heading">Earlier releases</h4>
              {earlier.map((release) => (
                // Always labelled with its version: out here the version is the
                // only thing telling one block from the next.
                <ReleaseSection key={release.version} release={release} showVersion />
              ))}
            </>
          ) : (
            <button type="button" className="whats-new-earlier-toggle" onClick={() => setShowEarlier(true)}>
              Earlier releases ({earlier.length})
            </button>
          ))}
        {awaitingEngine && (
          <p className="whats-new-engine-note">
            Some of this is installed but not running yet. The terminal host keeps your workers alive across an
            update, so it swaps separately — run the worker engine update when your workers are idle.
          </p>
        )}
        <div className="dialog-actions">
          <button type="button" className="primary-button" onClick={onDismiss}>
            {headingOverride ? "Close" : "Got it"}
          </button>
        </div>
      </div>
    </div>
  );
}

/**
 * One release, features before fixes.
 *
 * Grouped rather than interleaved: someone scanning for what they can now DO
 * should not have to read past a list of repairs to find it.
 */
function ReleaseSection({ release, showVersion }: { release: ReleaseVersionNotes; showVersion: boolean }) {
  const features = release.notes.filter((note) => note.kind !== "fix");
  const fixes = release.notes.filter((note) => note.kind === "fix");
  return (
    <section className="whats-new-release">
      {showVersion && <h4 className="whats-new-version">{release.version}</h4>}
      <ReleaseNoteList release={release} notes={features} label="New features" slug="feature" />
      <ReleaseNoteList release={release} notes={fixes} label="Fixes" slug="fix" />
    </section>
  );
}

function ReleaseNoteList({
  release,
  notes,
  label,
  slug,
}: {
  release: ReleaseVersionNotes;
  notes: ReleaseVersionNotes["notes"];
  label: string;
  slug: string;
}) {
  if (notes.length === 0) return null;
  return (
    <>
      <h5 className="whats-new-group">{label}</h5>
      <ul className="whats-new-notes">
        {notes.map((note, index) => (
          <li key={`${release.version}-${slug}-${index}`}>
            <span className="whats-new-summary">
              {renderInline(note.summary, `${release.version}-${slug}-${index}`)}
            </span>
            {note.needs_worker_engine_update && (
              <span className="whats-new-pending"> · after the worker engine update</span>
            )}
          </li>
        ))}
      </ul>
    </>
  );
}
