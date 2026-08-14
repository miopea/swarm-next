use rusqlite::{OptionalExtension, params};
use swarm_domain::{
    Apiary, ApiaryCollapseReadiness, ApiaryId, ApiaryInvitation, ApiaryInvitationId,
    ApiaryInvitationState, ApiaryJiraProject, ApiaryJoinReadiness, HiveId, JiraProjectBindingId,
    JiraProjectScope, OperatorId, SharedWorkBackend,
};

use crate::{TaskStore, TaskStoreError, parse_domain_id};

const MAX_INVITATION_LIFETIME_SECONDS: i64 = 30 * 24 * 60 * 60;
const MAX_APIARY_NAME_BYTES: usize = 120;
const MAX_PROJECT_ID_BYTES: usize = 128;
const MAX_PROJECT_KEY_BYTES: usize = 64;
const MAX_PROJECT_NAME_BYTES: usize = 240;

impl TaskStore {
    /// Atomically creates one Apiary around the local personal Hive. The local
    /// operator becomes Keeper and the backend is immutable after creation.
    ///
    /// # Errors
    /// Returns an error for invalid naming, time, or existing membership.
    pub fn create_apiary_for_local_hive(
        &self,
        name: &str,
        shared_work_backend: swarm_domain::SharedWorkBackend,
        now: i64,
    ) -> Result<swarm_domain::LocalApiaryContext, TaskStoreError> {
        let name = name.trim();
        if now < 0 || name.is_empty() || name.len() > MAX_APIARY_NAME_BYTES {
            return Err(TaskStoreError::InvalidApiary);
        }
        let identity = self.local_hive_identity()?;
        if identity.hive.apiary_id.is_some() {
            return Err(TaskStoreError::ApiaryMembershipConflict);
        }
        let apiary = Apiary::new(name, identity.operator.id, shared_work_backend);
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        transaction.execute(
            "INSERT INTO apiaries
                (id, name, keeper_operator_id, shared_work_backend, policy_revision,
                 created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)",
            params![
                apiary.id.to_string(),
                &apiary.name,
                apiary.keeper_operator_id.to_string(),
                apiary.shared_work_backend().to_string(),
                apiary.policy_revision(),
                now
            ],
        )?;
        if transaction.execute(
            "UPDATE hives SET apiary_id = ?1, updated_at = ?2
             WHERE id = ?3 AND apiary_id IS NULL",
            params![apiary.id.to_string(), now, identity.hive.id.to_string()],
        )? != 1
        {
            return Err(TaskStoreError::ApiaryMembershipConflict);
        }
        transaction.execute(
            "INSERT INTO apiary_lifecycle_events
                (apiary_id, actor_operator_id, hive_id, kind, occurred_at)
             VALUES (?1, ?2, ?3, 'founded', ?4)",
            params![
                apiary.id.to_string(),
                identity.operator.id.to_string(),
                identity.hive.id.to_string(),
                now
            ],
        )?;
        transaction.commit()?;
        drop(connection);
        self.local_apiary_context()
    }

    /// Returns one durable Apiary by identity without exposing membership or credentials.
    ///
    /// # Errors
    /// Returns an error when the Apiary does not exist or persisted data is invalid.
    pub fn get_apiary(&self, apiary_id: ApiaryId) -> Result<Apiary, TaskStoreError> {
        let connection = self.connection()?;
        connection
            .query_row(
                "SELECT id, name, keeper_operator_id, shared_work_backend, policy_revision
                 FROM apiaries WHERE id = ?1",
                [apiary_id.to_string()],
                |row| {
                    Ok(Apiary::persisted(
                        parse_domain_id(&row.get::<_, String>(0)?)?,
                        row.get::<_, String>(1)?,
                        parse_domain_id(&row.get::<_, String>(2)?)?,
                        row.get::<_, String>(3)?
                            .parse()
                            .map_err(|_| rusqlite::Error::InvalidQuery)?,
                        row.get::<_, u64>(4)?,
                    ))
                },
            )
            .optional()?
            .ok_or(TaskStoreError::ApiaryNotFound)
    }

    /// Derives the complete persisted safety boundary for collapsing the local
    /// Keeper's Apiary. Zero-valued distributed counters are trustworthy while
    /// those capabilities have no persistence surface and therefore cannot
    /// create state.
    ///
    /// # Errors
    /// Returns an error when the Apiary or its durable federation state cannot be read.
    pub fn apiary_collapse_readiness(
        &self,
        apiary_id: ApiaryId,
    ) -> Result<ApiaryCollapseReadiness, TaskStoreError> {
        let connection = self.connection()?;
        collapse_readiness(&connection, apiary_id)
    }

    /// Atomically converts a sole Keeper Apiary back into a personal Hive.
    /// Jira bindings become Hive-owned while the inactive Apiary and lifecycle
    /// event remain durable for identity and audit history.
    ///
    /// # Errors
    /// Rejects non-Keepers, stale readiness, invalid time, or persistence failures.
    pub fn collapse_local_apiary(
        &self,
        now: i64,
    ) -> Result<swarm_domain::LocalApiaryContext, TaskStoreError> {
        if now < 0 {
            return Err(TaskStoreError::InvalidApiary);
        }
        let identity = self.local_hive_identity()?;
        let apiary_id = identity
            .hive
            .apiary_id
            .ok_or(TaskStoreError::ApiaryNotFound)?;
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let keeper_operator_id = transaction
            .query_row(
                "SELECT keeper_operator_id FROM apiaries
                 WHERE id = ?1 AND collapsed_at IS NULL",
                [apiary_id.to_string()],
                |row| row.get::<_, String>(0),
            )
            .optional()?
            .ok_or(TaskStoreError::ApiaryNotFound)?;
        if keeper_operator_id != identity.operator.id.to_string()
            || !collapse_readiness(&transaction, apiary_id)?.can_collapse()
        {
            return Err(TaskStoreError::ApiaryCollapseNotReady);
        }
        transaction.execute(
            "UPDATE jira_project_bindings
             SET scope = 'hive', apiary_id = NULL, updated_at = ?1
             WHERE hive_id = ?2 AND scope = 'apiary' AND apiary_id = ?3",
            params![now, identity.hive.id.to_string(), apiary_id.to_string()],
        )?;
        if transaction.execute(
            "UPDATE hives SET apiary_id = NULL, updated_at = ?1
             WHERE id = ?2 AND apiary_id = ?3",
            params![now, identity.hive.id.to_string(), apiary_id.to_string()],
        )? != 1
        {
            return Err(TaskStoreError::ApiaryCollapseNotReady);
        }
        if transaction.execute(
            "UPDATE apiaries SET collapsed_at = ?1, updated_at = ?1
             WHERE id = ?2 AND collapsed_at IS NULL",
            params![now, apiary_id.to_string()],
        )? != 1
        {
            return Err(TaskStoreError::ApiaryCollapseNotReady);
        }
        transaction.execute(
            "INSERT INTO apiary_lifecycle_events
                (apiary_id, actor_operator_id, hive_id, kind, occurred_at)
             VALUES (?1, ?2, ?3, 'collapsed', ?4)",
            params![
                apiary_id.to_string(),
                identity.operator.id.to_string(),
                identity.hive.id.to_string(),
                now
            ],
        )?;
        transaction.commit()?;
        drop(connection);
        self.local_apiary_context()
    }

