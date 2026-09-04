use crate::TaskPriority;
use serde::{Deserialize, Serialize};
use std::{collections::HashSet, fmt};

/// A scope loaded from the authenticated integration's configuration.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OpsIntegrationScope {
    pub integration_id: String,
    pub bindings: Vec<OpsAppBinding>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct OpsAppBinding {
    pub app_id: String,
    pub workspace: String,
}

/// No caller-selected workspace or worker identity is accepted.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OpsTicketInput {
    pub app_id: String,
    pub request_id: String,
    pub conversation_id: String,
    pub title: String,
    pub description: String,
    pub priority: TaskPriority,
}

/// Only scope authorization constructs the persistence command.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[non_exhaustive]
pub struct AuthorizedOpsTicket {
    integration_id: String,
    workspace: String,
    input: OpsTicketInput,
}

impl AuthorizedOpsTicket {
    #[must_use]
    pub fn integration_id(&self) -> &str {
        &self.integration_id
    }
    #[must_use]
    pub fn workspace(&self) -> &str {
        &self.workspace
    }
    #[must_use]
    pub fn input(&self) -> &OpsTicketInput {
        &self.input
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OpsTicketValidationError {
    InvalidIdentity,
    InvalidScope,
    AppNotAllowed,
    InvalidTitle,
    InvalidDescription,
}

impl fmt::Display for OpsTicketValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::InvalidIdentity => "invalid Ops ticket identity",
            Self::InvalidScope => "invalid Ops integration scope",
            Self::AppNotAllowed => "application is not allowed for this integration",
            Self::InvalidTitle => "ticket title must contain 1 to 240 UTF-8 bytes",
            Self::InvalidDescription => "ticket description must contain 1 to 64000 UTF-8 bytes",
        })
    }
}
impl std::error::Error for OpsTicketValidationError {}

fn valid_identity(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._:-".contains(&b))
}

impl OpsIntegrationScope {
    /// Resolves only an explicit binding; never guesses a repository from an app name.
    ///
    /// # Errors
    /// Refuses malformed scopes and apps outside the integration's mapping.
    pub fn workspace_for(&self, app_id: &str) -> Result<&str, OpsTicketValidationError> {
        if !valid_identity(&self.integration_id)
            || self.bindings.is_empty()
            || self.bindings.len() > 32
        {
            return Err(OpsTicketValidationError::InvalidScope);
        }
        let mut seen = HashSet::new();
        for binding in &self.bindings {
            if !valid_identity(&binding.app_id)
                || !seen.insert(&binding.app_id)
                || binding.workspace.trim().is_empty()
                || binding.workspace.len() > 4096
                || binding.workspace.chars().any(char::is_control)
            {
                return Err(OpsTicketValidationError::InvalidScope);
            }
        }
        self.bindings
            .iter()
            .find(|binding| binding.app_id == app_id)
            .map(|binding| binding.workspace.trim())
            .ok_or(OpsTicketValidationError::AppNotAllowed)
    }

    /// Produces a normalized, bounded command with a server-owned workspace.
    ///
    /// # Errors
    /// Refuses invalid identifiers, missing scope, and oversized or empty text.
    pub fn authorize(
        &self,
        mut input: OpsTicketInput,
    ) -> Result<AuthorizedOpsTicket, OpsTicketValidationError> {
        if [&input.app_id, &input.request_id, &input.conversation_id]
            .iter()
            .any(|value| !valid_identity(value))
        {
            return Err(OpsTicketValidationError::InvalidIdentity);
        }
        let workspace = self.workspace_for(&input.app_id)?.to_owned();
        input.title = input.title.trim().to_owned();
        input.description = input.description.trim().to_owned();
        if input.title.is_empty()
            || input.title.len() > 240
            || input.title.chars().any(char::is_control)
        {
            return Err(OpsTicketValidationError::InvalidTitle);
        }
        if input.description.is_empty()
            || input.description.len() > 64_000
            || input.description.contains('\0')
        {
            return Err(OpsTicketValidationError::InvalidDescription);
        }
        Ok(AuthorizedOpsTicket {
            integration_id: self.integration_id.clone(),
            workspace,
            input,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn scope() -> OpsIntegrationScope {
        OpsIntegrationScope {
            integration_id: "console-one".into(),
            bindings: vec![OpsAppBinding {
                app_id: "app-one".into(),
                workspace: "/work/approved".into(),
            }],
        }
    }
    fn input() -> OpsTicketInput {
        OpsTicketInput {
            app_id: "app-one".into(),
            request_id: "request:1".into(),
            conversation_id: "feedback:1".into(),
            title: " Calendar export ".into(),
            description: " Reviewed requirement ".into(),
            priority: TaskPriority::Normal,
        }
    }
    #[test]
    fn scope_selects_the_workspace_and_normalizes_reviewed_text() {
        let authorized = scope().authorize(input()).unwrap();
        assert_eq!(authorized.workspace, "/work/approved");
        assert_eq!(authorized.input.title, "Calendar export");
        assert_eq!(authorized.input.description, "Reviewed requirement");
    }
    #[test]
    fn another_app_cannot_choose_a_workspace() {
        let mut command = input();
        command.app_id = "another-app".into();
        assert_eq!(
            scope().authorize(command),
            Err(OpsTicketValidationError::AppNotAllowed)
        );
        let mut serialized = serde_json::to_value(input()).unwrap();
        serialized["workspace"] = serde_json::json!("/work/unapproved");
        assert!(serde_json::from_value::<OpsTicketInput>(serialized).is_err());
    }
    #[test]
    fn duplicate_or_excessive_bindings_are_not_silently_resolved() {
        let mut grant = scope();
        grant.bindings.push(grant.bindings[0].clone());
        assert_eq!(
            grant.authorize(input()),
            Err(OpsTicketValidationError::InvalidScope)
        );
        grant.bindings = vec![grant.bindings[0].clone(); 33];
        assert_eq!(
            grant.authorize(input()),
            Err(OpsTicketValidationError::InvalidScope)
        );
    }
    #[test]
    fn bounded_identity_and_text_fail_closed() {
        let mut command = input();
        command.request_id = "../escape/path".into();
        assert_eq!(
            scope().authorize(command),
            Err(OpsTicketValidationError::InvalidIdentity)
        );
        let mut command = input();
        command.title = "é".repeat(121);
        assert_eq!(
            scope().authorize(command),
            Err(OpsTicketValidationError::InvalidTitle)
        );
        let mut command = input();
        command.description = "x".repeat(64_001);
        assert_eq!(
            scope().authorize(command),
            Err(OpsTicketValidationError::InvalidDescription)
        );
    }
}
