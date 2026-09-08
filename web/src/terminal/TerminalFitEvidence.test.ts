import { expect, test } from "vitest";
import { TerminalFitEvidence } from "./TerminalFitEvidence";

test("pairs font and frame waits with the same completed follow-up fit", () => {
  let now = 0;
  const evidence = new TerminalFitEvidence(() => now);
  const fit = evidence.begin(false);
  now = 10; fit.milestone("fonts_ready");
  now = 1010; fit.milestone("fit_frame");
  now = 1026; fit.milestone("fit_frame");
  now = 1030; fit.finish(false);
  expect(evidence.snapshot()).toMatchObject({ samples: 1, failed: 0,
    slowest: { total_ms: 1030, font_ms: 10, after_fonts_ms: 1020, frames: 2, max_frame_gap_ms: 1000 } });
  fit.finish(true);
  evidence.snapshot().slowest!.font_ms = 500;
  expect(evidence.snapshot().slowest!.font_ms).toBe(10);
  expect(evidence.snapshot().samples).toBe(1);
});

test("does not mix initial fits into follow-ups or hide failed attempts", () => {
  let now = 0;
  const evidence = new TerminalFitEvidence(() => now);
  const initial = evidence.begin(true);
  now = 2000; initial.finish(false);
  const failed = evidence.begin(false);
  now = 2050; failed.finish(true);
  expect(evidence.snapshot()).toMatchObject({ samples: 1, failed: 1,
    slowest: { total_ms: 50, font_ms: null, after_fonts_ms: null, failed: true } });
});

test("bounds retention and rejects invalid elapsed clocks without timers", () => {
  let now = 10;
  const evidence = new TerminalFitEvidence(() => now);
  for (let i = 0; i < 205; i++) evidence.begin(false).finish(false);
  expect(evidence.snapshot().samples).toBe(200);
  now += 3_600_001;
  expect(evidence.snapshot().samples).toBe(0);
  const backwards = evidence.begin(false);
  now -= 1; backwards.finish(false);
  const invalid = evidence.begin(false);
  now = NaN; invalid.finish(false);
  expect(evidence.snapshot().samples).toBe(0);
});
