//! Storing what each Hive can do.
//!
//! Two sides of one fact. A member derives its own capability and publishes it
//! upward; a Keeper accepts other members' reports and holds the fleet picture.
//! Both live here so the shapes cannot drift apart.

use base64ct::{Base64UrlUnpadded, Encoding};
use ed25519_dalek::{Signature, Signer, Verifier, VerifyingKey};
use rusqlite::{OptionalExtension, params};
use swarm_domain::{
    HIVE_CAPABILITY_SCHEMA_VERSION, HiveCapabilityPayload, HiveCapabilityUpdate,
    HiveCapabilityWorker, PublicHiveIdentity,
};

use crate::{TaskStore, TaskStoreError};

/// How many member reports a Keeper will hold. Matches the directory bound: a
/// fleet larger than this has outgrown more than this table.
const MAX_CAPABILITY_MEMBERS: usize = swarm_domain::MAX_APIARY_DIRECTORY_ENTRIES;

pub(super) fn migrate(transaction: &rusqlite::Transaction<'_>) -> rusqlite::Result<()> {
    transaction.execute_batch(
        "CREATE TABLE IF NOT EXISTS local_hive_capability (
            singleton INTEGER PRIMARY KEY CHECK(singleton = 1),
            revision INTEGER NOT NULL CHECK(revision > 0),
            observed_at INTEGER NOT NULL CHECK(observed_at >= 0),
            payload_json TEXT NOT NULL
         );
         CREATE TABLE IF NOT EXISTS apiary_hive_capabilities (
            apiary_id TEXT NOT NULL,
            node_id TEXT NOT NULL,
            hive_id TEXT NOT NULL,
            revision INTEGER NOT NULL CHECK(revision > 0),
            observed_at INTEGER NOT NULL CHECK(observed_at >= 0),
            received_at INTEGER NOT NULL CHECK(received_at >= 0),
            payload_json TEXT NOT NULL,
            PRIMARY KEY (apiary_id, node_id)
         );",
    )?;
    transaction.pragma_update(
        None,
        "user_version",
        crate::HIVE_CAPABILITY_SCHEMA_VERSION_MARKER,
    )
}

/// A member report as the Keeper holds it.
#[derive(Debug, Clone, serde::Serialize)]
pub struct StoredHiveCapability {
    pub payload: HiveCapabilityPayload,
    /// When this Hive derived the report, by its own clock.
    pub observed_at: i64,
    /// When Keeper received it. Kept separately from `observed_at` so a report
    /// that arrived late is distinguishable from one derived late.
    pub received_at: i64,
}

fn canonical_capability_payload(
    payload: &HiveCapabilityPayload,
) -> Result<Vec<u8>, TaskStoreError> {
    serde_json::to_vec(payload).map_err(|error| TaskStoreError::IntegrityFailure(error.to_string()))
}

/// Checks a member's signature over its own capability report.
///
/// # Errors
/// Rejects a payload outside the member's scope, a malformed key or signature,
/// and any signature that does not verify.
pub fn verify_hive_capability_update(
    update: &HiveCapabilityUpdate,
    apiary_id: swarm_domain::ApiaryId,
    member: PublicHiveIdentity,
    pinned_public_key: &str,
) -> Result<(), TaskStoreError> {
    if !update.payload.matches_member(apiary_id, member) {
        return Err(TaskStoreError::InvalidFederationCredential);
    }
    let key: [u8; 32] = Base64UrlUnpadded::decode_vec(pinned_public_key)
        .map_err(|_| TaskStoreError::InvalidFederationCredential)?
        .try_into()
        .map_err(|_| TaskStoreError::InvalidFederationCredential)?;
    let signature: [u8; 64] = Base64UrlUnpadded::decode_vec(&update.signature)
        .map_err(|_| TaskStoreError::InvalidFederationCredential)?
        .try_into()
        .map_err(|_| TaskStoreError::InvalidFederationCredential)?;
    let canonical = canonical_capability_payload(&update.payload)?;
    VerifyingKey::from_bytes(&key)
        .and_then(|key| key.verify(&canonical, &Signature::from_bytes(&signature)))
        .map_err(|_| TaskStoreError::InvalidFederationCredential)
}

