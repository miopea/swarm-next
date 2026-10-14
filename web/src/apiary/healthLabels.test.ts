import { expect, test } from "vitest";
import { conditionText } from "./healthLabels";

test("workers not yet on the Hive's model name the model and when they switch", () => {
  expect(conditionText({ kind: "workers_off_model", count: 1, model: "sonnet" }, 0)).toEqual({
    status: "1 worker not yet on Sonnet",
    next: "Each switches when it next starts; its operator can restart them sooner",
  });
  expect(conditionText({ kind: "workers_off_model", count: 3, model: "haiku" }, 0).status).toBe("3 workers not yet on Haiku");
});
