use rusqlite::{OptionalExtension, params};
use swarm_domain::{
    FederationClaimHandoff, FederationClaimHandoffId, FederationClaimHandoffState,
    FederationClaimId, FederationClaimState, FederationNodeId,
};

use crate::federation::{
    MemberCredentialContext, authenticate_member_credential, decode_node_credential,
    federation_claim_by_id,
};
use crate::{TaskStore, TaskStoreError, parse_domain_id};

const MAX_HANDOFF_REASON_BYTES: usize = 500;
const MAX_VISIBLE_HANDOFFS: i64 = 100;

impl TaskStore {
    /// Offers a confirmed claim to another active member. The source remains
    /// authoritative until the target confirms its Jira assignment succeeded.
    ///
    /// # Errors
    /// Rejects invalid credentials, actors, targets, claims, content, or a
    /// conflicting active handoff.
    pub fn offer_federation_claim_handoff(
        &self,
        node_credential: &str,
        claim_id: FederationClaimId,
        target_node_id: FederationNodeId,
        reason: Option<&str>,
        now: i64,
    ) -> Result<FederationClaimHandoff, TaskStoreError> {
        let reason = validate_reason(reason, now)?;
        let identity = self.local_hive_identity()?;
        let credential = decode_node_credential(node_credential)?;
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let source = authenticate_member_credential(&transaction, &identity, &credential, now)?;
        let claim = federation_claim_by_id(&transaction, claim_id)?
            .ok_or(TaskStoreError::InvalidFederationHandoff)?;
        if claim.apiary_id != source.apiary
            || claim.state != FederationClaimState::Confirmed
            || claim.home_node_id != source.node
            || target_node_id == source.node
        {
            return Err(TaskStoreError::FederationHandoffConflict);
        }
        let target = active_member_by_node(&transaction, source.apiary, target_node_id)?
            .ok_or(TaskStoreError::InvalidFederationHandoff)?;
        if let Some(existing) = active_handoff_for_claim(&transaction, claim_id)? {
            if existing.source_node_id == source.node
                && existing.target_node_id == target.node
                && existing.reason == reason
            {
                return Ok(existing);
            }
            return Err(TaskStoreError::FederationHandoffConflict);
        }
        let handoff = FederationClaimHandoff {
            id: FederationClaimHandoffId::new(),
            apiary_id: source.apiary,
            claim_id,
            project_id: claim.project_id,
            issue_id: claim.issue_id,
            issue_key: claim.issue_key,
            source_node_id: source.node,
            source_hive_id: source.hive,
            source_operator_id: source.operator,
            target_node_id: target.node,
            target_hive_id: target.hive,
            target_operator_id: target.operator,
            state: FederationClaimHandoffState::Offered,
            reason,
            offered_at: now,
            accepted_at: None,
            completed_at: None,
            closed_at: None,
        };
        insert_handoff(&transaction, &handoff)?;
        transaction.commit()?;
        Ok(handoff)
    }

