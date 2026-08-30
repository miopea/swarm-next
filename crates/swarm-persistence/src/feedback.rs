use rusqlite::OptionalExtension;
use rusqlite::params;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use swarm_domain::{Task, TaskId};

use super::{TaskStore, TaskStoreError};
use crate::events::insert_control_room_event;

/// The GitHub account this Hive files feedback as.
///
/// `access_token` is in here because the filing path needs it; it is never
/// serialised to a client. What the UI is told is the login and whether the
/// connection is still good.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GithubConnection {
    pub login: String,
    pub access_token: String,
    pub access_expires_at: Option<i64>,
    pub refresh_token: Option<String>,
    pub refresh_expires_at: Option<i64>,
    pub connected_at: i64,
}

const MAX_FEEDBACK_NOTE_BYTES: usize = 8_000;
const MAX_FEEDBACK_BUNDLE_BYTES: usize = 128 * 1024;
const MAX_FEEDBACK_ATTACHMENT_NAME_BYTES: usize = 128;
pub const MAX_DOGFOOD_REPORTS: usize = 50;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct DogfoodReport {
    pub id: String,
    pub expectation: String,
    pub observation: String,
    pub diagnostic_bundle: String,
    pub attachment_name: Option<String>,
    /// Where this report went, if it went anywhere.
    ///
    /// None means it is on this Hive and nowhere else — which is the honest
    /// answer, and was previously the ONLY answer while looking like a
    /// submission. A colleague filed a report, believed she had raised an
    /// issue, and it sat here unread.
    #[serde(default)]
    pub github_issue_url: Option<String>,
    pub created_at: i64,
}

