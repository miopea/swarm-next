import { describe, expect, it } from "vitest";
import type { DevelopmentRuntime, WorkerEngineAutoUpdateView } from "../api";
import { ENGINE_WAIT_ATTENTION_SECONDS, engineAutoUpdateStatus, engineUpdateAttention, refusalSentence } from "./engineAutoUpdate";

const on: WorkerEngineAutoUpdateView = { enabled: true, waiting_since: 1_000, refusal: null, last_update_workers: [] };

const runtime: DevelopmentRuntime = {
  enabled: false,
  version: "1.0.1",
  state: "disabled",
  reload_available: false,
  source_revision: null,
  source_dirty: false,
  deployed_source_published: true,
  worker_engine_update_required: true,
  worker_engine_auto_update: on,
};

describe("engineAutoUpdateStatus", () => {
  it("says it will happen by itself, and how to turn that off", () => {
    expect(engineAutoUpdateStatus(on, 2_000)).toBe(
      "This update applies by itself the moment every worker is resting. Turn that off here to apply it only with the button.",
    );
  });

  it("names the worker holding it up and for how long", () => {
    const sentence = engineAutoUpdateStatus(
      { ...on, refusal: { reason: "turn_in_progress", worker: "Pantry", since: 1_000 } },
      1_000 + 3 * 60,
    );
    expect(sentence).toBe("This update applies by itself the moment every worker is resting. Not yet: Pantry is mid-turn (for 3 minutes).");
  });

  it("says plainly when it is off, so the update waits for the operator", () => {
    expect(engineAutoUpdateStatus({ ...on, enabled: false }, 2_000)).toContain("Automatic engine updates are off");
    expect(engineAutoUpdateStatus(null, 2_000)).toBeNull();
  });

  it("words every reason the engine and the driver give", () => {
    expect(refusalSentence({ reason: "workers_busy", count: 2, since: 0 })).toBe("2 worker sessions are working");
    expect(refusalSentence({ reason: "unsent_input", worker: "Ledger", since: 0 })).toBe("Ledger has text typed and not sent");
    expect(refusalSentence({ reason: "operator_has_terminal", worker: "Ledger", since: 0 })).toBe("you have Ledger's terminal");
    expect(refusalSentence({ reason: "engine_cannot_check_workers", since: 0 })).toContain("apply this one with the button");
    expect(refusalSentence({ reason: "answer_lost", since: 0 })).toContain("comes back once the engine is updated");
    expect(refusalSentence({ reason: "session_without_worker", since: 0 })).toContain("no worker owns");
    expect(refusalSentence({ reason: "earlier_attempt_failed", since: 0 })).toContain("not tried again by itself");
    expect(refusalSentence({ reason: "something_newer", since: 0 })).toBe("it is waiting");
  });
});

describe("engineUpdateAttention", () => {
  it("is nothing while no update waits", () => {
    expect(engineUpdateAttention({ ...runtime, worker_engine_update_required: false }, 999_999)).toBeNull();
    expect(engineUpdateAttention(undefined, 999_999)).toBeNull();
  });

  it("waits a day before it is the operator's problem", () => {
    expect(engineUpdateAttention(runtime, 1_000 + ENGINE_WAIT_ATTENTION_SECONDS - 1)).toBeNull();
    expect(engineUpdateAttention({
      ...runtime,
      worker_engine_auto_update: { ...on, refusal: { reason: "background_work", worker: "Ledger", since: 1_000 } },
    }, 1_000 + ENGINE_WAIT_ATTENTION_SECONDS)).toEqual({
      kind: "waiting", hours: 24, reason: "Ledger has background work running", enabled: true,
    });
  });

  it("names the workers a partial stop left down, at once", () => {
    const attention = engineUpdateAttention({
      ...runtime,
      worker_engine_auto_update: {
        ...on,
        last_update_workers: [{ name: "Ledger", status: "returning" }, { name: "Spare", status: "back" }],
      },
      last_worker_engine_update: {
        id: "a", started_at: 1_500, from_version: "1.0.0", to_version: "1.0.1", stopped_sessions: 1,
        initiated: "automatic", outcome: "failed", detail: "Stopped Ledger while resting, then could not stop Pantry.",
      },
    }, 1_600);
    expect(attention).toEqual({
      kind: "stopped_partway",
      detail: "Stopped Ledger while resting, then could not stop Pantry.",
      stopped: ["Ledger"],
    });
  });
});