    /// Lists handoffs visible to the authenticated member, newest first.
    ///
    /// # Errors
    /// Rejects invalid credentials, time, corrupt state, or unavailable storage.
    pub fn list_federation_claim_handoffs(
        &self,
        node_credential: &str,
        now: i64,
    ) -> Result<Vec<FederationClaimHandoff>, TaskStoreError> {
        if now < 0 {
            return Err(TaskStoreError::InvalidFederationHandoff);
        }
        let identity = self.local_hive_identity()?;
        let credential = decode_node_credential(node_credential)?;
        let connection = self.connection()?;
        let member = authenticate_member_credential(&connection, &identity, &credential, now)?;
        let mut statement = connection.prepare(
            "SELECT id, apiary_id, claim_id, project_id, issue_id, issue_key,
                    source_node_id, source_hive_id, source_operator_id,
                    target_node_id, target_hive_id, target_operator_id,
                    state, reason, offered_at, accepted_at, completed_at, closed_at
             FROM apiary_federation_claim_handoffs
             WHERE apiary_id = ?1 AND (source_node_id = ?2 OR target_node_id = ?2)
             ORDER BY CASE state WHEN 'offered' THEN 0 WHEN 'accepted' THEN 1 ELSE 2 END,
                      offered_at DESC
             LIMIT ?3",
        )?;
        let rows = statement.query_map(
            params![
                member.apiary.to_string(),
                member.node.to_string(),
                MAX_VISIBLE_HANDOFFS
            ],
            handoff_from_row,
        )?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    /// Accepts an offered handoff as its exact target member.
    ///
    /// # Errors
    /// Rejects invalid credentials, actors, IDs, or lifecycle transitions.
    pub fn accept_federation_claim_handoff(
        &self,
        node_credential: &str,
        handoff_id: FederationClaimHandoffId,
        now: i64,
    ) -> Result<FederationClaimHandoff, TaskStoreError> {
        self.transition_handoff(node_credential, handoff_id, now, HandoffAction::Accept)
    }

    /// Declines an offered handoff as its exact target member.
    ///
    /// # Errors
    /// Rejects invalid credentials, actors, IDs, or lifecycle transitions.
    pub fn decline_federation_claim_handoff(
        &self,
        node_credential: &str,
        handoff_id: FederationClaimHandoffId,
        now: i64,
    ) -> Result<FederationClaimHandoff, TaskStoreError> {
        self.transition_handoff(node_credential, handoff_id, now, HandoffAction::Decline)
    }

    /// Cancels an unaccepted offer as its exact source member.
    ///
    /// # Errors
    /// Rejects invalid credentials, actors, IDs, or lifecycle transitions.
    pub fn cancel_federation_claim_handoff(
        &self,
        node_credential: &str,
        handoff_id: FederationClaimHandoffId,
        now: i64,
    ) -> Result<FederationClaimHandoff, TaskStoreError> {
        self.transition_handoff(node_credential, handoff_id, now, HandoffAction::Cancel)
    }

    /// Confirms the target completed its own Jira assignment and atomically
    /// moves the Keeper's durable claim owner to that member.
    ///
    /// # Errors
    /// Rejects invalid credentials, actors, IDs, lifecycle transitions, or
    /// source-claim drift.
    pub fn confirm_federation_claim_handoff(
        &self,
        node_credential: &str,
        handoff_id: FederationClaimHandoffId,
        now: i64,
    ) -> Result<FederationClaimHandoff, TaskStoreError> {
        self.transition_handoff(node_credential, handoff_id, now, HandoffAction::Confirm)
    }

    fn transition_handoff(
        &self,
        node_credential: &str,
        handoff_id: FederationClaimHandoffId,
        now: i64,
        action: HandoffAction,
    ) -> Result<FederationClaimHandoff, TaskStoreError> {
        if now < 0 {
            return Err(TaskStoreError::InvalidFederationHandoff);
        }
        let identity = self.local_hive_identity()?;
        let credential = decode_node_credential(node_credential)?;
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let member = authenticate_member_credential(&transaction, &identity, &credential, now)?;
        let mut handoff = handoff_by_id(&transaction, handoff_id)?
            .ok_or(TaskStoreError::InvalidFederationHandoff)?;
        if handoff.apiary_id != member.apiary || !action.allowed_actor(&handoff, &member) {
            return Err(TaskStoreError::FederationHandoffConflict);
        }
        if handoff.state == action.result_state() {
            return Ok(handoff);
        }
        if !action.allowed_state(handoff.state) {
            return Err(TaskStoreError::FederationHandoffConflict);
        }
        if action == HandoffAction::Confirm {
            let claim = federation_claim_by_id(&transaction, handoff.claim_id)?
                .ok_or(TaskStoreError::InvalidFederationHandoff)?;
            if claim.state != FederationClaimState::Confirmed
                || claim.apiary_id != handoff.apiary_id
                || claim.home_node_id != handoff.source_node_id
                || claim.home_hive_id != handoff.source_hive_id
                || claim.home_operator_id != handoff.source_operator_id
            {
                return Err(TaskStoreError::FederationHandoffConflict);
            }
            transaction.execute(
                "UPDATE apiary_federation_claims
                 SET home_node_id = ?1, home_hive_id = ?2, home_operator_id = ?3,
                     updated_at = ?4
                 WHERE id = ?5 AND state = 'confirmed' AND home_node_id = ?6",
                params![
                    handoff.target_node_id.to_string(),
                    handoff.target_hive_id.to_string(),
                    handoff.target_operator_id.to_string(),
                    now,
                    handoff.claim_id.to_string(),
                    handoff.source_node_id.to_string()
                ],
            )?;
        }
        handoff.state = action.result_state();
        match action {
            HandoffAction::Accept => handoff.accepted_at = Some(now),
            HandoffAction::Confirm => handoff.completed_at = Some(now),
            HandoffAction::Decline | HandoffAction::Cancel => handoff.closed_at = Some(now),
        }
        transaction.execute(
            "UPDATE apiary_federation_claim_handoffs
             SET state = ?1, accepted_at = ?2, completed_at = ?3, closed_at = ?4,
                 updated_at = ?5 WHERE id = ?6",
            params![
                handoff.state.to_string(),
                handoff.accepted_at,
                handoff.completed_at,
                handoff.closed_at,
                now,
                handoff.id.to_string()
            ],
        )?;
        transaction.commit()?;
        Ok(handoff)
    }
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum HandoffAction {
    Accept,
    Confirm,
    Decline,
    Cancel,
}

impl HandoffAction {
    fn result_state(self) -> FederationClaimHandoffState {
        match self {
            Self::Accept => FederationClaimHandoffState::Accepted,
            Self::Confirm => FederationClaimHandoffState::Completed,
            Self::Decline => FederationClaimHandoffState::Declined,
            Self::Cancel => FederationClaimHandoffState::Cancelled,
        }
    }

    fn allowed_actor(
        self,
        handoff: &FederationClaimHandoff,
        member: &MemberCredentialContext,
    ) -> bool {
        match self {
            Self::Cancel => handoff.source_node_id == member.node,
            Self::Accept | Self::Confirm | Self::Decline => handoff.target_node_id == member.node,
        }
    }

    fn allowed_state(self, state: FederationClaimHandoffState) -> bool {
        matches!(
            (self, state),
            (
                Self::Accept | Self::Decline | Self::Cancel,
                FederationClaimHandoffState::Offered
            ) | (Self::Confirm, FederationClaimHandoffState::Accepted)
        )
    }
}

fn validate_reason(reason: Option<&str>, now: i64) -> Result<Option<String>, TaskStoreError> {
    if now < 0 {
        return Err(TaskStoreError::InvalidFederationHandoff);
    }
    reason
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| {
            if value.len() > MAX_HANDOFF_REASON_BYTES || value.chars().any(char::is_control) {
                Err(TaskStoreError::InvalidFederationHandoff)
            } else {
                Ok(value.to_owned())
            }
        })
        .transpose()
}

fn active_member_by_node(
    connection: &rusqlite::Connection,
    apiary_id: swarm_domain::ApiaryId,
    node_id: FederationNodeId,
) -> Result<Option<MemberCredentialContext>, TaskStoreError> {
    connection
        .query_row(
            "SELECT member_hive_id, member_operator_id FROM apiary_federation_memberships
         WHERE apiary_id = ?1 AND member_node_id = ?2 AND state = 'active'",
            params![apiary_id.to_string(), node_id.to_string()],
            |row| {
                Ok(MemberCredentialContext {
                    apiary: apiary_id,
                    node: node_id,
                    hive: parse_domain_id(&row.get::<_, String>(0)?)?,
                    operator: parse_domain_id(&row.get::<_, String>(1)?)?,
                })
            },
        )
        .optional()
        .map_err(Into::into)
}

fn active_handoff_for_claim(
    connection: &rusqlite::Connection,
    claim_id: FederationClaimId,
) -> Result<Option<FederationClaimHandoff>, TaskStoreError> {
    connection
        .query_row(
            &format!(
                "{} WHERE claim_id = ?1 AND state IN ('offered','accepted')",
                handoff_select()
            ),
            [claim_id.to_string()],
            handoff_from_row,
        )
        .optional()
        .map_err(Into::into)
}

fn handoff_by_id(
    connection: &rusqlite::Connection,
    id: FederationClaimHandoffId,
) -> Result<Option<FederationClaimHandoff>, TaskStoreError> {
    connection
        .query_row(
            &format!("{} WHERE id = ?1", handoff_select()),
            [id.to_string()],
            handoff_from_row,
        )
        .optional()
        .map_err(Into::into)
}

fn handoff_select() -> &'static str {
    "SELECT id, apiary_id, claim_id, project_id, issue_id, issue_key,
            source_node_id, source_hive_id, source_operator_id,
            target_node_id, target_hive_id, target_operator_id,
            state, reason, offered_at, accepted_at, completed_at, closed_at
     FROM apiary_federation_claim_handoffs"
}

