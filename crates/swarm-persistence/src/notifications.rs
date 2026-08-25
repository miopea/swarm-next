use std::str::FromStr;

use rusqlite::{Connection, OptionalExtension, Transaction, params};
use swarm_domain::{
    ControlRoomEventKind, DecisionRequestId, DecisionUrgency, NotificationPolicy, OperatorId,
    PresenceDeviceClass, PresenceDeviceId, PresenceMode,
};

use super::{
    TaskStore, TaskStoreError, insert_control_room_event,
    presence::{local_operator_id, operator_presence_from_connection},
};

pub const MAX_NOTIFICATION_SUBSCRIPTIONS: i64 = 8;
const MAX_NOTIFICATION_DELIVERIES: i64 = 128;
const MAX_NOTIFICATION_CLAIMS: i64 = 8;
const MAX_NOTIFICATION_ATTEMPTS: i64 = 5;
const MAX_ENDPOINT_BYTES: usize = 4096;
const P256DH_BYTES: usize = 65;
const AUTH_BYTES: usize = 16;
const VAPID_PRIVATE_BYTES: usize = 32;
const VAPID_PUBLIC_BYTES: usize = 65;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VapidKeyMaterial {
    pub private_key: Vec<u8>,
    pub public_key: Vec<u8>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize)]
pub struct NotificationSettings {
    pub policy: NotificationPolicy,
    pub subscription_count: i64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PushSubscriptionInput {
    pub device_id: PresenceDeviceId,
    pub device_class: PresenceDeviceClass,
    pub endpoint: String,
    pub p256dh: Vec<u8>,
    pub auth: Vec<u8>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NotificationDispatch {
    pub delivery_id: i64,
    pub subscription_id: PresenceDeviceId,
    pub endpoint: String,
    pub p256dh: Vec<u8>,
    pub auth: Vec<u8>,
    pub decision_id: Option<DecisionRequestId>,
    pub urgency: DecisionUrgency,
    pub test: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NotificationDeliveryFailure {
    Retryable,
    Gone,
    Permanent,
}

impl TaskStore {
    /// Stores one installation VAPID key pair if absent and returns the durable pair.
    ///
    /// # Errors
    /// Rejects malformed key lengths or persistence failures.
    pub fn ensure_vapid_key(
        &self,
        private_key: &[u8],
        public_key: &[u8],
    ) -> Result<VapidKeyMaterial, TaskStoreError> {
        if private_key.len() != VAPID_PRIVATE_BYTES || public_key.len() != VAPID_PUBLIC_BYTES {
            return Err(TaskStoreError::InvalidVapidKey);
        }
        let connection = self.connection()?;
        connection.execute(
            "INSERT OR IGNORE INTO notification_vapid_keys (
                 singleton, private_key, public_key, created_at
             ) VALUES (1, ?1, ?2, unixepoch())",
            params![private_key, public_key],
        )?;
        connection
            .query_row(
                "SELECT private_key, public_key FROM notification_vapid_keys WHERE singleton = 1",
                [],
                |row| {
                    Ok(VapidKeyMaterial {
                        private_key: row.get(0)?,
                        public_key: row.get(1)?,
                    })
                },
            )
            .map_err(Into::into)
    }

    /// Returns policy and bounded subscription count without exposing endpoints or keys.
    ///
    /// # Errors
    /// Returns persistence or integrity failures.
    pub fn notification_settings(&self) -> Result<NotificationSettings, TaskStoreError> {
        let connection = self.connection()?;
        notification_settings_from_connection(&connection)
    }

    /// Reports whether the local operator still owns one device registration.
    ///
    /// The endpoint and key material never leave persistence. Clients use this
    /// bit to replace browser subscriptions that a push service has declared
    /// gone instead of repeatedly saving the same dead endpoint.
    ///
    /// # Errors
    /// Returns persistence or local-identity failures.
    pub fn has_notification_subscription(
        &self,
        device_id: PresenceDeviceId,
    ) -> Result<bool, TaskStoreError> {
        let connection = self.connection()?;
        let operator_id = local_operator_id(&connection)?;
        connection
            .query_row(
                "SELECT EXISTS(
                     SELECT 1 FROM notification_subscriptions
                     WHERE device_id = ?1 AND operator_id = ?2
                 )",
                params![device_id.to_string(), operator_id.to_string()],
                |row| row.get(0),
            )
            .map_err(Into::into)
    }

    /// Changes notification policy and queues newly eligible pending decisions.
    ///
    /// # Errors
    /// Returns persistence or integrity failures.
    pub fn set_notification_policy(
        &self,
        policy: NotificationPolicy,
        now: i64,
    ) -> Result<NotificationSettings, TaskStoreError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let operator_id = local_operator_id(&transaction)?;
        let before = notification_policy(&transaction, operator_id)?;
        transaction.execute(
            "INSERT INTO notification_preferences (operator_id, policy, updated_at)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(operator_id) DO UPDATE SET
                 policy = excluded.policy, updated_at = excluded.updated_at",
            params![operator_id.to_string(), policy.to_string(), now],
        )?;
        if before != policy {
            enqueue_pending_notifications(&transaction, now)?;
            insert_control_room_event(&transaction, ControlRoomEventKind::NotificationsChanged)?;
        }
        let settings = notification_settings_from_connection(&transaction)?;
        transaction.commit()?;
        Ok(settings)
    }