    /// Adds or refreshes one Jira project in the Keeper-owned Apiary catalog.
    /// Database constraints reject Native Apiaries and non-Keeper promoters.
    ///
    /// # Errors
    /// Returns an error for invalid project identity, unauthorized promotion, or persistence.
    pub fn promote_apiary_jira_project(
        &self,
        apiary_id: ApiaryId,
        project_id: &str,
        project_key: &str,
        project_name: &str,
        promoted_by_operator_id: OperatorId,
        now: i64,
    ) -> Result<ApiaryJiraProject, TaskStoreError> {
        let project_id = project_id.trim();
        let project_key = project_key.trim();
        let project_name = project_name.trim();
        if now < 0
            || project_id.is_empty()
            || project_id.len() > MAX_PROJECT_ID_BYTES
            || project_key.is_empty()
            || project_key.len() > MAX_PROJECT_KEY_BYTES
            || project_name.is_empty()
            || project_name.len() > MAX_PROJECT_NAME_BYTES
        {
            return Err(TaskStoreError::InvalidJiraProject);
        }
        let connection = self.connection()?;
        connection.execute(
            "INSERT INTO apiary_jira_projects
                (apiary_id, project_id, project_key, project_name,
                 promoted_by_operator_id, promoted_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(apiary_id, project_id) DO UPDATE SET
                 project_key = excluded.project_key,
                 project_name = excluded.project_name,
                 updated_at = unixepoch()",
            params![
                apiary_id.to_string(),
                project_id,
                project_key,
                project_name,
                promoted_by_operator_id.to_string(),
                now
            ],
        )?;
        apiary_jira_project(&connection, apiary_id, project_id)?
            .ok_or(TaskStoreError::JiraProjectBindingNotFound)
    }

    /// Lists the authoritative promoted Jira catalog for one Apiary.
    ///
    /// # Errors
    /// Returns an error when persisted catalog data cannot be read.
    pub fn list_apiary_jira_projects(
        &self,
        apiary_id: ApiaryId,
    ) -> Result<Vec<ApiaryJiraProject>, TaskStoreError> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT apiary_id, project_id, project_key, project_name,
                    promoted_by_operator_id, promoted_at
             FROM apiary_jira_projects WHERE apiary_id = ?1
             ORDER BY project_name COLLATE NOCASE, project_key",
        )?;
        Ok(statement
            .query_map([apiary_id.to_string()], apiary_jira_project_from_row)?
            .collect::<Result<Vec<_>, _>>()?)
    }

    /// Atomically promotes one ready local Hive Jira binding into the Keeper's
    /// Apiary catalog and changes the local binding to Apiary scope. Existing
    /// issue links and workflow mappings remain attached to the same binding.
    ///
    /// # Errors
    /// Rejects personal or member Hives, Native Apiaries, incomplete local
    /// access or workflow evidence, foreign bindings, invalid time, and
    /// persistence failures.
    pub fn promote_local_jira_binding_to_apiary(
        &self,
        binding_id: JiraProjectBindingId,
        now: i64,
    ) -> Result<ApiaryJiraProject, TaskStoreError> {
        if now < 0 {
            return Err(TaskStoreError::InvalidJiraProject);
        }
        let identity = self.local_hive_identity()?;
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let candidate = transaction
            .query_row(
                "SELECT a.id, a.keeper_operator_id, a.shared_work_backend,
                        b.project_id, b.project_key, b.project_name, b.scope,
                        b.apiary_id, b.access_verified, b.workflow_mapped
                 FROM hives h
                 JOIN apiaries a ON a.id = h.apiary_id AND a.collapsed_at IS NULL
                 JOIN jira_project_bindings b ON b.hive_id = h.id
                 WHERE h.id = ?1 AND b.id = ?2",
                params![identity.hive.id.to_string(), binding_id.to_string()],
                |row| {
                    Ok((
                        parse_domain_id::<ApiaryId>(&row.get::<_, String>(0)?)?,
                        parse_domain_id::<OperatorId>(&row.get::<_, String>(1)?)?,
                        row.get::<_, String>(2)?
                            .parse::<SharedWorkBackend>()
                            .map_err(|_| rusqlite::Error::InvalidQuery)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, String>(4)?,
                        row.get::<_, String>(5)?,
                        row.get::<_, String>(6)?
                            .parse::<JiraProjectScope>()
                            .map_err(|_| rusqlite::Error::InvalidQuery)?,
                        row.get::<_, Option<String>>(7)?
                            .as_deref()
                            .map(parse_domain_id::<ApiaryId>)
                            .transpose()?,
                        row.get::<_, bool>(8)?,
                        row.get::<_, bool>(9)?,
                    ))
                },
            )
            .optional()?
            .ok_or(TaskStoreError::ApiaryProjectPromotionNotReady)?;
        let (
            apiary_id,
            keeper_operator_id,
            backend,
            project_id,
            project_key,
            project_name,
            scope,
            binding_apiary_id,
            access_verified,
            workflow_mapped,
        ) = candidate;
        let scope_is_valid = scope == JiraProjectScope::Hive
            || (scope == JiraProjectScope::Apiary && binding_apiary_id == Some(apiary_id));
        if keeper_operator_id != identity.operator.id
            || backend != SharedWorkBackend::Jira
            || !access_verified
            || !workflow_mapped
            || !scope_is_valid
        {
            return Err(TaskStoreError::ApiaryProjectPromotionNotReady);
        }
        transaction.execute(
            "INSERT INTO apiary_jira_projects
                (apiary_id, project_id, project_key, project_name,
                 promoted_by_operator_id, promoted_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(apiary_id, project_id) DO UPDATE SET
                 project_key = excluded.project_key,
                 project_name = excluded.project_name",
            params![
                apiary_id.to_string(),
                project_id,
                project_key,
                project_name,
                identity.operator.id.to_string(),
                now
            ],
        )?;
        if transaction.execute(
            "UPDATE jira_project_bindings
             SET scope = 'apiary', apiary_id = ?1, updated_at = ?2
             WHERE id = ?3 AND hive_id = ?4",
            params![
                apiary_id.to_string(),
                now,
                binding_id.to_string(),
                identity.hive.id.to_string()
            ],
        )? != 1
        {
            return Err(TaskStoreError::ApiaryProjectPromotionNotReady);
        }
        let promoted = apiary_jira_project(&transaction, apiary_id, &project_id)?
            .ok_or(TaskStoreError::ApiaryProjectPromotionNotReady)?;
        transaction.commit()?;
        Ok(promoted)
    }

    /// Returns true only when every promoted project has a matching local,
    /// access-verified, fully mapped Apiary binding for the invited Hive.
    ///
    /// # Errors
    /// Returns an error when the local Hive or catalog cannot be read.
    pub fn apiary_jira_project_access_ready(
        &self,
        apiary_id: ApiaryId,
    ) -> Result<bool, TaskStoreError> {
        let hive_id = self.local_hive_identity()?.hive.id;
        let connection = self.connection()?;
        connection
            .query_row(
                "SELECT NOT EXISTS (
                     SELECT 1 FROM apiary_jira_projects p
                     WHERE p.apiary_id = ?1
                       AND NOT EXISTS (
                           SELECT 1 FROM jira_project_bindings b
                           WHERE b.hive_id = ?2
                             AND b.project_id = p.project_id
                             AND b.scope = 'apiary'
                             AND b.apiary_id = p.apiary_id
                             AND b.access_verified = 1
                             AND b.workflow_mapped = 1
                       )
                 )",
                params![apiary_id.to_string(), hive_id.to_string()],
                |row| row.get(0),
            )
            .map_err(Into::into)
    }

    /// Creates one bounded Keeper invitation for a currently personal Hive.
    /// Database constraints reject non-Keeper issuers and duplicate pending invitations.
    ///
    /// # Errors
    /// Returns an error for invalid bounds, membership conflicts, or unauthorized issuers.
    pub fn create_apiary_invitation(
        &self,
        apiary_id: ApiaryId,
        invited_hive_id: HiveId,
        invited_by_operator_id: OperatorId,
        now: i64,
        expires_at: i64,
    ) -> Result<ApiaryInvitation, TaskStoreError> {
        if now < 0
            || expires_at <= now
            || expires_at.saturating_sub(now) > MAX_INVITATION_LIFETIME_SECONDS
        {
            return Err(TaskStoreError::InvalidApiaryInvitation);
        }
        let id = ApiaryInvitationId::new();
        let connection = self.connection()?;
        connection.execute(
            "INSERT INTO apiary_invitations
                (id, apiary_id, invited_hive_id, invited_by_operator_id, created_at, expires_at,
                 required_policy_revision)
             SELECT ?1, ?2, ?3, ?4, ?5, ?6, a.policy_revision
             FROM apiaries a WHERE a.id = ?2",
            params![
                id.to_string(),
                apiary_id.to_string(),
                invited_hive_id.to_string(),
                invited_by_operator_id.to_string(),
                now,
                expires_at
            ],
        )?;
        invitation_by_id(&connection, id)?.ok_or(TaskStoreError::ApiaryInvitationNotFound)
    }

    /// Records the invited Hive operator's acceptance of the exact current
    /// Apiary policy revision. The caller supplies an action, not readiness;
    /// ownership and revision are derived and checked in the update predicate.
    ///
    /// # Errors
    /// Returns an error when actor, invitation state, or revision is stale.
    pub fn accept_apiary_policy(
        &self,
        invitation_id: ApiaryInvitationId,
        actor_operator_id: OperatorId,
        policy_revision: u64,
        now: i64,
    ) -> Result<ApiaryInvitation, TaskStoreError> {
        if policy_revision == 0 || now < 0 {
            return Err(TaskStoreError::InvalidApiaryInvitation);
        }
        let connection = self.connection()?;
        let changed = connection.execute(
            "UPDATE apiary_invitations
             SET accepted_policy_revision = ?1, policy_accepted_at = ?2
             WHERE id = ?3 AND state = 'pending' AND required_policy_revision = ?1
               AND EXISTS (
                   SELECT 1 FROM apiaries a
                   WHERE a.id = apiary_invitations.apiary_id
                     AND a.policy_revision = ?1
               )
               AND EXISTS (
                   SELECT 1 FROM hives h
                   WHERE h.id = apiary_invitations.invited_hive_id
                     AND h.operator_id = ?4
               )",
            params![
                policy_revision,
                now,
                invitation_id.to_string(),
                actor_operator_id.to_string()
            ],
        )?;
        if changed != 1 {
            return Err(TaskStoreError::ApiaryJoinNotReady);
        }
        invitation_by_id(&connection, invitation_id)?
            .ok_or(TaskStoreError::ApiaryInvitationNotFound)
    }

    /// Returns one durable invitation by identity.
    ///
    /// # Errors
    /// Returns an error for invalid persisted data or a missing invitation.
    pub fn get_apiary_invitation(
        &self,
        invitation_id: ApiaryInvitationId,
    ) -> Result<ApiaryInvitation, TaskStoreError> {
        let connection = self.connection()?;
        invitation_by_id(&connection, invitation_id)?
            .ok_or(TaskStoreError::ApiaryInvitationNotFound)
    }

    /// Lists only pending, unexpired invitations for one Hive.
    ///
    /// # Errors
    /// Returns an error when persisted invitation data cannot be read.
    pub fn pending_apiary_invitations_for_hive(
        &self,
        hive_id: HiveId,
        now: i64,
    ) -> Result<Vec<ApiaryInvitation>, TaskStoreError> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT id, apiary_id, invited_hive_id, invited_by_operator_id, state,
                    created_at, expires_at, resolved_at, required_policy_revision,
                    accepted_policy_revision, policy_accepted_at
             FROM apiary_invitations
             WHERE invited_hive_id = ?1 AND state = 'pending' AND expires_at > ?2
             ORDER BY created_at, id",
        )?;
        Ok(statement
            .query_map(params![hive_id.to_string(), now], invitation_from_row)?
            .collect::<Result<Vec<_>, _>>()?)
    }

    /// Atomically joins the invited Hive after every typed readiness check passes.
    /// Other pending invitations for that Hive are revoked in the same transaction.
    ///
    /// # Errors
    /// Returns an error when readiness, invitation state, expiry, or membership changed.
    pub fn accept_apiary_invitation(
        &self,
        invitation_id: ApiaryInvitationId,
        readiness: &ApiaryJoinReadiness,
        now: i64,
    ) -> Result<ApiaryInvitation, TaskStoreError> {
        if !readiness.can_join() {
            return Err(TaskStoreError::ApiaryJoinNotReady);
        }
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let invitation = invitation_by_id(&transaction, invitation_id)?
            .ok_or(TaskStoreError::ApiaryInvitationNotFound)?;
        if invitation.state != ApiaryInvitationState::Pending {
            return Err(TaskStoreError::ApiaryInvitationResolved);
        }
        if invitation.expires_at <= now
            || invitation.apiary_id != readiness.apiary_id()
            || invitation.invited_hive_id != readiness.hive_id()
        {
            return Err(TaskStoreError::ApiaryJoinNotReady);
        }
        if transaction.execute(
            "UPDATE hives SET apiary_id = ?1, updated_at = ?2
             WHERE id = ?3 AND apiary_id IS NULL",
            params![
                invitation.apiary_id.to_string(),
                now,
                invitation.invited_hive_id.to_string()
            ],
        )? != 1
        {
            return Err(TaskStoreError::ApiaryJoinNotReady);
        }
        transaction.execute(
            "UPDATE apiary_invitations
             SET state = 'accepted', resolved_at = ?1
             WHERE id = ?2 AND state = 'pending'",
            params![now, invitation_id.to_string()],
        )?;
        transaction.execute(
            "UPDATE apiary_invitations
             SET state = 'revoked', resolved_at = ?1
             WHERE invited_hive_id = ?2 AND id <> ?3 AND state = 'pending'",
            params![
                now,
                invitation.invited_hive_id.to_string(),
                invitation_id.to_string()
            ],
        )?;
        let accepted = invitation_by_id(&transaction, invitation_id)?
            .ok_or(TaskStoreError::ApiaryInvitationNotFound)?;
        transaction.commit()?;
        Ok(accepted)
    }
}

