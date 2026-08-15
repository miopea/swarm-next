import { useEffect, useState } from "react";

import {
  acceptFederationJoinPolicy,
  fetchFederationJoinInvitations,
  fetchFederationTransportReadiness,
  fetchHiveConnectionCard,
  importFederationJoinInvitation,
  prepareFederationJoin,
  type ApiaryInvitationBundle,
  type FederationJoinInvitationOverview,
  type FederationTransportReadiness,
} from "../api";
import { createApiaryHandoffLink, readApiaryHandoffLink } from "./apiaryHandoff";
import {
  ApiaryExchangeStep,
  ApiaryFileFallback,
  ApiaryGeneratedLink,
  ApiaryLinkEntry,
  FederationTransportStatus,
} from "./ApiaryHandoffControls";

type Props = {
  busy: boolean;
  operatorToken: string;
  onError: (message: string) => void;
  onMessage: (message: string) => void;
};

export default function PersonalHiveJoin({ busy, operatorToken, onError, onMessage }: Props) {
  const [transportReadiness, setTransportReadiness] = useState<FederationTransportReadiness>();
  const [joinInvitations, setJoinInvitations] = useState<FederationJoinInvitationOverview[]>([]);
  const [invitationPreview, setInvitationPreview] = useState<ApiaryInvitationBundle>();
  const [invitationLink, setInvitationLink] = useState("");
  const [generatedLink, setGeneratedLink] = useState<string>();
  const [working, setWorking] = useState(false);

  useEffect(() => {
    let cancelled = false;
    void Promise.allSettled([
      fetchFederationTransportReadiness(operatorToken),
      fetchFederationJoinInvitations(operatorToken),
    ]).then(([transport, invitations]) => {
      if (cancelled) return;
      setTransportReadiness(transport.status === "fulfilled" ? transport.value : undefined);
      setJoinInvitations(invitations.status === "fulfilled" ? invitations.value : []);
    });
    return () => { cancelled = true; };
  }, [operatorToken]);

  function clearFeedback() {
    onError("");
    onMessage("");
  }

  async function copyLink(link: string): Promise<boolean> {
    try {
      await navigator.clipboard.writeText(link);
      return true;
    } catch {
      return false;
    }
  }

  async function createConnectionLink() {
    setWorking(true);
    clearFeedback();
    try {
      const card = await fetchHiveConnectionCard(operatorToken);
      const link = createApiaryHandoffLink("connection", card);
      setGeneratedLink(link);
      const copied = await copyLink(link);
      onMessage(copied
        ? "Connection link copied. It expires in 24 hours and grants no access by itself."
        : "Connection link created. Copy it below; it expires in 24 hours and grants no access by itself.");
    } catch (cause) {
      onError(cause instanceof Error ? cause.message : "The connection link could not be created.");
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

  async function prepareJoinRequest(invitation: FederationJoinInvitationOverview) {
    setWorking(true);
    clearFeedback();
    try {
      await prepareFederationJoin(operatorToken, invitation.invitation_id);
      setJoinInvitations(await fetchFederationJoinInvitations(operatorToken));
      onMessage(`The signed join request for ${invitation.apiary_name} is prepared locally. Nothing has been sent to the Keeper yet.`);
    } catch (cause) {
      onError(cause instanceof Error ? cause.message : "The join request could not be prepared.");
    } finally {
      setWorking(false);
    }
  }

  return (
    <div className="personal-hive-join">
      <FederationTransportStatus readiness={transportReadiness} />
      <div className="apiary-exchange-intro">
        <span><strong>Join another Keeper&apos;s Apiary</strong><small>Three deliberate handoffs keep each operator in control. No membership or shared access changes until both Hives verify the exact identities.</small></span>
        <ol className="apiary-exchange-guide" aria-label="How to join an Apiary">
          <ApiaryExchangeStep number="1" title="Copy this Hive's connection link" detail="Send the short-lived link privately to the Keeper. Its signed identity contains no repositories, tasks, terminals, Jira access, credentials, or invitation secret.">
            <button className="secondary-button" disabled={busy || working} onClick={() => void createConnectionLink()}>Copy connection link</button>
          </ApiaryExchangeStep>
          <ApiaryExchangeStep number="2" title="Keeper verifies and invites" detail="The Keeper pastes the link, checks your Hive and operator names, then returns one invitation link bound only to this Hive." />
          <ApiaryExchangeStep number="3" title="Review before joining" detail="Paste the returned link below. Swarm verifies the Keeper, policy, Jira projects, and local readiness before it prepares any join request." />
        </ol>
      </div>
      {generatedLink ? <ApiaryGeneratedLink link={generatedLink} onCopy={copyLink} /> : null}
      <div className="apiary-join-card">
        <div>
          <strong>Paste the Keeper&apos;s invitation link</strong>
          <small>Reviewing the link does not send anything or join the Apiary. Its private payload stays after the # fragment and is not sent during web navigation.</small>
        </div>
        <ApiaryLinkEntry label="Invitation link" value={invitationLink} action="Review invitation" disabled={busy || working} onChange={setInvitationLink} onAction={previewInvitationLink} />
        <ApiaryFileFallback summary="Use an invitation file instead" ariaLabel="Choose Apiary invitation" disabled={busy || working} label="Choose invitation file" detail="or drop the Keeper's .json invitation here" onFile={(file) => void previewInvitationFile(file)} />
        {invitationPreview ? <InvitationPreview bundle={invitationPreview} working={working} onCancel={() => setInvitationPreview(undefined)} onTrust={() => void trustKeeperAndImport()} /> : null}
        {joinInvitations.length > 0 ? (
          <ul className="apiary-join-list" aria-label="Saved Apiary invitations">
            {joinInvitations.map((invitation) => (
              <InvitationReadiness key={invitation.invitation_id} invitation={invitation} working={working} onAccept={() => void acceptPolicy(invitation)} onPrepare={() => void prepareJoinRequest(invitation)} />
            ))}
          </ul>
        ) : <p className="empty-copy">No Apiary invitation is saved on this Hive.</p>}
      </div>
    </div>
  );
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

function InvitationReadiness({ invitation, working, onAccept, onPrepare }: { invitation: FederationJoinInvitationOverview; working: boolean; onAccept: () => void; onPrepare: () => void }) {
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
          : invitation.state === "submitted" ? <span className="readiness-ready">Join request prepared</span>
          : invitation.state === "keeper_pinned" ? <button className="secondary-button" disabled={working} onClick={onAccept}>Acknowledge revision {invitation.required_policy_revision}</button>
          : ready ? <button className="primary-action" disabled={working} onClick={onPrepare}>{working ? "Preparing…" : "Prepare join request"}</button>
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
      <small>{invitation.state === "submitted" ? "The signed request is durable and retry-stable. Delivery to the Keeper is not enabled yet." : "Nothing is sent to the Keeper and no membership is granted at this step."}</small>
    </li>
  );
}

function validateInvitation(bundle: ApiaryInvitationBundle) {
  if (!bundle?.keeper_connection_card?.payload || !bundle?.invitation?.payload || !bundle.one_time_secret) {
    throw new Error("That link is not a Swarm Apiary invitation.");
  }
}