    /// Adds or refreshes one validated browser subscription.
    ///
    /// # Errors
    /// Rejects invalid material, capacity overflow, or persistence failures.
    pub fn save_notification_subscription(
        &self,
        input: &PushSubscriptionInput,
        now: i64,
    ) -> Result<NotificationSettings, TaskStoreError> {
        validate_subscription(input)?;
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let operator_id = local_operator_id(&transaction)?;
        transaction.execute(
            "DELETE FROM notification_subscriptions
             WHERE endpoint = ?1 AND device_id <> ?2",
            params![input.endpoint, input.device_id.to_string()],
        )?;
        let existing = transaction
            .query_row(
                "SELECT device_class, endpoint, p256dh, auth
                 FROM notification_subscriptions WHERE device_id = ?1",
                [input.device_id.to_string()],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, Vec<u8>>(2)?,
                        row.get::<_, Vec<u8>>(3)?,
                    ))
                },
            )
            .optional()?;
        if existing.is_none() {
            let count: i64 = transaction.query_row(
                "SELECT COUNT(*) FROM notification_subscriptions WHERE operator_id = ?1",
                [operator_id.to_string()],
                |row| row.get(0),
            )?;
            if count >= MAX_NOTIFICATION_SUBSCRIPTIONS {
                return Err(TaskStoreError::NotificationSubscriptionLimit);
            }
        }
        let changed = existing.as_ref().is_none_or(|current| {
            current.0 != input.device_class.to_string()
                || current.1 != input.endpoint
                || current.2 != input.p256dh
                || current.3 != input.auth
        });
        transaction.execute(
            "INSERT INTO notification_subscriptions (
                 device_id, operator_id, device_class, endpoint, p256dh, auth,
                 failure_count, created_at, updated_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, 0, ?7, ?7)
             ON CONFLICT(device_id) DO UPDATE SET
                 operator_id = excluded.operator_id,
                 device_class = excluded.device_class,
                 endpoint = excluded.endpoint,
                 p256dh = excluded.p256dh,
                 auth = excluded.auth,
                 failure_count = 0,
                 updated_at = excluded.updated_at",
            params![
                input.device_id.to_string(),
                operator_id.to_string(),
                input.device_class.to_string(),
                input.endpoint,
                input.p256dh,
                input.auth,
                now,
            ],
        )?;
        if changed {
            insert_control_room_event(&transaction, ControlRoomEventKind::NotificationsChanged)?;
        }
        let settings = notification_settings_from_connection(&transaction)?;
        transaction.commit()?;
        Ok(settings)
    }

    /// Removes one device subscription and its cascaded queued deliveries.
    ///
    /// # Errors
    /// Returns persistence failures.
    pub fn remove_notification_subscription(
        &self,
        device_id: PresenceDeviceId,
    ) -> Result<NotificationSettings, TaskStoreError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let operator_id = local_operator_id(&transaction)?;
        let removed = transaction.execute(
            "DELETE FROM notification_subscriptions
             WHERE device_id = ?1 AND operator_id = ?2",
            params![device_id.to_string(), operator_id.to_string()],
        )?;
        if removed > 0 {
            insert_control_room_event(&transaction, ControlRoomEventKind::NotificationsChanged)?;
        }
        let settings = notification_settings_from_connection(&transaction)?;
        transaction.commit()?;
        Ok(settings)
    }

    /// Queues one explicit generic test notification per current subscription.
    ///
    /// # Errors
    /// Returns a bounded-capacity or persistence failure.
    pub fn enqueue_test_notifications(&self, now: i64) -> Result<i64, TaskStoreError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let operator_id = local_operator_id(&transaction)?;
        let available = available_delivery_slots(&transaction)?;
        let subscriptions: i64 = transaction.query_row(
            "SELECT COUNT(*) FROM notification_subscriptions WHERE operator_id = ?1",
            [operator_id.to_string()],
            |row| row.get(0),
        )?;
        if subscriptions > available {
            return Err(TaskStoreError::NotificationQueueFull);
        }
        let inserted = transaction.execute(
            // A FRESH SUBJECT PER TEST, so two tests never collapse into one.
            // Deduplication is now keyed on subject_key, and the old key was
            // (decision_id, subscription_id) with decision_id NULL — which
            // SQLite treats as distinct every time, so repeated tests always
            // queued. A constant key here would silently swallow the second
            // press of a button whose entire purpose is proving a device works.
            "INSERT INTO notification_deliveries (
                 operator_id, subscription_id, decision_id, subject_key, urgency,
                 kind, state, attempts, available_at, created_at
             )
             SELECT operator_id, device_id, NULL,
                    'test:' || lower(hex(randomblob(8))), 'time_sensitive', 'test',
                    'queued', 0, ?2, ?2
             FROM notification_subscriptions WHERE operator_id = ?1",
            params![operator_id.to_string(), now],
        )?;
        transaction.commit()?;
        i64::try_from(inserted)
            .map_err(|_| TaskStoreError::IntegrityFailure("notification count overflow".into()))
    }

    /// Queues one explicit generic test for the selected browser device only.
    ///
    /// # Errors
    /// Returns a bounded-capacity or persistence failure.
    pub fn enqueue_device_test_notification(
        &self,
        device_id: PresenceDeviceId,
        now: i64,
    ) -> Result<bool, TaskStoreError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        if available_delivery_slots(&transaction)? < 1 {
            return Err(TaskStoreError::NotificationQueueFull);
        }
        let operator_id = local_operator_id(&transaction)?;
        let inserted = transaction.execute(
            "INSERT INTO notification_deliveries (
                 operator_id, subscription_id, decision_id, subject_key, urgency,
                 kind, state, attempts, available_at, created_at
             )
             SELECT operator_id, device_id, NULL,
                    'test:' || lower(hex(randomblob(8))), 'time_sensitive', 'test',
                    'queued', 0, ?3, ?3
             FROM notification_subscriptions
             WHERE operator_id = ?1 AND device_id = ?2",
            params![operator_id.to_string(), device_id.to_string(), now],
        )?;
        transaction.commit()?;
        Ok(inserted == 1)
    }

    /// Recovers crash-interrupted sends for idempotent tagged retry.
    ///
    /// # Errors
    /// Returns persistence failures.
    /// The recorded watermark, for tests that need to see it moved.
    ///
    /// # Errors
    /// Returns a persistence error when it cannot be read.
    #[doc(hidden)]
    pub fn attention_watermark_for_test(&self) -> Result<i64, TaskStoreError> {
        let connection = self.connection()?;
        let operator_id = local_operator_id(&connection)?;
        attention_watermark_from(&connection, operator_id)
    }

    /// Sweeps the Needs-you queue and queues pushes for anything new.
    ///
    /// EXISTS BECAUSE NOTHING SWEPT ON A TIMER. The pending sweep ran only when
    /// presence or policy CHANGED, so it caught "they walked away and work was
    /// already waiting" and missed "they were already away and work arrived" —
    /// which is the case actually reported: the operator went out mid-session
    /// and work sat. Without a periodic caller the widening would have been
    /// untestable in the field and silent in the one scenario it was built for.
    ///
    /// Cheap and idempotent: `INSERT OR IGNORE` against a subject key, so
    /// running it every tick queues each item once and then does nothing.
    ///
    /// # Errors
    /// Returns a persistence error when the queue cannot be read or written.
    pub fn sweep_attention_notifications(&self, now: i64) -> Result<usize, TaskStoreError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        // WITHDRAW BEFORE QUEUEING. Retraction used to happen only when
        // something claimed, so an item handled between the queueing and the
        // next claim stayed on the queue waiting to be sent. On a Hive whose
        // operator is away — the case this whole path serves — claims are
        // exactly what is not happening, so "cancel on claim" is the weakest
        // possible moment to do it.
        cancel_ineligible_deliveries(&transaction, now)?;
        let queued = enqueue_pending_notifications(&transaction, now)?;
        transaction.commit()?;
        Ok(queued)
    }

    /// Records that the operator has just looked at Needs you.
    ///
    /// This is the anti-shouting mechanism, so it matters WHEN it is called:
    /// while the queue is actually on screen, not on every poll. Calling it
    /// from a background refresh would mark work seen that nobody read, and
    /// then nothing would ever fire.
    ///
    /// Per operator, never per device or per tab — they use more than one
    /// window, and a per-tab watermark would re-notify for work they read on
    /// the other screen an hour ago.
    ///
    /// Queued pushes for work they are looking at RIGHT NOW are withdrawn in
    /// the same breath. Walking away from a queue you just read should not be
    /// followed by a buzz about it.
    ///
    /// # Errors
    /// Returns a persistence error when the watermark cannot be written.
    pub fn record_attention_seen(&self, now: i64) -> Result<(), TaskStoreError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let operator_id = local_operator_id(&transaction)?;
        transaction.execute(
            "INSERT INTO operator_attention_watermarks (operator_id, seen_at)
             VALUES (?1, ?2)
             ON CONFLICT(operator_id) DO UPDATE SET seen_at = excluded.seen_at
             WHERE excluded.seen_at > operator_attention_watermarks.seen_at",
            params![operator_id.to_string(), now],
        )?;
        let live: Vec<String> = crate::attention::needs_you_subjects(&transaction, now)?
            .into_iter()
            .map(|subject| subject.subject_key)
            .collect();
        transaction.execute(
            "DELETE FROM notification_deliveries
             WHERE kind <> 'test'
               AND subject_key IN (SELECT value FROM json_each(?1))",
            params![serde_json::to_string(&live).unwrap_or_else(|_| "[]".to_owned())],
        )?;
        transaction.commit()?;
        Ok(())
    }

    pub fn recover_notification_deliveries(&self, now: i64) -> Result<usize, TaskStoreError> {
        let connection = self.connection()?;
        let removed = connection.execute(
            "DELETE FROM notification_deliveries
             WHERE state = 'dispatching' AND attempts >= ?1",
            [MAX_NOTIFICATION_ATTEMPTS],
        )?;
        let recovered = connection.execute(
            "UPDATE notification_deliveries SET state = 'queued', available_at = ?1
             WHERE state = 'dispatching'",
            [now],
        )?;
        Ok(removed.saturating_add(recovered))
    }

    /// Claims one bounded eligible batch after rechecking policy, presence, and decision state.
    ///
    /// # Errors
    /// Returns persistence or integrity failures.
    pub fn claim_notification_deliveries(
        &self,
        now: i64,
    ) -> Result<Vec<NotificationDispatch>, TaskStoreError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        cancel_ineligible_deliveries(&transaction, now)?;
        let deliveries = {
            let mut statement = transaction.prepare(
                "SELECT n.id, n.subscription_id, s.endpoint, s.p256dh, s.auth,
                        n.decision_id, n.urgency, n.kind
                 FROM notification_deliveries n
                 JOIN notification_subscriptions s
                   ON s.device_id = n.subscription_id AND s.operator_id = n.operator_id
                 WHERE n.state = 'queued' AND n.available_at <= ?1
                 ORDER BY n.urgency = 'time_sensitive' DESC, n.created_at, n.id
                 LIMIT ?2",
            )?;
            let rows = statement.query_map(params![now, MAX_NOTIFICATION_CLAIMS], |row| {
                let subscription_id = PresenceDeviceId::from_str(&row.get::<_, String>(1)?)
                    .map_err(|_| rusqlite::Error::InvalidQuery)?;
                let decision_id = row
                    .get::<_, Option<String>>(5)?
                    .map(|value| DecisionRequestId::from_str(&value))
                    .transpose()
                    .map_err(|_| rusqlite::Error::InvalidQuery)?;
                let urgency = DecisionUrgency::from_str(&row.get::<_, String>(6)?)
                    .map_err(|_| rusqlite::Error::InvalidQuery)?;
                Ok(NotificationDispatch {
                    delivery_id: row.get(0)?,
                    subscription_id,
                    endpoint: row.get(2)?,
                    p256dh: row.get(3)?,
                    auth: row.get(4)?,
                    decision_id,
                    urgency,
                    test: row.get::<_, String>(7)? == "test",
                })
            })?;
            rows.collect::<Result<Vec<_>, _>>()?
        };
        for delivery in &deliveries {
            transaction.execute(
                "UPDATE notification_deliveries
                 SET state = 'dispatching', attempts = attempts + 1
                 WHERE id = ?1 AND state = 'queued'",
                [delivery.delivery_id],
            )?;
        }
        transaction.commit()?;
        Ok(deliveries)
    }

    /// Completes one claimed delivery and clears prior endpoint failures.
    ///
    /// # Errors
    /// Returns persistence failures.
    pub fn complete_notification_delivery(
        &self,
        delivery_id: i64,
        subscription_id: PresenceDeviceId,
    ) -> Result<bool, TaskStoreError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let removed = transaction.execute(
            "DELETE FROM notification_deliveries WHERE id = ?1 AND state = 'dispatching'",
            [delivery_id],
        )?;
        if removed == 1 {
            transaction.execute(
                "UPDATE notification_subscriptions SET failure_count = 0
                 WHERE device_id = ?1",
                [subscription_id.to_string()],
            )?;
        }
        transaction.commit()?;
        Ok(removed == 1)
    }

    /// Applies bounded retry or removes an invalid endpoint.
    ///
    /// # Errors
    /// Returns persistence failures.
    pub fn fail_notification_delivery(
        &self,
        delivery_id: i64,
        subscription_id: PresenceDeviceId,
        failure: NotificationDeliveryFailure,
        now: i64,
    ) -> Result<bool, TaskStoreError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let changed = match failure {
            NotificationDeliveryFailure::Gone => {
                let removed = transaction.execute(
                    "DELETE FROM notification_subscriptions WHERE device_id = ?1",
                    [subscription_id.to_string()],
                )?;
                if removed > 0 {
                    insert_control_room_event(
                        &transaction,
                        ControlRoomEventKind::NotificationsChanged,
                    )?;
                }
                removed
            }
            NotificationDeliveryFailure::Permanent => {
                transaction.execute(
                    "UPDATE notification_subscriptions
                     SET failure_count = failure_count + 1 WHERE device_id = ?1",
                    [subscription_id.to_string()],
                )?;
                transaction.execute(
                    "DELETE FROM notification_deliveries WHERE id = ?1",
                    [delivery_id],
                )?
            }
            NotificationDeliveryFailure::Retryable => {
                let attempts: Option<i64> = transaction
                    .query_row(
                        "SELECT attempts FROM notification_deliveries
                         WHERE id = ?1 AND state = 'dispatching'",
                        [delivery_id],
                        |row| row.get(0),
                    )
                    .optional()?;
                match attempts {
                    Some(value) if value >= MAX_NOTIFICATION_ATTEMPTS => transaction.execute(
                        "DELETE FROM notification_deliveries WHERE id = ?1",
                        [delivery_id],
                    )?,
                    Some(value) => transaction.execute(
                        "UPDATE notification_deliveries
                         SET state = 'queued', available_at = ?2
                         WHERE id = ?1 AND state = 'dispatching'",
                        params![delivery_id, now.saturating_add(retry_delay(value))],
                    )?,
                    None => 0,
                }
            }
        };
        transaction.commit()?;
        Ok(changed > 0)
    }
}

