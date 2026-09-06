import type { StorageObservation } from "../api";
import type { MachinePressureNotice } from "./machinePressure";

export function storageLabel(value: StorageObservation): string {
  const role = { system: "System storage", temporary: "Temporary storage", database: "Hive storage" }[value.scope];
  return `${role}: ${storageValue(value)}`;
}

export function storageValue(value: StorageObservation): string {
  const free = value.available_bytes;
  if (free === null || !Number.isFinite(free) || free < 0 || value.pressure === "unavailable") return "unavailable";
  return `${(free / 1024 ** 3).toFixed(1)} GiB available`;
}

export function storagePressureNotice(values: StorageObservation[] | undefined): MachinePressureNotice | null {
  // An older API has no storage check; Diagnostics shows that gap explicitly.
  if (!values) return null;
  const affected = values.filter((value) => value.pressure !== "normal");
  if (!affected.length) return null;
  const level = affected.some((value) => value.pressure === "critical") ? "critical"
    : affected.some((value) => value.pressure === "advisory") ? "advisory" : "unknown";
  return {
    level,
    label: level === "critical" ? "Storage critically low" : level === "advisory" ? "Storage running low" : "Storage unknown",
    detail: `${affected.map(storageLabel).join("; ")}. Checks may share a filesystem; do not add their capacity. Observation only; no files are removed.`,
  };
}
