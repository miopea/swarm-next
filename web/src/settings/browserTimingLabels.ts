import type { BrowserMetric } from "../runtime/browserPerformance";

/** Labels describe the recorded boundary, not a stronger visual guarantee. */
export const browserTimingLabels: Record<BrowserMetric, string> = {
  long_task: "Main-thread blocks",
  interaction: "Interaction latency",
  route: "Navigation frame estimate",
  terminal_render: "Terminal apply latency",
  terminal_reconnect: "Terminal connection",
};

export const browserTimingLimitations = "Terminal apply includes queueing, parsing and snapshot setup, not confirmed screen paint. Navigation uses an animation-frame estimate. These timings do not measure tab CPU.";
