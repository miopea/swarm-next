import { expect, test } from "vitest";

import type { Worker } from "../api";
import { workerAttention, workerSilence, workerSwitcherDetail } from "./workerAttention";

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
