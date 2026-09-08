const assert = require("node:assert/strict");
const test = require("node:test");

const { readOwnedProcessMemory, recoverAfterGatewayInterruption } = require("./browser-memory-soak.cjs");

test("process sampling tolerates a helper that exited after enumeration", () => {
  const samples = readOwnedProcessMemory([process.pid, 2_147_483_647]);

  assert.ok(samples.some((sample) => sample.id === process.pid));
  assert.ok(samples.every((sample) => sample.id !== 2_147_483_647));
});

function recoveryPage({ displayedVersion = "1.5.0-dev-current", initialFailure = false } = {}) {
  const calls = [];
  const page = {
    context: () => ({ cookies: async () => [] }),
    request: { get: async () => {
      calls.push("health");
      if (initialFailure && calls.length === 1) return { ok: () => false };
      return { ok: () => true, json: async () => ({
        status: "ok", version: "1.5.0-dev-current", database_recovery_required: false, degraded: [],
      }) };
    } },
    reload: async () => { calls.push("reload"); },
    getByRole: (role, options) => {
      assert.equal(role, "button");
      assert.equal(options.name, "Download Hive backup");
      return { waitFor: async () => { calls.push("authenticated settings"); } };
    },
    locator: (selector) => {
      assert.equal(selector, ".rail-footer .runtime-status");
      return { first: () => ({ innerText: async () => `Runtime ${displayedVersion}\nDev` }) };
    },
  };
  return { page, calls };
}

test("gateway recovery verifies the current version and retains authentication without unlocking", async () => {
  const { page, calls } = recoveryPage({ initialFailure: true });
  const result = await recoverAfterGatewayInterruption(page, 90);
  assert.deepEqual(calls, ["health", "health", "reload", "authenticated settings"]);
  assert.equal(result.recovered_version, "1.5.0-dev-current");
  assert.equal(result.elapsed_seconds, 90);
});

test("a restored page showing an old runtime cannot pass recovery", async () => {
  const { page } = recoveryPage({ displayedVersion: "0.1.0-old" });
  await assert.rejects(recoverAfterGatewayInterruption(page, 90), /did not match the verified healthy API/);
});
