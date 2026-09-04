import { BrowserPerformanceRecorder } from "../runtime/browserPerformance";
import PerformanceEvidence from "../settings/PerformanceEvidence";
import { assessPerformance } from "../settings/performanceAssessment";
import type { RuntimeResources } from "../api";

export default function PerformanceEvidenceFixture() {
  const now = Date.now();
  const recorder = new BrowserPerformanceRecorder(() => now);
  recorder.record("interaction", 4200);
  const timing = { collection: "active", supported_observers: ["event"], current: recorder.snapshot(), before_reload: undefined };
  const resources: RuntimeResources = {
    sampled_at: now / 1000,
    policy: { mode: "observe_only", advisory_percent: 15, critical_percent: 25 },
    api: { resident_memory_bytes: 100, pressure: "normal" },
    terminal_host: { resident_memory_bytes: 100, pressure: "normal" },
    machine: { memory_total_bytes: 32e9, memory_available_bytes: 20e9, memory_used_percent: 38, swap_total_bytes: 0, swap_used_bytes: 0, swap_used_percent: 0, logical_cpus: 8, load_average: [1, 1, 1], cpu_pressure_avg10: 12, memory_pressure_avg10: 0, io_pressure_avg10: 0, pressure: "normal" },
  };
  return <main>
    <h2>Performance evidence — synthetic examples</h2>
    <p>Presentation checks only; these are not measurements of your browser or server.</p>
    <section className="settings-card"><PerformanceEvidence evidence={assessPerformance(timing, resources, now)} /></section>
    <section className="settings-card"><PerformanceEvidence evidence={assessPerformance(timing, { ...resources, sampled_at: now / 1000 - 60 }, now)} /></section>
    <section className="settings-card"><PerformanceEvidence evidence={assessPerformance({ ...timing, collection: "not_installed", current: new BrowserPerformanceRecorder(() => now).snapshot() }, undefined, now)} /></section>
  </main>;
}
