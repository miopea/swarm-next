import { useEffect, useState } from "react";
import type { DevelopmentRuntime } from "../api";
import { BROWSER_METRICS, readBrowserPerformance } from "../runtime/browserPerformance";
import { terminalWorkspace } from "../terminal/TerminalWorkspace";

const labels = {
  long_task: "Main-thread blocks", interaction: "Interaction latency", route: "Navigation paint",
  terminal_render: "Terminal paint", terminal_reconnect: "Terminal connection",
};

export default function DeveloperDogfoodWorkspace({ runtime, version, reachable }: {
  runtime: DevelopmentRuntime | undefined; version: string | undefined; reachable: boolean;
}) {
  const [evidence, setEvidence] = useState(readBrowserPerformance);
  const [preview, setPreview] = useState(false);
  const [retention, setRetention] = useState(() => terminalWorkspace.rendererRetention);
  useEffect(() => {
    if (!runtime?.enabled) {
      terminalWorkspace.setWarmPoolExperiment(false);
      setRetention(terminalWorkspace.rendererRetention);
    }
  }, [runtime?.enabled]);
  if (!runtime?.enabled) return null;
  return <section id="settings-dogfood" className="settings-card" aria-labelledby="dogfood-heading">
    <div><p className="eyebrow">Developer Dogfood</p><h3 id="dogfood-heading">Evidence from your daily Hive</h3></div>
    <p>Enabled by this Hive’s existing development configuration. These are historical observations, not active alerts.</p>
    {!reachable && <p role="status">Development status is unavailable. Showing the last known build details.</p>}
    <dl>
      <dt>Running build</dt><dd>{version ?? runtime.version}</dd>
      <dt>Checkout revision (may differ from running build)</dt><dd>{runtime.source_revision ?? "Unknown"}{runtime.source_dirty ? " · uncommitted changes" : ""}</dd>
      <dt>Browser collection</dt><dd>{evidence.collection === "active" ? "Active" : "Not installed"}</dd>
      <dt>Native observers</dt><dd>{evidence.supported_observers.join(", ") || "Unavailable in this browser"}</dd>
    </dl>
    <p>Up to one hour of aggregate timings and five incidents, expiring after 24 hours. No terminal text, prompts, filenames, or image contents.</p>
    <ul>{BROWSER_METRICS.map((metric) => {
      const samples = evidence.current.buckets.flatMap((bucket) => bucket.metrics[metric] ? [bucket.metrics[metric]!] : []);
      const count = samples.reduce((sum, sample) => sum + sample.count, 0);
      const total = samples.reduce((sum, sample) => sum + sample.total_ms, 0);
      const maximum = samples.reduce((max, sample) => Math.max(max, sample.max_ms), 0);
      return <li key={metric}>{labels[metric]}: {count ? `${count} samples · mean ${Math.round(total / count)} ms · max ${Math.round(maximum)} ms` : "No samples"}</li>;
    })}</ul>
    <p>Means and maxima are not percentiles. Missing samples do not establish a healthy session.</p>
    <h4>Terminal warm-pool experiment</h4>
    <p>Off by default. Retain the active terminal and four recent renderers; colder views reconnect to the engine’s newest snapshot. Workers keep running. This browser only; reload resets the experiment.</p>
    <p>Evaluate repeated cold restores against the 500 ms p95 target before adopting this policy. These counts do not prove restore speed or resource savings.</p>
    <button type="button" aria-pressed={retention.limit !== undefined} onClick={() => {
      terminalWorkspace.setWarmPoolExperiment(retention.limit === undefined);
      setRetention(terminalWorkspace.rendererRetention);
    }}>{retention.limit === undefined ? "Try five-renderer pool" : "Stop warm-pool experiment"}</button>
    <p>{retention.retained} retained · {retention.attached} attached · {retention.inactive} inactive · {retention.evictions} evicted</p>
    <small>Stopping keeps already-evicted views cold until opened. Attached views are never evicted; a handoff can briefly exceed the limit.</small>
    <p>{evidence.current.incidents.length} retained incidents. {evidence.before_reload ? "Before-reload evidence is available." : "No before-reload evidence available."}</p>
    <button type="button" onClick={() => { setEvidence(readBrowserPerformance()); setRetention(terminalWorkspace.rendererRetention); }}>Refresh evidence</button>
    <button type="button" aria-expanded={preview} onClick={() => setPreview(!preview)}>Preview browser evidence</button>
    {preview && <pre>{JSON.stringify({ running_version: version ?? runtime.version, checkout_revision: runtime.source_revision, source_dirty: runtime.source_dirty, browser: evidence }, null, 2)}</pre>}
    <small>Long-term revision comparisons and instrumentation-overhead validation are still pending. This panel does not publish or cut releases.</small>
  </section>;
}
