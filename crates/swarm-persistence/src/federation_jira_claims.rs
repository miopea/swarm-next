use std::str::FromStr;

use rusqlite::{OptionalExtension, params};
use swarm_domain::{
    ControlRoomEventKind, FederationClaimId, JiraProjectBindingId, JiraProjectScope,
    LocalApiaryContext, LocalApiaryRole,
};

use crate::{TaskStore, TaskStoreError, insert_control_room_event};

const MAX_PENDING_FEDERATION_JIRA_CLAIMS: i64 = 100;
pub const MAX_FEDERATION_JIRA_CLAIM_BATCH: usize = 16;
const MAX_IDENTITY_BYTES: usize = 128;
const MAX_ERROR_BYTES: usize = 128;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FederationJiraClaimPhase {
    Queued,
    Reserved,
    JiraAssigned,
    Confirmed,
    Complete,
    Attention,
}

impl FederationJiraClaimPhase {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Reserved => "reserved",
            Self::JiraAssigned => "jira_assigned",
            Self::Confirmed => "confirmed",
            Self::Complete => "complete",
            Self::Attention => "attention",
        }
    }
}

impl FromStr for FederationJiraClaimPhase {
    type Err = ();

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "queued" => Ok(Self::Queued),
            "reserved" => Ok(Self::Reserved),
            "jira_assigned" => Ok(Self::JiraAssigned),
            "confirmed" => Ok(Self::Confirmed),
            "complete" => Ok(Self::Complete),
            "attention" => Ok(Self::Attention),
            _ => Err(()),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FederationJiraClaimIntent {
    pub id: String,
    pub binding_id: JiraProjectBindingId,
    pub project_id: String,
    pub issue_id: String,
    pub issue_key: String,
    pub claim_id: Option<FederationClaimId>,
    pub reservation_expires_at: Option<i64>,
    pub phase: FederationJiraClaimPhase,
    pub attempts: u32,
    pub available_at: i64,
    pub last_error: Option<String>,
}

impl TaskStore {
    /// Records operator intent before Keeper or Jira receives a side effect.
    /// Exact retries return the existing non-terminal intent.
    ///
    /// # Errors
    ///
    /// Rejects invalid identities, non-ready Member bindings, full queues, and
    /// unavailable persistence.
    pub fn queue_federation_jira_claim(
        &self,
        binding_id: JiraProjectBindingId,
        issue_id: &str,
        issue_key: &str,
        now: i64,
    ) -> Result<FederationJiraClaimIntent, TaskStoreError> {
        self.require_local_federation_member()?;
        let issue_id = bounded_identity(issue_id)?;
        let issue_key = bounded_identity(issue_key)?;
        if now < 0 {
            return Err(TaskStoreError::InvalidFederationJiraClaim);
        }
        let binding = self.get_jira_project_binding(binding_id)?;
        let LocalApiaryContext::Federated { apiary, local_role } = self.local_apiary_context()?
        else {
            return Err(TaskStoreError::InvalidFederationJiraClaim);
        };
        if local_role != LocalApiaryRole::Member
            || binding.scope != JiraProjectScope::Apiary
            || binding.apiary_id != Some(apiary.id)
            || !binding.access_verified
            || !binding.workflow_mapped
        {
            return Err(TaskStoreError::InvalidFederationJiraClaim);
        }

        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        if !transaction.query_row(
            "SELECT EXISTS(SELECT 1 FROM local_federation_membership
             WHERE singleton = 1 AND state = 'active')",
            [],
            |row| row.get::<_, bool>(0),
        )? {
            return Err(TaskStoreError::InvalidFederationSync);
        }
        if let Some(existing) = transaction
            .query_row(
                "SELECT id, binding_id, project_id, issue_id, issue_key, claim_id,
                        reservation_expires_at, phase, attempts, available_at, last_error
                 FROM federation_jira_claim_intents
                 WHERE binding_id = ?1 AND issue_id = ?2 AND phase <> 'complete'",
                params![binding_id.to_string(), issue_id],
                claim_intent_from_row,
            )
            .optional()?
        {
            return Ok(existing);
        }
        let pending = transaction.query_row(
            "SELECT COUNT(*) FROM federation_jira_claim_intents
             WHERE phase <> 'complete'",
            [],
            |row| row.get::<_, i64>(0),
        )?;
        if pending >= MAX_PENDING_FEDERATION_JIRA_CLAIMS {
            return Err(TaskStoreError::FederationJiraClaimQueueFull);
        }
        let intent = FederationJiraClaimIntent {
            id: uuid::Uuid::now_v7().to_string(),
            binding_id,
            project_id: binding.project_id,
            issue_id: issue_id.to_owned(),
            issue_key: issue_key.to_owned(),
            claim_id: None,
            reservation_expires_at: None,
            phase: FederationJiraClaimPhase::Queued,
            attempts: 0,
            available_at: now,
            last_error: None,
        };
        transaction.execute(
            "INSERT INTO federation_jira_claim_intents
                (id, binding_id, project_id, issue_id, issue_key, phase, available_at,
                 created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, 'queued', ?6, ?6, ?6)",
            params![
                intent.id,
                binding_id.to_string(),
                intent.project_id,
                intent.issue_id,
                intent.issue_key,
                now,
            ],
        )?;
        insert_control_room_event(&transaction, ControlRoomEventKind::TasksChanged)?;
        transaction.commit()?;
        Ok(intent)
    }