fn handoff_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<FederationClaimHandoff> {
    Ok(FederationClaimHandoff {
        id: parse_domain_id(&row.get::<_, String>(0)?)?,
        apiary_id: parse_domain_id(&row.get::<_, String>(1)?)?,
        claim_id: parse_domain_id(&row.get::<_, String>(2)?)?,
        project_id: row.get(3)?,
        issue_id: row.get(4)?,
        issue_key: row.get(5)?,
        source_node_id: parse_domain_id(&row.get::<_, String>(6)?)?,
        source_hive_id: parse_domain_id(&row.get::<_, String>(7)?)?,
        source_operator_id: parse_domain_id(&row.get::<_, String>(8)?)?,
        target_node_id: parse_domain_id(&row.get::<_, String>(9)?)?,
        target_hive_id: parse_domain_id(&row.get::<_, String>(10)?)?,
        target_operator_id: parse_domain_id(&row.get::<_, String>(11)?)?,
        state: row
            .get::<_, String>(12)?
            .parse()
            .map_err(|()| rusqlite::Error::InvalidQuery)?,
        reason: row.get(13)?,
        offered_at: row.get(14)?,
        accepted_at: row.get(15)?,
        completed_at: row.get(16)?,
        closed_at: row.get(17)?,
    })
}

fn insert_handoff(
    connection: &rusqlite::Connection,
    handoff: &FederationClaimHandoff,
) -> Result<(), TaskStoreError> {
    connection.execute(
        "INSERT INTO apiary_federation_claim_handoffs
         (id, apiary_id, claim_id, project_id, issue_id, issue_key,
          source_node_id, source_hive_id, source_operator_id,
          target_node_id, target_hive_id, target_operator_id,
          state, reason, offered_at, accepted_at, completed_at, closed_at, updated_at)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?15)",
        params![
            handoff.id.to_string(),
            handoff.apiary_id.to_string(),
            handoff.claim_id.to_string(),
            handoff.project_id,
            handoff.issue_id,
            handoff.issue_key,
            handoff.source_node_id.to_string(),
            handoff.source_hive_id.to_string(),
            handoff.source_operator_id.to_string(),
            handoff.target_node_id.to_string(),
            handoff.target_hive_id.to_string(),
            handoff.target_operator_id.to_string(),
            handoff.state.to_string(),
            handoff.reason,
            handoff.offered_at,
            handoff.accepted_at,
            handoff.completed_at,
            handoff.closed_at
        ],
    )?;
    Ok(())
}

