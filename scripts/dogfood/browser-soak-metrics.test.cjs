const test = require("node:test");
const assert = require("node:assert/strict");

const { MIB, evaluateGrowth, processTotals, summarizeSeries } = require("./browser-soak-metrics.cjs");

test("summarizes a time series with a per-minute slope", () => {
  const summary = summarizeSeries([
    { elapsed_seconds: 0, bytes: 10 * MIB },
    { elapsed_seconds: 30, bytes: 11 * MIB },
    { elapsed_seconds: 60, bytes: 12 * MIB },
  ], "bytes");
  assert.equal(summary.growth, 2 * MIB);
  assert.equal(summary.slope_bytes_per_minute, 2 * MIB);
});

test("fails only sustained material growth after warmup", () => {
  const runaway = Array.from({ length: 10 }, (_, index) => ({
    elapsed_seconds: index * 60,
    bytes: index * 32 * MIB,
  }));
  assert.equal(evaluateGrowth(runaway, "bytes", { warmupSamples: 2 }).passed, false);

  const bounded = runaway.map((sample, index) => ({ ...sample, bytes: (index % 2) * 8 * MIB }));
  assert.equal(evaluateGrowth(bounded, "bytes", { warmupSamples: 2 }).passed, true);
});

test("totals only the processes owned by the browser", () => {
  assert.deepEqual(processTotals([
    { working_set_bytes: 10, private_bytes: 8 },
    { working_set_bytes: 20, private_bytes: 17 },
  ]), { working_set_bytes: 30, private_bytes: 25 });
});
