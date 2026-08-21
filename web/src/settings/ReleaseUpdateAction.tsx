import { useCallback, useEffect, useState } from "react";

import {
  applyRelease,
  checkForRelease,
  downloadRelease,
  fetchReleaseStatus,
  setReleaseCheckMode,
  type ReleaseStatus,
} from "../api";

/**
 * What the install unit's refusal codes mean.
 *
 * It records one and nothing read it, so a refusal reached the operator as a
 * bare "did not run" with the explanation sitting in a file on disk.
 */
function refusalReason(code: string): string {
  if (code === "outside-downloads") return "the request named a directory outside the download folder";
  if (code === "missing") return "the downloaded release was no longer there";
  if (code === "not-a-release") return "the downloaded directory is not a Swarm release";
  return code;
}

type Props = {
  busy: boolean;
  operatorToken: string;
};

/**
 * Whether this Hive looks for releases, and what to do about one it found.
 *
 * The question is asked rather than defaulted. "A Hive never contacts an
 * origin its owner did not choose" is satisfied by defaulting to off, but an
 * operator who never sees the switch has not chosen either — so an unanswered
 * Hive shows the question and a Hive that answered shows the answer.
 */
export default function ReleaseUpdateAction({ busy, operatorToken }: Props) {
  const [status, setStatus] = useState<ReleaseStatus | null>(null);
  const [working, setWorking] = useState(false);
  const [error, setError] = useState("");
  const [confirming, setConfirming] = useState(false);
  const [installed, setInstalled] = useState(false);

  useEffect(() => {
    let cancelled = false;
    void (async () => {
      try {
        const current = await fetchReleaseStatus(operatorToken);
        if (!cancelled) setStatus(current);
      } catch {
        // A status this card cannot read is not an error worth a banner: the
        // card simply does not appear.
      }
    })();
    return () => { cancelled = true; };
  }, [operatorToken]);

  const run = useCallback(async (action: () => Promise<ReleaseStatus>, failure: string) => {
    setWorking(true);
    setError("");
    try {
      setStatus(await action());
    } catch (caught) {
      setError(caught instanceof Error ? caught.message : failure);
    } finally {
      setWorking(false);
    }
  }, []);

  if (!status?.available) return null;
  const disabled = busy || working;

  if (status.mode === "unset") {
    return (
      <article className="runtime-subsystem-card runtime-subsystem-safe release-update-action" aria-label="Release updates">
        <header>
          <div><span className="runtime-component-name">Updates</span><strong>Check for new Swarm releases?</strong></div>
        </header>
        <p>Swarm can look once a day for a new release and tell you when one exists. It sends nothing — no version, no identity, no counts — and fetches one small signed file.</p>
        {status.development_build ? (
          <p><strong>This Hive builds from a working copy.</strong> Checking would only tell you a release exists — it will never offer to install one, because replacing a build made from your checkout would discard work nothing can enumerate. Your updates come from the App and API card.</p>
        ) : null}
        <small>Until you choose, this Hive contacts nothing.</small>
        <div className="settings-actions">
          <button className="primary-action" disabled={disabled} onClick={() => void run(() => setReleaseCheckMode(operatorToken, "daily"), "The preference could not be saved.")}>Check daily</button>
          <button className="secondary-button" disabled={disabled} onClick={() => void run(() => setReleaseCheckMode(operatorToken, "off"), "The preference could not be saved.")}>Don’t check</button>
        </div>
        {error && <p className="form-error" role="alert">{error}</p>}
      </article>
    );
  }

  const offered = status.upgrade_available && status.offer;
  const ready = status.downloaded_version !== null;

  return (
    <article
      className="runtime-subsystem-card runtime-subsystem-safe release-update-action"
      aria-label="Release updates"
    >
      <header>
        <div>
          <span className="runtime-component-name">Updates</span>
          <strong>{offered ? `Swarm ${status.offer?.version} is available` : status.development_build ? "This Hive builds from a working copy" : "Swarm is up to date"}</strong>
        </div>
        {offered && <span className="runtime-status-badge safe">Workers stay online</span>}
      </header>

      {status.development_build && status.offer && (
        <p>Version {status.offer.version} has been released. Nothing is offered here, because replacing a build made from your checkout would discard work nothing can enumerate. Rebuild from the App and API card instead.</p>
      )}

      {offered && !status.development_build && (
        <>
          <p>Installing this updates the app and API. Your workers keep running — Swarm restarts the API and leaves the terminal engine they are attached to alone.</p>
          {status.carries_new_worker_engine && (
            <p>It also carries a newer worker engine. That part is <strong>deferred while any worker is running</strong> and applied once they are idle, or when you ask for it from the worker engine card. Applying it restarts workers and brings back the ones loaded from their saved conversations.</p>
          )}
          {status.offer?.notes_url && <p><a href={status.offer.notes_url} target="_blank" rel="noreferrer noopener">What changed in {status.offer.version}</a></p>}
          {status.apply_state === "failed" || status.apply_state === "refused" ? (
            <p className="form-error" role="alert">
              The install did not run. Nothing was changed and this Hive is still on {status.current_version}.
              {status.apply_reason ? <> Reason: <strong>{refusalReason(status.apply_reason)}</strong></> : null}
              {" "}<code>journalctl --user -u swarm-release-apply.service -n 30</code> says more.
            </p>
          ) : null}
          {installed && status.apply_state !== "failed" && status.apply_state !== "refused" ? (
            <p className="form-message" role="status">Installing. Swarm restarts the API on its own; this page reconnects when it answers again. If nothing changes after a minute, the install did not start — check <code>systemctl --user status swarm-release-apply.path</code>.</p>
          ) : ready ? (
            confirming ? (
              <div className="maintenance-confirmation" role="group" aria-label="Confirm release install">
                <strong>Install Swarm {status.offer?.version} now?</strong>
                <span>Workers stay online.{status.carries_new_worker_engine ? " The newer worker engine waits until they are idle." : ""} Your Hive database, tasks and settings are kept, and the previous release is restored automatically if the new one does not answer.</span>
                <div className="settings-actions">
                  <button className="secondary-button" disabled={disabled} onClick={() => setConfirming(false)}>Not now</button>
                  <button
                    className="primary-action"
                    disabled={disabled}
                    onClick={() => {
                      setConfirming(false);
                      setWorking(true);
                      void applyRelease(operatorToken)
                        .then(() => setInstalled(true))
                        .catch((caught: unknown) => setError(caught instanceof Error ? caught.message : "The install could not be started."))
                        .finally(() => setWorking(false));
                    }}
                  >Install {status.offer?.version}</button>
                </div>
              </div>
            ) : (
              <div className="settings-actions">
                <button className="primary-action" disabled={disabled} onClick={() => setConfirming(true)}>Install Swarm {status.offer?.version}</button>
              </div>
            )
          ) : (
            <div className="settings-actions">
              <button className="primary-action" disabled={disabled} onClick={() => void run(() => downloadRelease(operatorToken), "The release could not be downloaded.")}>{working ? "Downloading and verifying…" : "Download Swarm " + (status.offer?.version ?? "")}</button>
            </div>
          )}
          {ready && !installed && <small>Downloaded and verified against the signed digest. Nothing is installed until you say so.</small>}
        </>
      )}

      {!offered && !status.development_build && (
        <p>Version {status.current_version} is the newest release for this Hive.</p>
      )}

      <footer className="release-check-footer">
        <small>
          {status.mode === "daily" ? "Checked daily. " : "Automatic checks are off. "}
          {status.last_outcome === "unreachable" ? "The last check could not reach the origin." : status.last_outcome === "rejected" ? "The last check found a manifest it could not verify, and ignored it." : status.last_checked_at ? `Last checked ${new Date(status.last_checked_at * 1000).toLocaleString()}.` : "Not checked yet."}
        </small>
        <span className="settings-actions">
          <button className="secondary-button" disabled={disabled} onClick={() => void run(() => checkForRelease(operatorToken), "The check could not be completed.")}>{working ? "Checking…" : "Check now"}</button>
          <button className="secondary-button" disabled={disabled} onClick={() => void run(() => setReleaseCheckMode(operatorToken, status.mode === "daily" ? "off" : "daily"), "The preference could not be saved.")}>{status.mode === "daily" ? "Stop checking" : "Check daily"}</button>
        </span>
      </footer>
      {error && <p className="form-error" role="alert">{error}</p>}
    </article>
  );
}
