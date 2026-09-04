import { expect, test } from "vitest";
import type { RuntimeResources } from "../api";
import { BrowserPerformanceRecorder } from "../runtime/browserPerformance";
import { assessPerformance, computePressure } from "./performanceAssessment";

const now = 1_000_000;
function timing(delay = 0, at = now) {
  const recorder = new BrowserPerformanceRecorder(() => at);
  if (delay) recorder.record("interaction", delay);
  return { collection: "active", supported_observers: ["event"], current: recorder.snapshot(), before_reload: undefined };
}
function server(): RuntimeResources {
  const process = { resident_memory_bytes: 100, process_tree_resident_memory_bytes: 100, process_tree_process_count: 1, pressure: "normal" as const };
  return { sampled_at: now / 1000, api: process, terminal_host: process,
    policy: { mode: "observe_only", advisory_percent: 15, critical_percent: 25 },
    machine: { memory_total_bytes: 1000, memory_available_bytes: 800, memory_used_percent: 20, swap_total_bytes: 0, swap_used_bytes: 0, swap_used_percent: 0, logical_cpus: 8, load_average: [1, 1, 1], cpu_pressure_avg10: 0, memory_pressure_avg10: 0, io_pressure_avg10: 0, pressure: "normal" },
  };
}

test("separates browser delays from fresh server pressure without claiming causality", () => {
  const resources = server();
  expect(assessPerformance(timing(4000), resources, now).server_state).toBe("no_pressure");
  resources.machine!.cpu_pressure_avg10 = 12;
  const result = assessPerformance(timing(4000), resources, now);
  expect(result.headline).toBe("Browser delays and server pressure observed");
  expect(result.server_detail).toContain("do not prove");
  expect(result.recent_delay_metrics).toEqual(["interaction"]);
});

test("stale or future server samples cannot establish current pressure", () => {
  const resources = server();
  resources.machine!.pressure = "critical";
  resources.sampled_at -= 31;
  expect(assessPerformance(timing(), resources, now).server_state).toBe("stale");
  resources.sampled_at = now / 1000 + 6;
  expect(assessPerformance(timing(), resources, now).server_state).toBe("clock_mismatch");
});

test("historical incidents and pre-reload evidence do not remain current faults", () => {
  const old = timing(5000, now - 60_000);
  expect(old.current.incidents).toHaveLength(1);
  const result = assessPerformance({ ...timing(), before_reload: old.current }, server(), now);
  expect(result.browser_state).toBe("no_recent_delay");
  expect(assessPerformance(old, server(), now).browser_state).toBe("no_recent_delay");
  expect(result.browser_detail).toContain("not proof");
});

test("missing collection and incomplete or invalid measurements stay unknown", () => {
  expect(assessPerformance({ ...timing(), collection: "not_installed" }, undefined, now).browser_state).toBe("unavailable");
  const resources = server();
  resources.machine = undefined;
  expect(assessPerformance(timing(), resources, now).server_state).toBe("incomplete");
  expect(computePressure({ ...server().machine!, cpu_pressure_avg10: NaN, load_average: null })).toBeUndefined();
});

test("repeated short main-thread stalls count without inventing CPU utilization", () => {
  const recorder = new BrowserPerformanceRecorder(() => now);
  for (let i = 0; i < 12; i++) recorder.record("long_task", 100);
  const result = assessPerformance({ ...timing(), current: recorder.snapshot() }, server(), now);
  expect(result.browser_state).toBe("delay");
  expect(result.limitations).toContain("not Edge Task Manager CPU");
});
