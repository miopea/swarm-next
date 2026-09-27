import { expect, test } from "vitest";
import type { ApiaryMember, FederationCatalogReadiness, FederationSyncHealth } from "../api";
import { catalogReadinessLabel, federationSyncCopy, federationSyncFailure, memberContact, MEMBER_SILENCE_SECONDS } from "./presentation";

test("queued synchronization does not imply that previous synchronization never happened", () => {
  expect(federationSyncCopy.idle[0]).toBe("Waiting to synchronize");
  expect(federationSyncCopy.idle[1]).toContain("Previously received work remains available");
});

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

test("no refusal is described as a permanent pause", () => {
  // Every failure retries on a bounded backoff; copy that says "paused until"
  // tells an operator to wait for something that is already happening.
  expect(federationSyncCopy.authentication_required[1]).not.toContain("paused");
  expect(federationSyncCopy.authentication_required[1]).toContain("keeps retrying");
  expect(federationSyncCopy.incompatible[1]).toContain("keeps retrying");
});

test("a failed synchronization names its step and its next attempt", () => {
  const sync = (overrides: Partial<FederationSyncHealth>): FederationSyncHealth => ({
    condition: "offline", last_attempt_at: 990, last_success_at: 900, consecutive_failures: 1, next_attempt_at: null, ...overrides,
  });
  expect(federationSyncFailure(sync({ condition: "current" }), 1000)).toBeUndefined();
  expect(federationSyncFailure(sync({ condition: "idle" }), 1000)).toBeUndefined();
  expect(federationSyncFailure(undefined, 1000)).toBeUndefined();
  expect(federationSyncFailure(sync({ failed_step: "stewardships", next_attempt_at: 1030 }), 1000)).toBe("Stopped at stewardships · next try in 30s");
  expect(federationSyncFailure(sync({ failed_step: "shared tasks", next_attempt_at: 1300 }), 1000)).toBe("Stopped at shared tasks · next try in 5m");
  expect(federationSyncFailure(sync({ next_attempt_at: 999 }), 1000)).toBe("Stopped · trying again now");
  // A record written before steps were kept has none, and says so without inventing one.
  expect(federationSyncFailure(sync({ failed_step: null }), 1000)).toBe("Stopped");
});

test("the Keeper raises a member it has not heard from in longer than the longest backoff", () => {
  const member = (last_contact_at: number | null | undefined): ApiaryMember => ({
    hive_id: "hive-2", hive_name: "Clover Hive", operator_id: "operator-2", operator_display_name: "Cora", role: "member", is_local: false, last_contact_at,
  });
  expect(memberContact(member(10_000 - 30), 10_000)).toEqual({ label: "heard from just now", silent: false });
  expect(memberContact(member(10_000 - 240), 10_000)).toEqual({ label: "heard from 4m ago", silent: false });
  expect(memberContact(member(10_000 - MEMBER_SILENCE_SECONDS), 10_000).silent).toBe(false);
  expect(memberContact(member(10_000 - MEMBER_SILENCE_SECONDS - 1), 10_000).silent).toBe(true);
  expect(memberContact(member(10_000 - 3 * 86_400), 10_000)).toEqual({ label: "heard from 3d ago", silent: true });
  expect(memberContact(member(null), 10_000)).toEqual({ label: "no contact recorded", silent: true });
  expect(memberContact(member(undefined), 10_000)).toEqual({ label: "no contact recorded", silent: true });
});
