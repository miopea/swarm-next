import { useCallback, useState } from "react";
import { fetchTaskCi, type IndependentCheck, type Task, type TaskCi } from "../api";
import { useVisiblePolling } from "../runtime/useVisiblePolling";

function short(shas: string[]) {
  return shas.map((sha) => sha.slice(0, 8)).join(", ");
}

type Tone = "good" | "bad" | "waiting" | "unchecked";

const TONE_WORDS = { good: "Passed", bad: "Failed", waiting: "Waiting", unchecked: "Not checked" } as const;

/** What the check gate shows for a task's CI, in the operator's words. */
export function ciSummary(ci: TaskCi): { tone: Tone; text: string } | undefined {
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

const VERDICTS = {
  pass: { tone: "good", word: "Passed" },
  partial: { tone: "waiting", word: "Partial" },
  fail: { tone: "bad", word: "Failed" },
} as const;

function sentence(text: string) {
  const trimmed = text.trim();
  return /[.!?]$/.test(trimmed) ? trimmed : `${trimmed}.`;
}

/**
 * The worker's independent check of the work (ADR 0171), in the operator's
 * words. Nothing when the task built no code. It is shown, never enforced, so
 * a missing or stale check reads as a fact rather than a fault.
 */
export function checkSummary(check: IndependentCheck | null): { tone: Tone; word: string; text: string } | undefined {
  if (!check) return undefined;
  switch (check.state) {
    case "not_checked":
      return { tone: "unchecked", word: "Not checked", text: "No independent check is recorded for these commits." };
    case "checked": {
      const { tone, word } = VERDICTS[check.verdict];
      const findings = check.findings.trim() ? ` Findings: ${sentence(check.findings)}` : "";
      return { tone, word, text: `Independent check at ${check.head.slice(0, 8)}: ${check.verdict}. Ran: ${sentence(check.ran)}${findings}` };
    }
    case "stale": {
      const count = check.since.length;
      return {
        tone: "unchecked",
        word: "Stale",
        text: `Independent check at ${check.head.slice(0, 8)} (${check.verdict}) does not cover ${count === 1 ? "a later commit" : `${count} later commits`}: ${short(check.since)}.`,
      };
    }
  }
}

/**
 * CI and the worker's independent check for work waiting on review or release
 * (ADRs 0115 and 0171), as rows of the task's details. Read while the task
 * has recorded commits, and again each minute while it waits, because the
 * Hive reads GitHub on its own schedule and the worker may record a check.
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
  const watching = ci === undefined || ci.recorded_commits > 0;
  useVisiblePolling(load, watching, 60_000);
  const summary = ci && ciSummary(ci);
  const check = ci && checkSummary(ci.independent_check);
  if (!summary && !check) return null;
  // One word fits the details grid; the sentence is the tooltip and is read out.
  return (
    <>
      {summary && <div><dt>CI</dt><dd><span className={`task-ci is-${summary.tone}`} title={summary.text}>{TONE_WORDS[summary.tone]}<span className="sr-only">: {summary.text}</span></span></dd></div>}
      {check && <div><dt>Check</dt><dd><span className={`task-ci is-${check.tone}`} title={check.text}>{check.word}<span className="sr-only">: {check.text}</span></span></dd></div>}
    </>
  );
}
