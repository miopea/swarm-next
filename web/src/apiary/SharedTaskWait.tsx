import type { ApiaryTask, ApiaryTaskWait } from "../api";

function backAt(wait: ApiaryTaskWait) {
  return wait.resume_at
    ? `, back ${new Date(wait.resume_at * 1000).toLocaleString(undefined, { dateStyle: "medium", timeStyle: "short" })}`
    : "";
}

/** Who must act and what must happen, as one line read after "Waiting for". */
export function sharedWaitLabel(wait: ApiaryTaskWait, localHiveId: string | undefined, hiveName: (hiveId: string) => string | undefined) {
  switch (wait.kind) {
    case "operator_action": {
      if (wait.hive_id && wait.hive_id === localHiveId) return `you: ${wait.needs}`;
      const hive = wait.hive_id ? hiveName(wait.hive_id) : undefined;
      return `${hive ? `${hive}'s operator` : "its Hive's operator"}: ${wait.needs}`;
    }
    case "outside":
      return `${wait.party ?? "an outside party"}: ${wait.needs}${backAt(wait)}`;
    case "date":
      return `a date${backAt(wait)}`;
    default:
      // Another task, a decision, or a kind from a later release: its words say it.
      return `${wait.needs || wait.kind.replaceAll("_", " ")}${backAt(wait)}`;
  }
}

/**
 * Why Blocked shared work waits (ADR 0119). Nothing for work that is not
 * Blocked. `silentRelease` names the home Hive's release when the Keeper knows
 * it is too old to say why, so a missing reason reads as that rather than as
 * nobody knowing.
 */
export default function SharedTaskWait({ task, localHiveId, hiveName, silentRelease }: {
  task: ApiaryTask;
  localHiveId?: string;
  hiveName: (hiveId: string) => string | undefined;
  silentRelease?: { hive: string; version: string };
}) {
  if (task.state !== "blocked") return null;
  if (task.wait) {
    return <small className="shared-task-wait"><span>Waiting for</span> {sharedWaitLabel(task.wait, localHiveId, hiveName)}</small>;
  }
  if (silentRelease) {
    return <small className="shared-task-wait">{silentRelease.hive} runs {silentRelease.version}, which does not say why work is blocked</small>;
  }
  return null;
}
