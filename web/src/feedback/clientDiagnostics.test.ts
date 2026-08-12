import { afterEach, expect, test } from "vitest";

import { readClientFailures, recordClientFailure } from "./clientDiagnostics";

afterEach(() => window.sessionStorage.clear());

test("keeps a bounded content-free failure history across reloads", () => {
  for (let index = 0; index < 24; index += 1) recordClientFailure(index % 2 ? "window_error" : "react_render");

  const failures = readClientFailures();
  expect(failures).toHaveLength(20);
  expect(failures.every((failure) => Object.keys(failure).sort().join(",") === "kind,occurred_at")).toBe(true);
});