    /// Returns the bounded, currently eligible reconciliation batch.
    ///
    /// # Errors
    ///
    /// Rejects invalid timestamps and unavailable persistence.
    pub fn pending_federation_jira_claims(
        &self,
        now: i64,
    ) -> Result<Vec<FederationJiraClaimIntent>, TaskStoreError> {
        if now < 0 {
            return Err(TaskStoreError::InvalidFederationJiraClaim);
        }
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT id, binding_id, project_id, issue_id, issue_key, claim_id,
                    reservation_expires_at, phase, attempts, available_at, last_error
             FROM federation_jira_claim_intents
             WHERE phase IN ('queued','reserved','jira_assigned','confirmed')
               AND available_at <= ?1
             ORDER BY created_at, id LIMIT ?2",
        )?;
        statement
            .query_map(
                params![now, MAX_FEDERATION_JIRA_CLAIM_BATCH],
                claim_intent_from_row,
            )?
            .collect::<Result<Vec<_>, _>>()
            .map_err(Into::into)
    }

    /// Finds the newest durable claim intent for one bound Jira issue.
    ///
    /// # Errors
    ///
    /// Rejects invalid issue identity and unavailable persistence.
    pub fn federation_jira_claim_for_issue(
        &self,
        binding_id: JiraProjectBindingId,
        issue_id: &str,
    ) -> Result<Option<FederationJiraClaimIntent>, TaskStoreError> {
        let issue_id = bounded_identity(issue_id)?;
        self.connection()?
            .query_row(
                "SELECT id, binding_id, project_id, issue_id, issue_key, claim_id,
                        reservation_expires_at, phase, attempts, available_at, last_error
                 FROM federation_jira_claim_intents
                 WHERE binding_id = ?1 AND issue_id = ?2
                 ORDER BY created_at DESC, id DESC LIMIT 1",
                params![binding_id.to_string(), issue_id],
                claim_intent_from_row,
            )
            .optional()
            .map_err(Into::into)
    }

    /// Atomically advances an intent only from its expected phase.
    ///
    /// # Errors
    ///
    /// Rejects invalid transitions, timestamps, and unavailable persistence.
    pub fn advance_federation_jira_claim(
        &self,
        id: &str,
        expected: FederationJiraClaimPhase,
        next: FederationJiraClaimPhase,
        claim_id: Option<FederationClaimId>,
        reservation_expires_at: Option<i64>,
        now: i64,
    ) -> Result<bool, TaskStoreError> {
        if now < 0 || !valid_transition(expected, next, claim_id, reservation_expires_at, now) {
            return Err(TaskStoreError::InvalidFederationJiraClaim);
        }
        let changed = self.connection()?.execute(
            "UPDATE federation_jira_claim_intents
             SET phase = ?2, claim_id = COALESCE(?3, claim_id),
                 reservation_expires_at = COALESCE(?4, reservation_expires_at),
                 last_error = NULL, available_at = ?5, updated_at = ?5
             WHERE id = ?1 AND phase = ?6",
            params![
                id,
                next.as_str(),
                claim_id.map(|value| value.to_string()),
                reservation_expires_at,
                now,
                expected.as_str(),
            ],
        )?;
        Ok(changed == 1)
    }

    /// Returns an expired reservation to the queued phase.
    ///
    /// # Errors
    ///
    /// Rejects invalid timestamps and unavailable persistence.
    pub fn reset_expired_federation_jira_claim(
        &self,
        id: &str,
        now: i64,
    ) -> Result<bool, TaskStoreError> {
        if now < 0 {
            return Err(TaskStoreError::InvalidFederationJiraClaim);
        }
        let changed = self.connection()?.execute(
            "UPDATE federation_jira_claim_intents
             SET phase = 'queued', claim_id = NULL, reservation_expires_at = NULL,
                 available_at = ?2, last_error = 'reservation_expired', updated_at = ?2
             WHERE id = ?1 AND phase = 'reserved'
               AND reservation_expires_at IS NOT NULL AND reservation_expires_at <= ?2",
            params![id, now],
        )?;
        Ok(changed == 1)
    }

    /// Records a bounded temporary failure and schedules durable backoff.
    ///
    /// # Errors
    ///
    /// Rejects invalid error codes and unavailable persistence.
    pub fn retry_federation_jira_claim(
        &self,
        id: &str,
        now: i64,
        error_code: &str,
    ) -> Result<bool, TaskStoreError> {
        let error_code = bounded_error(error_code)?;
        let changed = self.connection()?.execute(
            "UPDATE federation_jira_claim_intents
             SET attempts = attempts + 1,
                 available_at = ?2 + MIN(300, 15 * (attempts + 1)),
                 last_error = ?3, updated_at = ?2
             WHERE id = ?1 AND phase IN ('queued','reserved','jira_assigned','confirmed')",
            params![id, now, error_code],
        )?;
        Ok(changed == 1)
    }

    /// Stops automatic reconciliation for an intent that needs an operator.
    ///
    /// # Errors
    ///
    /// Rejects invalid error codes and unavailable persistence.
    pub fn require_attention_for_federation_jira_claim(
        &self,
        id: &str,
        now: i64,
        error_code: &str,
    ) -> Result<bool, TaskStoreError> {
        let error_code = bounded_error(error_code)?;
        let changed = self.connection()?.execute(
            "UPDATE federation_jira_claim_intents
             SET phase = 'attention', last_error = ?3, updated_at = ?2
             WHERE id = ?1 AND phase IN ('queued','reserved','jira_assigned','confirmed')",
            params![id, now, error_code],
        )?;
        Ok(changed == 1)
    }
}

