import { expect, test } from "vitest";
import type { AutoInstallHold } from "../api";
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

// A member behind its Apiary's release said nothing about why, so the roster
// could not tell busy workers from a release waiting on its operator's press.
// The member now reports where its automatic install stopped (ADR 0167).
test("a held release says why it is held and who moves it", () => {
  expect(conditionText({ kind: "release_held", hold: "protocol_change" }, 0)).toEqual({
    status: "Release would end worker sessions",
    next: "Its operator installs it from the Updates card",
  });
  expect(conditionText({ kind: "release_held", hold: "in_use" }, 0)).toEqual({
    status: "Release waiting for idle workers",
    next: "It installs by itself once its workers are idle or its operator is away",
  });
  expect(conditionText({ kind: "release_held", hold: "download_failed" }, 0)).toEqual({
    status: "Release download failing",
    next: "It tries again by itself; open its logs if it keeps failing",
  });
});

test("every hold reads as its own status with a next step", () => {
  const holds: AutoInstallHold[] = [
    "turned_off", "development_build", "nothing_newer", "in_use", "in_flight",
    "already_attempted", "protocol_change", "protocol_unknown", "download_failed", "install_refused",
  ];
  const statuses = holds.map((hold) => {
    const text = conditionText({ kind: "release_held", hold }, 0);
    expect(text.status, hold).not.toBe("");
    expect(text.next, hold).not.toBe("");
    return text.status;
  });
  expect(new Set(statuses).size).toBe(holds.length);
});
