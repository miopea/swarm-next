//! Who may set the paths a task's work must not touch, and what they are
//! compared with (ADR 0172).
//!
//! The filer sets them when filing, through [`TaskService::file_task`]. After
//! that only Queen changes them. The worker holding the task has no way to,
//! so it cannot loosen its own limits: it asks Queen, and she decides.

use swarm_domain::{MustNotTouchReport, TaskActivityActor, TaskId, must_not_touch_report};
use swarm_persistence::MustNotTouchChange;

use crate::{AgentPrincipal, ApplicationError, TaskService, require_queen, unix_now};

/// The longest reason Queen may give for a change.
pub const MAX_MUST_NOT_TOUCH_REASON_CHARS: usize = 1_000;

impl TaskService {
    /// Replaces a task's must-not-touch patterns. An empty list removes them.
    ///
    /// # Errors
    /// `NotAuthorized` for anyone but Queen. `Refused` for unusable patterns,
    /// a missing or overlong reason, or a finished task.
    pub fn set_must_not_touch(
        &self,
        principal: AgentPrincipal,
        task_id: TaskId,
        patterns: &[String],
        reason: &str,
    ) -> Result<MustNotTouchChange, ApplicationError> {
        require_queen(principal)?;
        let patterns =
            swarm_domain::validate_must_not_touch(patterns).map_err(ApplicationError::Refused)?;
        let reason = reason.trim();
        if reason.is_empty() || reason.chars().count() > MAX_MUST_NOT_TOUCH_REASON_CHARS {
            return Err(ApplicationError::Refused(format!(
                "Say why the must-not-touch patterns change, in at most \
                 {MAX_MUST_NOT_TOUCH_REASON_CHARS} characters. It goes on the task's record."
            )));
        }
        let change = self.store.set_must_not_touch(
            task_id,
            &patterns,
            reason,
            &TaskActivityActor::worker(principal.worker_id),
            unix_now(),
        )?;
        if change == MustNotTouchChange::Closed {
            return Err(ApplicationError::Refused(
                "This task is finished, so its must-not-touch patterns will not be compared \
                 with anything again. Nothing was changed."
                    .to_owned(),
            ));
        }
        Ok(change)
    }

    /// The task's patterns compared with the changed paths of its recorded
    /// commits. `None` when it has no patterns, so a task without them reads
    /// exactly as it did before they existed.
    ///
    /// Shown, never enforced: nothing that moves a task reads this.
    ///
    /// # Errors
    /// Propagates persistence failures, including a stored row that no longer
    /// reads as valid patterns.
    pub fn must_not_touch_report(
        &self,
        task_id: TaskId,
    ) -> Result<Option<MustNotTouchReport>, ApplicationError> {
        let patterns = self.store.task_must_not_touch(task_id)?;
        if patterns.is_empty() {
            return Ok(None);
        }
        let commits = self.store.task_commit_report(task_id)?;
        Ok(must_not_touch_report(patterns.as_slice(), commits.as_ref()))
    }
}

#[cfg(test)]
mod tests {
    use swarm_domain::{
        CommitRepositoryState, CommitVerdict, ProviderKind, TaskCommit, TaskPriority, TaskState,
        WorkerRole,
    };
    use swarm_persistence::{MustNotTouchChange, TaskStore};

    use crate::{AgentPrincipal, ApplicationError, TaskService};

    fn owned(patterns: &[&str]) -> Vec<String> {
        patterns
            .iter()
            .map(|pattern| (*pattern).to_owned())
            .collect()
    }

    struct Hive {
        service: TaskService,
        store: TaskStore,
        queen: AgentPrincipal,
        worker: AgentPrincipal,
    }

    fn hive() -> Hive {
        let store = TaskStore::in_memory().unwrap();
        let queen = store.ensure_queen("/workspace/queen").unwrap();
        let worker = store
            .create_worker(
                "Petal",
                ProviderKind::ClaudeCode,
                "/workspace/petal",
                false,
                1,
            )
            .unwrap();
        Hive {
            service: TaskService::new(store.clone()),
            store,
            queen: AgentPrincipal {
                worker_id: queen.id,
                role: WorkerRole::Queen,
                active_session_id: None,
            },
            worker: AgentPrincipal {
                worker_id: worker.id,
                role: WorkerRole::Worker,
                active_session_id: None,
            },
        }
    }

    fn commit(sha: &str, paths: &[&str]) -> TaskCommit {
        TaskCommit {
            sha: sha.to_owned(),
            verdict: CommitVerdict::Present,
            subject: format!("feat: {sha}"),
            changed_paths: owned(paths),
            found_in: None,
        }
    }

