import { expect, test } from "vitest";
import { readBrowserHeapEstimate } from "./browserHeapEstimate";

test("copies only valid numeric heap estimates without retaining browser objects", () => {
  const memory = { usedJSHeapSize: 10, totalJSHeapSize: 20, jsHeapSizeLimit: 100, privateText: "do not copy" };
  const sample = readBrowserHeapEstimate({ memory }, 123);
  memory.usedJSHeapSize = 15;
  expect(sample).toEqual({ available: true, source: "chromium_legacy_heap_estimate", captured_at: 123, used_bytes: 10, allocated_bytes: 20, limit_bytes: 100 });
});

test("unsupported browsers and throwing getters are unavailable, not zero memory", () => {
  for (const source of [null, {}, { get memory() { throw new Error("unsupported"); } }]) {
    expect(readBrowserHeapEstimate(source)).toEqual({ available: false });
  }
});

test.each([NaN, Infinity, -1, "10", 1.2, Number.MAX_SAFE_INTEGER + 1, 101])("invalid used heap %s cannot become evidence", (usedJSHeapSize) => {
  expect(readBrowserHeapEstimate({ memory: { usedJSHeapSize, totalJSHeapSize: 20, jsHeapSizeLimit: 100 } })).toEqual({ available: false });
});

test("inconsistent allocation bounds are unavailable", () => {
  expect(readBrowserHeapEstimate({ memory: { usedJSHeapSize: 1, totalJSHeapSize: 20, jsHeapSizeLimit: 10 } })).toEqual({ available: false });
  expect(readBrowserHeapEstimate({ memory: { usedJSHeapSize: 0, totalJSHeapSize: 0, jsHeapSizeLimit: 0 } })).toEqual({ available: false });
});