impl TaskStore {
    /// Saves one operator-reviewed report in the private Hive database.
    ///
    /// # Errors
    /// Returns validation or persistence failures.
    pub fn create_dogfood_report(
        &self,
        expectation: &str,
        observation: &str,
        diagnostic_bundle: &str,
        attachment_name: Option<&str>,
    ) -> Result<DogfoodReport, TaskStoreError> {
        validate_report(expectation, observation, diagnostic_bundle, attachment_name)?;
        let id = Uuid::now_v7().to_string();
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        transaction.execute(
            "INSERT INTO dogfood_reports (
                id, expectation, observation, diagnostic_bundle, attachment_name
             ) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                id,
                expectation,
                observation,
                diagnostic_bundle,
                attachment_name
            ],
        )?;
        transaction.execute(
            "DELETE FROM dogfood_reports
             WHERE id NOT IN (
                SELECT id FROM dogfood_reports ORDER BY created_at DESC, id DESC LIMIT ?1
             )",
            [i64::try_from(MAX_DOGFOOD_REPORTS).unwrap_or(i64::MAX)],
        )?;
        let report = read_report(&transaction, &id)?;
        transaction.commit()?;
        Ok(report)
    }

    /// Brings one GitHub issue down as a DRAFT task, once.
    ///
    /// DRAFT ON PURPOSE, and it is the operator's own shape: "the issues would
    /// come down to my copy of Swarm as a draft task that the queen could
    /// review or run by me. Then if valid make it ready, or if a duplicate
    /// merge." An issue is somebody else's opinion that something is wrong; it
    /// becomes work when Queen says so, not when a poller notices it.
    ///
    /// Returns None when this issue has already arrived. The intake polls, so
    /// without that memory the same issue would be filed on every tick and
    /// would bury the board it exists to feed.
    ///
    /// # Errors
    /// Returns validation or persistence failures.
    pub fn import_github_issue(
        &self,
        issue_url: &str,
        title: &str,
        body: &str,
        workspace: &str,
    ) -> Result<Option<Task>, TaskStoreError> {
        let issue_url = issue_url.trim();
        if issue_url.is_empty() || issue_url.len() > 2_048 {
            return Err(TaskStoreError::InvalidDogfoodReport);
        }
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let seen: bool = transaction.query_row(
            "SELECT EXISTS(SELECT 1 FROM github_issue_tasks WHERE issue_url = ?1)",
            [issue_url],
            |row| row.get(0),
        )?;
        if seen {
            transaction.commit()?;
            return Ok(None);
        }
        let hive_id: String = transaction.query_row(
            "SELECT hive_id FROM local_hive_identity WHERE singleton = 1",
            [],
            |row| row.get(0),
        )?;
        let task_id = TaskId::new();
        let title = clamp_issue_title(title);
        let description = format!("{}\n\nFrom {issue_url}", body.trim());
        transaction.execute(
            "INSERT INTO tasks (id, hive_id, title, description, priority, workspace, state, position)
             VALUES (?1, ?2, ?3, ?4, 'normal', ?5, 'draft',
                 COALESCE((SELECT MAX(position) + 1 FROM tasks WHERE hive_id = ?2), 0))",
            params![task_id.to_string(), hive_id, title, description, workspace],
        )?;
        transaction.execute(
            "INSERT INTO task_activity (task_id, kind, to_state, note, actor_kind)
             VALUES (?1, 'created', 'draft', ?2, 'system')",
            params![task_id.to_string(), format!("Arrived from {issue_url}")],
        )?;
        transaction.execute(
            "INSERT INTO github_issue_tasks (issue_url, task_id) VALUES (?1, ?2)",
            params![issue_url, task_id.to_string()],
        )?;
        insert_control_room_event(&transaction, crate::ControlRoomEventKind::TasksChanged)?;
        transaction.commit()?;
        drop(connection);
        // Read after commit, the way the email intake does: there is no
        // in-transaction task reader and inventing one to save a query would
        // duplicate the row mapping.
        Ok(Some(self.get_task(task_id)?))
    }

    /// Remembers the GitHub account this Hive files feedback as.
    ///
    /// Replaces rather than accumulates: connecting a second account means the
    /// first is no longer the answer, and keeping both would leave the filing
    /// path picking one.
    ///
    /// # Errors
    /// Returns validation or persistence failures.
    pub fn save_github_connection(
        &self,
        login: &str,
        access_token: &str,
        access_expires_at: Option<i64>,
        refresh_token: Option<&str>,
        refresh_expires_at: Option<i64>,
    ) -> Result<GithubConnection, TaskStoreError> {
        let login = login.trim();
        if login.is_empty() || login.len() > 128 || access_token.trim().is_empty() {
            return Err(TaskStoreError::InvalidDogfoodReport);
        }
        let connection = self.connection()?;
        connection.execute(
            "INSERT INTO github_user_connection (
                 singleton, login, access_token, access_expires_at, refresh_token, refresh_expires_at
             ) VALUES (1, ?1, ?2, ?3, ?4, ?5)
             ON CONFLICT (singleton) DO UPDATE SET
                 login = excluded.login,
                 access_token = excluded.access_token,
                 access_expires_at = excluded.access_expires_at,
                 refresh_token = excluded.refresh_token,
                 refresh_expires_at = excluded.refresh_expires_at,
                 connected_at = unixepoch()",
            params![login, access_token, access_expires_at, refresh_token, refresh_expires_at],
        )?;
        drop(connection);
        self.github_connection()?.ok_or(TaskStoreError::NotFound)
    }

    /// The connected GitHub account, if there is one.
    ///
    /// # Errors
    /// Returns persistence failures.
    pub fn github_connection(&self) -> Result<Option<GithubConnection>, TaskStoreError> {
        let connection = self.connection()?;
        let found = connection
            .query_row(
                "SELECT login, access_token, access_expires_at, refresh_token, refresh_expires_at, connected_at
                 FROM github_user_connection WHERE singleton = 1",
                [],
                |row| {
                    Ok(GithubConnection {
                        login: row.get(0)?,
                        access_token: row.get(1)?,
                        access_expires_at: row.get(2)?,
                        refresh_token: row.get(3)?,
                        refresh_expires_at: row.get(4)?,
                        connected_at: row.get(5)?,
                    })
                },
            )
            .optional()?;
        Ok(found)
    }

    /// Forgets the connected account, so filing falls back to anonymous.
    ///
    /// # Errors
    /// Returns persistence failures.
    pub fn forget_github_connection(&self) -> Result<(), TaskStoreError> {
        let connection = self.connection()?;
        connection.execute("DELETE FROM github_user_connection WHERE singleton = 1", [])?;
        Ok(())
    }

    /// Records where a report was filed, so a person can tell afterwards.
    ///
    /// # Errors
    /// Returns `NotFound` for an unknown report, or a persistence failure.
    pub fn record_dogfood_report_issue(
        &self,
        id: &str,
        issue_url: &str,
    ) -> Result<DogfoodReport, TaskStoreError> {
        let issue_url = issue_url.trim();
        if issue_url.is_empty() || issue_url.len() > 2_048 {
            return Err(TaskStoreError::InvalidDogfoodReport);
        }
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let changed = transaction.execute(
            "UPDATE dogfood_reports SET github_issue_url = ?2 WHERE id = ?1",
            params![id, issue_url],
        )?;
        if changed != 1 {
            return Err(TaskStoreError::NotFound);
        }
        let report = read_report(&transaction, id)?;
        transaction.commit()?;
        Ok(report)
    }

    /// Lists the newest private dogfood reports first.
    ///
    /// # Errors
    /// Returns validation or persistence failures.
    pub fn list_dogfood_reports(&self, limit: usize) -> Result<Vec<DogfoodReport>, TaskStoreError> {
        if !(1..=MAX_DOGFOOD_REPORTS).contains(&limit) {
            return Err(TaskStoreError::InvalidDogfoodReportLimit);
        }
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT id, expectation, observation, diagnostic_bundle, attachment_name,
                    github_issue_url, created_at
             FROM dogfood_reports ORDER BY created_at DESC, id DESC LIMIT ?1",
        )?;
        statement
            .query_map([i64::try_from(limit).unwrap_or(i64::MAX)], map_report)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(Into::into)
    }

    /// Confirms an opaque attachment belongs to one retained dogfood report.
    ///
    /// # Errors
    /// Returns database failures.
    pub fn dogfood_attachment_is_referenced(&self, name: &str) -> Result<bool, TaskStoreError> {
        let connection = self.connection()?;
        connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM dogfood_reports WHERE attachment_name = ?1)",
                [name],
                |row| row.get(0),
            )
            .map_err(Into::into)
    }
}

