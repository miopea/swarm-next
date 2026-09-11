import { useCallback, useEffect, useRef, useState } from "react";
import JoinPublicProfile, { type JoinPublicProfileHandle } from "./JoinPublicProfile";

import {
  fetchApiaryEnrollments, submitApiaryEnrollment, type ApiaryEnrollment,
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
import {
  clearStagedApiaryHandoff, peekStagedApiaryHandoff, readApiaryHandoffLink,
} from "./apiaryHandoff";
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
  const profileRef = useRef<JoinPublicProfileHandle>(null);
  const [keeperLinks, setKeeperLinks] = useState<ApiaryKeeperLink[]>([]);
  const [joinInvitations, setJoinInvitations] = useState<FederationJoinInvitationOverview[]>([]);
  const [invitationPreview, setInvitationPreview] = useState<ApiaryInvitationBundle>();
  const [keeperLink, setKeeperLink] = useState(() => peekStagedApiaryHandoff("keeper") ?? "");
  const [invitationLink, setInvitationLink] = useState("");
  const [working, setWorking] = useState(false);
  const [joinedApiary, setJoinedApiary] = useState<string>();
  const [confirmingDismissal, setConfirmingDismissal] = useState<string>();
  const [savedStateUnavailable, setSavedStateUnavailable] = useState(false);
  const [keeperPollingUnavailable, setKeeperPollingUnavailable] = useState(false);
  const [enrollments, setEnrollments] = useState<ApiaryEnrollment[]>([]);
  const joinedNotified = useRef(false);
  const enrollmentEpoch = useRef(0);
  let proposed: ApiaryKeeperJoinCapability | undefined;
  try { proposed = readApiaryHandoffLink<ApiaryKeeperJoinCapability>(keeperLink, "keeper"); } catch { /* Incomplete pasted link. */ }

  useEffect(() => {
    let cancelled = false;
    let running = false;
    const refresh = async () => {
      if (running) return;
      running = true;
      const epoch = enrollmentEpoch.current;
      try {
        const records = await fetchApiaryEnrollments(operatorToken);
        if (cancelled || epoch !== enrollmentEpoch.current) return;
        setEnrollments(records);
        if (records.some((record) => record.phase === "complete") && !joinedNotified.current) {
          joinedNotified.current = true;
          onMessage("Welcome to the Apiary. Your Hive has joined; Jira setup is optional.");
          await onJoined();
        }
      } catch { /* Older runtimes retain their explicit invitation flow. */ }
      finally { running = false; }
    };
    void refresh();
    const timer = window.setInterval(() => void refresh(), 5000);
    return () => { cancelled = true; window.clearInterval(timer); };
  }, [operatorToken, onJoined, onMessage]);

  const refreshSavedState = useCallback(async () => {
    const [links, invitations] = await Promise.allSettled([
      fetchApiaryKeeperLinks(operatorToken),
      fetchFederationJoinInvitations(operatorToken),
    ]);
    if (links.status === "fulfilled") setKeeperLinks(links.value);
    if (invitations.status === "fulfilled") setJoinInvitations(invitations.value);
    setSavedStateUnavailable(links.status === "rejected" || invitations.status === "rejected");
  }, [operatorToken]);

  useEffect(() => {
    let cancelled = false;
    void Promise.allSettled([
      fetchApiaryKeeperLinks(operatorToken),
      fetchFederationJoinInvitations(operatorToken),
    ]).then(([links, invitations]) => {
      if (cancelled) return;
      if (links.status === "fulfilled") setKeeperLinks(links.value);
      if (invitations.status === "fulfilled") setJoinInvitations(invitations.value);
      setSavedStateUnavailable(links.status === "rejected" || invitations.status === "rejected");
    });
    return () => { cancelled = true; };
  }, [operatorToken]);

  useEffect(() => {
    if (keeperLinks.length === 0) return;
    let cancelled = false;
    const poll = async () => {
      let refreshed = false;
      let unavailable = false;
      for (const link of keeperLinks.filter((candidate) => !isResolvedKeeperLink(candidate.state)
        && !enrollments.some((record) => record.consent.link_id === candidate.link_id))) {
        try {
          const result = await pollApiaryKeeperLink(operatorToken, link.link_id);
          if (cancelled) return;
          refreshed = true;
          if (result.invitation_received) {
            setJoinInvitations(await fetchFederationJoinInvitations(operatorToken));
            onMessage(`Invitation from ${result.link.apiary_name} received. Review its policy below; Jira setup is optional.`);
          }
        } catch {
          unavailable = true;
        }
      }
      if (refreshed && !cancelled) {
        try {
          setKeeperLinks(await fetchApiaryKeeperLinks(operatorToken));
        } catch {
          unavailable = true;
        }
      }
      if (!cancelled) setKeeperPollingUnavailable(unavailable);
    };
    const timer = window.setInterval(() => void poll(), 5_000);
    return () => { cancelled = true; window.clearInterval(timer); };
  }, [keeperLinks, enrollments, onMessage, operatorToken]);

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
      if (!profileRef.current) throw new Error("Your profile is not ready yet.");
      await profileRef.current.save();
      if (capability.enrollment_offer) {
        const record = await submitApiaryEnrollment(operatorToken, capability.enrollment_offer, capability.secret);
        enrollmentEpoch.current += 1;
        setEnrollments([record]);
        clearStagedApiaryHandoff("keeper");
        setKeeperLink("");
        onMessage("Request submitted. Keeper approval will finish joining automatically, even if you close this page.");
        return;
      }
      const result = await saveApiaryKeeperLink(operatorToken, capability);
      clearStagedApiaryHandoff("keeper");
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

  async function acceptPolicy(invitation: FederationJoinInvitationOverview, joinAfterAcceptance = false) {
    setWorking(true);
    clearFeedback();
    try {
      const accepted = await acceptFederationJoinPolicy(operatorToken, invitation.invitation_id, invitation.required_policy_revision);
      if (joinAfterAcceptance) {
        setJoinInvitations((current) => current.map((item) => item.invitation_id === invitation.invitation_id ? accepted : item));
        if (accepted.invitation_id === invitation.invitation_id
          && accepted.required_policy_revision === invitation.required_policy_revision
          && accepted.state === "policy_accepted"
          && !accepted.readiness_compatibility_fallback
          && accepted.readiness.blockers.length === 0) {
          await joinApiary(accepted);
        } else {
          onMessage("Policy accepted. Readiness changed; review the remaining setup steps before joining.");
        }
        return;
      }
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
      if (!profileRef.current) throw new Error("Your profile is not ready yet.");
      await profileRef.current.save();
      await joinFederationApiary(operatorToken, invitation.invitation_id);
      setJoinedApiary(invitation.apiary_name);
      onMessage(`This Hive joined ${invitation.apiary_name}. Open Apiary for shared work and any remaining setup.`);
      try {
        await onJoined();
      } catch {
        onError("You joined successfully, but this view could not refresh. Refresh the page; do not join again.");
      }
    } catch (cause) {
      onError(cause instanceof Error ? cause.message : "This Hive could not join the Apiary.");
    } finally {
      setWorking(false);
    }
  }

  if (joinedApiary) {
    return <section className="personal-hive-join" aria-label="Joined Apiary"><strong>Joined {joinedApiary}</strong><p>Your membership is saved. Refresh the page if the Apiary view has not opened. You do not need to join again.</p></section>;
  }

  if (enrollments.length > 0) {
    const record = enrollments[0];
    return <section className="personal-hive-join" aria-label="Apiary joining progress">
      <h3>{record.phase === "complete" ? "Welcome to your Apiary" : record.phase === "joining" ? "Joining your Apiary…" : record.phase === "attention" ? "Joining needs attention" : "Waiting for Keeper approval"}</h3>
      <p>{record.phase === "attention" ? "The saved request could not finish. Review the invitation with your Keeper; your local work is unchanged." : "You have submitted your request. There is nothing else to approve here; Swarm finishes the connection in the background."}</p>
      <p>Your local tasks, workers, repositories and credentials stay on this Hive. Jira is optional.</p>
      {record.problem ? <p role="status">{record.problem === "keeper_unavailable"
        ? "Keeper is temporarily unreachable. Your request is saved and Swarm will retry automatically."
        : record.problem === "invitation_unavailable"
          ? "This invitation expired or was cancelled. Ask your Keeper for a current invitation."
          : record.problem === "runtime_incompatible"
            ? "The Hives could not agree on the joining protocol. Check that both are updated; your local work is safe."
            : "The approved invitation no longer matches your submitted terms. Review the current invitation with your Keeper; Swarm has not accepted new permissions."}
        {record.next_attempt_at ? <> Next check: {new Date(record.next_attempt_at * 1000).toLocaleTimeString()}.</> : null}
      </p> : null}
      {record.phase === "awaiting_approval" || record.phase === "attention" ? <button className="secondary-button" disabled={working} onClick={() => {
        setWorking(true);
        void removeApiaryKeeperLink(operatorToken, record.consent.link_id).then(() => { enrollmentEpoch.current += 1; setEnrollments([]); })
          .catch(() => onError("The request may already be joining. Refresh its status before trying again."))
          .finally(() => setWorking(false));
      }}>Cancel request</button> : null}
    </section>;
  }

  return (
    <div className="personal-hive-join">
      <JoinPublicProfile ref={profileRef} operatorToken={operatorToken} disabled={busy || working} />
      {joinInvitations.length === 0 ? <div className="apiary-exchange-intro">
        <span><strong>Join a Keeper&apos;s Apiary</strong><small>The private link is handed to this personal Hive. Opening it now guides you here without joining through the Keeper&apos;s browser.</small></span>
        <ol className="apiary-exchange-guide" aria-label="How this Hive joins an Apiary">
          <ApiaryExchangeStep number="1" title="Hand the link to this Hive" detail="Open the private link and choose this personal Hive, or paste the complete link below." />
          <ApiaryExchangeStep number="2" title="Review and submit" detail="See what joining means and send your request once." />
          <ApiaryExchangeStep number="3" title="Keeper approves · you're in" detail="Swarm finishes joining automatically. Jira is optional." />
        </ol>
        {proposed?.enrollment_offer ? <div className="apiary-policy-acknowledgement">
          <div><strong>Join {proposed.enrollment_offer.payload.apiary_name}</strong>
            <p>Keeper: {proposed.enrollment_offer.payload.keeper.payload.operator_display_name} · {proposed.enrollment_offer.payload.keeper.payload.hive_name}</p>
            <p>Submitting accepts policy revision {proposed.enrollment_offer.payload.policy_revision}: Keeper manages shared work and Apiary-wide settings. Your local task system, workers, repositories and credentials remain yours. Joining does not grant unrestricted terminal or machine access.</p>
            <small>No Jira connection is required. Keeper approval completes your membership automatically.</small>
          </div>
        </div> : null}
        {proposed && !proposed.enrollment_offer ? <p role="note">This older invitation needs a policy review after Keeper approval. Ask for a newly generated link to use the simpler submit-once flow.</p> : null}
        <ApiaryLinkEntry label="Keeper invitation link" value={keeperLink} action={working ? "Submitting…" : proposed?.enrollment_offer ? "Request to join" : "Connect to Keeper"} disabled={busy || working} onChange={setKeeperLink} onAction={() => void connectToKeeper()} />
        <div className="apiary-transport-boundary" role="note">
          <span><strong>Optional Jira work</strong><small>If connected, this Hive reads Jira directly as you.</small></span>
          <span><strong>Swarm work</strong><small>This Hive polls the Keeper for shared Apiary tasks and coordination.</small></span>
        </div>
      </div> : null}
      {savedStateUnavailable ? <div className="form-error apiary-refresh-error" role="alert"><span>Saved Keeper invitations could not be fully refreshed. Last-known links remain unchanged.</span><button className="secondary-button" type="button" disabled={working} onClick={() => void refreshSavedState()}>Retry saved invitations</button></div> : null}
      {keeperPollingUnavailable ? <div className="form-error apiary-refresh-error" role="status"><span>The Keeper was not reachable on the last check. This Hive keeps the invitation safely and retries every five seconds.</span></div> : null}
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
      {!proposed?.enrollment_offer ? <div className="apiary-join-card">
        <div>
          <strong>Review before joining</strong>
          <small>After Keeper approval, the invitation appears here automatically. Review and accept the shared policy to join; configure optional integrations afterward.</small>
        </div>
        <details className="apiary-manual-fallback">
          <summary>Advanced: import a legacy invitation</summary>
          <ApiaryLinkEntry label="Legacy invitation link" value={invitationLink} action="Review invitation" disabled={busy || working} onChange={setInvitationLink} onAction={previewInvitationLink} />
          <ApiaryFileFallback summary="Use an invitation file" ariaLabel="Choose Apiary invitation" disabled={busy || working} label="Choose invitation file" detail="or drop the Keeper's .json invitation here" onFile={(file) => void previewInvitationFile(file)} />
        </details>
        {invitationPreview ? <InvitationPreview bundle={invitationPreview} working={working} onCancel={() => setInvitationPreview(undefined)} onTrust={() => void trustKeeperAndImport()} /> : null}
        {joinInvitations.length > 0 ? <div className="settings-actions">
          <a className="secondary-button" href="#settings-integrations">Open Jira settings</a>
          <button className="secondary-button" type="button" disabled={busy || working} onClick={() => void refreshSavedState()}>Refresh setup status</button>
        </div> : null}
        {joinInvitations.length > 0 ? (
          <ul className="apiary-join-list" aria-label="Saved Apiary invitations">
            {joinInvitations.map((invitation) => (
              <InvitationReadiness key={invitation.invitation_id} invitation={invitation} working={busy || working} onAccept={(joinAfterAcceptance) => void acceptPolicy(invitation, joinAfterAcceptance)} onJoin={() => void joinApiary(invitation)} />
            ))}
          </ul>
        ) : <p className="empty-copy">No Apiary invitation is saved on this Hive.</p>}
      </div> : null}
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

function InvitationReadiness({ invitation, working, onAccept, onJoin }: { invitation: FederationJoinInvitationOverview; working: boolean; onAccept: (joinAfterAcceptance: boolean) => void; onJoin: () => void }) {
  const ready = invitation.readiness.blockers.length === 0;
  const readyToAcceptAndJoin = !invitation.readiness_compatibility_fallback
    && invitation.readiness.blockers.every((blocker) => blocker === "policy_not_accepted");
  return (
    <li>
      <div className="apiary-join-summary">
        <span><strong>{invitation.apiary_name}</strong><small>{invitation.keeper_hive_name} · {invitation.keeper_operator_display_name}</small></span>
        <span className={ready || readyToAcceptAndJoin ? "readiness-ready" : "readiness-blocked"}>{invitation.readiness_compatibility_fallback ? "Runtime update in progress" : ready ? "Ready to join" : readyToAcceptAndJoin ? "Ready for your approval" : `${invitation.readiness.blockers.length} readiness ${invitation.readiness.blockers.length === 1 ? "step" : "steps"} left`}</span>
      </div>
      <div className="apiary-policy-acknowledgement">
        <span><strong>Policy revision {invitation.required_policy_revision}</strong><small>Swarm shared work · {invitation.promoted_projects.length} optional Jira {invitation.promoted_projects.length === 1 ? "project" : "projects"} · Keeper identity pinned</small></span>
        {invitation.readiness_compatibility_fallback ? <button className="secondary-button" disabled>Waiting for runtime</button>
          : invitation.state === "submitted" ? <button className="primary-action" disabled={working} onClick={onJoin}>{working ? "Joining…" : "Retry joining"}</button>
          : invitation.state === "keeper_pinned" ? <button className={readyToAcceptAndJoin ? "primary-action" : "secondary-button"} disabled={working} onClick={() => onAccept(readyToAcceptAndJoin)}>{working ? "Accepting…" : readyToAcceptAndJoin ? "Accept policy and join" : `Acknowledge revision ${invitation.required_policy_revision}`}</button>
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
      {invitation.readiness.blockers.some((blocker) => blocker === "integration_not_ready" || blocker === "project_access_not_ready") ? <p className="readiness-blocked">This runtime still requires Jira setup before joining. Update it to join without Jira; your invitation remains saved.</p>
        : invitation.readiness.jira_connection !== "ready" ? <p>Jira is optional. You can connect it later from your Apiary setup checklist.</p> : null}
      <small>{invitation.state === "submitted" ? "The signed request is durable and retry-stable. Retry after a temporary Keeper outage." : "Joining sends one signed request to the Keeper; Jira credentials and private Hive data stay local."}</small>
    </li>
  );
}

function validateInvitation(bundle: ApiaryInvitationBundle) {
  if (!bundle?.keeper_connection_card?.payload || !bundle?.invitation?.payload || !bundle.one_time_secret) {
    throw new Error("That link is not a Swarm Apiary invitation.");
  }
}