pub(super) fn migrate_apiary_invitations(
    transaction: &rusqlite::Transaction<'_>,
) -> rusqlite::Result<()> {
    transaction.execute_batch(
        "CREATE TABLE IF NOT EXISTS apiary_invitations (
             id TEXT PRIMARY KEY,
             apiary_id TEXT NOT NULL REFERENCES apiaries(id),
             invited_hive_id TEXT NOT NULL REFERENCES hives(id),
             invited_by_operator_id TEXT NOT NULL REFERENCES operators(id),
             state TEXT NOT NULL DEFAULT 'pending'
                 CHECK (state IN ('pending','accepted','revoked','expired')),
             created_at INTEGER NOT NULL,
             expires_at INTEGER NOT NULL CHECK (expires_at > created_at),
             resolved_at INTEGER,
             CHECK ((state = 'pending' AND resolved_at IS NULL) OR
                    (state <> 'pending' AND resolved_at IS NOT NULL))
         );
         CREATE UNIQUE INDEX IF NOT EXISTS one_pending_invitation_per_apiary_hive
             ON apiary_invitations(apiary_id, invited_hive_id) WHERE state = 'pending';
         CREATE TRIGGER IF NOT EXISTS apiary_invitation_keeper_insert
             BEFORE INSERT ON apiary_invitations
             WHEN NOT EXISTS (
                 SELECT 1 FROM apiaries a
                 WHERE a.id = NEW.apiary_id
                   AND a.keeper_operator_id = NEW.invited_by_operator_id
             )
             BEGIN SELECT RAISE(ABORT, 'Only the Apiary Keeper can invite a Hive'); END;
         CREATE TRIGGER IF NOT EXISTS apiary_invitation_personal_hive_insert
             BEFORE INSERT ON apiary_invitations
             WHEN NOT EXISTS (
                 SELECT 1 FROM hives h
                 WHERE h.id = NEW.invited_hive_id AND h.apiary_id IS NULL
             )
             BEGIN SELECT RAISE(ABORT, 'Hive must leave its Apiary before invitation'); END;
         CREATE TRIGGER IF NOT EXISTS immutable_apiary_invitation_identity
             BEFORE UPDATE OF id, apiary_id, invited_hive_id, invited_by_operator_id,
                              created_at, expires_at
             ON apiary_invitations
             BEGIN SELECT RAISE(ABORT, 'Apiary invitation identity is immutable'); END;
         CREATE TRIGGER IF NOT EXISTS apiary_invitation_terminal_state
             BEFORE UPDATE OF state ON apiary_invitations
             WHEN OLD.state <> 'pending'
                OR NEW.state NOT IN ('accepted','revoked','expired')
             BEGIN SELECT RAISE(ABORT, 'Apiary invitation transition is invalid'); END;
         PRAGMA user_version = 26;",
    )
}

