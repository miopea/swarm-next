import { expect, test } from "vitest";

import type { Worker } from "../api";
import { foreignEngagement, workerAttention, workerSilence, workerSwitcherDetail } from "./workerAttention";

const worker: Worker = {
  id: "worker", hive_id: "hive", name: "Daisy", role: "worker", provider: "claude_code",
  workspace: "/projects/daisy", autostart: false, position: 1, active_session_id: "session",
  running: true, attention_state: "buzzing", created_at: 1, updated_at: 1,
};

test("presents every worker state consistently", () => {
  expect(workerAttention(worker)).toMatchObject({ label: "Buzzing", compactLabel: "buzzing", expression: "thinking", presence: "online" });
  expect(workerAttention({ ...worker, attention_state: "awaiting_operator" })).toMatchObject({ label: "Awaiting you", presence: "waiting" });
  expect(workerAttention({ ...worker, attention_state: "blocked" })).toMatchObject({ label: "Blocked", expression: "blocked" });
  expect(workerAttention({ ...worker, running: false, attention_state: "sleeping" })).toMatchObject({ label: "Sleeping", presence: "offline" });
});

test("an expired operator lease presents as resting everywhere", () => {
  expect(workerAttention({ ...worker, attention_state: "with_operator", engagement_expires_at: 100 }, 100_000)).toMatchObject({
    state: "resting",
    label: "Resting",
    expression: "available",
  });
});

test("a pending decision does not claim the worker stopped at a terminal question", () => {
  const pending = { ...worker, attention_state: "awaiting_operator" as const, held_for_answer_since: 100 };
  expect(workerAttention(pending)).toMatchObject({
    state: "awaiting_operator", label: "Decision pending", compactLabel: "decision pending", presence: "waiting",
  });
  expect(workerSwitcherDetail(pending, "Review the release", true)).toBe("Decision pending · Review the release");
  // Removing the durable decision must not hide a provider-native question.
  expect(workerAttention({ ...pending, held_for_answer_since: undefined }).label).toBe("Awaiting you");
  for (const state of ["sleeping", "blocked", "with_operator"] as const) {
    expect(workerAttention({ ...pending, attention_state: state }).label).not.toBe("Decision pending");
  }
});

test("mobile worker details keep operational state visible before task context", () => {
  expect(workerSwitcherDetail({ ...worker, attention_state: "resting" }, "Review the release")).toBe("Resting · Review the release");
  expect(workerSwitcherDetail({ ...worker, running: false, attention_state: "sleeping" }, "Review the release")).toBe("Sleeping · Review the release");
  expect(workerSwitcherDetail({ ...worker, running: false, attention_state: "sleeping" })).toBe("Sleeping · tap to wake");
});

test("reports how long a loaded worker has been silent", () => {
  const now = 1_800_000_000_000;
  const silentFor = (seconds: number) => ({ ...worker, running: true, last_output_at: now / 1000 - seconds });

  expect(workerSilence(silentFor(30), now)).toBeUndefined();
  expect(workerSilence(silentFor(4 * 60), now)).toBe("4m");
  expect(workerSilence(silentFor(3 * 3600), now)).toBe("3h");
  expect(workerSilence(silentFor(2 * 86400), now)).toBe("2d");
});

test("says nothing about silence it cannot know", () => {
  const now = 1_800_000_000_000;

  // An unloaded worker has no terminal to have been silent.
  expect(workerSilence({ ...worker, running: false, last_output_at: now / 1000 - 9999 }, now)).toBeUndefined();
  // A terminal host that predates the field reports nothing rather than zero.
  expect(workerSilence({ ...worker, running: true }, now)).toBeUndefined();
});

test("names the other device holding a worker's terminal", () => {
  const engaged = { ...worker, engaged_device_id: "phone", engaged_device_class: "mobile" as const };

  expect(foreignEngagement(engaged, "desktop-1")?.deviceClass).toBe("mobile");
  expect(foreignEngagement(engaged, "desktop-1")?.detail).toContain("a phone");
  expect(foreignEngagement(engaged, "desktop-1")?.detail).toContain("take over");
});

test("stays quiet when this device already holds the terminal", () => {
  const mine = { ...worker, engaged_device_id: "desktop-1", engaged_device_class: "desktop" as const };

  expect(foreignEngagement(mine, "desktop-1")).toBeUndefined();
  // Nobody engaged is not the same as somebody else engaged.
  expect(foreignEngagement(worker, "desktop-1")).toBeUndefined();
});

