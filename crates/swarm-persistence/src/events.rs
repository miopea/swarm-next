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
        let page_limit = i64::try_from(MAX_CONTROL_ROOM_EVENT_PAGE)
            .map_err(|_| TaskStoreError::Sql(rusqlite::Error::InvalidQuery))?;
        // A CLIENT STARTING FRESH IS CAUGHT UP, NOT BEHIND.
        //
        // `after = 0` used to mean "replay everything", so a page load walked
        // the whole retained history 128 events at a time. Measured on the
        // operator's Hive: 4,096 retained events, THIRTY-TWO round trips before
        // the feed reached live, every one of them triggering a control-room
        // refresh and its conditional refetches — and the client keeps the last
        // SIXTEEN for display and throws the rest away.
        //
        // That is what they were feeling: "the app seems to die and reconnect
        // when I am not looking at it. Which turns into performance issues too,
        // I have to refresh to get it to load." A phone that suspends and
        // resumes pays that toll again on every return.
        //
        // A fresh client has just loaded full control-room state by other
        // means, so history tells it nothing it does not already have. It gets
        // the most recent page and a cursor at the head: one round trip, the
        // recent-activity list still populated, and no replay.
        let start_from_head = after == 0;
        let cursor = if reset_required || start_from_head {
            0
        } else {
            after.max(0)
        };
        let mut statement = if start_from_head {
            connection.prepare(
                "SELECT sequence, hive_id, kind, occurred_at FROM (
                     SELECT sequence, hive_id, kind, occurred_at
                     FROM control_room_events
                     WHERE hive_id = ?1 AND sequence > ?2
                     ORDER BY sequence DESC LIMIT ?3
                 ) ORDER BY sequence ASC",
            )?
        } else {
            connection.prepare(
                "SELECT sequence, hive_id, kind, occurred_at
                 FROM control_room_events
                 WHERE hive_id = ?1 AND sequence > ?2
                 ORDER BY sequence ASC LIMIT ?3",
            )?
        };
        let events = statement
            .query_map(
                params![identity.hive.id.to_string(), cursor, page_limit],
                control_room_event_from_row,
            )?
            .collect::<Result<Vec<_>, _>>()?;
        // The head, not the end of this page. For a fresh client those are the
        // same value; saying it this way makes it true even if the newest page
        // is short.
        let next_cursor = if start_from_head {
            latest.unwrap_or(0)
        } else {
            events.last().map_or(cursor, |event| event.sequence)
        };
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
#[cfg(test)]
mod tests {
    use super::MAX_CONTROL_ROOM_EVENT_PAGE;
    use crate::TaskStore;

    fn seed(store: &TaskStore, count: usize) {
        let hive = store.local_hive_identity().unwrap().hive.id.to_string();
        let connection = store.connection().unwrap();
        for _ in 0..count {
            connection
                .execute(
                    "INSERT INTO control_room_events (hive_id, kind, occurred_at)
                     VALUES (?1, 'tasks_changed', unixepoch())",
                    [&hive],
                )
                .unwrap();
        }
    }

    /// A page load must not walk the whole history to show sixteen events.
    ///
    /// THE OPERATOR'S PHONE: "the app seems to die and reconnect when I am not
    /// looking at it. Which turns into performance issues too, I have to
    /// refresh to get it to load."
    ///
    /// Measured on their Hive before this changed: 4,096 retained events, 128
    /// to a page, THIRTY-TWO round trips before the feed reached live — each
    /// one triggering a control-room refresh and its conditional refetches, to
    /// populate a list that keeps the last sixteen and discards the rest. A
    /// phone that suspends and resumes pays that toll again on every return.
    #[test]
    fn a_fresh_client_starts_at_the_head_instead_of_replaying_history() {
        let store = TaskStore::in_memory().unwrap();
        seed(&store, 500);

        let fresh = store.list_control_room_events(0).unwrap();

        assert!(fresh.events.len() <= MAX_CONTROL_ROOM_EVENT_PAGE);
        // CAUGHT UP AFTER ONE REQUEST. Before, this cursor was 128 events into
        // a 500-event history and the next four polls fetched the rest.
        let head = fresh.next_cursor;
        assert_eq!(
            store.list_control_room_events(head).unwrap().events.len(),
            0,
            "a fresh client should be live after one request"
        );
        // Still populated: starting empty would trade one defect for another,
        // because the recent-activity list is the thing this feeds.
        assert!(!fresh.events.is_empty());
        // The NEWEST events, not the oldest.
        assert_eq!(fresh.events.last().unwrap().sequence, head);
    }

    /// A client that is genuinely behind still receives what it missed, in
    /// order. The fix must not turn a resumed session into a silent gap.
    #[test]
    fn a_client_with_a_cursor_still_receives_what_it_missed() {
        let store = TaskStore::in_memory().unwrap();
        seed(&store, 5);

        let all = store.list_control_room_events(0).unwrap();
        let third = all.events[2].sequence;

        let behind = store.list_control_room_events(third).unwrap();

        assert_eq!(behind.events.len(), 2, "everything after the cursor");
        assert!(behind.events.iter().all(|event| event.sequence > third));
        assert!(!behind.reset_required);
    }
}