fn validate_report(
    expectation: &str,
    observation: &str,
    bundle: &str,
    attachment_name: Option<&str>,
) -> Result<(), TaskStoreError> {
    if (expectation.trim().is_empty() && observation.trim().is_empty())
        || expectation.len() > MAX_FEEDBACK_NOTE_BYTES
        || observation.len() > MAX_FEEDBACK_NOTE_BYTES
        || bundle.is_empty()
        || bundle.len() > MAX_FEEDBACK_BUNDLE_BYTES
    {
        return Err(TaskStoreError::InvalidDogfoodReport);
    }
    if attachment_name.is_some_and(|name| {
        name.is_empty()
            || name.len() > MAX_FEEDBACK_ATTACHMENT_NAME_BYTES
            || !name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_'))
    }) {
        return Err(TaskStoreError::InvalidDogfoodAttachment);
    }
    Ok(())
}

fn read_report(
    connection: &rusqlite::Connection,
    id: &str,
) -> Result<DogfoodReport, rusqlite::Error> {
    connection.query_row(
        "SELECT id, expectation, observation, diagnostic_bundle, attachment_name,
                github_issue_url, created_at
         FROM dogfood_reports WHERE id = ?1",
        [id],
        map_report,
    )
}

fn map_report(row: &rusqlite::Row<'_>) -> rusqlite::Result<DogfoodReport> {
    Ok(DogfoodReport {
        id: row.get(0)?,
        expectation: row.get(1)?,
        observation: row.get(2)?,
        diagnostic_bundle: row.get(3)?,
        attachment_name: row.get(4)?,
        github_issue_url: row.get(5)?,
        created_at: row.get(6)?,
    })
}

