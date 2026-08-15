import { expect, test, vi } from "vitest";

import { RuntimeRequestError } from "../api";
import { isExpectedRuntimeHandoff, requestRuntimeHandoff } from "./runtimeMaintenance";

test("treats gateway loss during an intentional runtime handoff as expected", async () => {
  await expect(requestRuntimeHandoff(vi.fn().mockRejectedValue(new RuntimeRequestError(504, "timed out")))).resolves.toBeUndefined();
  await expect(requestRuntimeHandoff(vi.fn().mockRejectedValue(new TypeError("fetch failed")))).resolves.toBeUndefined();
  expect(isExpectedRuntimeHandoff(new RuntimeRequestError(503, "unavailable"))).toBe(true);
});

test("preserves actionable maintenance failures", async () => {
  await expect(requestRuntimeHandoff(vi.fn().mockRejectedValue(new RuntimeRequestError(409, "already running")))).rejects.toThrow("already running");
});
