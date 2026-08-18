import { useCallback, useEffect, useMemo, useState } from "react";

import {
  acceptApiaryClaimHandoff,
  cancelApiaryClaimHandoff,
  declineApiaryClaimHandoff,
  fetchApiaryClaimHandoffs,
  fetchApiaryHandoffTargets,
  fetchApiaryMembers,
  fetchApiarySharedWork,
  fetchApiaryTasks,
  fetchLocalApiaryTaskExecutions,
  fetchFederationCatalogReadiness,
  fetchMyFederationStewardship,
  fetchFederationSyncHealth,
  fetchFederationTaskSyncStatus,
  fetchFederationTaskOutbox,
  fetchFederationTaskOutboxStatus,
  fetchFederationStewardTaskOutbox,
  fetchFederationStewardAssists,
  offerApiaryClaimHandoff,
  queueFederationStewardAssist,
  respondFederationStewardAssist,
  type ApiaryMember,
  type ApiarySharedWorkClaim,
  type ApiaryTask,
  type FederationCatalogReadiness,
  type FederationClaimHandoff,
  type FederationHandoffTarget,
  type FederationStewardshipSnapshot,
  type FederationStewardTaskOutboxEntry,
  type FederationStewardAssistLocalState,
  type FederationSyncHealth,
  type FederationTaskSyncStatus,
  type FederationTaskOutboxEntry,
  type FederationTaskOutboxStatus,
  type HiveIdentity,
  type LocalApiaryTaskExecution,
} from "../api";
import BeeMascot from "../brand/BeeMascot";
import { catalogBlockerLabel, federationSyncCopy } from "./presentation";

type Props = {
  identity: HiveIdentity;
  operatorToken: string;
  onManage: () => void;
  onOpenTasks: () => void;
};
type MemberSnapshot = {
  members: ApiaryMember[];
  sharedWork: ApiarySharedWorkClaim[];
  tasks: ApiaryTask[];
  sync?: FederationSyncHealth;
  taskSync?: FederationTaskSyncStatus;
  catalog?: FederationCatalogReadiness;
  outbox: FederationTaskOutboxEntry[];
  outboxStatus?: FederationTaskOutboxStatus;
  stewardship?: FederationStewardshipSnapshot | null;
  stewardTasks: FederationStewardTaskOutboxEntry[];
  stewardAssists: FederationStewardAssistLocalState;
  handoffs: FederationClaimHandoff[];
  handoffTargets: FederationHandoffTarget[];
  executions: LocalApiaryTaskExecution[];
};

const emptySnapshot: MemberSnapshot = { members: [], sharedWork: [], tasks: [], outbox: [], stewardTasks: [], stewardAssists: { incoming: [], outbox: [] }, handoffs: [], handoffTargets: [], executions: [] };