pub(super) fn enqueue_decision_notifications(
    transaction: &Transaction<'_>,
    decision_id: DecisionRequestId,
    urgency: DecisionUrgency,
    now: i64,
) -> Result<usize, TaskStoreError> {
    let operator_id = local_operator_id(transaction)?;
    let policy = notification_policy(transaction, operator_id)?;
    let presence = operator_presence_from_connection(transaction, now)?;
    if !policy.allows(urgency, presence.mode) {
        return Ok(0);
    }
    let available = available_delivery_slots(transaction)?;
    if available <= 0 {
        return Ok(0);
    }
    transaction
        .execute(
            "INSERT OR IGNORE INTO notification_deliveries (
                 operator_id, subscription_id, decision_id, urgency, kind,
                 state, attempts, available_at, created_at
             )
             SELECT operator_id, device_id, ?2, ?3, 'decision',
                    'queued', 0, ?4, ?4
             FROM notification_subscriptions
             WHERE operator_id = ?1
             ORDER BY updated_at DESC
             LIMIT ?5",
            params![
                operator_id.to_string(),
                decision_id.to_string(),
                urgency.to_string(),
                now,
                available,
            ],
        )
        .map_err(Into::into)
}

/// Queues a push for everything waiting on the operator that they have not
/// already seen.
///
/// PARITY IS THE RULE, and it is the operator's: "If it makes it to Needs You
/// then I should see it." So this no longer selects decisions and ignores the
/// other four sources — it walks the same queue the control room draws.
///
/// WHAT KEEPS IT QUIET is no longer WHICH sources are eligible, because they
/// all are. It is the watermark: only work that arrived since they last looked
/// at the queue can fire. Without that, turning on parity would notify for
/// every item already sitting on the board the moment they stepped away, which
/// is precisely the "trains you to ignore alerts" failure that is harder to
/// reverse than silence.
pub(super) fn enqueue_pending_notifications(
    transaction: &Transaction<'_>,
    now: i64,
) -> Result<usize, TaskStoreError> {
    let operator_id = local_operator_id(transaction)?;
    let policy = notification_policy(transaction, operator_id)?;
    let presence = operator_presence_from_connection(transaction, now)?;
    if presence.mode == PresenceMode::AtHive || policy == NotificationPolicy::Off {
        return Ok(0);
    }
    let mut available = available_delivery_slots(transaction)?;
    if available <= 0 {
        return Ok(0);
    }
    let seen_at = attention_watermark(transaction, operator_id)?;
    let mut queued = 0;
    for subject in crate::attention::needs_you_subjects(transaction, now)? {
        if available <= 0 {
            break;
        }
        // Already on the board when they last looked, so it is not news.
        if subject.created_at <= seen_at {
            continue;
        }
        if !policy.allows(urgency_of(&subject.urgency), presence.mode) {
            continue;
        }
        let inserted = transaction.execute(
            "INSERT OR IGNORE INTO notification_deliveries (
                 operator_id, subscription_id, decision_id, subject_key, urgency,
                 kind, state, attempts, available_at, created_at
             )
             SELECT operator_id, device_id, ?2, ?3, ?4, ?5, 'queued', 0, ?6, ?6
             FROM notification_subscriptions
             WHERE operator_id = ?1
             ORDER BY updated_at DESC
             LIMIT ?7",
            params![
                operator_id.to_string(),
                subject.decision_id(),
                subject.subject_key,
                subject.urgency,
                subject.kind,
                now,
                available,
            ],
        )?;
        available -= i64::try_from(inserted).unwrap_or(0);
        queued += inserted;
    }
    Ok(queued)
}

