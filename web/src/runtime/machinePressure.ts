import type { RuntimeResources } from "../api";

/**
 * What the control room says about the machine underneath it.
 *
 * WHY THIS EXISTS. Swarm starts processes. A Hive that quietly runs a box out
 * of memory takes the operator's whole machine down with it, and until this
 * existed nothing on the way there warned them. The server has computed
 * pressure for some time — ResourcePressure in crates/swarm-api/src/runtime.rs,
 * tuned against a real report so that a large share of an UNSTRESSED machine is
 * not called Critical — and ADR 0040 already refuses automatic worker starts
 * against it. It was simply never shown anywhere the operator looks: the only
 * render was Settings -> Diagnostics, which is not a screen anyone sits on
 * while their machine is dying.
 *
 * NORMAL IS SILENT, AND THAT IS THE POINT. A header that shows a state most of
 * the day teaches the operator to stop reading it, which is precisely the
 * failure this exists to prevent. So Normal returns null and the header keeps
 * saying only "Runtime <version>". A visible badge therefore always means
 * something changed.
 *
 * UNKNOWN IS NOT HEALTHY. A failed read, an absent machine block and an
 * explicit "unavailable" pressure all resolve to `unknown` and all stay
 * VISIBLE. This fleet has produced the opposite defect repeatedly — a release
 * manifest miss reported as "current", a dev dashboard pill where a null count
 * rendered green "all live" — and each time the shape was the same: absence
 * rendering as good news. Loading is the one silent case, because a badge that
 * flashes "unknown" on every page load is noise rather than a signal.
 */
export type MachinePressureLevel = "advisory" | "critical" | "unknown";

export type MachinePressureNotice = {
  level: MachinePressureLevel;
  /** Visible text. The signal must not depend on colour, so it is never only a hue. */
  label: string;
  /** The numbers behind the label, for the tooltip and the accessible description. */
  detail: string;
};

/**
 * How the header's copy of the resource read is doing.
 *
 * `failed` is deliberately distinct from `loading`. Collapsing them is how a
 * dead read becomes indistinguishable from a slow one, and a dead read must
 * reach the operator.
 */
export type MachineResourceState =
  | { kind: "loading" }
  | { kind: "failed" }
  | { kind: "ready"; resources: RuntimeResources };

function percent(value: number | null | undefined): string | null {
  if (value === null || value === undefined || !Number.isFinite(value)) return null;
  return `${Math.round(value)}%`;
}

/** The numbers worth carrying, in the order an operator would ask for them. */
function machineDetail(resources: RuntimeResources): string {
  const machine = resources.machine;
  if (!machine) return "The machine's own memory and load could not be read.";
  const parts: string[] = [];
  const memory = percent(machine.memory_used_percent);
  if (memory) parts.push(`memory ${memory} used`);
  const swap = percent(machine.swap_used_percent);
  if (swap) parts.push(`swap ${swap} used`);
  if (machine.memory_pressure_avg10 !== null && Number.isFinite(machine.memory_pressure_avg10)) {
    parts.push(`memory stall ${machine.memory_pressure_avg10.toFixed(1)}%`);
  }
  if (machine.load_average) parts.push(`load ${machine.load_average[0].toFixed(2)}`);
  if (parts.length === 0) return "The machine's own memory and load could not be read.";
  return `${parts.join(", ")}.`;
}

/**
 * The header notice, or null when there is nothing to say.
 *
 * Returning null for Normal is what keeps the header quiet enough to be worth
 * reading; see the note above.
 */
export function machinePressureNotice(state: MachineResourceState): MachinePressureNotice | null {
  if (state.kind === "loading") return null;
  if (state.kind === "failed") {
    return {
      level: "unknown",
      label: "Machine unknown",
      detail: "Swarm could not read this machine's resources, so it cannot warn you about pressure.",
    };
  }
  const machine = state.resources.machine;
  if (!machine || machine.pressure === "unavailable") {
    return {
      level: "unknown",
      label: "Machine unknown",
      detail: machine
        ? machineDetail(state.resources)
        : "Swarm could not read this machine's resources, so it cannot warn you about pressure.",
    };
  }
  if (machine.pressure === "critical") {
    return { level: "critical", label: "Machine critical", detail: machineDetail(state.resources) };
  }
  if (machine.pressure === "advisory") {
    return { level: "advisory", label: "Machine under load", detail: machineDetail(state.resources) };
  }
  return null;
}
