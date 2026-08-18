import { expect, test } from "vitest";

import { compactRuntimeVersion, deployedRevision, runtimeVersionIdentity, shortRevision } from "./runtimeVersion";

test("presents long development build identities without timestamp noise", () => {
  const version = "0.1.0-dev-2af2cf734c46-20260817233758-1821863";
  expect(compactRuntimeVersion(version)).toBe("Healthy · 0.1.0 · 2af2cf7");
  expect(runtimeVersionIdentity(version)).toBe("0.1.0 · revision 2af2cf7");
  expect(deployedRevision(version)).toBe("2af2cf7");
  expect(shortRevision("7d9538977d9c")).toBe("7d95389");
});

test("preserves ordinary release versions", () => {
  expect(compactRuntimeVersion("0.1.0-7d9538977d9c")).toBe("Healthy · 0.1.0-7d9538977d9c");
  expect(runtimeVersionIdentity("0.1.0-7d9538977d9c")).toBe("0.1.0-7d9538977d9c");
});
