import { useState } from "react";

type Props = {
  busy: boolean;
  runtime?: import("../api").DevelopmentRuntime;
  onReload: () => Promise<void>;
};

export default function DevelopmentReloadAction({ busy, runtime, onReload }: Props) {
  const [confirming, setConfirming] = useState(false);
  if (!runtime?.enabled) return null;
  if (runtime.state === "requested" || runtime.state === "building") {
    return <article className="runtime-subsystem-card runtime-subsystem-safe development-reload-action" aria-label="App and API status" role="status"><header><div><span className="runtime-component-name">App and API</span><strong>Building development changes</strong></div><span className="runtime-status-badge safe">No worker restart</span></header><p>Swarm is compiling and checking the working copy. This page will reconnect; every worker keeps running.</p></article>;
  }
  if (!runtime.reload_available) {
    return <article className="runtime-subsystem-card runtime-subsystem-current development-reload-action" aria-label="App and API status"><header><div><span className="runtime-component-name">App and API</span><strong>Development build is current</strong></div><span className="runtime-status-badge current">Current</span></header><p>The browser and API match the product code in the working copy. No action is needed.</p><small>Updating this layer never restarts Claude, Codex, or the worker engine.</small></article>;
  }
  return (
    <article className="runtime-subsystem-card runtime-subsystem-safe development-reload-action" aria-label="App and API status">
      <header><div><span className="runtime-component-name">App and API</span><strong>Development changes are ready</strong></div><span className="runtime-status-badge safe">No worker restart</span></header>
      <p>Build and switch this app to the working copy. The page briefly reconnects, but Claude and Codex continue in the separate worker engine.</p>
      <small>A failed compile is rejected and the current app remains active.</small>
      {!confirming ? (
        <button className="secondary-button" disabled={busy} onClick={() => setConfirming(true)}>Reload development build</button>
      ) : (
        <div className="maintenance-confirmation" role="group" aria-label="Confirm development reload">
          <strong>Build and activate the working copy?</strong>
          <span>This can take several minutes. Swarm will reopen itself when the new API and browser build are healthy; the separate worker engine is not restarted.</span>
          <div className="settings-actions">
            <button className="secondary-button" disabled={busy} onClick={() => setConfirming(false)}>Not now</button>
            <button className="primary-action" disabled={busy} onClick={() => { setConfirming(false); void onReload(); }}>Build and reload</button>
          </div>
        </div>
      )}
    </article>
  );
}
