import { describe, expect, it } from "vitest";

import type { MachineResources, ResourcePressure, RuntimeResources } from "../api";
import { machinePressureNotice } from "./machinePressure";

function machine(pressure: ResourcePressure, overrides: Partial<MachineResources> = {}): MachineResources {
  return {
    memory_total_bytes: 32 * 1024 ** 3,
    memory_available_bytes: 4 * 1024 ** 3,
    memory_used_percent: 88,
    swap_total_bytes: 8 * 1024 ** 3,
    swap_used_bytes: 1024 ** 3,
    swap_used_percent: 12.5,
    load_average: [7.25, 6.1, 5.4],
    logical_cpus: 4,
    memory_pressure_avg10: 3.2,
    cpu_pressure_avg10: 1.1,
    io_pressure_avg10: 0.4,
    pressure,
    ...overrides,
  };
}

function resources(m: MachineResources | undefined): RuntimeResources {
  return {
    sampled_at: 1,
    policy: { mode: "observe_only", advisory_percent: 85, critical_percent: 95 },
    api: { resident_memory_bytes: 1, pressure: "normal" },
    terminal_host: { resident_memory_bytes: 1, pressure: "normal" },
    machine: m,
  } as RuntimeResources;
}

const ready = (m: MachineResources | undefined) => ({ kind: "ready" as const, resources: resources(m) });

describe("machinePressureNotice", () => {
  it("says nothing when the machine is fine, so a badge always means something changed", () => {
    expect(machinePressureNotice(ready(machine("normal")))).toBeNull();
  });

  it("warns before the crash rather than at it", () => {
    const notice = machinePressureNotice(ready(machine("advisory")));
    expect(notice?.level).toBe("advisory");
    expect(notice?.label).toBe("Machine under load");
  });

  it("escalates a critical machine to its own level and label", () => {
    const notice = machinePressureNotice(ready(machine("critical")));
    expect(notice?.level).toBe("critical");
    expect(notice?.label).toBe("Machine critical");
  });

  // The three ways a reading can be missing. Each must reach the operator as
  // UNKNOWN, and none may be silent — silence here is indistinguishable from a
  // healthy machine, which is the defect this whole surface exists to avoid.
  it("reads a failed fetch as unknown, never as healthy", () => {
    const notice = machinePressureNotice({ kind: "failed" });
    expect(notice).not.toBeNull();
    expect(notice?.level).toBe("unknown");
    expect(notice?.label).toBe("Machine unknown");
  });

  it("reads an absent machine block as unknown, never as healthy", () => {
    const notice = machinePressureNotice(ready(undefined));
    expect(notice?.level).toBe("unknown");
  });

  it("reads an explicit unavailable pressure as unknown, never as healthy", () => {
    const notice = machinePressureNotice(ready(machine("unavailable")));
    expect(notice?.level).toBe("unknown");
  });

  it("stays silent only while loading, so the badge does not flash on every page load", () => {
    expect(machinePressureNotice({ kind: "loading" })).toBeNull();
  });

  it("distinguishes a failed read from a normal one — the ablation for the whole surface", () => {
    // If `failed` were ever collapsed into `loading` (or into normal), this is
    // the assertion that fails. A dead read rendering as a healthy machine is
    // the exact shape of defect this fleet has shipped before.
    const dead = machinePressureNotice({ kind: "failed" });
    const healthy = machinePressureNotice(ready(machine("normal")));
    expect(healthy).toBeNull();
    expect(dead).not.toBeNull();
  });

  it("carries the numbers behind the label so the operator can act on it", () => {
    const notice = machinePressureNotice(ready(machine("critical", { memory_used_percent: 96.4 })));
    expect(notice?.detail).toContain("memory 96% used");
    expect(notice?.detail).toContain("swap 13% used");
    expect(notice?.detail).toContain("memory stall 3.2%");
    expect(notice?.detail).toContain("load 7.25");
  });

  it("does not invent numbers it does not have", () => {
    const notice = machinePressureNotice(
      ready(machine("critical", {
        memory_used_percent: null,
        swap_used_percent: null,
        memory_pressure_avg10: null,
        load_average: null,
      })),
    );
    expect(notice?.detail).toBe("The machine's own memory and load could not be read.");
  });
});
