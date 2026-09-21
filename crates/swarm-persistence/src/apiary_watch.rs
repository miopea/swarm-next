//! The control plane for watching a Hive.
//!
//! ⚠️ THE SESSION IS RECORDED; THE FRAMES NEVER ARE. That split is the design.
//! These tables hold who watched which Hive and when — the audit trail that
//! answers "who looked" — and there is no column here, on any table, that could
//! hold terminal output. A relay that wanted to persist a frame would have to
//! add one, which is the point: the absence is the enforcement.
//!
//! Both sides keep a row. Keeper's is the authority; the member's mirror is
//! what its own operator is SHOWN, so the notice does not depend on Keeper
//! being reachable at the moment the operator looks.

use rusqlite::{OptionalExtension, Transaction, params};
use swarm_domain::{
    ApiaryId, ApiaryWatch, ApiaryWatchId, HiveId, OperatorId, StewardshipId, WATCH_LEASE_SECONDS,
    WatchAuthority, WatchState,
};

use crate::{TaskStore, TaskStoreError, parse_domain_id};

pub(super) fn migrate(transaction: &Transaction<'_>) -> rusqlite::Result<()> {
    transaction.execute_batch(
        "CREATE TABLE IF NOT EXISTS apiary_watches (
            watch_id TEXT PRIMARY KEY,
            apiary_id TEXT NOT NULL REFERENCES apiaries(id),
            watcher_operator_id TEXT NOT NULL REFERENCES operators(id),
            target_hive_id TEXT NOT NULL REFERENCES hives(id),
            stewardship_id TEXT REFERENCES stewardships(id),
            state TEXT NOT NULL CHECK (state IN ('requested','active','ended')),
            requested_at INTEGER NOT NULL,
            acknowledged_at INTEGER,
            expires_at INTEGER NOT NULL,
            ended_at INTEGER
         );
         CREATE INDEX IF NOT EXISTS apiary_watch_target
             ON apiary_watches(apiary_id, target_hive_id, requested_at DESC);
         CREATE TABLE IF NOT EXISTS local_federation_watches (
            watch_id TEXT PRIMARY KEY,
            apiary_id TEXT NOT NULL,
            watcher_operator_id TEXT NOT NULL,
            target_hive_id TEXT NOT NULL,
            stewardship_id TEXT,
            state TEXT NOT NULL CHECK (state IN ('requested','active','ended')),
            requested_at INTEGER NOT NULL,
            acknowledged_at INTEGER,
            expires_at INTEGER NOT NULL,
            ended_at INTEGER,
            synced_at INTEGER NOT NULL
         );",
    )?;
    transaction.pragma_update(None, "user_version", crate::APIARY_WATCH_SCHEMA_MARKER)
}

fn read_watches(
    transaction: &Transaction<'_>,
    table: &str,
    filter: &str,
    params: &[&dyn rusqlite::ToSql],
) -> Result<Vec<ApiaryWatch>, TaskStoreError> {
    let mut statement = transaction.prepare(&format!(
        "SELECT watch_id, apiary_id, watcher_operator_id, target_hive_id, stewardship_id,
                state, requested_at, acknowledged_at, expires_at, ended_at
         FROM {table} {filter}"
    ))?;
    let rows = statement.query_map(params, |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, Option<String>>(4)?,
            row.get::<_, String>(5)?,
            row.get::<_, i64>(6)?,
            row.get::<_, Option<i64>>(7)?,
            row.get::<_, i64>(8)?,
            row.get::<_, Option<i64>>(9)?,
        ))
    })?;
    let mut watches = Vec::new();
    for row in rows {
        let (id, apiary, observer, target, stewardship, state, requested, acked, expires, ended) =
            row?;
        watches.push(ApiaryWatch {
            id: parse_domain_id::<ApiaryWatchId>(&id)?,
            apiary_id: parse_domain_id::<ApiaryId>(&apiary)?,
            watcher_operator_id: parse_domain_id::<OperatorId>(&observer)?,
            target_hive_id: parse_domain_id::<HiveId>(&target)?,
            authority: match stewardship {
                Some(stewardship) => {
                    WatchAuthority::Steward(parse_domain_id::<StewardshipId>(&stewardship)?)
                }
                None => WatchAuthority::Keeper,
            },
            state: match state.as_str() {
                "requested" => WatchState::Requested,
                "active" => WatchState::Active,
                _ => WatchState::Ended,
            },
            requested_at: requested,
            acknowledged_at: acked,
            expires_at: expires,
            ended_at: ended,
        });
    }
    Ok(watches)
}

