const { test } = require('node:test');
const assert = require('node:assert/strict');
const { mkdtempSync, readdirSync, rmSync } = require('node:fs');
const { tmpdir } = require('node:os');
const { join } = require('node:path');
const { spawnSync } = require('node:child_process');

test('failed observation reports its phase without leaking authentication or leaving its config', () => {
  const directory = mkdtempSync(join(tmpdir(), 'swarm-observer-fixture-'));
  const fictionalToken = 'fictional-observer-secret-do-not-log';
  try {
    const result = spawnSync('bash', ['-c',
      'curl() { return 28; }; export -f curl; exec bash "$1"',
      'observer-fixture', join(__dirname, 'observe-live-soak.sh')], {
      encoding: 'utf8', timeout: 10000,
      env: { ...process.env, SWARM_OPERATOR_TOKEN: fictionalToken,
        SWARM_SOAK_REPORT_DIR: directory, TMPDIR: directory },
    });
    assert.ifError(result.error);
    assert.equal(result.status, 28);
    assert.match(result.stderr, /Observation incomplete: phase=initialization exit=28 completed_samples=0/);
    assert.equal((result.stdout + result.stderr).includes(fictionalToken), false);
    assert.deepEqual(readdirSync(directory), []);
  } finally {
    rmSync(directory, { recursive: true, force: true });
  }
});
