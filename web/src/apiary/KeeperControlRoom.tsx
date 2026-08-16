import { useCallback, useEffect, useMemo, useState } from "react";

import {
  createApiaryTask, fetchApiaryClaimHandoffs, fetchApiaryJiraProjects, fetchApiaryMembers, fetchApiarySharedWork, fetchApiaryStewardships, fetchApiaryStewardTaskAudit, fetchApiaryTasks,
  type ApiaryJiraProject, type ApiaryMember, type ApiarySharedWorkClaim, type ApiaryTask, type FederationClaimHandoff, type FederationStewardTaskAuditEntry, type HiveIdentity, type Stewardship, type TaskPriority,
} from "../api";
import BeeMascot from "../brand/BeeMascot";

type Props = { identity: HiveIdentity; operatorToken: string; onManage: () => void };
type KeeperSnapshot = { members: ApiaryMember[]; projects: ApiaryJiraProject[]; sharedWork: ApiarySharedWorkClaim[]; tasks: ApiaryTask[]; stewardships: Stewardship[]; stewardAudit: FederationStewardTaskAuditEntry[]; handoffs: FederationClaimHandoff[] };
const emptySnapshot: KeeperSnapshot = { members: [], projects: [], sharedWork: [], tasks: [], stewardships: [], stewardAudit: [], handoffs: [] };