fn state_text(state: WatchState) -> &'static str {
    match state {
        WatchState::Requested => "requested",
        WatchState::Active => "active",
        WatchState::Ended => "ended",
    }
}

fn stewardship_of(authority: WatchAuthority) -> Option<String> {
    match authority {
        WatchAuthority::Keeper => None,
        WatchAuthority::Steward(id) => Some(id.to_string()),
    }
}

impl TaskStore {
    /// Opens a watch on a member Hive, if this operator may.
    ///
    /// Authority is rechecked HERE, against the grants as they stand right now,
    /// rather than trusted from anything the caller holds — a revoked
    /// stewardship has to stop working at the moment it is revoked, not at the
    /// moment something cached expires.
    ///
    /// # Errors
    /// Refuses an operator without authority over that Hive, a Hive that is not
    /// an active member, and unavailable persistence.
    pub fn open_apiary_watch(
        &self,
        watcher_operator_id: OperatorId,
        target_hive_id: HiveId,
        now: i64,
    ) -> Result<ApiaryWatch, TaskStoreError> {
        let identity = self.local_hive_identity()?;
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let (apiary_id, keeper_operator_id) = transaction
            .query_row(
                "SELECT id, keeper_operator_id FROM apiaries
                 WHERE keeper_operator_id = ?1 AND collapsed_at IS NULL",
                params![identity.operator.id.to_string()],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
            )
            .optional()?
            .ok_or(TaskStoreError::InvalidStewardship)?;
        let apiary_id = parse_domain_id::<ApiaryId>(&apiary_id)?;
        let keeper_operator_id = parse_domain_id::<OperatorId>(&keeper_operator_id)?;
        let authority = watch_authority_in(
            &transaction,
            apiary_id,
            watcher_operator_id,
            keeper_operator_id,
            target_hive_id,
        )?
        .ok_or(TaskStoreError::InvalidStewardship)?;
        let member = transaction.query_row(
            "SELECT EXISTS(SELECT 1 FROM apiary_federation_memberships
             WHERE apiary_id = ?1 AND member_hive_id = ?2 AND state = 'active')",
            params![apiary_id.to_string(), target_hive_id.to_string()],
            |row| row.get::<_, bool>(0),
        )?;
        if !member {
            return Err(TaskStoreError::InvalidStewardship);
        }
        let watch = ApiaryWatch {
            id: ApiaryWatchId::new(),
            apiary_id,
            watcher_operator_id,
            target_hive_id,
            authority,
            state: WatchState::Requested,
            requested_at: now,
            acknowledged_at: None,
            expires_at: now.saturating_add(WATCH_LEASE_SECONDS),
            ended_at: None,
        };
        transaction.execute(
            "INSERT INTO apiary_watches
                (watch_id, apiary_id, watcher_operator_id, target_hive_id, stewardship_id,
                 state, requested_at, acknowledged_at, expires_at, ended_at)
             VALUES (?1, ?2, ?3, ?4, ?5, 'requested', ?6, NULL, ?7, NULL)",
            params![
                watch.id.to_string(),
                apiary_id.to_string(),
                watcher_operator_id.to_string(),
                target_hive_id.to_string(),
                stewardship_of(authority),
                now,
                watch.expires_at,
            ],
        )?;
        transaction.commit()?;
        Ok(watch)
    }

    /// The watches an authenticated member should be showing its operator.
    ///
    /// Served to the MEMBER about ITSELF. It carries no other Hive's watches,
    /// because who else is being watched is nobody's business but theirs.
    ///
    /// # Errors
    /// Rejects an invalid credential and unavailable persistence.
    pub fn federation_watch_inbox(
        &self,
        credential: &str,
        now: i64,
    ) -> Result<Vec<ApiaryWatch>, TaskStoreError> {
        let (apiary_id, hive_id) = self.authenticated_member_hive(credential, now)?;
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let watches = read_watches(
            &transaction,
            "apiary_watches",
            "WHERE apiary_id = ?1 AND target_hive_id = ?2 AND state IN ('requested','active')
             ORDER BY requested_at, watch_id",
            params![apiary_id.to_string(), hive_id.to_string()],
        )?;
        transaction.commit()?;
        Ok(watches)
    }

    /// A member acknowledging that it knows it is being watched.
    ///
    /// ⚠️ NOTHING IS RELAYED UNTIL THIS HAPPENS. The acknowledgement is the
    /// member saying it has the watch on its own screen; making it the gate is
    /// what turns "always visible" from a promise into a sequence.
    ///
    /// # Errors
    /// Rejects an invalid credential, a watch on another Hive, and unavailable
    /// persistence.
    pub fn acknowledge_federation_watch(
        &self,
        credential: &str,
        watch_id: ApiaryWatchId,
        now: i64,
    ) -> Result<ApiaryWatch, TaskStoreError> {
        let (apiary_id, hive_id) = self.authenticated_member_hive(credential, now)?;
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let changed = transaction.execute(
            "UPDATE apiary_watches SET state = 'active', acknowledged_at = COALESCE(acknowledged_at, ?4)
             WHERE watch_id = ?1 AND apiary_id = ?2 AND target_hive_id = ?3 AND state = 'requested'",
            params![
                watch_id.to_string(),
                apiary_id.to_string(),
                hive_id.to_string(),
                now
            ],
        )?;
        if changed == 0 {
            return Err(TaskStoreError::InvalidStewardship);
        }
        let watch = read_watches(
            &transaction,
            "apiary_watches",
            "WHERE watch_id = ?1",
            params![watch_id.to_string()],
        )?
        .pop()
        .ok_or(TaskStoreError::InvalidStewardship)?;
        transaction.commit()?;
        Ok(watch)
    }

    /// Extends a watch the watcher still has open.
    ///
    /// Renewal is what stops a forgotten window from becoming standing
    /// surveillance: the watcher has to keep saying they are there.
    ///
    /// # Errors
    /// Refuses a watch this operator does not hold, one already ended or
    /// lapsed, and unavailable persistence.
    pub fn renew_apiary_watch(
        &self,
        watcher_operator_id: OperatorId,
        watch_id: ApiaryWatchId,
        now: i64,
    ) -> Result<ApiaryWatch, TaskStoreError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        // ⚠️ `expires_at > ?4` REFUSES TO RESURRECT A LAPSED WATCH. Without it,
        // renewal would quietly revive one the target has already stopped
        // showing, and the window would be open again with no notice beside it.
        let changed = transaction.execute(
            "UPDATE apiary_watches SET expires_at = ?3
             WHERE watch_id = ?1 AND watcher_operator_id = ?2
               AND state IN ('requested','active') AND expires_at > ?4",
            params![
                watch_id.to_string(),
                watcher_operator_id.to_string(),
                now.saturating_add(WATCH_LEASE_SECONDS),
                now
            ],
        )?;
        if changed == 0 {
            return Err(TaskStoreError::InvalidStewardship);
        }
        let watch = read_watches(
            &transaction,
            "apiary_watches",
            "WHERE watch_id = ?1",
            params![watch_id.to_string()],
        )?
        .pop()
        .ok_or(TaskStoreError::InvalidStewardship)?;
        transaction.commit()?;
        Ok(watch)
    }

    /// Closes a watch. Idempotent, and open to either side.
    ///
    /// ⚠️ THE WATCHED OPERATOR MAY CLOSE IT, not only the watcher. The person
    /// at the machine may be mid-incident and is the only one who knows; ADR
    /// 0036 makes the same call for takeover reclaim, and the watcher losing a
    /// window they can simply reopen costs nothing.
    ///
    /// # Errors
    /// Returns an error when persistence is unavailable.
    pub fn end_apiary_watch(
        &self,
        watch_id: ApiaryWatchId,
        now: i64,
    ) -> Result<(), TaskStoreError> {
        let connection = self.connection()?;
        connection.execute(
            "UPDATE apiary_watches SET state = 'ended', ended_at = COALESCE(ended_at, ?2)
             WHERE watch_id = ?1 AND state IN ('requested','active')",
            params![watch_id.to_string(), now],
        )?;
        connection.execute(
            "UPDATE local_federation_watches SET state = 'ended', ended_at = COALESCE(ended_at, ?2)
             WHERE watch_id = ?1 AND state IN ('requested','active')",
            params![watch_id.to_string(), now],
        )?;
        Ok(())
    }

    /// A member mirroring what Keeper says is watching it.
    ///
    /// Replaces the local picture wholesale: a watch Keeper no longer lists is
    /// over, and leaving it on screen would be claiming an observer who is not
    /// there. The mirror exists so the notice survives Keeper being unreachable
    /// between polls, not so it can disagree with Keeper.
    ///
    /// # Errors
    /// Returns an error when persistence is unavailable.
    pub fn apply_federation_watch_inbox(
        &self,
        watches: &[ApiaryWatch],
        now: i64,
    ) -> Result<(), TaskStoreError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        transaction.execute(
            "UPDATE local_federation_watches SET state = 'ended', ended_at = COALESCE(ended_at, ?1)
             WHERE state IN ('requested','active')",
            params![now],
        )?;
        for watch in watches {
            transaction.execute(
                "INSERT INTO local_federation_watches
                    (watch_id, apiary_id, watcher_operator_id, target_hive_id, stewardship_id,
                     state, requested_at, acknowledged_at, expires_at, ended_at, synced_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, NULL, ?10)
                 ON CONFLICT(watch_id) DO UPDATE SET
                    state = excluded.state,
                    acknowledged_at = excluded.acknowledged_at,
                    expires_at = excluded.expires_at,
                    ended_at = NULL,
                    synced_at = excluded.synced_at",
                params![
                    watch.id.to_string(),
                    watch.apiary_id.to_string(),
                    watch.watcher_operator_id.to_string(),
                    watch.target_hive_id.to_string(),
                    stewardship_of(watch.authority),
                    state_text(watch.state),
                    watch.requested_at,
                    watch.acknowledged_at,
                    watch.expires_at,
                    now,
                ],
            )?;
        }
        transaction.commit()?;
        Ok(())
    }

    /// What this Hive's own operator must be shown: who is watching, right now.
    ///
    /// # Errors
    /// Returns an error when persistence is unavailable.
    pub fn local_open_watches(&self, now: i64) -> Result<Vec<ApiaryWatch>, TaskStoreError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let watches = read_watches(
            &transaction,
            "local_federation_watches",
            "WHERE state IN ('requested','active') ORDER BY requested_at, watch_id",
            params![],
        )?;
        transaction.commit()?;
        Ok(watches
            .into_iter()
            .filter(|watch| watch.is_visible(now))
            .collect())
    }

    /// Who looked, and when. The audit this design can answer.
    ///
    /// It cannot answer WHY, by the operator's explicit choice — see
    /// [`ApiaryWatch`].
    ///
    /// # Errors
    /// Returns an error when persistence is unavailable.
    pub fn apiary_watch_audit(&self, limit: u32) -> Result<Vec<ApiaryWatch>, TaskStoreError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let watches = read_watches(
            &transaction,
            "apiary_watches",
            "ORDER BY requested_at DESC, watch_id LIMIT ?1",
            params![limit.min(200)],
        )?;
        transaction.commit()?;
        Ok(watches)
    }

    /// The watch a Steward holds, checked for their outbound viewer socket.
    ///
    /// ⚠️ KEEPER IS THE AUTHORITY FOR THIS, NOT THE STEWARD'S OWN HIVE. The
    /// Steward's Hive proxies frames, and a proxy that decided for itself who
    /// may watch would be a second authority to keep in step with the grants —
    /// which is how a Steward ends up seeing a Hive after their stewardship was
    /// revoked.
    ///
    /// # Errors
    /// Rejects an invalid credential, and refuses a watch this member's
    /// operator does not hold or that is no longer live.
    pub fn federation_watch_for_watcher(
        &self,
        credential: &str,
        watch_id: ApiaryWatchId,
        now: i64,
    ) -> Result<ApiaryWatch, TaskStoreError> {
        let (apiary_id, _, operator) = self.authenticated_member(credential, now)?;
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let watch = read_watches(
            &transaction,
            "apiary_watches",
            "WHERE watch_id = ?1 AND apiary_id = ?2 AND watcher_operator_id = ?3",
            params![
                watch_id.to_string(),
                apiary_id.to_string(),
                operator.to_string()
            ],
        )?
        .pop()
        .ok_or(TaskStoreError::InvalidStewardship)?;
        if !watch.is_live(now) {
            return Err(TaskStoreError::InvalidStewardship);
        }
        transaction.commit()?;
        Ok(watch)
    }

    /// A Steward asking Keeper to open a watch, over its outbound connection.
    ///
    /// ⚠️ THE SAME `open_apiary_watch` BEHIND BOTH DOORS. Keeper's own operator
    /// enters one way and a Steward the other, and they meet at one authority
    /// function with no depth parameter to differ on. Two entry points that
    /// each did their own checking is how two observation depths get built by
    /// accident, which is exactly what this ticket says not to do.
    ///
    /// # Errors
    /// Rejects an invalid credential, refuses an operator without Observe over
    /// that Hive, and reports unavailable persistence.
    pub fn open_apiary_watch_for_member(
        &self,
        credential: &str,
        target_hive_id: HiveId,
        now: i64,
    ) -> Result<ApiaryWatch, TaskStoreError> {
        let (_, _, operator) = self.authenticated_member(credential, now)?;
        self.open_apiary_watch(operator, target_hive_id, now)
    }

    fn authenticated_member_hive(
        &self,
        credential: &str,
        now: i64,
    ) -> Result<(ApiaryId, HiveId), TaskStoreError> {
        let (apiary, hive, _) = self.authenticated_member(credential, now)?;
        Ok((apiary, hive))
    }

    fn authenticated_member(
        &self,
        credential: &str,
        now: i64,
    ) -> Result<(ApiaryId, HiveId, OperatorId), TaskStoreError> {
        use base64ct::{Base64UrlUnpadded, Encoding};
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
        transaction.commit()?;
        Ok((member.apiary, member.hive, member.operator))
    }
}

