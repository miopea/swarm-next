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