export default function MemberControlRoom({ identity, operatorToken, onManage, onOpenTasks }: Props) {
  const context = identity.apiary_context;
  const [snapshot, setSnapshot] = useState<MemberSnapshot>(emptySnapshot);
  const [state, setState] = useState<"loading" | "ready" | "partial">("loading");
  const refresh = useCallback(async () => {
    setState("loading");
    const [members, sharedWork, tasks, sync, taskSync, catalog, outbox, outboxStatus, stewardship, stewardTasks, stewardAssists, handoffs, handoffTargets, executions] = await Promise.allSettled([
      fetchApiaryMembers(operatorToken),
      fetchApiarySharedWork(operatorToken),
      fetchApiaryTasks(operatorToken),
      fetchFederationSyncHealth(operatorToken),
      fetchFederationTaskSyncStatus(operatorToken),
      fetchFederationCatalogReadiness(operatorToken),
      fetchFederationTaskOutbox(operatorToken),
      fetchFederationTaskOutboxStatus(operatorToken),
      fetchMyFederationStewardship(operatorToken),
      fetchFederationStewardTaskOutbox(operatorToken),
      fetchFederationStewardAssists(operatorToken),
      fetchApiaryClaimHandoffs(operatorToken),
      fetchApiaryHandoffTargets(operatorToken),
      fetchLocalApiaryTaskExecutions(operatorToken),
    ]);
    setSnapshot((current) => ({
      members: members.status === "fulfilled" ? members.value : current.members,
      sharedWork: sharedWork.status === "fulfilled" ? sharedWork.value : current.sharedWork,
      tasks: tasks.status === "fulfilled" ? tasks.value : current.tasks,
      sync: sync.status === "fulfilled" ? sync.value : current.sync,
      taskSync: taskSync.status === "fulfilled" ? taskSync.value : current.taskSync,
      catalog: catalog.status === "fulfilled" ? catalog.value : current.catalog,
      outbox: outbox.status === "fulfilled" ? outbox.value : current.outbox,
      outboxStatus: outboxStatus.status === "fulfilled" ? outboxStatus.value : current.outboxStatus,
      stewardship: stewardship.status === "fulfilled" ? stewardship.value : current.stewardship,
      stewardTasks: stewardTasks.status === "fulfilled" ? stewardTasks.value : current.stewardTasks,
      stewardAssists: stewardAssists.status === "fulfilled" && stewardAssists.value ? stewardAssists.value : current.stewardAssists,
      handoffs: handoffs.status === "fulfilled" && Array.isArray(handoffs.value) ? handoffs.value : current.handoffs,
      handoffTargets: handoffTargets.status === "fulfilled" && Array.isArray(handoffTargets.value) ? handoffTargets.value : current.handoffTargets,
      executions: executions.status === "fulfilled" && Array.isArray(executions.value) ? executions.value : current.executions,
    }));
    setState([members, sharedWork, tasks, sync, taskSync, catalog, outbox, outboxStatus, stewardship, stewardTasks, stewardAssists, handoffs, handoffTargets, executions].some((result) => result.status === "rejected") ? "partial" : "ready");
  }, [operatorToken]);
  useEffect(() => { void refresh(); }, [refresh]);

  const keeper = snapshot.members.find((member) => member.role === "keeper");
  const localClaims = useMemo(
    () => snapshot.sharedWork.filter((claim) => claim.home_hive_id === identity.hive.id),
    [identity.hive.id, snapshot.sharedWork],
  );
  const activeHandoffs = useMemo(() => snapshot.handoffs.filter((handoff) => handoff.state === "offered" || handoff.state === "accepted"), [snapshot.handoffs]);
  const incomingHandoffs = useMemo(() => activeHandoffs.filter((handoff) => handoff.target_hive_id === identity.hive.id), [activeHandoffs, identity.hive.id]);
  const outgoingHandoffs = useMemo(() => activeHandoffs.filter((handoff) => handoff.source_hive_id === identity.hive.id), [activeHandoffs, identity.hive.id]);
  const activeHandoffByClaim = useMemo(() => new Map(activeHandoffs.map((handoff) => [handoff.claim_id, handoff])), [activeHandoffs]);
  const hiveName = useMemo(() => new Map(snapshot.members.map((member) => [member.hive_id, member.hive_name])), [snapshot.members]);
  const readyProjects = snapshot.catalog?.projects.filter((project) => project.binding_id && project.access_verified && project.workflow_mapped).length ?? 0;
  const projectCount = snapshot.catalog?.projects.length ?? 0;
  const syncCondition = snapshot.sync?.condition ?? "idle";
  const [syncTitle, syncDetail] = federationSyncCopy[syncCondition];
  const stewardship = snapshot.stewardship?.stewardship;
  const managedMembers = stewardship?.managed_hive_ids.map((hiveId) => snapshot.members.find((member) => member.hive_id === hiveId) ?? { hive_id: hiveId, hive_name: "Registered Hive" }) ?? [];
  const managedHives = managedMembers.map((member) => member.hive_name);
  const stewardObservations = snapshot.stewardship?.observations ?? [];
  const stewardAssists = {
    incoming: snapshot.stewardAssists?.incoming ?? [],
    sent: snapshot.stewardAssists?.sent ?? [],
    outbox: snapshot.stewardAssists?.outbox ?? [],
  };
  const sentAssists = stewardAssists.sent;
  const canAssist = stewardship?.capabilities.includes("assist") ?? false;
  const [assistTarget, setAssistTarget] = useState("");
  const [assistMessage, setAssistMessage] = useState("");
  const [sendingAssist, setSendingAssist] = useState(false);
  const [actingAssist, setActingAssist] = useState<string>();
  const [assistError, setAssistError] = useState<string>();
  const [actingHandoff, setActingHandoff] = useState<string>();
  const transitionHandoff = useCallback(async (handoff: FederationClaimHandoff, action: "accept" | "decline" | "cancel") => {
    setActingHandoff(handoff.id);
    try {
      if (action === "accept") await acceptApiaryClaimHandoff(operatorToken, handoff.id);
      else if (action === "decline") await declineApiaryClaimHandoff(operatorToken, handoff.id);
      else await cancelApiaryClaimHandoff(operatorToken, handoff.id);
      await refresh();
    } finally { setActingHandoff(undefined); }
  }, [operatorToken, refresh]);
  const requestStewardAssist = useCallback(async () => {
    if (!assistTarget || !assistMessage.trim()) return;
    setSendingAssist(true);
    setAssistError(undefined);
    try {
      await queueFederationStewardAssist(operatorToken, { target_hive_id: assistTarget, message: assistMessage.trim() });
      setAssistMessage("");
      await refresh();
    } catch (error) {
      setAssistError(error instanceof Error ? error.message : "Keeper did not accept this assistance request.");
    } finally {
      setSendingAssist(false);
    }
  }, [assistMessage, assistTarget, operatorToken, refresh]);
  const respondToAssist = useCallback(async (requestId: string, decision: "accepted" | "declined") => {
    setActingAssist(requestId);
    setAssistError(undefined);
    try {
      await respondFederationStewardAssist(operatorToken, requestId, decision);
      await refresh();
    } catch (error) {
      setAssistError(error instanceof Error ? error.message : "Keeper did not accept this response.");
    } finally {
      setActingAssist(undefined);
    }
  }, [operatorToken, refresh]);

  if (context?.mode !== "federated" || context.local_role !== "member") return null;

  return (
    <section className="keeper-control-room member-control-room" aria-labelledby="member-control-heading">
      <header className="keeper-hero">
        <div className="keeper-hero-mark"><BeeMascot role="worker" expression="focused" /></div>
        <div><p className="eyebrow">Member Hive</p><h3 id="member-control-heading">{context.apiary.name}</h3><p>Your Hive reads Jira directly and polls Keeper for Swarm-generated Apiary work.</p></div>
        <span className="apiary-backend-badge">{context.apiary.shared_work_backend === "jira" ? "Jira-backed" : "Native"}</span>
        <button className="secondary-button" type="button" onClick={onManage}>Manage membership</button>
      </header>
      {state === "partial" ? <div className="keeper-load-state" role="alert"><span>Some Apiary status could not be refreshed. Local workers and owned work are unchanged.</span><button type="button" onClick={() => void refresh()}>Try again</button></div> : null}
      <dl className="keeper-summary member-summary" aria-label="Member Apiary summary">
        <div><dt>Keeper</dt><dd>{keeper?.hive_name ?? "Waiting"}</dd></div>
        <div><dt>Catalog</dt><dd>{snapshot.catalog?.acknowledgement ? "Verified" : "Waiting"}</dd></div>
        <div><dt>Projects ready</dt><dd>{projectCount ? `${readyProjects}/${projectCount}` : "0"}</dd></div>
        <div><dt>My Jira claims</dt><dd>{localClaims.length}</dd></div>
        <div><dt>Keeper tasks</dt><dd>{snapshot.tasks.length}</dd></div>
        <div><dt>Pending changes</dt><dd>{snapshot.outboxStatus?.queued_count ?? 0}</dd></div>
      </dl>
      <div className="keeper-dashboard-grid" aria-busy={state === "loading"}>
        {stewardAssists.incoming.some((request) => request.state === "pending") ? <article className="keeper-panel steward-assist-inbox">
          <header><div><p className="eyebrow">Steward assistance</p><h4>A trusted Steward offered help</h4></div><small>Visible queue · never injected into a terminal</small></header>
          <ul aria-label="Pending Steward assistance requests">{stewardAssists.incoming.filter((request) => request.state === "pending").map((request) => <li key={request.id}>
            <span><strong>{hiveName.get(request.source_hive_id) ?? "Steward Hive"}</strong><small>{request.message}</small></span>
            <span className="member-handoff-actions"><button className="secondary-button" disabled={actingAssist === request.id} onClick={() => void respondToAssist(request.id, "declined")}>Decline</button><button className="primary-action" disabled={actingAssist === request.id} onClick={() => void respondToAssist(request.id, "accepted")}>{actingAssist === request.id ? "Saving…" : "Accept help"}</button></span>
          </li>)}</ul>
          {assistError ? <p className="member-command-attention" role="alert">{assistError}</p> : null}
        </article> : null}
        {(incomingHandoffs.length > 0 || outgoingHandoffs.length > 0) ? <article className="keeper-panel member-handoff-panel">
          <header><div><p className="eyebrow">Work handoffs</p><h4>{incomingHandoffs.length ? "Another Hive needs your help" : "Waiting on another Hive"}</h4></div><small>Jira ownership changes only after acceptance</small></header>
          <ul className="member-handoff-list" aria-label="Active Jira work handoffs">
            {incomingHandoffs.map((handoff) => <li key={handoff.id}>
              <span><strong>{handoff.issue_key}</strong><small>From {hiveName.get(handoff.source_hive_id) ?? "another Hive"}{handoff.reason ? ` · ${handoff.reason}` : ""}</small></span>
              {handoff.state === "offered" ? <span className="member-handoff-actions"><button className="secondary-button" disabled={actingHandoff === handoff.id} onClick={() => void transitionHandoff(handoff, "decline")}>Decline</button><button className="primary-action" disabled={actingHandoff === handoff.id} onClick={() => void transitionHandoff(handoff, "accept")}>{actingHandoff === handoff.id ? "Accepting…" : "Accept work"}</button></span> : <span className="member-handoff-progress"><strong>Accepted</strong><small>Assigning to you in Jira, then adding it to this Hive</small></span>}
            </li>)}
            {outgoingHandoffs.map((handoff) => <li key={handoff.id}>
              <span><strong>{handoff.issue_key}</strong><small>Offered to {hiveName.get(handoff.target_hive_id) ?? "another Hive"}{handoff.reason ? ` · ${handoff.reason}` : ""}</small></span>
              {handoff.state === "offered" ? <button className="secondary-button" disabled={actingHandoff === handoff.id} onClick={() => void transitionHandoff(handoff, "cancel")}>Cancel offer</button> : <span className="member-handoff-progress"><strong>Accepted</strong><small>You remain responsible until Jira confirms the transfer</small></span>}
            </li>)}
          </ul>
        </article> : null}
        {stewardship ? <article className="keeper-panel member-stewardship-panel">
          <header><div><p className="eyebrow">My Stewardship</p><h4>Trusted support for {managedHives.length} Hive{managedHives.length === 1 ? "" : "s"}</h4></div><span className="keeper-role-badge steward">Steward</span></header>
          <p className="member-sync-copy">Keeper has synchronized this authority to your Hive. Every action is checked again by Keeper before it changes shared work.</p>
          <dl className="member-detail-list">
            <div><dt>Hives in scope</dt><dd>{managedHives.join(", ")}</dd></div>
            <div><dt>Capabilities</dt><dd>{stewardship.capabilities.map(stewardCapabilityLabel).join(", ")}</dd></div>
          </dl>
          {stewardObservations.length ? <section className="steward-observations" aria-labelledby="steward-observations-heading">
            <header><div><p className="eyebrow">Observe</p><h5 id="steward-observations-heading">Shared-work pulse</h5></div><small>Keeper-known work only · private workers and terminals stay local</small></header>
            <ul aria-label="Managed Hive shared-work status">{stewardObservations.map((observation) => {
              const member = managedMembers.find((candidate) => candidate.hive_id === observation.hive_id);
              return <li key={observation.hive_id}>
                <span className="steward-observation-title"><strong>{member?.hive_name ?? "Managed Hive"}</strong><small>Last shared change {formatTimestamp(observation.last_shared_activity_at)}</small></span>
                <dl>
                  <div><dt>Ready</dt><dd>{observation.ready_swarm_task_count}</dd></div>
                  <div><dt>Active</dt><dd>{observation.active_swarm_task_count}</dd></div>
                  <div><dt>Blocked</dt><dd>{observation.blocked_swarm_task_count}</dd></div>
                  <div><dt>Review</dt><dd>{observation.review_swarm_task_count}</dd></div>
                  <div><dt>Jira owned</dt><dd>{observation.active_jira_claim_count}</dd></div>
                </dl>
              </li>;
            })}</ul>
          </section> : null}
          {canAssist ? <form className="steward-assist-form" onSubmit={(event) => { event.preventDefault(); void requestStewardAssist(); }}>
            <div className="steward-task-form-heading"><div><p className="eyebrow">Assist</p><h5>Offer help without interrupting anyone</h5></div><small>The target operator accepts or declines from her own Hive.</small></div>
            <label>Hive<select aria-label="Assistance target Hive" required value={assistTarget} onChange={(event) => setAssistTarget(event.target.value)}><option value="">Choose a managed Hive</option>{managedMembers.map((member) => <option key={member.hive_id} value={member.hive_id}>{member.hive_name}</option>)}</select></label>
            <label className="steward-assist-message">How can you help?<textarea aria-label="Steward assistance message" required maxLength={2000} value={assistMessage} onChange={(event) => setAssistMessage(event.target.value)} placeholder="A short, useful offer the operator can review when ready" /></label>
            <button className="primary-action" disabled={sendingAssist || !assistTarget || !assistMessage.trim()} type="submit">{sendingAssist ? "Sending…" : "Offer help through Keeper"}</button>
            {assistError ? <p className="member-command-attention" role="alert">{assistError}</p> : null}
          </form> : null}
          {sentAssists.length ? <ul className="steward-task-outbox" aria-label="Sent Steward assistance">{sentAssists.slice(0, 5).map((request) => <li key={request.id}><span><strong>{request.message}</strong><small>{managedMembers.find((member) => member.hive_id === request.target_hive_id)?.hive_name ?? "Managed Hive"}</small></span><span className={`keeper-role-badge ${request.state === "declined" ? "keeper" : "member"}`}>{request.state === "pending" ? "Waiting for operator" : request.state === "accepted" ? "Help accepted" : "Help declined"}</span></li>)}</ul> : null}
          {stewardship.capabilities.includes("assign") ? <div className="apiary-work-boundary">
            <span><strong>Route shared work from Tasks</strong><small>Your Steward scope appears there without exposing another Hive's private workers or repositories.</small></span>
            <button className="secondary-button" type="button" onClick={onOpenTasks}>Open Tasks</button>
          </div> : null}
          {snapshot.stewardTasks.some((entry) => entry.state !== "applied") ? <ul className="steward-task-outbox" aria-label="Steward task delivery status">{snapshot.stewardTasks.filter((entry) => entry.state !== "applied").map((entry) => <li key={entry.command.id}><span><strong>{entry.command.title}</strong><small>{managedMembers.find((member) => member.hive_id === entry.command.target_hive_id)?.hive_name ?? "Managed Hive"}</small></span><span className={`keeper-role-badge ${entry.state === "rejected" ? "keeper" : "member"}`}>{entry.state === "queued" ? "Sending to Keeper" : "Keeper declined"}</span></li>)}</ul> : null}
          {stewardAssists.outbox.some((entry) => entry.state !== "applied" && entry.command.action.kind === "request") ? <ul className="steward-task-outbox" aria-label="Steward assistance delivery status">{stewardAssists.outbox.map((entry) => {
            const action = entry.command.action;
            if (entry.state === "applied" || action.kind !== "request") return null;
            return <li key={entry.command.id}><span><strong>{action.message}</strong><small>{managedMembers.find((member) => member.hive_id === action.target_hive_id)?.hive_name ?? "Managed Hive"}</small></span><span className={`keeper-role-badge ${entry.state === "rejected" ? "keeper" : "member"}`}>{entry.state === "queued" ? "Sending to Keeper" : "Keeper declined"}</span></li>;
          })}</ul> : null}
        </article> : null}
        <article className="keeper-panel member-coordination-panel">
          <header><div><p className="eyebrow">Coordination</p><h4>Your place in the Apiary</h4></div><small>One operator, one independent Hive</small></header>
          <dl className="member-detail-list">
            <div><dt>This Hive</dt><dd>{identity.hive.name}</dd></div>
            <div><dt>Operator</dt><dd>{identity.operator.display_name}</dd></div>
            <div><dt>Keeper Hive</dt><dd>{keeper?.hive_name ?? "Waiting for roster"}</dd></div>
            <div><dt>Keeper</dt><dd>{keeper?.operator_display_name ?? "Waiting for roster"}</dd></div>
          </dl>
        </article>
        <article className="keeper-panel member-roster-panel">
          <header><div><p className="eyebrow">People and Hives</p><h4>Hives in this Apiary</h4></div><small>Shared identity and role · not live presence</small></header>
          {snapshot.members.length ? <ul className="member-project-list" aria-label="Apiary Hive roster">{snapshot.members.map((member) => <li key={member.hive_id}>
            <span><strong>{member.hive_name}</strong><small>{member.operator_display_name}</small></span>
            <span className={`keeper-role-badge ${member.role}`}>{member.role === "keeper" ? "Keeper" : member.hive_id === identity.hive.id ? "This Hive" : "Member"}</span>
          </li>)}</ul> : <p className="keeper-empty">The Apiary roster has not arrived yet.</p>}
        </article>
        <article className="keeper-panel member-sync-panel">
          <header><div><p className="eyebrow">Synchronization</p><h4>{syncTitle}</h4></div><span className={`apiary-sync-indicator apiary-sync-${syncCondition}`} aria-hidden="true" /></header>
          <p className="member-sync-copy">{syncDetail}</p>
          <dl className="member-detail-list compact">
            <div><dt>Retries</dt><dd>{snapshot.sync?.consecutive_failures ?? 0}</dd></div>
            <div><dt>Last success</dt><dd>{formatTimestamp(snapshot.sync?.last_success_at)}</dd></div>
            <div><dt>Keeper task cursor</dt><dd>{snapshot.taskSync?.cursor ?? 0}</dd></div>
            <div><dt>Tasks applied</dt><dd>{snapshot.taskSync?.task_count ?? 0}</dd></div>
          </dl>
          {snapshot.catalog?.blockers.length ? <ul className="member-blocker-list" aria-label="Shared work blockers">{snapshot.catalog.blockers.map((blocker) => <li key={blocker}>{catalogBlockerLabel(blocker)}</li>)}</ul> : <p className="member-ready-copy">Shared catalog prerequisites are ready.</p>}
        </article>
        <article className="keeper-panel member-task-panel">
          <header><div><p className="eyebrow">Shared work pulse</p><h4>Swarm tasks polled from Keeper</h4></div><button className="secondary-button" type="button" onClick={onOpenTasks}>Manage in Tasks</button></header>
          {(snapshot.outboxStatus?.conflict_count ?? 0) + (snapshot.outboxStatus?.rejected_count ?? 0) > 0 ? <p className="member-command-attention" role="status">{(snapshot.outboxStatus?.conflict_count ?? 0) + (snapshot.outboxStatus?.rejected_count ?? 0)} change{(snapshot.outboxStatus?.conflict_count ?? 0) + (snapshot.outboxStatus?.rejected_count ?? 0) === 1 ? "" : "s"} need review after Keeper reconciliation.</p> : null}
          {snapshot.tasks.length ? <ul className="keeper-work-list member-task-list" aria-label="Member Keeper tasks">{snapshot.tasks.map((task) => {
            const mine = task.home_hive_id === identity.hive.id;
            return <li key={task.id}>
              <span><strong>{task.title}</strong><small>{task.state} · {task.priority} · revision {task.revision}</small></span>
              <span className="member-task-owner">
                <strong>{mine ? "This Hive" : task.home_hive_id ? "Another Hive" : "Unassigned"}</strong>
                <small>{mine ? "Worker assignment stays private to this Hive" : "View only from Apiary"}</small>
              </span>
            </li>;
          })}</ul> : <p className="keeper-empty">No Swarm-generated Apiary tasks have been received.</p>}
        </article>
        <article className="keeper-panel">
          <header><div><p className="eyebrow">Shared catalog</p><h4>Projects available to this Hive</h4></div><small>Access is verified with your Jira identity</small></header>
          {snapshot.catalog?.projects.length ? <ul className="member-project-list" aria-label="Member promoted Jira projects">{snapshot.catalog.projects.map(({ project, binding_id, access_verified, workflow_mapped }) => {
            const ready = Boolean(binding_id && access_verified && workflow_mapped);
            return <li key={project.project_id}><span><strong>{project.project_key}</strong><small>{project.project_name}</small></span><span className={`keeper-role-badge ${ready ? "member" : "keeper"}`}>{ready ? "Ready" : "Needs setup"}</span></li>;
          })}</ul> : <p className="keeper-empty">No promoted projects are available to this Hive yet.</p>}
        </article>
        <article className="keeper-panel">
          <header><div><p className="eyebrow">Shared work</p><h4>Owned by this Hive</h4></div><small>Reservations and confirmed homes only</small></header>
          {localClaims.length ? <ul className="keeper-work-list member-claim-list" aria-label="Member shared work ownership">{localClaims.map((claim) => <li key={claim.id}><span><strong>{claim.issue_key}</strong><small>{claim.project_name}</small></span><span><strong>{claim.state === "confirmed" ? "Owned" : "Reserved"}</strong><small>{claim.home_operator_display_name}</small></span>{claim.state === "confirmed" ? <ClaimHandoffControl claim={claim} existing={activeHandoffByClaim.get(claim.id)} targets={snapshot.handoffTargets} busy={Boolean(actingHandoff)} onOffer={async (target, reason) => { setActingHandoff(claim.id); try { await offerApiaryClaimHandoff(operatorToken, claim.id, target, reason); await refresh(); } finally { setActingHandoff(undefined); } }} /> : null}</li>)}</ul> : <p className="keeper-empty">This Hive does not currently own shared Apiary work.</p>}
        </article>
      </div>
    </section>
  );
}