fn valid_transition(
    expected: FederationJiraClaimPhase,
    next: FederationJiraClaimPhase,
    claim_id: Option<FederationClaimId>,
    reservation_expires_at: Option<i64>,
    now: i64,
) -> bool {
    matches!(
        (expected, next),
        (
            FederationJiraClaimPhase::Queued,
            FederationJiraClaimPhase::Reserved
        ) | (
            FederationJiraClaimPhase::Reserved,
            FederationJiraClaimPhase::JiraAssigned
        ) | (
            FederationJiraClaimPhase::JiraAssigned,
            FederationJiraClaimPhase::Confirmed
        ) | (
            FederationJiraClaimPhase::Confirmed,
            FederationJiraClaimPhase::Complete
        )
    ) && (expected != FederationJiraClaimPhase::Queued
        || (claim_id.is_some() && reservation_expires_at.is_some_and(|expires| expires > now)))
}

fn bounded_identity(value: &str) -> Result<&str, TaskStoreError> {
    let value = value.trim();
    if value.is_empty() || value.len() > MAX_IDENTITY_BYTES || value.chars().any(char::is_control) {
        return Err(TaskStoreError::InvalidFederationJiraClaim);
    }
    Ok(value)
}

fn bounded_error(value: &str) -> Result<&str, TaskStoreError> {
    let value = value.trim();
    if value.is_empty() || value.len() > MAX_ERROR_BYTES || value.chars().any(char::is_control) {
        return Err(TaskStoreError::InvalidFederationJiraClaim);
    }
    Ok(value)
}

