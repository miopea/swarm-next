import type { MachineResources, ResourcePressure, RuntimeResources } from "../api";
import type { readBrowserPerformance } from "../runtime/browserPerformance";

/** Evidence classification, not per-tab CPU measurement or a causal diagnosis. */
export function computePressure(machine: MachineResources | undefined): ResourcePressure | undefined {
  if (!machine) return undefined;
  const stall = machine.cpu_pressure_avg10;
  if (stall != null && Number.isFinite(stall) && stall >= 0) {
    return stall >= 10 ? "critical" : stall >= 2 ? "advisory" : "normal";
  }
  const load = machine.load_average?.[0];
  const cpus = machine.logical_cpus;
  if (load == null || !Number.isFinite(load) || load < 0 || !cpus || !Number.isFinite(cpus) || cpus < 1) return undefined;
  return load / cpus >= 2 ? "critical" : load / cpus >= 1 ? "advisory" : "normal";
}

export function assessPerformance(browser: ReturnType<typeof readBrowserPerformance>, resources: RuntimeResources | undefined, now = Date.now()) {
  const windowMs = 30_000;
  const recent = browser.current.buckets.filter((bucket) => bucket.at >= now - windowMs && bucket.at <= now);
  const delayed = recent.flatMap((bucket) => Object.entries(bucket.metrics).filter(([kind, value]) => value && (
    value.max_ms > (kind === "terminal_reconnect" ? 2_000 : 1_000)
    || (kind === "long_task" && value.total_ms > 1_000)
  )).map(([kind]) => kind));
  const browserState = delayed.length ? "delay" : browser.collection !== "active" ? "unavailable" : "no_recent_delay";
  const age = resources ? now - resources.sampled_at * 1_000 : NaN;
  const freshness = !resources || !Number.isFinite(age) ? "unavailable" : age < -5_000 ? "clock_mismatch" : age > windowMs ? "stale" : "fresh";
  const pressures = [resources?.api.pressure, resources?.terminal_host.pressure, resources?.machine?.pressure, computePressure(resources?.machine)];
  const serverState = freshness !== "fresh" ? freshness : pressures.some((pressure) => pressure === "advisory" || pressure === "critical")
    ? "pressure" : pressures.every((pressure) => pressure === "normal") ? "no_pressure" : "incomplete";
  const headline = browserState === "delay"
    ? serverState === "pressure" ? "Browser delays and server pressure observed" : "Recent browser delay captured"
    : serverState === "pressure" ? "Server pressure observed"
      : browserState === "unavailable" || serverState !== "no_pressure" ? "Performance evidence incomplete" : "No slowdown established by current evidence";
  return {
    headline,
    browser_state: browserState,
    server_state: serverState,
    server_sample_age_ms: Number.isFinite(age) ? Math.max(0, Math.round(age)) : null,
    recent_delay_metrics: [...new Set(delayed)],
    browser_detail: browserState === "delay" ? "A delay was captured in the last 30 seconds; this does not identify its cause."
      : browserState === "unavailable" ? "Browser timing collection is unavailable."
        : "No delay captured in the last 30 seconds; this is not proof that every interaction was responsive.",
    server_detail: serverState === "pressure" ? "Fresh server samples report pressure; concurrent browser delays do not prove the server caused them."
      : serverState === "no_pressure" ? "The latest server sample reports no pressure in the measured layers."
        : serverState === "stale" ? "The server sample is over 30 seconds old; it cannot describe current pressure."
          : serverState === "clock_mismatch" ? "Server and browser clocks disagree; sample freshness cannot be established."
            : "Server evidence is incomplete or unavailable; do not assume the server is healthy.",
    limitations: "Browser timings are not Edge Task Manager CPU. Network latency and per-process causes require further evidence.",
  };
}
