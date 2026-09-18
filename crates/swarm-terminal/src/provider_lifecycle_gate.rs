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
    selection: Option<swarm_domain::ConversationSelection>,
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
    ConversationChanged,
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
            selection: None,
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
        /*
          A FORK OR A COMPACT IS A CONVERSATION CHANGE, NOT NOISE.

          These two were dropped here as uninteresting lifecycle chatter. They
          are the opposite: both mint a NEW conversation id, and Claude reports
          it over this same authenticated capability. Discarding them left
          Swarm's saved marker pointing at the conversation the worker had
          ABANDONED, so every later start would resume the wrong thread and the
          drift card reported the worker's own work as a newer stranger.

          Measured on two workers: one resumed its pinned conversation, forked
          three seconds later, and did the next two hours of work in a
          conversation Swarm never recorded.

          They are handled as a SELECTION CHANGE rather than a startup. A
          startup settles identity once and a retry must not overwrite it with
          different facts — that rule is untouched below. A fork is the live
          session telling us where it went, which is what `ConversationChanged`
          already exists to carry.
        */
        if matches!(
            observation.kind,
            ProviderSessionStartKind::Forked | ProviderSessionStartKind::Compacted
        ) {
            return match self.selection.as_mut() {
                // Before any startup has settled there is nothing to move, and
                // a fork cannot establish identity on its own.
                None => ProviderLifecycleAcceptance::IgnoredLifecycle,
                Some(selection) => {
                    if selection.switch(observation.conversation).is_some() {
                        ProviderLifecycleAcceptance::ConversationChanged
                    } else {
                        // Already there. Reporting it twice changes nothing.
                        ProviderLifecycleAcceptance::Duplicate
                    }
                }
            };
        }
        if !matches!(
            observation.kind,
            ProviderSessionStartKind::New | ProviderSessionStartKind::Resumed
        ) {
            return ProviderLifecycleAcceptance::IgnoredLifecycle;
        }
        if observation.kind == ProviderSessionStartKind::Resumed
            && let Some(selection) = self.selection.as_mut()
            && selection
                .complete_resume(observation.conversation)
                .is_some()
        {
            return ProviderLifecycleAcceptance::ConversationChanged;
        }
        match self.observation {
            Some(_)
                if self.selection.as_ref().is_some_and(|selection| {
                    selection.current().conversation == observation.conversation
                }) =>
            {
                ProviderLifecycleAcceptance::Duplicate
            }
            Some(_) => ProviderLifecycleAcceptance::ConflictingStartup,
            None => {
                self.observation = Some(observation);
                self.selection = Some(swarm_domain::ConversationSelection::new(
                    observation.conversation,
                ));
                ProviderLifecycleAcceptance::Accepted
            }
        }
    }

    /// Called only for an authenticated provider SessionEnd(reason=resume).
    /// The process owner must apply the same live-child check as startup reports.
    pub fn begin_resume(
        &mut self,
        session: WorkerSessionId,
        capability: &[u8; 32],
        previous: swarm_domain::ProviderConversationId,
    ) -> bool {
        if self.revoked
            || session != self.session
            || !matches_capability(&self.capability, capability)
        {
            return false;
        }
        self.selection
            .as_mut()
            .is_some_and(|selection| selection.begin_resume(previous))
    }

    #[must_use]
    pub fn selection(&self) -> Option<swarm_domain::ProviderConversationSelection> {
        self.selection
            .as_ref()
            .map(swarm_domain::ConversationSelection::current)
    }

    /// The trusted engine owner orders an explicit operator default choice.
    /// Before initial evidence, revision zero fences no interactive transitions.
    pub fn fence_selection(&mut self) -> Option<u64> {
        if self.revoked {
            return None;
        }
        self.selection
            .as_mut()
            .map_or(Some(0), swarm_domain::ConversationSelection::fence)
    }

    pub fn revoke(&mut self) {
        self.revoked = true;
        self.capability.fill(0);
    }

    /// Authenticates one provider incarnation, never a durable worker token.
    #[must_use]
    pub fn authenticates(&self, session: WorkerSessionId, capability: &[u8; 32]) -> bool {
        !self.revoked && session == self.session && matches_capability(&self.capability, capability)
    }

    #[must_use]
    pub fn is_current_conversation(
        &self,
        conversation: swarm_domain::ProviderConversationId,
    ) -> bool {
        !self.revoked
            && self.selection.as_ref().is_some_and(|selection| {
                !selection.resume_pending() && selection.current().conversation == conversation
            })
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
    fn paired_interactive_resume_changes_selection_without_rewriting_startup() {
        let session = WorkerSessionId::new();
        let mut gate = ProviderLifecycleGate::new(session, [7; 32]);
        let first = ProviderSessionStartObservation {
            conversation: ProviderConversationId::new(),
            kind: ProviderSessionStartKind::Resumed,
        };
        assert!(!gate.begin_resume(session, &[7; 32], first.conversation));
        assert_eq!(
            gate.observe(session, &[7; 32], first),
            ProviderLifecycleAcceptance::Accepted
        );
        assert!(!gate.begin_resume(WorkerSessionId::new(), &[7; 32], first.conversation));
        assert!(!gate.begin_resume(session, &[8; 32], first.conversation));
        assert!(gate.begin_resume(session, &[7; 32], first.conversation));
        let next = ProviderSessionStartObservation {
            conversation: ProviderConversationId::new(),
            ..first
        };
        assert_eq!(
            gate.observe(session, &[7; 32], next),
            ProviderLifecycleAcceptance::ConversationChanged
        );
        assert_eq!(gate.observation(), Some(first));
        assert_eq!(gate.selection().unwrap().conversation, next.conversation);
        assert_eq!(gate.selection().unwrap().revision, 2);
        assert_eq!(
            gate.observe(session, &[7; 32], next),
            ProviderLifecycleAcceptance::Duplicate
        );
        assert_eq!(
            gate.observe(session, &[7; 32], first),
            ProviderLifecycleAcceptance::ConflictingStartup
        );
        gate.revoke();
        assert!(!gate.begin_resume(session, &[7; 32], next.conversation));
    }

    #[test]
    fn explicit_choice_fence_cancels_pending_resume_without_publishing_selection() {
        let session = WorkerSessionId::new();
        let mut gate = ProviderLifecycleGate::new(session, [7; 32]);
        assert_eq!(gate.fence_selection(), Some(0));
        let first = ProviderSessionStartObservation {
            conversation: ProviderConversationId::new(),
            kind: ProviderSessionStartKind::Resumed,
        };
        gate.observe(session, &[7; 32], first);
        assert!(gate.begin_resume(session, &[7; 32], first.conversation));
        assert_eq!(gate.fence_selection(), Some(2));
        assert_eq!(gate.selection().unwrap().revision, 1);
        let next = ProviderSessionStartObservation {
            conversation: ProviderConversationId::new(),
            ..first
        };
        assert_eq!(
            gate.observe(session, &[7; 32], next),
            ProviderLifecycleAcceptance::ConflictingStartup
        );
        assert!(gate.begin_resume(session, &[7; 32], first.conversation));
        assert_eq!(
            gate.observe(session, &[7; 32], next),
            ProviderLifecycleAcceptance::ConversationChanged
        );
        assert_eq!(gate.selection().unwrap().revision, 3);
        gate.revoke();
        assert_eq!(gate.fence_selection(), None);
    }

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

    #[test]
    fn a_fork_after_startup_moves_the_selection_to_the_new_conversation() {
        // THE DEFECT THIS CLOSES. Claude forks, reports a SessionStart carrying
        // a NEW conversation id, and the gate used to drop it as uninteresting
        // lifecycle chatter. The worker then did all its work in a conversation
        // Swarm had never recorded, the saved marker stayed on the abandoned
        // one, and the drift card reported the worker's own work as newer.
        let session = WorkerSessionId::new();
        let started = ProviderConversationId::new();
        let forked = ProviderConversationId::new();
        let mut gate = ProviderLifecycleGate::new(session, [9; 32]);
        assert_eq!(
            gate.observe(
                session,
                &[9; 32],
                ProviderSessionStartObservation {
                    conversation: started,
                    kind: ProviderSessionStartKind::New,
                },
            ),
            ProviderLifecycleAcceptance::Accepted
        );
        assert_eq!(
            gate.observe(
                session,
                &[9; 32],
                ProviderSessionStartObservation {
                    conversation: forked,
                    kind: ProviderSessionStartKind::Forked,
                },
            ),
            ProviderLifecycleAcceptance::ConversationChanged
        );
        let selection = gate.selection().expect("a settled startup has a selection");
        assert_eq!(selection.conversation, forked);
        // The revision MUST exceed 1: the persistence path that advances the
        // saved marker refuses anything at revision 1 or below, so without this
        // the gate would move and the marker still would not.
        assert!(selection.revision > 1);
    }

    #[test]
    fn a_compact_moves_the_selection_the_same_way_a_fork_does() {
        // Compacting also mints a new conversation id. It was dropped by the
        // same filter and for the same wrong reason.
        let session = WorkerSessionId::new();
        let started = ProviderConversationId::new();
        let compacted = ProviderConversationId::new();
        let mut gate = ProviderLifecycleGate::new(session, [9; 32]);
        gate.observe(
            session,
            &[9; 32],
            ProviderSessionStartObservation {
                conversation: started,
                kind: ProviderSessionStartKind::New,
            },
        );
        assert_eq!(
            gate.observe(
                session,
                &[9; 32],
                ProviderSessionStartObservation {
                    conversation: compacted,
                    kind: ProviderSessionStartKind::Compacted,
                },
            ),
            ProviderLifecycleAcceptance::ConversationChanged
        );
        assert_eq!(gate.selection().unwrap().conversation, compacted);
    }

    #[test]
    fn a_fork_reported_twice_does_not_advance_the_revision_again() {
        // The revision is what downstream reads as "something moved". A repeat
        // report must not manufacture a second move.
        let session = WorkerSessionId::new();
        let started = ProviderConversationId::new();
        let forked = ProviderConversationId::new();
        let mut gate = ProviderLifecycleGate::new(session, [9; 32]);
        gate.observe(
            session,
            &[9; 32],
            ProviderSessionStartObservation {
                conversation: started,
                kind: ProviderSessionStartKind::New,
            },
        );
        let fork = ProviderSessionStartObservation {
            conversation: forked,
            kind: ProviderSessionStartKind::Forked,
        };
        assert_eq!(
            gate.observe(session, &[9; 32], fork),
            ProviderLifecycleAcceptance::ConversationChanged
        );
        let after_first = gate.selection().unwrap().revision;
        assert_eq!(
            gate.observe(session, &[9; 32], fork),
            ProviderLifecycleAcceptance::Duplicate
        );
        assert_eq!(gate.selection().unwrap().revision, after_first);
    }

    #[test]
    fn a_fork_from_a_wrong_capability_is_still_denied() {
        // Accepting a new KIND must not widen who may report it.
        let session = WorkerSessionId::new();
        let mut gate = ProviderLifecycleGate::new(session, [9; 32]);
        gate.observe(
            session,
            &[9; 32],
            ProviderSessionStartObservation {
                conversation: ProviderConversationId::new(),
                kind: ProviderSessionStartKind::New,
            },
        );
        let before = gate.selection().unwrap().conversation;
        assert_eq!(
            gate.observe(
                session,
                &[1; 32],
                ProviderSessionStartObservation {
                    conversation: ProviderConversationId::new(),
                    kind: ProviderSessionStartKind::Forked,
                },
            ),
            ProviderLifecycleAcceptance::Denied
        );
        assert_eq!(gate.selection().unwrap().conversation, before);
    }
}