impl TaskStore {
    /// Seals this Hive's own capability into one signed, retry-stable report.
    ///
    /// The caller supplies the derived workers and versions, because deriving
    /// them means reading git and the process build — neither of which belongs
    /// behind the persistence boundary. This layer owns identity, the monotonic
    /// revision, and the signature.
    ///
    /// ⚠️ THE REVISION ONLY MOVES WHEN THE CONTENT DOES. Re-deriving an
    /// unchanged fleet must not produce a new revision, or every poll would look
    /// like a change to Keeper and the ordering guarantee would carry no
    /// information.
    ///
    /// # Errors
    /// Rejects non-members, invalid input, and unavailable persistence.
    pub fn seal_local_hive_capability(
        &self,
        workers: &[HiveCapabilityWorker],
        workers_truncated: bool,
        swarm_version: &str,
        database_schema_version: i64,
        now: i64,
    ) -> Result<HiveCapabilityUpdate, TaskStoreError> {
        // Read from the same place the member's own convergence view reads, so
        // the two can never disagree about what this Hive is running.
        let convergence = self.local_policy_convergence()?;
        let applied_policy = self.local_policy_settings()?;
        if now < 0 {
            return Err(TaskStoreError::InvalidHiveIdentity);
        }
        self.require_local_federation_member()?;
        let identity = self.local_hive_identity()?;
        let local_node = self.local_federation_identity(now)?;
        let apiary_id = identity
            .hive
            .apiary_id
            .ok_or(TaskStoreError::InvalidApiary)?;
        let member = PublicHiveIdentity {
            node_id: local_node.node_id,
            hive_id: identity.hive.id,
            operator_id: identity.operator.id,
        };
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let prior: Option<(u64, String)> = transaction
            .query_row(
                "SELECT revision, payload_json FROM local_hive_capability WHERE singleton = 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;

        let mut payload = HiveCapabilityPayload {
            schema_version: HIVE_CAPABILITY_SCHEMA_VERSION,
            apiary_id,
            identity: member,
            revision: 1,
            observed_at: now,
            swarm_version: swarm_version.to_owned(),
            database_schema_version,
            workers: workers.to_vec(),
            workers_truncated,
            policy_revision: convergence.policy_revision,
            applied_policy,
        };

        if let Some((revision, saved)) = &prior {
            let saved: HiveCapabilityPayload = serde_json::from_str(saved)
                .map_err(|error| TaskStoreError::IntegrityFailure(error.to_string()))?;
            // Everything except when we looked. If only `observed_at` differs,
            // nothing about this Hive changed and the revision must hold.
            let unchanged = saved.workers == payload.workers
                && saved.workers_truncated == payload.workers_truncated
                && saved.swarm_version == payload.swarm_version
                && saved.database_schema_version == payload.database_schema_version
                && saved.identity == payload.identity
                && saved.apiary_id == payload.apiary_id
                && saved.policy_revision == payload.policy_revision
                && saved.applied_policy == payload.applied_policy;
            payload.revision = if unchanged {
                *revision
            } else {
                revision
                    .checked_add(1)
                    .ok_or(TaskStoreError::InvalidHiveIdentity)?
            };
            if unchanged {
                payload.observed_at = saved.observed_at;
            }
        }

        if !payload.matches_member(apiary_id, member) {
            return Err(TaskStoreError::InvalidHiveIdentity);
        }
        let serialized = serde_json::to_string(&payload)
            .map_err(|error| TaskStoreError::IntegrityFailure(error.to_string()))?;
        transaction.execute(
            "INSERT INTO local_hive_capability (singleton, revision, observed_at, payload_json)
             VALUES (1, ?1, ?2, ?3)
             ON CONFLICT(singleton) DO UPDATE SET
                revision = excluded.revision,
                observed_at = excluded.observed_at,
                payload_json = excluded.payload_json",
            params![payload.revision, payload.observed_at, serialized],
        )?;
        let signature = local_node
            .signing_key
            .sign(&canonical_capability_payload(&payload)?);
        transaction.commit()?;
        Ok(HiveCapabilityUpdate {
            payload,
            signature: Base64UrlUnpadded::encode_string(&signature.to_bytes()),
        })
    }

    /// The report this Hive last sealed, so a restart resumes rather than
    /// restarting the revision sequence.
    ///
    /// # Errors
    /// Returns corrupt stored evidence rather than an empty report.
    pub fn local_hive_capability(&self) -> Result<Option<StoredHiveCapability>, TaskStoreError> {
        let connection = self.connection()?;
        let row: Option<(i64, String)> = connection
            .query_row(
                "SELECT observed_at, payload_json FROM local_hive_capability WHERE singleton = 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        let Some((observed_at, payload)) = row else {
            return Ok(None);
        };
        let payload: HiveCapabilityPayload = serde_json::from_str(&payload)
            .map_err(|error| TaskStoreError::IntegrityFailure(error.to_string()))?;
        Ok(Some(StoredHiveCapability {
            payload,
            observed_at,
            received_at: observed_at,
        }))
    }

    /// Keeper accepting one authenticated member's capability report.
    ///
    /// Returns whether anything changed, so an identical retry is free and
    /// emits no event. Mirrors `accept_federation_public_profile` deliberately:
    /// one authentication path, one revision rule, one shape to reason about.
    ///
    /// # Errors
    /// Rejects invalid credentials and signatures, wrong scope, stale or
    /// conflicting revisions, capacity overflow, and unavailable persistence.
    pub fn accept_hive_capability(
        &self,
        credential: &str,
        update: &HiveCapabilityUpdate,
        now: i64,
    ) -> Result<bool, TaskStoreError> {
        if now < 0 {
            return Err(TaskStoreError::InvalidFederationCredential);
        }
        let credential: [u8; 32] = Base64UrlUnpadded::decode_vec(credential)
            .map_err(|_| TaskStoreError::InvalidFederationCredential)?
            .try_into()
            .map_err(|_| TaskStoreError::InvalidFederationCredential)?;
        let identity = self.local_hive_identity()?;
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let member = crate::federation::authenticate_member_credential(
            &transaction,
            &identity,
            &credential,
            now,
        )?;
        let key: String = transaction.query_row(
            "SELECT public_key FROM apiary_hive_candidates
             WHERE apiary_id = ?1 AND node_id = ?2 AND hive_id = ?3 AND operator_id = ?4",
            params![
                member.apiary.to_string(),
                member.node.to_string(),
                member.hive.to_string(),
                member.operator.to_string()
            ],
            |row| row.get(0),
        )?;
        verify_hive_capability_update(
            update,
            member.apiary,
            PublicHiveIdentity {
                node_id: member.node,
                hive_id: member.hive,
                operator_id: member.operator,
            },
            &key,
        )?;
        let payload = serde_json::to_string(&update.payload)
            .map_err(|error| TaskStoreError::IntegrityFailure(error.to_string()))?;
        let prior: Option<(u64, String)> = transaction
            .query_row(
                "SELECT revision, payload_json FROM apiary_hive_capabilities
                 WHERE apiary_id = ?1 AND node_id = ?2",
                params![member.apiary.to_string(), member.node.to_string()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        if let Some((revision, saved)) = prior {
            if revision == update.payload.revision && saved == payload {
                return Ok(false);
            }
            if revision >= update.payload.revision {
                return Err(TaskStoreError::InvalidHiveIdentity);
            }
        } else {
            let count: usize = transaction.query_row(
                "SELECT count(*) FROM apiary_hive_capabilities WHERE apiary_id = ?1",
                [member.apiary.to_string()],
                |row| row.get(0),
            )?;
            if count >= MAX_CAPABILITY_MEMBERS {
                return Err(TaskStoreError::InvalidApiary);
            }
        }
        transaction.execute(
            "INSERT INTO apiary_hive_capabilities
                (apiary_id, node_id, hive_id, revision, observed_at, received_at, payload_json)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(apiary_id, node_id) DO UPDATE SET
                hive_id = excluded.hive_id,
                revision = excluded.revision,
                observed_at = excluded.observed_at,
                received_at = excluded.received_at,
                payload_json = excluded.payload_json",
            params![
                member.apiary.to_string(),
                member.node.to_string(),
                member.hive.to_string(),
                update.payload.revision,
                update.payload.observed_at,
                now,
                payload
            ],
        )?;
        crate::insert_control_room_event(
            &transaction,
            swarm_domain::ControlRoomEventKind::RuntimeChanged,
        )?;
        transaction.commit()?;
        Ok(true)
    }

    /// Each member's drift from the Apiary's shared defaults, as Keeper sees it.
    ///
    /// ⚠️ COMPUTED AT KEEPER, NOT REPORTED BY THE MEMBER. A member sends what it
    /// RUNS; Keeper holds what the Apiary EXPECTS; the difference is derived
    /// here. A member cannot declare itself compliant, which is the property
    /// that makes this worth having.
    ///
    /// # Errors
    /// Returns corrupt stored evidence rather than a false clean bill.
    pub fn member_policy_drift(
        &self,
    ) -> Result<Vec<(StoredHiveCapability, Vec<swarm_domain::PolicyDrift>)>, TaskStoreError> {
        let expected = self.apiary_policy_settings()?;
        let reports = self.federation_hive_capabilities()?;
        Ok(reports
            .into_iter()
            .map(|report| {
                let drift = report.payload.policy_drift_against(&expected);
                (report, drift)
            })
            .collect())
    }

    /// Every member capability report this Hive holds, newest observation first.
    ///
    /// ⚠️ `observed_at` AND `received_at` ARE BOTH RETURNED ON PURPOSE. A caller
    /// that knows only "there is a report" cannot tell a Hive that went quiet a
    /// week ago from one that answered a minute ago, and those demand opposite
    /// reactions. The acceptance for this work says a stale report must be
    /// distinguishable from a missing one; this is where that is possible.
    ///
    /// # Errors
    /// Returns corrupt stored evidence rather than a partial fleet.
    pub fn federation_hive_capabilities(
        &self,
    ) -> Result<Vec<StoredHiveCapability>, TaskStoreError> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT observed_at, received_at, payload_json FROM apiary_hive_capabilities
             ORDER BY observed_at DESC, node_id",
        )?;
        let rows = statement.query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, String>(2)?,
            ))
        })?;
        let mut reports = Vec::new();
        for row in rows {
            let (observed_at, received_at, payload) = row?;
            let payload: HiveCapabilityPayload = serde_json::from_str(&payload)
                .map_err(|error| TaskStoreError::IntegrityFailure(error.to_string()))?;
            reports.push(StoredHiveCapability {
                payload,
                observed_at,
                received_at,
            });
        }
        Ok(reports)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use swarm_domain::ProviderKind;

    fn worker(name: &str, repository: Option<&str>, awake: bool) -> HiveCapabilityWorker {
        HiveCapabilityWorker {
            name: name.to_owned(),
            provider: ProviderKind::ClaudeCode,
            repository: repository.map(str::to_owned),
            awake,
        }
    }

    fn fleet() -> Vec<HiveCapabilityWorker> {
        vec![
            worker("Platform", Some("git@github.com:rcg/platform.git"), true),
            // Sleeping, and reported anyway. The operator was explicit: "this
            // includes sleeping workers", because the report is of what a Hive
            // COULD do, not what it is doing.
            worker("Admin", Some("https://github.com/rcg/admin.git"), false),
            worker("Scout", None, false),
        ]
    }

    /// The whole loop: a member seals its capability, Keeper authenticates and
    /// stores it, and the fleet picture comes back readable.
    #[test]
    fn a_member_publishes_what_it_can_do_and_keeper_holds_it() {
        let now = 120_000;
        let (keeper, member) = crate::federation::tests::joined_member(now);
        let credential = member
            .federation_member_connection()
            .unwrap()
            .node_credential;

        let update = member
            .seal_local_hive_capability(&fleet(), false, "1.12.0-dev-f8c39fc2ed12", 185, now + 20)
            .unwrap();
        assert_eq!(update.payload.revision, 1);
        assert!(
            keeper
                .accept_hive_capability(&credential, &update, now + 21)
                .unwrap()
        );

        let held = keeper.federation_hive_capabilities().unwrap();
        assert_eq!(held.len(), 1);
        assert_eq!(held[0].payload.swarm_version, "1.12.0-dev-f8c39fc2ed12");
        assert_eq!(held[0].payload.database_schema_version, 185);
        assert_eq!(
            held[0].payload.repositories(),
            vec![
                "git@github.com:rcg/platform.git",
                "https://github.com/rcg/admin.git"
            ]
        );
        assert!(
            held[0]
                .payload
                .workers
                .iter()
                .any(|w| w.name == "Admin" && !w.awake),
            "a sleeping worker must still be reported"
        );

        // ⚠️ THE ACCEPTANCE, ASSERTED ON THE STORED BYTES rather than the type.
        // The type has no path field, but a reader deserves proof that nothing
        // reconstructed one on the way through.
        let stored = serde_json::to_string(&held[0].payload).unwrap();
        assert!(
            !stored.contains("/home/") && !stored.contains("\\Users\\"),
            "no absolute filesystem path may appear in a published report: {stored}"
        );
    }

    /// ⚠️ A STALE REPORT MUST BE DISTINGUISHABLE FROM A MISSING ONE. A Keeper
    /// that knows only "there is a report" cannot tell a Hive that went quiet
    /// last week from one that answered a minute ago, and those demand opposite
    /// reactions.
    #[test]
    fn a_stale_report_is_distinguishable_from_a_missing_one() {
        let now = 120_000;
        let (keeper, member) = crate::federation::tests::joined_member(now);
        let credential = member
            .federation_member_connection()
            .unwrap()
            .node_credential;

        assert!(
            keeper.federation_hive_capabilities().unwrap().is_empty(),
            "missing is an empty fleet, not a stale entry"
        );

        let update = member
            .seal_local_hive_capability(&fleet(), false, "1.12.0", 185, now + 20)
            .unwrap();
        // Received long after it was observed, which is exactly the shape of a
        // Hive that went quiet and then reconnected.
        keeper
            .accept_hive_capability(&credential, &update, now + 900_000)
            .unwrap();

        let held = keeper.federation_hive_capabilities().unwrap();
        assert_eq!(held[0].observed_at, now + 20);
        assert_eq!(held[0].received_at, now + 900_000);
        assert!(
            held[0].received_at - held[0].observed_at > 800_000,
            "both timestamps are kept so staleness is computable"
        );
    }

    /// Re-deriving an unchanged fleet must not look like a change, or every poll
    /// would burn a revision and the ordering would carry no information.
    #[test]
    fn an_unchanged_fleet_holds_its_revision_and_retries_are_free() {
        let now = 120_000;
        let (keeper, member) = crate::federation::tests::joined_member(now);
        let credential = member
            .federation_member_connection()
            .unwrap()
            .node_credential;

        let first = member
            .seal_local_hive_capability(&fleet(), false, "1.12.0", 185, now + 20)
            .unwrap();
        // Later clock, identical fleet.
        let second = member
            .seal_local_hive_capability(&fleet(), false, "1.12.0", 185, now + 5_000)
            .unwrap();
        assert_eq!(second.payload.revision, first.payload.revision);
        assert_eq!(
            second.payload.observed_at, first.payload.observed_at,
            "an unchanged report keeps its original observation, so it is byte-identical"
        );

        assert!(
            keeper
                .accept_hive_capability(&credential, &first, now + 21)
                .unwrap()
        );
        assert!(
            !keeper
                .accept_hive_capability(&credential, &second, now + 22)
                .unwrap(),
            "an identical retry changes nothing and must say so"
        );

        // A real change moves the revision.
        let mut grown = fleet();
        grown.push(worker("Nexus", Some("git@github.com:rcg/nexus.git"), false));
        let third = member
            .seal_local_hive_capability(&grown, false, "1.12.0", 185, now + 6_000)
            .unwrap();
        assert_eq!(third.payload.revision, first.payload.revision + 1);
        assert!(
            keeper
                .accept_hive_capability(&credential, &third, now + 23)
                .unwrap()
        );
    }

    /// ⚠️ SURVIVES A MEMBER RESTART, which the acceptance asks for directly. The
    /// revision sequence lives in the database rather than in memory, so a
    /// restarted Hive resumes rather than restarting at 1 — which Keeper would
    /// reject as stale and the member could never recover from.
    #[test]
    fn the_revision_sequence_survives_a_member_restart() {
        let now = 120_000;
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("member.sqlite3");
        let keeper = TaskStore::in_memory().unwrap();
        keeper
            .create_apiary_for_local_hive("Garden", swarm_domain::SharedWorkBackend::Jira, now)
            .unwrap();

        let revision = {
            let member = TaskStore::open(&path).unwrap();
            let acceptance =
                crate::federation::tests::register_remote_member(&keeper, &member, now + 1);
            member
                .apply_federation_join_acceptance(
                    acceptance.receipt.payload.invitation_id,
                    &acceptance,
                    now + 6,
                )
                .unwrap();
            let mut grown = fleet();
            member
                .seal_local_hive_capability(&fleet(), false, "1.12.0", 185, now + 20)
                .unwrap();
            grown.push(worker("Nexus", Some("git@github.com:rcg/nexus.git"), false));
            let second = member
                .seal_local_hive_capability(&grown, false, "1.12.0", 185, now + 30)
                .unwrap();
            assert_eq!(second.payload.revision, 2);
            second.payload.revision
        };

        // Reopened, as after a restart.
        let member = TaskStore::open(&path).unwrap();
        assert_eq!(
            member
                .local_hive_capability()
                .unwrap()
                .unwrap()
                .payload
                .revision,
            revision,
            "the last sealed report must still be there"
        );
        let mut grown = fleet();
        grown.push(worker("Nexus", Some("git@github.com:rcg/nexus.git"), false));
        grown.push(worker("Hub", Some("git@github.com:rcg/hub.git"), false));
        let next = member
            .seal_local_hive_capability(&grown, false, "1.12.0", 185, now + 40)
            .unwrap();
        assert_eq!(
            next.payload.revision,
            revision + 1,
            "it resumes, it does not restart"
        );
    }

    /// A report signed by one Hive cannot be published on behalf of another, and
    /// a tampered payload fails closed.
    #[test]
    fn another_hives_credential_cannot_publish_this_hives_capability() {
        let now = 120_000;
        let (keeper, member) = crate::federation::tests::joined_member(now);
        let stranger = TaskStore::in_memory().unwrap();
        let acceptance =
            crate::federation::tests::register_remote_member(&keeper, &stranger, now + 7);
        stranger
            .apply_federation_join_acceptance(
                acceptance.receipt.payload.invitation_id,
                &acceptance,
                now + 12,
            )
            .unwrap();

        let update = member
            .seal_local_hive_capability(&fleet(), false, "1.12.0", 185, now + 20)
            .unwrap();
        let stranger_credential = stranger
            .federation_member_connection()
            .unwrap()
            .node_credential;
        assert!(
            keeper
                .accept_hive_capability(&stranger_credential, &update, now + 21)
                .is_err(),
            "a credential authenticates the sender, and the signature must match it"
        );

        let mut tampered = update.clone();
        tampered
            .payload
            .workers
            .push(worker("Injected", Some("git@github.com:rcg/x.git"), false));
        let credential = member
            .federation_member_connection()
            .unwrap()
            .node_credential;
        assert!(
            keeper
                .accept_hive_capability(&credential, &tampered, now + 22)
                .is_err(),
            "an altered payload must not verify"
        );
    }
}

