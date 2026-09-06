//! Request-driven, content-free storage evidence. No cleanup or admission policy.
use serde::Serialize;
use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};
use tokio::sync::Semaphore;

const ADVISORY_BYTES: u64 = 2 * 1024 * 1024 * 1024;
const CRITICAL_BYTES: u64 = 512 * 1024 * 1024;
static PROBES: Semaphore = Semaphore::const_new(2);

#[derive(Debug, Serialize)]
pub(super) struct StorageObservation {
    scope: &'static str,
    total_bytes: Option<u64>,
    available_bytes: Option<u64>,
    pressure: &'static str,
    advisory_available_bytes: u64,
    critical_available_bytes: u64,
}

fn observation(scope: &'static str, capacity: Option<(u64, u64)>) -> StorageObservation {
    let capacity = capacity.filter(|(total, available)| *total > 0 && available <= total);
    StorageObservation {
        scope,
        total_bytes: capacity.map(|(total, _)| total),
        available_bytes: capacity.map(|(_, available)| available),
        pressure: match capacity {
            Some((_, available)) if available < CRITICAL_BYTES => "critical",
            Some((_, available)) if available < ADVISORY_BYTES => "advisory",
            Some(_) => "normal",
            None => "unavailable",
        },
        advisory_available_bytes: ADVISORY_BYTES,
        critical_available_bytes: CRITICAL_BYTES,
    }
}

fn unavailable() -> Vec<StorageObservation> {
    ["system", "temporary", "database"]
        .map(|scope| observation(scope, None))
        .into()
}

#[cfg(unix)]
fn capacity(path: &Path) -> Option<(u64, u64)> {
    let stats = nix::sys::statvfs::statvfs(path).ok()?;
    Some((
        stats.blocks().checked_mul(stats.fragment_size())?,
        stats
            .blocks_available()
            .checked_mul(stats.fragment_size())?,
    ))
}

#[cfg(not(unix))]
fn capacity(_path: &Path) -> Option<(u64, u64)> {
    None
}

pub(super) async fn sample(database: Option<Arc<PathBuf>>) -> Vec<StorageObservation> {
    bounded_probe(&PROBES, Duration::from_secs(2), move || {
        vec![
            observation("system", capacity(Path::new("/"))),
            observation("temporary", capacity(&std::env::temp_dir())),
            observation(
                "database",
                database.as_deref().and_then(|path| capacity(path)),
            ),
        ]
    })
    .await
}

async fn bounded_probe<F>(
    gate: &'static Semaphore,
    deadline: Duration,
    probe: F,
) -> Vec<StorageObservation>
where
    F: FnOnce() -> Vec<StorageObservation> + Send + 'static,
{
    let Ok(permit) = gate.try_acquire() else {
        return unavailable();
    };
    let task = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        probe()
    });
    match tokio::time::timeout(deadline, task).await {
        Ok(Ok(observations)) => observations,
        _ => unavailable(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn timed_out_probe_retains_capacity_until_its_owned_work_returns() {
        static GATE: Semaphore = Semaphore::const_new(1);
        let (release, blocked) = std::sync::mpsc::channel();
        let (started, ready) = tokio::sync::oneshot::channel();
        let (done, finished) = tokio::sync::oneshot::channel();
        let probe = tokio::spawn(bounded_probe(&GATE, Duration::from_millis(20), move || {
            let _ = started.send(());
            let _ = blocked.recv_timeout(Duration::from_secs(5));
            let _ = done.send(());
            unavailable()
        }));
        ready.await.unwrap();
        let result = probe.await.unwrap();
        let held = GATE.available_permits();
        let rejected = bounded_probe(&GATE, Duration::from_secs(1), || {
            panic!("must not queue another probe")
        })
        .await;
        release.send(()).unwrap();
        finished.await.unwrap();
        // Acquiring proves the original blocking closure dropped its permit.
        let _returned = GATE.acquire().await.unwrap();
        assert_eq!(held, 0);
        assert!(
            result
                .iter()
                .chain(&rejected)
                .all(|value| value.pressure == "unavailable")
        );
    }
    #[test]
    fn capacity_boundaries_and_invalid_readings_are_honest() {
        for (available, expected) in [
            (0, "critical"),
            (CRITICAL_BYTES - 1, "critical"),
            (CRITICAL_BYTES, "advisory"),
            (ADVISORY_BYTES - 1, "advisory"),
            (ADVISORY_BYTES, "normal"),
        ] {
            assert_eq!(
                observation("system", Some((4 * ADVISORY_BYTES, available))).pressure,
                expected
            );
        }
        for capacity in [None, Some((0, 0)), Some((1, 2))] {
            let value = observation("database", capacity);
            assert_eq!(value.pressure, "unavailable");
            assert!(value.available_bytes.is_none());
        }
    }
    #[tokio::test]
    async fn saturated_probe_admission_returns_unavailable_without_starting_work() {
        let _permits = PROBES.acquire_many(2).await.unwrap();
        let values = sample(None).await;
        assert_eq!(values.len(), 3);
        assert!(values.iter().all(|value| value.pressure == "unavailable"));
        let json = serde_json::to_string(&values).unwrap();
        assert!(!json.contains("path"));
    }
}