test("tells a worker with nothing to do apart from one whose task is still running", () => {
  // The operator saw a roster of Resting workers and could not tell which had
  // finished and which had left something going. The classifier is right that
  // the turn is over — a resting prompt outranks a background shell, and
  // treating it as busy stalled the whole Hive — so this changes the label, not
  // the state.
  const idle = { ...worker, running: true, attention_state: "resting" as const };
  const stillRunning = { ...idle, background_work: true };

  expect(workerAttention(idle).label).toBe("Resting");
  expect(workerAttention(stillRunning).label).toBe("Resting · task running");
  // Same state, so nothing that routes on it sees a new case.
  expect(workerAttention(stillRunning).state).toBe("resting");
  expect(workerAttention(stillRunning).presence).toBe(workerAttention(idle).presence);
});

/**
 * A worker inside an active turn should not read like one with nothing to do.
 *
 * The operator's row said "Resting · An email task can comple…" while that
 * worker was mid-turn, and they wrote "it shows resting even though it's
 * actively working". The assignment was already on the line; the state word led
 * and the title read as decoration.
 *
 * Resting is not wrong — it describes the PROMPT, which really is idle between
 * one thing and the next. It just does not describe the TURN, and the turn is
 * what somebody scanning a list of workers is asking about.
 */
test("an active assignment leads the switcher line instead of the resting prompt", () => {
  const running = { ...worker, running: true, attention_state: "resting" as const };

  expect(workerSwitcherDetail(running, "Fix the importer", true)).toBe("Working · Fix the importer");

  // NOT for work that is merely assigned. Ready, blocked and draft do not make
  // a worker busy, and claiming they do would make the word meaningless.
  expect(workerSwitcherDetail(running, "Fix the importer", false)).toBe("Resting · Fix the importer");
  expect(workerSwitcherDetail(running, "Fix the importer")).toBe("Resting · Fix the importer");

  // AND NOT WHEN THE WORKER IS WAITING ON THE OPERATOR. That state outranks
  // resting, so it must survive untouched — telling somebody a worker is
  // Working when it is blocked on their answer is worse than the original bug.
  const waiting = { ...running, attention_state: "awaiting_operator" as const };
  expect(workerSwitcherDetail(waiting, "Fix the importer", true)).toBe("Awaiting you · Fix the importer");

  // AND THE TURN-ENDED-WITH-WORK-RUNNING LABEL KEEPS ITS OWN ANSWER, which is a
  // different question from this one.
  const leftRunning = { ...running, background_work: true };
  expect(workerSwitcherDetail(leftRunning, "Fix the importer", false))
    .toBe("Resting · task running · Fix the importer");

  // A sleeping worker is still sleeping, whatever the board believes.
  const asleep = { ...worker, running: false, attention_state: "sleeping" as const };
  expect(workerSwitcherDetail(asleep, "Fix the importer", true)).toBe("Sleeping · Fix the importer");
});

/**
 * Four and a half minutes of silence is what actually hurt.
 *
 * Two tasks were routed to a sleeping Voice Bridge worker at 20:40:57. The
 * coordinator woke it at 20:45:28. Nothing anywhere said a wake was coming, so
 * the operator watched a worker marked Sleeping, concluded "the queen didn't
 * wake the worker", and filed that report FIFTEEN SECONDS after it woke.
 *
 * Queen had done nothing wrong and neither had they. The wake sat in
 * coordinator_actions the whole time. The fact existed; no surface showed it.
 */
test("a worker with a wake in flight says so instead of just sleeping", () => {
  const asleep = { ...worker, running: false, attention_state: "sleeping" as const };
  const waking = { ...asleep, waking_since: 1_788_036_057 };

  expect(workerAttention(asleep).label).toBe("Sleeping");
  expect(workerAttention(waking).label).toBe("Waking…");

  // SAME STATE, deliberately. An asleep worker really is asleep; this is a fact
  // beside the state, like background_work. Rewriting what Sleeping means for
  // every consumer to carry one more piece of news is how the Resting collapse
  // happened, and delivery and coordination both route on this.
  expect(workerAttention(waking).state).toBe("sleeping");
  expect(workerAttention(waking).presence).toBe(workerAttention(asleep).presence);
});

/**
 * A wake recorded against a worker that is already running is stale
 * bookkeeping, not news — and saying "Waking…" over a live terminal would be
 * worse than saying nothing.
 */
test("a running worker is never described as waking", () => {
  const running = {
    ...worker, running: true, attention_state: "resting" as const,
    waking_since: 1_788_036_057,
  };

  expect(workerAttention(running).label).toBe("Resting");
});