pub(super) fn migrate_apiary_jira_projects(
    transaction: &rusqlite::Transaction<'_>,
) -> rusqlite::Result<()> {
    transaction.execute_batch(
        "CREATE TABLE IF NOT EXISTS apiary_jira_projects (
             apiary_id TEXT NOT NULL REFERENCES apiaries(id),
             project_id TEXT NOT NULL,
             project_key TEXT NOT NULL,
             project_name TEXT NOT NULL,
             promoted_by_operator_id TEXT NOT NULL REFERENCES operators(id),
             promoted_at INTEGER NOT NULL,
             updated_at INTEGER NOT NULL DEFAULT (unixepoch()),
             PRIMARY KEY (apiary_id, project_id)
         );
         CREATE TRIGGER IF NOT EXISTS apiary_jira_project_keeper_insert
             BEFORE INSERT ON apiary_jira_projects
             WHEN NOT EXISTS (
                 SELECT 1 FROM apiaries a
                 WHERE a.id = NEW.apiary_id
                   AND a.shared_work_backend = 'jira'
                   AND a.keeper_operator_id = NEW.promoted_by_operator_id
             )
             BEGIN SELECT RAISE(ABORT, 'Only a Jira Apiary Keeper can promote a project'); END;
         CREATE TRIGGER IF NOT EXISTS immutable_apiary_jira_project_identity
             BEFORE UPDATE OF apiary_id, project_id, promoted_by_operator_id, promoted_at
             ON apiary_jira_projects
             BEGIN SELECT RAISE(ABORT, 'Promoted Apiary project identity is immutable'); END;
         PRAGMA user_version = 27;",
    )
}

pub(super) fn migrate_apiary_policy_acceptance(
    transaction: &rusqlite::Transaction<'_>,
) -> rusqlite::Result<()> {
    if table_exists(transaction, "apiaries")?
        && !column_exists(transaction, "apiaries", "policy_revision")?
    {
        transaction.execute_batch(
            "ALTER TABLE apiaries
                 ADD COLUMN policy_revision INTEGER NOT NULL DEFAULT 1
                     CHECK (policy_revision > 0);",
        )?;
    }
    if table_exists(transaction, "apiary_invitations")? {
        if !column_exists(
            transaction,
            "apiary_invitations",
            "required_policy_revision",
        )? {
            transaction.execute_batch(
                "ALTER TABLE apiary_invitations
                     ADD COLUMN required_policy_revision INTEGER NOT NULL DEFAULT 1
                         CHECK (required_policy_revision > 0);",
            )?;
        }
        if !column_exists(
            transaction,
            "apiary_invitations",
            "accepted_policy_revision",
        )? {
            transaction.execute_batch(
                "ALTER TABLE apiary_invitations
                     ADD COLUMN accepted_policy_revision INTEGER
                         CHECK (accepted_policy_revision IS NULL OR accepted_policy_revision > 0);",
            )?;
        }
        if !column_exists(transaction, "apiary_invitations", "policy_accepted_at")? {
            transaction.execute_batch(
                "ALTER TABLE apiary_invitations ADD COLUMN policy_accepted_at INTEGER;",
            )?;
        }
        transaction.execute_batch(
            "DROP TRIGGER IF EXISTS immutable_apiary_invitation_identity;
             CREATE TRIGGER immutable_apiary_invitation_identity
                 BEFORE UPDATE OF id, apiary_id, invited_hive_id, invited_by_operator_id,
                                  created_at, expires_at, required_policy_revision
                 ON apiary_invitations
                 BEGIN SELECT RAISE(ABORT, 'Apiary invitation identity is immutable'); END;
             CREATE TRIGGER IF NOT EXISTS apiary_invitation_policy_acceptance_guard
                 BEFORE UPDATE OF accepted_policy_revision, policy_accepted_at
                 ON apiary_invitations
                 WHEN OLD.state <> 'pending'
                    OR NEW.accepted_policy_revision IS NULL
                    OR NEW.accepted_policy_revision <> NEW.required_policy_revision
                    OR NEW.policy_accepted_at IS NULL
                 BEGIN SELECT RAISE(ABORT, 'Apiary policy acceptance is invalid'); END;",
        )?;
    }
    transaction.execute_batch("PRAGMA user_version = 28;")
}