/// Whether this operator may watch this Hive, read from the grants as they
/// stand.
///
/// ⚠️ THIS ANSWERS "WHICH", NEVER "HOW MUCH". There is no depth to return.
fn watch_authority_in(
    transaction: &Transaction<'_>,
    apiary_id: ApiaryId,
    viewer: OperatorId,
    keeper_operator_id: OperatorId,
    target_hive_id: HiveId,
) -> Result<Option<WatchAuthority>, TaskStoreError> {
    if viewer == keeper_operator_id {
        return Ok(Some(WatchAuthority::Keeper));
    }
    let stewardship = transaction
        .query_row(
            "SELECT s.id FROM stewardships s
             JOIN stewardship_hive_grants h ON h.stewardship_id = s.id AND h.hive_id = ?3
             JOIN stewardship_capability_grants c ON c.stewardship_id = s.id AND c.capability = 'observe'
             WHERE s.apiary_id = ?1 AND s.steward_operator_id = ?2 AND s.revoked_at IS NULL
             ORDER BY s.created_at DESC LIMIT 1",
            params![
                apiary_id.to_string(),
                viewer.to_string(),
                target_hive_id.to_string()
            ],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    stewardship
        .map(|id| {
            Ok(WatchAuthority::Steward(parse_domain_id::<StewardshipId>(
                &id,
            )?))
        })
        .transpose()
}

#[cfg(test)]
mod tests {
    use swarm_domain::StewardCapability;

    use super::*;

    /// Keeper plus one joined member, and the member's credential.
    fn apiary(now: i64) -> (TaskStore, TaskStore, String, HiveId) {
        let (keeper, member) = crate::federation::tests::joined_member(now);
        let connection = member.federation_member_connection().unwrap();
        let hive = member.local_hive_identity().unwrap().hive.id;
        (keeper, member, connection.node_credential, hive)
    }

    fn keeper_operator(keeper: &TaskStore) -> OperatorId {
        keeper.local_hive_identity().unwrap().operator.id
    }

    /// ⚠️ THE PROMISE THIS WHOLE DESIGN RESTS ON. A watch the member cannot see
    /// is the standing surveillance the ticket says must not ship, so the
    /// notice is asserted from the MEMBER's own store rather than from Keeper's.
    #[test]
    fn a_watch_is_on_the_members_own_screen_before_anything_is_relayed() {
        let now = 200_000;
        let (keeper, member, credential, hive) = apiary(now);
        let watch = keeper
            .open_apiary_watch(keeper_operator(&keeper), hive, now)
            .unwrap();
        assert_eq!(watch.state, WatchState::Requested);
        assert!(!watch.is_live(now), "nothing flows before acknowledgement");

        let inbox = keeper.federation_watch_inbox(&credential, now).unwrap();
        assert_eq!(inbox.len(), 1);
        member.apply_federation_watch_inbox(&inbox, now).unwrap();

        let shown = member.local_open_watches(now).unwrap();
        assert_eq!(shown.len(), 1, "the member is told before the window opens");
        assert_eq!(shown[0].watcher_operator_id, keeper_operator(&keeper));
        assert_eq!(shown[0].authority, WatchAuthority::Keeper);

        let acknowledged = keeper
            .acknowledge_federation_watch(&credential, watch.id, now + 1)
            .unwrap();
        assert!(acknowledged.is_live(now + 1), "and only then does it relay");
        assert_eq!(acknowledged.acknowledged_at, Some(now + 1));
    }

    /// Ending it must clear the notice too. A watch that has stopped while the
    /// screen still says someone is looking is the same lie as the reverse.
    #[test]
    fn ending_a_watch_takes_it_off_the_members_screen() {
        let now = 200_000;
        let (keeper, member, credential, hive) = apiary(now);
        let watch = keeper
            .open_apiary_watch(keeper_operator(&keeper), hive, now)
            .unwrap();
        let inbox = keeper.federation_watch_inbox(&credential, now).unwrap();
        member.apply_federation_watch_inbox(&inbox, now).unwrap();
        assert_eq!(member.local_open_watches(now).unwrap().len(), 1);

        member.end_apiary_watch(watch.id, now + 5).unwrap();
        assert!(
            member.local_open_watches(now + 6).unwrap().is_empty(),
            "the watched operator can close it from their own machine"
        );

        keeper.end_apiary_watch(watch.id, now + 5).unwrap();
        assert!(
            keeper
                .federation_watch_inbox(&credential, now + 6)
                .unwrap()
                .is_empty()
        );
        let audit = keeper.apiary_watch_audit(10).unwrap();
        assert_eq!(audit.len(), 1, "but the record of who looked survives it");
        assert_eq!(audit[0].ended_at, Some(now + 5));
    }

    /// ⚠️ A FORGOTTEN WINDOW MUST CLOSE ITSELF. Expiry is what keeps this from
    /// becoming the standing surveillance it is allowed to exist instead of.
    #[test]
    fn a_watch_nobody_renewed_lapses_and_cannot_be_revived() {
        let now = 200_000;
        let (keeper, member, credential, hive) = apiary(now);
        let watcher = keeper_operator(&keeper);
        let watch = keeper.open_apiary_watch(watcher, hive, now).unwrap();
        keeper
            .acknowledge_federation_watch(&credential, watch.id, now + 1)
            .unwrap();
        let inbox = keeper.federation_watch_inbox(&credential, now + 1).unwrap();
        member
            .apply_federation_watch_inbox(&inbox, now + 1)
            .unwrap();

        let lapsed = now + WATCH_LEASE_SECONDS;
        assert!(
            member.local_open_watches(lapsed).unwrap().is_empty(),
            "the notice goes when the lease does, not when something sweeps it"
        );
        assert!(
            keeper
                .renew_apiary_watch(watcher, watch.id, lapsed)
                .is_err(),
            "and a lapsed watch cannot be quietly resurrected behind the notice"
        );

        // Renewing while it is still live does extend it.
        let watch = keeper.open_apiary_watch(watcher, hive, lapsed).unwrap();
        let renewed = keeper
            .renew_apiary_watch(watcher, watch.id, lapsed + 10)
            .unwrap();
        assert_eq!(renewed.expires_at, lapsed + 10 + WATCH_LEASE_SECONDS);
    }

    /// ⚠️ THE GRANT DECIDES WHICH HIVES, NEVER HOW MUCH — and a stewardship
    /// without Observe opens no window at all. Rechecked at Keeper against the
    /// grants as they stand, so revoking one stops it working immediately
    /// rather than whenever something cached expires.
    #[test]
    fn authority_is_rechecked_at_keeper_against_the_grants_as_they_stand() {
        let now = 200_000;
        let (keeper, member, _credential, hive) = apiary(now);
        let steward = member.local_hive_identity().unwrap().operator.id;

        assert!(
            keeper
                .open_apiary_watch(OperatorId::new(), hive, now)
                .is_err(),
            "an operator with no grant over this Hive is refused"
        );
        assert!(
            keeper.open_apiary_watch(steward, hive, now).is_err(),
            "and so is one whose stewardship does not exist yet"
        );

        // ⚠️ A STEWARDSHIP WITHOUT OBSERVE CANNOT BE CREATED AT ALL, which
        // matters more than it looks. It means this upgrade widens EVERY
        // stewardship that will ever exist, not only the ones whose Keeper
        // happened to tick Observe — there is no assist-only Steward to be had.
        // Whoever is told about the widening should be told on those terms.
        assert!(
            keeper
                .set_stewardship(
                    steward,
                    &[hive],
                    &[StewardCapability::Assign, StewardCapability::Assist],
                    now,
                )
                .is_err(),
            "Observe is mandatory in a grant, so line of sight is not opt-in"
        );

        let granted = keeper
            .set_stewardship(steward, &[hive], &[StewardCapability::Observe], now + 2)
            .unwrap();
        let watch = keeper.open_apiary_watch(steward, hive, now + 3).unwrap();
        assert_eq!(watch.authority, WatchAuthority::Steward(granted.id));

        keeper.revoke_stewardship(granted.id, now + 4).unwrap();
        assert!(
            keeper.open_apiary_watch(steward, hive, now + 5).is_err(),
            "a revoked stewardship stops working at once"
        );
        assert_eq!(
            keeper.apiary_watch_audit(10).unwrap().len(),
            1,
            "only the one authorized watch was ever recorded"
        );
    }
}
