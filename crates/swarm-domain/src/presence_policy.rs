use crate::{
    OperatorPresence, PresenceDeviceClass, PresenceMode, PresenceObservationState, PresenceSource,
};

/// One fresh activity lease, shared by observation expiry and hidden-tab bridging.
pub const PRESENCE_ACTIVE_TTL_SECONDS: i64 = 150;

/// Content-free observation; the persistence owner admits at most sixteen devices.
#[derive(Clone, Copy, Debug)]
pub struct PresenceDeviceEvidence {
    pub device_class: PresenceDeviceClass,
    pub state: PresenceObservationState,
    pub expires_at: i64,
    pub last_active_at: Option<i64>,
}

/// Automatic presence only. Manual and scheduled policy is applied before this.
/// Reachable is availability policy, not proof that a phone is online.
#[must_use]
pub fn derive_device_presence(devices: &[PresenceDeviceEvidence], now: i64) -> OperatorPresence {
    let mut best = 5;
    for device in devices.iter().filter(|device| device.expires_at > now) {
        let active = device.state == PresenceObservationState::Active
            || (device.state == PresenceObservationState::Hidden
                && device.last_active_at.is_some_and(|at| {
                    at <= now && at.saturating_add(PRESENCE_ACTIVE_TTL_SECONDS) > now
                }));
        let rank = if active && device.device_class == PresenceDeviceClass::Desktop {
            0
        } else if device.state == PresenceObservationState::Locked {
            1
        } else if active {
            2
        } else {
            3
        };
        best = best.min(rank);
    }
    OperatorPresence {
        mode: if best == 0 {
            PresenceMode::AtHive
        } else {
            PresenceMode::Reachable
        },
        manual_mode: None,
        source: match best {
            0 | 2 => PresenceSource::ActiveDevice,
            1 => PresenceSource::ScreenLocked,
            3 => PresenceSource::InactiveDevice,
            _ => PresenceSource::TimedOut,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reachable_preserves_installed_wire_and_storage_contracts() {
        assert_eq!(PresenceMode::Reachable.to_string(), "away");
        assert_eq!(
            serde_json::to_string(&PresenceMode::Reachable).unwrap(),
            "\"away\""
        );
        assert_eq!(
            serde_json::from_str::<PresenceMode>("\"away\"").unwrap(),
            PresenceMode::Reachable
        );
        assert_eq!(
            "away".parse::<PresenceMode>().unwrap(),
            PresenceMode::Reachable
        );
    }

    #[test]
    fn phone_activity_does_not_claim_desktop_engagement() {
        let phone = PresenceDeviceEvidence {
            device_class: PresenceDeviceClass::Mobile,
            state: PresenceObservationState::Active,
            expires_at: 200,
            last_active_at: Some(100),
        };
        let locked = PresenceDeviceEvidence {
            device_class: PresenceDeviceClass::Desktop,
            state: PresenceObservationState::Locked,
            ..phone
        };
        assert_eq!(
            derive_device_presence(&[phone], 101).mode,
            PresenceMode::Reachable
        );
        let presence = derive_device_presence(&[phone, locked], 101);
        assert_eq!(presence.mode, PresenceMode::Reachable);
        assert_eq!(presence.source, PresenceSource::ScreenLocked);
        assert_eq!(derive_device_presence(&[locked, phone], 101), presence);
        let desktop = PresenceDeviceEvidence {
            device_class: PresenceDeviceClass::Desktop,
            ..phone
        };
        assert_eq!(
            derive_device_presence(&[phone, desktop, locked], 101).mode,
            PresenceMode::AtHive
        );
        assert_eq!(
            derive_device_presence(&[desktop], 200).source,
            PresenceSource::TimedOut
        );
    }

    #[test]
    fn hidden_activity_is_bounded_and_never_overrides_explicit_lock() {
        let device = PresenceDeviceEvidence {
            device_class: PresenceDeviceClass::Desktop,
            state: PresenceObservationState::Hidden,
            expires_at: 400,
            last_active_at: Some(100),
        };
        assert_eq!(
            derive_device_presence(&[device], 249).mode,
            PresenceMode::AtHive
        );
        assert_eq!(
            derive_device_presence(&[device], 250).mode,
            PresenceMode::Reachable
        );
        for state in [
            PresenceObservationState::Locked,
            PresenceObservationState::Idle,
        ] {
            assert_eq!(
                derive_device_presence(&[PresenceDeviceEvidence { state, ..device }], 101).mode,
                PresenceMode::Reachable
            );
        }
        assert_eq!(
            derive_device_presence(&[device], 99).mode,
            PresenceMode::Reachable
        );
    }
}
