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
    /// No startup evidence: Swarm cannot tell the provider's hooks run.
    ProviderEvidenceUnavailable,
    /// Only Claude's turns are measured well enough to stop it unattended
    /// (ADR 0156).
    NotClaude,
    /// A turn, or a message queued behind one, has not ended.
    TurnInProgress,
    /// The last turn failed and said nothing about what it left.
    TurnFailed,
    /// A question, a permission prompt or a menu is waiting for someone.
    WaitingOnOperator,
    /// The turn reports do not add up, so nothing they say is believed until
    /// the session restarts.
    TurnCountUnknown,
    /// The last turn left background work running, or could not list it.
    BackgroundWork,
    /// The last turn left a session cron, or did not say.
    ScheduledWork,
    /// Something was typed that Claude has not taken as a prompt.
    UnsentInput,
    /// The screen does not show Claude resting at its prompt.
    ScreenNotResting,
}

/// One more session that refused the same admission.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SessionRefusal {
    pub session_id: WorkerSessionId,
    pub reason: MaintenanceRefusal,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum MaintenanceOutcome {
    Refused {
        reason: MaintenanceRefusal,
        session_id: Option<WorkerSessionId>,
        /// Every other session the same admission refused, so whoever asked
        /// can settle them all at once rather than one per admission (ADR
        /// 0162). Additive: an older engine sends none, and an older API
        /// ignores it.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        others: Vec<SessionRefusal>,
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
        Self::Refused {
            reason,
            session_id,
            others: Vec::new(),
        }
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
    // Every session is asked, not only up to the first refusal: a caller that
    // can settle what refused needs to know all of it (ADR 0162). Nothing is
    // stopped unless none refuses.
    let mut refusals = guards.iter().enumerate().filter_map(|(index, guard)| {
        let reason = if guard.has_owner() {
            Err(MaintenanceRefusal::InteractiveOwner)
        } else {
            eligibility(index)
        };
        reason.err().map(|reason| SessionRefusal {
            session_id: sessions[index].0,
            reason,
        })
    });
    if let Some(first) = refusals.next() {
        return MaintenanceOutcome::Refused {
            reason: first.reason,
            session_id: Some(first.session_id),
            others: refusals.collect(),
        };
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

    /// ADR 0162: one admission names every session that refused, so the
    /// caller settles them in one round, and still stops nobody.
    #[test]
    fn maintenance_names_every_refusing_session_and_stops_none() {
        let gates = [
            TerminalControlGate::default(),
            TerminalControlGate::default(),
            TerminalControlGate::default(),
        ];
        let sessions = controls(&gates);
        let asked = Cell::new(0);
        let result = stop_set(
            &sessions,
            |index| {
                asked.set(asked.get() + 1);
                match index {
                    0 => Err(MaintenanceRefusal::TurnCountUnknown),
                    2 => Err(MaintenanceRefusal::ScheduledWork),
                    _ => Ok(()),
                }
            },
            |_| -> Result<(), ()> { panic!("a refusal stops nobody") },
        );
        assert_eq!(
            result,
            MaintenanceOutcome::Refused {
                reason: MaintenanceRefusal::TurnCountUnknown,
                session_id: Some(sessions[0].0),
                others: vec![SessionRefusal {
                    session_id: sessions[2].0,
                    reason: MaintenanceRefusal::ScheduledWork,
                }],
            }
        );
        assert_eq!(
            asked.get(),
            3,
            "every session is asked, past the first refusal"
        );
        for gate in &gates {
            gate.legacy(false, || Ok::<_, ()>(())).unwrap();
        }
        // An older API, which knows nothing of `others`, still reads the answer.
        let wire = serde_json::to_value(&result).unwrap();
        assert_eq!(wire["others"][0]["reason"], "scheduled_work");
        let older = serde_json::json!({"status": "refused", "reason": "turn_in_progress", "session_id": null});
        assert_eq!(
            serde_json::from_value::<MaintenanceOutcome>(older).unwrap(),
            MaintenanceOutcome::refused(MaintenanceRefusal::TurnInProgress, None),
            "an older engine's answer, without `others`, reads as none"
        );
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
