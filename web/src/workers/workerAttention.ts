import type { Worker } from "../api";
import type { BeeExpression } from "../brand/BeeMascot";

export type WorkerAttentionPresentation = {
  state: Worker["attention_state"];
  label: string;
  compactLabel: string;
  expression: BeeExpression;
  presence: "offline" | "online" | "engaged" | "waiting" | "blocked";
};

const presentation: Record<Worker["attention_state"], Omit<WorkerAttentionPresentation, "state">> = {
  sleeping: { label: "Sleeping", compactLabel: "sleeping", expression: "sleeping", presence: "offline" },
  resting: { label: "Resting", compactLabel: "resting", expression: "available", presence: "online" },
  buzzing: { label: "Buzzing", compactLabel: "buzzing", expression: "thinking", presence: "online" },
  with_operator: { label: "With you", compactLabel: "with you", expression: "focused", presence: "engaged" },
  awaiting_operator: { label: "Awaiting you", compactLabel: "awaiting you", expression: "available", presence: "waiting" },
  blocked: { label: "Blocked", compactLabel: "blocked", expression: "blocked", presence: "blocked" },
};

export function workerAttention(worker: Worker, now = Date.now()): WorkerAttentionPresentation {
  const state = worker.attention_state === "with_operator"
    && worker.engagement_expires_at !== undefined
    && worker.engagement_expires_at * 1000 <= now
    ? "resting"
    : worker.attention_state;
  const shown = presentation[state];
  // Resting with something still running is not the same as resting with
  // nothing to do, and both used to read "Resting". The classifier is right to
  // call the turn over — treating the worker as busy stalled the whole Hive —
  // so this distinguishes the two without changing which one it is.
  if (state === "resting" && worker.background_work) {
    return {
      state,
      ...shown,
      label: "Resting · task running",
      compactLabel: "task running",
    };
  }
  return { state, ...shown };
}

/**
 * How long this worker's terminal has been silent, for the roster badge, so the
 * operator can spot a stalled worker without opening it.
 *
 * Absent while a worker is unloaded, before the terminal host reports the fact
 * at all, and for the first minute, where a number would only ever be noise.
 */
export function workerSilence(worker: Worker, now = Date.now()): string | undefined {
  if (!worker.running || worker.last_output_at === undefined) return undefined;
  const seconds = Math.floor(now / 1000) - worker.last_output_at;
  if (seconds < 60) return undefined;
  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) return `${minutes}m`;
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return `${hours}h`;
  return `${Math.floor(hours / 24)}d`;
}

/**
 * Says when a *different* device holds this worker's input and terminal width.
 *
 * Terminal geometry follows the engaged device, so a desktop rendering at phone
 * width is the rule working rather than a fault. Naming the owner is what makes
 * that difference visible; sending input claims it back.
 */
export function foreignEngagement(
  worker: Worker,
  thisDeviceId: string,
): { deviceClass: "desktop" | "mobile"; detail: string } | undefined {
  if (!worker.engaged_device_id || worker.engaged_device_id === thisDeviceId) return undefined;
  const deviceClass = worker.engaged_device_class ?? "desktop";
  const where = deviceClass === "mobile" ? "a phone" : "another desktop";
  return {
    deviceClass,
    detail: `${where} is driving this worker, so its terminal width follows that device. Type here to take over.`,
  };
}

/**
 * The one line under a worker's name in the switcher.
 *
 * WHY AN ACTIVE ASSIGNMENT OUTRANKS "Resting" HERE, and only here. The operator
 * photographed this row reading "Resting · An email task can comple…" and wrote
 * "it shows resting even though it's actively working". The assignment was
 * already on the line and they still read the row as idle, because the state
 * word came first and the title read as decoration.
 *
 * `Resting` is not wrong — it describes the PROMPT, which is genuinely idle
 * between one thing and the next. It just does not describe the TURN, and the
 * turn is what somebody scanning this list wants to know about.
 *
 * CLASSIFICATION IS UNTOUCHED. This changes a sentence, not a state: nothing
 * downstream — delivery, the coordinator, the stale-work flag — sees anything
 * different, and 0d3d920's rule that a resting prompt outranks a background
 * shell is exactly as it was. Only the switcher's own line reads the board.
 *
 * Narrow on purpose, so it cannot lie in the other direction:
 *   - only when the label would otherwise be exactly "Resting", so a worker
 *     waiting on the operator (AwaitingOperator, which outranks Resting) still
 *     reads "Awaiting you" and never "Working";
 *   - only for an ACTIVE assignment, because ready, blocked and draft work does
 *     not make a worker busy;
 *   - "Resting · task running" keeps its own label, which answers the different
 *     question of a turn that ENDED with something still running.
 *
 * The trade, stated rather than buried: this line now trusts the BOARD about
 * activity. If a worker's turn ends while its task is still Active, the row
 * says Working beside an idle terminal. That is the stale-owned-work case, it
 * has its own signal and its own handling, and papering over it here would hide
 * it rather than fix it.
 */
export function workerSwitcherDetail(
  worker: Worker,
  assignedTaskTitle?: string,
  assignedTaskIsActive = false,
): string {
  const attention = worker.running ? workerAttention(worker) : undefined;
  const resting = attention?.state === "resting" && attention.label === "Resting";
  const state = attention
    ? (resting && assignedTaskIsActive ? "Working" : attention.label)
    : "Sleeping";
  if (assignedTaskTitle) return `${state} · ${assignedTaskTitle}`;
  return worker.running ? state : "Sleeping · tap to wake";
}

/**
 * How long this worker has been holding for an operator answer.
 *
 * Distinct from silence: a held worker stopped producing output as a
 * consequence of stopping, so silence age happens to look right and measures
 * the wrong thing. What the operator needs to see is how long an answer has
 * been owed, which is when the request was filed.
 *
 * Shown from the first minute rather than suppressed like silence is. A
 * one-minute silence is noise; a worker pinned for a minute waiting on you is
 * already the fact.
 */
export function heldForAnswer(worker: Worker, now = Date.now()): string | undefined {
  if (worker.held_for_answer_since === undefined) return undefined;
  const seconds = Math.max(0, Math.floor(now / 1000) - worker.held_for_answer_since);
  const minutes = Math.floor(seconds / 60);
  if (minutes < 1) return "just now";
  if (minutes < 60) return `${minutes}m`;
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return `${hours}h`;
  return `${Math.floor(hours / 24)}d`;
}
