//! Typed local-protocol adapter. Authority and effects stay in the engine gate.

use swarm_domain::{TerminalControlError, WorkerSessionId};

use crate::{HostResponse, SessionRegistry, SessionRegistryError, TerminalControlCommand};

/// Executes a negotiated control command without a legacy-write fallback.
#[must_use]
pub fn dispatch_terminal_control(
    registry: &SessionRegistry,
    session_id: WorkerSessionId,
    command: TerminalControlCommand,
) -> HostResponse {
    let result = (|| {
        let session = registry.get(session_id)?;
        match command {
            TerminalControlCommand::Status => {}
            TerminalControlCommand::Claim {
                identity,
                observed_generation,
                size,
            } => {
                registry.claim_control(session_id, identity, observed_generation, size)?;
            }
            TerminalControlCommand::Renew {
                identity,
                generation,
            } => {
                session.renew_control(identity, generation)?;
            }
            TerminalControlCommand::Release {
                identity,
                generation,
            } => {
                session.release_control(identity, generation)?;
            }
            TerminalControlCommand::Input {
                identity,
                generation,
                bytes,
            } => {
                registry.write_controlled(session_id, identity, generation, &bytes)?;
            }
            TerminalControlCommand::Resize {
                identity,
                generation,
                size,
            } => {
                session.resize_controlled(identity, generation, size)?;
            }
        }
        session.control_wire_status()
    })();
    match result {
        Ok(control) => HostResponse::Control {
            session_id,
            control,
        },
        Err(error) => HostResponse::Error {
            code: error_code(&error).into(),
            message: error.to_string(),
        },
    }
}

fn error_code(error: &SessionRegistryError) -> &'static str {
    match error {
        SessionRegistryError::ControlDenied(reason) => match reason {
            TerminalControlError::OwnedElsewhere => "terminal_control_owned_elsewhere",
            TerminalControlError::StaleGeneration => "terminal_control_stale",
            TerminalControlError::Expired => "terminal_control_expired",
            TerminalControlError::Exhausted => "terminal_control_exhausted",
        },
        SessionRegistryError::ControlGenerationRequired => "terminal_control_required",
        SessionRegistryError::InvalidControlInput => "terminal_input_invalid",
        SessionRegistryError::SessionNotFound => "terminal_session_not_found",
        SessionRegistryError::TakeoverDenied | SessionRegistryError::TakeoverConflict => {
            "terminal_takeover_active"
        }
        _ => "terminal_operation_failed",
    }
}

#[cfg(all(test, any(unix, windows)))]
mod tests {
    use super::*;
    use crate::{
        MAX_CONTROL_INPUT_BYTES, TerminalControlStatus, TerminalSize,
        process::control_tests::fixture,
    };
    use swarm_domain::{PresenceDeviceId, TerminalControlIdentity, TerminalViewId};

    fn identity() -> TerminalControlIdentity {
        TerminalControlIdentity {
            device: PresenceDeviceId::new(),
            view: TerminalViewId::new(),
        }
    }

    fn control(
        response: &HostResponse,
        expected_session: WorkerSessionId,
    ) -> TerminalControlStatus {
        let HostResponse::Control {
            session_id,
            control,
        } = response
        else {
            panic!("expected control response: {response:?}")
        };
        assert_eq!(*session_id, expected_session);
        *control
    }

    #[test]
    fn typed_dispatch_performs_handoff_without_falling_back_on_stale_input() {
        let (registry, session) = fixture();
        let id = session.id();
        let desktop = identity();
        let phone = identity();
        let claim = |identity, observed_generation| TerminalControlCommand::Claim {
            identity,
            observed_generation,
            size: TerminalSize::new(24, 80),
        };
        let initial = control(
            &dispatch_terminal_control(&registry, id, TerminalControlCommand::Status),
            id,
        );
        assert_eq!(initial.owner, None);
        let first = control(
            &dispatch_terminal_control(&registry, id, claim(desktop, None)),
            id,
        );
        assert_eq!(first.owner, Some(desktop));
        assert!(first.lease_remaining_ms > 0 && first.lease_remaining_ms <= 90_000);
        let second = control(
            &dispatch_terminal_control(&registry, id, claim(phone, Some(first.generation))),
            id,
        );
        assert_eq!(second.owner, Some(phone));
        let stale = dispatch_terminal_control(
            &registry,
            id,
            TerminalControlCommand::Input {
                identity: desktop,
                generation: first.generation,
                bytes: b"stale".to_vec(),
            },
        );
        assert!(
            matches!(stale, HostResponse::Error { code, .. } if code == "terminal_control_stale")
        );
        let renewed = control(
            &dispatch_terminal_control(
                &registry,
                id,
                TerminalControlCommand::Renew {
                    identity: phone,
                    generation: second.generation,
                },
            ),
            id,
        );
        assert_eq!(renewed.generation, second.generation);
        let released = control(
            &dispatch_terminal_control(
                &registry,
                id,
                TerminalControlCommand::Release {
                    identity: phone,
                    generation: second.generation,
                },
            ),
            id,
        );
        assert_eq!(released.owner, None);
        assert_eq!(released.lease_remaining_ms, 0);
        assert!(released.generation > second.generation);
        assert!(session.is_running().unwrap());
    }

    #[test]
    fn empty_and_oversized_inputs_cannot_renew_or_reach_the_pty() {
        let (registry, session) = fixture();
        let id = session.id();
        let identity = identity();
        let initial = session
            .claim_control(identity, None, TerminalSize::new(24, 80))
            .unwrap();
        for bytes in [vec![], vec![b'x'; MAX_CONTROL_INPUT_BYTES + 1]] {
            let response = dispatch_terminal_control(
                &registry,
                id,
                TerminalControlCommand::Input {
                    identity,
                    generation: initial.generation,
                    bytes,
                },
            );
            assert!(
                matches!(response, HostResponse::Error { code, .. } if code == "terminal_input_invalid")
            );
        }
        assert_eq!(session.control_status().unwrap().1, Some(initial));
        assert!(registry.recent_write_audit(10).unwrap().is_empty());
    }

    #[test]
    fn unknown_session_is_not_redirected_to_another_running_worker() {
        let (registry, session) = fixture();
        let response = dispatch_terminal_control(
            &registry,
            WorkerSessionId::new(),
            TerminalControlCommand::Status,
        );
        assert!(
            matches!(response, HostResponse::Error { code, .. } if code == "terminal_session_not_found")
        );
        assert_eq!(session.control_status().unwrap().1, None);
    }
}
