import { useCallback, useEffect, useMemo, useState } from "react";

import {
  acceptApiaryClaimHandoff,
  cancelApiaryClaimHandoff,
  claimApiaryTask,
  declineApiaryClaimHandoff,
  fetchApiaryClaimHandoffs,
  fetchApiaryHandoffTargets,
  fetchApiaryMembers,
  fetchApiarySharedWork,
  fetchApiaryTasks,
  fetchFederationCatalogReadiness,
  fetchMyFederationStewardship,
  fetchFederationSyncHealth,
  fetchFederationTaskSyncStatus,
  fetchFederationTaskOutbox,
  fetchFederationTaskOutboxStatus,
  offerApiaryClaimHandoff,
  transitionApiaryTask,
  type ApiaryMember,
  type ApiarySharedWorkClaim,
  type ApiaryTask,
  type FederationCatalogReadiness,
  type FederationClaimHandoff,
  type FederationHandoffTarget,
  type FederationStewardshipSnapshot,
  type FederationSyncHealth,
  type FederationTaskSyncStatus,
  type FederationTaskOutboxEntry,
  type FederationTaskOutboxStatus,
  type HiveIdentity,
} from "../api";
import BeeMascot from "../brand/BeeMascot";
import { catalogBlockerLabel, federationSyncCopy } from "./presentation";

type Props = { identity: HiveIdentity; operatorToken: string; onManage: () => void };
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
  handoffs: FederationClaimHandoff[];
  handoffTargets: FederationHandoffTarget[];
};

const emptySnapshot: MemberSnapshot = { members: [], sharedWork: [], tasks: [], outbox: [], handoffs: [], handoffTargets: [] };

export default function MemberControlRoom({ identity, operatorToken, onManage }: Props) {
  const context = identity.apiary_context;
  const [snapshot, setSnapshot] = useState<MemberSnapshot>(emptySnapshot);
  const [state, setState] = useState<"loading" | "ready" | "partial">("loading");
  const refresh = useCallback(async () => {
    setState("loading");
    const [members, sharedWork, tasks, sync, taskSync, catalog, outbox, outboxStatus, stewardship, handoffs, handoffTargets] = await Promise.allSettled([
      fetchApiaryMembers(operatorToken),
      fetchApiarySharedWork(operatorToken),
      fetchApiaryTasks(operatorToken),
      fetchFederationSyncHealth(operatorToken),
      fetchFederationTaskSyncStatus(operatorToken),
      fetchFederationCatalogReadiness(operatorToken),
      fetchFederationTaskOutbox(operatorToken),
      fetchFederationTaskOutboxStatus(operatorToken),
      fetchMyFederationStewardship(operatorToken),
      fetchApiaryClaimHandoffs(operatorToken),
      fetchApiaryHandoffTargets(operatorToken),
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
      handoffs: handoffs.status === "fulfilled" && Array.isArray(handoffs.value) ? handoffs.value : current.handoffs,
      handoffTargets: handoffTargets.status === "fulfilled" && Array.isArray(handoffTargets.value) ? handoffTargets.value : current.handoffTargets,
    }));
    setState([members, sharedWork, tasks, sync, taskSync, catalog, outbox, outboxStatus, stewardship, handoffs, handoffTargets].some((result) => result.status === "rejected") ? "partial" : "ready");
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
  const managedHives = stewardship?.managed_hive_ids.map((hiveId) => snapshot.members.find((member) => member.hive_id === hiveId)?.hive_name ?? "Registered Hive") ?? [];
  const [actingTask, setActingTask] = useState<string>();
  const [actingHandoff, setActingHandoff] = useState<string>();
  const queuedTaskIds = useMemo(() => new Set(snapshot.outbox.filter((entry) => entry.state === "queued").map((entry) => entry.command.task_id)), [snapshot.outbox]);
  const act = useCallback(async (task: ApiaryTask, target?: ApiaryTask["state"]) => {
    setActingTask(task.id);
    try {
      if (target) await transitionApiaryTask(operatorToken, task.id, target);
      else await claimApiaryTask(operatorToken, task.id);
      await refresh();
    } finally { setActingTask(undefined); }
  }, [operatorToken, refresh]);
  const transitionHandoff = useCallback(async (handoff: FederationClaimHandoff, action: "accept" | "decline" | "cancel") => {
    setActingHandoff(handoff.id);
    try {
      if (action === "accept") await acceptApiaryClaimHandoff(operatorToken, handoff.id);
      else if (action === "decline") await declineApiaryClaimHandoff(operatorToken, handoff.id);
      else await cancelApiaryClaimHandoff(operatorToken, handoff.id);
      await refresh();
    } finally { setActingHandoff(undefined); }
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
          <p className="member-sync-copy">Keeper has synchronized this authority to your Hive. Each remote action remains unavailable until its matching guarded command is enabled.</p>
          <dl className="member-detail-list">
            <div><dt>Hives in scope</dt><dd>{managedHives.join(", ")}</dd></div>
            <div><dt>Capabilities</dt><dd>{stewardship.capabilities.map(stewardCapabilityLabel).join(", ")}</dd></div>
          </dl>
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
        <article className="keeper-panel">
          <header><div><p className="eyebrow">Swarm tasks</p><h4>Polled from Keeper</h4></div><small>Keeper is canonical for this source</small></header>
          {(snapshot.outboxStatus?.conflict_count ?? 0) + (snapshot.outboxStatus?.rejected_count ?? 0) > 0 ? <p className="member-command-attention" role="status">{(snapshot.outboxStatus?.conflict_count ?? 0) + (snapshot.outboxStatus?.rejected_count ?? 0)} change{(snapshot.outboxStatus?.conflict_count ?? 0) + (snapshot.outboxStatus?.rejected_count ?? 0) === 1 ? "" : "s"} need review after Keeper reconciliation.</p> : null}
          {snapshot.tasks.length ? <ul className="keeper-work-list member-task-list" aria-label="Member Keeper tasks">{snapshot.tasks.map((task) => {
            const mine = task.home_hive_id === identity.hive.id;
            const queued = queuedTaskIds.has(task.id);
            const next = task.state === "ready" ? "active" : task.state === "active" ? "review" : task.state === "review" ? "completed" : undefined;
            return <li key={task.id}><span><strong>{task.title}</strong><small>{task.state} · {task.priority} · revision {task.revision}</small></span><span className="member-task-owner"><strong>{mine ? "This Hive" : task.home_hive_id ? "Another Hive" : "Unassigned"}</strong>{queued ? <small>Queued for Keeper</small> : !task.home_hive_id ? <button className="secondary-button" type="button" disabled={actingTask === task.id} onClick={() => void act(task)}>Claim for this Hive</button> : mine && next ? <button className="secondary-button" type="button" disabled={actingTask === task.id} onClick={() => void act(task, next)}>Move to {next}</button> : null}</span></li>;
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