/// When the operator last looked at Needs you, or the beginning of time.
///
/// A Hive that has never recorded a look treats everything as news, which is
/// the safe direction: the failure it avoids is silence about work nobody has
/// seen.
fn attention_watermark(
    transaction: &Transaction<'_>,
    operator_id: OperatorId,
) -> Result<i64, TaskStoreError> {
    attention_watermark_from(transaction, operator_id)
}

fn attention_watermark_from(
    connection: &Connection,
    operator_id: OperatorId,
) -> Result<i64, TaskStoreError> {
    Ok(connection
        .query_row(
            "SELECT seen_at FROM operator_attention_watermarks WHERE operator_id = ?1",
            params![operator_id.to_string()],
            |row| row.get(0),
        )
        .optional()?
        .unwrap_or(0))
}

/// Only decisions carry an urgency of their own; everything else is normal.
fn urgency_of(urgency: &str) -> DecisionUrgency {
    if urgency == "time_sensitive" {
        DecisionUrgency::TimeSensitive
    } else {
        DecisionUrgency::Normal
    }
}

/// Withdraws queued pushes that no longer describe anything waiting.
///
/// A notification for work that has since been handled is worse than none: it
/// sends the operator to a queue that has already moved on, and doing that
/// twice teaches them the buzz is not worth walking back for.
///
/// Now keyed on the SUBJECT rather than on a decision id, so it covers all five
/// sources. The old version could only retract decision deliveries — for the
/// other four there was nothing to retract, because nothing could be queued.
fn cancel_ineligible_deliveries(
    transaction: &Transaction<'_>,
    now: i64,
) -> Result<(), TaskStoreError> {
    let operator_id = local_operator_id(transaction)?;
    let policy = notification_policy(transaction, operator_id)?;
    let presence = operator_presence_from_connection(transaction, now)?;
    // Test deliveries are the operator proving their own device works, so they
    // answer to the policy but not to the queue.
    if presence.mode == PresenceMode::AtHive || policy == NotificationPolicy::Off {
        transaction.execute(
            "DELETE FROM notification_deliveries WHERE kind <> 'test'",
            [],
        )?;
        return Ok(());
    }
    let live: Vec<String> = crate::attention::needs_you_subjects(transaction, now)?
        .into_iter()
        .map(|subject| subject.subject_key)
        .collect();
    let mut statement = transaction.prepare(
        "DELETE FROM notification_deliveries
         WHERE kind <> 'test'
           AND (
               subject_key NOT IN (SELECT value FROM json_each(?1))
               OR (?2 = 'important_only' AND urgency <> 'time_sensitive'
                   AND kind = 'decision')
           )",
    )?;
    statement.execute(params![
        serde_json::to_string(&live).unwrap_or_else(|_| "[]".to_owned()),
        policy.to_string(),
    ])?;
    Ok(())
}

