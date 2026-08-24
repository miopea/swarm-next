import { useCallback, useEffect, useState } from "react";

import { readTunnel, startTunnel, stopTunnel, type TunnelStatus } from "../api";

type Props = { busy: boolean; operatorToken: string };

/**
 * A temporary public address, for getting this Hive onto a phone.
 *
 * Deliberately framed as the first five minutes rather than as how a Hive is
 * published. The hostname is random and changes every time, and three things
 * are bound to an origin — passkeys are registered against a domain, an
 * installed PWA is a different app on a different origin, and the session
 * cookie is per-origin. Saying so here is cheaper than the operator finding out
 * by re-registering a passkey that stopped working overnight.
 */
export default function RemoteAccessSettings({ busy, operatorToken }: Props) {
  const [status, setStatus] = useState<TunnelStatus>();
  const [working, setWorking] = useState(false);
  const [error, setError] = useState("");

  const refresh = useCallback(async () => {
    try {
      setStatus(await readTunnel(operatorToken));
    } catch {
      // A Hive that cannot answer shows the card as unavailable rather than
      // raising an error about a feature the operator may never use.
    }
  }, [operatorToken]);

  useEffect(() => { void refresh(); }, [refresh]);

  // Keep asking while the address is being checked.
  //
  // Moving the reachability check off the request took the 45-second block out
  // of the browser, and left the card with no reason to look again — so it sat
  // on "Checking the address is reachable" for good, whether the address came
  // up or the tunnel was stopped for never serving. An asynchronous answer
  // nobody reads is not an answer.
  const awaitingAddress = status?.running === true && status.serving === false;
  useEffect(() => {
    if (!awaitingAddress) return undefined;
    const timer = window.setInterval(() => { void refresh(); }, 2_000);
    return () => window.clearInterval(timer);
  }, [awaitingAddress, refresh]);

  async function run(action: (token: string) => Promise<TunnelStatus>, failure: string) {
    setWorking(true);
    setError("");
    try {
      setStatus(await action(operatorToken));
    } catch (caught) {
      setError(caught instanceof Error ? caught.message : failure);
      await refresh();
    } finally {
      setWorking(false);
    }
  }

  const disabled = busy || working;
  const running = status?.running === true;

  return (
    <section id="settings-remote" className="settings-card remote-access-settings" aria-labelledby="remote-access-heading">
      <h3 id="remote-access-heading">Open on my phone</h3>
      <p>
        Publishes this Hive at a temporary web address so you can scan it and carry on from another
        device. Nothing is exposed until you start it, and it stops when you say.
      </p>

      {status && !status.available && !running ? (
        <p className="form-message" role="status">
          This needs <code>cloudflared</code> installed on the machine running Swarm.
        </p>
      ) : null}

      {/* The address exists but has not answered yet. Saying so beats a spinner:
          cloudflared's own banner warns it "may take some time to be reachable",
          and an operator watching nothing happen assumes it has hung. */}
      {running && status?.serving === false ? (
        <p className="form-message" role="status">
          Checking the address is reachable before showing it. Cloudflare can take a
          few seconds to start serving a new address.
        </p>
      ) : null}

      {status?.error && !running ? (
        <p className="form-error" role="alert">{status.error}</p>
      ) : null}

      {running && status?.serving && status?.url ? (
        <>
          <div className="tunnel-address">
            {status.qr_svg ? (
              <div className="tunnel-qr" aria-hidden="true" dangerouslySetInnerHTML={{ __html: status.qr_svg }} />
            ) : null}
            <div>
              <a href={status.url} target="_blank" rel="noreferrer noopener">{status.url}</a>
              <small>
                Scan it, then sign in with your operator token. The QR carries the address only —
                a token in a link ends up in browser history and in every log it passes through.
              </small>
            </div>
          </div>
          <p className="form-message" role="status">
            This address is temporary. It changes every time you start it, and anything tied to the
            address — a passkey, an installed app icon, a signed-in session — will not follow it.
            For an address that lasts, put a named tunnel on a domain you own.
          </p>
        </>
      ) : null}

      <div className="settings-actions">
        {running ? (
          <button type="button" className="danger-action" disabled={disabled} onClick={() => void run(stopTunnel, "The tunnel could not be stopped.")}>
            {working ? "Stopping…" : "Stop sharing"}
          </button>
        ) : (
          <button type="button" className="primary-action" disabled={disabled || status?.available === false} onClick={() => void run(startTunnel, "The tunnel could not be started.")}>
            {working ? "Opening the address…" : "Open on my phone"}
          </button>
        )}
      </div>

      {error && <p className="form-error" role="alert">{error}</p>}
    </section>
  );
}