fn claim_intent_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<FederationJiraClaimIntent> {
    let claim_id = row
        .get::<_, Option<String>>(5)?
        .map(|value| FederationClaimId::from_str(&value).map_err(|_| rusqlite::Error::InvalidQuery))
        .transpose()?;
    Ok(FederationJiraClaimIntent {
        id: row.get(0)?,
        binding_id: JiraProjectBindingId::from_str(&row.get::<_, String>(1)?)
            .map_err(|_| rusqlite::Error::InvalidQuery)?,
        project_id: row.get(2)?,
        issue_id: row.get(3)?,
        issue_key: row.get(4)?,
        claim_id,
        reservation_expires_at: row.get(6)?,
        phase: FederationJiraClaimPhase::from_str(&row.get::<_, String>(7)?)
            .map_err(|()| rusqlite::Error::InvalidQuery)?,
        attempts: row.get(8)?,
        available_at: row.get(9)?,
        last_error: row.get(10)?,
    })
}

pub(super) fn migrate_federation_jira_claims(
    transaction: &rusqlite::Transaction<'_>,
) -> rusqlite::Result<()> {
    transaction.execute_batch(
        "CREATE TABLE IF NOT EXISTS federation_jira_claim_intents (
             id TEXT PRIMARY KEY NOT NULL,
             binding_id TEXT NOT NULL REFERENCES jira_project_bindings(id),
             project_id TEXT NOT NULL,
             issue_id TEXT NOT NULL,
             issue_key TEXT NOT NULL,
             claim_id TEXT,
             reservation_expires_at INTEGER,
             phase TEXT NOT NULL CHECK (
                 phase IN ('queued','reserved','jira_assigned','confirmed','complete','attention')
             ),
             attempts INTEGER NOT NULL DEFAULT 0 CHECK (attempts >= 0),
             available_at INTEGER NOT NULL,
             last_error TEXT,
             created_at INTEGER NOT NULL,
             updated_at INTEGER NOT NULL
         );
         CREATE UNIQUE INDEX IF NOT EXISTS federation_jira_claim_active_issue
             ON federation_jira_claim_intents(binding_id, issue_id)
             WHERE phase <> 'complete';
         CREATE INDEX IF NOT EXISTS federation_jira_claim_delivery
             ON federation_jira_claim_intents(phase, available_at, created_at);",
    )?;
    transaction.pragma_update(None, "user_version", 50)
}

#[cfg(test)]
mod tests {
    use swarm_domain::{
        FederationJoinReadiness, JiraConnectionState, JiraStatusMapping, SharedWorkBackend,
        TaskState,
    };

    use super::*;
    use crate::JiraProjectBindingInput;

