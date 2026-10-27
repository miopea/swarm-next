//! An outside tool files work for Queen to route (ADR 0126).
//!
//! A connected tool is a worker-level profile with no session, so the worker
//! rules gave it nothing to read and demanded a repository it often cannot
//! know. Intake is the scope built for it: it files with or without a
//! repository, reads in full only the tasks it filed, finds other open work by
//! title, and sees which workers exist. It still cannot assign or approve.

use swarm_domain::{
    IntakeSource, MAX_TASK_SEARCH_HITS, RoutingCandidate, Task, TaskActivityActor, TaskPriority,
    TaskSearchHit, WorkerProfile, WorkerRole, task_search_score, task_search_words,
};
use swarm_persistence::IntakeFiling;

use crate::{AgentPrincipal, ApplicationError, TaskService};

/// What an outside tool sends when it files work.
pub struct IntakeRequest<'a> {
    pub title: &'a str,
    pub description: &'a str,
    pub priority: TaskPriority,
    pub workspace: Option<&'a str>,
    /// A worker's name, as `routing_roster` lists it.
    pub recommended_worker: Option<&'a str>,
    pub source_link: Option<&'a str>,
    pub source_jira_key: Option<&'a str>,
    pub requested_by: Option<&'a str>,
}

impl TaskService {
    /// Whether this principal is an outside tool, and so works by intake.
    ///
    /// Read from the store rather than carried on the principal, for the reason
    /// `reads_the_board` gives: the profile is the authority, not a copy of it.
    #[must_use]
    pub fn files_by_intake(&self, principal: AgentPrincipal) -> bool {
        principal.role == WorkerRole::Worker
            && self
                .store
                .is_connection(principal.worker_id)
                .is_ok_and(|connection| connection)
    }

    /// Files a draft for Queen to route, with or without a repository.
    ///
    /// # Errors
    /// Refuses anyone who is not an outside tool, a recommended worker that is
    /// not on the routing roster, and a source that does not validate.
    pub fn file_intake_task(
        &self,
        principal: AgentPrincipal,
        request: &IntakeRequest<'_>,
    ) -> Result<Task, ApplicationError> {
        self.require_intake(principal)?;
        let source = IntakeSource::normalized(
            request.source_link,
            request.source_jira_key,
            request.requested_by,
        )
        .map_err(ApplicationError::Refused)?;
        let recommended_worker = match request
            .recommended_worker
            .map(str::trim)
            .filter(|name| !name.is_empty())
        {
            None => None,
            Some(name) => Some(
                self.routable_workers()?
                    .into_iter()
                    .find(|worker| worker.name.eq_ignore_ascii_case(name))
                    .map(|worker| worker.id)
                    .ok_or_else(|| {
                        ApplicationError::Refused(format!(
                            "No worker is named \"{name}\". swarm_list_workers lists the names \
                             work can be routed to. Leave recommended_worker out and Queen \
                             routes from the description."
                        ))
                    })?,
            ),
        };
        Ok(self.store.file_intake_task_as(
            &IntakeFiling {
                title: request.title,
                description: request.description,
                priority: request.priority,
                workspace: request.workspace,
                recommended_worker,
                source: &source,
            },
            &TaskActivityActor::worker(principal.worker_id),
        )?)
    }

    /// The workers work can be routed to: name, what each owns, and its
    /// repository. Not Queen, not other tools, not temporary workers.
    ///
    /// # Errors
    /// Refuses anyone who is not an outside tool.
    pub fn routing_roster(
        &self,
        principal: AgentPrincipal,
    ) -> Result<Vec<RoutingCandidate>, ApplicationError> {
        self.require_intake(principal)?;
        Ok(self
            .routable_workers()?
            .into_iter()
            .map(|worker| RoutingCandidate {
                name: worker.name,
                description: worker.description,
                workspace: worker.workspace,
            })
            .collect())
    }