fn notification_settings_from_connection(
    connection: &Connection,
) -> Result<NotificationSettings, TaskStoreError> {
    let operator_id = local_operator_id(connection)?;
    let policy = notification_policy(connection, operator_id)?;
    let subscription_count = connection.query_row(
        "SELECT COUNT(*) FROM notification_subscriptions WHERE operator_id = ?1",
        [operator_id.to_string()],
        |row| row.get(0),
    )?;
    Ok(NotificationSettings {
        policy,
        subscription_count,
    })
}

fn notification_policy(
    connection: &Connection,
    operator_id: OperatorId,
) -> Result<NotificationPolicy, TaskStoreError> {
    connection
        .query_row(
            "SELECT policy FROM notification_preferences WHERE operator_id = ?1",
            [operator_id.to_string()],
            |row| row.get::<_, String>(0),
        )
        .optional()?
        .map_or(Ok(NotificationPolicy::ImportantOnly), |value| {
            NotificationPolicy::from_str(&value)
                .map_err(|_| TaskStoreError::IntegrityFailure("invalid notification policy".into()))
        })
}

fn validate_subscription(input: &PushSubscriptionInput) -> Result<(), TaskStoreError> {
    if input.endpoint.is_empty()
        || input.endpoint.len() > MAX_ENDPOINT_BYTES
        || input.p256dh.len() != P256DH_BYTES
        || input.auth.len() != AUTH_BYTES
    {
        return Err(TaskStoreError::InvalidNotificationSubscription);
    }
    Ok(())
}

fn available_delivery_slots(transaction: &Transaction<'_>) -> Result<i64, TaskStoreError> {
    let current: i64 =
        transaction.query_row("SELECT COUNT(*) FROM notification_deliveries", [], |row| {
            row.get(0)
        })?;
    Ok(MAX_NOTIFICATION_DELIVERIES.saturating_sub(current))
}

