//! Member-owned enrollment journal. No transport or approval is performed here.

use crate::{TaskStore, TaskStoreError};
use rusqlite::{OptionalExtension, Transaction, params};
use swarm_domain::{
    ApiaryEnrollment, ApiaryEnrollmentConsent, ApiaryEnrollmentPhase, ApiaryInvitationEnvelope,
    ApiaryInvitationId, ApiaryJoinLinkId,
};

const MAX_ENROLLMENTS: i64 = 32;

pub(crate) fn migrate(transaction: &Transaction<'_>) -> rusqlite::Result<()> {
    transaction.execute_batch(
        "CREATE TABLE IF NOT EXISTS apiary_enrollments (
            link_id TEXT PRIMARY KEY REFERENCES local_apiary_keeper_links(link_id) ON DELETE CASCADE,
            record_json TEXT NOT NULL
        );"
    )?;
    transaction.pragma_update(
        None,
        "user_version",
        super::APIARY_ENROLLMENT_SCHEMA_VERSION,
    )
}

impl TaskStore {
    /// Closes the journal only after the existing signed-receipt path consumed
    /// this exact invitation. Also recovers a crash after receipt application.
    ///
    /// # Errors
    /// Returns persistence failures or corrupt enrollment records.
    pub fn finish_consented_apiary_join(
        &self,
        link_id: ApiaryJoinLinkId,
    ) -> Result<bool, TaskStoreError> {
        let mut connection = self.connection()?;
        let tx = connection.transaction()?;
        let stored: Option<String> = tx
            .query_row(
                "SELECT record_json FROM apiary_enrollments WHERE link_id = ?1",
                [link_id.to_string()],
                |r| r.get(0),
            )
            .optional()?;
        let Some(stored) = stored else {
            return Ok(false);
        };
        let mut record = decode(&stored)?;
        if record.phase == ApiaryEnrollmentPhase::Complete {
            return Ok(true);
        }
        if record.phase != ApiaryEnrollmentPhase::Joining {
            return Ok(false);
        }
        let consumed: bool = tx.query_row(
            "SELECT EXISTS (SELECT 1 FROM apiary_join_invitations i
             JOIN local_apiary_keeper_links l ON l.link_id = ?1
             WHERE i.one_time_secret = l.one_time_secret AND i.keeper_endpoint = l.keeper_endpoint
               AND i.state = 'consumed' AND i.apiary_id = ?2 AND i.invited_node_id = ?3
               AND i.invited_hive_id = ?4 AND i.invited_operator_id = ?5
               AND i.required_policy_revision = ?6 AND i.keeper_node_id = ?7)",
            params![
                link_id.to_string(),
                record.consent.apiary_id.to_string(),
                record.consent.member_node_id.to_string(),
                record.consent.member_hive_id.to_string(),
                record.consent.member_operator_id.to_string(),
                record.consent.policy_revision,
                record.consent.keeper_node_id.to_string()
            ],
            |r| r.get(0),
        )?;
        if !consumed {
            return Ok(false);
        }
        record.phase = ApiaryEnrollmentPhase::Complete;
        tx.execute(
            "UPDATE apiary_enrollments SET record_json = ?2 WHERE link_id = ?1",
            params![link_id.to_string(), encode(&record)?],
        )?;
        tx.commit()?;
        Ok(true)
    }

    /// Reuses consent only for the verified invitation delivered by this link.
    /// Policy acceptance and journal advancement commit together; cancellation
    /// and a stale reconciliation attempt cannot both win.
    ///
    /// # Errors
    /// Rejects missing consent, mismatched link credentials/identity/policy,
    /// expired invitations, cancelled work, and unavailable persistence.
    pub fn prepare_consented_apiary_join(
        &self,
        link_id: ApiaryJoinLinkId,
        invitation_id: ApiaryInvitationId,
        now: i64,
    ) -> Result<ApiaryEnrollment, TaskStoreError> {
        let mut connection = self.connection()?;
        let tx = connection.transaction()?;
        let stored: String = tx
            .query_row(
                "SELECT record_json FROM apiary_enrollments WHERE link_id = ?1",
                [link_id.to_string()],
                |row| row.get(0),
            )
            .optional()?
            .ok_or(TaskStoreError::ApiaryJoinNotReady)?;
        let mut record = decode(&stored)?;
        if !matches!(
            record.phase,
            ApiaryEnrollmentPhase::AwaitingApproval | ApiaryEnrollmentPhase::Joining
        ) {
            return Err(TaskStoreError::ApiaryJoinNotReady);
        }
        // Import already checked the signature. Compare its immutable envelope
        // and private bootstrap binding rather than accepting an arbitrary ID.
        let (envelope, state): (String, String) = tx
            .query_row(
                "SELECT i.envelope_json, i.state FROM apiary_join_invitations i
             JOIN local_apiary_keeper_links l ON l.link_id = ?1
             WHERE i.id = ?2 AND i.one_time_secret = l.one_time_secret
               AND i.keeper_endpoint = l.keeper_endpoint
               AND EXISTS (SELECT 1 FROM hives h
                   WHERE h.id = i.invited_hive_id AND h.apiary_id IS NULL)",
                params![link_id.to_string(), invitation_id.to_string()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?
            .ok_or(TaskStoreError::ApiaryJoinNotReady)?;
        let envelope: ApiaryInvitationEnvelope = serde_json::from_str(&envelope)
            .map_err(|error| TaskStoreError::IntegrityFailure(error.to_string()))?;
        record
            .consent
            .validate_invitation(link_id, &envelope.payload, now)
            .map_err(|_| TaskStoreError::ApiaryJoinNotReady)?;
        if state == "keeper_pinned" {
            tx.execute(
                "UPDATE apiary_join_invitations
                SET state = 'policy_accepted', policy_accepted_at = ?2 WHERE id = ?1",
                params![invitation_id.to_string(), record.consent.accepted_at],
            )?;
        } else if !matches!(state.as_str(), "policy_accepted" | "submitted") {
            return Err(TaskStoreError::ApiaryJoinNotReady);
        }
        record.phase = ApiaryEnrollmentPhase::Joining;
        tx.execute(
            "UPDATE apiary_enrollments SET record_json = ?2 WHERE link_id = ?1",
            params![link_id.to_string(), encode(&record)?],
        )?;
        tx.commit()?;
        Ok(record)
    }

    /// Records authenticated local consent before any enrollment side effect.
    /// The application must verify the disclosed Keeper offer before calling.
    /// Identical retries retain the original phase; consent cannot be replaced.
    ///
    /// # Errors
    /// Rejects a different local identity, missing link, changed consent, expired
    /// consent, a full journal, or persistence failure.
    pub fn save_apiary_enrollment(
        &self,
        consent: &ApiaryEnrollmentConsent,
        now: i64,
    ) -> Result<ApiaryEnrollment, TaskStoreError> {
        let identity = self.local_hive_identity()?;
        if identity.hive.apiary_id.is_some()
            || identity.hive.id != consent.member_hive_id
            || identity.operator.id != consent.member_operator_id
            || consent.policy_revision == 0
            || consent.accepted_at < 0
            || consent.accepted_at > now
            || consent.expires_at <= now
        {
            return Err(TaskStoreError::InvalidApiaryJoinLink);
        }
        let mut connection = self.connection()?;
        let tx = connection.transaction()?;
        let node: Option<String> = tx
            .query_row(
                "SELECT node_id FROM local_federation_identity WHERE singleton = 1",
                [],
                |row| row.get(0),
            )
            .optional()?;
        if node.as_deref() != Some(consent.member_node_id.to_string().as_str()) {
            return Err(TaskStoreError::InvalidApiaryJoinLink);
        }
        let existing: Option<String> = tx
            .query_row(
                "SELECT record_json FROM apiary_enrollments WHERE link_id = ?1",
                [consent.link_id.to_string()],
                |row| row.get(0),
            )
            .optional()?;
        if let Some(existing) = existing {
            let existing: ApiaryEnrollment = decode(&existing)?;
            return if existing.consent == *consent {
                Ok(existing)
            } else {
                Err(TaskStoreError::InvalidApiaryJoinLink)
            };
        }
        let record = ApiaryEnrollment {
            consent: consent.clone(),
            phase: ApiaryEnrollmentPhase::AwaitingApproval,
        };
        let changed = tx.execute(
            "INSERT INTO apiary_enrollments (link_id, record_json)
             SELECT link_id, ?2 FROM local_apiary_keeper_links
             WHERE link_id = ?1 AND state IN ('open', 'awaiting_approval', 'approved')
             AND (SELECT COUNT(*) FROM apiary_enrollments) < ?3",
            params![
                consent.link_id.to_string(),
                encode(&record)?,
                MAX_ENROLLMENTS
            ],
        )?;
        if changed != 1 {
            return Err(TaskStoreError::InvalidApiaryJoinLink);
        }
        tx.commit()?;
        Ok(record)
    }

    /// Loads a bounded journal, including terminal states for honest UI status.
    ///
    /// # Errors
    /// Rejects corrupt records or unavailable persistence.
    pub fn apiary_enrollments(&self) -> Result<Vec<ApiaryEnrollment>, TaskStoreError> {
        let connection = self.connection()?;
        let mut query = connection
            .prepare("SELECT record_json FROM apiary_enrollments ORDER BY link_id LIMIT ?1")?;
        query
            .query_map([MAX_ENROLLMENTS], |row| row.get::<_, String>(0))?
            .map(|row| decode(&row?))
            .collect()
    }

    /// Compare-and-swap progress so stale workers cannot undo cancellation.
    /// Exact retries succeed without changing consent or creating another row.
    ///
    /// # Errors
    /// Rejects forbidden transitions, stale phase, missing rows, and corruption.
    pub fn advance_apiary_enrollment(
        &self,
        link_id: ApiaryJoinLinkId,
        expected: ApiaryEnrollmentPhase,
        next: ApiaryEnrollmentPhase,
    ) -> Result<ApiaryEnrollment, TaskStoreError> {
        if !expected.can_transition_to(next) {
            return Err(TaskStoreError::InvalidApiaryJoinLink);
        }
        let mut connection = self.connection()?;
        let tx = connection.transaction()?;
        let stored: String = tx
            .query_row(
                "SELECT record_json FROM apiary_enrollments WHERE link_id = ?1",
                [link_id.to_string()],
                |row| row.get(0),
            )
            .optional()?
            .ok_or(TaskStoreError::ApiaryJoinLinkNotFound)?;
        let mut record: ApiaryEnrollment = decode(&stored)?;
        if record.phase == next {
            return Ok(record);
        }
        if record.phase != expected {
            return Err(TaskStoreError::ApiaryJoinLinkResolved);
        }
        record.phase = next;
        tx.execute(
            "UPDATE apiary_enrollments SET record_json = ?2 WHERE link_id = ?1",
            params![link_id.to_string(), encode(&record)?],
        )?;
        tx.commit()?;
        Ok(record)
    }
}

fn encode(record: &ApiaryEnrollment) -> Result<String, TaskStoreError> {
    serde_json::to_string(record)
        .map_err(|error| TaskStoreError::IntegrityFailure(error.to_string()))
}

fn decode(value: &str) -> Result<ApiaryEnrollment, TaskStoreError> {
    serde_json::from_str(value).map_err(|error| TaskStoreError::IntegrityFailure(error.to_string()))
}

#[cfg(test)]
mod tests;
