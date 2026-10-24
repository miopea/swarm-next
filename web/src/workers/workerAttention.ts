import type { Worker } from "../api";
import type { BeeExpression } from "../brand/BeeMascot";

export type WorkerAttentionPresentation = {
  state: Worker["attention_state"];
  label: string;
  compactLabel: string;
  expression: BeeExpression;
  presence: "offline" | "online" | "engaged" | "waiting" | "blocked";
  /**
   * What a finished turn left running, as its own line: "2 shells running".
   * The roster's pill shares a row with the worker's name, and the whole label
   * in it cut the name to "Or…" in a 280-pixel rail.
   */
  backgroundWork?: string;
  /**
   * The pill's word when a line under the name carries the rest of the label,
   * so "Buzzing · 2 shells running" reads BUZZING over "2 shells running".
   */
  word?: string;
  /**
   * What Claude itself reported that needs a look, as its own line: why a turn
   * stopped, what auto mode refused, tools that dropped (ADR 0123).
   */
  notice?: string;
};

const TURN_FAILURE: Record<NonNullable<Worker["turn_failure"]>, string> = {
  rate_limit: "rate limit",
  overloaded: "API overloaded",
  authentication: "login refused",
  billing: "account or billing",
  server: "API error",
  other: "API error",
};

/** "Auto mode blocked Bash (Irreversible Local Destruction)", "Swarm tools disconnected". */
function reportedNotices(worker: Worker): string[] {
  const notices: string[] = [];
  if (worker.auto_mode_denial) {
    const reason = worker.auto_mode_denial.reason.replace(/^\[(.*)\]$/, "$1").trim();
    notices.push(`Auto mode blocked ${worker.auto_mode_denial.tool}${reason ? ` (${reason})` : ""}`);
  }
  if (worker.tools_disconnected) notices.push("Swarm tools disconnected");
  return notices;
}

const presentation: Record<Worker["attention_state"], Omit<WorkerAttentionPresentation, "state">> = {
  sleeping: { label: "Sleeping", compactLabel: "sleeping", expression: "sleeping", presence: "offline" },
  resting: { label: "Resting", compactLabel: "resting", expression: "available", presence: "online" },
  buzzing: { label: "Buzzing", compactLabel: "buzzing", expression: "thinking", presence: "online" },
  with_operator: { label: "With you", compactLabel: "with you", expression: "focused", presence: "engaged" },
  awaiting_operator: { label: "Awaiting you", compactLabel: "awaiting you", expression: "available", presence: "waiting" },
  blocked: { label: "Blocked", compactLabel: "blocked", expression: "blocked", presence: "blocked" },
};

export function workerAttention(worker: Worker, now = Date.now()): WorkerAttentionPresentation {
  const shown = workerStatePresentation(worker, now);
  const notices = worker.running ? reportedNotices(worker) : [];
  // A failed turn that is not read as Blocked, such as a usage limit Claude
  // will not resume (Awaiting you), still says what stopped it.
  if (worker.running && worker.turn_failure && shown.notice === undefined) {
    notices.unshift(`Turn stopped: ${TURN_FAILURE[worker.turn_failure]}`);
  }
  if (notices.length === 0) return shown;
  return { ...shown, notice: [shown.notice, ...notices].filter(Boolean).join(" · ") };
}

function workerStatePresentation(worker: Worker, now: number): WorkerAttentionPresentation {
  const state = worker.attention_state === "with_operator"
    && worker.engagement_expires_at !== undefined
    && worker.engagement_expires_at * 1000 <= now
    ? "resting"
    : worker.attention_state;
  const shown = presentation[state];
  // A turn that died on an API error left a resting prompt that will not do
  // anything until the limit resets or someone fixes the login. Resting would
  // read as "free for work"; it is blocked, and the line says on what.
  if (worker.running && state === "resting" && worker.turn_failure) {
    const reason = TURN_FAILURE[worker.turn_failure];
    return {
      ...presentation.blocked,
      state: "blocked",
      label: `Blocked · ${reason}`,
      compactLabel: `blocked · ${reason}`,
      word: presentation.blocked.label,
      notice: `Turn stopped: ${reason}`,
    };
  }
  // The legacy held-for-answer field is the oldest pending decision timestamp,
  // not proof that the provider stopped. Queen can keep coordinating other work.
  if (state === "awaiting_operator" && worker.held_for_answer_since !== undefined) {
    if (worker.running && worker.provider_activity === "active") {
      return { state, ...shown, label: "Buzzing · decision pending", compactLabel: "buzzing · decision pending", expression: "thinking" };
    }
    return { state, ...shown, label: "Decision pending", compactLabel: "decision pending" };
  }
  // A WAKE IS COMING, AND SAYING SO IS THE WHOLE FIX. Two tasks were routed to
  // a sleeping worker; the coordinator woke it four and a half minutes later.
  // In between nothing said anything, so the operator concluded "the queen
  // didn't wake the worker" and filed that report fifteen seconds after it
  // woke. Queen had done nothing wrong and neither had they — the wake was
  // sitting in coordinator_actions the entire time and no surface showed it.
  //
  // Only for a worker that is NOT running: a wake for one already awake is
  // stale bookkeeping, not news.
  if (!worker.running && worker.waking_since !== undefined) {
    return { state, ...shown, label: "Waking…", compactLabel: "waking" };
  }
  // Resting with something still running is not the same as resting with
  // nothing to do, and both used to read "Resting". The classifier is right to
  // call the turn over — treating the worker as busy stalled the whole Hive —
  // so delivery still sees a resting prompt. What the operator sees is that it
  // is busy: "not really resting but buzzing" (2026-10-22).
  if (state === "resting" && worker.background_work) {
    const counted = backgroundWorkCount(worker);
    const running = counted ?? "task running";
    return {
      ...presentation.buzzing,
      state: "buzzing",
      label: `Buzzing · ${running}`,
      compactLabel: `buzzing · ${running}`,
      word: presentation.buzzing.label,
      backgroundWork: counted ? `${counted} running` : running,
    };
  }
  return { state, ...shown };
}