    #[test]
    fn the_filer_sets_patterns_when_filing_and_unusable_ones_file_nothing() {
        let hive = hive();

        let task = hive
            .service
            .file_task(
                hive.worker,
                "Speed up the board",
                "",
                TaskPriority::Normal,
                "/workspace/petal",
                &owned(&["crates/swarm-terminal/**"]),
            )
            .unwrap();
        assert_eq!(
            hive.store.task_must_not_touch(task.id).unwrap().as_slice(),
            owned(&["crates/swarm-terminal/**"])
        );

        let before = hive.store.list_tasks().unwrap().len();
        let refused = hive.service.file_task(
            hive.worker,
            "Climbs out",
            "",
            TaskPriority::Normal,
            "/workspace/petal",
            &owned(&["../elsewhere/**"]),
        );
        assert!(
            matches!(refused, Err(ApplicationError::Refused(reason)) if reason.contains("climbs out"))
        );
        assert_eq!(
            hive.store.list_tasks().unwrap().len(),
            before,
            "nothing was filed"
        );
    }

    /// The worker holding the task can neither loosen nor clear its limits.
    #[test]
    fn only_queen_changes_patterns_and_the_worker_holding_the_task_cannot() {
        let hive = hive();
        let task = hive
            .service
            .file_task(
                hive.queen,
                "Scoped work",
                "",
                TaskPriority::Normal,
                "/workspace/petal",
                &owned(&["web/**", "packaging/**"]),
            )
            .unwrap();
        hive.store
            .assign_task_to_worker(task.id, hive.worker.worker_id)
            .unwrap();

        for loosened in [owned(&["web/**"]), Vec::new()] {
            assert!(matches!(
                hive.service
                    .set_must_not_touch(hive.worker, task.id, &loosened, "I need to."),
                Err(ApplicationError::NotAuthorized)
            ));
        }
        assert_eq!(
            hive.store.task_must_not_touch(task.id).unwrap().as_slice(),
            owned(&["web/**", "packaging/**"])
        );

        let change = hive
            .service
            .set_must_not_touch(
                hive.queen,
                task.id,
                &owned(&["web/**"]),
                "The worker asked; packaging must change too.",
            )
            .unwrap();
        assert!(matches!(change, MustNotTouchChange::Changed { .. }));
        assert_eq!(
            hive.store.task_must_not_touch(task.id).unwrap().as_slice(),
            owned(&["web/**"])
        );
    }

    #[test]
    fn queen_must_give_a_reason_and_cannot_change_finished_work() {
        let hive = hive();
        let task = hive
            .store
            .create_task("Scoped", "/workspace/petal")
            .unwrap();

        assert!(matches!(
            hive.service
                .set_must_not_touch(hive.queen, task.id, &owned(&["web/**"]), "  "),
            Err(ApplicationError::Refused(_))
        ));
        assert!(matches!(
            hive.service
                .set_must_not_touch(hive.queen, task.id, &owned(&["/abs"]), "Routing."),
            Err(ApplicationError::Refused(_))
        ));

        hive.store
            .transition_task(task.id, TaskState::Abandoned)
            .unwrap();
        assert!(matches!(
            hive.service
                .set_must_not_touch(hive.queen, task.id, &owned(&["web/**"]), "Late."),
            Err(ApplicationError::Refused(reason)) if reason.contains("finished")
        ));
    }

    #[test]
    fn the_report_names_crossings_and_a_task_without_patterns_has_none() {
        let hive = hive();
        let plain = hive.store.create_task("Plain", "/workspace/petal").unwrap();
        let scoped = hive
            .service
            .file_task(
                hive.queen,
                "Scoped",
                "",
                TaskPriority::Normal,
                "/workspace/petal",
                &owned(&["crates/swarm-terminal/**"]),
            )
            .unwrap();
        let commits = [
            commit("aaaa1111", &["web/src/a.ts"]),
            commit("bbbb2222", &["crates/swarm-terminal/src/ipc.rs"]),
        ];
        for task in [plain.id, scoped.id] {
            hive.store
                .record_task_commits(
                    task,
                    "/workspace/petal",
                    CommitRepositoryState::Read,
                    &commits,
                    50,
                )
                .unwrap();
        }

        assert_eq!(hive.service.must_not_touch_report(plain.id).unwrap(), None);
        let report = hive
            .service
            .must_not_touch_report(scoped.id)
            .unwrap()
            .unwrap();
        assert_eq!(report.commits_compared, 2);
        assert_eq!(report.crossings.len(), 1);
        assert_eq!(report.crossings[0].commit, "bbbb2222");
        assert_eq!(
            report.crossings[0].paths[0].path,
            "crates/swarm-terminal/src/ipc.rs"
        );
    }
}
