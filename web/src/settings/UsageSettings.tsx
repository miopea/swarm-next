import { useCallback, useState } from "react";

import { fetchUsage, type UsageReport } from "../api/usage";
import { useVisiblePolling } from "../runtime/useVisiblePolling";

type Props = { operatorToken: string; visible: boolean };

const WINDOWS = [7, 14, 30] as const;

/** 1_234_567_890 -> "1.23B". Tokens run to billions; digits stop being read. */
function compact(value: number): string {
  const abs = Math.abs(value);
  if (abs >= 1_000_000_000) return `${(value / 1_000_000_000).toFixed(2)}B`;
  if (abs >= 1_000_000) return `${(value / 1_000_000).toFixed(1)}M`;
  if (abs >= 1_000) return `${(value / 1_000).toFixed(1)}k`;
  return `${value}`;
}

function percentChange(now: number, before: number): string | undefined {
  if (before <= 0) return undefined;
  const change = Math.round((now / before - 1) * 100);
  return `${change >= 0 ? "+" : ""}${change}%`;
}

function ago(unix: number, now: number): string {
  const seconds = Math.max(0, now - unix);
  if (seconds < 90) return "just now";
  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) return `${minutes}m ago`;
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return `${hours}h ago`;
  return `${Math.floor(hours / 24)}d ago`;
}

/** The repository name, which is what the operator calls a workspace. */
function shortWorkspace(path: string): string {
  const parts = path.split("/").filter(Boolean);
  return parts[parts.length - 1] ?? path;
}

/**
 * What each worker is spending with its provider.
 *
 * WHY THIS PANEL EXISTS, in the operator's words: "I 'feel' like I am burning
 * through tokens faster than normal but want to measure it. My guess is the
 * queen, but I am not sure." A feeling is not a measurement and this replaces
 * it with one.
 *
 * ⚠️ IT SAYS HOW OLD IT IS, ALWAYS. The figures come from a scan of the
 * provider's own transcripts, which on a busy Hive is gigabytes and takes
 * minutes. Asking for them schedules the next pass rather than waiting for it,
 * so what is drawn is always the last completed pass. A number with no stated
 * age gets trusted for longer than it deserves, so the age is in the header and
 * not in a tooltip.
 */
export default function UsageSettings({ operatorToken, visible }: Props) {
  const [report, setReport] = useState<UsageReport>();
  const [failed, setFailed] = useState(false);
  const [days, setDays] = useState<number>(7);
  const now = Math.floor(Date.now() / 1000);

  const load = useCallback(
    async (signal: AbortSignal) => {
      try {
        setReport(await fetchUsage(operatorToken, days, signal));
        setFailed(false);
      } catch {
        setFailed(true);
      }
    },
    [operatorToken, days],
  );
  // Polls while a scan is running so the panel fills in without a reload, and
  // slowly otherwise -- the underlying pass will not run more than every five
  // minutes however often this asks.
  useVisiblePolling(load, Boolean(operatorToken) && visible, report?.scanning ? 10_000 : 60_000, 20_000, {
    refreshKey: days,
  });

  const total = report?.total_weighted ?? 0;
  const change = report ? percentChange(total, report.total_weighted_previous) : undefined;
  const cacheRead = report?.by_day.reduce((sum, day) => sum + day.cache_read_tokens, 0) ?? 0;
  const cacheWrite = report?.by_day.reduce((sum, day) => sum + day.cache_write_tokens, 0) ?? 0;
  const hitRate = cacheRead + cacheWrite > 0 ? cacheRead / (cacheRead + cacheWrite) : undefined;
  const peak = Math.max(1, ...(report?.by_workspace.map((entry) => entry.weighted) ?? [1]));

  return (
    <section id="settings-usage" className="settings-card usage-settings" aria-labelledby="usage-heading">
      <div>
        <p className="eyebrow">Provider usage</p>
        <h3 id="usage-heading">What your workers are spending</h3>
      </div>
      <p className="usage-note">
        Counted from the provider&rsquo;s own transcripts, weighted by what each kind of token costs
        relative to one input token. A comparison between workers, not a bill &mdash; your invoice is
        the authority on money.
      </p>

      <div className="usage-controls">
        <div className="usage-window" role="group" aria-label="Window">
          {WINDOWS.map((option) => (
            <button
              key={option}
              type="button"
              aria-pressed={days === option}
              onClick={() => setDays(option)}
            >
              {option} days
            </button>
          ))}
        </div>
        <small className="usage-freshness">
          {report?.scanning
            ? "Reading transcripts…"
            : report?.last_scan_at
              ? `Measured ${ago(report.last_scan_at, now)}`
              : "Not measured yet"}
        </small>
      </div>

      {failed && !report ? (
        <p role="alert" className="usage-empty">
          Usage could not be read. The panel will try again on its own.
        </p>
      ) : null}

      {report && report.by_workspace.length === 0 ? (
        <p className="usage-empty">
          {report.last_scan_at
            ? "No provider usage recorded in this window."
            : "The first pass reads every transcript once and can take a few minutes. This fills in on its own."}
        </p>
      ) : null}

      {report && report.by_workspace.length > 0 ? (
        <>
          <div className="usage-headline">
            <strong>{compact(total)}</strong>
            <span>
              weighted tokens over {report.days} days
              {change ? <em className={total >= report.total_weighted_previous ? "usage-up" : "usage-down"}>{change}</em> : null}
            </span>
            {hitRate === undefined ? null : (
              <small title="Cache reads are billed at a tenth of fresh input. A high share here is cheap burn; cache writes rivalling reads is a cache being rebuilt rather than reused.">
                Cache hit {(hitRate * 100).toFixed(1)}%
              </small>
            )}
          </div>

          <ol className="usage-list">
            {report.by_workspace.map((entry) => {
              const share = total > 0 ? entry.weighted / total : 0;
              const entryChange = percentChange(entry.weighted, entry.weighted_previous);
              return (
                <li key={entry.workspace}>
                  <span className="usage-row-name">
                    <strong>{entry.workers.length > 0 ? entry.workers.join(", ") : shortWorkspace(entry.workspace)}</strong>
                    <small>{shortWorkspace(entry.workspace)}</small>
                  </span>
                  <span className="usage-bar" aria-hidden="true">
                    <span style={{ width: `${Math.round((entry.weighted / peak) * 100)}%` }} />
                  </span>
                  <span className="usage-row-figures">
                    <strong>{compact(entry.weighted)}</strong>
                    <small>
                      {(share * 100).toFixed(1)}%
                      {entryChange ? ` · ${entryChange}` : ""}
                    </small>
                  </span>
                </li>
              );
            })}
          </ol>

          <details className="usage-detail">
            <summary>Cached and uncached, by day</summary>
            <table>
              <thead>
                <tr>
                  <th scope="col">Day</th>
                  <th scope="col">Weighted</th>
                  <th scope="col">Cache read</th>
                  <th scope="col">Cache write</th>
                  <th scope="col">Fresh in</th>
                  <th scope="col">Out</th>
                </tr>
              </thead>
              <tbody>
                {report.by_day.map((day) => (
                  <tr key={day.day}>
                    <th scope="row">{day.day}</th>
                    <td>{compact(day.weighted)}</td>
                    <td>{compact(day.cache_read_tokens)}</td>
                    <td>{compact(day.cache_write_tokens)}</td>
                    <td>{compact(day.input_tokens)}</td>
                    <td>{compact(day.output_tokens)}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </details>
        </>
      ) : null}
    </section>
  );
}
