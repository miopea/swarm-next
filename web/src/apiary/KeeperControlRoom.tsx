import { useCallback, useMemo, useState, type ReactNode } from "react";

import {
  fetchApiaryClaimHandoffs, fetchApiaryJiraProjects, fetchApiaryMembers, fetchApiarySharedWork, fetchApiaryStewardships, fetchApiaryStewardTaskAudit, fetchApiaryTasks, fetchFleetVersions, fetchTakeoverAudit, fetchTakeoverStatus, openApiaryTakeover, openApiaryWatch, endApiaryWatch,
  fetchApiaryMemberRemovalReadiness, removeApiaryMember, releaseApiaryTakeover,
  type ApiaryJiraProject, type ApiaryMember, type ApiarySharedWorkClaim, type ApiaryTask, type FederationClaimHandoff, type FederationStewardTaskAuditEntry, type ApiaryWatch, type FleetVersions as Fleet, type HiveIdentity, type TakeoverAuditEntry, type Stewardship,
} from "../api";
import BeeMascot from "../brand/BeeMascot";
import SharedTaskGroups, { isClosedSharedTask } from "./SharedTaskGroups";
import SharedProfileHint from "./SharedProfileHint";
import FleetVersions from "./FleetVersions";
import WatchWindow from "./WatchWindow";
import TakeoverAudit from "./TakeoverAudit";
import TakeoverWindow from "./TakeoverWindow";
import { useVisiblePolling } from "../runtime/useVisiblePolling";

type Props = { refreshKey?: string; identity: HiveIdentity; operatorToken: string; onManage: () => void; onReviewProfile?: () => void; onInvite: () => void; onOpenTasks: () => void };
type KeeperSnapshot = { members: ApiaryMember[]; projects: ApiaryJiraProject[]; sharedWork: ApiarySharedWorkClaim[]; tasks: ApiaryTask[]; stewardships: Stewardship[]; stewardAudit: FederationStewardTaskAuditEntry[]; handoffs: FederationClaimHandoff[]; fleet?: Fleet; takeovers: TakeoverAuditEntry[] };
const emptySnapshot: KeeperSnapshot = { members: [], projects: [], sharedWork: [], tasks: [], stewardships: [], stewardAudit: [], handoffs: [], fleet: undefined, takeovers: [] };
const snapshotKeys = ["members", "projects", "sharedWork", "tasks", "stewardships", "stewardAudit", "handoffs", "fleet", "takeovers"] as const;

