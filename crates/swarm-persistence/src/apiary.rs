use rusqlite::{OptionalExtension, params};
use swarm_domain::{
    Apiary, ApiaryId, ApiaryInvitation, ApiaryInvitationId, ApiaryInvitationState,
    ApiaryJiraProject, ApiaryJoinReadiness, HiveId, OperatorId,
};

use crate::{TaskStore, TaskStoreError, parse_domain_id};

const MAX_INVITATION_LIFETIME_SECONDS: i64 = 30 * 24 * 60 * 60;
const MAX_PROJECT_ID_BYTES: usize = 128;
const MAX_PROJECT_KEY_BYTES: usize = 64;
const MAX_PROJECT_NAME_BYTES: usize = 240;

impl TaskStore {
    /// Returns one durable Apiary by identity without exposing membership or credentials.
    ///
    /// # Errors
    /// Returns an error when the Apiary does not exist or persisted data is invalid.
    pub fn get_apiary(&self, apiary_id: ApiaryId) -> Result<Apiary, TaskStoreError> {
        let connection = self.connection()?;
        connection
            .query_row(
                "SELECT id, name, keeper_operator_id, shared_work_backend
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
                    ))
                },
            )
            .optional()?
            .ok_or(TaskStoreError::ApiaryNotFound)
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
                (id, apiary_id, invited_hive_id, invited_by_operator_id, created_at, expires_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
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
                    created_at, expires_at, resolved_at
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

fn invitation_by_id(
    connection: &rusqlite::Connection,
    invitation_id: ApiaryInvitationId,
) -> rusqlite::Result<Option<ApiaryInvitation>> {
    connection
        .query_row(
            "SELECT id, apiary_id, invited_hive_id, invited_by_operator_id, state,
                    created_at, expires_at, resolved_at
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
        identity: &HiveIdentity,
        apiary: &Apiary,
        invitation: &ApiaryInvitation,
        now: i64,
    ) -> ApiaryJoinReadiness {
        ApiaryJoinReadiness::evaluate(
            &identity.hive,
            apiary,
            Some(invitation),
            ApiaryJoinChecks {
                identity: ApiaryJoinCheckState::Ready,
                integration: ApiaryJoinCheckState::Ready,
                project_access: ApiaryJoinCheckState::Ready,
                policy: ApiaryJoinCheckState::Ready,
                protocol: ApiaryJoinCheckState::Ready,
            },
            now,
        )
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

        let readiness = ready(&identity, &garden, &accepted_candidate, 50);
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
    fn acceptance_fails_closed_when_any_readiness_check_is_missing() {
        let store = TaskStore::in_memory().unwrap();
        let identity = store.local_hive_identity().unwrap();
        let (apiary, keeper_id) = add_apiary(&store, "Garden");
        let invitation = store
            .create_apiary_invitation(apiary.id, identity.hive.id, keeper_id, 10, 100)
            .unwrap();
        let readiness = ApiaryJoinReadiness::evaluate(
            &identity.hive,
            &apiary,
            Some(&invitation),
            ApiaryJoinChecks {
                identity: ApiaryJoinCheckState::Ready,
                integration: ApiaryJoinCheckState::Ready,
                project_access: ApiaryJoinCheckState::Ready,
                policy: ApiaryJoinCheckState::Blocked,
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
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("swarm-next.sqlite3");
        {
            let store = TaskStore::open(&path).unwrap();
            store
                .connection()
                .unwrap()
                .execute_batch(
                    "DROP TRIGGER apiary_invitation_terminal_state;
                     DROP TRIGGER immutable_apiary_invitation_identity;
                     DROP TRIGGER apiary_invitation_personal_hive_insert;
                     DROP TRIGGER apiary_invitation_keeper_insert;
                     DROP TABLE apiary_invitations;
                     PRAGMA user_version = 25;",
                )
                .unwrap();
        }

        let migrated = TaskStore::open(path).unwrap();
        let connection = migrated.connection().unwrap();
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
            crate::CURRENT_SCHEMA_VERSION
        );
    }

    #[test]
    fn migrates_schema_v26_to_promoted_apiary_jira_catalog() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("swarm-next.sqlite3");
        {
            let store = TaskStore::open(&path).unwrap();
            store
                .connection()
                .unwrap()
                .execute_batch(
                    "DROP TRIGGER apiary_jira_project_keeper_insert;
                     DROP TRIGGER immutable_apiary_jira_project_identity;
                     DROP TABLE apiary_jira_projects;
                     PRAGMA user_version = 26;",
                )
                .unwrap();
        }

        let migrated = TaskStore::open(path).unwrap();
        let connection = migrated.connection().unwrap();
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
            crate::CURRENT_SCHEMA_VERSION
        );
    }
}