#[cfg(test)]
mod policy_drift_tests {
    use super::*;
    use swarm_domain::{ApiaryPolicySetting, ProviderKind};

    fn setting(key: &str, value: &str) -> ApiaryPolicySetting {
        ApiaryPolicySetting {
            key: key.to_owned(),
            value: value.to_owned(),
        }
    }

    fn worker() -> HiveCapabilityWorker {
        HiveCapabilityWorker {
            name: "Platform".to_owned(),
            provider: ProviderKind::ClaudeCode,
            repository: Some("git@github.com:rcg/platform.git".to_owned()),
            awake: false,
        }
    }

    /// ⚠️ THE DONE WHEN'S SECOND HALF: drift visible to KEEPER, not just the
    /// member. The member sends what it RUNS; Keeper holds what the Apiary
    /// EXPECTS; the gap is derived at Keeper. A member cannot declare itself
    /// compliant, which is the property that makes this worth having.
    #[test]
    fn keeper_computes_each_members_drift_from_the_report_it_already_holds() {
        let now = 120_000;
        let (keeper, member) = crate::federation::tests::joined_member(now);
        let credential = member
            .federation_member_connection()
            .unwrap()
            .node_credential;

        let defaults = vec![
            setting("checks.max_warnings", "0"),
            setting("checks.required", "true"),
        ];
        keeper
            .set_apiary_policy_settings(&defaults, now + 5)
            .unwrap();
        let policy = keeper.signed_apiary_policy(&credential, now + 6).unwrap();
        member.apply_apiary_policy(&policy, now + 7).unwrap();

        // The member converges on one default and overrides the other.
        member
            .record_local_policy_setting(&setting("checks.required", "true"), now + 8)
            .unwrap();
        member
            .record_local_policy_setting(&setting("checks.max_warnings", "5"), now + 8)
            .unwrap();

        let report = member
            .seal_local_hive_capability(&[worker()], false, "1.12.0", 186, now + 9)
            .unwrap();
        assert_eq!(
            report.payload.policy_revision,
            policy.payload.policy_revision.into()
        );
        keeper
            .accept_hive_capability(&credential, &report, now + 10)
            .unwrap();

        let drift = keeper.member_policy_drift().unwrap();
        assert_eq!(drift.len(), 1);
        let (_, entries) = &drift[0];
        assert_eq!(entries.len(), 1, "only the override differs");
        assert_eq!(entries[0].key, "checks.max_warnings");
        assert_eq!(entries[0].expected, "0");
        assert_eq!(
            entries[0].local.as_deref(),
            Some("5"),
            "Keeper sees what the member actually runs"
        );
    }

