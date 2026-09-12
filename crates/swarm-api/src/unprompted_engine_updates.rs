//! Noticing an engine replacement that nobody asked for.
//!
//! ⚠️ THE RECORD BUILT IN SCHEMA 169 WATCHED THE WRONG DOOR. Its only writer was
//! the maintenance endpoint — the path that asks permission first and tells the
//! operator what it will cost. Meanwhile `swarm-host-reconcile.timer` fires
//! every two minutes, and `swarm-package reconcile-host-if-idle` replaces the
//! engine whenever no session reports mid-turn. That path stops loaded workers,
//! runs entirely in the packaging layer, and touched nothing here.
//!
//! So the update that happens TO you left no trace, and the one you chose left a
//! full one. Operator decision 01a092cd, 2026-09-11: "keep it automatic, but say
//! so when it happens."
//!
//! OBSERVED, NOT REPORTED. Nothing asks the package layer to tell us; this
//! compares the engine the host says it is running against the engine it said
//! last time. That is deliberate — it also catches a swap made by hand at a
//! shell, by a future path nobody has written yet, or by a script, and it cannot
//! be defeated by a reporter that forgot to report. It is the same rule the
//! package layer already learned the hard way: ask the running process, not the
//! symlink it was just handed.

use swarm_domain::ControlRoomEventKind;
use swarm_persistence::{WorkerEngineUpdateInitiator, WorkerEngineUpdateOutcome};

use crate::{AppState, ObservedEngine, maintenance::host_status_snapshot, task_store};

impl AppState {
    /// Compares the running engine with the one last seen, and records a change.
    ///
    /// FIRST SIGHTING RECORDS NOTHING, and that is not a missed case. After this
    /// process starts there is no earlier observation to compare against, so a
    /// difference cannot be distinguished from never having looked. Claiming a
    /// swap there would report one on every API restart.
    pub(crate) async fn notice_unprompted_engine_update(&self) {
        // A lifecycle operation in flight is an engine change somebody ASKED
        // for, and that path writes its own record. Taking this lock is only a
        // question -- is anything in progress -- so it is released at once.
        if self.worker_lifecycle.try_lock().is_err() {
            return;
        }
        let Ok(status) = host_status_snapshot(self).await else {
            // An unreachable host is not a changed engine. The baseline is kept
            // so a host that comes back on the SAME engine stays silent.
            return;
        };
        let seen = ObservedEngine {
            build_id: status.host_build_id.clone(),
            host_version: status.host_version.clone(),
            protocol_version: status.protocol_version,
        };
        let previous = match self.observed_worker_engine.write() {
            Ok(mut observed) => observed.replace(seen.clone()),
            Err(_) => return,
        };
        let Some(previous) = previous else {
            return;
        };
        if !engine_moved(&previous, &seen) {
            return;
        }
        record(self, &previous, &seen);
    }

    /// Accepts an engine this API changed on purpose, so the observer stays quiet.
    ///
    /// Without this the supervisor's next pass would see the new engine, find no
    /// lifecycle operation still running, and file the operator's own deliberate
    /// update a second time as one nobody asked for.
    pub(crate) fn accept_worker_engine(&self, status: &swarm_terminal::TerminalHostStatus) {
        if let Ok(mut observed) = self.observed_worker_engine.write() {
            *observed = Some(ObservedEngine {
                build_id: status.host_build_id.clone(),
                host_version: status.host_version.clone(),
                protocol_version: status.protocol_version,
            });
        }
    }
}

