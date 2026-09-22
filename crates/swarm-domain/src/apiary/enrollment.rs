//! Consent carried from a member's submission to the approved invitation.
//! Cryptographic verification belongs to the federation boundary; these rules
//! compare already verified facts and never infer consent from Keeper approval.

use super::{
    ApiaryId, ApiaryInvitationEnvelopePayload, ApiaryJoinLinkId, FederationNodeId, HiveId,
    OperatorId,
};
use serde::{Deserialize, Serialize};

/// Signed disclosure delivered in the Keeper link before member submission.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ApiaryEnrollmentOfferPayload {
    pub schema_version: u16,
    pub link_id: ApiaryJoinLinkId,
    pub apiary_id: ApiaryId,
    pub apiary_name: String,
    pub keeper_endpoint: String,
    pub keeper: super::HiveConnectionCard,
    pub policy_revision: u64,
    /// Version of the product's displayed Keeper-management consent text.
    pub management_terms_version: u16,
    pub issued_at: i64,
    pub expires_at: i64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ApiaryEnrollmentOffer {
    pub payload: ApiaryEnrollmentOfferPayload,
    pub signature: String,
}

/// Persisted only after the operator submits the disclosed joining terms.
/// It contains no bearer secret, private key, or integration credential.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ApiaryEnrollmentConsent {
    pub link_id: ApiaryJoinLinkId,
    pub apiary_id: ApiaryId,
    pub keeper_node_id: FederationNodeId,
    pub member_node_id: FederationNodeId,
    pub member_hive_id: HiveId,
    pub member_operator_id: OperatorId,
    pub policy_revision: u64,
    pub accepted_at: i64,
    pub expires_at: i64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ApiaryEnrollmentConsentError {
    DifferentIdentity,
    PolicyChanged,
    Expired,
    InvalidTime,
}

/// Durable progress owned by the member application, not its browser.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApiaryEnrollmentPhase {
    AwaitingApproval,
    Joining,
    Complete,
    Cancelled,
    Attention,
}

