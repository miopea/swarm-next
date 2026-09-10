import { expect, test } from "vitest";
import { decisionCommandPresentation } from "./decisionCommandPresentation";

test.each([
  ["pending", "Attention", "Needs your answer"],
  ["resolved", "Decision history", "Answered"],
  ["withdrawn", "Decision history", "Withdrawn"],
] as const)("decision search identifies %s without losing searchable context", (state, group, label) => {
  expect(decisionCommandPresentation({ state, reason: "Original scope" })).toEqual({ group, detail: `${label} · Original scope` });
});

test("clarification waiting is not an operator action or an answer", () => {
  const clarification = { next_move: "requester" } as NonNullable<Parameters<typeof decisionCommandPresentation>[0]["clarification"]>;
  expect(decisionCommandPresentation({ state: "pending", reason: "", clarification })).toEqual({
    group: "Waiting for reply", detail: "Waiting for the requester · not answered",
  });
  expect(decisionCommandPresentation({ state: "resolved", reason: "", clarification }).group).toBe("Decision history");
});