    fn joined_member_with_project(now: i64) -> (TaskStore, JiraProjectBindingId) {
        let keeper = TaskStore::in_memory().unwrap();
        let context = keeper
            .create_apiary_for_local_hive("Garden", SharedWorkBackend::Jira, now)
            .unwrap();
        let LocalApiaryContext::Federated { apiary, .. } = context else {
            panic!("expected Keeper Apiary");
        };
        keeper
            .promote_apiary_jira_project(
                apiary.id,
                "10001",
                "WEB",
                "Website",
                keeper.local_hive_identity().unwrap().operator.id,
                now,
            )
            .unwrap();

        let member = TaskStore::in_memory().unwrap();
        let binding = member
            .upsert_jira_project_binding(&JiraProjectBindingInput {
                project_id: "10001",
                project_key: "WEB",
                project_name: "Website",
                scope: JiraProjectScope::Hive,
                apiary_id: None,
            })
            .unwrap();
        member
            .replace_jira_status_mappings(
                binding.id,
                &[JiraStatusMapping {
                    jira_status_id: "1".into(),
                    jira_status_name: "To Do".into(),
                    task_state: TaskState::Ready,
                }],
            )
            .unwrap();
        let card = member.issue_hive_connection_card(now, 3_600).unwrap();
        keeper.pin_hive_candidate(&card, now).unwrap();
        let bundle = keeper
            .issue_apiary_invitation_bundle(
                card.payload.hive_id,
                "https://keeper.example.test/swarm",
                now,
                3_600,
            )
            .unwrap();
        let invitation = member
            .import_apiary_invitation_bundle(&bundle, now + 1)
            .unwrap();
        member
            .accept_federation_join_policy(invitation.invitation_id, 1, now + 2)
            .unwrap();
        let projects = member
            .federation_project_readiness(invitation.invitation_id)
            .unwrap();
        let submission = member
            .prepare_federation_join_submission(
                invitation.invitation_id,
                &FederationJoinReadiness {
                    jira_connection: JiraConnectionState::Ready,
                    projects,
                    blockers: Vec::new(),
                },
                now + 3,
            )
            .unwrap();
        let acceptance = keeper
            .consume_federation_join_submission(&submission, now + 4)
            .unwrap();
        member
            .apply_federation_join_acceptance(invitation.invitation_id, &acceptance, now + 5)
            .unwrap();
        let binding = member
            .upsert_jira_project_binding(&JiraProjectBindingInput {
                project_id: "10001",
                project_key: "WEB",
                project_name: "Website",
                scope: JiraProjectScope::Apiary,
                apiary_id: Some(apiary.id),
            })
            .unwrap();
        (member, binding.id)
    }

    #[test]
    fn migration_creates_federated_jira_claim_journal() {
        let store = TaskStore::in_memory().unwrap();
        let connection = store.connection().unwrap();
        let exists = connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_master
                 WHERE type = 'table' AND name = 'federation_jira_claim_intents')",
                [],
                |row| row.get::<_, bool>(0),
            )
            .unwrap();
        assert!(exists);
    }

    #[test]
    fn phase_machine_requires_claim_before_reservation() {
        assert!(!valid_transition(
            FederationJiraClaimPhase::Queued,
            FederationJiraClaimPhase::Reserved,
            None,
            None,
            10,
        ));
        assert!(valid_transition(
            FederationJiraClaimPhase::Queued,
            FederationJiraClaimPhase::Reserved,
            Some(FederationClaimId::new()),
            Some(100),
            10,
        ));
        assert!(!valid_transition(
            FederationJiraClaimPhase::Reserved,
            FederationJiraClaimPhase::Complete,
            None,
            None,
            10,
        ));
    }

    #[test]
    fn joined_member_journals_one_retry_stable_claim_before_side_effects() {
        let now = 90_000;
        let (member, binding_id) = joined_member_with_project(now);
        let first = member
            .queue_federation_jira_claim(binding_id, "20001", "WEB-42", now + 10)
            .unwrap();
        let retry = member
            .queue_federation_jira_claim(binding_id, "20001", "WEB-42", now + 11)
            .unwrap();
        assert_eq!(retry.id, first.id);
        assert_eq!(
            member.pending_federation_jira_claims(now + 11).unwrap(),
            vec![first.clone()]
        );
        let claim_id = FederationClaimId::new();
        assert!(
            member
                .advance_federation_jira_claim(
                    &first.id,
                    FederationJiraClaimPhase::Queued,
                    FederationJiraClaimPhase::Reserved,
                    Some(claim_id),
                    Some(now + 120),
                    now + 12,
                )
                .unwrap()
        );
        let reserved = member
            .federation_jira_claim_for_issue(binding_id, "20001")
            .unwrap()
            .unwrap();
        assert_eq!(reserved.claim_id, Some(claim_id));
        assert_eq!(reserved.phase, FederationJiraClaimPhase::Reserved);
    }
}
