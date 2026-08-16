import { expect, test } from "vitest";

import type { QueenAutomationStatus } from "../api";
import { queenAutomationNeedsAttention } from "./queenAutomationPresentation";

const idle: QueenAutomationStatus = {
  enabled: false,
  state: "idle",
  run_id: null,
  trigger: null,
  actionable_count: 0,
  attempts: 0,
  requested_at: null,
  delivered_at: null,
  finished_at: null,
  outcome: null,
  waiting_reason: null,
};

test("only treats interrupted or operator-blocked Queen reviews as attention", () => {
  expect(queenAutomationNeedsAttention(undefined)).toBe(false);
  expect(queenAutomationNeedsAttention(idle)).toBe(false);
  expect(queenAutomationNeedsAttention({ ...idle, state: "running" })).toBe(false);
  expect(queenAutomationNeedsAttention({ ...idle, state: "completed", outcome: "completed" })).toBe(false);
  expect(queenAutomationNeedsAttention({ ...idle, state: "completed", outcome: "no_action" })).toBe(false);
  expect(queenAutomationNeedsAttention({ ...idle, state: "uncertain" })).toBe(true);
  expect(queenAutomationNeedsAttention({ ...idle, state: "completed", outcome: "needs_operator" })).toBe(true);
});