function ClaimHandoffControl({ claim, existing, targets, busy, onOffer }: { claim: ApiarySharedWorkClaim; existing?: FederationClaimHandoff; targets: FederationHandoffTarget[]; busy: boolean; onOffer: (target: string, reason: string) => Promise<void> }) {
  const [open, setOpen] = useState(false);
  const [target, setTarget] = useState("");
  const [reason, setReason] = useState("");
  if (existing) return <span className="member-claim-handoff-state">Handoff {existing.state}</span>;
  if (!targets.length) return null;
  if (!open) return <button className="secondary-button" type="button" onClick={() => setOpen(true)}>Offer to another Hive</button>;
  return <form className="member-claim-handoff-form" onSubmit={(event) => { event.preventDefault(); if (target) void onOffer(target, reason); }}>
    <label><span>Receiving Hive</span><select required value={target} onChange={(event) => setTarget(event.target.value)}><option value="">Choose a Hive</option>{targets.map((candidate) => <option key={candidate.node_id} value={candidate.node_id}>{candidate.hive_name} · {candidate.operator_display_name}</option>)}</select></label>
    <label><span>Why hand this off? <small>optional</small></span><input value={reason} maxLength={500} placeholder={`Context for ${claim.issue_key}`} onChange={(event) => setReason(event.target.value)} /></label>
    <span className="member-handoff-actions"><button className="secondary-button" type="button" disabled={busy} onClick={() => setOpen(false)}>Keep here</button><button className="primary-action" type="submit" disabled={busy || !target}>{busy ? "Offering…" : "Send offer"}</button></span>
  </form>;
}

function formatTimestamp(timestamp: number | null | undefined) {
  if (!timestamp) return "Not yet";
  return new Intl.DateTimeFormat(undefined, { dateStyle: "medium", timeStyle: "short" }).format(new Date(timestamp * 1000));
}

function stewardCapabilityLabel(capability: string) {
  return ({ observe: "Observe", assign: "Assign", assist: "Assist", takeover: "Take over", manage_projects: "Manage projects", manage_members: "Manage members" } as Record<string, string>)[capability] ?? capability;
}
