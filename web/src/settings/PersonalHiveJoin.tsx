import { useEffect, useState } from "react";

import {
  acceptFederationJoinPolicy,
  fetchApiaryKeeperLinks,
  fetchFederationJoinInvitations,
  importFederationJoinInvitation,
  pollApiaryKeeperLink,
  removeApiaryKeeperLink,
  joinFederationApiary,
  saveApiaryKeeperLink,
  type ApiaryInvitationBundle,
  type ApiaryKeeperJoinCapability,
  type ApiaryKeeperLink,
  type FederationJoinInvitationOverview,
} from "../api";
import { readApiaryHandoffLink } from "./apiaryHandoff";
import {
  ApiaryExchangeStep,
  ApiaryFileFallback,
  ApiaryLinkEntry,
} from "./ApiaryHandoffControls";

type Props = {
  busy: boolean;
  operatorToken: string;
  onError: (message: string) => void;
  onMessage: (message: string) => void;
  onJoined: () => Promise<void>;
};

export default function PersonalHiveJoin({ busy, operatorToken, onError, onMessage, onJoined }: Props) {
  const [keeperLinks, setKeeperLinks] = useState<ApiaryKeeperLink[]>([]);
  const [joinInvitations, setJoinInvitations] = useState<FederationJoinInvitationOverview[]>([]);
  const [invitationPreview, setInvitationPreview] = useState<ApiaryInvitationBundle>();
  const [keeperLink, setKeeperLink] = useState("");
  const [invitationLink, setInvitationLink] = useState("");
  const [working, setWorking] = useState(false);
  const [confirmingDismissal, setConfirmingDismissal] = useState<string>();

  useEffect(() => {
    let cancelled = false;
    void Promise.allSettled([
      fetchApiaryKeeperLinks(operatorToken),
      fetchFederationJoinInvitations(operatorToken),
    ]).then(([links, invitations]) => {
      if (cancelled) return;
      setKeeperLinks(links.status === "fulfilled" ? links.value : []);
      setJoinInvitations(invitations.status === "fulfilled" ? invitations.value : []);
    });
    return () => { cancelled = true; };
  }, [operatorToken]);

  useEffect(() => {
    if (keeperLinks.length === 0) return;
    let cancelled = false;
    const poll = async () => {
      let refreshed = false;
      for (const link of keeperLinks.filter((candidate) => !isResolvedKeeperLink(candidate.state))) {
        try {
          const result = await pollApiaryKeeperLink(operatorToken, link.link_id);
          if (cancelled) return;
          refreshed = true;
          if (result.invitation_received) {
            setJoinInvitations(await fetchFederationJoinInvitations(operatorToken));
            onMessage(`Invitation from ${result.link.apiary_name} received. Review its policy and Jira readiness below.`);
          }
        } catch {
          // Pending links remain durable. Temporary Keeper outages are expected.
        }
      }
      if (refreshed && !cancelled) setKeeperLinks(await fetchApiaryKeeperLinks(operatorToken));
    };
    const timer = window.setInterval(() => void poll(), 5_000);
    return () => { cancelled = true; window.clearInterval(timer); };
  }, [keeperLinks, onMessage, operatorToken]);

  function clearFeedback() {
    onError("");
    onMessage("");
  }

  async function connectToKeeper() {
    setWorking(true);
    clearFeedback();
    try {
      const capability = readApiaryHandoffLink<ApiaryKeeperJoinCapability>(keeperLink, "keeper");
      if (!capability.link_id || !capability.keeper_endpoint || !capability.secret) {
        throw new Error("That link is not a Keeper invitation.");
      }
      const result = await saveApiaryKeeperLink(operatorToken, capability);
      setKeeperLink("");
      setKeeperLinks(await fetchApiaryKeeperLinks(operatorToken));
      if (result.invitation_received) {
        setJoinInvitations(await fetchFederationJoinInvitations(operatorToken));
        onMessage(`Invitation from ${result.link.apiary_name} received. Review it below before joining.`);
      } else {
        onMessage(`This Hive introduced itself to ${result.link.apiary_name}. Waiting for the Keeper to approve the exact identity.`);
      }
    } catch (cause) {
      onError(cause instanceof Error ? cause.message : "The Keeper invitation link could not be used.");
    } finally {
      setWorking(false);
    }
  }

  async function dismissKeeperLink(link: ApiaryKeeperLink) {
    setWorking(true);
    clearFeedback();
    try {
      await removeApiaryKeeperLink(operatorToken, link.link_id);
      setConfirmingDismissal(undefined);
      setKeeperLinks(await fetchApiaryKeeperLinks(operatorToken));
      onMessage(isResolvedKeeperLink(link.state)
        ? "The cancelled or expired invitation was removed from this Hive."
        : "This Hive stopped waiting for that Keeper invitation. The private link must be pasted again to reconnect.");
    } catch (cause) {
      onError(cause instanceof Error ? cause.message : "That saved Keeper invitation could not be removed.");
    } finally {
      setWorking(false);
    }
  }

  async function previewInvitationFile(file: File | undefined) {
    if (!file) return;
    clearFeedback();
    try {
      if (file.size > 512 * 1024) throw new Error("That invitation file is unexpectedly large.");
      const bundle = JSON.parse(await file.text()) as ApiaryInvitationBundle;
      validateInvitation(bundle);
      setInvitationPreview(bundle);
    } catch (cause) {
      setInvitationPreview(undefined);
      onError(cause instanceof Error ? cause.message : "That Apiary invitation could not be read.");
    }
  }

  function previewInvitationLink() {
    clearFeedback();
    try {
      const bundle = readApiaryHandoffLink<ApiaryInvitationBundle>(invitationLink, "invitation");
      validateInvitation(bundle);
      setInvitationPreview(bundle);
      setInvitationLink("");
    } catch (cause) {
      setInvitationPreview(undefined);
      onError(cause instanceof Error ? cause.message : "That Apiary invitation link could not be read.");
    }
  }

  async function trustKeeperAndImport() {
    if (!invitationPreview) return;
    setWorking(true);
    clearFeedback();
    try {
      const imported = await importFederationJoinInvitation(operatorToken, invitationPreview);
      setJoinInvitations(await fetchFederationJoinInvitations(operatorToken));
      setInvitationPreview(undefined);
      onMessage(`${imported.keeper_hive_name} is pinned as Keeper for ${imported.apiary_name}. You have not joined or accepted its policy yet.`);
    } catch (cause) {
      onError(cause instanceof Error ? cause.message : "That Apiary invitation could not be verified.");
    } finally {
      setWorking(false);
    }
  }

  async function acceptPolicy(invitation: FederationJoinInvitationOverview) {
    setWorking(true);
    clearFeedback();
    try {
      await acceptFederationJoinPolicy(operatorToken, invitation.invitation_id, invitation.required_policy_revision);
      setJoinInvitations(await fetchFederationJoinInvitations(operatorToken));
      onMessage(`Policy revision ${invitation.required_policy_revision} accepted locally. This Hive has not joined ${invitation.apiary_name} yet.`);
    } catch (cause) {
      onError(cause instanceof Error ? cause.message : "That policy revision could not be accepted.");
    } finally {
      setWorking(false);
    }
  }

  async function joinApiary(invitation: FederationJoinInvitationOverview) {
    setWorking(true);
    clearFeedback();
    try {
      await joinFederationApiary(operatorToken, invitation.invitation_id);
      await onJoined();
      onMessage(`This Hive joined ${invitation.apiary_name}. Jira continues syncing directly; Swarm coordination now polls the Keeper.`);
    } catch (cause) {
      onError(cause instanceof Error ? cause.message : "This Hive could not join the Apiary.");
    } finally {
      setWorking(false);
    }
  }

  return (
    <div className="personal-hive-join">
      <div className="apiary-exchange-intro">
        <span><strong>Join a Keeper&apos;s Apiary</strong><small>The private link is handed to this personal Hive; opening it does not join through the Keeper&apos;s browser.</small></span>
        <ol className="apiary-exchange-guide" aria-label="How this Hive joins an Apiary">
          <ApiaryExchangeStep number="1" title="Paste her private link" detail="Copy the complete link the Keeper sent and paste it below in this Hive." />
          <ApiaryExchangeStep number="2" title="Wait for her approval" detail="This Hive introduces only its signed identity and keeps polling outward." />
          <ApiaryExchangeStep number="3" title="Review and join" detail="After approval, check policy and Jira readiness before joining explicitly." />
        </ol>
        <ApiaryLinkEntry label="Keeper invitation link" value={keeperLink} action={working ? "Connecting…" : "Connect to Keeper"} disabled={busy || working} onChange={setKeeperLink} onAction={() => void connectToKeeper()} />
        <div className="apiary-transport-boundary" role="note">
          <span><strong>Jira work</strong><small>This Hive continues polling Jira directly as you.</small></span>
          <span><strong>Swarm work</strong><small>This Hive polls the Keeper for shared Apiary tasks and coordination.</small></span>
        </div>
      </div>
      {keeperLinks.length > 0 ? (
        <ul className="apiary-link-status" aria-label="Pending Keeper invitations">
          {keeperLinks.map((link) => <li key={link.link_id}>
            <span><strong>{link.apiary_name ?? "Keeper invitation"}</strong><small>{link.keeper_endpoint}</small></span>
            <span className="apiary-link-actions">
              <span className={`apiary-link-state state-${link.state}`}>{keeperLinkStateLabel(link.state)}</span>
              {confirmingDismissal === link.link_id
                ? <span className="apiary-cancel-confirm" role="group" aria-label="Confirm saved invitation removal"><button className="danger-button" disabled={working} onClick={() => void dismissKeeperLink(link)}>Remove link</button><button className="secondary-button" disabled={working} onClick={() => setConfirmingDismissal(undefined)}>Keep waiting</button></span>
                : <button className="danger-link" disabled={working} onClick={() => setConfirmingDismissal(link.link_id)}>{isResolvedKeeperLink(link.state) ? "Dismiss" : "Stop waiting"}</button>}
            </span>
          </li>)}
        </ul>
      ) : null}
      <div className="apiary-join-card">
        <div>
          <strong>Review before joining</strong>
          <small>After Keeper approval, her signed invitation appears here automatically. Policy acceptance, Jira readiness, and final membership remain explicit.</small>
        </div>
        <details className="apiary-manual-fallback">
          <summary>Advanced: import a legacy invitation</summary>
          <ApiaryLinkEntry label="Legacy invitation link" value={invitationLink} action="Review invitation" disabled={busy || working} onChange={setInvitationLink} onAction={previewInvitationLink} />
          <ApiaryFileFallback summary="Use an invitation file" ariaLabel="Choose Apiary invitation" disabled={busy || working} label="Choose invitation file" detail="or drop the Keeper's .json invitation here" onFile={(file) => void previewInvitationFile(file)} />
        </details>
        {invitationPreview ? <InvitationPreview bundle={invitationPreview} working={working} onCancel={() => setInvitationPreview(undefined)} onTrust={() => void trustKeeperAndImport()} /> : null}
        {joinInvitations.length > 0 ? (
          <ul className="apiary-join-list" aria-label="Saved Apiary invitations">
            {joinInvitations.map((invitation) => (
              <InvitationReadiness key={invitation.invitation_id} invitation={invitation} working={working} onAccept={() => void acceptPolicy(invitation)} onJoin={() => void joinApiary(invitation)} />
            ))}
          </ul>
        ) : <p className="empty-copy">No Apiary invitation is saved on this Hive.</p>}
      </div>
    </div>
  );
}

