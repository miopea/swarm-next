import { afterEach, expect, test, vi } from "vitest";

import { buildSanitizedDiagnosticReport } from "./diagnosticReport";
import type { RuntimeResources } from "../api";

test("storage evidence exports measurements but not unexpected path fields", () => {
  const resources = {
    sampled_at: 1, policy: { mode: "observe_only", advisory_percent: 85, critical_percent: 95 },
    api: { resident_memory_bytes: null, pressure: "unavailable" },
    terminal_host: { resident_memory_bytes: null, pressure: "unavailable" },
    storage: [{ scope: "database", available_bytes: 100, total_bytes: 1000,
      pressure: "critical", advisory_available_bytes: 200, critical_available_bytes: 150,
      path: "/private/fictional/customer-data" }],
  } as unknown as RuntimeResources; // Simulate an unexpected server field at the export boundary.
  const result = buildSanitizedDiagnosticReport({
    health: undefined, hiveIdentity: undefined, liveFeedState: "connected", recentEvents: [],
    runtime: { loaded: true, resources }, sessions: [], workers: [],
  });
  expect(result.runtime_resources?.storage?.[0].available_bytes).toBe(100);
  expect(JSON.stringify(result.runtime_resources?.storage)).not.toContain("private");
});

afterEach(() => vi.unstubAllGlobals());

function report() {
  return buildSanitizedDiagnosticReport({
    health: undefined,
    hiveIdentity: undefined,
    liveFeedState: "connected",
    recentEvents: [],
    runtime: { loaded: false },
    sessions: [],
    workers: [],
  });
}

test("database integrity is not inferred from reachability", () => {
  expect(report().database.integrity).toBe("not_checked");
});

/**
 * A layout report is unanswerable without the size the layout decided from.
 *
 * The operator sent a screenshot of a window 863 pixels wide showing the
 * stacked layout, saying "I am not using my phone at all". Both halves were
 * true: at 1.5x display scaling that window is 575 CSS pixels, under the 680px
 * breakpoint, so the browser was right and the window looked wrong. Nothing in
 * the diagnostic bundle carried either number, so the report could not be
 * answered from the report.
 */
test("the bundle carries the size the layout decided from, not only the one you can see", () => {
  vi.stubGlobal("innerWidth", 575);
  vi.stubGlobal("innerHeight", 573);
  vi.stubGlobal("devicePixelRatio", 1.5);
  vi.stubGlobal("matchMedia", vi.fn().mockReturnValue({ matches: true }));

  const viewport = report().browser.viewport;

  expect(viewport.css_width).toBe(575);
  expect(viewport.device_pixel_ratio).toBe(1.5);
  // What the person is looking at. The gap between this and css_width IS the
  // diagnosis, and reporting only one of them hides it.
  expect(viewport.physical_width).toBe(863);
  expect(viewport.stacked_layout).toBe(true);
});

test("an ordinary unscaled desktop window reports no stacking", () => {
  vi.stubGlobal("innerWidth", 1440);
  vi.stubGlobal("innerHeight", 900);
  vi.stubGlobal("devicePixelRatio", 1);
  vi.stubGlobal("matchMedia", vi.fn().mockReturnValue({ matches: false }));

  const viewport = report().browser.viewport;

  expect(viewport.css_width).toBe(1440);
  expect(viewport.physical_width).toBe(1440);
  expect(viewport.stacked_layout).toBe(false);
});

test("a browser that refuses matchMedia still produces a report", () => {
  // The bundle exists to be gathered when something is already wrong, so no
  // part of collecting it may throw.
  vi.stubGlobal("innerWidth", 1024);
  vi.stubGlobal("devicePixelRatio", 1);
  vi.stubGlobal("matchMedia", () => { throw new Error("blocked"); });

  expect(() => report()).not.toThrow();
  expect(report().browser.viewport.stacked_layout).toBeNull();
});

/**
 * The report promises in its own text that automatic collection is
 * content-free. Viewport facts are sizes and a boolean, and must stay that way.
 */
test("the viewport section stays content-free", () => {
  vi.stubGlobal("innerWidth", 1440);
  vi.stubGlobal("devicePixelRatio", 1);
  vi.stubGlobal("matchMedia", vi.fn().mockReturnValue({ matches: false }));

  for (const value of Object.values(report().browser.viewport)) {
    expect(["number", "boolean", "object"]).toContain(typeof value);
  }
});
