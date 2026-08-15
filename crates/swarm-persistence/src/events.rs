use std::str::FromStr;

use rusqlite::params;
use swarm_domain::{ControlRoomEvent, ControlRoomEventKind, ControlRoomEventPage, HiveId};

use super::{TaskStore, TaskStoreError, parse_domain_id};

pub(super) const MAX_CONTROL_ROOM_EVENTS: i64 = 4096;
pub(super) const MAX_CONTROL_ROOM_EVENT_PAGE: usize = 128;

impl TaskStore {
    /// Appends one content-free invalidation event and enforces the durable event bound.
    ///
    /// # Errors
    /// Returns an error when the event cannot be committed atomically.
    pub fn record_control_room_event(
        &self,
        kind: ControlRoomEventKind,
    ) -> Result<ControlRoomEvent, TaskStoreError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let event = insert_control_room_event(&transaction, kind)?;
        transaction.commit()?;
        Ok(event)
    }

    /// Reads a bounded resumable page of content-free control-room invalidations.
    ///
    /// A cursor from an evicted or replaced database requests a full snapshot reset.
    ///
    /// # Errors
    /// Returns an error when the event page cannot be read or decoded.
    pub fn list_control_room_events(
        &self,
        after: i64,
    ) -> Result<ControlRoomEventPage, TaskStoreError> {
        let identity = self.local_hive_identity()?;
        let connection = self.connection()?;
        let (earliest, latest) = connection.query_row(
            "SELECT MIN(sequence), MAX(sequence)
             FROM control_room_events WHERE hive_id = ?1",
            [identity.hive.id.to_string()],
            |row| Ok((row.get::<_, Option<i64>>(0)?, row.get::<_, Option<i64>>(1)?)),
        )?;
        let reset_required = after != 0
            && match (earliest, latest) {
                (Some(first), Some(last)) => after < first.saturating_sub(1) || after > last,
                _ => true,
            };
        let cursor = if reset_required { 0 } else { after.max(0) };
        let page_limit = i64::try_from(MAX_CONTROL_ROOM_EVENT_PAGE)
            .map_err(|_| TaskStoreError::Sql(rusqlite::Error::InvalidQuery))?;
        let mut statement = connection.prepare(
            "SELECT sequence, hive_id, kind, occurred_at
             FROM control_room_events
             WHERE hive_id = ?1 AND sequence > ?2
             ORDER BY sequence ASC LIMIT ?3",
        )?;
        let events = statement
            .query_map(
                params![identity.hive.id.to_string(), cursor, page_limit],
                control_room_event_from_row,
            )?
            .collect::<Result<Vec<_>, _>>()?;
        let next_cursor = events.last().map_or(cursor, |event| event.sequence);
        Ok(ControlRoomEventPage {
            events,
            next_cursor,
            reset_required,
        })
    }
}

pub(super) fn insert_control_room_event(
    transaction: &rusqlite::Transaction<'_>,
    kind: ControlRoomEventKind,
) -> rusqlite::Result<ControlRoomEvent> {
    transaction.execute(
        "INSERT INTO control_room_events (hive_id, kind)
         SELECT hive_id, ?1 FROM local_hive_identity WHERE singleton = 1",
        [kind.to_string()],
    )?;
    let sequence = transaction.last_insert_rowid();
    transaction.execute(
        "DELETE FROM control_room_events
         WHERE sequence <= (SELECT MAX(sequence) - ?1 FROM control_room_events)",
        [MAX_CONTROL_ROOM_EVENTS],
    )?;
    transaction.query_row(
        "SELECT sequence, hive_id, kind, occurred_at
         FROM control_room_events WHERE sequence = ?1",
        [sequence],
        control_room_event_from_row,
    )
}

fn control_room_event_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ControlRoomEvent> {
    let kind = ControlRoomEventKind::from_str(&row.get::<_, String>(2)?)
        .map_err(|_| rusqlite::Error::InvalidQuery)?;
    Ok(ControlRoomEvent {
        sequence: row.get(0)?,
        hive_id: parse_domain_id::<HiveId>(&row.get::<_, String>(1)?)?,
        kind,
        occurred_at: row.get(3)?,
    })
}
