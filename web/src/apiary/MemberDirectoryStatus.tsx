import { useEffect, useState } from "react";
import { fetchApiaryDirectory, requestApiarySyncRetry, type ApiaryDirectory, type ApiaryMember } from "../api";

/** Display provenance only: a directory snapshot is not live worker presence. */
export default function MemberDirectoryStatus({ operatorToken, members, onRefresh }: {
  operatorToken: string; members: ApiaryMember[]; onRefresh: () => void;
}) {
  const [directory, setDirectory] = useState<ApiaryDirectory | null>();
  const [state, setState] = useState<"loading" | "ready" | "failed">("loading");
  const [attempt, setAttempt] = useState(0);
  const [retrying, setRetrying] = useState(false);
  const [retryMessage, setRetryMessage] = useState("");
  async function retrySynchronization() {
    setRetrying(true);
    setRetryMessage("");
    try {
      await requestApiarySyncRetry(operatorToken);
      setRetryMessage("Synchronization retry requested. Membership is unchanged; this is not yet a successful connection.");
      onRefresh();
    } catch {
      setRetryMessage("The retry could not be requested. Membership and credentials are unchanged; try again.");
    } finally { setRetrying(false); }
  }
  useEffect(() => {
    const controller = new AbortController();
    setState("loading");
    void fetchApiaryDirectory(operatorToken, controller.signal).then((value) => {
      if (controller.signal.aborted) return;
      setDirectory(value);
      setState("ready");
    }).catch(() => { if (!controller.signal.aborted) setState("failed"); });
    return () => controller.abort();
  }, [operatorToken, members, attempt]);
  return <div className="member-sync-copy" aria-label="Member directory status">
    {state === "loading" ? <p>Checking the saved member list…</p> : state === "failed"
      ? <p>Member list freshness could not be checked. The names shown may be incomplete or out of date.</p>
      : directory === null ? <p>The full member list has not arrived yet. The names shown may be incomplete. Check synchronization and that the Keeper supports directory sharing; do not leave and rejoin.</p> : null}
    {directory ? <p>Keeper directory issued {new Date(directory.issued_at * 1000).toLocaleString()}. This is a saved snapshot, not live presence.</p> : null}
    {directory && directory.expires_at * 1000 <= Date.now() ? <p>This snapshot is past its verification window. Refresh to check for membership changes; existing Hive work stays available.</p> : null}
    {state !== "loading" ? <button type="button" className="secondary-button" onClick={() => { setAttempt((value) => value + 1); onRefresh(); }}>Refresh member list</button> : null}
    <button type="button" className="secondary-button" disabled={retrying} onClick={() => void retrySynchronization()}>{retrying ? "Requesting retry…" : "Retry Apiary synchronization"}</button>
    {retryMessage ? <p role="status">{retryMessage}</p> : null}
  </div>;
}
