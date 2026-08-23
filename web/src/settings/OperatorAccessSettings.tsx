import { useCallback, useEffect, useState } from "react";

import { createBrowserSession, rotateOperatorToken } from "../api";
import { listPasskeys, passkeysSupported, registerPasskey, removePasskey, type RegisteredPasskey } from "./passkeys";

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
  const [passkeys, setPasskeys] = useState<RegisteredPasskey[]>([]);
  const [passkeyLabel, setPasskeyLabel] = useState("");
  const [passkeyBusy, setPasskeyBusy] = useState(false);
  const [passkeyError, setPasskeyError] = useState("");

  const refreshPasskeys = useCallback(async () => {
    try {
      setPasskeys(await listPasskeys(operatorToken));
    } catch {
      // A Hive that cannot list them simply shows none; this is not the place
      // to raise an error about a feature the operator may never use.
    }
  }, [operatorToken]);

  useEffect(() => { void refreshPasskeys(); }, [refreshPasskeys]);

  async function addPasskey() {
    setPasskeyBusy(true);
    setPasskeyError("");
    try {
      await registerPasskey(operatorToken, passkeyLabel.trim() || "This device");
      setPasskeyLabel("");
      await refreshPasskeys();
    } catch (caught) {
      setPasskeyError(caught instanceof Error ? caught.message : "This passkey could not be registered.");
    } finally {
      setPasskeyBusy(false);
    }
  }

  async function forgetPasskey(credentialId: string) {
    setPasskeyBusy(true);
    setPasskeyError("");
    try {
      await removePasskey(operatorToken, credentialId);
      await refreshPasskeys();
    } catch (caught) {
      setPasskeyError(caught instanceof Error ? caught.message : "This passkey could not be removed.");
    } finally {
      setPasskeyBusy(false);
    }
  }

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

      <div className="passkey-settings">
        <h4>Passkeys</h4>
        <p>
          A passkey signs you in with the device you are holding, so there is nothing to copy and
          nothing to keep in a password manager. The token above stays as the way back in if you
          lose the device.
        </p>
        {!passkeysSupported() ? (
          <p className="form-message" role="status">This browser cannot use passkeys.</p>
        ) : (
          <>
            <label htmlFor="passkey-label">
              <span>Name this device</span>
              <input
                id="passkey-label"
                type="text"
                value={passkeyLabel}
                disabled={disabled || passkeyBusy}
                onChange={(event) => setPasskeyLabel(event.target.value)}
                placeholder="Work phone"
              />
            </label>
            <div className="settings-actions">
              <button type="button" className="primary-action" disabled={disabled || passkeyBusy} onClick={() => void addPasskey()}>
                {passkeyBusy ? "Waiting for the device…" : "Add a passkey"}
              </button>
            </div>
            <small>
              Registered against the address you are at now. A passkey created here does not work at a
              different address, which is why each one says where it belongs.
            </small>
          </>
        )}
        {passkeys.length > 0 && (
          <ul className="passkey-list">
            {passkeys.map((passkey) => (
              <li key={passkey.credential_id}>
                <span>
                  <strong>{passkey.label}</strong>
                  <small>
                    {passkey.relying_party}
                    {passkey.usable_here ? "" : " · not usable at this address"}
                    {passkey.last_used_at ? ` · last used ${new Date(passkey.last_used_at * 1000).toLocaleDateString()}` : " · never used"}
                  </small>
                </span>
                <button type="button" className="danger-link" disabled={disabled || passkeyBusy} onClick={() => void forgetPasskey(passkey.credential_id)}>
                  Remove
                </button>
              </li>
            ))}
          </ul>
        )}
        {passkeyError && <p className="form-error" role="alert">{passkeyError}</p>}
      </div>
    </section>
  );
}