function isResolvedKeeperLink(state: ApiaryKeeperLink["state"]): boolean {
  return state === "revoked" || state === "expired" || state === "invitation_issued";
}

function keeperLinkStateLabel(state: ApiaryKeeperLink["state"]): string {
  switch (state) {
    case "open": return "Introducing this Hive";
    case "awaiting_approval": return "Waiting for Keeper approval";
    case "approved": return "Approved · retrieving invitation";
    case "invitation_issued": return "Invitation received";
    case "revoked": return "Cancelled by Keeper";
    case "expired": return "Invitation expired";
  }
}

function InvitationPreview({ bundle, working, onCancel, onTrust }: { bundle: ApiaryInvitationBundle; working: boolean; onCancel: () => void; onTrust: () => void }) {
  return (
    <div className="apiary-invitation-preview" role="group" aria-label="Review Apiary invitation">
      <div><span>Apiary</span><strong>{bundle.invitation.payload.apiary_name}</strong></div>
      <div><span>Keeper Hive</span><strong>{bundle.keeper_connection_card.payload.hive_name}</strong></div>
      <div><span>Keeper operator</span><strong>{bundle.keeper_connection_card.payload.operator_display_name}</strong></div>
      <div><span>Shared work</span><strong>{bundle.invitation.payload.shared_work_backend === "jira" ? "Jira-backed" : "Native Swarm"}</strong></div>
      <div><span>Policy revision</span><strong>{bundle.invitation.payload.required_policy_revision}</strong></div>
      <div><span>Shared Jira projects</span><strong>{bundle.promoted_projects.length}</strong></div>
      <div><span>Expires</span><strong>{new Date(bundle.invitation.payload.expires_at * 1000).toLocaleString()}</strong></div>
      {bundle.promoted_projects.length > 0 ? (
        <ul className="apiary-project-manifest" aria-label="Promoted Jira projects">
          {bundle.promoted_projects.map((project) => <li key={project.project_id}><strong>{project.project_key}</strong><span>{project.project_name}</span></li>)}
        </ul>
      ) : <p className="empty-copy">This Apiary has no promoted Jira projects yet.</p>}
      <p>Trusting pins this exact Keeper key and saves the one-time invitation privately. It does not join the Apiary, accept policy, share work, or grant terminal access.</p>
      <div className="settings-actions">
        <button className="secondary-button" disabled={working} onClick={onCancel}>Choose another</button>
        <button className="primary-action" disabled={working} onClick={onTrust}>{working ? "Verifying…" : "Trust Keeper and save invitation"}</button>
      </div>
    </div>
  );
}