export default function KeeperControlRoom({ identity, operatorToken, onManage, onReviewProfile, onInvite, onOpenTasks, refreshKey }: Props) {
  const context = identity.apiary_context;
  const [snapshot, setSnapshot] = useState(emptySnapshot);
  const [observed, setObserved] = useState<Set<keyof KeeperSnapshot>>(() => new Set());
  const [failed, setFailed] = useState<Set<keyof KeeperSnapshot>>(() => new Set());
  const [state, setState] = useState<"loading" | "ready" | "error">("loading");
  // The window this Keeper currently has open, if any. One at a time: a wall of
  // other people's terminals is surveillance wearing a dashboard, and nobody
  // asked for it.
  const [watching, setWatching] = useState<{ watch: ApiaryWatch; hiveName: string }>();
  const [watchError, setWatchError] = useState<string>();
  // The Hive the operator is being asked to confirm removing, with whatever
  // still holds it to the Apiary. Confirmation is deliberate: removal is not
  // destructive to the member's private work, but it IS visible to everyone
  // else in the Apiary and cannot be undone without a fresh invitation.
  const [removing, setRemoving] = useState<{ hiveId: string; hiveName: string; blockers: string[] }>();
  // The Hive this Keeper is currently controlling, if any.
  const [controlling, setControlling] = useState<{ leaseId: string; hiveName: string }>();
  const loadSnapshot = useCallback(async (signal: AbortSignal) => {
    setState("loading");
      const results = await Promise.allSettled([
        fetchApiaryMembers(operatorToken, signal), fetchApiaryJiraProjects(operatorToken, signal),
        fetchApiarySharedWork(operatorToken, signal), fetchApiaryTasks(operatorToken, signal), fetchApiaryStewardships(operatorToken, signal),
        fetchApiaryStewardTaskAudit(operatorToken, signal),
        fetchApiaryClaimHandoffs(operatorToken, signal),
        fetchFleetVersions(operatorToken, signal),
        fetchTakeoverAudit(operatorToken, signal),
      ]);
      if (signal.aborted) {
        if (signal.reason?.name === "TimeoutError") {
          setFailed(new Set(snapshotKeys));
          setState("error");
        }
        return;
      }
      const [members, projects, sharedWork, tasks, stewardships, stewardAudit, handoffs, fleet, takeovers] = results;
      setObserved((current) => new Set([...current, ...snapshotKeys.filter((_, index) => results[index].status === "fulfilled")]));
      setFailed(new Set(snapshotKeys.filter((_, index) => results[index].status === "rejected")));
      setSnapshot((current) => ({
        members: members.status === "fulfilled" ? members.value : current.members,
        projects: projects.status === "fulfilled" ? projects.value : current.projects,
        sharedWork: sharedWork.status === "fulfilled" ? sharedWork.value : current.sharedWork,
        tasks: tasks.status === "fulfilled" ? tasks.value : current.tasks,
        stewardships: stewardships.status === "fulfilled" ? stewardships.value : current.stewardships,
        stewardAudit: stewardAudit.status === "fulfilled" && Array.isArray(stewardAudit.value) ? stewardAudit.value : current.stewardAudit,
        handoffs: handoffs.status === "fulfilled" && Array.isArray(handoffs.value) ? handoffs.value : current.handoffs,
        fleet: fleet.status === "fulfilled" ? fleet.value : current.fleet,
        takeovers: takeovers.status === "fulfilled" && Array.isArray(takeovers.value) ? takeovers.value : current.takeovers,
      }));
      setState(results.some((result) => result.status === "rejected") ? "error" : "ready");
  }, [operatorToken]);
  const refresh = useVisiblePolling(loadSnapshot, Boolean(operatorToken), null, 8_000, { refreshKey });

  const members = useMemo(() => [...snapshot.members].sort((left, right) => Number(right.is_local) - Number(left.is_local) || left.hive_name.localeCompare(right.hive_name)), [snapshot.members]);
  const memberByOperator = useMemo(() => new Map(members.map((member) => [member.operator_id, member])), [members]);
  const memberByHive = useMemo(() => new Map(members.map((member) => [member.hive_id, member])), [members]);
  // ⚠️ THE ROSTER SHOWED NOTHING ABOUT A HIVE BUT ITS NAME. The version data
  // already arrived for the fleet panel; a roster row is where an operator
  // actually looks before deciding to watch or take one over.
  const versionByHive = useMemo(
    () => new Map((snapshot.fleet?.hives ?? []).map((hive) => [hive.hive_id, hive])),
    [snapshot.fleet],
  );
  const stewardAuditByTask = useMemo(() => new Map(snapshot.stewardAudit.flatMap((entry) => entry.task_id ? [[entry.task_id, entry] as const] : [])), [snapshot.stewardAudit]);
  const activeHandoffs = useMemo(() => snapshot.handoffs.filter((handoff) => handoff.state === "offered" || handoff.state === "accepted"), [snapshot.handoffs]);
  const count = (key: keyof KeeperSnapshot, value: number) => observed.has(key)
    ? failed.has(key) ? `${value} (last known)` : value
    : failed.has(key) ? "Unavailable" : "Loading…";
  const section = (key: keyof KeeperSnapshot, label: string, content: ReactNode) => !observed.has(key)
    ? <p className="keeper-empty">{failed.has(key) ? `${label} unavailable.` : `Loading ${label.toLowerCase()}…`}</p>
    : <>{failed.has(key) ? <p className="keeper-empty" role="status">{label}: showing last-known information.</p> : null}{content}</>;
  if (context?.mode !== "federated" || context.local_role !== "keeper") return null;

  return (
    <section className="keeper-control-room" aria-labelledby="keeper-control-heading">
      <header className="keeper-hero">
        <div className="keeper-hero-mark"><BeeMascot role="queen" expression="focused" /></div>
        <div><p className="eyebrow">Keeper overview</p><h3 id="keeper-control-heading">{context.apiary.name}</h3><p>See durable Apiary ownership without pulling routine worker activity out of each Hive.</p></div>
        <span className="apiary-backend-badge">Swarm shared work</span>
        <div className="keeper-hero-actions">
          <button className="primary-action" type="button" onClick={onInvite}>Invite a Hive</button>
          <button className="secondary-button" type="button" onClick={onManage}>Manage Apiary</button>
        </div>
      </header>
      <SharedProfileHint name={identity.operator.display_name} onReview={onReviewProfile ?? onManage} />
      {state === "error" ? <div className="keeper-load-state" role="alert"><span>Some Apiary status could not be refreshed. Last-known information is kept where available.</span><button type="button" onClick={() => void refresh()}>Try again</button></div> : null}
      <dl className="keeper-summary" aria-label="Apiary summary">
        <div><dt>Registered Hives</dt><dd>{count("members", members.length)}</dd></div><div><dt>Promoted Jira projects</dt><dd>{count("projects", snapshot.projects.length)}</dd></div>
        <div><dt>Active Jira claims</dt><dd>{count("sharedWork", snapshot.sharedWork.length)}</dd></div><div><dt>Work handoffs</dt><dd>{count("handoffs", activeHandoffs.length)}</dd></div><div><dt>Open Swarm tasks</dt><dd>{count("tasks", snapshot.tasks.filter((task) => !isClosedSharedTask(task)).length)}</dd></div><div><dt>Steward scopes</dt><dd>{count("stewardships", snapshot.stewardships.length)}</dd></div>
      </dl>
      <div className="keeper-dashboard-grid" aria-busy={state === "loading"}>
        <article className="keeper-panel">
          <header><div><p className="eyebrow">People and Hives</p><h4>Apiary Hives</h4></div><small>Registration, not live presence</small></header>
          {section("members", "Hive roster", <>
{state === "loading" && members.length === 0 ? <p className="keeper-empty">Gathering the Apiary roster…</p> : members.length ? <ul className="keeper-hive-list" aria-label="Keeper Apiary Hives">{members.map((member) => <li key={member.hive_id}><span className="worker-avatar"><BeeMascot role={member.role === "keeper" ? "queen" : "worker"} expression="available" /></span><span><strong>{member.hive_name}</strong><small>{member.operator_display_name}{member.operator_email ? ` · ${member.operator_email}` : ""}</small></span><span className={`keeper-role-badge ${member.role}`}>{member.role === "keeper" ? "Keeper" : "Hive"}{member.is_local ? " · This Hive" : ""}</span>{/* Not offered for this Hive: its terminal is already on this machine, and a window into yourself is a mirror. */}{member.is_local ? null : <span className="hive-actions"><small className={`hive-standing${versionByHive.get(member.hive_id)?.raises ? " raised" : ""}`}>{(() => {
            const seen = versionByHive.get(member.hive_id);
            if (!seen) return "No version reported";
            const standing = seen.standing === "current" ? "up to date"
              : seen.standing === "behind" ? "behind"
              : seen.standing === "behind_within_grace" ? "behind, within grace"
              : seen.standing === "schema_behind" ? "schema behind"
              : seen.standing === "development" ? "development build"
              : seen.standing === "unreadable" ? "version unreadable"
              : "not compared";
            return `${seen.swarm_version} · ${standing}`;
          })()}</small><button type="button" className="secondary-button" onClick={async () => {
            setWatchError(undefined);
            try {
              const watch = await openApiaryWatch(operatorToken, member.hive_id);
              setWatching({ watch, hiveName: member.hive_name });
            } catch {
              setWatchError(`${member.hive_name} could not be watched.`);
            }
          }}>Watch</button><button type="button" className="hive-takeover-button" onClick={async () => {
            // ⚠️ A REASON IS REQUIRED TO START, as ADR 0036 demands and unlike
            // watching, which the operator explicitly exempted. Typing on
            // somebody's machine should cost a sentence.
            const reason = window.prompt(`Why are you taking over ${member.hive_name}?`)?.trim();
            if (!reason) return;
            setWatchError(undefined);
            try {
              await openApiaryTakeover(operatorToken, member.hive_id, reason);
              // The target must ACKNOWLEDGE before anything is controllable, so
              // the lease is found on the next status read rather than assumed.
              const status = await fetchTakeoverStatus(operatorToken);
              const lease = status.held_by_me.find((held) => held.target_hive_id === member.hive_id);
              if (lease) setControlling({ leaseId: lease.id, hiveName: member.hive_name });
            } catch {
              setWatchError(`${member.hive_name} could not be taken over.`);
            }
          }}>Take over</button><button type="button" className="secondary-button hive-remove-button" onClick={async () => {
            setWatchError(undefined);
            try {
              // Asked BEFORE the confirmation, so the dialog can say what to
              // clear instead of refusing after the operator has committed.
              const readiness = await fetchApiaryMemberRemovalReadiness(operatorToken, member.hive_id);
              const blockers = [
                [readiness.active_jira_claim_count, "Jira claim"],
                [readiness.open_swarm_task_count, "open shared task"],
                [readiness.active_stewardship_count, "stewardship"],
                [readiness.pending_task_command_count, "task command still in flight"],
                [readiness.pending_jira_claim_count, "Jira claim still in flight"],
              ] as const;
              setRemoving({
                hiveId: member.hive_id,
                hiveName: member.hive_name,
                blockers: blockers.filter(([count]) => count > 0)
                  .map(([count, noun]) => `${count} ${noun}${count === 1 ? "" : "s"}`),
              });
            } catch {
              setWatchError(`${member.hive_name} could not be checked for removal.`);
            }
          }}>Remove</button></span>}</li>)}</ul> : <p className="keeper-empty">No registered Hives are visible yet.</p>}
          {removing ? <div className="keeper-remove-confirm" role="alertdialog" aria-label={`Remove ${removing.hiveName} from the Apiary`}>
            <p><strong>Remove {removing.hiveName} from the Apiary?</strong></p>
            {removing.blockers.length
              ? <p>It still holds {removing.blockers.join(", ")}. Removing is refused until that work is finished or reassigned.</p>
              : <p>Its shared work is clear. Its own workers, tasks and repositories stay on that machine; only its place in this Apiary ends. If it ever connects again it is told, and rejoining needs a fresh invitation.</p>}
            <span className="hive-actions">
              <button type="button" className="secondary-button" onClick={() => setRemoving(undefined)}>Cancel</button>
              <button type="button" className="hive-takeover-button" disabled={removing.blockers.length > 0} onClick={async () => {
                const { hiveId, hiveName } = removing;
                setRemoving(undefined);
                try {
                  await removeApiaryMember(operatorToken, hiveId);
                  await refresh();
                } catch {
                  setWatchError(`${hiveName} could not be removed.`);
                }
              }}>Remove from Apiary</button>
            </span>
          </div> : null}
          {watchError ? <p className="keeper-empty" role="alert">{watchError}</p> : null}
          {controlling ? <TakeoverWindow
            leaseId={controlling.leaseId}
            operatorToken={operatorToken}
            hiveName={controlling.hiveName}
            onClose={() => {
              // ⚠️ ENDED, NOT MERELY HIDDEN — the same lesson the watch window
              // already carries three lines below. Closing this used to leave
              // the lease open: the Hive went on telling its operator someone
              // else was controlling it, and every later takeover of it was
              // refused, because a Hive may hold only one open lease.
              void releaseApiaryTakeover(operatorToken, controlling.leaseId).catch(() => undefined);
              setControlling(undefined);
            }}
          /> : null}
          {watching ? <WatchWindow
            watchId={watching.watch.id}
            operatorToken={operatorToken}
            hiveName={watching.hiveName}
            onTakeOver={() => {
              // Watching is how an operator finds out a Hive needs hands on it.
              // The escalation belongs here rather than back in the roster.
              const member = members.find((entry) => entry.hive_name === watching.hiveName);
              const reason = window.prompt(`Why are you taking over ${watching.hiveName}?`)?.trim();
              if (!member || !reason) return;
              void endApiaryWatch(operatorToken, watching.watch.id).catch(() => undefined);
              setWatching(undefined);
              void (async () => {
                try {
                  await openApiaryTakeover(operatorToken, member.hive_id, reason);
                  const status = await fetchTakeoverStatus(operatorToken);
                  const lease = status.held_by_me.find((held) => held.target_hive_id === member.hive_id);
                  if (lease) setControlling({ leaseId: lease.id, hiveName: member.hive_name });
                } catch {
                  setWatchError(`${watching.hiveName} could not be taken over.`);
                }
              })();
            }}
            onClose={() => {
              // Ended rather than merely hidden. A closed window that left the
              // watch open would keep the other operator's notice up while
              // nobody was looking, which is its own kind of lie.
              void endApiaryWatch(operatorToken, watching.watch.id).catch(() => undefined);
              setWatching(undefined);
            }}
          /> : null}
          </>)}
        </article>
        <article className="keeper-panel keeper-shared-work-panel">
          <header className="keeper-task-header"><div><p className="eyebrow">Shared work</p><h4>Keeper-canonical Swarm tasks</h4><small>Members retrieve these by polling Keeper</small></div><button className="secondary-button" type="button" onClick={onOpenTasks}>Open Tasks</button></header>
          <p className="keeper-work-boundary">Create, route, and manage all work from Tasks. Apiary keeps this supervisory rollup focused on ownership across Hives.</p>
          {section("tasks", "Swarm tasks", <>
          <SharedTaskGroups tasks={snapshot.tasks} emptyMessage="No Swarm-generated Apiary tasks are waiting." renderTasks={(tasks) => <ul className="keeper-work-list" aria-label="Keeper Swarm tasks">{tasks.map((task) => { const stewardAction = stewardAuditByTask.get(task.id); const steward = stewardAction ? memberByOperator.get(stewardAction.member_operator_id) : undefined; return <li key={task.id}><span><strong>{task.title}</strong><small>Swarm · {task.state}</small></span><span><strong>{task.home_hive_id ? memberByHive.get(task.home_hive_id)?.hive_name ?? "Assigned Hive" : "Unassigned"}</strong><small>{steward ? `Routed by Steward ${steward.operator_display_name}` : task.home_hive_id ? "Routed by Keeper" : "Available to claim"} · revision {task.revision}</small></span></li>; })}</ul>} />
          </>)}
          <header><div><p className="eyebrow">Jira ownership</p><h4>Current claims</h4></div><small>Issue data stays in Jira</small></header>
          {section("sharedWork", "Jira claims", <>
          {snapshot.sharedWork.length ? <ul className="keeper-work-list" aria-label="Keeper shared work ownership">{snapshot.sharedWork.map((claim) => <li key={claim.id}><span><strong>{claim.issue_key}</strong><small>{claim.project_key} · {claim.state === "confirmed" ? "Owned" : "Reserved"}</small></span><span><strong>{claim.home_hive_name}</strong><small>{claim.home_operator_display_name}</small></span></li>)}</ul> : <p className="keeper-empty">No shared Jira work is currently claimed by an Apiary Hive.</p>}
          </>)}
          {section("handoffs", "Work handoffs", <>{activeHandoffs.length ? <><header className="keeper-handoff-heading"><div><p className="eyebrow">Transfers</p><h4>Active Hive handoffs</h4></div><small>Source remains responsible until Jira confirms the new assignee</small></header><ul className="keeper-work-list" aria-label="Keeper active Jira handoffs">{activeHandoffs.map((handoff) => <li key={handoff.id}><span><strong>{handoff.issue_key}</strong><small>{handoff.state === "offered" ? "Awaiting acceptance" : "Changing Jira owner"}</small></span><span><strong>{memberByHive.get(handoff.source_hive_id)?.hive_name ?? "Source Hive"} → {memberByHive.get(handoff.target_hive_id)?.hive_name ?? "Receiving Hive"}</strong><small>{handoff.reason ?? "No handoff note"}</small></span></li>)}</ul></> : null}</>)}
        </article>
        {section("fleet", "Swarm versions", <FleetVersions fleet={snapshot.fleet} nameFor={(hiveId) => memberByHive.get(hiveId)?.hive_name} />)}
        {section("takeovers", "Takeover history", <TakeoverAudit entries={snapshot.takeovers} nameFor={(hiveId) => memberByHive.get(hiveId)?.hive_name} />)}
        <article className="keeper-panel">
          <header><div><p className="eyebrow">Optional Jira work</p><h4>Promoted Jira projects</h4></div><small>Each Hive uses only projects its operator can access</small></header>
          {section("projects", "Jira projects", <>
          {snapshot.projects.length ? <ul className="keeper-project-list" aria-label="Keeper promoted Jira projects">{snapshot.projects.map((project) => <li key={project.project_id}><strong>{project.project_key}</strong><span>{project.project_name}</span></li>)}</ul> : <p className="keeper-empty">No Jira projects have been promoted to this Apiary.</p>}
          </>)}
        </article>
        <article className="keeper-panel">
          <header><div><p className="eyebrow">Delegation</p><h4>Stewards</h4></div><small>Durable scopes, not routine noise</small></header>
          {section("stewardships", "Steward scopes", <>
          {snapshot.stewardships.length ? <ul className="keeper-steward-list" aria-label="Keeper Steward scopes">{snapshot.stewardships.map((scope) => { const steward = memberByOperator.get(scope.steward_operator_id); const hives = scope.managed_hive_ids.map((id) => memberByHive.get(id)?.hive_name ?? "Unknown Hive"); return <li key={scope.id}><span><strong>{steward?.operator_display_name ?? "Steward"}</strong><small>{steward?.hive_name ?? "Registered operator"}</small></span><span>{hives.join(", ") || "No Hives assigned"}</span></li>; })}</ul> : <p className="keeper-empty">No Stewards are delegated. Member Hives escalate directly to you.</p>}
          </>)}
          {section("stewardAudit", "Steward routing", <>{snapshot.stewardAudit.length ? <><header className="keeper-steward-audit-heading"><div><p className="eyebrow">Guarded actions</p><h4>Recent Steward routing</h4></div><small>Keeper rechecked every action</small></header><ul className="keeper-steward-audit-list" aria-label="Keeper Steward task audit">{snapshot.stewardAudit.slice(0, 8).map((entry) => { const steward = memberByOperator.get(entry.member_operator_id); const target = memberByHive.get(entry.target_hive_id); return <li key={entry.command_id}><span><strong>{entry.title}</strong><small>{steward?.operator_display_name ?? "Steward"} → {target?.hive_name ?? "Managed Hive"}</small></span><span className={`keeper-role-badge ${entry.outcome === "rejected" ? "keeper" : "member"}`}>{entry.outcome === "applied" ? "Accepted" : "Declined"}</span></li>; })}</ul></> : null}</>)}
        </article>
      </div>
    </section>
  );
}
