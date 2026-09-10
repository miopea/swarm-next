import { expect, test } from "vitest";
import type { FederationCatalogReadiness } from "../api";
import { catalogReadinessLabel, federationSyncCopy } from "./presentation";

test("a retained signature acknowledgement does not label a stale catalog verified", () => {
  const catalog = { acknowledgement: {}, blockers: [] } as unknown as FederationCatalogReadiness;
  expect(catalogReadinessLabel(catalog)).toBe("Verified");
  catalog.blockers = ["catalog_stale"];
  expect(catalogReadinessLabel(catalog)).toBe("Refresh needed");
  catalog.blockers = ["policy_revision_changed"];
  expect(catalogReadinessLabel(catalog)).toBe("Policy changed");
  catalog.blockers = ["catalog_missing"];
  expect(catalogReadinessLabel(catalog)).toBe("Waiting");
  expect(catalogReadinessLabel()).toBe("Waiting");
});

test("generic rejected federation responses do not prescribe an unproven runtime update", () => {
  expect(federationSyncCopy.incompatible[0]).toBe("Shared setup needs attention");
  expect(federationSyncCopy.incompatible.join(" ")).not.toContain("update required");
  expect(federationSyncCopy.incompatible[1]).toContain("local workers are unaffected");
});
