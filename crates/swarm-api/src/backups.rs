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
    let permit = state
        .database_export_limit
        .clone()
        .try_acquire_owned()
        .map_err(|_| {
            ApiError::new(
                StatusCode::CONFLICT,
                "backup_busy",
                "another database export is already in progress",
            )
        })?;
    let store = task_store(&state)?.clone();
    // The job, not the request future, owns admission. Disconnect/timeout cannot
    // admit another backup while SQLite is still preparing the first one.
    let prepared = tokio::task::spawn_blocking(move || {
        let backup = tempfile::NamedTempFile::new().map_err(|error| export_io_error(&error))?;
        store
            .backup_to(backup.path())
            .map_err(|error| task_store_error(&error))?;
        let file = backup.reopen().map_err(|error| export_io_error(&error))?;
        let length = file
            .metadata()
            .map_err(|error| export_io_error(&error))?
            .len();
        if length > 2 * 1_024 * 1_024 * 1_024 {
            return Err(ApiError::new(
                StatusCode::SERVICE_UNAVAILABLE,
                "backup_too_large",
                "database export exceeds the 2 GiB transfer limit",
            ));
        }
        Ok((backup, file, length, permit))
    });
    let (backup, file, length, permit) =
        tokio::time::timeout(std::time::Duration::from_secs(60), prepared)
            .await
            .map_err(|_| {
                ApiError::new(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "backup_timeout",
                    "database export preparation exceeded its request deadline",
                )
            })?
            .map_err(|_| {
                ApiError::new(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "backup_unavailable",
                    "database export preparation failed",
                )
            })??;
    let (sender, receiver) = tokio::sync::mpsc::channel::<Result<Vec<u8>, std::io::Error>>(2);
    tokio::spawn(async move {
        let _owned = (backup, permit);
        let mut file = tokio::fs::File::from_std(file);
        let transfer = async {
            loop {
                let mut chunk = vec![0; 65_536];
                match file.read(&mut chunk).await {
                    Ok(0) => break,
                    Ok(count) => {
                        chunk.truncate(count);
                        if sender.send(Ok(chunk)).await.is_err() {
                            break;
                        }
                    }
                    Err(error) => {
                        let _ = sender.send(Err(error)).await;
                        break;
                    }
                }
            }
        };
        let _ = tokio::time::timeout(std::time::Duration::from_secs(120), transfer).await;
    });
    let mut response_headers = HeaderMap::new();
    response_headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response_headers.insert(header::CONTENT_LENGTH, HeaderValue::from(length));
    response_headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/vnd.sqlite3"),
    );
    response_headers.insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_static("attachment; filename=swarm-next-hive.sqlite3"),
    );
    Ok((response_headers, export_body(receiver, length)).into_response())
}

fn export_body(
    receiver: tokio::sync::mpsc::Receiver<Result<Vec<u8>, std::io::Error>>,
    length: u64,
) -> axum::body::Body {
    let stream =
        futures_util::stream::unfold((receiver, length), |(mut receiver, remaining)| async move {
            match receiver.recv().await {
                Some(Ok(bytes)) => {
                    let remaining = remaining.saturating_sub(bytes.len() as u64);
                    Some((Ok(bytes), (receiver, remaining)))
                }
                Some(Err(error)) => Some((Err(error), (receiver, 0))),
                None if remaining > 0 => Some((
                    Err(std::io::Error::new(
                        std::io::ErrorKind::UnexpectedEof,
                        "database export ended before its complete snapshot was transferred",
                    )),
                    (receiver, 0),
                )),
                None => None,
            }
        });
    axum::body::Body::from_stream(stream)
}

fn export_io_error(error: &std::io::Error) -> ApiError {
    ApiError::new(
        StatusCode::SERVICE_UNAVAILABLE,
        "backup_unavailable",
        format!("database export file could not be prepared: {error}"),
    )
}

#[cfg(test)]
mod daily_tests {
    use super::*;
    #[tokio::test]
    async fn incomplete_export_is_an_error_not_a_successful_short_file() {
        let (sender, receiver) = tokio::sync::mpsc::channel(2);
        sender.send(Ok(vec![1, 2])).await.unwrap();
        drop(sender);
        assert!(
            axum::body::to_bytes(export_body(receiver, 3), 10)
                .await
                .is_err()
        );
        let (sender, receiver) = tokio::sync::mpsc::channel(2);
        sender.send(Ok(vec![1, 2])).await.unwrap();
        drop(sender);
        assert_eq!(
            axum::body::to_bytes(export_body(receiver, 2), 10)
                .await
                .unwrap()
                .as_ref(),
            &[1, 2]
        );
    }
    #[tokio::test]
    async fn export_admission_is_shared_and_does_not_queue() {
        let state = Arc::new(AppState::default().with_terminal_host(
            swarm_terminal::HostClient::new("/unreachable/host.sock"),
            "secret",
        ));
        let _permit = state
            .database_export_limit
            .clone()
            .try_acquire_owned()
            .unwrap();
        let mut headers = HeaderMap::new();
        headers.insert(
            header::AUTHORIZATION,
            HeaderValue::from_static("Bearer secret"),
        );
        let error = download_database(State(state), headers).await.unwrap_err();
        assert_eq!(error.status, StatusCode::CONFLICT);
    }
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