fn retry_delay(attempts: i64) -> i64 {
    match attempts {
        0 | 1 => 30,
        2 => 120,
        3 => 600,
        _ => 1_800,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::NewDecisionRequest;
    use swarm_domain::{DecisionRequestKind, PresenceObservationState};

    fn subscription(device_id: PresenceDeviceId, endpoint: &str) -> PushSubscriptionInput {
        PushSubscriptionInput {
            device_id,
            device_class: PresenceDeviceClass::Mobile,
            endpoint: endpoint.to_owned(),
            p256dh: vec![7; P256DH_BYTES],
            auth: vec![9; AUTH_BYTES],
        }
    }

    fn decision(
        worker_id: swarm_domain::WorkerId,
        urgency: DecisionUrgency,
        actions: &[String],
    ) -> NewDecisionRequest<'_> {
        NewDecisionRequest {
            requesting_worker_id: worker_id,
            task_id: None,
            kind: DecisionRequestKind::Input,
            urgency,
            title: "Need a decision",
            summary: "Whether to proceed, and what it costs if we do not.",
            reason: "A bounded reason",
            risk: "",
            evidence: "",
            suggested_action: "Choose",
            allowed_actions: actions,
            questions: &[],
            deadline: None,
        }
    }

    /// A completed task whose sender was never answered.
    ///
    /// Written as rows rather than driven through the lifecycle because what is
    /// under test is the notification path, and the shape that reaches it is
    /// "completed, linked to a message, with no delivered reply" — the same
    /// predicate `completed_email_tasks_awaiting_a_reply` selects on.
    fn seed_unanswered_email(store: &TaskStore, received_at: i64) -> uuid::Uuid {
        let task = uuid::Uuid::now_v7();
        let connection = store.connection().unwrap();
        // A task belongs to this Hive; a trigger enforces it.
        let hive: String = connection
            .query_row(
                "SELECT hive_id FROM local_hive_identity WHERE singleton = 1",
                [],
                |row| row.get(0),
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO tasks (id, title, workspace, state, hive_id, created_at, updated_at)
                 VALUES (?1, 'Confirm email address', '/workspace', 'completed', ?3, ?2, ?2)",
                params![task.to_string(), received_at, hive],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO email_message_links (
                     id, task_id, integration_id, message_id, conversation_id,
                     sender_name, sender_address, received_at, web_url
                 ) VALUES (?1, ?2, 'operator-outlook', ?1, 'thread', 'A Sender',
                           'sender@example.org', ?3, 'https://example.org/message')",
                params![task.to_string(), task.to_string(), received_at],
            )
            .unwrap();
        drop(connection);
        task
    }

    /// A source that is not a decision can now reach the operator's phone.
    ///
    /// This is the whole point of the widening, and it is the operator's own
    /// ruling: "If it makes it to Needs You then I should see it." Before this,
    /// an email nobody had answered could sit on the queue indefinitely while
    /// push stayed silent, because the schema could not represent it and the
    /// enqueue never looked. One did sit, for eleven days.
    #[test]
    fn an_unanswered_email_reaches_the_phone_now_that_decisions_are_not_the_only_source() {
        let store = TaskStore::in_memory().unwrap();
        let device = PresenceDeviceId::new();
        store
            .save_notification_subscription(
                &subscription(device, "https://fcm.googleapis.com/fcm/send/email"),
                10,
            )
            .unwrap();
        store
            .set_notification_policy(NotificationPolicy::AllDecisions, 11)
            .unwrap();
        store
            .set_manual_presence(Some(PresenceMode::Away), 12)
            .unwrap();
        let task = seed_unanswered_email(&store, 100);
        // The tick, which is the only thing that catches work arriving after
        // they had already gone.
        store.sweep_attention_notifications(150).unwrap();

        let queued = store.claim_notification_deliveries(200).unwrap();

        assert_eq!(queued.len(), 1, "the unanswered email must notify");
        // Addressed by its own subject rather than by a decision it does not have.
        assert!(queued[0].decision_id.is_none());
        let _ = task;
    }

    /// Work already on the board when they last looked does not buzz.
    ///
    /// With every source eligible, source selection is no longer what keeps the
    /// phone quiet — this is. Without it, turning on parity would fire for the
    /// entire standing queue the moment the operator stepped away, and an alert
    /// on every board change teaches them to ignore alerts. That failure is
    /// harder to reverse than silence.
    #[test]
    fn only_work_newer_than_the_last_look_is_worth_waking_someone_for() {
        let store = TaskStore::in_memory().unwrap();
        let device = PresenceDeviceId::new();
        store
            .save_notification_subscription(
                &subscription(device, "https://fcm.googleapis.com/fcm/send/seen"),
                10,
            )
            .unwrap();
        store
            .set_notification_policy(NotificationPolicy::AllDecisions, 11)
            .unwrap();
        seed_unanswered_email(&store, 100);

        // They read the queue at the Hive, then walked away.
        store.record_attention_seen(150).unwrap();
        store
            .set_manual_presence(Some(PresenceMode::Away), 160)
            .unwrap();
        // Swept explicitly, so this cannot pass merely because nothing ran.
        assert_eq!(store.sweep_attention_notifications(170).unwrap(), 0);

        assert!(
            store.claim_notification_deliveries(200).unwrap().is_empty(),
            "work they already read must not follow them out of the building"
        );
    }

    /// A notification for work that has since been handled is withdrawn.
    ///
    /// Sending someone back to a queue that has already moved on, twice,
    /// teaches them the buzz is not worth walking back for.
    ///
    /// Counts the QUEUED ROW rather than claiming twice. The first draft of
    /// this test claimed, handled the work, then claimed again and asserted
    /// empty — and it passed with withdrawal disabled, because claiming moves a
    /// row to 'dispatching' and the second claim only ever looks at 'queued'.
    /// It proved nothing. The ablation caught that, which is the entire reason
    /// for running one.
    #[test]
    fn a_queued_push_is_withdrawn_once_its_subject_is_handled() {
        let store = TaskStore::in_memory().unwrap();
        let device = PresenceDeviceId::new();
        store
            .save_notification_subscription(
                &subscription(device, "https://fcm.googleapis.com/fcm/send/gone"),
                10,
            )
            .unwrap();
        store
            .set_notification_policy(NotificationPolicy::AllDecisions, 11)
            .unwrap();
        store
            .set_manual_presence(Some(PresenceMode::Away), 12)
            .unwrap();
        let task = seed_unanswered_email(&store, 100);
        store.sweep_attention_notifications(150).unwrap();
        assert_eq!(queued_rows(&store), 1, "the email should be queued first");

        // The reply goes out, so the item leaves Needs you. A reply needs
        // settled evidence first — the trigger enforces it — so this records a
        // deployment the way finishing the work actually would.
        let connection = store.connection().unwrap();
        let operator: String = connection
            .query_row("SELECT id FROM operators LIMIT 1", [], |row| row.get(0))
            .unwrap();
        connection
            .execute(
                "INSERT INTO task_deployments
                     (id, task_id, environment, reference, deployed_at,
                      approved_by_operator_id, recorded_at)
                 VALUES (?1, ?1, 'production', 'abc123', ?2, ?3, ?2)",
                params![task.to_string(), 155, operator],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO email_reply_deliveries
                     (id, task_id, body, state, idempotency_key, delivered_at,
                      provider_reply_id, available_at, created_at, updated_at)
                 VALUES (?1, ?1, 'Answered.', 'delivered', ?1, ?2, 'reply-1', ?2, ?2, ?2)",
                params![task.to_string(), 160],
            )
            .unwrap();
        drop(connection);
        store.sweep_attention_notifications(170).unwrap();

        assert_eq!(
            queued_rows(&store),
            0,
            "a push for handled work must be retracted rather than delivered"
        );
    }

    /// Deliveries still sitting in the table, claimed or not.
    fn queued_rows(store: &TaskStore) -> i64 {
        store
            .connection()
            .unwrap()
            .query_row(
                "SELECT COUNT(*) FROM notification_deliveries WHERE kind <> 'test'",
                [],
                |row| row.get(0),
            )
            .unwrap()
    }

    #[test]
    fn important_decisions_queue_only_while_operator_is_away() {
        let store = TaskStore::in_memory().unwrap();
        let queen = store.ensure_queen("/queen").unwrap();
        let device = PresenceDeviceId::new();
        store
            .save_notification_subscription(
                &subscription(device, "https://fcm.googleapis.com/fcm/send/test"),
                10,
            )
            .unwrap();
        store
            .record_presence_observation(
                device,
                PresenceDeviceClass::Desktop,
                PresenceObservationState::Active,
                20,
            )
            .unwrap();
        let actions = vec!["Proceed".to_owned()];
        store
            .create_decision_request(&decision(
                queen.id,
                DecisionUrgency::TimeSensitive,
                &actions,
            ))
            .unwrap();
        assert!(store.claim_notification_deliveries(21).unwrap().is_empty());

        store
            .set_manual_presence(Some(PresenceMode::Away), 22)
            .unwrap();
        let queued = store.claim_notification_deliveries(22).unwrap();
        assert_eq!(queued.len(), 1);
        assert!(queued[0].decision_id.is_some());
    }

    #[test]
    fn policy_and_queue_are_bounded_and_test_delivery_is_explicit() {
        let store = TaskStore::in_memory().unwrap();
        for index in 0..MAX_NOTIFICATION_SUBSCRIPTIONS {
            store
                .save_notification_subscription(
                    &subscription(
                        PresenceDeviceId::new(),
                        &format!("https://fcm.googleapis.com/fcm/send/{index}"),
                    ),
                    index,
                )
                .unwrap();
        }
        assert!(matches!(
            store.save_notification_subscription(
                &subscription(
                    PresenceDeviceId::new(),
                    "https://fcm.googleapis.com/fcm/send/overflow",
                ),
                99,
            ),
            Err(TaskStoreError::NotificationSubscriptionLimit)
        ));
        assert_eq!(
            store.enqueue_test_notifications(100).unwrap(),
            MAX_NOTIFICATION_SUBSCRIPTIONS
        );
        let claimed = store.claim_notification_deliveries(100).unwrap();
        assert_eq!(
            claimed.len(),
            usize::try_from(MAX_NOTIFICATION_CLAIMS).unwrap()
        );
        assert!(claimed.iter().all(|delivery| delivery.test));
    }

    #[test]
    fn explicit_test_targets_only_the_selected_device() {
        let store = TaskStore::in_memory().unwrap();
        let selected = PresenceDeviceId::new();
        let other = PresenceDeviceId::new();
        store
            .save_notification_subscription(
                &subscription(selected, "https://fcm.googleapis.com/fcm/send/selected"),
                1,
            )
            .unwrap();
        store
            .save_notification_subscription(
                &subscription(other, "https://fcm.googleapis.com/fcm/send/other"),
                2,
            )
            .unwrap();

        assert!(store.enqueue_device_test_notification(selected, 3).unwrap());
        let claimed = store.claim_notification_deliveries(3).unwrap();
        assert_eq!(claimed.len(), 1);
        assert_eq!(claimed[0].subscription_id, selected);
        assert!(
            !store
                .enqueue_device_test_notification(PresenceDeviceId::new(), 4)
                .unwrap()
        );
    }

    #[test]
    fn gone_endpoint_removes_subscription_and_cascades_queued_work() {
        let store = TaskStore::in_memory().unwrap();
        let device = PresenceDeviceId::new();
        store
            .save_notification_subscription(
                &subscription(device, "https://fcm.googleapis.com/fcm/send/gone"),
                10,
            )
            .unwrap();
        store.enqueue_test_notifications(11).unwrap();
        let dispatch = store.claim_notification_deliveries(11).unwrap().remove(0);
        assert!(
            store
                .fail_notification_delivery(
                    dispatch.delivery_id,
                    device,
                    NotificationDeliveryFailure::Gone,
                    12,
                )
                .unwrap()
        );
        assert_eq!(store.notification_settings().unwrap().subscription_count, 0);
        assert!(!store.has_notification_subscription(device).unwrap());
        assert!(store.claim_notification_deliveries(12).unwrap().is_empty());
    }

    #[test]
    fn device_registration_status_never_exposes_subscription_material() {
        let store = TaskStore::in_memory().unwrap();
        let registered = PresenceDeviceId::new();
        let absent = PresenceDeviceId::new();
        store
            .save_notification_subscription(
                &subscription(registered, "https://fcm.googleapis.com/fcm/send/status"),
                10,
            )
            .unwrap();

        assert!(store.has_notification_subscription(registered).unwrap());
        assert!(!store.has_notification_subscription(absent).unwrap());
    }

    #[test]
    fn retry_is_bounded_and_recovered_after_api_restart() {
        let store = TaskStore::in_memory().unwrap();
        let device = PresenceDeviceId::new();
        store
            .save_notification_subscription(
                &subscription(device, "https://fcm.googleapis.com/fcm/send/retry"),
                10,
            )
            .unwrap();
        store.enqueue_test_notifications(11).unwrap();
        let first = store.claim_notification_deliveries(11).unwrap().remove(0);
        assert_eq!(store.recover_notification_deliveries(12).unwrap(), 1);
        let recovered = store.claim_notification_deliveries(12).unwrap().remove(0);
        assert_eq!(first.delivery_id, recovered.delivery_id);
        store
            .fail_notification_delivery(
                recovered.delivery_id,
                device,
                NotificationDeliveryFailure::Retryable,
                12,
            )
            .unwrap();
        assert!(store.claim_notification_deliveries(131).unwrap().is_empty());
        assert_eq!(store.claim_notification_deliveries(132).unwrap().len(), 1);
    }
}

