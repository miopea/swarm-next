//! Engine-owned serialization of authority and terminal effects (ADR 0062).
//! Effect callbacks must not reenter this gate. No socket/API lifetime owns it.

use std::{
    convert::Infallible,
    sync::{
        Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Instant,
};

use swarm_domain::{
    TerminalControl, TerminalControlError, TerminalControlGrant, TerminalControlIdentity,
    TerminalControlPresence,
};

#[derive(Debug, Eq, PartialEq)]
pub(crate) enum ControlGateError<E> {
    Authority(TerminalControlError),
    GenerationRequired,
    Poisoned,
    Stopped,
    Interrupted,
    Effect(E),
}

#[derive(Debug)]
pub(crate) struct TerminalControlGate {
    control: Mutex<TerminalControl>,
    stopped: AtomicBool,
    stop_effect: Mutex<()>,
    epoch: Instant,
}

impl Default for TerminalControlGate {
    fn default() -> Self {
        Self {
            control: Mutex::new(TerminalControl::default()),
            stopped: AtomicBool::new(false),
            stop_effect: Mutex::new(()),
            epoch: Instant::now(),
        }
    }
}

impl TerminalControlGate {
    // Effect callers hold the control mutex; explicit stop can cancel a blocked
    // writer without waiting for that mutex. An in-flight effect is checked again.
    fn require_running<E>(&self) -> Result<(), ControlGateError<E>> {
        if self.stopped.load(Ordering::Acquire) {
            return Err(ControlGateError::Stopped);
        }
        Ok(())
    }

    fn complete_effect<E, T>(&self, result: Result<T, E>) -> Result<T, ControlGateError<E>> {
        if self.stopped.load(Ordering::Acquire) {
            return Err(ControlGateError::Interrupted);
        }
        result.map_err(ControlGateError::Effect)
    }

    /// Explicit cancellation, not automatic-maintenance admission. Fence new
    /// effects before killing, without waiting for a potentially blocked PTY
    /// write. In-flight effects report uncertainty rather than a false success.
    /// Stop attempts serialize separately. Failure permits ordinary recovery.
    pub(crate) fn stop<E>(
        &self,
        effect: impl FnOnce() -> Result<(), E>,
    ) -> Result<(), ControlGateError<E>> {
        let _stop = self
            .stop_effect
            .lock()
            .map_err(|_| ControlGateError::Poisoned)?;
        if self.stopped.load(Ordering::Acquire) {
            return Ok(());
        }
        self.stopped.store(true, Ordering::Release);
        match effect() {
            Ok(()) => Ok(()),
            Err(error) => {
                self.stopped.store(false, Ordering::Release);
                Err(ControlGateError::Effect(error))
            }
        }
    }

    fn now(&self) -> u64 {
        u64::try_from(self.epoch.elapsed().as_millis()).unwrap_or(u64::MAX)
    }

    pub(crate) fn status(
        &self,
    ) -> Result<(u64, Option<TerminalControlGrant>), ControlGateError<Infallible>> {
        self.snapshot()
            .map(|(generation, owner, _)| (generation, owner))
    }

    pub(crate) fn snapshot(
        &self,
    ) -> Result<(u64, Option<TerminalControlGrant>, u64), ControlGateError<Infallible>> {
        let control = self
            .control
            .lock()
            .map_err(|_| ControlGateError::Poisoned)?;
        let now = self.now();
        let owner = if self.stopped.load(Ordering::Acquire) {
            None
        } else {
            control.owner(now)
        };
        Ok((control.generation(), owner, now))
    }

    pub(crate) fn claim<E>(
        &self,
        identity: TerminalControlIdentity,
        observed_generation: Option<u64>,
        resize: impl FnOnce() -> Result<(), E>,
    ) -> Result<TerminalControlGrant, ControlGateError<E>> {
        let mut control = self
            .control
            .lock()
            .map_err(|_| ControlGateError::Poisoned)?;
        let mut proposed = *control;
        self.require_running()?;
        let now = self.now();
        let grant = match observed_generation {
            Some(generation) => proposed.take_over(identity, generation, now),
            None => proposed.acquire(identity, now),
        }
        .map_err(ControlGateError::Authority)?;
        self.complete_effect(resize())?;
        *control = proposed;
        Ok(grant)
    }

    pub(crate) fn input<E>(
        &self,
        identity: TerminalControlIdentity,
        generation: u64,
        write: impl FnOnce() -> Result<(), E>,
    ) -> Result<(), ControlGateError<E>> {
        let mut control = self
            .control
            .lock()
            .map_err(|_| ControlGateError::Poisoned)?;
        let mut proposed = *control;
        self.require_running()?;
        proposed
            .renew(
                identity,
                generation,
                self.now(),
                TerminalControlPresence::Typing,
            )
            .map_err(ControlGateError::Authority)?;
        self.complete_effect(write())?;
        *control = proposed;
        Ok(())
    }

    pub(crate) fn resize<E>(
        &self,
        identity: TerminalControlIdentity,
        generation: u64,
        resize: impl FnOnce() -> Result<(), E>,
    ) -> Result<(), ControlGateError<E>> {
        let control = self
            .control
            .lock()
            .map_err(|_| ControlGateError::Poisoned)?;
        self.require_running()?;
        control
            .authorize(identity, generation, self.now())
            .map_err(ControlGateError::Authority)?;
        self.complete_effect(resize())
    }

    pub(crate) fn renew(
        &self,
        identity: TerminalControlIdentity,
        generation: u64,
    ) -> Result<TerminalControlGrant, ControlGateError<Infallible>> {
        let mut control = self
            .control
            .lock()
            .map_err(|_| ControlGateError::Poisoned)?;
        self.require_running()?;
        control
            .renew(
                identity,
                generation,
                self.now(),
                TerminalControlPresence::Viewing,
            )
            .map_err(ControlGateError::Authority)
    }

    pub(crate) fn release(
        &self,
        identity: TerminalControlIdentity,
        generation: u64,
    ) -> Result<(), ControlGateError<Infallible>> {
        self.control
            .lock()
            .map_err(|_| ControlGateError::Poisoned)?
            .release(identity, generation, self.now())
            .map_err(ControlGateError::Authority)
    }

    /// Compatibility ends for operator writes/resizes as soon as a session has
    /// used the new contract, even after expiry/release. Coordination still uses
    /// its existing API authorization but cannot inject under an active owner.
    pub(crate) fn legacy<E>(
        &self,
        coordination: bool,
        effect: impl FnOnce() -> Result<(), E>,
    ) -> Result<(), ControlGateError<E>> {
        let control = self
            .control
            .lock()
            .map_err(|_| ControlGateError::Poisoned)?;
        self.require_running()?;
        if control.generation() != 0 && (!coordination || control.owner(self.now()).is_some()) {
            return Err(ControlGateError::GenerationRequired);
        }
        self.complete_effect(effect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{cell::Cell, sync::TryLockError, time::Duration};
    use swarm_domain::{PresenceDeviceId, TerminalViewId};

    fn identity() -> TerminalControlIdentity {
        TerminalControlIdentity {
            device: PresenceDeviceId::new(),
            view: TerminalViewId::new(),
        }
    }

    fn effect_while_locked(gate: &TerminalControlGate) -> Result<(), &'static str> {
        match gate.control.try_lock() {
            Err(TryLockError::WouldBlock) => Ok(()),
            _ => Err("the authority guard was released before the effect"),
        }
    }

    fn effect_while_stopping(gate: &TerminalControlGate) -> Result<(), &'static str> {
        if !matches!(gate.stop_effect.try_lock(), Err(TryLockError::WouldBlock)) {
            return Err("stop attempt guard was released");
        }
        match gate.legacy(false, || Ok::<_, ()>(())) {
            Err(ControlGateError::Stopped) => Ok(()),
            _ => Err("new input was not fenced"),
        }
    }

    #[test]
    fn authority_guard_spans_claim_input_and_resize_effects() {
        let gate = TerminalControlGate::default();
        let desktop = identity();
        let grant = gate
            .claim(desktop, None, || effect_while_locked(&gate))
            .unwrap();
        gate.input(desktop, grant.generation, || effect_while_locked(&gate))
            .unwrap();
        gate.resize(desktop, grant.generation, || effect_while_locked(&gate))
            .unwrap();
    }

    #[test]
    fn failed_resize_does_not_commit_a_handoff() {
        let gate = TerminalControlGate::default();
        let desktop = identity();
        let old = gate.claim(desktop, None, || Ok::<_, &str>(())).unwrap();
        assert_eq!(
            gate.claim(identity(), Some(old.generation), || Err("resize failed")),
            Err(ControlGateError::Effect("resize failed"))
        );
        assert_eq!(gate.status().unwrap(), (old.generation, Some(old)));
        gate.input(desktop, old.generation, || Ok::<_, &str>(()))
            .unwrap();
    }

    #[test]
    fn stale_commands_never_reach_the_effect() {
        let gate = TerminalControlGate::default();
        let desktop = identity();
        let old = gate.claim(desktop, None, || Ok::<_, &str>(())).unwrap();
        gate.claim(identity(), Some(old.generation), || Ok::<_, &str>(()))
            .unwrap();
        let called = Cell::new(false);
        let effect = || {
            called.set(true);
            Ok::<_, &str>(())
        };
        assert_eq!(
            gate.input(desktop, old.generation, effect),
            Err(ControlGateError::Authority(
                TerminalControlError::StaleGeneration
            ))
        );
        assert_eq!(
            gate.resize(desktop, old.generation, effect),
            Err(ControlGateError::Authority(
                TerminalControlError::StaleGeneration
            ))
        );
        assert_eq!(
            gate.claim(desktop, Some(old.generation), effect),
            Err(ControlGateError::Authority(
                TerminalControlError::StaleGeneration
            ))
        );
        assert!(!called.get());
    }

    #[test]
    fn uncertain_input_is_not_retried_or_given_a_successful_renewal() {
        let gate = TerminalControlGate::default();
        let desktop = identity();
        let grant = gate.claim(desktop, None, || Ok::<_, &str>(())).unwrap();
        let calls = Cell::new(0);
        assert_eq!(
            gate.input(desktop, grant.generation, || {
                calls.set(calls.get() + 1);
                Err("partial write")
            }),
            Err(ControlGateError::Effect("partial write"))
        );
        assert_eq!(calls.get(), 1);
        assert_eq!(gate.status().unwrap().1, Some(grant));
    }

    #[test]
    fn observing_and_resizing_do_not_renew_presence() {
        let gate = TerminalControlGate::default();
        let desktop = identity();
        let grant = gate.claim(desktop, None, || Ok::<_, &str>(())).unwrap();
        gate.resize(desktop, grant.generation, || Ok::<_, &str>(()))
            .unwrap();
        assert_eq!(gate.status().unwrap().1, Some(grant));
        gate.renew(desktop, grant.generation).unwrap();
        gate.release(desktop, grant.generation).unwrap();
        assert_eq!(gate.status().unwrap().1, None);
    }

    #[test]
    fn legacy_paths_cannot_bypass_an_enabled_control_contract() {
        let gate = TerminalControlGate::default();
        gate.legacy(false, || effect_while_locked(&gate)).unwrap();
        let desktop = identity();
        let grant = gate.claim(desktop, None, || Ok::<_, &str>(())).unwrap();
        for coordination in [false, true] {
            assert_eq!(
                gate.legacy(coordination, || panic!("must not execute")),
                Err::<(), ControlGateError<()>>(ControlGateError::GenerationRequired)
            );
        }
        gate.release(desktop, grant.generation).unwrap();
        assert_eq!(
            gate.legacy(false, || Ok::<_, ()>(())),
            Err(ControlGateError::GenerationRequired)
        );
        gate.legacy(true, || effect_while_locked(&gate)).unwrap();
    }

    #[test]
    fn expiry_does_not_reenable_unversioned_operator_input() {
        let mut gate = TerminalControlGate::default();
        let desktop = identity();
        gate.claim(desktop, None, || Ok::<_, &str>(())).unwrap();
        gate.epoch -= Duration::from_secs(91);
        assert_eq!(gate.status().unwrap().1, None);
        assert_eq!(
            gate.legacy(false, || Ok::<_, ()>(())),
            Err(ControlGateError::GenerationRequired)
        );
        gate.legacy(true, || effect_while_locked(&gate)).unwrap();
    }

    #[test]
    fn stopped_session_rejects_every_effect_and_remains_readable() {
        let gate = TerminalControlGate::default();
        let desktop = identity();
        let grant = gate.claim(desktop, None, || Ok::<_, &str>(())).unwrap();
        gate.stop(|| effect_while_stopping(&gate)).unwrap();
        assert_eq!(gate.status().unwrap(), (grant.generation, None));
        assert_eq!(
            gate.claim(identity(), Some(grant.generation), || Ok::<_, &str>(())),
            Err(ControlGateError::Stopped)
        );
        assert_eq!(
            gate.input(desktop, grant.generation, || panic!("stopped input")),
            Err::<(), ControlGateError<()>>(ControlGateError::Stopped)
        );
        assert_eq!(
            gate.resize(desktop, grant.generation, || panic!("stopped resize")),
            Err::<(), ControlGateError<()>>(ControlGateError::Stopped)
        );
        assert_eq!(
            gate.renew(desktop, grant.generation),
            Err(ControlGateError::Stopped)
        );
        for coordination in [false, true] {
            assert_eq!(
                gate.legacy(coordination, || panic!("stopped legacy effect")),
                Err::<(), ControlGateError<()>>(ControlGateError::Stopped)
            );
        }
        gate.stop(|| panic!("successful stop must not repeat"))
            .unwrap_or_else(|_: ControlGateError<()>| panic!("idempotent stop"));
    }

    #[test]
    fn failed_stop_releases_guard_without_disabling_input() {
        let gate = TerminalControlGate::default();
        let desktop = identity();
        let grant = gate.claim(desktop, None, || Ok::<_, &str>(())).unwrap();
        assert_eq!(
            gate.stop(|| Err("kill failed")),
            Err(ControlGateError::Effect("kill failed"))
        );
        gate.input(desktop, grant.generation, || effect_while_locked(&gate))
            .unwrap();
        gate.stop(|| effect_while_stopping(&gate)).unwrap();
    }

    #[test]
    fn explicit_stop_does_not_wait_for_blocked_input_or_acknowledge_it() {
        use std::sync::{Barrier, mpsc};
        let gate = TerminalControlGate::default();
        let entered = Barrier::new(2);
        let release = Barrier::new(2);
        let (events, received) = mpsc::channel();
        std::thread::scope(|scope| {
            let input = scope.spawn(|| {
                gate.legacy(false, || {
                    entered.wait();
                    release.wait();
                    events.send("input").unwrap();
                    Ok::<_, &str>(())
                })
            });
            entered.wait();
            let stop = scope.spawn(|| {
                gate.stop(|| {
                    events.send("stop").unwrap();
                    Ok::<_, &str>(())
                })
            });
            assert!(matches!(
                gate.control.try_lock(),
                Err(TryLockError::WouldBlock)
            ));
            // A deadline bounds a failing test; the channel event, not elapsed
            // time, proves stop ran while the writer is still held at a barrier.
            let stopped = received.recv_timeout(Duration::from_secs(2));
            release.wait();
            let input_result = input.join().unwrap();
            stop.join().unwrap().unwrap();
            assert_eq!(stopped.unwrap(), "stop");
            assert_eq!(input_result, Err(ControlGateError::Interrupted));
        });
        assert_eq!(received.try_iter().collect::<Vec<_>>(), ["input"]);
    }

    #[test]
    fn input_waiting_behind_stop_is_rejected_without_its_effect() {
        use std::sync::{Barrier, mpsc};
        let gate = TerminalControlGate::default();
        let entered = Barrier::new(2);
        let release = Barrier::new(2);
        let (reports, report) = mpsc::channel();
        std::thread::scope(|scope| {
            let stop = scope.spawn(|| {
                gate.stop(|| {
                    entered.wait();
                    release.wait();
                    Ok::<_, &str>(())
                })
            });
            entered.wait();
            let input = scope.spawn(|| {
                let result: Result<(), ControlGateError<()>> =
                    gate.legacy(false, || panic!("must not write after stop"));
                reports.send(result).unwrap();
            });
            let result = report.recv_timeout(Duration::from_secs(2));
            release.wait();
            stop.join().unwrap().unwrap();
            assert_eq!(
                result.unwrap(),
                Err::<(), ControlGateError<()>>(ControlGateError::Stopped)
            );
            input.join().unwrap();
        });
    }
}
