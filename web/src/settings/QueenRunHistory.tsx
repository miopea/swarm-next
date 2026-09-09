import { useCallback, useMemo, useState } from "react";
import { fetchQueenRunHistory, type QueenRunHistory as History, type QueenRunOutcome } from "../api/queenHistory";
import { useVisiblePolling } from "../runtime/useVisiblePolling";

const outcomes: Record<QueenRunOutcome,string> = {
  completed: "Finished", no_action: "No action", needs_operator: "Operator request pending", incomplete: "Incomplete coverage",
};

function interval(start: number | null, end: number | null): number | null {
  return start !== null && end !== null && Number.isSafeInteger(start) && Number.isSafeInteger(end)
    && start >= 0 && end >= start ? end - start : null;
}

export default function QueenRunHistory({ operatorToken }: { operatorToken: string }) {
  const [history, setHistory] = useState<History>();
  const [unavailable, setUnavailable] = useState(false);
  const load = useCallback(async (signal: AbortSignal) => {
    try {
      const current = await fetchQueenRunHistory(operatorToken, signal);
      if (signal.aborted) return;
      setHistory(current);
      setUnavailable(false);
    } catch {
      if (!signal.aborted || (signal.reason instanceof DOMException && signal.reason.name === "TimeoutError")) setUnavailable(true);
    }
  }, [operatorToken]);
  const refresh = useVisiblePolling(load, Boolean(operatorToken), null);
  const groups = useMemo(() => {
    const builds = new Map<string, NonNullable<History>["records"]>();
    for (const run of history?.records ?? []) {
      const build = run.finished_on_build ?? "Build not recorded";
      const group = builds.get(build) ?? [];
      group.push(run);
      builds.set(build, group);
    }
    return [...builds];
  }, [history]);
  return <section aria-labelledby="queen-run-history-heading">
    <h4 id="queen-run-history-heading">Queen run history</h4>
    {unavailable && <p role="status">Queen history is unavailable. Any displayed records are last known.</p>}
    {!unavailable && !history && <p role="status">Loading Queen history…</p>}
    {history && <p>{history.records.length} recent finishes shown of {history.retained_count} retained · up to {history.max_retained.toLocaleString()} records for {history.retention_days} days.</p>}
    {history?.records.length === 0 && <p>No recorded finishes yet. This does not mean Queen has done no work.</p>}
    {groups.map(([build, runs]) => {
      const counts = Object.fromEntries(Object.keys(outcomes).map(outcome => [outcome, runs.filter(run => run.accepted_outcome === outcome).length])) as Record<QueenRunOutcome,number>;
      const waits = runs.flatMap(run => { const seconds = interval(run.requested_at, run.delivered_at); return seconds === null ? [] : [seconds]; });
      const durations = runs.flatMap(run => { const seconds = interval(run.delivered_at, run.finished_at); return seconds === null ? [] : [seconds]; });
      const average = (values: number[]) => values.length ? `${Math.round(values.reduce((sum,value) => sum + value, 0) / values.length)} seconds (${values.length} samples)` : "Unavailable";
      return <details key={build}>
        <summary className="decision-prose">{build} · {runs.length} finishes · {counts.no_action} no action · {counts.incomplete} incomplete</summary>
        <dl>{Object.entries(outcomes).map(([outcome,label]) => <div key={outcome}><dt>{label}</dt><dd>{counts[outcome as QueenRunOutcome]}</dd></div>)}</dl>
        <p>Mean request-to-delivery: {average(waits)}. Mean delivery-to-finish: {average(durations)}.</p>
        <p>{runs.filter(run => run.requested_outcome !== run.accepted_outcome).length} requested outcomes changed by the existing coverage or decision checks.</p>
      </details>;
    })}
    <p>Explicit run finishes, not task completions or a productivity score. Unfinished, abandoned and pre-feature runs are not included.</p>
    <small>Build is recorded at finish; runs may span updates. Timing includes interruptions and continuations, not model CPU time. Means are not p95; compare equivalent workloads. Missing timing stays unavailable. Private Hive history; no terminal or message content.</small>
    <ReviewReturns history={history} />
    <button type="button" onClick={() => void refresh()}>Refresh Queen history</button>
  </section>;
}

function ReviewReturns({ history }: { history: History | undefined }) {
  if (!history) return null;
  const returns = history.review_returns;
  if (!returns) return <p>Review-return history is unavailable on this API. No zero-return result was inferred.</p>;
  const groups = new Map<string, typeof returns.records>();
  for (const record of returns.records) {
    const build = record.returned_on_build ?? "Build not recorded";
    const rows = groups.get(build) ?? [];
    rows.push(record);
    groups.set(build, rows);
  }
  return <section aria-labelledby="review-return-history-heading">
    <h4 id="review-return-history-heading">Review follow-ups</h4>
    <p>{returns.records.length} recent returns shown of {returns.retained_count} retained · up to {returns.max_retained.toLocaleString()} records for {returns.retention_days} days.</p>
    {[...groups].map(([build, rows]) => {
      const answered = rows.filter(row => row.answered_at !== null).length;
      const waits = rows.flatMap(row => { const seconds = interval(row.returned_at, row.answered_at); return seconds === null ? [] : [seconds]; });
      const mean = waits.length ? `${Math.round(waits.reduce((sum,value) => sum+value,0)/waits.length)} seconds (${waits.length} ${waits.length === 1 ? "sample" : "samples"})` : "Unavailable";
      return <details key={build}>
        <summary className="decision-prose">{build} · {rows.length} {rows.length === 1 ? "follow-up" : "follow-ups"} · {answered} exact {answered === 1 ? "answer" : "answers"}</summary>
        <p>Mean return-to-answer: {mean}.</p>
        <p>Without a recorded exact answer: {rows.length - answered}. These requests may have been superseded or reassigned; this is not the current waiting queue.</p>
      </details>;
    })}
    <small>Counts actual return requests and exact replies, not whether review improved the work. Repeated follow-ups count separately. History starts with this feature; build is recorded at return. No task or message content is collected.</small>
  </section>;
}