pub(super) fn migrate_apiary_lifecycle(
    transaction: &rusqlite::Transaction<'_>,
) -> rusqlite::Result<()> {
    // Some isolated historical migration tests intentionally contain only the
    // table owned by that migration. A real v28 store always has the complete
    // Hive schema; keep partial fixtures schema-aware instead of inventing it.
    if !(table_exists(transaction, "apiaries")?
        && table_exists(transaction, "operators")?
        && table_exists(transaction, "hives")?
        && table_exists(transaction, "apiary_invitations")?
        && table_exists(transaction, "apiary_jira_projects")?)
    {
        return transaction.execute_batch("PRAGMA user_version = 29;");
    }
    if !column_exists(transaction, "apiaries", "collapsed_at")? {
        transaction.execute_batch("ALTER TABLE apiaries ADD COLUMN collapsed_at INTEGER;")?;
    }
    transaction.execute_batch(
        "CREATE TABLE IF NOT EXISTS apiary_lifecycle_events (
             sequence INTEGER PRIMARY KEY AUTOINCREMENT,
             apiary_id TEXT NOT NULL REFERENCES apiaries(id),
             actor_operator_id TEXT NOT NULL REFERENCES operators(id),
             hive_id TEXT NOT NULL REFERENCES hives(id),
             kind TEXT NOT NULL CHECK (kind IN ('founded','collapsed')),
             occurred_at INTEGER NOT NULL
         );
         CREATE INDEX IF NOT EXISTS apiary_lifecycle_events_by_apiary
             ON apiary_lifecycle_events(apiary_id, sequence);
         CREATE TRIGGER IF NOT EXISTS active_apiary_hive_insert
             BEFORE INSERT ON hives WHEN NEW.apiary_id IS NOT NULL AND NOT EXISTS (
                 SELECT 1 FROM apiaries a
                 WHERE a.id = NEW.apiary_id AND a.collapsed_at IS NULL
             )
             BEGIN SELECT RAISE(ABORT, 'Hive cannot join an inactive Apiary'); END;
         CREATE TRIGGER IF NOT EXISTS active_apiary_hive_update
             BEFORE UPDATE OF apiary_id ON hives WHEN NEW.apiary_id IS NOT NULL AND NOT EXISTS (
                 SELECT 1 FROM apiaries a
                 WHERE a.id = NEW.apiary_id AND a.collapsed_at IS NULL
             )
             BEGIN SELECT RAISE(ABORT, 'Hive cannot join an inactive Apiary'); END;
         CREATE TRIGGER IF NOT EXISTS active_apiary_invitation_insert
             BEFORE INSERT ON apiary_invitations WHEN NOT EXISTS (
                 SELECT 1 FROM apiaries a
                 WHERE a.id = NEW.apiary_id AND a.collapsed_at IS NULL
             )
             BEGIN SELECT RAISE(ABORT, 'Inactive Apiary cannot invite a Hive'); END;
         CREATE TRIGGER IF NOT EXISTS active_apiary_project_insert
             BEFORE INSERT ON apiary_jira_projects WHEN NOT EXISTS (
                 SELECT 1 FROM apiaries a
                 WHERE a.id = NEW.apiary_id AND a.collapsed_at IS NULL
             )
             BEGIN SELECT RAISE(ABORT, 'Inactive Apiary cannot promote a project'); END;
         PRAGMA user_version = 29;",
    )
}

fn collapse_readiness(
    connection: &rusqlite::Connection,
    apiary_id: ApiaryId,
) -> Result<ApiaryCollapseReadiness, TaskStoreError> {
    let active = connection
        .query_row(
            "SELECT collapsed_at IS NULL FROM apiaries WHERE id = ?1",
            [apiary_id.to_string()],
            |row| row.get::<_, bool>(0),
        )
        .optional()?
        .ok_or(TaskStoreError::ApiaryNotFound)?;
    if !active {
        return Err(TaskStoreError::ApiaryNotFound);
    }
    let count = |sql: &str| -> Result<usize, TaskStoreError> {
        let value =
            connection.query_row(sql, [apiary_id.to_string()], |row| row.get::<_, i64>(0))?;
        usize::try_from(value).map_err(|_| TaskStoreError::Sql(rusqlite::Error::InvalidQuery))
    };
    Ok(ApiaryCollapseReadiness {
        active_hive_count: count("SELECT COUNT(*) FROM hives WHERE apiary_id = ?1")?,
        pending_invitation_count: count(
            "SELECT COUNT(*) FROM apiary_invitations WHERE apiary_id = ?1 AND state = 'pending'",
        )?,
        active_stewardship_count: count(
            "SELECT COUNT(*) FROM stewardships WHERE apiary_id = ?1 AND revoked_at IS NULL",
        )?,
        // Cross-Hive work and departed execution nodes have no persistence
        // surface yet, so they cannot currently contain hidden durable state.
        open_cross_hive_work_count: 0,
        departed_node_count: 0,
    })
}

fn table_exists(transaction: &rusqlite::Transaction<'_>, table: &str) -> rusqlite::Result<bool> {
    transaction.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1)",
        [table],
        |row| row.get(0),
    )
}

fn column_exists(
    transaction: &rusqlite::Transaction<'_>,
    table: &str,
    column: &str,
) -> rusqlite::Result<bool> {
    let query =
        format!("SELECT EXISTS(SELECT 1 FROM pragma_table_info('{table}') WHERE name = ?1)");
    transaction.query_row(&query, [column], |row| row.get(0))
}

fn invitation_by_id(
    connection: &rusqlite::Connection,
    invitation_id: ApiaryInvitationId,
) -> rusqlite::Result<Option<ApiaryInvitation>> {
    connection
        .query_row(
            "SELECT id, apiary_id, invited_hive_id, invited_by_operator_id, state,
                    created_at, expires_at, resolved_at, required_policy_revision,
                    accepted_policy_revision, policy_accepted_at
             FROM apiary_invitations WHERE id = ?1",
            [invitation_id.to_string()],
            invitation_from_row,
        )
        .optional()
}

fn invitation_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ApiaryInvitation> {
    Ok(ApiaryInvitation {
        id: parse_domain_id(&row.get::<_, String>(0)?)?,
        apiary_id: parse_domain_id(&row.get::<_, String>(1)?)?,
        invited_hive_id: parse_domain_id(&row.get::<_, String>(2)?)?,
        invited_by_operator_id: parse_domain_id(&row.get::<_, String>(3)?)?,
        state: row
            .get::<_, String>(4)?
            .parse()
            .map_err(|_| rusqlite::Error::InvalidQuery)?,
        created_at: row.get(5)?,
        expires_at: row.get(6)?,
        resolved_at: row.get(7)?,
        required_policy_revision: row.get(8)?,
        accepted_policy_revision: row.get(9)?,
        policy_accepted_at: row.get(10)?,
    })
}

fn apiary_jira_project(
    connection: &rusqlite::Connection,
    apiary_id: ApiaryId,
    project_id: &str,
) -> rusqlite::Result<Option<ApiaryJiraProject>> {
    connection
        .query_row(
            "SELECT apiary_id, project_id, project_key, project_name,
                    promoted_by_operator_id, promoted_at
             FROM apiary_jira_projects WHERE apiary_id = ?1 AND project_id = ?2",
            params![apiary_id.to_string(), project_id],
            apiary_jira_project_from_row,
        )
        .optional()
}