/// Whether these two observations describe different engines.
///
/// ⚠️ THE BUILD ID IS THE FACT AND THE VERSION IS THE FALLBACK, which is the
/// same rule the engine-staleness check already uses: two releases can share a
/// version string while carrying different engines, and a host old enough not to
/// report a build id can still report its version.
///
/// AN ABSENT BUILD ID IS NOT A DIFFERENCE. Comparing `Some(x)` with `None`
/// directly would fire on a host restarting into a build that does not report
/// one, filing a swap that did not happen — so an absence on either side drops
/// to the version, and equal versions stay silent. This is the direction to err
/// in: a missed swap costs a line on a card, an invented one costs the
/// operator's trust in every other line.
fn engine_moved(previous: &ObservedEngine, seen: &ObservedEngine) -> bool {
    match (&previous.build_id, &seen.build_id) {
        (Some(before), Some(after)) => before != after,
        _ => previous.host_version != seen.host_version,
    }
}

/// Writes the attempt, already finished, and wakes anything watching.
///
/// RECORDED AS SUCCEEDED BECAUSE THE EVIDENCE IS THE OUTCOME. Everywhere else in
/// this table an open row means "started, never heard from again"; here there is
/// no start to have heard from. The engine demonstrably changed, which is the
/// only thing being claimed.
///
/// WHAT IT COST IS LEFT UNKNOWN rather than guessed. An observer learns the
/// engine moved and cannot know how many sessions went down with it; `None` says
/// nobody counted, and 0 would say it cost nothing.
fn record(state: &AppState, previous: &ObservedEngine, seen: &ObservedEngine) {
    let Ok(store) = task_store(state) else {
        return;
    };
    let now = crate::unix_timestamp();
    let protocol_moved =
        (previous.protocol_version != seen.protocol_version).then_some(seen.protocol_version);
    let attempt = store.begin_worker_engine_update(
        &previous.host_version,
        &seen.host_version,
        protocol_moved,
        None,
        WorkerEngineUpdateInitiator::Automatic,
        now,
    );
    match attempt {
        Ok(attempt) => {
            let _ = store.finish_worker_engine_update(
                &attempt,
                WorkerEngineUpdateOutcome::Succeeded,
                "Observed by Swarm: the terminal host reported a different engine than it did last time, and no maintenance request was in flight.",
                now,
            );
            tracing::info!(
                from = %previous.host_version,
                to = %seen.host_version,
                "the worker engine was replaced without being asked for"
            );
            let _ = store.record_control_room_event(ControlRoomEventKind::RuntimeChanged);
            state.control_room_notify.notify_waiters();
        }
        Err(error) => {
            tracing::warn!(%error, "an unprompted worker engine update could not be recorded");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::engine_moved;
    use crate::ObservedEngine;

    fn engine(build_id: Option<&str>, version: &str) -> ObservedEngine {
        ObservedEngine {
            build_id: build_id.map(str::to_owned),
            host_version: version.to_owned(),
            protocol_version: 17,
        }
    }

    #[test]
    fn the_build_id_decides_when_both_hosts_report_one() {
        assert!(engine_moved(
            &engine(Some("aaa"), "1.8.1"),
            &engine(Some("bbb"), "1.8.1")
        ));
        // Same engine, different version string: two releases can carry one.
        assert!(!engine_moved(
            &engine(Some("aaa"), "1.8.1"),
            &engine(Some("aaa"), "1.8.2")
        ));
    }

    #[test]
    fn an_absent_build_id_is_not_a_difference() {
        // ⚠️ THE INVENTED SWAP THIS PREVENTS. A host restarting into a build
        // that does not report an id would otherwise read as a replacement, and
        // Swarm would tell the operator something happened that did not.
        assert!(!engine_moved(
            &engine(Some("aaa"), "1.8.1"),
            &engine(None, "1.8.1")
        ));
        assert!(!engine_moved(
            &engine(None, "1.8.1"),
            &engine(Some("aaa"), "1.8.1")
        ));
        assert!(!engine_moved(
            &engine(None, "1.8.1"),
            &engine(None, "1.8.1")
        ));
    }

    #[test]
    fn without_build_ids_the_version_is_all_there_is() {
        assert!(engine_moved(&engine(None, "1.8.1"), &engine(None, "1.9.0")));
    }
}
