use serde::{Deserialize, Serialize};
use std::fmt;
use uuid::Uuid;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SupportKind {
    Feedback,
    BugReport,
    FeatureRequest,
    Email,
}

/// Reviewed public input. No actor, task, delivery or thread-read authority.
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SupportSubmissionInput {
    pub submission_key: Uuid,
    pub kind: SupportKind,
    pub email: String,
    pub name: Option<String>,
    pub subject: String,
    pub body: String,
}

/// Only validation constructs this immutable persistence command.
/// Deliberately not Debug: contact identity and customer text are private.
#[derive(Clone, Serialize)]
pub struct SupportSubmission {
    submission_key: Uuid,
    kind: SupportKind,
    email: String,
    name: Option<String>,
    subject: String,
    body: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SupportValidationError {
    SubmissionKey,
    Email,
    Name,
    Subject,
    Body,
}

impl fmt::Display for SupportValidationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::SubmissionKey => "submission key must not be nil",
            Self::Email => "a bounded, single email address is required",
            Self::Name => "name must be nonempty and at most 300 characters when supplied",
            Self::Subject => "subject must contain 1 to 240 characters",
            Self::Body => "body must contain 1 to 20000 characters",
        })
    }
}
impl std::error::Error for SupportValidationError {}

fn bounded_text(value: &str, maximum: usize, multiline: bool) -> bool {
    !value.trim().is_empty()
        && value.encode_utf16().count() <= maximum
        && !value
            .chars()
            .any(|c| c.is_control() && !(multiline && matches!(c, '\n' | '\r' | '\t')))
}

impl SupportSubmissionInput {
    /// Validates content without treating supplied contact details as verified identity.
    ///
    /// # Errors
    /// Refuses missing, oversized or malformed input before persistence.
    pub fn validate(self) -> Result<SupportSubmission, SupportValidationError> {
        if self.submission_key.is_nil() {
            return Err(SupportValidationError::SubmissionKey);
        }
        // A transport-safe contact shape, not proof of mailbox ownership or delivery.
        if self.email.len() > 320
            || !self.email.split_once('@').is_some_and(|(local, domain)| {
                !local.is_empty() && !domain.is_empty() && !domain.contains('@')
            })
            || self
                .email
                .chars()
                .any(|c| c.is_whitespace() || c.is_control() || matches!(c, '<' | '>' | ',' | ';'))
        {
            return Err(SupportValidationError::Email);
        }
        if self
            .name
            .as_ref()
            .is_some_and(|name| !bounded_text(name, 300, false))
        {
            return Err(SupportValidationError::Name);
        }
        if !bounded_text(&self.subject, 240, false) {
            return Err(SupportValidationError::Subject);
        }
        if !bounded_text(&self.body, 20_000, true) {
            return Err(SupportValidationError::Body);
        }
        Ok(SupportSubmission {
            submission_key: self.submission_key,
            kind: self.kind,
            email: self.email,
            name: self.name,
            subject: self.subject,
            body: self.body,
        })
    }
}

impl SupportSubmission {
    #[must_use]
    pub const fn submission_key(&self) -> Uuid {
        self.submission_key
    }
    #[must_use]
    pub const fn kind(&self) -> SupportKind {
        self.kind
    }
    #[must_use]
    pub fn email(&self) -> &str {
        &self.email
    }
    #[must_use]
    pub fn name(&self) -> Option<&str> {
        self.name.as_deref()
    }
    #[must_use]
    pub fn subject(&self) -> &str {
        &self.subject
    }
    #[must_use]
    pub fn body(&self) -> &str {
        &self.body
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input() -> SupportSubmissionInput {
        SupportSubmissionInput {
            submission_key: Uuid::from_u128(1),
            kind: SupportKind::BugReport,
            email: "person@example.invalid".into(),
            name: None,
            subject: "Reconnect".into(),
            body: "Expected a connected view.\nObserved stale output.".into(),
        }
    }

    #[test]
    fn preserves_exact_contact_and_multiline_report_without_requiring_an_account() {
        let mut input = input();
        input.email = "Person+support@example.invalid".into();
        input.name = Some("Zoë Example".into());
        let result = input.validate().unwrap();
        assert_eq!(result.email(), "Person+support@example.invalid");
        assert_eq!(result.name(), Some("Zoë Example"));
        assert!(result.body().contains('\n'));
    }

    #[test]
    fn rejects_header_injection_multiple_recipients_and_empty_contact() {
        for email in [
            "",
            "@example.invalid",
            "person@",
            "a@b@c",
            "a@b\r\nBcc:c@d",
            "a@b,c@d",
        ] {
            let mut value = input();
            value.email = email.into();
            assert!(matches!(
                value.validate(),
                Err(SupportValidationError::Email)
            ));
        }
    }

    #[test]
    fn public_input_cannot_claim_an_actor_or_existing_conversation() {
        for field in [
            "actor",
            "conversation_id",
            "approved",
            "account_id",
            "diagnostic_bundle",
        ] {
            let mut value = serde_json::json!({"submission_key":Uuid::from_u128(1),"kind":"feedback",
                "email":"person@example.invalid","name":null,"subject":"Hello","body":"Report"});
            value[field] = serde_json::json!("untrusted");
            assert!(serde_json::from_value::<SupportSubmissionInput>(value).is_err());
        }
    }

    #[test]
    fn refuses_oversized_body_and_nil_keys() {
        let mut value = input();
        value.body = "x".repeat(20_001);
        assert!(matches!(
            value.validate(),
            Err(SupportValidationError::Body)
        ));
        let mut value = input();
        value.submission_key = Uuid::nil();
        assert!(matches!(
            value.validate(),
            Err(SupportValidationError::SubmissionKey)
        ));
    }

    #[test]
    fn body_bound_matches_admin_utf16_contract_for_non_bmp_text() {
        let mut value = input();
        value.body = "🐝".repeat(10_000);
        assert!(value.validate().is_ok());
        let mut value = input();
        value.body = "🐝".repeat(10_001);
        assert!(matches!(
            value.validate(),
            Err(SupportValidationError::Body)
        ));
    }
}
