import { useCallback, useEffect, useMemo, useState } from "react";

import {
  fetchApiaryDepartureStatus,
  leaveApiary,
  type ApiaryDepartureStatus,
} from "../api";

type Props = {
  apiaryName: string;
  busy: boolean;
  operatorToken: string;
  onLeft: () => Promise<void>;
};

export default function MemberDeparturePanel({ apiaryName, busy, operatorToken, onLeft }: Props) {
  const [status, setStatus] = useState<ApiaryDepartureStatus>();
  const [loading, setLoading] = useState(true);
  const [working, setWorking] = useState(false);
  const [reviewing, setReviewing] = useState(false);
  const [confirmation, setConfirmation] = useState("");
  const [error, setError] = useState("");

  const refresh = useCallback(async () => {
    setLoading(true);
    try {
      setStatus(await fetchApiaryDepartureStatus(operatorToken));
      setError("");
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "Departure readiness could not be checked.");
    } finally {
      setLoading(false);
    }
  }, [operatorToken]);

  useEffect(() => { void refresh(); }, [refresh]);

  const blockers = useMemo(() => departureBlockers(status), [status]);
  const canLeave = Boolean(status?.keeper_reachable && blockers.length === 0);
  const frozen = status?.state === "departing";

  async function leave() {
    setWorking(true);
    setError("");
    try {
      await leaveApiary(operatorToken);
      await onLeft();
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "The Keeper did not confirm this departure.");
      await refresh();
    } finally {
      setWorking(false);
    }
  }

  return (
    <section className={`apiary-departure${frozen ? " departure-paused" : ""}`} aria-labelledby="apiary-departure-heading">
      <div className="apiary-section-heading">
        <span>
          <strong id="apiary-departure-heading">Leave this Apiary</strong>
          <small>Return this installation to a personal Hive without moving or deleting your private work.</small>
        </span>
        {frozen ? <span className="apiary-departure-state">Departure paused safely</span> : null}
      </div>

      <div className="apiary-departure-boundary" role="note">
        <span><strong>Stays with this Hive</strong><small>Workers, repositories, provider conversations, private tasks, settings, and Hive-owned integrations.</small></span>
        <span><strong>Leaves shared coordination</strong><small>Apiary membership, shared task projections, Steward authority, and active cross-Hive ownership.</small></span>
      </div>

      {status ? (
        <dl className="apiary-departure-readiness">
          <div><dt>Jira claims</dt><dd>{status.readiness.active_jira_claim_count}</dd></div>
          <div><dt>Apiary tasks</dt><dd>{status.readiness.open_swarm_task_count}</dd></div>
          <div><dt>Stewardships</dt><dd>{status.readiness.active_stewardship_count}</dd></div>
          <div><dt>Task updates</dt><dd>{status.readiness.pending_task_command_count}</dd></div>
          <div><dt>Claim updates</dt><dd>{status.readiness.pending_jira_claim_count}</dd></div>
        </dl>
      ) : loading ? <small>Checking this Hive and its Keeper…</small> : null}

      {status && !status.keeper_reachable ? (
        <p className="apiary-blockers">The Keeper cannot be reached right now. This Hive remains a member. Reconnect and check again before leaving.</p>
      ) : null}
      {blockers.length > 0 ? <p className="apiary-blockers">Clear before leaving: {blockers.join(", ")}.</p> : null}
      {frozen ? (
        <p className="apiary-departure-explanation">No partial departure occurred. New shared changes are paused on this Hive while Swarm retries the same signed request.</p>
      ) : null}
      {error ? <p className="form-error" role="alert">{error}</p> : null}

      {frozen ? (
        <div className="settings-actions">
          <button className="secondary-button" disabled={busy || working || loading} onClick={() => void refresh()}>Check Keeper</button>
          <button className="danger-button" disabled={busy || working || !canLeave} onClick={() => void leave()}>{working ? "Retrying…" : "Retry departure"}</button>
        </div>
      ) : !reviewing ? (
        <button className="secondary-button" disabled={busy || working || loading || !canLeave} onClick={() => setReviewing(true)}>Review leaving Apiary</button>
      ) : (
        <div className="apiary-confirmation" role="group" aria-label="Confirm leaving Apiary">
          <strong>Leave {apiaryName}?</strong>
          <span>The Keeper keeps the shared audit history. Jira projects connected through this Apiary become Hive-owned here so linked work remains readable.</span>
          <label className="field-stack" htmlFor="leave-apiary-confirmation">
            <span>Type {apiaryName} to confirm</span>
            <input id="leave-apiary-confirmation" autoComplete="off" value={confirmation} onChange={(event) => setConfirmation(event.target.value)} />
          </label>
          <div className="settings-actions">
            <button className="secondary-button" disabled={working} onClick={() => { setReviewing(false); setConfirmation(""); }}>Keep membership</button>
            <button className="danger-button" disabled={busy || working || confirmation !== apiaryName} onClick={() => void leave()}>{working ? "Leaving…" : "Leave Apiary"}</button>
          </div>
        </div>
      )}
    </section>
  );
}

function departureBlockers(status: ApiaryDepartureStatus | undefined) {
  if (!status) return [];
  const value = status.readiness;
  const blockers: string[] = [];
  if (value.active_jira_claim_count) blockers.push(`${value.active_jira_claim_count} active Jira ${value.active_jira_claim_count === 1 ? "claim" : "claims"}`);
  if (value.open_swarm_task_count) blockers.push(`${value.open_swarm_task_count} open Apiary ${value.open_swarm_task_count === 1 ? "task" : "tasks"}`);
  if (value.active_stewardship_count) blockers.push(`${value.active_stewardship_count} active ${value.active_stewardship_count === 1 ? "Stewardship" : "Stewardships"}`);
  if (value.pending_task_command_count) blockers.push(`${value.pending_task_command_count} unsent task ${value.pending_task_command_count === 1 ? "update" : "updates"}`);
  if (value.pending_jira_claim_count) blockers.push(`${value.pending_jira_claim_count} pending Jira claim ${value.pending_jira_claim_count === 1 ? "change" : "changes"}`);
  return blockers;
}
