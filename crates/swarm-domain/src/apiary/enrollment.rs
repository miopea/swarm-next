//! Consent carried from a member's submission to the approved invitation.
//! Cryptographic verification belongs to the federation boundary; these rules
//! compare already verified facts and never infer consent from Keeper approval.

use super::{
    ApiaryId, ApiaryInvitationEnvelopePayload, ApiaryJoinLinkId, FederationNodeId, HiveId,
    OperatorId,
};
use serde::{Deserialize, Serialize};

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
    #[must_use]
    pub fn can_transition_to(self, next: Self) -> bool {
        self == next
            || matches!(
                (self, next),
                (
                    Self::AwaitingApproval,
                    Self::Joining | Self::Cancelled | Self::Attention
                ) | (Self::Joining, Self::Complete | Self::Attention)
            )
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ApiaryEnrollment {
    pub consent: ApiaryEnrollmentConsent,
    pub phase: ApiaryEnrollmentPhase,
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
                    );
                assert_eq!(from.can_transition_to(to), allowed);
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
