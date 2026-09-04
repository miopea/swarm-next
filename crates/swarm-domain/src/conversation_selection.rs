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

    /// Consumes a matched resume boundary once. No boundary means no switch.
    pub fn complete_resume(
        &mut self,
        conversation: ProviderConversationId,
    ) -> Option<ProviderConversationSelection> {
        if !self.resume_pending {
            return None;
        }
        let revision = self.current.revision.checked_add(1)?;
        self.current = ProviderConversationSelection {
            revision,
            conversation,
        };
        self.resume_pending = false;
        Some(self.current)
    }

    #[must_use]
    pub const fn current(&self) -> ProviderConversationSelection {
        self.current
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        selection.current.revision = u64::MAX;
        assert!(selection.begin_resume(conversation));
        assert_eq!(selection.complete_resume(conversation), None);
        assert_eq!(selection.current().revision, u64::MAX);
    }
}