/** "2 shells" or "1 shell · 1 monitor"; undefined when nothing is counted. */
function backgroundWorkCount(worker: Worker): string | undefined {
  const named: string[] = [];
  for (const [count, noun] of [[worker.background_shells, "shell"], [worker.background_monitors, "monitor"]] as const) {
    if (count !== undefined && count > 0) named.push(`${count} ${noun}${count === 1 ? "" : "s"}`);
  }
  return named.length > 0 ? named.join(" · ") : undefined;
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
 * An Active task is an assignment, not proof of execution. Keep that task
 * prominent without relabeling a resting provider as Working. Stale-work
 * recovery remains owned by the coordinator and Queen, not this sentence.
 */
export function workerSwitcherDetail(
  worker: Worker,
  assignedTaskTitle?: string,
  assignedTaskIsActive = false,
): string {
  const { state, rest } = workerSwitcherLine(worker, assignedTaskTitle, assignedTaskIsActive);
  return rest ? `${state} · ${rest}` : state;
}

/**
 * The same line, split where the phone needs to draw it differently.
 *
 * WHY THE PARTS ARE EXPOSED AT ALL. The rail gives worker state a dot whose hue
 * and fill say which state it is, and a filled pill for the states worth
 * reading. The phone's picker had neither — its dot had two values, running and
 * not — so the state survived only as the first word of this sentence. The
 * operator read that on a phone on 2026-09-13: "You have to look specifically
 * at the word buzzing or idle or resting or whatnot. Visually it should be
 * distinct."
 *
 * So the word becomes the pill and the rest stays a sentence. Nothing new is
 * said and nothing is dropped — `workerSwitcherDetail` still joins these two
 * back into exactly the string it always returned, which is what the picker's
 * accessible name and every existing test read.
 */
export function workerSwitcherLine(
  worker: Worker,
  assignedTaskTitle?: string,
  assignedTaskIsActive = false,
): { key: Worker["attention_state"]; presence: WorkerAttentionPresentation["presence"]; state: string; rest?: string } {
  const attention = worker.running ? workerAttention(worker) : undefined;
  const resting = attention?.state === "resting" && attention.label === "Resting";
  // The phone draws the state as a pill too, so what is running, and what
  // Claude reported, join the sentence instead, as on the roster's own lines.
  const running = [attention?.backgroundWork, attention?.notice].filter(Boolean).join(" · ") || undefined;
  const state = attention
    ? (resting && assignedTaskIsActive ? "Active assignment" : attention.word ?? attention.label)
    : "Sleeping";
  // ⚠️ THE KEY FOLLOWS THE WORD, and that is the point of returning it rather
  // than letting the caller reach for `workerAttention` itself. This line calls
  // an unloaded worker "Sleeping" whatever its last reported attention state
  // was, so a worker recorded as resting-but-not-running — which the board does
  // produce — drew a SLEEPING pill beside a resting dot the first time both
  // were on screen together. Two channels saying different things is worse than
  // the one channel this replaced.
  const key = worker.running ? attention?.state ?? "resting" : "sleeping";
  const presence = worker.running ? attention?.presence ?? "online" : "offline";
  const rest = running && assignedTaskTitle ? `${running} · ${assignedTaskTitle}` : running ?? assignedTaskTitle;
  if (rest) return { key, presence, state, rest };
  return worker.running ? { key, presence, state } : { key, presence, state: "Sleeping", rest: "tap to wake" };
}

/**
 * Age of this worker's oldest pending operator decision.
 *
 * The legacy field name does not prove that the terminal stopped: a worker may
 * continue other work while its decision remains pending. Never use this age
 * as terminal silence or as evidence that orchestration cannot progress.
 *
 * Recent requests show "just now"; older ones show their age independently of
 * terminal output.
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
