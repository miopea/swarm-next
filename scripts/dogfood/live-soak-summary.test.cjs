const { test } = require('node:test');
const assert = require('node:assert/strict');
const { summarize } = require('./live-soak-summary.cjs');

const header = 'elapsed_seconds,api_memory_bytes,terminal_host_memory_bytes,running_sessions,history_bytes,dropped_history_bytes,api_cpu_nanoseconds,terminal_host_cpu_nanoseconds,collection_seconds';
const row = (time, api, host) => `${time},100,200,15,300,0,${api},${host},1`;
const fixture = [header, row(1, 1000000000, 2000000000), row(11, 2000000000, 22000000000), row(41, 14000000000, 52000000000)];

test('weights CPU by elapsed time and retains multi-core percentages', () => {
  const result = summarize(fixture.join('\n'));
  assert.equal(result.observed_span_seconds, 40);
  assert.equal(result.cpu.api_cgroup.average_percent_of_one_core, 32.5);
  assert.equal(result.cpu.api_cgroup.max_interval_percent_of_one_core, 40);
  assert.equal(result.cpu.terminal_host_cgroup_including_workers.average_percent_of_one_core, 125);
  assert.equal(result.cpu.terminal_host_cgroup_including_workers.max_interval_percent_of_one_core, 200);
  assert.equal(result.performance_acceptance, 'not_evaluated');
  assert.equal(result.continuity, 'requires_observer_final_report');
  assert.deepEqual(result.memory_bytes.api_cgroup, { min: 100, max: 100 });
});

test('accepts CRLF input without inventing completion evidence', () => {
  assert.equal(summarize(fixture.join('\r\n') + '\r\n').sample_count, 3);
});

test('separates process anonymous memory from cgroup totals without fabricating old fields', () => {
  const memory = fixture.map((line, i) => i === 0
    ? `${line},api_process_rss_bytes,api_process_anon_bytes,api_process_file_bytes`
    : `${line},${1000 + i * 100},${500 + i * 100},500`);
  const result = summarize(memory.join('\n'));
  assert.deepEqual(result.memory_bytes.api_process, {
    rss: { min: 1100, max: 1300 }, anonymous: { min: 600, max: 800 }, file_backed: { min: 500, max: 500 },
  });
  assert.equal(summarize(fixture.join('\n')).memory_bytes.api_process, null);
  assert.equal(result.performance_acceptance, 'not_evaluated');
  assert.throws(() => summarize(memory.join('\n').replace('api_process_file_bytes', 'missing')));
  assert.throws(() => summarize(memory.join('\n').replace('api_process_file_bytes', 'api_process_anon_bytes')));
});

test('engine CPU excludes child workers and respects the host clock rate', () => {
  const engine = fixture.map((line, i) => i === 0
    ? `${line},engine_process_cpu_ticks,engine_process_start_ticks,clock_ticks_per_second`
    : `${line},${[0, 100, 200, 500][i]},900,100`);
  const result = summarize(engine.join('\n'));
  assert.equal(result.cpu.engine_process_only.average_percent_of_one_core, 10);
  assert.equal(result.cpu.terminal_host_cgroup_including_workers.average_percent_of_one_core, 125);
  assert.equal(summarize(fixture.join('\n')).cpu.engine_process_only, null);
  for (const invalid of [engine.join('\n').replace(',500,900,100', ',500,901,100'),
    engine.join('\n').replace(',500,900,100', ',500,900,200'),
    engine.join('\n').replaceAll(',900,100', ',900,0'),
    engine.join('\n').replace('engine_process_start_ticks', 'missing')]) {
    assert.throws(() => summarize(invalid));
  }
});

test('rejects truncated, invalid and insufficient samples', () => {
  for (const csv of [header, fixture.slice(0, 2).join('\n'),
    [...fixture, '42,100'].join('\n'), fixture.join('\n').replace(',100,', ',,')]) {
    assert.throws(() => summarize(csv));
  }
});

test('rejects duplicate times and counter resets instead of reporting recovery as efficiency', () => {
  assert.throws(() => summarize([header, row(1, 10, 20), row(1, 20, 30)].join('\n')), /times must increase/);
  assert.throws(() => summarize([header, row(1, 10, 20), row(2, 9, 30)].join('\n')), /counter reset/);
  assert.throws(() => summarize([header, row(1, 10, 20), row(2, 20, 19)].join('\n')), /counter reset/);
});
