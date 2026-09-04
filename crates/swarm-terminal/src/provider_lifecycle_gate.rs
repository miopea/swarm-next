//! One engine-owned startup capability. No worker-level identity is sufficient.
use swarm_domain::{ProviderSessionStartKind, WorkerSessionId};

use crate::ProviderSessionStartObservation;

/// A secret minted by the engine for one process incarnation, never serialized
/// as part of public session summaries or included in debug output.
pub struct ProviderLifecycleGate {
    session: WorkerSessionId,
    capability: [u8; 32],
    revoked: bool,
    observation: Option<ProviderSessionStartObservation>,
}

impl std::fmt::Debug for ProviderLifecycleGate {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ProviderLifecycleGate")
            .field("session", &self.session)
            .field("revoked", &self.revoked)
            .field("observed", &self.observation.is_some())
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProviderLifecycleAcceptance {
    Accepted,
    Duplicate,
    IgnoredLifecycle,
    Denied,
    ConflictingStartup,
}

impl ProviderLifecycleGate {
    /// The owner must obtain this capability from secure OS entropy, not a
    /// conversation ID, timestamp, worker credential, or caller-supplied value.
    #[must_use]
    pub const fn new(session: WorkerSessionId, capability: [u8; 32]) -> Self {
        Self {
            session,
            capability,
            revoked: false,
            observation: None,
        }
    }

    /// Caller holds the engine session lifecycle lock and confirms it is live.
    /// One startup may settle; a retry cannot overwrite it with different facts.
    pub fn observe(
        &mut self,
        session: WorkerSessionId,
        capability: &[u8; 32],
        observation: ProviderSessionStartObservation,
    ) -> ProviderLifecycleAcceptance {
        if self.revoked
            || session != self.session
            || !matches_capability(&self.capability, capability)
        {
            return ProviderLifecycleAcceptance::Denied;
        }
        if !matches!(
            observation.kind,
            ProviderSessionStartKind::New | ProviderSessionStartKind::Resumed
        ) {
            return ProviderLifecycleAcceptance::IgnoredLifecycle;
        }
        match self.observation {
            Some(previous) if previous == observation => ProviderLifecycleAcceptance::Duplicate,
            Some(_) => ProviderLifecycleAcceptance::ConflictingStartup,
            None => {
                self.observation = Some(observation);
                ProviderLifecycleAcceptance::Accepted
            }
        }
    }

    pub fn revoke(&mut self) {
        self.revoked = true;
        self.capability.fill(0);
    }

    #[must_use]
    pub const fn observation(&self) -> Option<ProviderSessionStartObservation> {
        self.observation
    }
}

fn matches_capability(expected: &[u8; 32], supplied: &[u8; 32]) -> bool {
    // Inspect every byte rather than returning the index of the first mismatch.
    // This is not a claim of compiler-guaranteed constant-time cryptography.
    expected
        .iter()
        .zip(supplied)
        .fold(0_u8, |difference, (a, b)| difference | (a ^ b))
        == 0
}

#[cfg(test)]
mod tests {
    use super::*;
    use swarm_domain::ProviderConversationId;

    #[test]
    fn only_current_capability_can_record_and_retries_cannot_rewrite_startup() {
        let session = WorkerSessionId::new();
        let mut gate = ProviderLifecycleGate::new(session, [7; 32]);
        let observation = ProviderSessionStartObservation {
            conversation: ProviderConversationId::new(),
            kind: ProviderSessionStartKind::Resumed,
        };
        assert_eq!(
            gate.observe(WorkerSessionId::new(), &[7; 32], observation),
            ProviderLifecycleAcceptance::Denied
        );
        for index in 0..32 {
            let mut wrong = [7; 32];
            wrong[index] = 8;
            assert_eq!(
                gate.observe(session, &wrong, observation),
                ProviderLifecycleAcceptance::Denied
            );
        }
        assert_eq!(gate.observation(), None);
        assert_eq!(
            gate.observe(session, &[7; 32], observation),
            ProviderLifecycleAcceptance::Accepted
        );
        assert_eq!(
            gate.observe(session, &[7; 32], observation),
            ProviderLifecycleAcceptance::Duplicate
        );
        let changed = ProviderSessionStartObservation {
            conversation: ProviderConversationId::new(),
            ..observation
        };
        assert_eq!(
            gate.observe(session, &[7; 32], changed),
            ProviderLifecycleAcceptance::ConflictingStartup
        );
        assert_eq!(gate.observation(), Some(observation));
        gate.revoke();
        assert_eq!(
            gate.observe(session, &[7; 32], observation),
            ProviderLifecycleAcceptance::Denied
        );
        assert_eq!(
            gate.observe(session, &[0; 32], observation),
            ProviderLifecycleAcceptance::Denied
        );
        assert!(!format!("{gate:?}").contains("capability"));
    }

    #[test]
    fn unrelated_events_do_not_consume_the_startup_capability() {
        let session = WorkerSessionId::new();
        let mut gate = ProviderLifecycleGate::new(session, [9; 32]);
        for kind in [
            ProviderSessionStartKind::Reset,
            ProviderSessionStartKind::Compacted,
            ProviderSessionStartKind::Forked,
            ProviderSessionStartKind::Unknown,
        ] {
            let observation = ProviderSessionStartObservation {
                conversation: ProviderConversationId::new(),
                kind,
            };
            assert_eq!(
                gate.observe(session, &[9; 32], observation),
                ProviderLifecycleAcceptance::IgnoredLifecycle
            );
            assert_eq!(gate.observation(), None);
        }
    }
}