    /// A member that holds no policy body has nothing to have deviated from.
    /// Listing every default as missing would make it look non-compliant for a
    /// change it was never handed.
    #[test]
    fn a_pre_body_member_reports_no_drift_rather_than_total_drift() {
        let now = 120_000;
        let (keeper, member) = crate::federation::tests::joined_member(now);
        let credential = member
            .federation_member_connection()
            .unwrap()
            .node_credential;
        keeper
            .set_apiary_policy_settings(&[setting("checks.required", "true")], now + 5)
            .unwrap();

        // Sealed WITHOUT ever applying a policy body.
        let report = member
            .seal_local_hive_capability(&[worker()], false, "1.12.0", 186, now + 9)
            .unwrap();
        assert_eq!(report.payload.policy_revision, None);
        keeper
            .accept_hive_capability(&credential, &report, now + 10)
            .unwrap();

        let drift = keeper.member_policy_drift().unwrap();
        assert_eq!(drift.len(), 1);
        assert!(
            drift[0].1.is_empty(),
            "a member awaiting a body is not in breach of it"
        );
    }

    /// Applying a policy setting locally must move the report, or Keeper's view
    /// of drift would be frozen at whatever the member ran first.
    #[test]
    fn changing_an_applied_setting_advances_the_report() {
        let now = 120_000;
        let (_keeper, member) = crate::federation::tests::joined_member(now);
        let first = member
            .seal_local_hive_capability(&[worker()], false, "1.12.0", 186, now + 9)
            .unwrap();
        member
            .record_local_policy_setting(&setting("checks.required", "true"), now + 10)
            .unwrap();
        let second = member
            .seal_local_hive_capability(&[worker()], false, "1.12.0", 186, now + 11)
            .unwrap();
        assert_eq!(
            second.payload.revision,
            first.payload.revision + 1,
            "a changed local setting is a changed capability report"
        );
        assert_eq!(second.payload.applied_policy.len(), 1);
    }
}
