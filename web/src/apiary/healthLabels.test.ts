import { expect, test } from "vitest";
import { conditionText } from "./healthLabels";

test("workers not yet on the Hive's model name the model and when they switch", () => {
  expect(conditionText({ kind: "workers_off_model", count: 1, model: "sonnet" }, 0)).toEqual({
    status: "1 worker not yet on Sonnet",
    next: "Each switches when it next starts; its operator can restart them sooner",
  });
  expect(conditionText({ kind: "workers_off_model", count: 3, model: "haiku" }, 0).status).toBe("3 workers not yet on Haiku");
});

// A member on 1.17.10 has no idle engine update, and an engine older than
// protocol 18 cannot check its workers, so "swaps by itself" was false there.
// It is said only when the member reports it can (ADR 0167).
test("a pending engine update says it swaps by itself only when the Hive says it can", () => {
  expect(conditionText({ kind: "engine_update_pending", applies_itself: true }, 0)).toEqual({
    status: "Engine update pending",
    next: "It swaps by itself once its workers are idle",
  });
  expect(conditionText({ kind: "engine_update_pending", applies_itself: false }, 0)).toEqual({
    status: "Engine update pending",
    next: "Its operator applies it from the Worker engine card",
  });
});
