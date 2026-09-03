//! Engine-owned serialization of authority and terminal effects (ADR 0062).
//! Effect callbacks must not reenter this gate. No socket/API lifetime owns it.

use std::{convert::Infallible, sync::Mutex, time::Instant};

use swarm_domain::{
    TerminalControl, TerminalControlError, TerminalControlGrant, TerminalControlIdentity,
    TerminalControlPresence,
};

#[derive(Debug, Eq, PartialEq)]
pub(crate) enum ControlGateError<E> {
    Authority(TerminalControlError),
    GenerationRequired,
    Poisoned,
    Effect(E),
}

#[derive(Debug)]
pub(crate) struct TerminalControlGate {
    control: Mutex<TerminalControl>,
    epoch: Instant,
}

impl Default for TerminalControlGate {
    fn default() -> Self {
        Self {
            control: Mutex::new(TerminalControl::default()),
            epoch: Instant::now(),
        }
    }
}

impl TerminalControlGate {
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
        Ok((control.generation(), control.owner(now), now))
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
        let now = self.now();
        let grant = match observed_generation {
            Some(generation) => proposed.take_over(identity, generation, now),
            None => proposed.acquire(identity, now),
        }
        .map_err(ControlGateError::Authority)?;
        resize().map_err(ControlGateError::Effect)?;
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
        proposed
            .renew(
                identity,
                generation,
                self.now(),
                TerminalControlPresence::Typing,
            )
            .map_err(ControlGateError::Authority)?;
        write().map_err(ControlGateError::Effect)?;
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
        control
            .authorize(identity, generation, self.now())
            .map_err(ControlGateError::Authority)?;
        resize().map_err(ControlGateError::Effect)
    }

    pub(crate) fn renew(
        &self,
        identity: TerminalControlIdentity,
        generation: u64,
    ) -> Result<TerminalControlGrant, ControlGateError<Infallible>> {
        self.control
            .lock()
            .map_err(|_| ControlGateError::Poisoned)?
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
        if control.generation() != 0 && (!coordination || control.owner(self.now()).is_some()) {
            return Err(ControlGateError::GenerationRequired);
        }
        effect().map_err(ControlGateError::Effect)
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
}
