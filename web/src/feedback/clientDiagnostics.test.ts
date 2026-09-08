import { afterEach, expect, test } from "vitest";

import { readClientFailures, recordClientFailure } from "./clientDiagnostics";

afterEach(() => window.sessionStorage.clear());

test("terminal timeout evidence is allowlisted and discards arbitrary stored fields", () => {
  window.sessionStorage.setItem("swarm-next.client-failures.v1", JSON.stringify([
    { kind: "terminal_socket_open_timeout", occurred_at: 1, token: "private", text: "terminal bytes" },
    { kind: "arbitrary terminal content", occurred_at: 2 },
    { kind: "terminal_grant_timeout", occurred_at: -1 },
    { kind: "terminal_restore_timeout", occurred_at: null },
  ]));
  expect(readClientFailures()).toEqual([{ kind: "terminal_socket_open_timeout", occurred_at: 1 }]);
  recordClientFailure("terminal_probe_timeout");
  expect(readClientFailures().at(-1)?.kind).toBe("terminal_probe_timeout");
  expect(JSON.stringify(readClientFailures())).not.toMatch(/private|terminal bytes|token/);
});

test("keeps a bounded content-free failure history across reloads", () => {
  for (let index = 0; index < 24; index += 1) recordClientFailure(index % 2 ? "window_error" : "react_render");

  const failures = readClientFailures();
  expect(failures).toHaveLength(20);
  expect(failures.every((failure) => Object.keys(failure).sort().join(",") === "kind,occurred_at")).toBe(true);
});