/// Stores the GitHub account this Hive files feedback as, when someone has
/// connected one.
///
/// ONE ROW, because a Hive has one operator identity. Keyed on a singleton
/// rather than an operator id so that reading it needs no join and cannot
/// return two answers; if Hives ever carry several people this becomes a real
/// key and the migration is obvious.
///
/// The tokens EXPIRE — the app was registered with "Expire user authorization
/// tokens" on, which is the safer setting and the reason `refresh_token` is here
/// rather than a permanent credential sitting in a table forever.
pub(super) fn migrate_github_user_connection(
    transaction: &rusqlite::Transaction<'_>,
) -> rusqlite::Result<()> {
    transaction.execute_batch(
        "CREATE TABLE IF NOT EXISTS github_user_connection (
             singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
             login TEXT NOT NULL,
             access_token TEXT NOT NULL,
             access_expires_at INTEGER,
             refresh_token TEXT,
             refresh_expires_at INTEGER,
             connected_at INTEGER NOT NULL DEFAULT (unixepoch())
         );
         PRAGMA user_version = 109;",
    )
}

/// A title that fits the board, cut on a char boundary.
fn clamp_issue_title(title: &str) -> String {
    let title = title.trim();
    if title.is_empty() {
        return "GitHub issue".to_owned();
    }
    if title.len() <= crate::MAX_TASK_TITLE_BYTES {
        return title.to_owned();
    }
    let mut end = crate::MAX_TASK_TITLE_BYTES;
    while end > 0 && !title.is_char_boundary(end) {
        end -= 1;
    }
    title[..end].trim_end().to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_are_private_bounded_and_newest_first() {
        let store = TaskStore::in_memory().unwrap();
        for index in 0..=MAX_DOGFOOD_REPORTS {
            store
                .create_dogfood_report(
                    "Expected a stable terminal",
                    &format!("Observed failure {index}"),
                    "sanitized evidence",
                    Some("screen-123.png"),
                )
                .unwrap();
        }
        let reports = store.list_dogfood_reports(MAX_DOGFOOD_REPORTS).unwrap();
        assert_eq!(reports.len(), MAX_DOGFOOD_REPORTS);
        assert_eq!(reports[0].observation, "Observed failure 50");
        assert_eq!(reports.last().unwrap().observation, "Observed failure 1");
    }

    #[test]
    fn rejects_empty_or_unsafe_reports() {
        let store = TaskStore::in_memory().unwrap();
        assert!(matches!(
            store.create_dogfood_report("", "", "bundle", None),
            Err(TaskStoreError::InvalidDogfoodReport)
        ));
        assert!(matches!(
            store.create_dogfood_report("expected", "observed", "bundle", Some("../secret")),
            Err(TaskStoreError::InvalidDogfoodAttachment)
        ));
    }
    #[test]
    fn an_issue_arrives_as_a_draft_and_never_arrives_twice() {
        let store = TaskStore::in_memory().unwrap();
        let url = "https://github.com/miopea/swarm-next/issues/7";

        let first = store
            .import_github_issue(
                url,
                "Terminal drops a line on my phone",
                "Every redraw.",
                "github://issues",
            )
            .unwrap()
            .expect("the first sighting files a task");

        // A DRAFT, which is the operator's ruling: an issue is somebody's
        // opinion that something is wrong, and Queen decides whether it is work.
        assert_eq!(first.state, swarm_domain::TaskState::Draft);
        assert_eq!(first.title, "Terminal drops a line on my phone");
        assert!(
            first.description.contains(url),
            "the task says where it came from: {}",
            first.description
        );

        // The poll runs every five minutes forever. Without this, the board
        // fills with one copy of every open issue per tick.
        let second = store
            .import_github_issue(
                url,
                "Terminal drops a line on my phone",
                "Every redraw.",
                "github://issues",
            )
            .unwrap();
        assert!(second.is_none(), "the same issue does not refile");
        assert_eq!(store.list_tasks().unwrap().len(), 1);
    }

    #[test]
    fn an_issue_with_no_url_is_refused_rather_than_filed() {
        let store = TaskStore::in_memory().unwrap();
        assert!(matches!(
            store.import_github_issue("  ", "A title", "A body", "github://issues"),
            Err(TaskStoreError::InvalidDogfoodReport)
        ));
    }
}
