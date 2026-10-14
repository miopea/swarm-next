import type { HiveCondition } from "../api";
import { WORKER_MODEL_LABELS } from "../settings/workerModelLabels";

/**
 * What each condition on a member Hive reads as, and what to do about it
 * (ADR 0114).
 *
 * ⚠️ EVERY ONE NAMES ITS NEXT STEP. The operator's rule: no "data that we
 * wouldn't act upon". A condition with nothing to do says so ("it does this by
 * itself"), so reading it is never a puzzle about whether to act.
 */
export function conditionText(condition: HiveCondition, nowSeconds: number): { status: string; next: string } {
  switch (condition.kind) {
    case "quiet": return { status: "Quiet", next: "It hasn't reached the Keeper in ten minutes" };
    case "failing_sync": return {
      status: condition.step ? `Sync failing at ${condition.step}` : "Sync failing",
      next: condition.sustained
        ? `${condition.last_success_at ? `Last worked ${ago(condition.last_success_at, nowSeconds)}` : "It hasn't worked yet"}; open its logs to see why`
        : "It is retrying by itself",
    };
    case "schema_behind": return { status: `Schema behind (${condition.running})`, next: "Update it to take shared work" };
    case "behind": return { status: `Behind (${condition.running})`, next: "Update it" };
    case "engine_update_failing": return { status: "Engine update failing", next: "Its last swap didn't finish; open its logs, or ask its operator to apply it" };
    case "low_disk": return {
      status: condition.lowest_available_bytes === null ? "Low disk" : `Low disk: ${formatBytes(condition.lowest_available_bytes)} free`,
      next: condition.critical ? "About to run out; ask its operator to free space" : "Ask its operator to free space",
    };
    case "short_of_memory": return { status: "Short of memory", next: "Ask its operator what is using it" };
    case "workers_stuck": return {
      status: `${condition.count} ${condition.count === 1 ? "worker" : "workers"} stuck`,
      next: "Watch it to see where they stopped",
    };
    case "ci_unreadable": return { status: `Can't read CI: ${condition.why}`, next: "Work it closes goes unchecked; ask its operator to log gh in" };
    case "workers_off_model": return {
      status: `${condition.count} ${condition.count === 1 ? "worker" : "workers"} not yet on ${WORKER_MODEL_LABELS[condition.model]}`,
      next: "Each switches when it next starts; its operator can restart them sooner",
    };
    case "engine_update_pending": return { status: "Engine update pending", next: "It swaps by itself once its workers are idle" };
  }
}

/**
 * Said already by the roster's version and contact line, so the health line
 * leaves them out rather than saying them twice.
 */
export function shownOnHealthLine(condition: HiveCondition): boolean {
  return condition.kind !== "quiet" && condition.kind !== "behind" && condition.kind !== "schema_behind";
}

function ago(at: number, nowSeconds: number): string {
  const minutes = Math.max(0, Math.round((nowSeconds - at) / 60));
  if (minutes < 60) return `${minutes}m ago`;
  const hours = Math.round(minutes / 60);
  return hours < 48 ? `${hours}h ago` : `${Math.round(hours / 24)}d ago`;
}

function formatBytes(bytes: number): string {
  if (bytes < 1024 * 1024 * 1024) return `${Math.round(bytes / (1024 * 1024))} MiB`;
  return `${(bytes / (1024 * 1024 * 1024)).toFixed(1)} GiB`;
}
