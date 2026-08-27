import { useState } from "react";

import type { SupersededProvider } from "../api";

// The alpha three are listed for exhaustiveness, not because they can appear
// here: nothing probes their releases, so the API never reports one superseded.
// Naming them keeps this a total map, so adding a provider is a type error
// rather than a blank label.
const PROVIDER_NAME: Record<SupersededProvider["provider"], string> = {
  claude_code: "Claude",
  codex: "Codex",
  gemini: "Gemini",
  grok: "Grok",
  opencode: "OpenCode",
};

/**
 * Workers still running a provider release that disk has moved past.
 *
 * Claude and Codex update themselves, and a running process keeps executing the
 * release it started with. An update that lands while workers are up is
 * therefore installed and not running, on every worker, until each restarts —
 * and nothing said so. The provider tells its own terminal ("Restart to
 * update"); nothing told the operator how many workers that applied to.
 */
export default function ProviderReleaseAction({ superseded, busy, onRestart }: {
  superseded: SupersededProvider[];
  busy: boolean;
  onRestart: () => Promise<void>;
}) {
  const [confirming, setConfirming] = useState(false);
  if (superseded.length === 0) return null;
  const workerCount = new Set(superseded.flatMap((entry) => entry.worker_ids)).size;
  const named = superseded
    .map((entry) => `${PROVIDER_NAME[entry.provider]}${entry.version ? ` ${entry.version}` : ""}`)
    .join(" and ");
  return (
    <article className="runtime-subsystem-card runtime-subsystem-restart provider-release-action" aria-label="Provider release status">
      <header>
        <div><span className="runtime-component-name">Providers</span><strong>Update installed · restart to run it</strong></div>
        <span className="runtime-status-badge restart">Restart required</span>
      </header>
      <p>{named} {superseded.length === 1 ? "is" : "are"} installed, and {workerCount} running worker{workerCount === 1 ? "" : "s"} started before that, so {workerCount === 1 ? "it is" : "they are"} still running the older release.</p>
      <small>A provider keeps executing the release it started with. Restarting reloads {workerCount === 1 ? "that worker" : "those workers"} from their saved conversations; identities, tasks, and terminal history remain durable.</small>
      {!confirming ? (
        <button className="secondary-button" type="button" disabled={busy} onClick={() => setConfirming(true)}>Restart {workerCount} worker{workerCount === 1 ? "" : "s"}</button>
      ) : (
        <div className="maintenance-confirmation" role="group" aria-label="Confirm provider restart">
          <strong>Restart {workerCount} worker{workerCount === 1 ? "" : "s"} now?</strong>
          <span>Anything they are running is interrupted and is not resumed. Workers already on the installed release are left alone.</span>
          <div className="settings-actions">
            <button className="secondary-button" type="button" disabled={busy} onClick={() => setConfirming(false)}>Not now</button>
            <button className="primary-action" type="button" disabled={busy} onClick={() => { setConfirming(false); void onRestart(); }}>Restart and update</button>
          </div>
        </div>
      )}
    </article>
  );
}
