const assert = require("node:assert/strict");
const test = require("node:test");

const { readOwnedProcessMemory } = require("./browser-memory-soak.cjs");

test("process sampling tolerates a helper that exited after enumeration", () => {
  const samples = readOwnedProcessMemory([process.pid, 2_147_483_647]);

  assert.ok(samples.some((sample) => sample.id === process.pid));
  assert.ok(samples.every((sample) => sample.id !== 2_147_483_647));
});
