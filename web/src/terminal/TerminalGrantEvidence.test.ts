import { expect, test } from "vitest";
import { TerminalGrantEvidence, parseGrantServerTiming } from "./TerminalGrantEvidence";

const resource = { entryType: "resource", initiatorType: "fetch", startTime: 110,
  requestStart: 120, responseStart: 180, responseEnd: 200 };

test("pairs network response with client completion without retaining identity", () => {
  const evidence = new TerminalGrantEvidence(() => 1000);
  evidence.record(100, 900, [{ ...resource, name: "private-session-url" } as typeof resource]);
  expect(evidence.snapshot()).toEqual({ samples: 1, matched_resource_samples: 1, slowest: {
    at: 1000, fetch_elapsed_ms: 800, resource_ms: 90, request_to_first_byte_ms: 60,
    body_transfer_ms: 20, response_to_client_ms: 700, server_handler_ms: null } });
  expect(JSON.stringify(evidence.snapshot())).not.toMatch(/private|url|session/);
  evidence.snapshot().slowest!.resource_ms = 0;
  expect(evidence.snapshot().slowest!.resource_ms).toBe(90);
});

test("missing, ambiguous, stale and restricted entries are unknown rather than zero", () => {
  for (const entries of [[], [resource, resource], [{ ...resource, startTime: 99 }],
    [{ ...resource, requestStart: 0 }], [{ ...resource, responseEnd: 901 }],
    [{ ...resource, responseStart: NaN }], [{ ...resource, initiatorType: "xmlhttprequest" }]]) {
    const evidence = new TerminalGrantEvidence(() => 1000);
    evidence.record(100, 900, entries);
    expect(evidence.snapshot()).toMatchObject({ samples: 1, matched_resource_samples: 0,
      slowest: { resource_ms: null, response_to_client_ms: null } });
  }
});

test("bounds retention, expires old samples and refuses invalid clocks", () => {
  let now = 1000;
  const evidence = new TerminalGrantEvidence(() => now);
  for (let i = 0; i < 201; i++) evidence.record(100, 900 + i, [resource]);
  expect(evidence.snapshot().samples).toBe(200);
  now += 3_600_001;
  expect(evidence.snapshot().samples).toBe(0);
  for (const [start, end] of [[-1, 1], [NaN, 1], [2, 1], [0, Infinity], [0, 3_600_001]]) {
    evidence.record(start, end, []);
  }
  now = NaN;
  evidence.record(0, 1, []);
  expect(evidence.snapshot().samples).toBe(0);
});

test("server timing accepts only one bounded matching duration", () => {
  expect(parseGrantServerTiming("swarm_grant;dur=0.250", 50)).toBe(0.25);
  expect(parseGrantServerTiming('edge;dur=40, swarm_grant;dur=12.500', 50)).toBe(12.5);
  for (const header of [null, "", "private=value", "swarm_grant;dur=-1", "swarm_grant;dur=NaN",
    "swarm_grant;dur=60001", "swarm_grant;dur=52", "swarm_grant;dur=1,swarm_grant;dur=2",
    "swarm_grant;dur=1,swarm_grant;dur=invalid", "swarm_grant;dur=1;desc=private", "x".repeat(1025)]) {
    expect(parseGrantServerTiming(header, 50)).toBeNull();
  }
  const evidence = new TerminalGrantEvidence(() => 1000);
  evidence.record(100, 900, [resource], "edge;desc=private,swarm_grant;dur=12.500");
  expect(evidence.snapshot().slowest!.server_handler_ms).toBe(12.5);
  expect(JSON.stringify(evidence.snapshot())).not.toContain("private");
});
