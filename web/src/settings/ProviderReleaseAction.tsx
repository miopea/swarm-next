import { useState } from "react";

import type { SupersededProvider } from "../api";
import { providerUpdateCounts, supersededReleaseNames } from "../runtime/runtimeUpdates";

function workers(count: number): string {
  return `${count} worker${count === 1 ? "" : "s"}`;
}

/**
 * Workers still running a provider release that disk has moved past.
 *
 * Claude and Codex update themselves, and a running process keeps executing the
 * release it started with. An update that lands while workers are up is
 * therefore installed and not running, on every worker, until each restarts —
 * and nothing said so. The provider tells its own terminal ("Restart to
 * update"); nothing told the operator how many workers that applied to.
 *
 * A Claude worker whose release the worker engine can read is moved onto the
 * update by Swarm itself, with /restart, once it is at rest (ADR 0122). Only
 * the rest need the operator, because only stopping and starting moves them.
 */
export default function ProviderReleaseAction({ superseded, busy, onRestart }: {
  superseded: SupersededProvider[];
  busy: boolean;
  onRestart: () => Promise<void>;
}) {
  const [confirming, setConfirming] = useState(false);
  if (superseded.length === 0) return null;
  const counts = providerUpdateCounts(superseded);
  const named = supersededReleaseNames(superseded);
  const installed = `${named} ${superseded.length === 1 ? "is" : "are"} installed`;
  if (counts.stopAndStart === 0) {
    return (
      <article className="runtime-subsystem-card provider-release-action" aria-label="Provider release status">
        <header>
          <div><span className="runtime-component-name">Providers</span><strong>Update installed · Swarm is moving workers onto it</strong></div>
          <span className={`runtime-status-badge ${counts.refused > 0 ? "attention" : "checking"}`}>{counts.refused > 0 ? "Needs a retry" : "Automatic"}</span>
        </header>
        <p>{installed}, and {workers(counts.inPlace)} still {counts.inPlace === 1 ? "runs" : "run"} the older release. Swarm restarts each one onto it with /restart as soon as it is between turns with nothing running in the background. Its conversation carries on, and a worker with work in hand is told to continue.</p>
        {counts.refused > 0 ? (
          <>
            <small role="status">Claude did not restart {workers(counts.refused)} when asked. It refuses while Remote Control is connected or background work is running, and Swarm asks only once for each release.</small>
            <button className="secondary-button" type="button" disabled={busy} onClick={() => void onRestart()}>Ask again</button>
          </>
        ) : (
          <small>Nothing is interrupted. A worker that is busy is restarted after it finishes.</small>
        )}
      </article>
    );
  }
  const count = counts.stopAndStart;
  return (
    <article className="runtime-subsystem-card runtime-subsystem-restart provider-release-action" aria-label="Provider release status">
      <header>
        <div><span className="runtime-component-name">Providers</span><strong>Update installed · restart to run it</strong></div>
        <span className="runtime-status-badge restart">Restart required</span>
      </header>
      <p>{installed}, and {count} running worker{count === 1 ? "" : "s"} started before that, so {count === 1 ? "it is" : "they are"} still running the older release.</p>
      <small>A provider keeps executing the release it started with. Restarting reloads {count === 1 ? "that worker" : "those workers"} from their saved conversations; identities, tasks, and terminal history remain durable.</small>
      {counts.inPlace > 0 ? <small>Swarm moves the other {workers(counts.inPlace)} onto it by itself, with /restart, without interrupting them.</small> : null}
      {!confirming ? (
        <button className="secondary-button" type="button" disabled={busy} onClick={() => setConfirming(true)}>Restart {workers(count)}</button>
      ) : (
        <div className="maintenance-confirmation" role="group" aria-label="Confirm provider restart">
          <strong>Restart {workers(count)} now?</strong>
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
