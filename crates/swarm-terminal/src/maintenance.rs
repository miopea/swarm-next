//! All-session admission, not a screen census or a reusable permission token.
use serde::{Deserialize, Serialize};
use swarm_domain::WorkerSessionId;

use crate::control_gate::{MaintenanceHoldError, TerminalControlGate};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MaintenanceRefusal {
    NotDraining,
    ReturnSetMismatch,
    InputOrStopInFlight,
    LockPoisoned,
    SessionChanged,
    InteractiveOwner,
    ProviderEvidenceUnavailable,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum MaintenanceOutcome {
    Refused {
        reason: MaintenanceRefusal,
        session_id: Option<WorkerSessionId>,
    },
    Stopped {
        session_ids: Vec<WorkerSessionId>,
    },
    Partial {
        stopped: Vec<WorkerSessionId>,
        failed: WorkerSessionId,
    },
}

impl MaintenanceOutcome {
    pub(crate) const fn refused(
        reason: MaintenanceRefusal,
        session_id: Option<WorkerSessionId>,
    ) -> Self {
        Self::Refused { reason, session_id }
    }
}

/// Membership is frozen by the registry; order is stable. No callback may
/// reenter an input/control or explicit-stop gate. No output lock spans a stop.
/// A failed hold or eligibility check releases ALL guards without any stop.
pub(crate) fn stop_set<E>(
    sessions: &[(WorkerSessionId, &TerminalControlGate)],
    eligibility: impl Fn(usize) -> Result<(), MaintenanceRefusal>,
    stop: impl Fn(usize) -> Result<(), E>,
) -> MaintenanceOutcome {
    let mut guards = Vec::with_capacity(sessions.len());
    for &(id, gate) in sessions {
        match gate.try_maintenance() {
            Ok(guard) => guards.push(guard),
            Err(error) => {
                let reason = match error {
                    MaintenanceHoldError::InFlight => MaintenanceRefusal::InputOrStopInFlight,
                    MaintenanceHoldError::Poisoned => MaintenanceRefusal::LockPoisoned,
                    MaintenanceHoldError::Stopped => MaintenanceRefusal::SessionChanged,
                };
                return MaintenanceOutcome::refused(reason, Some(id));
            }
        }
    }
    for (index, guard) in guards.iter().enumerate() {
        let reason = if guard.has_owner() {
            Err(MaintenanceRefusal::InteractiveOwner)
        } else {
            eligibility(index)
        };
        if let Err(reason) = reason {
            return MaintenanceOutcome::refused(reason, Some(sessions[index].0));
        }
    }
    let mut stopped = Vec::with_capacity(sessions.len());
    for (index, guard) in guards.iter().enumerate() {
        if guard.stop(|| stop(index)).is_err() {
            return MaintenanceOutcome::Partial {
                stopped,
                failed: sessions[index].0,
            };
        }
        stopped.push(sessions[index].0);
    }
    MaintenanceOutcome::Stopped {
        session_ids: stopped,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::control_gate::ControlGateError;
    use std::{cell::Cell, sync::Barrier};
    use swarm_domain::{PresenceDeviceId, TerminalControlIdentity, TerminalViewId};

    fn controls(gates: &[TerminalControlGate]) -> Vec<(WorkerSessionId, &TerminalControlGate)> {
        gates
            .iter()
            .map(|gate| (WorkerSessionId::new(), gate))
            .collect()
    }

    #[test]
    fn maintenance_checks_all_sessions_before_stopping_any_and_releases_on_refusal() {
        let gates = [
            TerminalControlGate::default(),
            TerminalControlGate::default(),
        ];
        let sessions = controls(&gates);
        let stops = Cell::new(0);
        let result = stop_set(
            &sessions,
            |index| {
                // Every guard must already be held, even while checking session 0.
                assert!(gates.iter().all(|gate| gate.try_maintenance().is_err()));
                if index == 1 {
                    Err(MaintenanceRefusal::ProviderEvidenceUnavailable)
                } else {
                    Ok(())
                }
            },
            |_| {
                stops.set(stops.get() + 1);
                Ok::<_, ()>(())
            },
        );
        assert_eq!(
            result,
            MaintenanceOutcome::refused(
                MaintenanceRefusal::ProviderEvidenceUnavailable,
                Some(sessions[1].0)
            )
        );
        assert_eq!(stops.get(), 0);
        for gate in &gates {
            gate.legacy(false, || Ok::<_, ()>(())).unwrap();
        }
    }

    #[test]
    fn maintenance_owner_refuses_and_explicit_release_allows_new_admission() {
        let gates = [
            TerminalControlGate::default(),
            TerminalControlGate::default(),
        ];
        let sessions = controls(&gates);
        let identity = TerminalControlIdentity {
            device: PresenceDeviceId::new(),
            view: TerminalViewId::new(),
        };
        let grant = gates[1].claim(identity, None, || Ok::<_, ()>(())).unwrap();
        let refused = stop_set(
            &sessions,
            |_| Ok(()),
            |_| -> Result<(), ()> { panic!("owner must prevent every stop") },
        );
        assert_eq!(
            refused,
            MaintenanceOutcome::refused(MaintenanceRefusal::InteractiveOwner, Some(sessions[1].0))
        );
        gates[1].release(identity, grant.generation).unwrap();
        assert_eq!(
            stop_set(&sessions, |_| Ok(()), |_| Ok::<_, ()>(())),
            MaintenanceOutcome::Stopped {
                session_ids: sessions.iter().map(|s| s.0).collect()
            }
        );
    }

    #[test]
    fn maintenance_refuses_an_inflight_write_without_waiting_or_partial_stop() {
        let gates = [
            TerminalControlGate::default(),
            TerminalControlGate::default(),
        ];
        let sessions = controls(&gates);
        let entered = Barrier::new(2);
        let release = Barrier::new(2);
        let (send, receive) = std::sync::mpsc::channel();
        std::thread::scope(|scope| {
            let writer = scope.spawn(|| {
                gates[1].legacy(false, || {
                    entered.wait();
                    release.wait();
                    Ok::<_, ()>(())
                })
            });
            entered.wait();
            let admission = scope.spawn(|| {
                let result = stop_set(&sessions, |_| Ok(()), |_| Ok::<_, ()>(()));
                send.send(result).unwrap();
            });
            // The deadline only bounds test failure. The held writer and channel
            // receipt prove refusal without waiting; elapsed time grants nothing.
            let result = receive.recv_timeout(std::time::Duration::from_secs(2));
            release.wait();
            writer.join().unwrap().unwrap();
            admission.join().unwrap();
            assert_eq!(
                result.unwrap(),
                MaintenanceOutcome::refused(
                    MaintenanceRefusal::InputOrStopInFlight,
                    Some(sessions[1].0)
                )
            );
        });
        for gate in &gates {
            gate.legacy(false, || Ok::<_, ()>(())).unwrap();
        }
    }

    #[test]
    fn maintenance_winning_race_rejects_new_input_and_control_without_effects() {
        let gates = [TerminalControlGate::default()];
        let sessions = controls(&gates);
        let checked = Barrier::new(2);
        std::thread::scope(|scope| {
            let input = scope.spawn(|| {
                checked.wait();
                gates[0].legacy(true, || -> Result<(), ()> {
                    panic!("no delivery after maintenance")
                })
            });
            let result = stop_set(
                &sessions,
                |_| {
                    checked.wait();
                    Ok(())
                },
                |_| Ok::<_, ()>(()),
            );
            assert!(matches!(result, MaintenanceOutcome::Stopped { .. }));
            assert_eq!(input.join().unwrap(), Err(ControlGateError::Stopped));
        });
        let identity = TerminalControlIdentity {
            device: PresenceDeviceId::new(),
            view: TerminalViewId::new(),
        };
        assert_eq!(
            gates[0].claim(identity, None, || -> Result<(), ()> { panic!("no resize") }),
            Err(ControlGateError::Stopped)
        );
        // Losing an admission response cannot cause an identical stop to repeat.
        assert_eq!(
            stop_set(
                &sessions,
                |_| Ok(()),
                |_| -> Result<(), ()> { panic!("no repeated kill") }
            ),
            MaintenanceOutcome::refused(MaintenanceRefusal::SessionChanged, Some(sessions[0].0))
        );
    }

    #[test]
    fn maintenance_partial_failure_reports_exact_stops_and_releases_remaining_input() {
        let gates = [
            TerminalControlGate::default(),
            TerminalControlGate::default(),
            TerminalControlGate::default(),
        ];
        let sessions = controls(&gates);
        let calls = Cell::new(0);
        let result = stop_set(
            &sessions,
            |_| Ok(()),
            |index| {
                calls.set(calls.get() + 1);
                if index == 1 { Err(()) } else { Ok(()) }
            },
        );
        assert_eq!(
            result,
            MaintenanceOutcome::Partial {
                stopped: vec![sessions[0].0],
                failed: sessions[1].0
            }
        );
        assert_eq!(calls.get(), 2);
        assert_eq!(
            gates[0].legacy(false, || Ok::<_, ()>(())),
            Err(ControlGateError::Stopped)
        );
        for gate in &gates[1..] {
            gate.legacy(false, || Ok::<_, ()>(())).unwrap();
        }
    }
}
