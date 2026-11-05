import { describe, expect, it } from "vitest";
import { engineUpdateHistory } from "./engineUpdateHistory";
import type { WorkerEngineUpdateAttempt } from "../api";

const base: WorkerEngineUpdateAttempt = {
  id: "01a0",
  started_at: 1_000,
  from_version: "1.8.1",
  to_version: "1.9.0",
  to_protocol: null,
  stopped_sessions: 3,
  outcome: "succeeded",
  detail: "",
  finished_at: 1_100,
};

describe("engineUpdateHistory", () => {
  it("says nothing when this Hive has never attempted one", () => {
    // Absent is also what a store that could not be read looks like. Inventing
    // a reassuring sentence for either would be worse than a quiet card.
    expect(engineUpdateHistory(undefined, 2_000)).toBeNull();
    expect(engineUpdateHistory(null, 2_000)).toBeNull();
  });

  it("reports an attempt with no ending as unknown, never as success", () => {
    // ⚠️ THE CASE THE WHOLE RECORD EXISTS FOR. A protocol migration replaces
    // the API, so nothing is left to write the outcome.
    const sentence = engineUpdateHistory(
      { ...base, outcome: null, finished_at: null, to_protocol: 18 },
      1_600,
    );
    expect(sentence).toContain("never recorded how it ended");
    expect(sentence).toContain("not confirmation that it worked");
    expect(sentence).not.toContain("succeeded");
  });

  it("names what a timeout costs, because those workers are still owed", () => {
    const sentence = engineUpdateHistory({ ...base, outcome: "timed_out" }, 4_600);
    expect(sentence).toContain("owed a return");
    expect(sentence).toContain("stopped 3 worker sessions");
  });

  it("gives a failure its own reason rather than a generic one", () => {
    const sentence = engineUpdateHistory(
      { ...base, outcome: "failed", detail: "the request file could not be written" },
      1_600,
    );
    expect(sentence).toContain("the request file could not be written");
  });

  it("still explains a failure that recorded no reason", () => {
    const sentence = engineUpdateHistory({ ...base, outcome: "failed", detail: "" }, 1_600);
    expect(sentence).toContain("no reason was recorded");
  });

  it("counts one session in the singular and none plainly", () => {
    expect(engineUpdateHistory({ ...base, stopped_sessions: 1 }, 1_600))
      .toContain("stopped 1 worker session.");
    expect(engineUpdateHistory({ ...base, stopped_sessions: 0 }, 1_600))
      .toContain("stopped no worker sessions");
  });

  it("calls a protocol migration what it is", () => {
    expect(engineUpdateHistory({ ...base, to_protocol: 18 }, 1_600))
      .toContain("cannot preserve running terminals");
    expect(engineUpdateHistory(base, 1_600))
      .not.toContain("protocol migration");
  });

  it("does not render a negative age as time having passed", () => {
    // A clock that moved, or a database copied from a machine ahead of this
    // one. "in -3 minutes" reads as a bug in Swarm rather than in the clock.
    expect(engineUpdateHistory(base, 500)).toContain("stamped in the future");
  });
});

describe("engineUpdateHistory, for updates nobody asked for", () => {
  const automatic: WorkerEngineUpdateAttempt = {
    ...base,
    initiated: "automatic",
    stopped_sessions: null,
    detail: "Observed by Swarm",
  };

  it("says Swarm did it on its own, first thing", () => {
    // ⚠️ WHAT THE OPERATOR RULED ON. Most engine swaps are this one, and the
    // sentence used to read as a record of something they had done.
    const sentence = engineUpdateHistory(automatic, 1_600);
    expect(sentence).toContain("Swarm replaced the worker engine on its own");
    expect(sentence).toContain("Nobody was asked");
    expect(sentence).toContain("whenever no worker reports being mid-turn");
  });

  it("does not say it cost nothing when nobody counted", () => {
    // 0 and "unknown" are different answers, and only one of them is reassuring.
    const sentence = engineUpdateHistory(automatic, 1_600);
    expect(sentence).toContain("unrecorded number of worker sessions");
    expect(sentence).not.toContain("stopped no worker sessions");
  });

  it("still names a protocol migration it made on its own", () => {
    expect(engineUpdateHistory({ ...automatic, to_protocol: 18 }, 1_600))
      .toContain("cannot preserve running terminals");
  });

  it("leaves an update the operator asked for reading as theirs", () => {
    const sentence = engineUpdateHistory({ ...base, initiated: "operator" }, 1_600);
    expect(sentence).toContain("The last worker engine update");
    expect(sentence).not.toContain("on its own");
  });
});

describe("engineUpdateHistory, for the update Swarm applies once every worker rests", () => {
  const counted: WorkerEngineUpdateAttempt = { ...base, initiated: "automatic", stopped_sessions: 2 };

  it("names who it stopped and says whether each is back", () => {
    // ADR 0156: the notice names the workers and the engine version.
    expect(engineUpdateHistory(counted, 1_600, [
      { name: "Ledger", status: "back" }, { name: "Pantry", status: "back" },
    ])).toBe("Swarm updated the worker engine by itself 10 minutes ago, moving from 1.8.1 to 1.9.0, while every worker was resting. It stopped Ledger and Pantry and brought them back.");
    const pending = engineUpdateHistory(counted, 1_600, [
      { name: "Ledger", status: "back" }, { name: "Pantry", status: "returning" }, { name: "Spare", status: "needs_you" },
    ]);
    expect(pending).toContain("It stopped Ledger, Pantry and Spare.");
    expect(pending).toContain("Pantry is still coming back.");
    expect(pending).toContain("Spare was not brought back and is in Needs you.");
  });

  it("says why it stopped when it stopped partway", () => {
    const sentence = engineUpdateHistory(
      { ...counted, outcome: "failed", detail: "Stopped Ledger while resting, then could not stop Pantry." },
      1_600,
    );
    expect(sentence).toContain("Swarm tried to update the worker engine by itself");
    expect(sentence).toContain("could not stop Pantry");
  });

  it("still counts when the names could not be read", () => {
    expect(engineUpdateHistory(counted, 1_600)).toContain("It stopped 2 worker sessions.");
  });
});
