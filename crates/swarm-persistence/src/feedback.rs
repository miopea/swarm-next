use rusqlite::params;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::{TaskStore, TaskStoreError};

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
}
