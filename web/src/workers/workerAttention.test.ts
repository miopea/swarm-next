import { expect, test } from "vitest";

import type { Worker } from "../api";
import { workerAttention, workerSwitcherDetail } from "./workerAttention";

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
