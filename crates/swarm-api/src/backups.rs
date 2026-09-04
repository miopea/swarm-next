use serde::Serialize;
use std::path::Path;
use std::sync::Arc;
use tokio::io::AsyncReadExt;

use axum::{
    extract::State,
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};

use crate::{ApiError, AppState, authorize, task_store, task_store_error};

#[derive(Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub(super) enum DailyBackupStatus {
    NotReported,
    Unavailable,
    Ready { snapshot_day: String },
    Failed,
}

pub(super) async fn daily_status(directory: Option<&Path>) -> DailyBackupStatus {
    let Some(directory) = directory else {
        return DailyBackupStatus::NotReported;
    };
    let path = directory.join("daily-backup.status");
    let metadata = match tokio::fs::symlink_metadata(&path).await {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return DailyBackupStatus::NotReported;
        }
        Err(_) => return DailyBackupStatus::Unavailable,
    };
    if !metadata.is_file() || metadata.len() > 1_024 {
        return DailyBackupStatus::Unavailable;
    }
    let Ok(file) = tokio::fs::File::open(path).await else {
        return DailyBackupStatus::Unavailable;
    };
    let mut bytes = Vec::new();
    if file.take(1_025).read_to_end(&mut bytes).await.is_err() || bytes.len() > 1_024 {
        return DailyBackupStatus::Unavailable;
    }
    let Ok(text) = std::str::from_utf8(&bytes) else {
        return DailyBackupStatus::Unavailable;
    };
    parse_daily_status(text)
}

fn parse_daily_status(text: &str) -> DailyBackupStatus {
    let mut fields = std::collections::HashMap::new();
    for line in text.lines() {
        let Some((key, value)) = line.split_once('=') else {
            return DailyBackupStatus::Unavailable;
        };
        if fields.insert(key, value).is_some() {
            return DailyBackupStatus::Unavailable;
        }
    }
    match fields.get("state").copied() {
        Some("failed") if fields.get("step") == Some(&"daily-backup") => DailyBackupStatus::Failed,
        Some("ready") => {
            let Some(day) = fields.get("snapshot_day") else {
                return DailyBackupStatus::Unavailable;
            };
            if day.len() != 8
                || !day.bytes().all(|byte| byte.is_ascii_digit())
                || chrono::NaiveDate::parse_from_str(day, "%Y%m%d").is_err()
            {
                return DailyBackupStatus::Unavailable;
            }
            DailyBackupStatus::Ready {
                snapshot_day: (*day).to_owned(),
            }
        }
        _ => DailyBackupStatus::Unavailable,
    }
}

pub(super) async fn download_database(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let backup = tempfile::NamedTempFile::new().map_err(|error| {
        ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "backup_unavailable",
            format!("a temporary backup could not be created: {error}"),
        )
    })?;
    task_store(&state)?
        .backup_to(backup.path())
        .map_err(|error| task_store_error(&error))?;
    let bytes = std::fs::read(backup.path()).map_err(|error| {
        ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "backup_unavailable",
            format!("the completed backup could not be read: {error}"),
        )
    })?;
    let mut response_headers = HeaderMap::new();
    response_headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response_headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/vnd.sqlite3"),
    );
    response_headers.insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_static("attachment; filename=swarm-next-hive.sqlite3"),
    );
    Ok((response_headers, bytes).into_response())
}

#[cfg(test)]
mod daily_tests {
    use super::*;
    #[tokio::test]
    async fn bounded_status_distinguishes_absence_failure_and_recovery() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("daily-backup.status");
        assert_eq!(
            daily_status(Some(directory.path())).await,
            DailyBackupStatus::NotReported
        );
        std::fs::write(
            &path,
            "state=failed\nstep=daily-backup\ndetail=private detail must not be exported\n",
        )
        .unwrap();
        assert_eq!(
            serde_json::to_string(&daily_status(Some(directory.path())).await).unwrap(),
            "{\"state\":\"failed\"}"
        );
        std::fs::write(&path, "state=ready\nsnapshot_day=20260904\n").unwrap();
        assert_eq!(
            daily_status(Some(directory.path())).await,
            DailyBackupStatus::Ready {
                snapshot_day: "20260904".into()
            }
        );
        std::fs::write(&path, "x".repeat(1_025)).unwrap();
        assert_eq!(
            daily_status(Some(directory.path())).await,
            DailyBackupStatus::Unavailable
        );
    }
    #[test]
    fn malformed_or_ambiguous_status_is_not_success() {
        for text in [
            "state=ready\nsnapshot_day=20260230\n",
            "state=ready\nstate=failed\nsnapshot_day=20260904\n",
            "state=failed\nstep=other\n",
            "garbage",
        ] {
            assert_eq!(parse_daily_status(text), DailyBackupStatus::Unavailable);
        }
    }
}
