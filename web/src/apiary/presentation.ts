import type { ApiaryMember, FederationCatalogReadiness, FederationSyncCondition, FederationSyncHealth } from "../api";

export const federationSyncCopy: Record<FederationSyncCondition, readonly [string, string]> = {
  idle: ["Waiting to synchronize", "This Hive will poll Keeper automatically. Previously received work remains available."],
  current: ["Up to date", "This Hive completed its latest Keeper reconciliation."],
  offline: ["Keeper temporarily unavailable", "Owned work remains local; new shared claims wait."],
  authentication_required: ["Membership credentials need attention", "The Keeper refused this Hive's credential. It keeps retrying on its own; shared changes wait until the Keeper accepts it."],
  incompatible: ["Shared setup needs attention", "The Keeper response could not be accepted. It keeps retrying on its own; check connection and version details in Diagnostics; local workers are unaffected."],
};

function span(seconds: number): string {
  if (seconds < 90) return `${Math.max(1, Math.round(seconds))}s`;
  const minutes = Math.round(seconds / 60);
  if (minutes < 90) return `${minutes}m`;
  const hours = Math.round(minutes / 60);
  return hours < 48 ? `${hours}h` : `${Math.round(hours / 24)}d`;
}

/**
 * Which part of synchronization failed and when it is tried again.
 *
 * ⚠️ A CONDITION ALONE SENT AN OPERATOR TO GUESS. "Shared setup needs attention"
 * was all a member said for days while one refused request kept it silent;
 * the step it stopped at is the first thing anyone diagnosing it needs.
 */
export function federationSyncFailure(sync: FederationSyncHealth | undefined, nowSeconds: number): string | undefined {
  if (!sync || sync.condition === "idle" || sync.condition === "current") return undefined;
  const stopped = sync.failed_step ? `Stopped at ${sync.failed_step}` : "Stopped";
  if (sync.next_attempt_at == null) return stopped;
  return sync.next_attempt_at <= nowSeconds ? `${stopped} · trying again now` : `${stopped} · next try in ${span(sync.next_attempt_at - nowSeconds)}`;
}

/** A member unheard from for longer than this is behind on more than pacing: the longest backoff is five minutes. */
export const MEMBER_SILENCE_SECONDS = 10 * 60;

/**
 * When a member last reached the Keeper, for the Keeper's roster.
 *
 * ⚠️ WITHOUT THIS A STALE VERSION LOOKED CURRENT. The roster showed the last
 * release a member reported, and a member that had stopped reaching the Keeper
 * went on "running" it indefinitely — an operator saw 1.16.2 on a Hive already
 * on 1.16.5 and had no way to tell the report was days old.
 */
export function memberContact(member: ApiaryMember, nowSeconds: number): { label: string; silent: boolean } {
  if (member.last_contact_at == null) return { label: "no contact recorded", silent: true };
  const since = Math.max(0, nowSeconds - member.last_contact_at);
  return { label: since < 90 ? "heard from just now" : `heard from ${span(since)} ago`, silent: since > MEMBER_SILENCE_SECONDS };
}

/**
 * What a watch or takeover window says at once when its Hive has gone quiet.
 *
 * ⚠️ OPERATOR RULING, 2026-09-30: "Warn when I use it." A Hive that has not
 * reached the Keeper cannot accept anything, so the window used to wait out its
 * whole minute and then fail. The roster's amber line only helps someone who
 * looked at it first. Said at the moment of use instead, and nowhere else: a
 * laptop that sleeps every night should not raise every morning.
 */
export function silenceWarning(member: ApiaryMember, nowSeconds: number): string | undefined {
  if (!memberContact(member, nowSeconds).silent) return undefined;
  const since = member.last_contact_at == null
    ? "has not reached this Keeper since it began recording contact"
    : `was last heard from ${span(Math.max(0, nowSeconds - member.last_contact_at))} ago`;
  return `${member.hive_name} ${since}, so it may be asleep or offline. This opens only once it reconnects.`;
}

export function catalogReadinessLabel(catalog?: FederationCatalogReadiness) {
  if (!catalog?.acknowledgement) return "Waiting";
  if (catalog.blockers.includes("catalog_stale")) return "Refresh needed";
  if (catalog.blockers.includes("policy_revision_changed")) return "Policy changed";
  if (catalog.blockers.includes("catalog_missing")) return "Waiting";
  return "Verified";
}

export function jiraSetupLabel(connection?: FederationCatalogReadiness["jira_connection"]) {
  if (!connection) return "Checking";
  return ({ ready: "Connected", not_connected: "Not connected (optional)",
    network_unavailable: "Temporarily unavailable", credentials_invalid: "Sign-in needed",
    permission_denied: "Access needs review" } as const)[connection];
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
