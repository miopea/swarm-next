import { useState } from "react";

import { createBrowserSession, rotateOperatorToken } from "../api";

type Props = {
  busy: boolean;
  operatorToken: string;
};

/** Long enough not to be guessed, and the same shape the installer generates. */
function generatedToken(): string {
  const bytes = new Uint8Array(32);
  crypto.getRandomValues(bytes);
  return Array.from(bytes, (byte) => byte.toString(16).padStart(2, "0")).join("");
}

/**
 * Changing the operator token without editing a file and restarting a service.
 *
 * The operator lost an hour to a stale token in a password manager, and the
 * only remedy at the time was `swarm.env` and `systemctl`. Neither is available
 * from a phone, which is where they were.
 */
export default function OperatorAccessSettings({ busy, operatorToken }: Props) {
  const [draft, setDraft] = useState("");
  const [confirming, setConfirming] = useState(false);
  const [working, setWorking] = useState(false);
  const [error, setError] = useState("");
  const [rotated, setRotated] = useState(false);

  const valid = draft.trim().length >= 16 && !/\s/.test(draft.trim());

  async function rotate() {
    setWorking(true);
    setError("");
    try {
      const next = draft.trim();
      await rotateOperatorToken(operatorToken, next);
      // Sign this device back in immediately. The rotation invalidated the
      // session making the request, so without this the operator is locked out
      // by their own change — the exact failure this card exists to end.
      await createBrowserSession(next);
      setRotated(true);
      setDraft("");
      setConfirming(false);
    } catch (caught) {
      setError(caught instanceof Error ? caught.message : "The token could not be changed.");
    } finally {
      setWorking(false);
    }
  }

  const disabled = busy || working;

  return (
    <section id="settings-access" className="settings-card" aria-labelledby="access-heading">
      <div>
        <p className="eyebrow">Access</p>
        <h3 id="access-heading">Your operator token</h3>
      </div>
      <p>
        One token opens this Hive from anywhere that is not this machine. Requests from the machine
        itself do not need it.
      </p>
      <p>
        Changing it <strong>signs out every device at once</strong>, including this one — which is the
        point of changing it. This browser is signed straight back in; anywhere else needs the new
        token.
      </p>

      <label htmlFor="operator-token-next">
        <span>New token</span>
        <input
          id="operator-token-next"
          type="text"
          autoComplete="off"
          spellCheck={false}
          value={draft}
          disabled={disabled}
          onChange={(event) => { setDraft(event.target.value); setConfirming(false); setRotated(false); }}
          placeholder="At least 16 characters, no spaces"
        />
      </label>
      <div className="settings-actions">
        <button type="button" className="secondary-button" disabled={disabled} onClick={() => { setDraft(generatedToken()); setConfirming(false); setRotated(false); }}>
          Generate one
        </button>
        {confirming ? (
          <>
            <button type="button" className="secondary-button" disabled={disabled} onClick={() => setConfirming(false)}>Not now</button>
            <button type="button" className="danger-button" disabled={disabled} onClick={() => void rotate()}>
              {working ? "Changing…" : "Sign out everywhere and change it"}
            </button>
          </>
        ) : (
          <button type="button" className="primary-action" disabled={disabled || !valid} onClick={() => setConfirming(true)}>
            Change token
          </button>
        )}
      </div>
      {rotated && (
        <p className="form-message" role="status">
          Token changed. Save it somewhere you trust — every other device needs it to sign in again.
        </p>
      )}
      {error && <p className="form-error" role="alert">{error}</p>}
      <small className="privacy-note">
        Stored with owner-only permissions in this Hive's configuration, and written there as well as
        applied now, so it survives a restart. It never leaves the machine.
      </small>
    </section>
  );
}