/// Widens notification deliveries past decisions, and remembers when the
/// operator last looked at the queue.
///
/// THE OLD SHAPE COULD ONLY EVER CARRY DECISIONS, by construction:
///
///   kind TEXT NOT NULL CHECK (kind IN ('decision','test'))
///   CHECK ((kind = 'decision' AND decision_id IS NOT NULL)
///          OR (kind = 'test' AND decision_id IS NULL))
///   UNIQUE(decision_id, subscription_id)
///
/// Every non-test row had to name a decision, and the deduplication key was a
/// decision id, so there was nowhere to hang the other four sources of Needs
/// you and no way to tell two of them apart if there had been. The operator's
/// ruling is parity — "If it makes it to Needs You then I should see it" — so
/// `subject_key` replaces `decision_id` as the identity of a delivery, and
/// `decision_id` stays only for its cascade.
///
/// SQLite cannot alter a CHECK, so this is a rebuild. Existing rows keep their
/// meaning: a queued decision delivery is still a queued decision delivery,
/// addressed by the key it would be given today.
///
/// Guarded on the table existing and on the column being absent, because the
/// migration tests rewind `user_version` WITHOUT rewinding the tables — they
/// model a database restored from a backup or half-upgraded, and a migration
/// that assumes the old shape is still there fails on exactly that.
pub(super) fn migrate_attention_notifications(
    transaction: &rusqlite::Transaction<'_>,
) -> rusqlite::Result<()> {
    let widened: bool = transaction.query_row(
        "SELECT EXISTS(
             SELECT 1 FROM pragma_table_info('notification_deliveries')
             WHERE name = 'subject_key'
         )",
        [],
        |row| row.get(0),
    )?;
    let present: bool = transaction.query_row(
        "SELECT EXISTS(
             SELECT 1 FROM sqlite_master
             WHERE type = 'table' AND name = 'notification_deliveries'
         )",
        [],
        |row| row.get(0),
    )?;
    if present && !widened {
        transaction.execute_batch(
            "CREATE TABLE notification_deliveries_widened (
                 id INTEGER PRIMARY KEY AUTOINCREMENT,
                 operator_id TEXT NOT NULL REFERENCES operators(id) ON DELETE CASCADE,
                 subscription_id TEXT NOT NULL
                     REFERENCES notification_subscriptions(device_id) ON DELETE CASCADE,
                 decision_id TEXT REFERENCES decision_requests(id) ON DELETE CASCADE,
                 subject_key TEXT NOT NULL,
                 urgency TEXT NOT NULL CHECK (urgency IN ('normal','time_sensitive')),
                 kind TEXT NOT NULL CHECK (kind IN (
                     'decision','assist','queen_automation','held_delivery',
                     'email_reply','test'
                 )),
                 state TEXT NOT NULL CHECK (state IN ('queued','dispatching')),
                 attempts INTEGER NOT NULL DEFAULT 0 CHECK (attempts BETWEEN 0 AND 5),
                 available_at INTEGER NOT NULL,
                 created_at INTEGER NOT NULL,
                 CHECK ((kind = 'decision' AND decision_id IS NOT NULL)
                        OR (kind <> 'decision' AND decision_id IS NULL)),
                 UNIQUE(subject_key, subscription_id)
             );
             INSERT INTO notification_deliveries_widened (
                 id, operator_id, subscription_id, decision_id, subject_key,
                 urgency, kind, state, attempts, available_at, created_at
             )
             SELECT id, operator_id, subscription_id, decision_id,
                    CASE WHEN kind = 'decision' THEN 'decision:' || decision_id
                         ELSE 'test:' || id END,
                    urgency, kind, state, attempts, available_at, created_at
             FROM notification_deliveries;
             DROP TABLE notification_deliveries;
             ALTER TABLE notification_deliveries_widened
                 RENAME TO notification_deliveries;",
        )?;
    }
    // WHEN THE OPERATOR LAST LOOKED, per operator rather than per device.
    //
    // It cannot be per-tab or per-browser state: they use more than one window,
    // and a watermark that resets per tab would re-notify for work they read an
    // hour ago on the other screen.
    //
    // This is the whole defence against shouting. Under parity every source is
    // eligible, so what keeps the phone quiet is no longer "which sources" but
    // "newer than the last time they looked at the queue".
    transaction.execute_batch(
        "CREATE TABLE IF NOT EXISTS operator_attention_watermarks (
             operator_id TEXT PRIMARY KEY REFERENCES operators(id) ON DELETE CASCADE,
             seen_at INTEGER NOT NULL
         );
         PRAGMA user_version = 94;",
    )
}
