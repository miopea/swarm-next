//! The drafts GitHub's asks of the operator filed, and whether GitHub still
//! asks (ADR 0170).
//!
//! One row per ask, keyed by GitHub's own link to it. The row outlives the
//! ask's ending on purpose: a review requested again after the operator
//! reviewed is a new ask about the same pull request, and only a remembered
//! ending can tell it apart from a draft Queen chose to close while GitHub
//! went on asking.

use rusqlite::{OptionalExtension as _, params};
use swarm_domain::{
    AskEnded, EndedAskEffect, GithubAsk, GithubAskKind, GithubAskReading, HiveId,
    TaskActivityActor, TaskId, TaskPriority, TaskState, WorkerId, ended_ask_effect,
};

use crate::events::insert_control_room_event;
use crate::{ControlRoomEventKind, TaskStore, TaskStoreError, insert_draft, parse_domain_id};

/// The most standing asks one listing returns. Far above what one person is
/// asked at once; it bounds a read, it is not a quota.
const MAX_STANDING_ASKS: i64 = 500;
/// The longest failure kept.
const MAX_FAILURE_BYTES: usize = 500;

pub(crate) fn migrate_github_asks(
    transaction: &rusqlite::Transaction<'_>,
    version: i64,
) -> rusqlite::Result<()> {
    transaction.execute_batch(
        "CREATE TABLE IF NOT EXISTS github_ask_links (
             url TEXT PRIMARY KEY CHECK (length(url) BETWEEN 1 AND 2048),
             kind TEXT NOT NULL CHECK (kind IN ('review_request','assigned_issue')),
             repository TEXT NOT NULL CHECK (length(repository) BETWEEN 3 AND 140),
             number INTEGER NOT NULL CHECK (number > 0),
             task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
             ended_at INTEGER,
             checked_at INTEGER NOT NULL DEFAULT (unixepoch()),
             created_at INTEGER NOT NULL DEFAULT (unixepoch())
         ) WITHOUT ROWID;
         CREATE INDEX IF NOT EXISTS github_ask_links_standing
             ON github_ask_links(checked_at) WHERE ended_at IS NULL;
         CREATE TABLE IF NOT EXISTS github_ask_reading (
             singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
             last_read_at INTEGER,
             failed_at INTEGER,
             failure TEXT CHECK (failure IS NULL OR length(failure) BETWEEN 1 AND 500)
         );",
    )?;
    transaction.pragma_update(None, "user_version", version)
}

/// One ask to file, with where it was routed by checkout.
pub struct GithubAskFiling<'a> {
    pub ask: &'a GithubAsk,
    pub description: &'a str,
    /// The one matching worker's checkout, or the repository's placeholder.
    pub workspace: &'a str,
    pub repository_named: bool,
    pub recommended_worker: Option<WorkerId>,
}

/// What receiving an ask did.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GithubAskArrival {
    /// A new draft.
    Filed(TaskId),
    /// The whole-repository issue intake had already filed this issue; that
    /// task is the ask's now, rather than a second one.
    Adopted(TaskId),
    /// GitHub asks again about work that is still open. Its history says so.
    AskedAgain(TaskId),
    /// Already linked and standing. Nothing changed.
    Known,
}

/// An ask that had not ended when last read.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StandingGithubAsk {
    pub url: String,
    pub kind: GithubAskKind,
    pub repository: String,
    pub number: u64,
    pub task_id: TaskId,
}

impl TaskStore {
    /// Which of these asks would file or reopen something: those with no
    /// link, and those whose link ended. Routing a filing reads every
    /// worker's checkout, so a pass asks this first and pays for that only
    /// when something is new.
    ///
    /// # Errors
    /// Returns an error when persistence is unavailable.
    pub fn github_asks_not_standing(&self, urls: &[String]) -> Result<Vec<String>, TaskStoreError> {
        let connection = self.connection()?;
        let mut statement = connection
            .prepare_cached("SELECT ended_at IS NULL FROM github_ask_links WHERE url = ?1")?;
        let mut not_standing = Vec::new();
        for url in urls {
            let standing: Option<bool> = statement.query_row([url], |row| row.get(0)).optional()?;
            if standing != Some(true) {
                not_standing.push(url.clone());
            }
        }
        Ok(not_standing)
    }

