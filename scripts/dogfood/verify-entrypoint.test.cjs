const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { spawnSync } = require('node:child_process');

const root = path.resolve(__dirname, '../..');
const shell = process.platform === 'win32'
  ? path.join(process.env.ProgramFiles || 'C:/Program Files', 'Git/bin/bash.exe') : 'sh';

function run(args, fail = '') {
  const bin = fs.mkdtempSync(path.join(os.tmpdir(), 'swarm-verify-test-'));
  try {
    fs.mkdirSync(path.join(bin, 'scripts'));
    fs.mkdirSync(path.join(bin, 'web'));
    fs.copyFileSync(path.join(root, 'scripts/verify.sh'), path.join(bin, 'scripts/verify.sh'));
    // Run the actual entrypoint against an isolated audit adapter, never the
    // network. The adapter also verifies CI's working-directory contract.
    fs.writeFileSync(path.join(bin, 'scripts/audit-web.sh'), `#!/bin/sh\n[ "$(basename "$PWD")" = web ] || exit 9\nprintf 'CALL sh ../scripts/audit-web.sh\\n'\n[ "$VERIFY_TEST_FAIL" != audit ] || exit 7\n`);
    for (const command of ['cargo', 'pnpm']) {
      fs.writeFileSync(path.join(bin, command), `#!/bin/sh\nprintf 'CALL ${command} %s\\n' "$*"\nif [ "$*" = "$VERIFY_TEST_FAIL" ]; then exit 7; fi\n`, { mode: 0o755 });
    }
    return spawnSync(shell, ['scripts/verify.sh', ...args], {
      cwd: bin, encoding: 'utf8', timeout: 10_000,
      env: { ...process.env, PATH: `${bin}${path.delimiter}${process.env.PATH}`, VERIFY_TEST_FAIL: fail },
    });
  } finally { fs.rmSync(bin, { recursive: true, force: true }); }
}

test('verification rejects unknown modes instead of declaring an empty run passed', () => {
  for (const args of [['typo'], ['web', 'unexpected']]) {
    const result = run(args);
    assert.equal(result.status, 2, result.stderr);
    assert.doesNotMatch(result.stdout, /all checks passed|CALL/);
  }
});

test('web verification uses all CI validation commands and reports every failure', () => {
  const result = run(['web'], 'check');
  assert.equal(result.status, 1, result.stderr);
  assert.deepEqual(result.stdout.split('\n').filter(line => line.startsWith('CALL')), [
    'CALL sh ../scripts/audit-web.sh', 'CALL pnpm check',
    'CALL pnpm test', 'CALL pnpm test:dogfood', 'CALL pnpm build',
  ]);
  assert.match(result.stdout, /web check: FAILED \(exit 7\)/);
  assert.doesNotMatch(result.stdout, /all checks passed/);
});

test('an audit wrapper failure fails verification but still runs the remaining checks', () => {
  const result = run(['web'], 'audit');
  assert.equal(result.status, 1, result.stderr);
  assert.match(result.stdout, /web audit: FAILED \(exit 7\)/);
  assert.match(result.stdout, /CALL pnpm build/);
  assert.doesNotMatch(result.stdout, /all checks passed/);
});

test('Rust verification pins the toolchain and includes the optimized resize regression', () => {
  const result = run(['rust']);
  assert.equal(result.status, 0, result.stderr);
  assert.deepEqual(result.stdout.split('\n').filter(line => line.startsWith('CALL')), [
    'CALL cargo +1.97.1 fmt --all --check',
    'CALL cargo +1.97.1 clippy --workspace --all-targets --all-features -- -D warnings',
    'CALL cargo +1.97.1 test --workspace --all-features',
    'CALL cargo +1.97.1 test --release -p swarm-terminal resize_updates_pty_and_canonical_dimensions',
  ]);
});

test('the entrypoint command list matches the Rust and web CI validation steps', () => {
  const workflow = fs.readFileSync(path.join(root, '.github/workflows/ci.yml'), 'utf8');
  const jobs = workflow.slice(workflow.indexOf('\n  rust:'), workflow.indexOf('\n  rust-audit:'));
  const expected = [...jobs.matchAll(/^\s+- run: ((?:cargo|pnpm|sh) .+)$/gm)]
    .map(match => match[1]).filter(command => !command.startsWith('pnpm install '));
  const result = run(['all']);
  assert.equal(result.status, 0, result.stderr);
  const actual = result.stdout.split('\n').filter(line => line.startsWith('CALL '))
    .map(line => line.slice(5).replace('cargo +1.97.1 ', 'cargo '));
  assert.deepEqual(actual, expected, 'Update local validation when CI commands change');
});