function InvitationReadiness({ invitation, working, onAccept, onJoin }: { invitation: FederationJoinInvitationOverview; working: boolean; onAccept: () => void; onJoin: () => void }) {
  const ready = invitation.readiness.blockers.length === 0;
  return (
    <li>
      <div className="apiary-join-summary">
        <span><strong>{invitation.apiary_name}</strong><small>{invitation.keeper_hive_name} · {invitation.keeper_operator_display_name}</small></span>
        <span className={ready ? "readiness-ready" : "readiness-blocked"}>{invitation.readiness_compatibility_fallback ? "Runtime update in progress" : ready ? "Ready to contact Keeper" : `${invitation.readiness.blockers.length} readiness ${invitation.readiness.blockers.length === 1 ? "step" : "steps"} left`}</span>
      </div>
      <div className="apiary-policy-acknowledgement">
        <span><strong>Policy revision {invitation.required_policy_revision}</strong><small>Jira-backed shared work · {invitation.promoted_projects.length} signed {invitation.promoted_projects.length === 1 ? "project" : "projects"} · Keeper identity pinned</small></span>
        {invitation.readiness_compatibility_fallback ? <button className="secondary-button" disabled>Waiting for runtime</button>
          : invitation.state === "submitted" ? <button className="primary-action" disabled={working} onClick={onJoin}>{working ? "Joining…" : "Retry joining"}</button>
          : invitation.state === "keeper_pinned" ? <button className="secondary-button" disabled={working} onClick={onAccept}>Acknowledge revision {invitation.required_policy_revision}</button>
          : ready ? <button className="primary-action" disabled={working} onClick={onJoin}>{working ? "Joining…" : "Join Apiary"}</button>
          : <span className="readiness-ready">Acknowledged</span>}
      </div>
      <ul className="apiary-project-readiness" aria-label={`Jira readiness for ${invitation.apiary_name}`}>
        {invitation.readiness.projects.map((project) => {
          const projectReady = Boolean(project.binding_id && project.access_verified && project.workflow_mapped);
          const status = invitation.readiness_compatibility_fallback ? "Readiness refresh pending" : !project.binding_id ? "Connect this Jira project" : !project.access_verified ? "Verify Jira access" : !project.workflow_mapped ? "Finish workflow mapping" : "Connected and mapped";
          return <li key={project.project.project_id}><span><strong>{project.project.project_key}</strong><small>{project.project.project_name}</small></span><span className={projectReady ? "readiness-ready" : "readiness-blocked"}>{status}</span></li>;
        })}
      </ul>
      {invitation.readiness.jira_connection !== "ready" ? <p className="readiness-blocked">Connect Jira on this Hive before it can join.</p> : null}
      <small>{invitation.state === "submitted" ? "The signed request is durable and retry-stable. Retry after a temporary Keeper outage." : "Joining sends one signed request to the Keeper; Jira credentials and private Hive data stay local."}</small>
    </li>
  );
}

function validateInvitation(bundle: ApiaryInvitationBundle) {
  if (!bundle?.keeper_connection_card?.payload || !bundle?.invitation?.payload || !bundle.one_time_secret) {
    throw new Error("That link is not a Swarm Apiary invitation.");
  }
}