    /// Open tasks whose titles match, best first, at most twenty.
    ///
    /// Matches titles and the Jira key work was filed under, never
    /// descriptions: a match on a description would be a read of it, and an
    /// outside tool reads other work by its title only.
    ///
    /// # Errors
    /// Refuses anyone who is not an outside tool, and a search with no word
    /// left to match.
    pub fn search_open_tasks(
        &self,
        principal: AgentPrincipal,
        query: &str,
    ) -> Result<Vec<TaskSearchHit>, ApplicationError> {
        self.require_intake(principal)?;
        let words = task_search_words(query).map_err(ApplicationError::Refused)?;
        let mut scored = self
            .store
            .open_task_titles(principal.worker_id)?
            .into_iter()
            .filter_map(|task| {
                let score = task_search_score(&task.title, task.jira_key.as_deref(), &words);
                (score > 0).then_some((score, task))
            })
            .collect::<Vec<_>>();
        // Stable, so equal scores keep the store's newest-first order.
        scored.sort_by_key(|(score, _)| std::cmp::Reverse(*score));
        Ok(scored
            .into_iter()
            .take(MAX_TASK_SEARCH_HITS)
            .map(|(_, task)| TaskSearchHit {
                id: task.id,
                title: task.title,
                state: task.state,
                workspace: task.workspace,
                filed_by_you: task.filed_by_reader,
            })
            .collect())
    }

    fn require_intake(&self, principal: AgentPrincipal) -> Result<(), ApplicationError> {
        if self.files_by_intake(principal) {
            Ok(())
        } else {
            Err(ApplicationError::NotAuthorized)
        }
    }

