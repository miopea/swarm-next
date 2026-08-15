import { useEffect, useState } from "react";

import {
  fetchApiaryHiveCandidates,
  inviteApiaryHiveCandidate,
  pinApiaryHiveCandidate,
  type ApiaryHiveCandidate,
  type HiveConnectionCard,
} from "../api";
import { createApiaryHandoffLink, readApiaryHandoffLink } from "./apiaryHandoff";
import {
  ApiaryExchangeStep,
  ApiaryFileFallback,
  ApiaryGeneratedLink,
  ApiaryLinkEntry,
} from "./ApiaryHandoffControls";

type Props = {
  busy: boolean;
  operatorToken: string;
  onInvitationCreated: () => Promise<void>;
};

export default function KeeperInvitationManager({ busy, operatorToken, onInvitationCreated }: Props) {
  const [candidates, setCandidates] = useState<ApiaryHiveCandidate[]>([]);
  const [connectionLink, setConnectionLink] = useState("");
  const [generatedLink, setGeneratedLink] = useState("");
  const [working, setWorking] = useState(false);
  const [loadError, setLoadError] = useState(false);
  const [message, setMessage] = useState("");
  const [error, setError] = useState("");

  useEffect(() => {
    let cancelled = false;
    void fetchApiaryHiveCandidates(operatorToken)
      .then((value) => {
        if (cancelled) return;
        setCandidates(value);
        setLoadError(false);
      })
      .catch(() => { if (!cancelled) setLoadError(true); });
    return () => { cancelled = true; };
  }, [operatorToken]);

  async function refreshCandidates() {
    setCandidates(await fetchApiaryHiveCandidates(operatorToken));
    setLoadError(false);
  }

  async function importConnectionCard(file: File | undefined) {
    if (!file) return;
    await perform(async () => {
      if (file.size > 64 * 1024) throw new Error("That connection card is unexpectedly large.");
      const card = JSON.parse(await file.text()) as HiveConnectionCard;
      const candidate = await pinApiaryHiveCandidate(operatorToken, card);
      await refreshCandidates();
      setMessage(`${candidate.hive_name} is verified and pinned. No membership or access was granted.`);
    }, "That Hive connection card could not be verified.");
  }

  async function importConnectionLink() {
    await perform(async () => {
      const card = readApiaryHandoffLink<HiveConnectionCard>(connectionLink, "connection");
      const candidate = await pinApiaryHiveCandidate(operatorToken, card);
      await refreshCandidates();
      setConnectionLink("");
      setMessage(`${candidate.hive_name} is verified and pinned. No membership or access was granted.`);
    }, "That Hive connection link could not be verified.");
  }

  async function createInvitation(candidate: ApiaryHiveCandidate) {
    await perform(async () => {
      const bundle = await inviteApiaryHiveCandidate(operatorToken, candidate.hive_id);
      const link = createApiaryHandoffLink("invitation", bundle);
      setGeneratedLink(link);
      const copied = await copyHandoffLink(link);
      await Promise.all([refreshCandidates(), onInvitationCreated()]);
      setMessage(copied
        ? `Invitation link for ${candidate.hive_name} copied. It is bound to that Hive, expires, and can be used only once.`
        : `Invitation link for ${candidate.hive_name} created. Copy it below and share it privately.`);
    }, "The one-time invitation could not be created.");
  }

  async function perform(action: () => Promise<void>, fallback: string) {
    setWorking(true);
    setError("");
    setMessage("");
    try {
      await action();
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : fallback);
    } finally {
      setWorking(false);
    }
  }

  async function copyHandoffLink(link: string): Promise<boolean> {
    try {
      await navigator.clipboard.writeText(link);
      return true;
    } catch {
      return false;
    }
  }

  return (
    <div className="apiary-hive-candidates">
      <div>
        <strong>Invite a Hive</strong>
        <small>The other operator sends you a short-lived connection link. Swarm verifies its signed identity before it enables an invitation.</small>
      </div>
      <ol className="apiary-exchange-guide apiary-keeper-exchange" aria-label="How to invite a Hive">
        <ApiaryExchangeStep number="1" title="Receive her connection link" detail="Ask the Hive operator to copy her link under Join another Keeper's Apiary and send it privately to you." />
        <ApiaryExchangeStep number="2" title="Verify the exact identity" detail="Paste the link below. Swarm verifies its signature and shows the Hive and operator before any invitation exists." />
        <ApiaryExchangeStep number="3" title="Return the invitation link" detail="Create invitation copies one bounded link for that exact Hive. It expires and its secret can be consumed only once." />
      </ol>
      <ApiaryLinkEntry label="Hive connection link" value={connectionLink} action={working ? "Verifying…" : "Verify Hive"} disabled={busy || working} onChange={setConnectionLink} onAction={() => void importConnectionLink()} />
      <ApiaryFileFallback summary="Use a connection file instead" ariaLabel="Choose Hive connection card" disabled={busy || working} label={working ? "Verifying…" : "Choose connection card"} detail="or drop the Hive's .json connection card here" onFile={(file) => void importConnectionCard(file)} />
      {loadError ? <p className="apiary-blockers">Pinned Hive identities could not be refreshed. No membership changed.</p> : null}
      {candidates.length > 0 ? (
        <ul className="apiary-candidate-list" aria-label="Pinned Hive identities">
          {candidates.map((candidate) => (
            <li key={candidate.hive_id}>
              <span><strong>{candidate.hive_name}</strong><small>{candidate.operator_display_name}</small></span>
              <span className="apiary-candidate-state">
                <span className="readiness-ready">{candidate.invitation_pending ? "Invitation pending" : "Identity pinned"}</span>
                <button
                  className="secondary-button"
                  disabled={working || candidate.invitation_pending}
                  onClick={() => void createInvitation(candidate)}
                >{candidate.invitation_pending ? "Invitation created" : "Create invitation"}</button>
              </span>
            </li>
          ))}
        </ul>
      ) : <p className="empty-copy">No other Hive identities are pinned yet.</p>}
      {generatedLink ? <ApiaryGeneratedLink link={generatedLink} onCopy={copyHandoffLink} /> : null}
      {message ? <p className="form-message" role="status">{message}</p> : null}
      {error ? <p className="form-error" role="alert">{error}</p> : null}
    </div>
  );
}
