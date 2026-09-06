// Content-free analysis of observe-live-soak.sh CSVs. Never declares a soak
// accepted: service/session continuity belongs to the observer's final report.
const fs = require('node:fs');

function summarize(csv) {
  const lines = csv.trim().split(/\r?\n/);
  const columns = lines.shift().split(',');
  const required = ['elapsed_seconds', 'api_memory_bytes', 'terminal_host_memory_bytes',
    'running_sessions', 'history_bytes', 'dropped_history_bytes',
    'api_cpu_nanoseconds', 'terminal_host_cpu_nanoseconds', 'collection_seconds'];
  // Historical reports remain readable but cannot invent engine-only evidence.
  const engineColumns = ['engine_process_cpu_ticks', 'engine_process_start_ticks', 'clock_ticks_per_second'];
  const hasEngine = engineColumns.some(key => columns.includes(key));
  if (hasEngine) required.push(...engineColumns);
  if (required.some(key => columns.filter(column => column === key).length !== 1)) {
    throw new Error('Missing or duplicated sample columns');
  }
  const rows = lines.map(line => {
    const cells = line.split(',');
    if (cells.length !== columns.length) throw new Error('Incomplete sample row');
    return Object.fromEntries(required.map(key => {
      const cell = cells[columns.indexOf(key)];
      const value = Number(cell);
      if (!/^\d+$/.test(cell) || !Number.isSafeInteger(value)) throw new Error(`Invalid ${key}`);
      return [key, value];
    }));
  });
  if (rows.length < 2) throw new Error('At least two samples are required');
  const range = key => ({ min: Math.min(...rows.map(row => row[key])), max: Math.max(...rows.map(row => row[key])) });
  const first = rows[0];
  const last = rows.at(-1);
  const span = last.elapsed_seconds - first.elapsed_seconds;
  const cpu = (key, unitsPerSecond = 1e9) => {
    const intervals = rows.slice(1).map((row, i) => {
      const seconds = row.elapsed_seconds - rows[i].elapsed_seconds;
      const delta = row[key] - rows[i][key];
      if (seconds <= 0) throw new Error('Sample times must increase');
      if (delta < 0) throw new Error('CPU counter reset: do not join different process lifetimes');
      return delta / (seconds * unitsPerSecond) * 100;
    });
    return {
      average_percent_of_one_core: (last[key] - first[key]) / (span * unitsPerSecond) * 100,
      max_interval_percent_of_one_core: Math.max(...intervals),
    };
  };
  if (hasEngine && (first.clock_ticks_per_second === 0 || rows.some(row =>
    row.engine_process_start_ticks !== first.engine_process_start_ticks ||
    row.clock_ticks_per_second !== first.clock_ticks_per_second))) {
    throw new Error('Engine process lifetime or clock rate changed');
  }
  return {
    sample_count: rows.length,
    observed_span_seconds: span,
    performance_acceptance: 'not_evaluated',
    continuity: 'requires_observer_final_report',
    cpu: { api_cgroup: cpu('api_cpu_nanoseconds'), terminal_host_cgroup_including_workers: cpu('terminal_host_cpu_nanoseconds'),
      engine_process_only: hasEngine ? cpu('engine_process_cpu_ticks', first.clock_ticks_per_second) : null },
    memory_bytes: { api_cgroup: range('api_memory_bytes'), terminal_host_cgroup_including_workers: range('terminal_host_memory_bytes') },
    running_sessions: range('running_sessions'),
    history_bytes: range('history_bytes'),
    dropped_history_bytes: range('dropped_history_bytes'),
    collection_seconds: range('collection_seconds'),
  };
}

module.exports = { summarize };
if (require.main === module) {
  try {
    if (process.argv.length !== 3) throw new Error('Usage: node live-soak-summary.cjs <samples.csv|->');
    const csv = fs.readFileSync(process.argv[2] === '-' ? 0 : process.argv[2], 'utf8');
    console.log(JSON.stringify(summarize(csv), null, 2));
  } catch (error) {
    console.error(error.message);
    process.exitCode = 1;
  }
}