    /// Receives one ask GitHub makes of the operator, once.
    ///
    /// DRAFTS ONLY, like every other intake: an ask becomes work when Queen
    /// routes it. One transaction holds the task, what intake said about it,
    /// and the link, so no draft exists without the memory that stops it being
    /// filed again on the next pass.
    ///
    /// # Errors
    /// Returns validation or persistence failures.
    pub fn receive_github_ask(
        &self,
        filing: &GithubAskFiling<'_>,
    ) -> Result<GithubAskArrival, TaskStoreError> {
        let url = filing.ask.url();
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let link: Option<(String, Option<i64>)> = transaction
            .query_row(
                "SELECT task_id, ended_at FROM github_ask_links WHERE url = ?1",
                [&url],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        let arrival = match link {
            Some((_, None)) => GithubAskArrival::Known,
            Some((task_id, Some(_))) => {
                let task_id = parse_domain_id::<TaskId>(&task_id)?;
                if task_is_open(&transaction, task_id)? {
                    transaction.execute(
                        "UPDATE github_ask_links SET ended_at = NULL, checked_at = unixepoch()
                         WHERE url = ?1",
                        [&url],
                    )?;
                    transaction.execute(
                        "INSERT INTO task_activity (task_id, kind, note, actor_kind)
                         VALUES (?1, 'noted', ?2, 'system')",
                        params![
                            task_id.to_string(),
                            format!("GitHub asks again: {}.", filing.ask.reference())
                        ],
                    )?;
                    insert_control_room_event(&transaction, ControlRoomEventKind::TasksChanged)?;
                    GithubAskArrival::AskedAgain(task_id)
                } else {
                    let task_id = file_draft(&transaction, filing)?;
                    transaction.execute(
                        "UPDATE github_ask_links
                         SET task_id = ?2, ended_at = NULL, checked_at = unixepoch()
                         WHERE url = ?1",
                        params![url, task_id.to_string()],
                    )?;
                    GithubAskArrival::Filed(task_id)
                }
            }
            None => {
                // The whole-repository intake keys on the same link, so an
                // assigned issue on a repository it takes may already be here.
                let filed: Option<String> = transaction
                    .query_row(
                        "SELECT task_id FROM github_issue_tasks WHERE issue_url = ?1",
                        [&url],
                        |row| row.get(0),
                    )
                    .optional()?;
                let arrival = match filed {
                    Some(task_id) => {
                        GithubAskArrival::Adopted(parse_domain_id::<TaskId>(&task_id)?)
                    }
                    None => GithubAskArrival::Filed(file_draft(&transaction, filing)?),
                };
                let (GithubAskArrival::Adopted(task_id) | GithubAskArrival::Filed(task_id)) =
                    arrival
                else {
                    unreachable!("only adoption or filing reaches here");
                };
                transaction.execute(
                    "INSERT INTO github_ask_links (url, kind, repository, number, task_id)
                     VALUES (?1, ?2, ?3, ?4, ?5)",
                    params![
                        url,
                        filing.ask.kind().as_str(),
                        filing.ask.repository(),
                        i64::try_from(filing.ask.number())
                            .map_err(|_| TaskStoreError::InvalidTitle)?,
                        task_id.to_string(),
                    ],
                )?;
                arrival
            }
        };
        transaction.commit()?;
        Ok(arrival)
    }

    /// The asks that had not ended when last read, the longest unchecked
    /// first, so a pass that can confirm only some of them reaches the rest
    /// on the passes after.
    ///
    /// # Errors
    /// Returns an error when persistence is unavailable or a row is corrupt.
    pub fn standing_github_asks(&self) -> Result<Vec<StandingGithubAsk>, TaskStoreError> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT url, kind, repository, number, task_id FROM github_ask_links
             WHERE ended_at IS NULL ORDER BY checked_at, url LIMIT ?1",
        )?;
        let rows = statement.query_map([MAX_STANDING_ASKS], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, String>(4)?,
            ))
        })?;
        let mut standing = Vec::new();
        for row in rows {
            let (url, kind, repository, number, task_id) = row?;
            standing.push(StandingGithubAsk {
                url,
                kind: kind
                    .parse()
                    .map_err(|_| TaskStoreError::Sql(rusqlite::Error::InvalidQuery))?,
                repository,
                number: u64::try_from(number)
                    .map_err(|_| TaskStoreError::Sql(rusqlite::Error::InvalidQuery))?,
                task_id: parse_domain_id(&task_id)?,
            });
        }
        Ok(standing)
    }

    /// A direct read of this ask was made: it still stands, or it could not be
    /// seen and is left as it was. Either way it goes to the back of the line.
    ///
    /// # Errors
    /// Returns an error when persistence is unavailable.
    pub fn record_github_ask_checked(&self, url: &str) -> Result<(), TaskStoreError> {
        self.connection()?.execute(
            "UPDATE github_ask_links SET checked_at = unixepoch() WHERE url = ?1",
            [url],
        )?;
        Ok(())
    }

    /// GitHub no longer asks, and a direct read said why. Closes the draft
    /// nobody routed, tells routed work, and remembers the ending, all in one
    /// transaction.
    ///
    /// Returns what it did, so the pass can say so. An ask already ended, or
    /// never linked, does nothing.
    ///
    /// # Errors
    /// Returns an error when persistence is unavailable.
    pub fn record_github_ask_ended(
        &self,
        url: &str,
        ended: AskEnded,
    ) -> Result<EndedAskEffect, TaskStoreError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let link: Option<(String, String, i64, String, bool)> = transaction
            .query_row(
                "SELECT link.task_id, link.repository, link.number, task.state,
                        task.removed_at IS NOT NULL
                 FROM github_ask_links link JOIN tasks task ON task.id = link.task_id
                 WHERE link.url = ?1 AND link.ended_at IS NULL",
                [url],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                    ))
                },
            )
            .optional()?;
        let Some((task_id, repository, number, state, removed)) = link else {
            return Ok(EndedAskEffect::Nothing);
        };
        let task_id = parse_domain_id::<TaskId>(&task_id)?;
        let state = state
            .parse::<TaskState>()
            .map_err(|_| TaskStoreError::Sql(rusqlite::Error::InvalidQuery))?;
        let note = ended.note(&format!("{repository}#{number}"));
        let effect = if removed {
            EndedAskEffect::Nothing
        } else {
            ended_ask_effect(state)
        };
        match effect {
            EndedAskEffect::Close => {
                // Conditional on still being a draft. The transaction holds the
                // write lock from its start, so the state read above is the
                // state this changes; the condition says so in the statement.
                transaction.execute(
                    "UPDATE tasks SET state = 'abandoned', updated_at = unixepoch()
                     WHERE id = ?1 AND state = 'draft'",
                    [task_id.to_string()],
                )?;
                transaction.execute(
                    "INSERT INTO task_activity (task_id, kind, from_state, to_state, note, actor_kind)
                     VALUES (?1, 'state_changed', 'draft', 'abandoned', ?2, 'system')",
                    params![task_id.to_string(), note],
                )?;
                crate::decisions::withdraw_command_requests_of_closed_task(&transaction, task_id)?;
                insert_control_room_event(&transaction, ControlRoomEventKind::TasksChanged)?;
            }
            EndedAskEffect::Note => {
                transaction.execute(
                    "INSERT INTO task_activity (task_id, kind, note, actor_kind)
                     VALUES (?1, 'noted', ?2, 'system')",
                    params![task_id.to_string(), note],
                )?;
                insert_control_room_event(&transaction, ControlRoomEventKind::TasksChanged)?;
            }
            EndedAskEffect::Nothing => {}
        }
        transaction.execute(
            "UPDATE github_ask_links SET ended_at = unixepoch(), checked_at = unixepoch()
             WHERE url = ?1",
            [url],
        )?;
        transaction.commit()?;
        Ok(effect)
    }

    /// Records how a pass's read of GitHub went: `Ok` when it read GitHub
    /// whole, or why it could not.
    ///
    /// # Errors
    /// Returns an error when persistence is unavailable.
    pub fn record_github_ask_reading(
        &self,
        outcome: Result<(), &str>,
    ) -> Result<(), TaskStoreError> {
        let connection = self.connection()?;
        match outcome {
            Ok(()) => connection.execute(
                "INSERT INTO github_ask_reading (singleton, last_read_at) VALUES (1, unixepoch())
                 ON CONFLICT (singleton) DO UPDATE SET
                     last_read_at = unixepoch(), failed_at = NULL, failure = NULL",
                [],
            )?,
            Err(why) => {
                let why = clamp(why.trim(), MAX_FAILURE_BYTES);
                let why = if why.is_empty() {
                    "no reason given"
                } else {
                    why
                };
                connection.execute(
                    "INSERT INTO github_ask_reading (singleton, failed_at, failure)
                     VALUES (1, unixepoch(), ?1)
                     ON CONFLICT (singleton) DO UPDATE SET
                         failed_at = unixepoch(), failure = excluded.failure",
                    [why],
                )?
            }
        };
        Ok(())
    }

    /// How this Hive's reading of GitHub last went.
    ///
    /// # Errors
    /// Returns an error when persistence is unavailable.
    pub fn github_ask_reading(&self) -> Result<GithubAskReading, TaskStoreError> {
        Ok(self
            .connection()?
            .query_row(
                "SELECT last_read_at, failed_at, failure FROM github_ask_reading
                 WHERE singleton = 1",
                [],
                |row| {
                    Ok(GithubAskReading {
                        last_read_at: row.get(0)?,
                        failed_at: row.get(1)?,
                        failure: row.get(2)?,
                    })
                },
            )
            .optional()?
            .unwrap_or_default())
    }
}

