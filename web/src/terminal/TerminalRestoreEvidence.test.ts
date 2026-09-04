import { expect, test } from "vitest";
import { TerminalRestoreEvidence } from "./TerminalRestoreEvidence";

test("records nearest-rank p95 without hiding interrupted and failed attempts", () => {
  let now = 0;
  const evidence = new TerminalRestoreEvidence(() => now);
  for (let index = 1; index <= 20; index += 1) {
    const finish = evidence.begin();
    now += index;
    finish("rendered");
    finish("failed");
  }
  evidence.begin()("interrupted");
  evidence.begin()("failed");
  evidence.begin();
  expect(evidence.snapshot()).toEqual({ started: 23, pending: 1, interrupted: 1, failed: 1, samples: 20, p95_ms: 19, max_ms: 20 });
});

test("caps samples, expires old observations, and reports missing evidence as null", () => {
  let now = 0;
  const evidence = new TerminalRestoreEvidence(() => now);
  for (let index = 0; index < 250; index += 1) {
    const finish = evidence.begin();
    now += 2;
    finish("rendered");
  }
  expect(evidence.snapshot()).toMatchObject({ samples: 200, p95_ms: 2 });
  now += 60 * 60_000 + 1;
  expect(evidence.snapshot()).toMatchObject({ samples: 0, p95_ms: null, max_ms: null });
});

test("stopping retains results but invalidates pending callbacks; reset starts a new experiment", () => {
  let now = 0;
  const evidence = new TerminalRestoreEvidence(() => now);
  const done = evidence.begin();
  now = 100;
  done("rendered");
  const late = evidence.begin();
  evidence.stop();
  late("rendered");
  expect(evidence.snapshot()).toMatchObject({ samples: 1, pending: 0, interrupted: 1 });
  evidence.reset();
  late("failed");
  expect(evidence.snapshot()).toEqual({ started: 0, pending: 0, interrupted: 0, failed: 0, samples: 0, p95_ms: null, max_ms: null });
});

test("invalid clocks do not produce a healthy latency sample", () => {
  let now = 10;
  const evidence = new TerminalRestoreEvidence(() => now);
  const finish = evidence.begin();
  now = 5;
  finish("rendered");
  expect(evidence.snapshot()).toMatchObject({ failed: 1, samples: 0, pending: 0 });
});
