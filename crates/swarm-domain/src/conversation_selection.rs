//! Interactive provider selection is separate from immutable startup recovery.
use crate::ProviderConversationId;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ProviderConversationSelection {
    pub revision: u64,
    pub conversation: ProviderConversationId,
}

/// One current selection and at most one pending interactive-resume boundary.
/// The adapter must authenticate and serialize lifecycle observations from the
/// same process; revisions order accepted evidence, not provider wall-clock time.
pub struct ConversationSelection {
    current: ProviderConversationSelection,
    revision: u64,
    resume_pending: bool,
}

impl ConversationSelection {
    #[must_use]
    pub const fn new(conversation: ProviderConversationId) -> Self {
        Self {
            current: ProviderConversationSelection {
                revision: 1,
                conversation,
            },
            revision: 1,
            resume_pending: false,
        }
    }

    pub fn begin_resume(&mut self, previous: ProviderConversationId) -> bool {
        if previous != self.current.conversation {
            return false;
        }
        self.resume_pending = true;
        true
    }

    /// Orders an explicit future-resumption choice without changing live context.
    /// Canceling the pending boundary prevents a pre-fence end from authorizing
    /// a post-fence start. A later complete resume pair can still advance.
    pub fn fence(&mut self) -> Option<u64> {
        let revision = self.revision.checked_add(1)?;
        self.revision = revision;
        self.resume_pending = false;
        Some(revision)
    }

    /// Consumes a matched resume boundary once. No boundary means no switch.
    pub fn complete_resume(
        &mut self,
        conversation: ProviderConversationId,
    ) -> Option<ProviderConversationSelection> {
        if !self.resume_pending {
            return None;
        }
        let revision = self.revision.checked_add(1)?;
        self.revision = revision;
        self.current = ProviderConversationSelection {
            revision,
            conversation,
        };
        self.resume_pending = false;
        Some(self.current)
    }

    /// Records a conversation change the PROVIDER reported, with no resume pair.
    ///
    /// `complete_resume` exists for a switch Swarm asked for: a `SessionEnd`
    /// (reason=resume) fences the old conversation and the matching
    /// `SessionStart` (source=resume) completes it. A fork or a compact produces
    /// no such pair — Claude simply reports a `SessionStart` carrying a NEW
    /// conversation id — so the paired path can never settle one.
    ///
    /// ⚠️ THIS IS WHY MARKERS WENT STALE. Without it, a forked or compacted
    /// conversation was observed, found to be neither New nor Resumed, and
    /// dropped. The worker went on doing all its work in a conversation Swarm
    /// did not know about; the saved marker stayed on the abandoned one, and
    /// every later start would have resumed the wrong thread while the drift
    /// card reported the worker's own work as a stranger's.
    ///
    /// Returning to the conversation already current is NOT a switch: it
    /// advances nothing and reports nothing, so a duplicate report cannot
    /// inflate the revision.
    pub fn switch(
        &mut self,
        conversation: ProviderConversationId,
    ) -> Option<ProviderConversationSelection> {
        if conversation == self.current.conversation {
            return None;
        }
        let revision = self.revision.checked_add(1)?;
        self.revision = revision;
        self.current = ProviderConversationSelection {
            revision,
            conversation,
        };
        // A pending resume is abandoned by a fork: the boundary it was waiting
        // for can no longer arrive for the conversation it fenced.
        self.resume_pending = false;
        Some(self.current)
    }

    #[must_use]
    pub const fn current(&self) -> ProviderConversationSelection {
        self.current
    }

    #[must_use]
    pub const fn resume_pending(&self) -> bool {
        self.resume_pending
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_provider_reported_switch_advances_without_a_resume_pair() {
        // A fork produces no SessionEnd(resume)/SessionStart(resume) pair, so
        // `complete_resume` can never settle one. Before `switch` existed the
        // worker's real conversation was simply lost.
        let first = ProviderConversationId::new();
        let forked = ProviderConversationId::new();
        let mut selection = ConversationSelection::new(first);
        assert_eq!(selection.current().revision, 1);
        let moved = selection.switch(forked).expect("a fork is a switch");
        assert_eq!(moved.conversation, forked);
        assert_eq!(moved.revision, 2);
        assert_eq!(selection.current().conversation, forked);
        // Revision must exceed 1, or the persistence path that advances the
        // saved marker refuses it and the fix accomplishes nothing.
        assert!(selection.current().revision > 1);
    }

    #[test]
    fn switching_to_the_conversation_already_current_changes_nothing() {
        // A duplicate report must not inflate the revision: the revision is
        // what downstream treats as "something moved".
        let only = ProviderConversationId::new();
        let mut selection = ConversationSelection::new(only);
        assert_eq!(selection.switch(only), None);
        assert_eq!(selection.current().revision, 1);
    }

    #[test]
    fn a_switch_abandons_a_pending_resume() {
        // The fenced conversation is gone; the boundary it waited for cannot
        // arrive. Leaving it pending would let a later unrelated SessionStart
        // complete a resume for a conversation nobody is in.
        let first = ProviderConversationId::new();
        let forked = ProviderConversationId::new();
        let stale = ProviderConversationId::new();
        let mut selection = ConversationSelection::new(first);
        assert!(selection.begin_resume(first));
        assert!(selection.resume_pending());
        assert!(selection.switch(forked).is_some());
        assert!(!selection.resume_pending());
        assert_eq!(selection.complete_resume(stale), None);
        assert_eq!(selection.current().conversation, forked);
    }

    #[test]
    fn fence_is_not_a_selection_and_requires_a_new_resume_pair() {
        let first = ProviderConversationId::new();
        let next = ProviderConversationId::new();
        let mut selection = ConversationSelection::new(first);
        assert!(selection.begin_resume(first));
        assert_eq!(selection.fence(), Some(2));
        assert_eq!(selection.current().revision, 1);
        assert_eq!(selection.current().conversation, first);
        assert_eq!(selection.complete_resume(next), None);
        assert!(selection.begin_resume(first));
        assert_eq!(selection.complete_resume(next).unwrap().revision, 3);
    }

    #[test]
    fn only_paired_resumes_advance_and_returning_to_an_old_conversation_is_new() {
        let first = ProviderConversationId::new();
        let second = ProviderConversationId::new();
        let mut selection = ConversationSelection::new(first);
        assert_eq!(selection.complete_resume(second), None);
        assert!(!selection.begin_resume(second));
        assert!(selection.begin_resume(first));
        assert!(selection.begin_resume(first));
        assert_eq!(selection.complete_resume(second).unwrap().revision, 2);
        assert!(!selection.begin_resume(first));
        assert_eq!(selection.complete_resume(first), None);
        assert!(selection.begin_resume(second));
        assert_eq!(selection.complete_resume(first).unwrap().revision, 3);
        assert_eq!(selection.current().conversation, first);
    }

    #[test]
    fn same_conversation_resume_consumes_boundary_and_revision_never_wraps() {
        let conversation = ProviderConversationId::new();
        let mut selection = ConversationSelection::new(conversation);
        assert!(selection.begin_resume(conversation));
        assert_eq!(selection.complete_resume(conversation).unwrap().revision, 2);
        assert_eq!(selection.complete_resume(conversation), None);
        selection.revision = u64::MAX;
        assert!(selection.begin_resume(conversation));
        assert_eq!(selection.complete_resume(conversation), None);
        assert_eq!(selection.current().revision, 2);
    }
}
