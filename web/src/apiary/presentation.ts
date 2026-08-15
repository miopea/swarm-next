import type { FederationCatalogReadiness, FederationSyncCondition } from "../api";

export const federationSyncCopy: Record<FederationSyncCondition, readonly [string, string]> = {
  idle: ["Not connected yet", "Automatic Keeper sync is not enabled in this build."],
  current: ["Up to date", "This Hive completed its latest Keeper reconciliation."],
  offline: ["Keeper temporarily unavailable", "Owned work remains local; new shared claims wait."],
  authentication_required: ["Membership credentials need attention", "Keeper synchronization is paused until access is restored."],
  incompatible: ["Runtime update required", "This Hive and its Keeper need compatible federation versions."],
};

export function catalogBlockerLabel(blocker: FederationCatalogReadiness["blockers"][number]) {
  return ({
    catalog_missing: "Keeper catalog has not arrived",
    catalog_stale: "Keeper catalog needs refreshing",
    integration_not_ready: "Jira connection needs attention",
    policy_revision_changed: "Apiary policy changed",
    project_access_not_ready: "Project access or workflow mapping is incomplete",
  } as const)[blocker];
}
