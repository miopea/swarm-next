import type { BrowserMetric } from "../runtime/browserPerformance";

/** Labels describe the recorded boundary, not a stronger visual guarantee. */
export const browserTimingLabels: Record<BrowserMetric, string> = {
  long_task: "Main-thread blocks",
  interaction: "Interaction latency",
  route: "Navigation frame estimate",
  terminal_render: "Terminal apply latency",
  terminal_reconnect: "Terminal connection",
  terminal_grant: "Terminal access setup",
  terminal_socket: "Terminal socket opening",
  terminal_restore: "Terminal initial state",
};

export const browserTimingLimitations = "Terminal apply includes queueing, parsing and snapshot setup, not confirmed screen paint. Access setup includes the grant response; socket opening ends at WebSocket open; initial state runs from open through state application and overlaps apply latency. Phases count completed attempts, not necessarily successful full connections; do not add their averages. Navigation uses an animation-frame estimate. These timings do not measure tab CPU.";
