import { useCallback, useMemo, useState } from "react";
import { fetchBrowserEvidence, type BrowserEvidenceHour, type EvidenceTiming } from "../api";
import { BROWSER_METRICS } from "../runtime/browserPerformance";
import { useVisiblePolling } from "../runtime/useVisiblePolling";
import { browserTimingLabels, browserTimingLimitations } from "./browserTimingLabels";

export function summarizeBrowserEvidence(captures: BrowserEvidenceHour[]) {
  const builds = new Map<string, { build: string; captures: number; first: number; last: number;
    metrics: Record<typeof BROWSER_METRICS[number], { count: number; total_ms: number; max_ms: number }> }>();
  for (const capture of captures.slice(0, 100)) {
    let summary = builds.get(capture.build);
    if (!summary) {
      summary = { build: capture.build, captures: 0, first: capture.hour, last: capture.hour,
        metrics: Object.fromEntries(BROWSER_METRICS.map((metric) => [metric, { count: 0, total_ms: 0, max_ms: 0 }])) as Record<typeof BROWSER_METRICS[number], EvidenceTiming> };
      builds.set(capture.build, summary);
    }
    summary.captures++;
    summary.first = Math.min(summary.first, capture.hour);
    summary.last = Math.max(summary.last, capture.hour);
    for (const metric of BROWSER_METRICS) {
      const timing = summary.metrics[metric];
      timing.count += capture[metric].count;
      timing.total_ms += capture[metric].total_ms;
      timing.max_ms = Math.max(timing.max_ms, capture[metric].max_ms);
    }
  }
  return [...builds.values()].sort((a, b) => b.last - a.last);
}

export default function SavedBrowserEvidence({ operatorToken }: { operatorToken: string }) {
  const [captures, setCaptures] = useState<BrowserEvidenceHour[]>();
  const [unavailable, setUnavailable] = useState(false);
  const load = useCallback(async (signal: AbortSignal) => {
    try {
      const result = await fetchBrowserEvidence(operatorToken, signal);
      if (signal.aborted) return;
      setCaptures(result);
      setUnavailable(false);
    } catch {
      if (!signal.aborted || (signal.reason instanceof DOMException && signal.reason.name === "TimeoutError")) setUnavailable(true);
    }
  }, [operatorToken]);
  const refresh = useVisiblePolling(load, Boolean(operatorToken), null);
  const summaries = useMemo(() => summarizeBrowserEvidence(captures ?? []), [captures]);
  return <section aria-labelledby="saved-browser-evidence-title">
    <h4 id="saved-browser-evidence-title">Saved browser evidence by build</h4>
    <p>Latest {captures?.length ?? 0} captures, capped at 100 per read. Hour ranges show observed captures, not continuous coverage or time spent working.</p>
    {unavailable && <p role="status">Saved history is unavailable. Any displayed evidence is last known.</p>}
    {!unavailable && !captures && <p role="status">Loading saved history…</p>}
    {captures?.length === 0 && <p>No saved browser captures yet. Missing evidence is not a healthy result.</p>}
    {summaries.map((summary) => <details key={summary.build}>
      <summary>{summary.build} · {summary.captures} {summary.captures === 1 ? "capture" : "captures"}</summary>
      <p>UTC hours: {new Date(summary.first * 1000).toISOString().slice(0, 16)} to {new Date(summary.last * 1000).toISOString().slice(0, 16)}</p>
      <ul>{BROWSER_METRICS.map((metric) => {
        const timing = summary.metrics[metric];
        return <li key={metric}>{browserTimingLabels[metric]}: {timing.count ? `${timing.count} samples · mean ${Math.round(timing.total_ms / timing.count)} ms · max ${timing.max_ms} ms` : "No samples"}</li>;
      })}</ul>
      <p>{browserTimingLimitations}</p>
    </details>)}
    <p>These captures may mix devices and workloads. Means and maxima are not p95, and a difference between builds does not establish its cause. Compare equivalent workloads before judging a regression.</p>
    <button type="button" onClick={() => void refresh()}>Refresh saved history</button>
  </section>;
}
