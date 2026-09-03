//! Single interactive owner of a terminal session. The engine owns this value;
//! adapters must serialize its transitions with the corresponding PTY operation.
//! Time is an engine-local monotonic millisecond tick, never browser time.

use crate::{PresenceDeviceId, TerminalViewId};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct TerminalControlIdentity {
    pub device: PresenceDeviceId,
    pub view: TerminalViewId,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TerminalControlGrant {
    pub identity: TerminalControlIdentity,
    pub generation: u64,
    pub expires_at_ms: u64,
}

/// Presence refreshes never shorten the protection earned by actual typing.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TerminalControlPresence {
    Viewing,
    Typing,
}

impl TerminalControlPresence {
    const fn lease_ms(self) -> u64 {
        match self {
            Self::Viewing => 90_000,
            Self::Typing => 300_000,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TerminalControlError {
    OwnedElsewhere,
    StaleGeneration,
    Expired,
    Exhausted,
}

/// Constant space: one owner and one revision, independent of attachment count.
///
/// Disconnect is intentionally not a transition. Only an authenticated foreground
/// attachment may acquire/renew; passive readers never call these operations.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct TerminalControl {
    generation: u64,
    owner: Option<TerminalControlGrant>,
}

impl TerminalControl {
    #[must_use]
    pub const fn generation(&self) -> u64 {
        self.generation
    }

    #[must_use]
    pub fn owner(&self, now_ms: u64) -> Option<TerminalControlGrant> {
        self.owner.filter(|owner| now_ms < owner.expires_at_ms)
    }

    /// Automatically resumes only an unowned session or this exact view.
    ///
    /// # Errors
    /// Refuses another live owner and fails closed on counter/time overflow.
    pub fn acquire(
        &mut self,
        identity: TerminalControlIdentity,
        now_ms: u64,
    ) -> Result<TerminalControlGrant, TerminalControlError> {
        if let Some(owner) = self.owner(now_ms) {
            if owner.identity != identity {
                return Err(TerminalControlError::OwnedElsewhere);
            }
            return self.renew(
                identity,
                owner.generation,
                now_ms,
                TerminalControlPresence::Viewing,
            );
        }
        self.replace_owner(identity, now_ms)
    }

    /// Explicit Resume Here is compare-and-swap, not a delayed unconditional steal.
    ///
    /// # Errors
    /// A request based on an old revision cannot undo a newer takeover/release.
    /// Also fails closed on counter/time overflow.
    pub fn take_over(
        &mut self,
        identity: TerminalControlIdentity,
        observed_generation: u64,
        now_ms: u64,
    ) -> Result<TerminalControlGrant, TerminalControlError> {
        if observed_generation != self.generation {
            return Err(TerminalControlError::StaleGeneration);
        }
        if self
            .owner(now_ms)
            .is_some_and(|owner| owner.identity == identity)
        {
            return self.renew(
                identity,
                observed_generation,
                now_ms,
                TerminalControlPresence::Viewing,
            );
        }
        self.replace_owner(identity, now_ms)
    }

    /// Checks the exact view and generation immediately before input or resize.
    ///
    /// # Errors
    /// Rejects stale generations, other views (including same-device popouts),
    /// missing owners, and expired leases. It never acquires ownership implicitly.
    pub fn authorize(
        &self,
        identity: TerminalControlIdentity,
        generation: u64,
        now_ms: u64,
    ) -> Result<(), TerminalControlError> {
        if generation != self.generation {
            return Err(TerminalControlError::StaleGeneration);
        }
        let Some(owner) = self.owner else {
            return Err(TerminalControlError::OwnedElsewhere);
        };
        if owner.identity != identity {
            return Err(TerminalControlError::OwnedElsewhere);
        }
        if now_ms >= owner.expires_at_ms {
            return Err(TerminalControlError::Expired);
        }
        Ok(())
    }

    /// # Errors
    /// As `authorize`; overflow leaves the previous grant unchanged.
    pub fn renew(
        &mut self,
        identity: TerminalControlIdentity,
        generation: u64,
        now_ms: u64,
        presence: TerminalControlPresence,
    ) -> Result<TerminalControlGrant, TerminalControlError> {
        self.authorize(identity, generation, now_ms)?;
        let expires_at_ms = now_ms
            .checked_add(presence.lease_ms())
            .ok_or(TerminalControlError::Exhausted)?;
        let owner = self
            .owner
            .as_mut()
            .ok_or(TerminalControlError::OwnedElsewhere)?;
        owner.expires_at_ms = owner.expires_at_ms.max(expires_at_ms);
        Ok(*owner)
    }

    /// # Errors
    /// Only the current unexpired owner can release; overflow fails closed.
    pub fn release(
        &mut self,
        identity: TerminalControlIdentity,
        generation: u64,
        now_ms: u64,
    ) -> Result<(), TerminalControlError> {
        self.authorize(identity, generation, now_ms)?;
        let next = self
            .generation
            .checked_add(1)
            .ok_or(TerminalControlError::Exhausted)?;
        self.generation = next;
        self.owner = None;
        Ok(())
    }

    fn replace_owner(
        &mut self,
        identity: TerminalControlIdentity,
        now_ms: u64,
    ) -> Result<TerminalControlGrant, TerminalControlError> {
        let generation = self
            .generation
            .checked_add(1)
            .ok_or(TerminalControlError::Exhausted)?;
        let expires_at_ms = now_ms
            .checked_add(TerminalControlPresence::Viewing.lease_ms())
            .ok_or(TerminalControlError::Exhausted)?;
        let owner = TerminalControlGrant {
            identity,
            generation,
            expires_at_ms,
        };
        self.owner = Some(owner);
        self.generation = generation;
        Ok(owner)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn view(device: PresenceDeviceId) -> TerminalControlIdentity {
        TerminalControlIdentity {
            device,
            view: TerminalViewId::new(),
        }
    }

    #[test]
    fn passive_read_does_not_acquire_and_first_foreground_view_can_resume() {
        let mut control = TerminalControl::default();
        assert_eq!(control.owner(0), None);
        assert_eq!(control.generation(), 0);
        let desktop = view(PresenceDeviceId::new());
        let grant = control.acquire(desktop, 0).unwrap();
        assert_eq!(grant.generation, 1);
        assert_eq!(control.authorize(desktop, grant.generation, 1), Ok(()));
    }

    #[test]
    fn another_device_and_another_window_cannot_implicitly_take_over() {
        let mut control = TerminalControl::default();
        let desktop = view(PresenceDeviceId::new());
        control.acquire(desktop, 0).unwrap();
        for other in [view(desktop.device), view(PresenceDeviceId::new())] {
            assert_eq!(
                control.acquire(other, 1),
                Err(TerminalControlError::OwnedElsewhere)
            );
            assert_eq!(
                control.authorize(other, 1, 1),
                Err(TerminalControlError::OwnedElsewhere)
            );
        }
    }

    #[test]
    fn takeover_rejects_old_input_resize_renewal_and_release() {
        let mut control = TerminalControl::default();
        let desktop = view(PresenceDeviceId::new());
        let phone = view(PresenceDeviceId::new());
        let old = control.acquire(desktop, 0).unwrap();
        let new = control.take_over(phone, old.generation, 1).unwrap();
        assert!(new.generation > old.generation);
        assert_eq!(
            control.authorize(desktop, old.generation, 2),
            Err(TerminalControlError::StaleGeneration)
        );
        assert_eq!(
            control.renew(desktop, old.generation, 2, TerminalControlPresence::Typing),
            Err(TerminalControlError::StaleGeneration)
        );
        assert_eq!(
            control.release(desktop, old.generation, 2),
            Err(TerminalControlError::StaleGeneration)
        );
        assert_eq!(control.authorize(phone, new.generation, 2), Ok(()));
    }

    #[test]
    fn delayed_explicit_claim_cannot_reverse_a_newer_takeover() {
        let mut control = TerminalControl::default();
        let desktop = view(PresenceDeviceId::new());
        let phone = view(PresenceDeviceId::new());
        control.acquire(desktop, 0).unwrap();
        control.take_over(phone, 1, 1).unwrap();
        assert_eq!(
            control.take_over(desktop, 1, 2),
            Err(TerminalControlError::StaleGeneration)
        );
        assert_eq!(control.owner(2).unwrap().identity, phone);
    }

    #[test]
    fn reconnect_to_same_engine_owner_preserves_generation() {
        let mut control = TerminalControl::default();
        let desktop = view(PresenceDeviceId::new());
        let original = control.acquire(desktop, 0).unwrap();
        let resumed = control.acquire(desktop, 5_000).unwrap();
        assert_eq!(original.generation, resumed.generation);
        assert_eq!(resumed.expires_at_ms, 95_000);
    }

    #[test]
    fn expired_input_fails_and_reacquisition_invalidates_old_commands() {
        let mut control = TerminalControl::default();
        let desktop = view(PresenceDeviceId::new());
        let grant = control.acquire(desktop, 0).unwrap();
        assert_eq!(control.owner(90_000), None);
        assert_eq!(
            control.authorize(desktop, grant.generation, 90_000),
            Err(TerminalControlError::Expired)
        );
        let resumed = control.acquire(desktop, 90_000).unwrap();
        assert!(resumed.generation > grant.generation);
        assert_eq!(
            control.authorize(desktop, grant.generation, 90_001),
            Err(TerminalControlError::StaleGeneration)
        );
    }

    #[test]
    fn typing_extends_lease_and_viewing_does_not_shorten_it() {
        let mut control = TerminalControl::default();
        let desktop = view(PresenceDeviceId::new());
        let grant = control.acquire(desktop, 0).unwrap();
        let typed = control
            .renew(
                desktop,
                grant.generation,
                1,
                TerminalControlPresence::Typing,
            )
            .unwrap();
        let viewed = control
            .renew(
                desktop,
                grant.generation,
                2,
                TerminalControlPresence::Viewing,
            )
            .unwrap();
        assert_eq!(typed.expires_at_ms, 300_001);
        assert_eq!(viewed.expires_at_ms, typed.expires_at_ms);
    }

    #[test]
    fn release_revokes_outstanding_claims_and_allows_automatic_resume() {
        let mut control = TerminalControl::default();
        let desktop = view(PresenceDeviceId::new());
        let phone = view(PresenceDeviceId::new());
        control.acquire(desktop, 0).unwrap();
        control.release(desktop, 1, 1).unwrap();
        assert_eq!(control.owner(1), None);
        assert_eq!(
            control.take_over(phone, 1, 2),
            Err(TerminalControlError::StaleGeneration)
        );
        assert_eq!(control.acquire(phone, 2).unwrap().generation, 3);
    }

    #[test]
    fn unsuccessful_pty_operation_can_discard_proposed_transition() {
        let mut current = TerminalControl::default();
        let desktop = view(PresenceDeviceId::new());
        let phone = view(PresenceDeviceId::new());
        current.acquire(desktop, 0).unwrap();
        let mut proposed = current;
        proposed.take_over(phone, 1, 1).unwrap();
        // Engine commits this copy only after the PTY geometry operation succeeds.
        assert_eq!(current.owner(1).unwrap().identity, desktop);
        assert_eq!(current.authorize(desktop, 1, 1), Ok(()));
    }

    #[test]
    fn exhausted_counter_or_clock_cannot_reuse_a_generation_or_mutate_owner() {
        let desktop = view(PresenceDeviceId::new());
        for (mut control, now) in [
            (TerminalControl::default(), u64::MAX),
            (
                TerminalControl {
                    generation: u64::MAX,
                    owner: None,
                },
                0,
            ),
        ] {
            let before = control;
            assert_eq!(
                control.acquire(desktop, now),
                Err(TerminalControlError::Exhausted)
            );
            assert_eq!(control, before);
        }
    }

    #[test]
    fn renewal_overflow_preserves_current_lease() {
        let mut control = TerminalControl::default();
        let desktop = view(PresenceDeviceId::new());
        let now = u64::MAX - 100_000;
        let grant = control.acquire(desktop, now).unwrap();
        let before = control;
        assert_eq!(
            control.renew(
                desktop,
                grant.generation,
                now + 1,
                TerminalControlPresence::Typing
            ),
            Err(TerminalControlError::Exhausted)
        );
        assert_eq!(control, before);
    }

    #[test]
    fn repeated_current_view_claim_is_idempotent_for_generation() {
        let mut control = TerminalControl::default();
        let desktop = view(PresenceDeviceId::new());
        let grant = control.acquire(desktop, 0).unwrap();
        assert_eq!(
            control
                .take_over(desktop, grant.generation, 1)
                .unwrap()
                .generation,
            grant.generation
        );
    }
}
