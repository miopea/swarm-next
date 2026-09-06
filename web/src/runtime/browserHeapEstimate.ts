/**
 * Developer-only, on-demand fallback. Chromium's legacy heap estimate can be
 * shared/quantized and excludes allocations outside its reported JS heap.
 * Never use it for admission, automatic eviction or a memory-leak diagnosis.
 * Remove this adapter when a supported cross-browser measurement replaces it.
 */
export function readBrowserHeapEstimate(source: unknown = performance, now = Date.now()) {
  try {
    const memory = (source as { memory?: Record<string, unknown> } | null)?.memory;
    if (!memory) return { available: false as const };
    const used = memory.usedJSHeapSize;
    const allocated = memory.totalJSHeapSize;
    const limit = memory.jsHeapSizeLimit;
    if (![used, allocated, limit].every((value) => typeof value === "number" && Number.isSafeInteger(value) && value >= 0)
      || (used as number) > (allocated as number) || (allocated as number) > (limit as number) || limit === 0
      || !Number.isSafeInteger(now) || now < 0) return { available: false as const };
    return {
      available: true as const,
      source: "chromium_legacy_heap_estimate" as const,
      captured_at: now,
      used_bytes: used as number,
      allocated_bytes: allocated as number,
      limit_bytes: limit as number,
    };
  } catch { return { available: false as const }; }
}
