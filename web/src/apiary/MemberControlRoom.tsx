import { useCallback, useEffect, useMemo, useState } from "react";

import {
  fetchApiaryMembers,
  fetchApiarySharedWork,
  fetchApiaryTasks,
  fetchFederationCatalogReadiness,
  fetchFederationSyncHealth,
  fetchFederationTaskSyncStatus,
  type ApiaryMember,
  type ApiarySharedWorkClaim,
  type ApiaryTask,
  type FederationCatalogReadiness,
  type FederationSyncHealth,
  type FederationTaskSyncStatus,
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
};

const emptySnapshot: MemberSnapshot = { members: [], sharedWork: [], tasks: [] };

export default function MemberControlRoom({ identity, operatorToken, onManage }: Props) {
  const context = identity.apiary_context;
  const [snapshot, setSnapshot] = useState<MemberSnapshot>(emptySnapshot);
  const [state, setState] = useState<"loading" | "ready" | "partial">("loading");
  const refresh = useCallback(async () => {
    setState("loading");
    const [members, sharedWork, tasks, sync, taskSync, catalog] = await Promise.allSettled([
      fetchApiaryMembers(operatorToken),
      fetchApiarySharedWork(operatorToken),
      fetchApiaryTasks(operatorToken),
      fetchFederationSyncHealth(operatorToken),
      fetchFederationTaskSyncStatus(operatorToken),
      fetchFederationCatalogReadiness(operatorToken),
    ]);
    setSnapshot((current) => ({
      members: members.status === "fulfilled" ? members.value : current.members,
      sharedWork: sharedWork.status === "fulfilled" ? sharedWork.value : current.sharedWork,
      tasks: tasks.status === "fulfilled" ? tasks.value : current.tasks,
      sync: sync.status === "fulfilled" ? sync.value : current.sync,
      taskSync: taskSync.status === "fulfilled" ? taskSync.value : current.taskSync,
      catalog: catalog.status === "fulfilled" ? catalog.value : current.catalog,
    }));
    setState([members, sharedWork, tasks, sync, taskSync, catalog].some((result) => result.status === "rejected") ? "partial" : "ready");
  }, [operatorToken]);
  useEffect(() => { void refresh(); }, [refresh]);

  const keeper = snapshot.members.find((member) => member.role === "keeper");
  const localClaims = useMemo(
    () => snapshot.sharedWork.filter((claim) => claim.home_hive_id === identity.hive.id),
    [identity.hive.id, snapshot.sharedWork],
  );
  const readyProjects = snapshot.catalog?.projects.filter((project) => project.binding_id && project.access_verified && project.workflow_mapped).length ?? 0;
  const projectCount = snapshot.catalog?.projects.length ?? 0;
  const syncCondition = snapshot.sync?.condition ?? "idle";
  const [syncTitle, syncDetail] = federationSyncCopy[syncCondition];

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
      </dl>
      <div className="keeper-dashboard-grid" aria-busy={state === "loading"}>
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
          {snapshot.tasks.length ? <ul className="keeper-work-list" aria-label="Member Keeper tasks">{snapshot.tasks.map((task) => <li key={task.id}><span><strong>{task.title}</strong><small>{task.state} · {task.priority}</small></span><span><strong>{task.home_hive_id === identity.hive.id ? "This Hive" : task.home_hive_id ? "Another Hive" : "Unassigned"}</strong><small>Revision {task.revision}</small></span></li>)}</ul> : <p className="keeper-empty">No Swarm-generated Apiary tasks have been received.</p>}
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
          {localClaims.length ? <ul className="keeper-work-list" aria-label="Member shared work ownership">{localClaims.map((claim) => <li key={claim.id}><span><strong>{claim.issue_key}</strong><small>{claim.project_name}</small></span><span><strong>{claim.state === "confirmed" ? "Owned" : "Reserved"}</strong><small>{claim.home_operator_display_name}</small></span></li>)}</ul> : <p className="keeper-empty">This Hive does not currently own shared Apiary work.</p>}
        </article>
      </div>
    </section>
  );
}

function formatTimestamp(timestamp: number | null | undefined) {
  if (!timestamp) return "Not yet";
  return new Intl.DateTimeFormat(undefined, { dateStyle: "medium", timeStyle: "short" }).format(new Date(timestamp * 1000));
}