pub(crate) fn migrate_federation_handoffs(
    transaction: &rusqlite::Transaction<'_>,
) -> rusqlite::Result<()> {
    transaction.execute_batch(
        "CREATE TABLE IF NOT EXISTS apiary_federation_claim_handoffs (
             id TEXT PRIMARY KEY,
             apiary_id TEXT NOT NULL REFERENCES apiaries(id),
             claim_id TEXT NOT NULL REFERENCES apiary_federation_claims(id),
             project_id TEXT NOT NULL, issue_id TEXT NOT NULL, issue_key TEXT NOT NULL,
             source_node_id TEXT NOT NULL, source_hive_id TEXT NOT NULL REFERENCES hives(id),
             source_operator_id TEXT NOT NULL REFERENCES operators(id),
             target_node_id TEXT NOT NULL, target_hive_id TEXT NOT NULL REFERENCES hives(id),
             target_operator_id TEXT NOT NULL REFERENCES operators(id),
             state TEXT NOT NULL CHECK (state IN ('offered','accepted','completed','declined','cancelled')),
             reason TEXT, offered_at INTEGER NOT NULL CHECK (offered_at >= 0),
             accepted_at INTEGER, completed_at INTEGER, closed_at INTEGER,
             updated_at INTEGER NOT NULL CHECK (updated_at >= offered_at),
             CHECK (source_node_id <> target_node_id)
         );
         CREATE UNIQUE INDEX IF NOT EXISTS one_active_handoff_per_claim
             ON apiary_federation_claim_handoffs(claim_id)
             WHERE state IN ('offered','accepted');
         CREATE INDEX IF NOT EXISTS handoffs_by_source
             ON apiary_federation_claim_handoffs(apiary_id, source_node_id, state);
         CREATE INDEX IF NOT EXISTS handoffs_by_target
             ON apiary_federation_claim_handoffs(apiary_id, target_node_id, state);
         PRAGMA user_version = 54;"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schema_v53_adds_atomic_claim_handoffs_without_replacing_identity() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("swarm-next.sqlite3");
        let store = TaskStore::open(&path).unwrap();
        let identity = store.local_hive_identity().unwrap();
        store
            .connection()
            .unwrap()
            .execute_batch(
                "DROP TABLE apiary_federation_claim_handoffs;
                 PRAGMA user_version = 53;",
            )
            .unwrap();
        drop(store);

        let migrated = TaskStore::open(path).unwrap();
        assert_eq!(migrated.local_hive_identity().unwrap(), identity);
        assert_eq!(
            migrated
                .connection()
                .unwrap()
                .query_row(
                    "SELECT COUNT(*) FROM sqlite_master
                     WHERE type = 'table' AND name = 'apiary_federation_claim_handoffs'",
                    [],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap(),
            1
        );
    }
}
