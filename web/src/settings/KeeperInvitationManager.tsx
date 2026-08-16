import { useCallback, useEffect, useState } from "react";

import {
  approveApiaryJoinLink,
  createApiaryJoinLink,
  fetchApiaryJoinLinks,
  type ApiaryJoinLink,
  type ApiaryKeeperJoinCapability,
} from "../api";
import { createApiaryHandoffLink } from "./apiaryHandoff";
import { ApiaryGeneratedLink } from "./ApiaryHandoffControls";

type Props = {
  busy: boolean;
  operatorToken: string;
  onInvitationCreated: () => Promise<void>;
};

export default function KeeperInvitationManager({ busy, operatorToken, onInvitationCreated }: Props) {
  const [links, setLinks] = useState<ApiaryJoinLink[]>([]);
  const [generatedLink, setGeneratedLink] = useState("");
  const [working, setWorking] = useState(false);
  const [message, setMessage] = useState("");
  const [error, setError] = useState("");

  const refresh = useCallback(async () => {
    setLinks(await fetchApiaryJoinLinks(operatorToken));
  }, [operatorToken]);

  useEffect(() => {
    let cancelled = false;
    const load = async () => {
      try {
        const current = await fetchApiaryJoinLinks(operatorToken);
        if (!cancelled) setLinks(current);
      } catch {
        if (!cancelled) setError("Invitation status could not be refreshed. No membership changed.");
      }
    };
    void load();
    const timer = window.setInterval(() => void load(), 5_000);
    return () => { cancelled = true; window.clearInterval(timer); };
  }, [operatorToken]);

  async function createLink() {
    await perform(async () => {
      const bundle = await createApiaryJoinLink(operatorToken);
      const capability: ApiaryKeeperJoinCapability = {
        link_id: bundle.link.id,
        keeper_endpoint: bundle.link.keeper_endpoint,
        secret: bundle.one_time_secret,
      };
      const link = createApiaryHandoffLink("keeper", capability, bundle.link.keeper_endpoint);
      setGeneratedLink(link);
      await refresh();
      const copied = await copyLink(link);
      setMessage(copied
        ? "Invitation link copied. Send it privately to the Hive operator; it expires in 24 hours."
        : "Invitation link created. Copy it below and send it privately to the Hive operator.");
    }, "The invitation link could not be created.");
  }

  async function approve(link: ApiaryJoinLink) {
    if (!link.candidate) return;
    await perform(async () => {
      await approveApiaryJoinLink(operatorToken, link.id);
      await Promise.all([refresh(), onInvitationCreated()]);
      setMessage(`${link.candidate?.hive_name} is approved. Her Hive will receive the signed invitation on its next outbound poll.`);
    }, "That Hive could not be approved.");
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

  async function copyLink(link: string): Promise<boolean> {
    try {
      await navigator.clipboard.writeText(link);
      return true;
    } catch {
      return false;
    }
  }

  const pending = links.filter((link) => link.state === "awaiting_approval");
  const active = links.filter((link) => link.state !== "expired" && link.state !== "revoked");

  return (
    <div className="apiary-hive-candidates apiary-link-invitations">
      <div className="apiary-invite-heading">
        <span>
          <strong>Invite a Hive</strong>
          <small>Create one private link, then send it to the other operator. In her personal Hive, she pastes it under Settings → Apiary → Join a Keeper&apos;s Apiary. Her Hive connects outward to this Keeper, so no inbound access to her computer is required.</small>
        </span>
        <button className="primary-action" disabled={busy || working} onClick={() => void createLink()}>
          {working ? "Working…" : "Create invitation link"}
        </button>
      </div>
      <div className="apiary-transport-boundary" role="note">
        <span><strong>Jira work</strong><small>Each Hive polls Jira directly with its own operator identity.</small></span>
        <span><strong>Swarm work</strong><small>Member Hives poll this Keeper for shared tasks, policy, and coordination.</small></span>
      </div>
      {generatedLink ? <ApiaryGeneratedLink link={generatedLink} onCopy={copyLink} /> : null}
      {pending.length > 0 ? (
        <section className="apiary-pending-approvals" aria-label="Hives waiting for approval">
          <h4>Waiting for your approval</h4>
          <ul className="apiary-candidate-list">
            {pending.map((link) => (
              <li key={link.id}>
                <span>
                  <strong>{link.candidate?.hive_name}</strong>
                  <small>{link.candidate?.operator_display_name} · identity verified</small>
                </span>
                <button className="primary-action" disabled={working} onClick={() => void approve(link)}>Approve Hive</button>
              </li>
            ))}
          </ul>
        </section>
      ) : null}
      {active.length > 0 ? (
        <ul className="apiary-link-status" aria-label="Apiary invitation links">
          {active.map((link) => (
            <li key={link.id}>
              <span><strong>{link.candidate?.hive_name ?? "Invitation link"}</strong><small>Expires {new Date(link.expires_at * 1000).toLocaleString()}</small></span>
              <span className={`apiary-link-state state-${link.state}`}>{joinStateLabel(link.state)}</span>
            </li>
          ))}
        </ul>
      ) : <p className="empty-copy">No active invitation links. Create one when another Hive is ready to join.</p>}
      {message ? <p className="form-message" role="status">{message}</p> : null}
      {error ? <p className="form-error" role="alert">{error}</p> : null}
    </div>
  );
}

function joinStateLabel(state: ApiaryJoinLink["state"]): string {
  switch (state) {
    case "open": return "Waiting for Hive";
    case "awaiting_approval": return "Needs approval";
    case "approved": return "Approved · awaiting poll";
    case "invitation_issued": return "Invitation delivered";
    case "expired": return "Expired";
    case "revoked": return "Revoked";
  }
}