impl ApiaryEnrollmentPhase {
    /// A retry may repeat a phase, but cannot revive cancelled/completed work.
    ///
    /// ⚠️ `Attention` HAD NO WAY OUT AND THAT MADE IT A TRAP. Any problem other
    /// than an unreachable Keeper moves a record here, and nothing retries a
    /// record in this phase — so ONE failure, however transient, froze the join
    /// permanently. The screen then kept showing that first failure's code
    /// forever, which is why an operator on 2026-09-22 saw the same
    /// `apiary_join_not_ready` for days, unchanged by a valid invitation issued
    /// in the meantime: nothing was reading it.
    ///
    /// `Attention` is a request for a HUMAN, not a verdict. Going back to
    /// `AwaitingApproval` is that human saying "try it again", which is why this
    /// edge is deliberate and why nothing takes it automatically — an automatic
    /// retry is what this phase exists to stop. `Complete` and `Cancelled` stay
    /// terminal, because those really are finished.
    #[must_use]
    pub fn can_transition_to(self, next: Self) -> bool {
        self == next
            || matches!(
                (self, next),
                (
                    Self::AwaitingApproval,
                    Self::Joining | Self::Cancelled | Self::Attention
                ) | (Self::Joining, Self::Complete | Self::Attention)
                    | (Self::Attention, Self::AwaitingApproval | Self::Cancelled)
            )
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ApiaryEnrollment {
    pub consent: ApiaryEnrollmentConsent,
    pub phase: ApiaryEnrollmentPhase,
    #[serde(default)]
    pub consecutive_failures: u32,
    #[serde(default)]
    pub next_attempt_at: Option<i64>,
    #[serde(default)]
    pub problem: Option<ApiaryEnrollmentProblem>,
    /// The transport code and status actually observed, when the failure could
    /// not be classified.
    ///
    /// ⚠️ WITHOUT THIS NOBODY CAN SEE WHY A JOIN FAILED. The classified variants
    /// each carry a cause somebody established; `Unclassified` carries none, and
    /// the code was previously discarded at the match arm that produced it. An
    /// operator then read a confident sentence about a cause nobody had checked
    /// and went looking in the wrong place -- which is exactly what happened on
    /// 2026-09-14, where the real block was a stranded invitation and the screen
    /// said their terms had changed.
    #[serde(default)]
    pub problem_code: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApiaryEnrollmentProblem {
    KeeperUnavailable,
    InvitationUnavailable,
    RuntimeIncompatible,
    /// Something refused the join and nothing here knows what.
    ///
    /// ⚠️ THIS REPLACES `ApprovalChanged`, WHICH WAS A GUESS WEARING A FACT.
    /// Nothing ever detected a changed approval: that variant was only ever the
    /// `_ =>` fallback arm, and the member's screen rendered it as "The approved
    /// invitation no longer matches your submitted terms." Every unrecognised
    /// client error -- a stranded invitation, an auth refusal, a payload the
    /// Keeper would not take -- was reported as the one cause it almost never
    /// was. The serde alias keeps records written before this rename readable;
    /// they were never really approval changes either.
    #[serde(alias = "approval_changed")]
    Unclassified,
}

impl ApiaryEnrollment {
    /// Keeps retry authority bounded by the original consent lifetime.
    /// Permanent refusals require review; successful observations clear trouble.
    pub fn record_attempt(
        &mut self,
        problem: Option<ApiaryEnrollmentProblem>,
        problem_code: Option<String>,
        now: i64,
    ) {
        if !matches!(
            self.phase,
            ApiaryEnrollmentPhase::AwaitingApproval | ApiaryEnrollmentPhase::Joining
        ) {
            return;
        }
        self.problem = problem;
        // Cleared alongside the problem, so a recovered enrollment never shows a
        // stale code from the attempt before it.
        self.problem_code = problem_code;
        self.next_attempt_at = None;
        if problem.is_none() {
            self.consecutive_failures = 0;
        } else {
            self.consecutive_failures = self.consecutive_failures.saturating_add(1).min(1000);
            if problem == Some(ApiaryEnrollmentProblem::KeeperUnavailable)
                && now < self.consent.expires_at
                && self.consecutive_failures < 1000
            {
                self.next_attempt_at = Some(
                    now.saturating_add(super::federation_retry_delay_seconds(
                        self.consecutive_failures,
                    ))
                    .min(self.consent.expires_at),
                );
            } else {
                self.phase = ApiaryEnrollmentPhase::Attention;
            }
        }
    }
}

impl ApiaryEnrollmentConsent {
    /// Checks whether an approved invitation can use the consent already given.
    /// The caller must verify the invitation signature and link association first.
    /// Jira configuration is intentionally not part of membership consent.
    ///
    /// # Errors
    /// Rejects substituted identities, changed policy, expiry, and invalid time.
    pub fn validate_invitation(
        &self,
        link_id: ApiaryJoinLinkId,
        invitation: &ApiaryInvitationEnvelopePayload,
        now: i64,
    ) -> Result<(), ApiaryEnrollmentConsentError> {
        if self.link_id != link_id
            || self.apiary_id != invitation.apiary_id
            || self.keeper_node_id != invitation.keeper_node_id
            || self.member_node_id != invitation.invited_node_id
            || self.member_hive_id != invitation.invited_hive_id
            || self.member_operator_id != invitation.invited_operator_id
        {
            return Err(ApiaryEnrollmentConsentError::DifferentIdentity);
        }
        if self.policy_revision == 0 || self.policy_revision != invitation.required_policy_revision
        {
            return Err(ApiaryEnrollmentConsentError::PolicyChanged);
        }
        if self.accepted_at > now
            || self.expires_at <= self.accepted_at
            || invitation.issued_at > now
            || invitation.expires_at <= invitation.issued_at
        {
            return Err(ApiaryEnrollmentConsentError::InvalidTime);
        }
        if now >= self.expires_at || now >= invitation.expires_at {
            return Err(ApiaryEnrollmentConsentError::Expired);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ApiaryInvitationId, SharedWorkBackend};

    #[test]
    fn retries_never_extend_consent_and_failure_count_is_bounded() {
        let (consent, _) = fixture();
        let mut record = ApiaryEnrollment {
            consent,
            phase: ApiaryEnrollmentPhase::AwaitingApproval,
            consecutive_failures: 0,
            next_attempt_at: None,
            problem: None,
            problem_code: None,
        };
        for _ in 0..1100 {
            record.record_attempt(Some(ApiaryEnrollmentProblem::KeeperUnavailable), None, 99);
            assert!(record.next_attempt_at.is_none_or(|next| next <= 100));
        }
        assert_eq!(record.consecutive_failures, 1000);
        assert_eq!(record.phase, ApiaryEnrollmentPhase::Attention);
        assert_eq!(record.next_attempt_at, None);
    }

    #[test]
    fn enrollment_cannot_skip_approval_or_revive_terminal_phases() {
        use ApiaryEnrollmentPhase::{Attention, AwaitingApproval, Cancelled, Complete, Joining};
        let phases = [AwaitingApproval, Joining, Complete, Cancelled, Attention];
        for from in phases {
            for to in phases {
                let allowed = from == to
                    || matches!(
                        (from, to),
                        (AwaitingApproval, Joining | Cancelled | Attention)
                            | (Joining, Complete | Attention)
                            | (Attention, AwaitingApproval | Cancelled)
                    );
                assert_eq!(from.can_transition_to(to), allowed, "{from:?} -> {to:?}");
            }
        }

        // ⚠️ THE PART THAT IS AN INVARIANT RATHER THAN A MIRROR. The loop above
        // restates the rule and so agrees with whatever the rule says; these
        // two assertions say what must be TRUE of it. `Attention` having no way
        // out is what froze an operator's join permanently on 2026-09-22 —
        // parked by one failure, retried by nothing, and still displaying that
        // failure's code days later. It is a request for a human, so a human
        // can now send it back round.
        assert!(
            Attention.can_transition_to(AwaitingApproval),
            "a parked join must have a way back that is not starting over"
        );
        for finished in [Complete, Cancelled] {
            for to in phases {
                assert_eq!(
                    finished.can_transition_to(to),
                    finished == to,
                    "{finished:?} is finished and must not be revived into {to:?}"
                );
            }
        }
    }

    fn fixture() -> (ApiaryEnrollmentConsent, ApiaryInvitationEnvelopePayload) {
        let consent = ApiaryEnrollmentConsent {
            link_id: ApiaryJoinLinkId::new(),
            apiary_id: ApiaryId::new(),
            keeper_node_id: FederationNodeId::new(),
            member_node_id: FederationNodeId::new(),
            member_hive_id: HiveId::new(),
            member_operator_id: OperatorId::new(),
            policy_revision: 1,
            accepted_at: 10,
            expires_at: 100,
        };
        let invitation = ApiaryInvitationEnvelopePayload {
            schema_version: 1,
            protocol_version: 1,
            invitation_id: ApiaryInvitationId::new(),
            apiary_id: consent.apiary_id,
            apiary_name: "Test garden".into(),
            shared_work_backend: SharedWorkBackend::Jira,
            required_policy_revision: 1,
            promoted_project_catalog_digest: "empty-catalog".into(),
            keeper_node_id: consent.keeper_node_id,
            keeper_hive_id: HiveId::new(),
            keeper_operator_id: OperatorId::new(),
            invited_node_id: consent.member_node_id,
            invited_hive_id: consent.member_hive_id,
            invited_operator_id: consent.member_operator_id,
            keeper_endpoint: "https://keeper.example".into(),
            issued_at: 20,
            expires_at: 100,
            nonce: "test-nonce".into(),
        };
        (consent, invitation)
    }

    #[test]
    fn approved_invitation_reuses_exact_consent_without_jira_or_second_acceptance() {
        let (consent, invitation) = fixture();
        for now in [20, 21, 99] {
            assert_eq!(
                consent.validate_invitation(consent.link_id, &invitation, now),
                Ok(())
            );
        }
        let restored: ApiaryEnrollmentConsent =
            serde_json::from_str(&serde_json::to_string(&consent).unwrap()).unwrap();
        assert_eq!(restored, consent);
        assert_eq!(
            restored.validate_invitation(consent.link_id, &invitation, 30),
            Ok(())
        );
    }

    #[test]
    fn approval_cannot_accept_changed_policy_or_identity() {
        let (consent, original) = fixture();
        let mut changed = original.clone();
        changed.required_policy_revision += 1;
        assert_eq!(
            consent.validate_invitation(consent.link_id, &changed, 30),
            Err(ApiaryEnrollmentConsentError::PolicyChanged)
        );
        let substitutions: [fn(&mut ApiaryInvitationEnvelopePayload); 5] = [
            |p| p.apiary_id = ApiaryId::new(),
            |p| p.keeper_node_id = FederationNodeId::new(),
            |p| p.invited_node_id = FederationNodeId::new(),
            |p| p.invited_hive_id = HiveId::new(),
            |p| p.invited_operator_id = OperatorId::new(),
        ];
        for substitute in substitutions {
            let mut changed = original.clone();
            substitute(&mut changed);
            assert_eq!(
                consent.validate_invitation(consent.link_id, &changed, 30),
                Err(ApiaryEnrollmentConsentError::DifferentIdentity)
            );
        }
        assert_eq!(
            consent.validate_invitation(ApiaryJoinLinkId::new(), &original, 30),
            Err(ApiaryEnrollmentConsentError::DifferentIdentity)
        );
    }

    #[test]
    fn retries_do_not_extend_consent_or_invitation_lifetime() {
        let (consent, mut invitation) = fixture();
        assert_eq!(
            consent.validate_invitation(consent.link_id, &invitation, 100),
            Err(ApiaryEnrollmentConsentError::Expired)
        );
        invitation.expires_at = 25;
        assert_eq!(
            consent.validate_invitation(consent.link_id, &invitation, 25),
            Err(ApiaryEnrollmentConsentError::Expired)
        );
        assert_eq!(
            consent.validate_invitation(consent.link_id, &invitation, 9),
            Err(ApiaryEnrollmentConsentError::InvalidTime)
        );
    }
}
