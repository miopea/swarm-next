import { useState } from "react";

type Props = {
  busy: boolean;
  runtime?: import("../api").DevelopmentRuntime;
  onReload: () => Promise<void>;
};

export default function DevelopmentReloadAction({ busy, runtime, onReload }: Props) {
  const [confirming, setConfirming] = useState(false);
  if (!runtime?.enabled) return null;
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
    return <article className="runtime-subsystem-card runtime-subsystem-safe development-reload-action" aria-label="App and API status" role="status"><header><div><span className="runtime-component-name">App and API</span><strong>Building development changes</strong></div><span className="runtime-status-badge safe">Workers stay online</span></header><p>Revision {runningRevision} remains active while Swarm compiles and checks {workingRevision}.</p><small>The page reconnects only after the new browser and API build is healthy. Claude, Codex, and the worker engine keep running.</small></article>;
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
    return <article className="runtime-subsystem-card runtime-subsystem-current development-reload-action" aria-label="App and API status"><header><div><span className="runtime-component-name">App and API</span><strong>Running build matches the working copy</strong></div><span className="runtime-status-badge current">Current</span></header><p>Active revision {runningRevision} matches the product code in this checkout. No App/API build is waiting.</p><small>Swarm checks the working copy every 15 seconds. When product code changes, you can build and activate it without restarting Claude, Codex, or the worker engine.</small></article>;
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

function deployedRevision(version: string) {
  return version.match(/-(?:dev-)?([0-9a-f]{7,40})(?:-|$)/i)?.[1]?.slice(0, 7) ?? "the current build";
}

function shortRevision(revision?: string | null) {
  return revision?.slice(0, 7);
}
