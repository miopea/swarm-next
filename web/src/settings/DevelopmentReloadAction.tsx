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
    return <div className="maintenance-action development-reload-action" role="status"><p><strong>Development build in progress.</strong> Workers remain online while Swarm builds and checks the working copy.</p></div>;
  }
  if (!runtime.reload_available) {
    return <div className="maintenance-action development-reload-action"><p><strong>Development build is current.</strong> The API and browser already match the product code in this working copy. No reload is needed.</p></div>;
  }
  return (
    <div className="maintenance-action development-reload-action">
      <p><strong>Development changes are ready.</strong> Build the current working copy and switch this same app to it with no worker interruption. The page briefly reconnects, but Claude and Codex processes keep running in the separate worker engine. A failed compile leaves the current release active.</p>
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
    </div>
  );
}