fn task_is_open(
    transaction: &rusqlite::Transaction<'_>,
    task_id: TaskId,
) -> Result<bool, TaskStoreError> {
    Ok(transaction
        .query_row(
            "SELECT state NOT IN ('completed','abandoned') AND removed_at IS NULL
             FROM tasks WHERE id = ?1",
            [task_id.to_string()],
            |row| row.get(0),
        )
        .optional()?
        .unwrap_or(false))
}

fn file_draft(
    transaction: &rusqlite::Transaction<'_>,
    filing: &GithubAskFiling<'_>,
) -> Result<TaskId, TaskStoreError> {
    let hive_id: String = transaction.query_row(
        "SELECT hive_id FROM local_hive_identity WHERE singleton = 1",
        [],
        |row| row.get(0),
    )?;
    let hive_id = parse_domain_id::<HiveId>(&hive_id)?;
    let task_id = insert_draft(
        transaction,
        hive_id,
        &filing.ask.task_title(),
        filing.description,
        TaskPriority::Normal,
        filing.workspace,
        &TaskActivityActor::system(),
    )?;
    transaction.execute(
        "INSERT INTO task_intake (task_id, repository_named, recommended_worker_id,
                                  source_link, source_jira_key, requested_by)
         VALUES (?1, ?2, ?3, ?4, NULL, ?5)",
        params![
            task_id.to_string(),
            filing.repository_named,
            filing.recommended_worker.map(|worker| worker.to_string()),
            filing.ask.url(),
            filing.ask.author(),
        ],
    )?;
    Ok(task_id)
}

