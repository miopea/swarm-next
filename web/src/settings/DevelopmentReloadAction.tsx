import { useState } from "react";
import { deployedRevision, runtimeVersionIdentity, shortRevision } from "./runtimeVersion";

type Props = {
  busy: boolean;
  runtime?: import("../api").DevelopmentRuntime;
  /** Whether the API is currently answering. False while it restarts under a reload. */
  reachable?: boolean;
  healthVersion?: string;
  onReload: () => Promise<void>;
};

export default function DevelopmentReloadAction({ busy, runtime, reachable = true, healthVersion, onReload }: Props) {
  const [confirming, setConfirming] = useState(false);
  if (!runtime?.enabled) return null;
  // Activating a build restarts the API, so this card loses contact in the
  // middle of the operation it is reporting. It holds its place and says so:
  // disappearing at that exact moment reads as the build having destroyed
  // something.
  if (!reachable) {
    return (
      <article className="runtime-subsystem-card runtime-subsystem-safe development-reload-action" aria-label="App and API status" role="status">
        <header><div><span className="runtime-component-name">App and API</span><strong>Waiting for the API to answer</strong></div><span className="runtime-status-badge safe">Workers stay online</span></header>
        <div className="maintenance-progress" aria-live="polite">
          <span className="maintenance-spinner" aria-hidden="true" />
          <div>
            <strong>Reconnecting…</strong>
            <span>The API is not answering right now, which is expected while a new build takes over. This page reconnects on its own.</span>
          </div>
        </div>
        <small>Claude, Codex, and the worker engine are not restarted by an App and API release.</small>
      </article>
    );
  }
  const runningRevision = shortRevision(runtime.deployed_source_revision) ?? deployedRevision(runtime.version);
  const workingRevision = shortRevision(runtime.source_revision) ?? "the working copy";
  if (runtime.state === "source_mismatch") return (
    <article className="runtime-subsystem-card runtime-subsystem-restart development-reload-action" aria-label="App and API status" role="alert">
      <header><div><span className="runtime-component-name">App and API</span><strong>Development checkout needs to catch up</strong></div><span className="runtime-status-badge restart">Reload blocked</span></header>
      <p>Revision {runningRevision} is active, but working-copy revision {workingRevision} does not contain that deployed source.</p>
      <small>Swarm will not build or activate an older or unrelated checkout. Update the development checkout first; Claude, Codex, and the worker engine remain online.</small>
    </article>
  );
  if (runtime.state === "requested" || runtime.state === "building") {
    // The same inline progress the worker engine card shows. A build that only
    // changes the wording on a card reads as another resting state, which is
    // why this one ran without the operator being able to find it.
    return (
      <article className="runtime-subsystem-card runtime-subsystem-safe development-reload-action" aria-label="App and API status" role="status">
        <header><div><span className="runtime-component-name">App and API</span><strong>Building development changes</strong></div><span className="runtime-status-badge safe">Workers stay online</span></header>
        <div className="maintenance-progress" aria-live="polite">
          <span className="maintenance-spinner" aria-hidden="true" />
          <div>
            <strong>{runtime.state === "requested" ? "Starting the App and API build…" : "Building App and API…"}</strong>
            <span>Compiling and checking {workingRevision}. Revision {runningRevision} keeps serving this page until the new build is healthy.</span>
          </div>
        </div>
        <small>The page reconnects only after the new browser and API build is healthy. Claude, Codex, and the worker engine keep running.</small>
      </article>
    );
  }
  if (runtime.state === "failed") return (
    <article className="runtime-subsystem-card runtime-subsystem-restart development-reload-action" aria-label="App and API status" role="alert">
      <header><div><span className="runtime-component-name">App and API</span><strong>Development build failed</strong></div><span className="runtime-status-badge restart">Current app preserved</span></header>
      <p>Revision {runningRevision} is still serving this page. The attempted {workingRevision} build was rejected before activation.</p>
      <small>Workers were never restarted or interrupted. Check the development reload service log before retrying.</small>
      {runtime.reload_available && !confirming ? <button className="secondary-button" disabled={busy} onClick={() => setConfirming(true)}>Retry development build</button> : null}
      {confirming ? <ReloadConfirmation busy={busy} onCancel={() => setConfirming(false)} onConfirm={() => { setConfirming(false); void onReload(); }} /> : null}
    </article>
  );
  if (!runtime.reload_available) {
    return <article className="runtime-subsystem-card runtime-subsystem-current development-reload-action" aria-label="App and API status"><header><div><span className="runtime-component-name">App and API</span><strong>Running build matches the working copy</strong></div><span className="runtime-status-badge current">Current</span></header><p className="runtime-version"><strong>Installed</strong> {runtimeVersionIdentity(healthVersion ?? runtime.version)}</p><p>Active revision {runningRevision} matches the product code in this checkout. No App/API build is waiting.</p><small>Swarm checks the working copy every 15 seconds. When product code changes, you can build and activate it without restarting Claude, Codex, or the worker engine.</small></article>;
  }
  return (
    <article className="runtime-subsystem-card runtime-subsystem-safe development-reload-action" aria-label="App and API status">
      <header><div><span className="runtime-component-name">App and API</span><strong>Development reload available</strong></div><span className="runtime-status-badge safe">Workers stay online</span></header>
      <p>Revision {runningRevision} is active. Build and switch the browser and API to working-copy revision {workingRevision}.</p>
      <small>A failed compile is rejected and {runningRevision} remains active. Claude, Codex, and the worker engine are not restarted.</small>
      {!confirming ? (
        <button className="secondary-button" disabled={busy} onClick={() => setConfirming(true)}>Reload development build</button>
      ) : (
        <ReloadConfirmation busy={busy} onCancel={() => setConfirming(false)} onConfirm={() => { setConfirming(false); void onReload(); }} />
      )}
    </article>
  );
}

function ReloadConfirmation({ busy, onCancel, onConfirm }: { busy: boolean; onCancel: () => void; onConfirm: () => void }) {
  return <div className="maintenance-confirmation" role="group" aria-label="Confirm development reload">
    <strong>Build and activate the working copy?</strong>
    <span>This can take several minutes. Swarm reopens only after the new API and browser build are healthy; the separate worker engine is not restarted.</span>
    <div className="settings-actions">
      <button className="secondary-button" disabled={busy} onClick={onCancel}>Not now</button>
      <button className="primary-action" disabled={busy} onClick={onConfirm}>Build and reload</button>
    </div>
  </div>;
}

