import { expect, test } from "vitest";
import { TerminalApplicationEvidence } from "./TerminalApplicationEvidence";

test("retains paired phases and size from the slowest completed application", () => {
  const evidence = new TerminalApplicationEvidence(() => 100);
  evidence.record(1024, 150, 10);
  evidence.record(2048, 10, 140);
  expect(evidence.snapshot()).toEqual({ samples: 2, slowest: { at: 100, bytes: 1024, total_ms: 160, state_ms: 150, geometry_ms: 10 } });
  evidence.snapshot().slowest!.bytes = 9;
  expect(evidence.snapshot().slowest!.bytes).toBe(1024);
});

test("bounds samples and expires them without a timer", () => {
  let now = 100;
  const evidence = new TerminalApplicationEvidence(() => now);
  for (let i = 0; i < 201; i++) evidence.record(100, 201 - i, 0);
  expect(evidence.snapshot()).toMatchObject({ samples: 200, slowest: { state_ms: 200 } });
  now += 3_600_001;
  expect(evidence.snapshot()).toEqual({ samples: 0, slowest: null });
});

test("invalid sizes, durations and clocks never become healthy samples", () => {
  let now = 10;
  const evidence = new TerminalApplicationEvidence(() => now);
  for (const bytes of [-1, 0.5, 3 * 1024 * 1024 + 1, Infinity]) evidence.record(bytes, 10, 10);
  for (const duration of [-1, NaN, Infinity, 3_600_001]) {
    evidence.record(10, duration, 10);
    evidence.record(10, 10, duration);
  }
  now = NaN;
  evidence.record(10, 1, 1);
  expect(evidence.snapshot()).toEqual({ samples: 0, slowest: null });
  now = 10;
  evidence.record(10, 1, 1);
  now = 9;
  expect(evidence.snapshot()).toEqual({ samples: 0, slowest: null });
});