fn clamp(text: &str, most: usize) -> &str {
    if text.len() <= most {
        return text;
    }
    let mut end = most;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

#[cfg(test)]
mod tests {
    use swarm_domain::{
        AskEnded, EndedAskEffect, GithubAsk, GithubAskKind, ProviderKind, TaskActivityActor,
        TaskActivityActorKind, TaskActivityKind, TaskId, TaskState, WorkerId,
    };

    use super::{GithubAskArrival, GithubAskFiling};
    use crate::TaskStore;

    const REVIEW_URL: &str = "https://github.com/acme/petal/pull/42";

    fn review() -> GithubAsk {
        GithubAsk::new(
            GithubAskKind::ReviewRequest,
            "acme/petal",
            42,
            "Make the petals bloom",
            Some("sorrel"),
            "Fixes the stem.",
        )
        .unwrap()
    }

    fn issue(number: u64) -> GithubAsk {
        GithubAsk::new(
            GithubAskKind::AssignedIssue,
            "acme/petal",
            number,
            "Stem snaps",
            Some("ash"),
            "",
        )
        .unwrap()
    }

    fn receive(store: &TaskStore, ask: &GithubAsk, worker: Option<WorkerId>) -> GithubAskArrival {
        store
            .receive_github_ask(&GithubAskFiling {
                ask,
                description: "Read it.",
                workspace: "/workspace/petal",
                repository_named: true,
                recommended_worker: worker,
            })
            .unwrap()
    }

    fn filed(arrival: GithubAskArrival) -> TaskId {
        match arrival {
            GithubAskArrival::Filed(task) => task,
            other => panic!("expected a filing, got {other:?}"),
        }
    }

    fn last_note(
        store: &TaskStore,
        task: TaskId,
    ) -> (TaskActivityKind, String, TaskActivityActorKind) {
        let page = store.list_task_activity(task, 50).unwrap();
        let last = page
            .events
            .iter()
            .max_by_key(|event| event.sequence)
            .unwrap();
        (last.kind, last.note.clone(), last.actor_kind)
    }

    #[test]
    fn an_ask_files_one_draft_with_where_it_came_from_and_never_a_second() {
        let store = TaskStore::in_memory().unwrap();
        store.ensure_queen("/workspace/queen").unwrap();
        let worker = store
            .create_worker(
                "Petal",
                ProviderKind::ClaudeCode,
                "/workspace/petal",
                false,
                1,
            )
            .unwrap();
        let ask = review();

        assert_eq!(
            store.github_asks_not_standing(&[ask.url()]).unwrap(),
            vec![ask.url()]
        );
        let task = filed(receive(&store, &ask, Some(worker.id)));
        let draft = store.get_task(task).unwrap();
        assert_eq!(draft.state, TaskState::Draft);
        assert_eq!(draft.title, "Review acme/petal#42: Make the petals bloom");
        assert_eq!(draft.workspace, "/workspace/petal");
        let intake = store
            .intake_for_tasks(&[task])
            .unwrap()
            .remove(&task)
            .unwrap();
        assert!(intake.repository_named);
        assert_eq!(intake.recommended_worker.unwrap().name, "Petal");
        assert_eq!(intake.source.link.as_deref(), Some(REVIEW_URL));
        assert_eq!(intake.source.requested_by.as_deref(), Some("sorrel"));
        let (kind, _, actor) = last_note(&store, task);
        assert_eq!(
            (kind, actor),
            (TaskActivityKind::Created, TaskActivityActorKind::System)
        );

        assert_eq!(
            receive(&store, &ask, Some(worker.id)),
            GithubAskArrival::Known
        );
        assert!(
            store
                .github_asks_not_standing(&[ask.url()])
                .unwrap()
                .is_empty()
        );
        let standing = store.standing_github_asks().unwrap();
        assert_eq!(standing.len(), 1);
        assert_eq!(standing[0].url, REVIEW_URL);
        assert_eq!(standing[0].kind, GithubAskKind::ReviewRequest);
        assert_eq!(
            (standing[0].repository.as_str(), standing[0].number),
            ("acme/petal", 42)
        );
        assert_eq!(standing[0].task_id, task);
    }

    #[test]
    fn a_draft_closes_when_github_stops_asking_and_says_why() {
        let store = TaskStore::in_memory().unwrap();
        let task = filed(receive(&store, &review(), None));

        assert_eq!(
            store
                .record_github_ask_ended(REVIEW_URL, AskEnded::ReviewNoLongerRequested)
                .unwrap(),
            EndedAskEffect::Close
        );
        assert_eq!(store.get_task(task).unwrap().state, TaskState::Abandoned);
        let (kind, note, actor) = last_note(&store, task);
        assert_eq!(
            (kind, actor),
            (
                TaskActivityKind::StateChanged,
                TaskActivityActorKind::System
            )
        );
        assert!(
            note.contains("no longer requested on acme/petal#42"),
            "{note}"
        );
        assert!(store.standing_github_asks().unwrap().is_empty());
        // Ended once is ended: a second confirmation changes nothing.
        assert_eq!(
            store
                .record_github_ask_ended(REVIEW_URL, AskEnded::Merged)
                .unwrap(),
            EndedAskEffect::Nothing
        );
    }

    #[test]
    fn routed_work_is_told_and_left_where_it_is() {
        let store = TaskStore::in_memory().unwrap();
        let task = filed(receive(&store, &review(), None));
        store.transition_task(task, TaskState::Ready).unwrap();

        assert_eq!(
            store
                .record_github_ask_ended(REVIEW_URL, AskEnded::Merged)
                .unwrap(),
            EndedAskEffect::Note
        );
        assert_eq!(store.get_task(task).unwrap().state, TaskState::Ready);
        let (kind, note, actor) = last_note(&store, task);
        assert_eq!(
            (kind, actor),
            (TaskActivityKind::Noted, TaskActivityActorKind::System)
        );
        assert_eq!(note, "acme/petal#42 was merged on GitHub.");
    }

    #[test]
    fn asked_again_after_it_ended_reopens_open_work_or_files_anew() {
        let store = TaskStore::in_memory().unwrap();
        let ask = review();

        // Routed work still open: the same task hears it is asked again.
        let routed = filed(receive(&store, &ask, None));
        store.transition_task(routed, TaskState::Ready).unwrap();
        store
            .record_github_ask_ended(REVIEW_URL, AskEnded::ReviewNoLongerRequested)
            .unwrap();
        assert_eq!(
            store.github_asks_not_standing(&[ask.url()]).unwrap(),
            vec![ask.url()]
        );
        assert_eq!(
            receive(&store, &ask, None),
            GithubAskArrival::AskedAgain(routed)
        );
        let (kind, note, _) = last_note(&store, routed);
        assert_eq!(kind, TaskActivityKind::Noted);
        assert_eq!(note, "GitHub asks again: acme/petal#42.");
        assert_eq!(store.standing_github_asks().unwrap()[0].task_id, routed);

        // Closed work: a new draft, and the link follows it.
        store.transition_task(routed, TaskState::Abandoned).unwrap();
        store
            .record_github_ask_ended(REVIEW_URL, AskEnded::ReviewNoLongerRequested)
            .unwrap();
        let again = filed(receive(&store, &ask, None));
        assert_ne!(again, routed);
        assert_eq!(store.get_task(again).unwrap().state, TaskState::Draft);
        assert_eq!(store.standing_github_asks().unwrap()[0].task_id, again);
    }

    #[test]
    fn a_draft_closed_while_github_still_asks_stays_closed() {
        let store = TaskStore::in_memory().unwrap();
        let ask = review();
        let task = filed(receive(&store, &ask, None));
        store.transition_task(task, TaskState::Abandoned).unwrap();

        assert_eq!(receive(&store, &ask, None), GithubAskArrival::Known);
        assert!(
            store
                .github_asks_not_standing(&[ask.url()])
                .unwrap()
                .is_empty()
        );
        assert_eq!(store.get_task(task).unwrap().state, TaskState::Abandoned);
    }

    #[test]
    fn an_issue_the_repository_intake_filed_is_one_task_whichever_came_first() {
        let store = TaskStore::in_memory().unwrap();

        let earlier = issue(7);
        let filed_by_intake = store
            .import_github_issue(&earlier.url(), "Stem snaps", "", "github://issues")
            .unwrap()
            .unwrap();
        assert_eq!(
            receive(&store, &earlier, None),
            GithubAskArrival::Adopted(filed_by_intake.id)
        );
        assert_eq!(
            store.standing_github_asks().unwrap()[0].task_id,
            filed_by_intake.id
        );

        let later = issue(8);
        let asked = filed(receive(&store, &later, None));
        assert!(
            store
                .import_github_issue(&later.url(), "Stem snaps", "", "github://issues")
                .unwrap()
                .is_none()
        );
        // And it is remembered there, so the intake does not ask again.
        assert!(
            store
                .import_github_issue(&later.url(), "Stem snaps", "", "github://issues")
                .unwrap()
                .is_none()
        );
        assert_eq!(store.get_task(asked).unwrap().state, TaskState::Draft);
    }

    #[test]
    fn a_removed_task_is_not_closed_or_told() {
        let store = TaskStore::in_memory().unwrap();
        let task = filed(receive(&store, &review(), None));
        store
            .remove_task_as(task, &TaskActivityActor::system(), "Not ours.")
            .unwrap();

        assert_eq!(
            store
                .record_github_ask_ended(REVIEW_URL, AskEnded::Closed)
                .unwrap(),
            EndedAskEffect::Nothing
        );
        assert!(store.standing_github_asks().unwrap().is_empty());
    }

    #[test]
    fn a_reading_that_stops_working_is_remembered_as_lapsed() {
        let store = TaskStore::in_memory().unwrap();
        assert_eq!(store.github_ask_reading().unwrap().lapsed(), None);

        store
            .record_github_ask_reading(Err("gh is not signed in on this machine"))
            .unwrap();
        assert_eq!(
            store.github_ask_reading().unwrap().lapsed(),
            None,
            "a Hive that never read GitHub has nothing to report"
        );

        store.record_github_ask_reading(Ok(())).unwrap();
        assert_eq!(store.github_ask_reading().unwrap().lapsed(), None);
        store
            .record_github_ask_reading(Err("gh is not signed in on this machine"))
            .unwrap();
        assert_eq!(
            store.github_ask_reading().unwrap().lapsed(),
            Some("gh is not signed in on this machine")
        );
        store.record_github_ask_reading(Ok(())).unwrap();
        assert_eq!(store.github_ask_reading().unwrap().lapsed(), None);
    }
}