fn apiary_jira_project_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ApiaryJiraProject> {
    Ok(ApiaryJiraProject {
        apiary_id: parse_domain_id(&row.get::<_, String>(0)?)?,
        project_id: row.get(1)?,
        project_key: row.get(2)?,
        project_name: row.get(3)?,
        promoted_by_operator_id: parse_domain_id(&row.get::<_, String>(4)?)?,
        promoted_at: row.get(5)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::JiraProjectBindingInput;
    use swarm_domain::{
        Apiary, ApiaryJoinCheckState, ApiaryJoinChecks, HiveIdentity, JiraProjectScope,
        JiraStatusMapping, SharedWorkBackend, TaskState,
    };

    fn add_apiary(store: &TaskStore, name: &str) -> (Apiary, OperatorId) {
        add_apiary_with_backend(store, name, SharedWorkBackend::Jira)
    }

    fn add_apiary_with_backend(
        store: &TaskStore,
        name: &str,
        backend: SharedWorkBackend,
    ) -> (Apiary, OperatorId) {
        let keeper_id = OperatorId::new();
        let apiary = Apiary::new(name, keeper_id, backend);
        let connection = store.connection().unwrap();
        connection
            .execute(
                "INSERT INTO operators (id, display_name) VALUES (?1, ?2)",
                params![keeper_id.to_string(), format!("{name} Keeper")],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO apiaries (id, name, keeper_operator_id, shared_work_backend)
                 VALUES (?1, ?2, ?3, ?4)",
                params![
                    apiary.id.to_string(),
                    name,
                    keeper_id.to_string(),
                    backend.to_string()
                ],
            )
            .unwrap();
        (apiary, keeper_id)
    }

    fn ready(
        store: &TaskStore,
        identity: &HiveIdentity,
        apiary: &Apiary,
        invitation: &ApiaryInvitation,
        now: i64,
    ) -> ApiaryJoinReadiness {
        let accepted = store
            .accept_apiary_policy(
                invitation.id,
                identity.operator.id,
                apiary.policy_revision(),
                now,
            )
            .unwrap();
        ApiaryJoinReadiness::evaluate(
            &identity.hive,
            apiary,
            Some(&accepted),
            ApiaryJoinChecks {
                identity: ApiaryJoinCheckState::Ready,
                integration: ApiaryJoinCheckState::Ready,
                project_access: ApiaryJoinCheckState::Ready,
                protocol: ApiaryJoinCheckState::Ready,
            },
            now,
        )
    }

    #[test]
    fn personal_hive_founds_one_immutable_backend_apiary_atomically() {
        let store = TaskStore::in_memory().unwrap();
        let identity = store.local_hive_identity().unwrap();

        let context = store
            .create_apiary_for_local_hive("  Wildflower Garden  ", SharedWorkBackend::Jira, 10)
            .unwrap();
        let swarm_domain::LocalApiaryContext::Federated { apiary, local_role } = context else {
            panic!("founding an Apiary must federate the local Hive");
        };
        assert_eq!(apiary.name, "Wildflower Garden");
        assert_eq!(apiary.keeper_operator_id, identity.operator.id);
        assert_eq!(apiary.shared_work_backend(), SharedWorkBackend::Jira);
        assert_eq!(apiary.policy_revision(), 1);
        assert_eq!(local_role, swarm_domain::LocalApiaryRole::Keeper);
        assert_eq!(
            store.local_hive_identity().unwrap().hive.apiary_id,
            Some(apiary.id)
        );
        assert!(matches!(
            store.create_apiary_for_local_hive("Another", SharedWorkBackend::Native, 20),
            Err(TaskStoreError::ApiaryMembershipConflict)
        ));
    }

    #[test]
    fn founding_an_apiary_rejects_invalid_operator_content_without_side_effects() {
        let store = TaskStore::in_memory().unwrap();
        for (name, now) in [("", 1), ("   ", 1), (&"a".repeat(121), 1), ("Garden", -1)] {
            assert!(matches!(
                store.create_apiary_for_local_hive(name, SharedWorkBackend::Jira, now),
                Err(TaskStoreError::InvalidApiary)
            ));
        }
        assert_eq!(
            store.local_apiary_context().unwrap(),
            swarm_domain::LocalApiaryContext::Personal
        );
    }

    #[test]
    fn sole_keeper_collapse_is_atomic_audited_and_converts_jira_scope() {
        let store = TaskStore::in_memory().unwrap();
        let identity = store.local_hive_identity().unwrap();
        let context = store
            .create_apiary_for_local_hive("Wildflower Garden", SharedWorkBackend::Jira, 10)
            .unwrap();
        let swarm_domain::LocalApiaryContext::Federated { apiary, .. } = context else {
            panic!("expected a federated Hive");
        };
        let binding = store
            .upsert_jira_project_binding(&JiraProjectBindingInput {
                project_id: "10001",
                project_key: "WEB",
                project_name: "Website Services",
                scope: JiraProjectScope::Apiary,
                apiary_id: Some(apiary.id),
            })
            .unwrap();

        assert_eq!(
            store.apiary_collapse_readiness(apiary.id).unwrap(),
            ApiaryCollapseReadiness {
                active_hive_count: 1,
                ..ApiaryCollapseReadiness::default()
            }
        );
        assert_eq!(
            store.collapse_local_apiary(20).unwrap(),
            swarm_domain::LocalApiaryContext::Personal
        );
        assert_eq!(store.local_hive_identity().unwrap().hive.apiary_id, None);
        let converted = store.get_jira_project_binding(binding.id).unwrap();
        assert_eq!(converted.scope, JiraProjectScope::Hive);
        assert_eq!(converted.apiary_id, None);

        let connection = store.connection().unwrap();
        assert_eq!(
            connection
                .query_row(
                    "SELECT collapsed_at FROM apiaries WHERE id = ?1",
                    [apiary.id.to_string()],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap(),
            20
        );
        assert_eq!(
            connection
                .query_row(
                    "SELECT group_concat(kind, ',') FROM apiary_lifecycle_events
                     WHERE apiary_id = ?1 ORDER BY sequence",
                    [apiary.id.to_string()],
                    |row| row.get::<_, String>(0),
                )
                .unwrap(),
            "founded,collapsed"
        );
        assert_eq!(
            connection
                .query_row(
                    "SELECT actor_operator_id FROM apiary_lifecycle_events
                     WHERE apiary_id = ?1 AND kind = 'collapsed'",
                    [apiary.id.to_string()],
                    |row| row.get::<_, String>(0),
                )
                .unwrap(),
            identity.operator.id.to_string()
        );
    }

    #[test]
    fn keeper_promotes_only_a_ready_local_jira_binding_atomically() {
        let store = TaskStore::in_memory().unwrap();
        let binding = store
            .upsert_jira_project_binding(&JiraProjectBindingInput {
                project_id: "10001",
                project_key: "WEB",
                project_name: "Website Services",
                scope: JiraProjectScope::Hive,
                apiary_id: None,
            })
            .unwrap();
        store
            .create_apiary_for_local_hive("Wildflower Garden", SharedWorkBackend::Jira, 10)
            .unwrap();

        assert!(matches!(
            store.promote_local_jira_binding_to_apiary(binding.id, 20),
            Err(TaskStoreError::ApiaryProjectPromotionNotReady)
        ));
        assert!(
            store
                .list_apiary_jira_projects(
                    store.local_hive_identity().unwrap().hive.apiary_id.unwrap()
                )
                .unwrap()
                .is_empty()
        );

        store
            .replace_jira_status_mappings(
                binding.id,
                &[JiraStatusMapping {
                    jira_status_id: "1".into(),
                    jira_status_name: "To Do".into(),
                    task_state: TaskState::Ready,
                }],
            )
            .unwrap();
        let promoted = store
            .promote_local_jira_binding_to_apiary(binding.id, 30)
            .unwrap();
        assert_eq!(promoted.project_id, "10001");
        assert_eq!(promoted.project_key, "WEB");
        let converted = store.get_jira_project_binding(binding.id).unwrap();
        assert_eq!(converted.scope, JiraProjectScope::Apiary);
        assert_eq!(converted.apiary_id, Some(promoted.apiary_id));
        assert!(converted.workflow_mapped);
        assert_eq!(
            store.list_apiary_jira_projects(promoted.apiary_id).unwrap(),
            vec![promoted]
        );
    }

    #[test]
    fn collapse_fails_closed_while_federation_state_exists() {
        let store = TaskStore::in_memory().unwrap();
        let identity = store.local_hive_identity().unwrap();
        let context = store
            .create_apiary_for_local_hive("Wildflower Garden", SharedWorkBackend::Jira, 10)
            .unwrap();
        let swarm_domain::LocalApiaryContext::Federated { apiary, .. } = context else {
            panic!("expected a federated Hive");
        };
        let invited_operator = OperatorId::new();
        let invited_hive = HiveId::new();
        {
            let connection = store.connection().unwrap();
            connection
                .execute(
                    "INSERT INTO operators (id, display_name) VALUES (?1, 'Guest')",
                    [invited_operator.to_string()],
                )
                .unwrap();
            connection
                .execute(
                    "INSERT INTO hives (id, name, operator_id) VALUES (?1, 'Guest Hive', ?2)",
                    params![invited_hive.to_string(), invited_operator.to_string()],
                )
                .unwrap();
        }
        store
            .create_apiary_invitation(apiary.id, invited_hive, identity.operator.id, 20, 100)
            .unwrap();
        assert_eq!(
            store.apiary_collapse_readiness(apiary.id).unwrap(),
            ApiaryCollapseReadiness {
                active_hive_count: 1,
                pending_invitation_count: 1,
                ..ApiaryCollapseReadiness::default()
            }
        );
        assert!(matches!(
            store.collapse_local_apiary(30),
            Err(TaskStoreError::ApiaryCollapseNotReady)
        ));
        assert_eq!(
            store.local_hive_identity().unwrap().hive.apiary_id,
            Some(apiary.id)
        );
    }

    #[test]
    fn only_keeper_can_create_one_bounded_pending_invitation() {
        let store = TaskStore::in_memory().unwrap();
        let identity = store.local_hive_identity().unwrap();
        let (apiary, keeper_id) = add_apiary(&store, "Garden");
        assert_eq!(store.get_apiary(apiary.id).unwrap(), apiary);

        assert_eq!(
            store
                .create_apiary_invitation(apiary.id, identity.hive.id, keeper_id, 10, 10)
                .unwrap_err()
                .to_string(),
            TaskStoreError::InvalidApiaryInvitation.to_string()
        );
        assert!(
            store
                .create_apiary_invitation(
                    apiary.id,
                    identity.hive.id,
                    identity.operator.id,
                    10,
                    100,
                )
                .is_err()
        );
        let invitation = store
            .create_apiary_invitation(apiary.id, identity.hive.id, keeper_id, 10, 100)
            .unwrap();
        assert_eq!(invitation.state, ApiaryInvitationState::Pending);
        assert!(
            store
                .create_apiary_invitation(apiary.id, identity.hive.id, keeper_id, 11, 101)
                .is_err()
        );
        assert_eq!(
            store
                .pending_apiary_invitations_for_hive(identity.hive.id, 50)
                .unwrap(),
            vec![invitation]
        );
        assert!(
            store
                .pending_apiary_invitations_for_hive(identity.hive.id, 100)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn accepting_ready_invitation_joins_once_and_revokes_competing_invites() {
        let store = TaskStore::in_memory().unwrap();
        let identity = store.local_hive_identity().unwrap();
        let (garden, garden_keeper) = add_apiary(&store, "Garden");
        let (orchard, orchard_keeper) = add_apiary(&store, "Orchard");
        let accepted_candidate = store
            .create_apiary_invitation(garden.id, identity.hive.id, garden_keeper, 10, 100)
            .unwrap();
        let competing = store
            .create_apiary_invitation(orchard.id, identity.hive.id, orchard_keeper, 10, 100)
            .unwrap();

        let readiness = ready(&store, &identity, &garden, &accepted_candidate, 50);
        let accepted = store
            .accept_apiary_invitation(accepted_candidate.id, &readiness, 50)
            .unwrap();
        assert_eq!(accepted.state, ApiaryInvitationState::Accepted);
        assert_eq!(accepted.resolved_at, Some(50));
        assert_eq!(
            store.local_hive_identity().unwrap().hive.apiary_id,
            Some(garden.id)
        );
        assert_eq!(
            store.get_apiary_invitation(competing.id).unwrap().state,
            ApiaryInvitationState::Revoked
        );
        assert!(
            store
                .accept_apiary_invitation(accepted_candidate.id, &readiness, 51)
                .is_err()
        );
    }

    #[test]
    fn policy_acceptance_is_operator_owned_and_revision_bound() {
        let store = TaskStore::in_memory().unwrap();
        let identity = store.local_hive_identity().unwrap();
        let (apiary, keeper_id) = add_apiary(&store, "Garden");
        let invitation = store
            .create_apiary_invitation(apiary.id, identity.hive.id, keeper_id, 10, 100)
            .unwrap();
        assert_eq!(invitation.required_policy_revision, 1);
        assert!(invitation.accepted_policy_revision.is_none());
        assert!(
            store
                .accept_apiary_policy(invitation.id, keeper_id, 1, 20)
                .is_err()
        );
        assert!(
            store
                .accept_apiary_policy(invitation.id, identity.operator.id, 2, 20)
                .is_err()
        );
        let accepted = store
            .accept_apiary_policy(invitation.id, identity.operator.id, 1, 20)
            .unwrap();
        assert_eq!(accepted.accepted_policy_revision, Some(1));
        assert_eq!(accepted.policy_accepted_at, Some(20));

        store
            .connection()
            .unwrap()
            .execute(
                "UPDATE apiaries SET policy_revision = 2 WHERE id = ?1",
                [apiary.id.to_string()],
            )
            .unwrap();
        assert!(
            store
                .accept_apiary_policy(invitation.id, identity.operator.id, 1, 30)
                .is_err()
        );
        let current = store.get_apiary(apiary.id).unwrap();
        let readiness = ApiaryJoinReadiness::evaluate(
            &identity.hive,
            &current,
            Some(&accepted),
            ApiaryJoinChecks {
                identity: ApiaryJoinCheckState::Ready,
                integration: ApiaryJoinCheckState::Ready,
                project_access: ApiaryJoinCheckState::Ready,
                protocol: ApiaryJoinCheckState::Ready,
            },
            30,
        );
        assert_eq!(
            readiness.blockers(),
            &[swarm_domain::ApiaryJoinBlocker::PolicyNotAccepted]
        );
    }

    #[test]
    fn acceptance_fails_closed_when_any_readiness_check_is_missing() {
        let store = TaskStore::in_memory().unwrap();
        let identity = store.local_hive_identity().unwrap();
        let (apiary, keeper_id) = add_apiary(&store, "Garden");
        let invitation = store
            .create_apiary_invitation(apiary.id, identity.hive.id, keeper_id, 10, 100)
            .unwrap();
        let accepted = store
            .accept_apiary_policy(
                invitation.id,
                identity.operator.id,
                apiary.policy_revision(),
                40,
            )
            .unwrap();
        let readiness = ApiaryJoinReadiness::evaluate(
            &identity.hive,
            &apiary,
            Some(&accepted),
            ApiaryJoinChecks {
                identity: ApiaryJoinCheckState::Ready,
                integration: ApiaryJoinCheckState::Ready,
                project_access: ApiaryJoinCheckState::Blocked,
                protocol: ApiaryJoinCheckState::Ready,
            },
            50,
        );

        assert!(matches!(
            store.accept_apiary_invitation(invitation.id, &readiness, 50),
            Err(TaskStoreError::ApiaryJoinNotReady)
        ));
        assert_eq!(store.local_hive_identity().unwrap().hive.apiary_id, None);
        assert_eq!(
            store.get_apiary_invitation(invitation.id).unwrap().state,
            ApiaryInvitationState::Pending
        );
    }

    #[test]
    fn promoted_jira_catalog_requires_keeper_and_complete_local_readiness() {
        let store = TaskStore::in_memory().unwrap();
        let identity = store.local_hive_identity().unwrap();
        let (apiary, keeper_id) = add_apiary(&store, "Garden");
        assert!(store.apiary_jira_project_access_ready(apiary.id).unwrap());
        assert!(
            store
                .promote_apiary_jira_project(
                    apiary.id,
                    "10001",
                    "WEB",
                    "Website Services",
                    identity.operator.id,
                    10,
                )
                .is_err()
        );
        let promoted = store
            .promote_apiary_jira_project(
                apiary.id,
                "10001",
                "WEB",
                "Website Services",
                keeper_id,
                10,
            )
            .unwrap();
        assert_eq!(
            store.list_apiary_jira_projects(apiary.id).unwrap(),
            vec![promoted]
        );
        assert!(!store.apiary_jira_project_access_ready(apiary.id).unwrap());

        let binding = store
            .upsert_jira_project_binding(&JiraProjectBindingInput {
                project_id: "10001",
                project_key: "WEB",
                project_name: "Website Services",
                scope: JiraProjectScope::Apiary,
                apiary_id: Some(apiary.id),
            })
            .unwrap();
        assert!(!store.apiary_jira_project_access_ready(apiary.id).unwrap());
        store
            .replace_jira_status_mappings(
                binding.id,
                &[JiraStatusMapping {
                    jira_status_id: "3".into(),
                    jira_status_name: "In Progress".into(),
                    task_state: TaskState::Active,
                }],
            )
            .unwrap();
        assert!(store.apiary_jira_project_access_ready(apiary.id).unwrap());

        let (native, native_keeper) =
            add_apiary_with_backend(&store, "Orchard", SharedWorkBackend::Native);
        assert!(
            store
                .promote_apiary_jira_project(
                    native.id,
                    "10002",
                    "OPS",
                    "Operations",
                    native_keeper,
                    10,
                )
                .is_err()
        );
    }

    #[test]
    fn migrates_schema_v25_to_durable_apiary_invitations() {
        let mut connection = rusqlite::Connection::open_in_memory().unwrap();
        connection
            .execute_batch(
                "CREATE TABLE operators (id TEXT PRIMARY KEY);
                 CREATE TABLE apiaries (
                     id TEXT PRIMARY KEY,
                     keeper_operator_id TEXT NOT NULL REFERENCES operators(id)
                 );
                 CREATE TABLE hives (
                     id TEXT PRIMARY KEY,
                     apiary_id TEXT REFERENCES apiaries(id)
                 );",
            )
            .unwrap();
        let transaction = connection.transaction().unwrap();
        migrate_apiary_invitations(&transaction).unwrap();
        transaction.commit().unwrap();
        assert_eq!(
            connection
                .query_row(
                    "SELECT COUNT(*) FROM sqlite_master
                     WHERE type = 'table' AND name = 'apiary_invitations'",
                    [],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap(),
            1
        );
        assert_eq!(
            connection
                .pragma_query_value::<i64, _>(None, "user_version", |row| row.get(0))
                .unwrap(),
            26
        );
    }

    #[test]
    fn migrates_schema_v26_to_promoted_apiary_jira_catalog() {
        let mut connection = rusqlite::Connection::open_in_memory().unwrap();
        connection
            .execute_batch(
                "CREATE TABLE operators (id TEXT PRIMARY KEY);
                 CREATE TABLE apiaries (
                     id TEXT PRIMARY KEY,
                     keeper_operator_id TEXT NOT NULL REFERENCES operators(id),
                     shared_work_backend TEXT NOT NULL
                 );",
            )
            .unwrap();
        let transaction = connection.transaction().unwrap();
        migrate_apiary_jira_projects(&transaction).unwrap();
        transaction.commit().unwrap();
        assert_eq!(
            connection
                .query_row(
                    "SELECT COUNT(*) FROM sqlite_master
                     WHERE type = 'table' AND name = 'apiary_jira_projects'",
                    [],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap(),
            1
        );
        assert_eq!(
            connection
                .pragma_query_value::<i64, _>(None, "user_version", |row| row.get(0))
                .unwrap(),
            27
        );
    }

    #[test]
    fn migrates_schema_v27_to_revision_bound_policy_acceptance() {
        let mut connection = rusqlite::Connection::open_in_memory().unwrap();
        connection
            .execute_batch(
                "CREATE TABLE apiaries (
                     id TEXT PRIMARY KEY,
                     name TEXT NOT NULL,
                     keeper_operator_id TEXT NOT NULL,
                     shared_work_backend TEXT NOT NULL
                 );
                 CREATE TABLE apiary_invitations (
                     id TEXT PRIMARY KEY,
                     apiary_id TEXT NOT NULL,
                     invited_hive_id TEXT NOT NULL,
                     invited_by_operator_id TEXT NOT NULL,
                     state TEXT NOT NULL,
                     created_at INTEGER NOT NULL,
                     expires_at INTEGER NOT NULL,
                     resolved_at INTEGER
                 );
                 CREATE TRIGGER immutable_apiary_invitation_identity
                     BEFORE UPDATE OF id ON apiary_invitations
                     BEGIN SELECT RAISE(ABORT, 'immutable'); END;",
            )
            .unwrap();
        let transaction = connection.transaction().unwrap();
        migrate_apiary_policy_acceptance(&transaction).unwrap();
        transaction.commit().unwrap();
        assert!(
            connection
                .query_row(
                    "SELECT EXISTS(
                         SELECT 1 FROM pragma_table_info('apiaries')
                         WHERE name = 'policy_revision'
                     )",
                    [],
                    |row| row.get::<_, bool>(0),
                )
                .unwrap()
        );
        assert_eq!(
            connection
                .pragma_query_value::<i64, _>(None, "user_version", |row| row.get(0))
                .unwrap(),
            28
        );
    }

    #[test]
    fn migrates_schema_v28_to_audited_apiary_lifecycle() {
        let mut connection = rusqlite::Connection::open_in_memory().unwrap();
        connection
            .execute_batch(
                "CREATE TABLE operators (id TEXT PRIMARY KEY);
                 CREATE TABLE apiaries (id TEXT PRIMARY KEY);
                 CREATE TABLE hives (
                     id TEXT PRIMARY KEY,
                     operator_id TEXT NOT NULL,
                     apiary_id TEXT REFERENCES apiaries(id)
                 );
                 CREATE TABLE apiary_invitations (
                     id TEXT PRIMARY KEY,
                     apiary_id TEXT NOT NULL REFERENCES apiaries(id)
                 );
                 CREATE TABLE apiary_jira_projects (
                     apiary_id TEXT NOT NULL REFERENCES apiaries(id),
                     project_id TEXT NOT NULL
                 );",
            )
            .unwrap();
        let transaction = connection.transaction().unwrap();
        migrate_apiary_lifecycle(&transaction).unwrap();
        transaction.commit().unwrap();
        assert!(
            connection
                .query_row(
                    "SELECT EXISTS(
                         SELECT 1 FROM pragma_table_info('apiaries')
                         WHERE name = 'collapsed_at'
                     )",
                    [],
                    |row| row.get::<_, bool>(0),
                )
                .unwrap()
        );
        assert_eq!(
            connection
                .query_row(
                    "SELECT COUNT(*) FROM sqlite_master
                     WHERE type = 'table' AND name = 'apiary_lifecycle_events'",
                    [],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap(),
            1
        );
        assert_eq!(
            connection
                .pragma_query_value::<i64, _>(None, "user_version", |row| row.get(0))
                .unwrap(),
            29
        );
    }
}
