import { describe, expect, it } from "vitest";
import type { DevelopmentRuntime, WorkerEngineAutoUpdateView } from "../api";
import { ENGINE_WAIT_ATTENTION_SECONDS, clock, engineAutoUpdateStatus, engineUpdateAttention, refusalSentence } from "./engineAutoUpdate";

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
  it("says it applies within a minute when nobody holds it", () => {
    expect(engineAutoUpdateStatus(on)).toEqual({
      summary: "Every worker is resting, so this update applies within a minute.",
      holders: [],
      notes: [],
    });
  });

  // The operator, 2026-11-06: "it should be clear what is happening and if
  // not why". One list, from the roster and the engine both.
  it("lists each worker holding it, from the roster", () => {
    const status = engineAutoUpdateStatus(
      { ...on, refusal: { reason: "workers_busy", count: 2, since: 1_000 } },
      [{ name: "Ledger", background: false }, { name: "Pantry", background: true }],
    );
    expect(status).toEqual({
      summary: "This update applies by itself once every worker is resting. Waiting on:",
      holders: ["Ledger is working", "Pantry has a command running in the background"],
      notes: [],
    });
    expect(engineAutoUpdateStatus({ ...on, refusal: { reason: "workers_busy", count: 1, since: 1_000 } })?.holders)
      .toEqual(["1 worker session is working"]);
  });

  // ADR 0162: the engine names every worker it refused, and the card says
  // for each what Swarm does, or why it leaves it.
  it("lists every worker the engine named, with what Swarm does about each", () => {
    const status = engineAutoUpdateStatus({
      ...on,
      refusal: { reason: "turns_unaccounted", worker: "Larder", since: 1_000 },
      holders: [
        { worker: "Larder", reason: "turns_unaccounted", swarm: "restarts" },
        { worker: "Ledger", reason: "turn_in_progress", swarm: "in_use" },
        { worker: "Cellar", reason: "unsent_input", swarm: null },
        { worker: "Pantry", reason: "turn_in_progress", swarm: "not_resting" },
      ],
    }, [{ name: "Pantry", background: false }]);
    expect(status?.holders).toEqual([
      "Pantry is working",
      "The engine lost count of Larder's turns. Swarm restarts Larder at its next check, within a minute.",
      "The engine counts Ledger as mid-turn. You are using Ledger's terminal, so Swarm restarts it once you stop.",
      "Cellar has text typed and not sent",
    ]);
  });

  it("says what Swarm already did, and when that is the button's", () => {
    const status = engineAutoUpdateStatus({
      ...on,
      refusal: { reason: "turn_in_progress", worker: "Pantry", since: 1_000 },
      holders: [{ worker: "Pantry", reason: "turn_in_progress", swarm: "already_restarted" }],
      restarted: [{ worker: "Pantry", at: 1_200 }, { worker: "Larder", at: 1_200 }, { worker: "Ledger", at: 1_200 }],
    });
    expect(status?.holders).toEqual([
      "The engine counts Pantry as mid-turn. Swarm already restarted Pantry once for this update, so use the button.",
    ]);
    expect(status?.notes).toEqual([
      `Swarm restarted Pantry, Larder and Ledger at ${clock(1_200)}, because the engine could not confirm they were resting.`,
    ]);
    expect(engineAutoUpdateStatus({ ...on, restarted: [{ worker: "Pantry", at: 1_200 }] })?.notes).toEqual([
      `Swarm restarted Pantry at ${clock(1_200)}, because the engine could not confirm it was resting.`,
    ]);
  });

  it("names a reason that is nobody's in one sentence", () => {
    expect(engineAutoUpdateStatus({ ...on, refusal: { reason: "earlier_attempt_failed", since: 1_000 } })?.summary)
      .toContain("Not now: the last try at this update stopped workers");
  });

  it("says plainly when it is off, so the update waits for the operator", () => {
    expect(engineAutoUpdateStatus({ ...on, enabled: false })?.summary).toContain("Automatic engine updates are off");
    expect(engineAutoUpdateStatus(null)).toBeNull();
  });

  it("words every reason the engine and the driver give", () => {
    expect(refusalSentence({ reason: "workers_busy", count: 2, since: 0 })).toBe("2 worker sessions are working");
    expect(refusalSentence({ reason: "unsent_input", worker: "Ledger", since: 0 })).toBe("Ledger has text typed and not sent");
    expect(refusalSentence({ reason: "operator_has_terminal", worker: "Ledger", since: 0 })).toBe("you have Ledger's terminal");
    expect(refusalSentence({ reason: "turns_unaccounted", worker: "Ledger", since: 0 })).toBe("the engine lost count of Ledger's turns");
    expect(refusalSentence({ reason: "no_startup_evidence", worker: "Ledger", since: 0 }))
      .toBe("the engine never heard Ledger start, so it cannot count its turns");
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