    /// One definition of who work can be routed to, so the roster a tool reads
    /// and the names it may recommend cannot drift apart.
    fn routable_workers(&self) -> Result<Vec<WorkerProfile>, ApplicationError> {
        Ok(self
            .store
            .list_worker_profiles()?
            .into_iter()
            .filter(|worker| worker.role == WorkerRole::Worker && !worker.ephemeral)
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use swarm_domain::{ProviderKind, TaskPriority, TaskState, WorkerProfile};
    use swarm_persistence::TaskStore;

    use super::IntakeRequest;
    use crate::{AgentPrincipal, ApplicationError, TaskService};

    struct Hive {
        service: TaskService,
        queen: WorkerProfile,
        worker: WorkerProfile,
        tool: AgentPrincipal,
    }

    fn hive() -> Hive {
        let store = TaskStore::in_memory().unwrap();
        let queen = store.ensure_queen("/workspace/queen").unwrap();
        let worker = store
            .create_worker("Web", ProviderKind::ClaudeCode, "/workspace/web", false, 1)
            .unwrap();
        let tool = store.connection_principal("client-a", "Claude").unwrap();
        Hive {
            service: TaskService::new(store),
            queen,
            worker,
            tool: AgentPrincipal::from(&tool),
        }
    }

    fn request(title: &str) -> IntakeRequest<'_> {
        IntakeRequest {
            title,
            description: "From an email.",
            priority: TaskPriority::Normal,
            workspace: None,
            recommended_worker: None,
            source_link: None,
            source_jira_key: None,
            requested_by: None,
        }
    }

    /// The tool reads what it filed and nothing else, and reading it is not
    /// acting on it.
    #[test]
    fn a_tool_reads_the_tasks_it_filed_and_only_those() {
        let hive = hive();
        let filed = hive
            .service
            .file_intake_task(hive.tool, &request("Renewal email is late"))
            .unwrap();
        let other = hive
            .service
            .store()
            .create_task("Someone else's", "/workspace/web")
            .unwrap();

        let readable = hive.service.list_tasks_readable_by(hive.tool).unwrap();
        assert_eq!(
            readable.iter().map(|task| task.id).collect::<Vec<_>>(),
            [filed.id]
        );
        assert!(
            hive.service
                .read_task_history(hive.tool, filed.id, 10)
                .is_ok()
        );
        assert!(matches!(
            hive.service.read_task_history(hive.tool, other.id, 10),
            Err(ApplicationError::NotAuthorized)
        ));
        assert!(
            hive.service
                .list_visible_tasks(hive.tool)
                .unwrap()
                .is_empty(),
            "what it may act on is unchanged"
        );
    }

    #[test]
    fn a_recommendation_must_name_a_worker_on_the_roster() {
        let hive = hive();
        for name in ["Nobody", "Queen", "Claude"] {
            let refused = hive.service.file_intake_task(
                hive.tool,
                &IntakeRequest {
                    recommended_worker: Some(name),
                    ..request("Fix it")
                },
            );
            assert!(
                matches!(refused, Err(ApplicationError::Refused(_))),
                "{name} is not a routing target"
            );
        }
        let filed = hive
            .service
            .file_intake_task(
                hive.tool,
                &IntakeRequest {
                    recommended_worker: Some(" web "),
                    source_jira_key: Some("abc-12"),
                    ..request("Fix it")
                },
            )
            .unwrap();
        let intake = hive.service.store().intake_for_tasks(&[filed.id]).unwrap();
        let intake = &intake[&filed.id];
        assert_eq!(
            intake.recommended_worker.as_ref().unwrap().id,
            hive.worker.id
        );
        assert_eq!(intake.source.jira_key.as_deref(), Some("ABC-12"));
        assert!(matches!(
            hive.service.file_intake_task(
                hive.tool,
                &IntakeRequest {
                    source_jira_key: Some("not a key"),
                    ..request("Fix it")
                },
            ),
            Err(ApplicationError::Refused(_))
        ));
    }

    #[test]
    fn the_roster_is_the_workers_work_can_go_to() {
        let hive = hive();
        hive.service
            .store()
            .create_worker("Old", ProviderKind::ClaudeCode, "/workspace/old", false, 2)
            .and_then(|old| hive.service.store().archive_worker_profile(old.id))
            .unwrap();
        let roster = hive.service.routing_roster(hive.tool).unwrap();
        let names = roster
            .iter()
            .map(|worker| worker.name.as_str())
            .collect::<Vec<_>>();
        assert_eq!(names, ["Web"]);
        assert_eq!(roster[0].workspace, "/workspace/web");
    }

    /// Titles match, descriptions do not, closed work is left out, and the
    /// Jira key it was filed under finds it outright.
    #[test]
    fn search_reads_open_titles_and_never_descriptions() {
        let hive = hive();
        let store = hive.service.store();
        let by_title = store
            .create_task("Renewal email is late", "/workspace/web")
            .unwrap();
        store
            .create_task_with_details(
                "Unrelated",
                "the renewal email again",
                TaskPriority::Normal,
                "/workspace/web",
            )
            .unwrap();
        let closed = store
            .create_task("Renewal email closed", "/workspace/web")
            .unwrap();
        store
            .transition_task(closed.id, TaskState::Abandoned)
            .unwrap();
        let keyed = hive
            .service
            .file_intake_task(
                hive.tool,
                &IntakeRequest {
                    source_jira_key: Some("ABC-12"),
                    ..request("Something else")
                },
            )
            .unwrap();

        let hits = hive
            .service
            .search_open_tasks(hive.tool, "renewal email")
            .unwrap();
        assert_eq!(
            hits.iter().map(|hit| hit.id).collect::<Vec<_>>(),
            [by_title.id]
        );
        assert!(!hits[0].filed_by_you);

        let hits = hive.service.search_open_tasks(hive.tool, "abc-12").unwrap();
        assert_eq!(
            hits.iter().map(|hit| hit.id).collect::<Vec<_>>(),
            [keyed.id]
        );
        assert!(hits[0].filed_by_you);

        assert!(matches!(
            hive.service.search_open_tasks(hive.tool, "the a"),
            Err(ApplicationError::Refused(_))
        ));
    }

    #[test]
    fn search_is_bounded_and_ranks_the_closest_title_first() {
        let hive = hive();
        let store = hive.service.store();
        for index in 0..30 {
            store
                .create_task(&format!("Email number {index}"), "/workspace/web")
                .unwrap();
        }
        let closest = store
            .create_task("Old renewal email", "/workspace/web")
            .unwrap();
        // Touch the others so the closest is not also the newest.
        store.create_task("Email newest", "/workspace/web").unwrap();

        let hits = hive
            .service
            .search_open_tasks(hive.tool, "renewal email")
            .unwrap();
        assert_eq!(hits.len(), swarm_domain::MAX_TASK_SEARCH_HITS);
        assert_eq!(hits[0].id, closest.id);
    }

    /// Workers and Queen keep the surface they had: none of this is theirs.
    #[test]
    fn intake_is_refused_to_workers_and_queen() {
        let hive = hive();
        for principal in [
            AgentPrincipal::from(&hive.worker),
            AgentPrincipal::from(&hive.queen),
        ] {
            assert!(!hive.service.files_by_intake(principal));
            assert!(matches!(
                hive.service.file_intake_task(principal, &request("Fix it")),
                Err(ApplicationError::NotAuthorized)
            ));
            assert!(matches!(
                hive.service.routing_roster(principal),
                Err(ApplicationError::NotAuthorized)
            ));
            assert!(matches!(
                hive.service.search_open_tasks(principal, "renewal"),
                Err(ApplicationError::NotAuthorized)
            ));
        }
    }
}
