//! When an Apiary member renews its membership credential (ADR 0117).
//!
//! A member's credential lasts thirty days from when it was issued. Before
//! this, nothing renewed it, so every member fell out of its Apiary a month
//! after joining while its Keeper's roster said it had simply not reported.

/// How long before its credential expires a member starts renewing it. Half
/// the lifetime, so a Hive that is off for a fortnight still renews in time.
pub const MEMBERSHIP_RENEWAL_WINDOW_SECONDS: i64 = 15 * 86_400;

/// How long after its credential expired a member may still renew it, and
/// only renew it (the operator's ruling, decision recorded on task 01a13c1a:
/// "Renews itself, 60-day grace"). A Keeper that removed the Hive ends this.
pub const MEMBERSHIP_RENEWAL_GRACE_SECONDS: i64 = 60 * 86_400;

/// Whether a member should renew now: inside the window before expiry, or
/// within the grace period after it. Past the grace period only a new
/// invitation brings it back, so asking would only be refused.
#[must_use]
pub const fn membership_renewal_due(credential_expires_at: i64, now: i64) -> bool {
    now >= credential_expires_at.saturating_sub(MEMBERSHIP_RENEWAL_WINDOW_SECONDS)
        && membership_renewable(credential_expires_at, now)
}

/// Whether a Keeper may renew a credential that expires, or expired, at
/// `credential_expires_at`.
#[must_use]
pub const fn membership_renewable(credential_expires_at: i64, now: i64) -> bool {
    now < credential_expires_at.saturating_add(MEMBERSHIP_RENEWAL_GRACE_SECONDS)
}

#[cfg(test)]
mod tests {
    use super::*;

    const EXPIRES: i64 = 1_800_000_000;
    const DAY: i64 = 86_400;

    #[test]
    fn a_member_renews_in_the_last_fortnight_and_for_sixty_days_after() {
        assert!(!membership_renewal_due(EXPIRES, EXPIRES - 16 * DAY));
        assert!(membership_renewal_due(EXPIRES, EXPIRES - 15 * DAY));
        assert!(membership_renewal_due(EXPIRES, EXPIRES - 1));
        assert!(
            membership_renewal_due(EXPIRES, EXPIRES + 59 * DAY),
            "an expired member still renews within the grace period"
        );
        assert!(!membership_renewal_due(EXPIRES, EXPIRES + 60 * DAY));
    }

    #[test]
    fn a_keeper_renews_any_credential_until_the_grace_period_ends() {
        assert!(
            membership_renewable(EXPIRES, EXPIRES - 29 * DAY),
            "early is allowed; the member decides when"
        );
        assert!(membership_renewable(EXPIRES, EXPIRES + 60 * DAY - 1));
        assert!(!membership_renewable(EXPIRES, EXPIRES + 60 * DAY));
    }
}
