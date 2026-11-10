import type { AutoInstallHold, HiveCondition } from "../api";
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
    // Only a member that says so swaps by itself (ADR 0167). One on 1.17.10, or
    // with an engine older than protocol 18, waits for its operator's press.
    case "engine_update_pending": return {
      status: "Engine update pending",
      next: condition.applies_itself
        ? "It swaps by itself once its workers are idle"
        : "Its operator applies it from the Worker engine card",
    };
    // Shown only beside "behind", so it answers why that Hive has not caught up
    // and who moves it (ADR 0167).
    case "release_held": return releaseHeldText(condition.hold);
  }
}

function releaseHeldText(hold: AutoInstallHold): { status: string; next: string } {
  const fromUpdatesCard = "Its operator installs it from the Updates card";
  const askForInstall = "Open its logs, or ask its operator to press Install on the Updates card";
  switch (hold) {
    case "turned_off": return { status: "Release waiting: automatic install is off", next: fromUpdatesCard };
    case "development_build": return { status: "Runs a development build", next: "It never installs a release by itself; its operator reloads it" };
    case "nothing_newer": return { status: "Hasn't seen the release yet", next: "It finds it at its next check; its operator can check now from the Updates card" };
    case "in_use": return { status: "Release waiting for idle workers", next: "It installs by itself once its workers are idle or its operator is away" };
    case "in_flight": return { status: "Installing the release", next: "It finishes by itself" };
    case "already_attempted": return { status: "Automatic install didn't finish", next: askForInstall };
    case "protocol_change": return { status: "Release would end worker sessions", next: fromUpdatesCard };
    case "protocol_unknown": return { status: "Can't tell if the release ends worker sessions", next: fromUpdatesCard };
    case "download_failed": return { status: "Release download failing", next: "It tries again by itself; open its logs if it keeps failing" };
    case "install_refused": return { status: "Install couldn't start", next: askForInstall };
    // A release that swaps the engine prepares itself and swaps once every
    // worker rests (ADR 0169).
    case "swap_waits_for_rest": return {
      status: "Release waits for its workers to rest",
      next: "It swaps the engine by itself once every worker rests, restarting each of them",
    };
    case "swap_attempted": return {
      status: "Release swap stopped its workers and didn't finish",
      next: "It is not tried again by itself; its operator applies it from the Worker engine card",
    };
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
