import type { FederationCatalogReadiness, FederationSyncCondition } from "../api";

export const federationSyncCopy: Record<FederationSyncCondition, readonly [string, string]> = {
  idle: ["Waiting for first sync", "This Hive will poll Keeper automatically."],
  current: ["Up to date", "This Hive completed its latest Keeper reconciliation."],
  offline: ["Keeper temporarily unavailable", "Owned work remains local; new shared claims wait."],
  authentication_required: ["Membership credentials need attention", "Keeper synchronization is paused until access is restored."],
  incompatible: ["Shared setup needs attention", "The Keeper response could not be accepted. Check connection and version details in Diagnostics; local workers are unaffected."],
};

export function catalogReadinessLabel(catalog?: FederationCatalogReadiness) {
  if (!catalog?.acknowledgement) return "Waiting";
  if (catalog.blockers.includes("catalog_stale")) return "Refresh needed";
  if (catalog.blockers.includes("policy_revision_changed")) return "Policy changed";
  if (catalog.blockers.includes("catalog_missing")) return "Waiting";
  return "Verified";
}

export function catalogBlockerLabel(blocker: FederationCatalogReadiness["blockers"][number]) {
  return ({
    catalog_missing: "Keeper catalog has not arrived",
    catalog_stale: "Keeper catalog needs refreshing",
    integration_not_ready: "Jira connection needs attention",
    policy_revision_changed: "Apiary policy changed",
    project_access_not_ready: "Project access or workflow mapping is incomplete",
  } as const)[blocker];
}
