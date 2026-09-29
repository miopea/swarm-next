import { useCallback, useEffect, useRef, useState } from "react";
import { fetchHiveLog, fetchLocalLog, type DiagnosticEntry, type HiveLog } from "../api";

type Props = {
  operatorToken: string;
  hiveName: string;
  /** A member's log as shared with this Keeper, or this Hive's own. */
  hiveId: string | undefined;
  onClose: () => void;
};

/** How often an open log refreshes itself. Display only; nothing waits on it. */
const REFRESH_MS = 10_000;

/**
 * A Hive's recent log, read from the Keeper's roster (ADR 0112).
 *
 * ⚠️ THIS EXISTS SO NOBODY HAS TO GO AND LOOK. On 2026-09-29 a takeover's
 * keystrokes were refused on the held Hive and the only record was a debug line
 * on that machine; the operator had to reproduce it by hand. A member now sends
 * its log as it synchronises, and this is where the Keeper reads it.
 */
export default function HiveLogDialog({ operatorToken, hiveName, hiveId, onClose }: Props) {
  const [log, setLog] = useState<HiveLog>();
  const [failed, setFailed] = useState(false);
  const [problemsOnly, setProblemsOnly] = useState(false);
  const dialog = useRef<HTMLElement>(null);

  const load = useCallback(async (signal?: AbortSignal) => {
    try {
      const next = hiveId === undefined ? await fetchLocalLog(operatorToken, signal) : await fetchHiveLog(operatorToken, hiveId, signal);
      setLog(next);
      setFailed(false);
    } catch (error) {
      if (!signal?.aborted && !(error instanceof DOMException && error.name === "AbortError")) setFailed(true);
    }
  }, [hiveId, operatorToken]);

  useEffect(() => {
    const controller = new AbortController();
    void load(controller.signal);
    const timer = window.setInterval(() => void load(controller.signal), REFRESH_MS);
    return () => { controller.abort(); window.clearInterval(timer); };
  }, [load]);

  useEffect(() => { dialog.current?.focus(); }, []);

  const shown = (log?.entries ?? [])
    .filter((entry) => !problemsOnly || entry.level === "error" || entry.level === "warn")
    .slice()
    .reverse();

  return (
    <div className="watch-overlay" onKeyDown={(event) => { if (event.key === "Escape") onClose(); }}>
      <section ref={dialog} tabIndex={-1} className="watch-window hive-log-window" role="dialog" aria-modal="true" aria-labelledby="hive-log-heading">
        <header>
          <div><p className="eyebrow">Recent log</p><h4 id="hive-log-heading">{hiveName}</h4></div>
          <span className="watch-window-tools">
            <label className="hive-log-filter"><input type="checkbox" checked={problemsOnly} onChange={(event) => setProblemsOnly(event.target.checked)} /> Warnings and errors only</label>
            <button type="button" className="secondary-button" onClick={() => void load()}>Refresh</button>
            <button type="button" className="secondary-button" onClick={onClose}>Close</button>
          </span>
        </header>
        <div className="hive-log-body">
          <p className="hive-log-status" role="status">{status(log, failed, hiveId === undefined)}</p>
          {shown.length ? (
            <ol className="hive-log-lines" aria-label={`Log lines from ${hiveName}, newest first`}>
              {/* Sequences restart when a member does; the time tells the runs apart. */}
              {shown.map((entry) => <LogLine key={`${entry.at_ms}-${entry.sequence}`} entry={entry} />)}
            </ol>
          ) : log ? <p className="keeper-empty">{problemsOnly ? "No warnings or errors in what was received." : "No log lines yet."}</p> : null}
        </div>
        <small>Swarm&apos;s own log lines only — never terminal output or anything typed.</small>
      </section>
    </div>
  );
}

function LogLine({ entry }: { entry: DiagnosticEntry }) {
  return (
    <li className={`hive-log-line ${entry.level}`}>
      <time dateTime={new Date(entry.at_ms).toISOString()}>{new Date(entry.at_ms).toLocaleTimeString()}</time>
      <span className="hive-log-level">{entry.level}</span>
      <span className="hive-log-message">{entry.message}<small>{entry.target}</small></span>
    </li>
  );
}

function status(log: HiveLog | undefined, failed: boolean, local: boolean): string {
  if (failed) return log ? "The log could not be refreshed; showing what was last received." : "The log could not be read.";
  if (!log) return "Reading the log…";
  if (local) return `This Hive's own log, ${log.entries.length} recent lines.`;
  if (log.received_at === null) return "Nothing received from this Hive yet. A member sends its log as it synchronises; one on an older release sends nothing.";
  const lost = log.dropped > 0 ? ` ${log.dropped} lines were lost before they could be sent.` : "";
  return `Last received ${new Date(log.received_at * 1000).toLocaleTimeString()}.${lost}`;
}
