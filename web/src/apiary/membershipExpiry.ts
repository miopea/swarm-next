import type { ApiaryMember } from "../api";

const DAY = 86_400;
/** How long an expired member may still renew itself (ADR 0117; swarm_domain::MEMBERSHIP_RENEWAL_GRACE_SECONDS). */
const RENEWAL_GRACE = 60 * DAY;
/** A member on a release that renews does so with fifteen days left, so one inside fourteen is not renewing. */
const WARN_WITHIN = 14 * DAY;

/**
 * What the roster says about a member's membership credential, or nothing when
 * it is in good standing (ADR 0117).
 *
 * ⚠️ AN EXPIRED MEMBER IS NOT A SILENT ONE. Before renewal existed every member
 * lapsed thirty days after joining, and the roster blamed the Hive: "hasn't
 * reported to the Keeper yet". The expiry is the cause, so it is what is said,
 * with what will bring it back.
 */
export function membershipNote(member: ApiaryMember, nowSeconds: number): { text: string; expired: boolean } | undefined {
  const expires = member.credential_expires_at;
  if (member.is_local || expires === undefined || expires === null) return undefined;
  if (expires <= nowSeconds) {
    return {
      expired: true,
      text: nowSeconds < expires + RENEWAL_GRACE
        ? `Membership expired ${day(expires)}. It reconnects by itself once it updates, until ${day(expires + RENEWAL_GRACE)}.`
        : `Membership expired ${day(expires)}, too long ago to renew. Remove it and invite it again.`,
    };
  }
  if (expires - nowSeconds <= WARN_WITHIN) {
    return { expired: false, text: `Membership expires ${day(expires)}. It renews by itself once it updates.` };
  }
  return undefined;
}

function day(seconds: number): string {
  return new Intl.DateTimeFormat(undefined, { month: "short", day: "numeric" }).format(new Date(seconds * 1000));
}
