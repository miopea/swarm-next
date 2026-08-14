import { useState } from "react";

type Props = {
  busy: boolean;
  enabled: boolean;
  onReload: () => Promise<void>;
};

export default function DevelopmentReloadAction({ busy, enabled, onReload }: Props) {
  const [confirming, setConfirming] = useState(false);
  if (!enabled) return null;
  return (
    <div className="maintenance-action development-reload-action">
      <p><strong>Development checkout connected.</strong> Build the current working copy and switch this same app to it. A failed compile leaves the current release running, and active worker terminals stay attached to the independent worker engine.</p>
      {!confirming ? (
        <button className="secondary-button" disabled={busy} onClick={() => setConfirming(true)}>Reload development build</button>
      ) : (
        <div className="maintenance-confirmation" role="group" aria-label="Confirm development reload">
          <strong>Build and activate the working copy?</strong>
          <span>This can take several minutes. Swarm will reopen itself when the new API and browser build are healthy.</span>
          <div className="settings-actions">
            <button className="secondary-button" disabled={busy} onClick={() => setConfirming(false)}>Not now</button>
            <button className="primary-action" disabled={busy} onClick={() => { setConfirming(false); void onReload(); }}>Build and reload</button>
          </div>
        </div>
      )}
    </div>
  );
}