export default function KeeperControlRoom({ identity, operatorToken, onManage }: Props) {
  const context = identity.apiary_context;
  const [snapshot, setSnapshot] = useState(emptySnapshot);
  const [state, setState] = useState<"loading" | "ready" | "error">("loading");
  const [composeOpen, setComposeOpen] = useState(false);
  const [title, setTitle] = useState("");
  const [description, setDescription] = useState("");
  const [priority, setPriority] = useState<TaskPriority>("normal");
  const [targetHiveId, setTargetHiveId] = useState("");
  const [creating, setCreating] = useState(false);
  const [createError, setCreateError] = useState<string>();
  const refresh = useCallback(async () => {
    setState("loading");
    try {
      const [members, projects, sharedWork, tasks, stewardships, stewardAudit, handoffs] = await Promise.all([
        fetchApiaryMembers(operatorToken), fetchApiaryJiraProjects(operatorToken),
        fetchApiarySharedWork(operatorToken), fetchApiaryTasks(operatorToken), fetchApiaryStewardships(operatorToken),
        fetchApiaryStewardTaskAudit(operatorToken),
        fetchApiaryClaimHandoffs(operatorToken),
      ]);
      setSnapshot({ members, projects, sharedWork, tasks, stewardships, stewardAudit: Array.isArray(stewardAudit) ? stewardAudit : [], handoffs: Array.isArray(handoffs) ? handoffs : [] });
      setState("ready");
    } catch { setState("error"); }
  }, [operatorToken]);
  useEffect(() => { void refresh(); }, [refresh]);

  const createSharedTask = useCallback(async (event: React.FormEvent) => {
    event.preventDefault();
    const normalizedTitle = title.trim();
    if (!normalizedTitle) return;
    setCreating(true);
    setCreateError(undefined);
    try {
      await createApiaryTask(operatorToken, { title: normalizedTitle, description: description.trim(), priority, home_hive_id: targetHiveId || undefined });
      setTitle("");
      setDescription("");
      setPriority("normal");
      setTargetHiveId("");
      setComposeOpen(false);
      await refresh();
    } catch {
      setCreateError("The shared task was not created. Your existing Apiary work is unchanged.");
    } finally {
      setCreating(false);
    }
  }, [description, operatorToken, priority, refresh, targetHiveId, title]);

  const members = useMemo(() => [...snapshot.members].sort((left, right) => Number(right.is_local) - Number(left.is_local) || left.hive_name.localeCompare(right.hive_name)), [snapshot.members]);
  const memberByOperator = useMemo(() => new Map(members.map((member) => [member.operator_id, member])), [members]);
  const memberByHive = useMemo(() => new Map(members.map((member) => [member.hive_id, member])), [members]);
  const stewardAuditByTask = useMemo(() => new Map(snapshot.stewardAudit.flatMap((entry) => entry.task_id ? [[entry.task_id, entry] as const] : [])), [snapshot.stewardAudit]);
  const activeHandoffs = useMemo(() => snapshot.handoffs.filter((handoff) => handoff.state === "offered" || handoff.state === "accepted"), [snapshot.handoffs]);
  if (context?.mode !== "federated" || context.local_role !== "keeper") return null;

  return (
    <section className="keeper-control-room" aria-labelledby="keeper-control-heading">
      <header className="keeper-hero">
        <div className="keeper-hero-mark"><BeeMascot role="queen" expression="focused" /></div>
        <div><p className="eyebrow">Keeper overview</p><h3 id="keeper-control-heading">{context.apiary.name}</h3><p>See durable Apiary ownership without pulling routine worker activity out of each Hive.</p></div>
        <span className="apiary-backend-badge">{context.apiary.shared_work_backend === "jira" ? "Jira-backed" : "Native"}</span>
        <button className="secondary-button" type="button" onClick={onManage}>Manage Apiary</button>
      </header>
      {state === "error" ? <div className="keeper-load-state" role="alert"><span>Apiary status could not be refreshed.</span><button type="button" onClick={() => void refresh()}>Try again</button></div> : null}
      <dl className="keeper-summary" aria-label="Apiary summary">
        <div><dt>Registered Hives</dt><dd>{members.length}</dd></div><div><dt>Promoted Jira projects</dt><dd>{snapshot.projects.length}</dd></div>
        <div><dt>Active Jira claims</dt><dd>{snapshot.sharedWork.length}</dd></div><div><dt>Work handoffs</dt><dd>{activeHandoffs.length}</dd></div><div><dt>Swarm tasks</dt><dd>{snapshot.tasks.length}</dd></div><div><dt>Steward scopes</dt><dd>{snapshot.stewardships.length}</dd></div>
      </dl>
      <div className="keeper-dashboard-grid" aria-busy={state === "loading"}>
        <article className="keeper-panel">
          <header><div><p className="eyebrow">People and Hives</p><h4>Apiary Hives</h4></div><small>Registration, not live presence</small></header>
          {state === "loading" && members.length === 0 ? <p className="keeper-empty">Gathering the Apiary roster…</p> : members.length ? <ul className="keeper-hive-list" aria-label="Keeper Apiary Hives">{members.map((member) => <li key={member.hive_id}><span className="worker-avatar"><BeeMascot role={member.role === "keeper" ? "queen" : "worker"} expression="available" /></span><span><strong>{member.hive_name}</strong><small>{member.operator_display_name}</small></span><span className={`keeper-role-badge ${member.role}`}>{member.role === "keeper" ? "Keeper" : "Hive"}{member.is_local ? " · This Hive" : ""}</span></li>)}</ul> : <p className="keeper-empty">No registered Hives are visible yet.</p>}
        </article>
        <article className="keeper-panel keeper-shared-work-panel">
          <header className="keeper-task-header"><div><p className="eyebrow">Shared work</p><h4>Keeper-canonical Swarm tasks</h4><small>Members retrieve these by polling Keeper</small></div><button className="secondary-button" type="button" aria-expanded={composeOpen} onClick={() => { setComposeOpen((open) => !open); setCreateError(undefined); }}>{composeOpen ? "Close task form" : "Create shared task"}</button></header>
          {composeOpen ? <form className="keeper-task-form" aria-label="Create shared Apiary task" onSubmit={(event) => void createSharedTask(event)}>
            <label className="keeper-task-title"><span>Outcome</span><input value={title} maxLength={240} required autoFocus placeholder="What should be true when this is done?" onChange={(event) => setTitle(event.target.value)} /></label>
            <label className="keeper-task-description"><span>Context <small>optional</small></span><textarea value={description} maxLength={10000} rows={3} placeholder="Why this matters, constraints, or what done looks like" onChange={(event) => setDescription(event.target.value)} /></label>
            <label><span>Priority</span><select value={priority} onChange={(event) => setPriority(event.target.value as TaskPriority)}><option value="low">Low</option><option value="normal">Normal</option><option value="high">High</option><option value="urgent">Urgent</option></select></label>
            <label><span>Route to Hive <small>optional</small></span><select value={targetHiveId} onChange={(event) => setTargetHiveId(event.target.value)}><option value="">Unassigned · any Member may claim</option>{members.filter((member) => member.role === "member").map((member) => <option key={member.hive_id} value={member.hive_id}>{member.hive_name} · {member.operator_display_name}</option>)}</select></label>
            <div className="keeper-task-submit"><p>{targetHiveId ? "The selected Hive owns this work. Her Queen chooses the private worker and repository." : "Created as unassigned shared work. A Member Hive can claim it without exposing private workers or repositories."}</p><button type="submit" disabled={creating || !title.trim()}>{creating ? "Creating…" : targetHiveId ? "Route to Hive" : "Create for Apiary"}</button></div>
            {createError ? <p className="form-error" role="alert">{createError}</p> : null}
          </form> : null}
          {snapshot.tasks.length ? <ul className="keeper-work-list" aria-label="Keeper Swarm tasks">{snapshot.tasks.map((task) => { const stewardAction = stewardAuditByTask.get(task.id); const steward = stewardAction ? memberByOperator.get(stewardAction.member_operator_id) : undefined; return <li key={task.id}><span><strong>{task.title}</strong><small>Swarm · {task.state}</small></span><span><strong>{task.home_hive_id ? memberByHive.get(task.home_hive_id)?.hive_name ?? "Assigned Hive" : "Unassigned"}</strong><small>{steward ? `Routed by Steward ${steward.operator_display_name}` : task.home_hive_id ? "Routed by Keeper" : "Available to claim"} · revision {task.revision}</small></span></li>; })}</ul> : <p className="keeper-empty">No Swarm-generated Apiary tasks are waiting.</p>}
          <header><div><p className="eyebrow">Jira ownership</p><h4>Current claims</h4></div><small>Issue data stays in Jira</small></header>
          {snapshot.sharedWork.length ? <ul className="keeper-work-list" aria-label="Keeper shared work ownership">{snapshot.sharedWork.map((claim) => <li key={claim.id}><span><strong>{claim.issue_key}</strong><small>{claim.project_key} · {claim.state === "confirmed" ? "Owned" : "Reserved"}</small></span><span><strong>{claim.home_hive_name}</strong><small>{claim.home_operator_display_name}</small></span></li>)}</ul> : <p className="keeper-empty">No shared Jira work is currently claimed by an Apiary Hive.</p>}
          {activeHandoffs.length ? <><header className="keeper-handoff-heading"><div><p className="eyebrow">Transfers</p><h4>Active Hive handoffs</h4></div><small>Source remains responsible until Jira confirms the new assignee</small></header><ul className="keeper-work-list" aria-label="Keeper active Jira handoffs">{activeHandoffs.map((handoff) => <li key={handoff.id}><span><strong>{handoff.issue_key}</strong><small>{handoff.state === "offered" ? "Awaiting acceptance" : "Changing Jira owner"}</small></span><span><strong>{memberByHive.get(handoff.source_hive_id)?.hive_name ?? "Source Hive"} → {memberByHive.get(handoff.target_hive_id)?.hive_name ?? "Receiving Hive"}</strong><small>{handoff.reason ?? "No handoff note"}</small></span></li>)}</ul></> : null}
        </article>
        <article className="keeper-panel">
          <header><div><p className="eyebrow">Shared catalog</p><h4>Promoted Jira projects</h4></div><small>Available to every joined Hive</small></header>
          {snapshot.projects.length ? <ul className="keeper-project-list" aria-label="Keeper promoted Jira projects">{snapshot.projects.map((project) => <li key={project.project_id}><strong>{project.project_key}</strong><span>{project.project_name}</span></li>)}</ul> : <p className="keeper-empty">No Jira projects have been promoted to this Apiary.</p>}
        </article>
        <article className="keeper-panel">
          <header><div><p className="eyebrow">Delegation</p><h4>Stewards</h4></div><small>Durable scopes, not routine noise</small></header>
          {snapshot.stewardships.length ? <ul className="keeper-steward-list" aria-label="Keeper Steward scopes">{snapshot.stewardships.map((scope) => { const steward = memberByOperator.get(scope.steward_operator_id); const hives = scope.managed_hive_ids.map((id) => memberByHive.get(id)?.hive_name ?? "Unknown Hive"); return <li key={scope.id}><span><strong>{steward?.operator_display_name ?? "Steward"}</strong><small>{steward?.hive_name ?? "Registered operator"}</small></span><span>{hives.join(", ") || "No Hives assigned"}</span></li>; })}</ul> : <p className="keeper-empty">No Stewards are delegated. Member Hives escalate directly to you.</p>}
          {snapshot.stewardAudit.length ? <><header className="keeper-steward-audit-heading"><div><p className="eyebrow">Guarded actions</p><h4>Recent Steward routing</h4></div><small>Keeper rechecked every action</small></header><ul className="keeper-steward-audit-list" aria-label="Keeper Steward task audit">{snapshot.stewardAudit.slice(0, 8).map((entry) => { const steward = memberByOperator.get(entry.member_operator_id); const target = memberByHive.get(entry.target_hive_id); return <li key={entry.command_id}><span><strong>{entry.title}</strong><small>{steward?.operator_display_name ?? "Steward"} → {target?.hive_name ?? "Managed Hive"}</small></span><span className={`keeper-role-badge ${entry.outcome === "rejected" ? "keeper" : "member"}`}>{entry.outcome === "applied" ? "Accepted" : "Declined"}</span></li>; })}</ul></> : null}
        </article>
      </div>
    </section>
  );
}
