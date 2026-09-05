const MIB = 1024 * 1024;

function summarizeSeries(samples, field) {
  const points = samples
    .map((sample) => ({ seconds: sample.elapsed_seconds, value: sample[field] }))
    .filter((point) => Number.isFinite(point.seconds) && Number.isFinite(point.value));
  if (points.length === 0) return { min: null, max: null, growth: null, slope_bytes_per_minute: null };
  const values = points.map((point) => point.value);
  const meanSeconds = points.reduce((total, point) => total + point.seconds, 0) / points.length;
  const meanValue = values.reduce((total, value) => total + value, 0) / values.length;
  const numerator = points.reduce((total, point) => total + ((point.seconds - meanSeconds) * (point.value - meanValue)), 0);
  const denominator = points.reduce((total, point) => total + ((point.seconds - meanSeconds) ** 2), 0);
  const slopeBytesPerSecond = denominator === 0 ? 0 : numerator / denominator;
  return {
    min: Math.min(...values),
    max: Math.max(...values),
    growth: values.at(-1) - values[0],
    slope_bytes_per_minute: slopeBytesPerSecond * 60,
  };
}

function evaluateGrowth(samples, field, options = {}) {
  const warmupSamples = options.warmupSamples ?? 5;
  const growthLimit = options.growthLimit ?? 128 * MIB;
  const slopeLimit = options.slopeLimit ?? 4 * MIB;
  const measured = samples.slice(Math.min(warmupSamples, Math.max(0, samples.length - 2)));
  const usable = measured.length >= 2 && measured.every((sample, index) =>
    Number.isFinite(sample.elapsed_seconds) && sample.elapsed_seconds >= 0
    && Number.isFinite(sample[field]) && sample[field] >= 0
    && (index === 0 || sample.elapsed_seconds > measured[index - 1].elapsed_seconds));
  const observed = summarizeSeries(measured, field);
  return {
    ...observed,
    passed: usable ? observed.growth <= growthLimit || observed.slope_bytes_per_minute <= slopeLimit : null,
    evidence_status: usable ? "measured" : "insufficient_or_invalid_samples",
    growth_limit: growthLimit,
    slope_limit_bytes_per_minute: slopeLimit,
  };
}

function growthResult(evaluations) {
  if (evaluations.some((value) => value.passed === false)) return "failed";
  return evaluations.length > 0 && evaluations.every((value) => value.passed === true) ? "passed" : "inconclusive";
}

function processTotals(processes) {
  return processes.reduce((totals, process) => ({
    working_set_bytes: totals.working_set_bytes + (process.working_set_bytes ?? 0),
    private_bytes: totals.private_bytes + (process.private_bytes ?? 0),
  }), { working_set_bytes: 0, private_bytes: 0 });
}

function isTransientGatewayError(message) {
  return /^Failed to load resource: the server responded with a status of 502 \(\)$/.test(message);
}

module.exports = { MIB, evaluateGrowth, growthResult, isTransientGatewayError, processTotals, summarizeSeries };
