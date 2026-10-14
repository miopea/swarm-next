import { useCallback, useState } from "react";
import { fetchTaskCi, type Task, type TaskCi } from "../api";
import { useVisiblePolling } from "../runtime/useVisiblePolling";

function short(shas: string[]) {
  return shas.map((sha) => sha.slice(0, 8)).join(", ");
}

const TONE_WORDS = { good: "Passed", bad: "Failed", waiting: "Waiting", unchecked: "Not checked" } as const;

/** What the check gate shows for a task's CI, in the operator's words. */
export function ciSummary(ci: TaskCi): { tone: "good" | "bad" | "waiting" | "unchecked"; text: string } | undefined {
  if (ci.level === "off" || ci.recorded_commits === 0) return undefined;
  const holds = ci.level === "require";
  const observation = ci.observation;
  if (!observation) return { tone: "waiting", text: holds ? "Not read yet. It can't close until it is." : "Not read yet." };
  const on = short(observation.judged);
  switch (observation.outcome) {
    case "passed": return { tone: "good", text: `Passed on ${on}.` };
    case "failed": return { tone: "bad", text: `Failed on ${on}: ${observation.failing.join(", ")}.${holds ? " It can't close until this passes." : ""}` };
    case "pending": return { tone: "waiting", text: `Waiting for ${observation.waiting.join(", ")} on ${on}.${holds ? " It can't close until CI passes." : ""}` };
    case "unreadable": return { tone: "unchecked", text: `Not checked: ${observation.why}.` };
  }
}

/**
 * CI for work waiting on review or release (ADR 0115), as a row of the task's
 * details. Read only while the
 * gate is on and the task recorded commits, and again each minute while it
 * waits, because the Hive reads GitHub on its own schedule.
 */
export default function TaskCiStatus({ task, operatorToken }: { task: Task; operatorToken: string }) {
  const [ci, setCi] = useState<TaskCi>();
  const load = useCallback(async (signal: AbortSignal) => {
    try {
      setCi(await fetchTaskCi(operatorToken, task.id, signal));
    } catch {
      // Keep what was shown; the next read replaces it.
    }
  }, [operatorToken, task.id]);
  const gated = ci === undefined || (ci.level !== "off" && ci.recorded_commits > 0);
  useVisiblePolling(load, gated, 60_000);
  const summary = ci && ciSummary(ci);
  if (!summary) return null;
  // One word fits the details grid; the sentence is the tooltip and is read out.
  return <div><dt>CI</dt><dd><span className={`task-ci is-${summary.tone}`} title={summary.text}>{TONE_WORDS[summary.tone]}<span className="sr-only">: {summary.text}</span></span></dd></div>;
}
