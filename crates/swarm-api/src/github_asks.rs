//! Reads what GitHub asks of the operator, through the machine's gh login, and
//! brings it onto the board (ADR 0170).
//!
//! The operator's ruling: like Jira, Swarm notices GitHub work for them on
//! every repository their login reaches. "Mine, on every repo": pull requests
//! where their own review is requested, and issues assigned to them. Each
//! becomes a draft for Queen; a review request is routed to the repository's
//! worker, who writes review notes, and the operator submits the review.
//!
//! The credential is the one the check gate already reads CI with (ADR 0115):
//! the machine's own gh login. Swarm holds no token for this.

use std::{
    collections::HashSet,
    path::Path,
    sync::Mutex,
    time::{Duration, Instant},
};

use serde::Deserialize;
use swarm_domain::{
    AskStanding, CheckoutMatch, EndedAskEffect, GithubAsk, GithubAskKind, WorkerId, WorkerRole,
    assigned_issue_standing, github_ask_placeholder_workspace, match_checkout,
    review_request_standing,
};
use swarm_persistence::{GithubAskArrival, GithubAskFiling, TaskStore};

use crate::AppState;
use crate::gh_cli::{GhFailure, gh_api};
use crate::runtime::git_output;

/// How many asks that left the search one pass reads directly. The rest wait
/// for the next pass, longest unchecked first.
const DIRECT_READS_PER_PASS: usize = 10;
/// How long one pass may keep starting direct reads, so it ends soon after,
/// whatever GitHub is doing, and shutdown never waits long on it.
const PASS_BUDGET: Duration = Duration::from_secs(60);
/// The most results one search returns. Far above what one person is asked
/// at once; a fuller search is read as incomplete, which closes nothing.
const SEARCH_PAGE: usize = 100;
/// The pass runs every five minutes; a failure waits this long, doubling.
const FIRST_BACKOFF: Duration = Duration::from_secs(5 * 60);
/// The longest a failing pass waits before trying again.
const LONGEST_BACKOFF: Duration = Duration::from_secs(60 * 60);
/// Taken off each wait so the tick that ends it is not missed by a hair.
const TICK_SLACK: Duration = Duration::from_secs(30);

/// One search for one kind of ask.
pub(crate) struct AskSearch {
    pub(crate) asks: Vec<GithubAsk>,
    /// Whether GitHub said this is all of them. An incomplete search files
    /// what it found and closes nothing, because absence from it proves
    /// nothing.
    pub(crate) complete: bool,
}

/// GitHub, as the asks pass reads it.
pub(crate) trait AskReader {
    /// The login the machine's gh is signed in as.
    fn login(&self) -> Result<String, GhFailure>;
    /// Every open ask of one kind.
    fn search(&self, kind: GithubAskKind) -> Result<AskSearch, GhFailure>;
    /// Where one ask stands, read directly rather than inferred from a search.
    fn standing(
        &self,
        kind: GithubAskKind,
        repository: &str,
        number: u64,
        operator: &str,
    ) -> Result<AskStanding, GhFailure>;
}

/// Reads through `gh api`, as the machine's logged-in GitHub user.
pub(crate) struct GhAsks;

#[derive(Deserialize)]
struct Login {
    login: String,
}

#[derive(Deserialize)]
struct SearchPage {
    total: u64,
    incomplete: bool,
    items: Vec<SearchItem>,
}

#[derive(Deserialize)]
struct SearchItem {
    number: u64,
    title: String,
    body: Option<String>,
    author: Option<String>,
    repository_url: String,
}

#[derive(Deserialize)]
struct PullStanding {
    state: String,
    merged: bool,
    requested: Vec<String>,
}

#[derive(Deserialize)]
struct IssueStanding {
    state: String,
    assignees: Vec<String>,
}

impl AskReader for GhAsks {
    fn login(&self) -> Result<String, GhFailure> {
        // An object, not `.login`: gh's --jq prints a bare string without its
        // quotes, which is not JSON.
        gh_api::<Login>("user", "{login}").map(|user| user.login)
    }

    fn search(&self, kind: GithubAskKind) -> Result<AskSearch, GhFailure> {
        // `user-review-requested`, not `review-requested`: the operator's own
        // review, not every team they belong to. "Mine" was the ruling.
        let query = match kind {
            GithubAskKind::ReviewRequest => {
                "is:open+is:pr+user-review-requested:@me+archived:false"
            }
            GithubAskKind::AssignedIssue => "is:open+is:issue+assignee:@me+archived:false",
        };
        let page: SearchPage = gh_api(
            &format!("search/issues?q={query}&per_page={SEARCH_PAGE}&sort=created&order=asc"),
            "{total: .total_count, incomplete: .incomplete_results, items: [.items[] | \
             {number, title, body, author: .user.login, repository_url}]}",
        )?;
        Ok(asks_from(kind, page))
    }

    fn standing(
        &self,
        kind: GithubAskKind,
        repository: &str,
        number: u64,
        operator: &str,
    ) -> Result<AskStanding, GhFailure> {
        Ok(match kind {
            GithubAskKind::ReviewRequest => {
                let pull: PullStanding = gh_api(
                    &format!("repos/{repository}/pulls/{number}"),
                    "{state, merged, requested: [.requested_reviewers[].login]}",
                )?;
                review_request_standing(&pull.state, pull.merged, &pull.requested, operator)
            }
            GithubAskKind::AssignedIssue => {
                let issue: IssueStanding = gh_api(
                    &format!("repos/{repository}/issues/{number}"),
                    "{state, assignees: [.assignees[].login]}",
                )?;
                assigned_issue_standing(&issue.state, &issue.assignees, operator)
            }
        })
    }
}

/// The asks in one search page. An item that does not name a real ask makes
/// the search incomplete rather than disappearing from it.
fn asks_from(kind: GithubAskKind, page: SearchPage) -> AskSearch {
    let mut complete = !page.incomplete && page.total <= page.items.len() as u64;
    let mut asks = Vec::with_capacity(page.items.len());
    for item in page.items {
        let ask = item
            .repository_url
            .strip_prefix("https://api.github.com/repos/")
            .and_then(|repository| {
                GithubAsk::new(
                    kind,
                    repository,
                    item.number,
                    &item.title,
                    item.author.as_deref(),
                    item.body.as_deref().unwrap_or_default(),
                )
            });
        match ask {
            Some(ask) => asks.push(ask),
            None => complete = false,
        }
    }
    AskSearch { asks, complete }
}

/// A routable worker's checkout, and the GitHub repository its origin names.
pub(crate) struct Checkout {
    pub(crate) worker: WorkerId,
    pub(crate) name: String,
    pub(crate) workspace: String,
    pub(crate) repository: String,
}

/// The routable workers whose checkout's origin is on GitHub. Read only when
/// a pass has something to file: one `git` call per worker.
fn routable_checkouts(store: &TaskStore) -> Vec<Checkout> {
    let Ok(workers) = store.list_worker_profiles() else {
        return Vec::new();
    };
    workers
        .into_iter()
        .filter(|worker| worker.role == WorkerRole::Worker && !worker.ephemeral)
        .filter_map(|worker| {
            let repository = git_output(
                Path::new(&worker.workspace),
                &["remote", "get-url", "origin"],
            )
            .and_then(|remote| crate::ci_checks::github_repository(&remote))?;
            Some(Checkout {
                worker: worker.id,
                name: worker.name,
                workspace: worker.workspace,
                repository,
            })
        })
        .collect()
}

/// When the next pass may read GitHub after failures. Owned by `AppState`,
/// bounded by `LONGEST_BACKOFF`, and reset by the first pass that succeeds.
///
/// AN OPTIMISATION, NEVER A TRUTH: a pass that runs reads GitHub afresh and
/// decides from what it reads, whatever the wait was.
#[derive(Default)]
pub(crate) struct AskBackoff {
    state: Mutex<BackoffState>,
}

#[derive(Default)]
struct BackoffState {
    failures: u32,
    resume_at: Option<Instant>,
}

impl AskBackoff {
    fn due(&self, now: Instant) -> bool {
        self.state
            .lock()
            .map_or(true, |state| state.resume_at.is_none_or(|at| now >= at))
    }

    fn failed(&self, now: Instant) {
        if let Ok(mut state) = self.state.lock() {
            state.failures = state.failures.saturating_add(1);
            let doubled = FIRST_BACKOFF
                .checked_mul(1 << state.failures.saturating_sub(1).min(6))
                .unwrap_or(LONGEST_BACKOFF);
            state.resume_at = Some(now + doubled.min(LONGEST_BACKOFF).saturating_sub(TICK_SLACK));
        }
    }

    fn succeeded(&self) {
        if let Ok(mut state) = self.state.lock() {
            *state = BackoffState::default();
        }
    }
}

/// What one pass did, for its log line and its tests.
#[derive(Debug, Default, Eq, PartialEq)]
pub(crate) struct AskPass {
    pub(crate) waited: bool,
    pub(crate) filed: usize,
    pub(crate) adopted: usize,
    pub(crate) asked_again: usize,
    pub(crate) closed: usize,
    pub(crate) told: usize,
}

impl AskPass {
    const fn changed(&self) -> bool {
        self.filed + self.adopted + self.asked_again + self.closed + self.told > 0
    }
}

impl AppState {
    /// Brings what GitHub asks of the operator onto the board, and settles
    /// what it has stopped asking.
    ///
    /// Synchronous on purpose, like the check gate's reads: gh and git are
    /// subprocesses and the store is rusqlite. Its caller puts it on a
    /// blocking thread. Silent when this machine has no gh login, which for
    /// most Hives is the ordinary case; loud only once it has worked and then
    /// stops (see `GithubAskReading::lapsed`).
    pub fn read_github_asks(&self) {
        let Some(store) = self.task_store.as_ref() else {
            return;
        };
        let pass = read_asks(
            store,
            &GhAsks,
            &|| routable_checkouts(store),
            &self.github_asks,
            Instant::now(),
        );
        if pass.changed() {
            tracing::info!(?pass, "GitHub asks of the operator reached the board");
            self.control_room_notify.notify_waiters();
        }
    }
}

/// One pass: search, file what is new, then read directly each standing ask
/// the search no longer lists.
pub(crate) fn read_asks(
    store: &TaskStore,
    reader: &dyn AskReader,
    checkouts: &dyn Fn() -> Vec<Checkout>,
    backoff: &AskBackoff,
    now: Instant,
) -> AskPass {
    let mut pass = AskPass::default();
    if !backoff.due(now) {
        pass.waited = true;
        return pass;
    }
    let fail = |failure: &GhFailure| {
        let why = failure.describe();
        let worked_before = store
            .github_ask_reading()
            .is_ok_and(|reading| reading.last_read_at.is_some());
        if worked_before {
            tracing::warn!(%why, "GitHub's asks of the operator could not be read");
        } else {
            tracing::debug!(%why, "GitHub's asks of the operator could not be read");
        }
        if let Err(error) = store.record_github_ask_reading(Err(&why)) {
            tracing::warn!(%error, "a failed read of GitHub's asks could not be recorded");
        }
        backoff.failed(now);
    };
    let operator = match reader.login() {
        Ok(login) => login,
        Err(failure) => {
            fail(&failure);
            return pass;
        }
    };
    let mut asks = Vec::new();
    let mut complete = true;
    for kind in [GithubAskKind::ReviewRequest, GithubAskKind::AssignedIssue] {
        match reader.search(kind) {
            Ok(search) => {
                complete &= search.complete;
                asks.extend(search.asks);
            }
            Err(failure) => {
                fail(&failure);
                return pass;
            }
        }
    }
    file_asks(store, &asks, checkouts, &mut pass);
    if complete {
        let present = asks.iter().map(GithubAsk::url).collect::<HashSet<_>>();
        if let Err(failure) = settle_ended(store, reader, &operator, &present, &mut pass) {
            fail(&failure);
            return pass;
        }
    }
    if let Err(error) = store.record_github_ask_reading(Ok(())) {
        tracing::warn!(%error, "a read of GitHub's asks could not be recorded");
    }
    backoff.succeeded();
    pass
}

fn file_asks(
    store: &TaskStore,
    asks: &[GithubAsk],
    checkouts: &dyn Fn() -> Vec<Checkout>,
    pass: &mut AskPass,
) {
    let urls = asks.iter().map(GithubAsk::url).collect::<Vec<_>>();
    let new = match store.github_asks_not_standing(&urls) {
        Ok(new) => new.into_iter().collect::<HashSet<_>>(),
        Err(error) => {
            tracing::warn!(%error, "GitHub's asks could not be checked against the board");
            return;
        }
    };
    if new.is_empty() {
        return;
    }
    let checkouts = checkouts();
    let named = checkouts
        .iter()
        .map(|checkout| {
            (
                checkout.name.clone(),
                checkout.workspace.clone(),
                checkout.repository.clone(),
            )
        })
        .collect::<Vec<_>>();
    for ask in asks.iter().filter(|ask| new.contains(&ask.url())) {
        let routing = match_checkout(ask.repository(), &named);
        let (workspace, recommended_worker) = match &routing {
            CheckoutMatch::One { worker, workspace } => (
                workspace.clone(),
                checkouts
                    .iter()
                    .find(|checkout| &checkout.name == worker && &checkout.workspace == workspace)
                    .map(|checkout| checkout.worker),
            ),
            CheckoutMatch::Nobody | CheckoutMatch::Several(_) => {
                (github_ask_placeholder_workspace(ask.repository()), None)
            }
        };
        let description = ask.task_description(&routing);
        let filing = GithubAskFiling {
            ask,
            description: &description,
            workspace: &workspace,
            repository_named: recommended_worker.is_some(),
            recommended_worker,
        };
        match store.receive_github_ask(&filing) {
            Ok(GithubAskArrival::Filed(_)) => pass.filed += 1,
            Ok(GithubAskArrival::Adopted(_)) => pass.adopted += 1,
            Ok(GithubAskArrival::AskedAgain(_)) => pass.asked_again += 1,
            Ok(GithubAskArrival::Known) => {}
            Err(error) => {
                tracing::warn!(%error, ask = %ask.url(), "a GitHub ask could not reach the board");
            }
        }
    }
}

/// Reads directly each standing ask the search no longer lists, and settles
/// the ones GitHub confirms have ended.
///
/// ⚠️ ONLY A DIRECT READ ENDS AN ASK. Absence from a search is search lag as
/// often as it is an ending, and an answer that could not be read (a
/// repository the login lost, a 404) is not an ending either: those asks are
/// left exactly as they were and tried again on later passes.
fn settle_ended(
    store: &TaskStore,
    reader: &dyn AskReader,
    operator: &str,
    present: &HashSet<String>,
    pass: &mut AskPass,
) -> Result<(), GhFailure> {
    let standing = match store.standing_github_asks() {
        Ok(standing) => standing,
        Err(error) => {
            tracing::warn!(%error, "standing GitHub asks could not be listed");
            return Ok(());
        }
    };
    let started = Instant::now();
    for ask in standing
        .into_iter()
        .filter(|ask| !present.contains(&ask.url))
        .take(DIRECT_READS_PER_PASS)
    {
        if started.elapsed() >= PASS_BUDGET {
            break;
        }
        let recorded = match reader.standing(ask.kind, &ask.repository, ask.number, operator) {
            Ok(AskStanding::Stands) => store.record_github_ask_checked(&ask.url),
            Ok(AskStanding::Ended(ended)) => {
                store
                    .record_github_ask_ended(&ask.url, ended)
                    .map(|effect| match effect {
                        EndedAskEffect::Close => pass.closed += 1,
                        EndedAskEffect::Note => pass.told += 1,
                        EndedAskEffect::Nothing => {}
                    })
            }
            Err(GhFailure::NotFound) => {
                tracing::debug!(ask = %ask.url, "a GitHub ask could not be seen; left as it was");
                store.record_github_ask_checked(&ask.url)
            }
            Err(failure) => return Err(failure),
        };
        if let Err(error) = recorded {
            tracing::warn!(%error, ask = %ask.url, "a GitHub ask's standing could not be recorded");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::cell::{Cell, RefCell};

    use swarm_domain::{AskEnded, ProviderKind, TaskState};
    use swarm_persistence::TaskStore;

    use super::*;

    /// GitHub as a test says it is.
    struct Fake {
        login: RefCell<Result<String, GhFailure>>,
        reviews: RefCell<Result<(Vec<GithubAsk>, bool), GhFailure>>,
        issues: RefCell<Result<(Vec<GithubAsk>, bool), GhFailure>>,
        standing: RefCell<Result<AskStanding, GhFailure>>,
        calls: Cell<usize>,
        direct_reads: Cell<usize>,
    }

    impl Fake {
        fn asking(reviews: Vec<GithubAsk>) -> Self {
            Self {
                login: RefCell::new(Ok("miopea".into())),
                reviews: RefCell::new(Ok((reviews, true))),
                issues: RefCell::new(Ok((Vec::new(), true))),
                standing: RefCell::new(Ok(AskStanding::Stands)),
                calls: Cell::new(0),
                direct_reads: Cell::new(0),
            }
        }
    }

    impl AskReader for Fake {
        fn login(&self) -> Result<String, GhFailure> {
            self.calls.set(self.calls.get() + 1);
            self.login.borrow().clone()
        }

        fn search(&self, kind: GithubAskKind) -> Result<AskSearch, GhFailure> {
            self.calls.set(self.calls.get() + 1);
            let found = match kind {
                GithubAskKind::ReviewRequest => self.reviews.borrow().clone(),
                GithubAskKind::AssignedIssue => self.issues.borrow().clone(),
            };
            found.map(|(asks, complete)| AskSearch { asks, complete })
        }

        fn standing(
            &self,
            _kind: GithubAskKind,
            _repository: &str,
            _number: u64,
            operator: &str,
        ) -> Result<AskStanding, GhFailure> {
            assert_eq!(
                operator, "miopea",
                "a direct read judges for the signed-in login"
            );
            self.calls.set(self.calls.get() + 1);
            self.direct_reads.set(self.direct_reads.get() + 1);
            self.standing.borrow().clone()
        }
    }

    fn review(number: u64) -> GithubAsk {
        GithubAsk::new(
            GithubAskKind::ReviewRequest,
            "acme/petal",
            number,
            "Make the petals bloom",
            Some("sorrel"),
            "Fixes the stem.",
        )
        .unwrap()
    }

    fn no_checkouts() -> Vec<Checkout> {
        Vec::new()
    }

    fn only_task(store: &TaskStore) -> swarm_domain::Task {
        let tasks = store.list_tasks().unwrap();
        assert_eq!(tasks.len(), 1, "{tasks:?}");
        tasks.into_iter().next().unwrap()
    }

    #[test]
    fn a_review_request_arrives_routed_to_the_one_checkout_and_only_once() {
        let store = TaskStore::in_memory().unwrap();
        store.ensure_queen("/workspace/queen").unwrap();
        let petal = store
            .create_worker(
                "Petal",
                ProviderKind::ClaudeCode,
                "/workspace/petal",
                false,
                1,
            )
            .unwrap();
        let checkouts = || {
            vec![
                Checkout {
                    worker: petal.id,
                    name: "Petal".into(),
                    workspace: "/workspace/petal".into(),
                    repository: "acme/petal".into(),
                },
                Checkout {
                    worker: WorkerId::new(),
                    name: "Ledger".into(),
                    workspace: "/workspace/ledger".into(),
                    repository: "acme/ledger".into(),
                },
            ]
        };
        let github = Fake::asking(vec![review(42)]);
        let backoff = AskBackoff::default();
        let now = Instant::now();

        let pass = read_asks(&store, &github, &checkouts, &backoff, now);
        assert_eq!(pass.filed, 1);
        let task = only_task(&store);
        assert_eq!(task.state, TaskState::Draft);
        assert_eq!(task.title, "Review acme/petal#42: Make the petals bloom");
        assert_eq!(task.workspace, "/workspace/petal");
        assert!(
            task.description
                .contains("Do not approve, comment or request changes")
        );
        let intake = store
            .intake_for_tasks(&[task.id])
            .unwrap()
            .remove(&task.id)
            .unwrap();
        assert_eq!(intake.recommended_worker.unwrap().id, petal.id);
        assert!(intake.repository_named);

        let again = read_asks(&store, &github, &checkouts, &backoff, now);
        assert_eq!(
            again,
            AskPass::default(),
            "the same ask files nothing twice"
        );
        assert_eq!(store.list_tasks().unwrap().len(), 1);
        assert!(store.github_ask_reading().unwrap().last_read_at.is_some());
    }

    #[test]
    fn several_checkouts_suggest_nobody_and_hold_the_placeholder() {
        let store = TaskStore::in_memory().unwrap();
        let checkouts = || {
            ["Ash", "Sorrel"]
                .into_iter()
                .map(|name| Checkout {
                    worker: WorkerId::new(),
                    name: name.into(),
                    workspace: format!("/workspace/{name}"),
                    repository: "acme/petal".into(),
                })
                .collect()
        };
        read_asks(
            &store,
            &Fake::asking(vec![review(42)]),
            &checkouts,
            &AskBackoff::default(),
            Instant::now(),
        );
        let task = only_task(&store);
        assert_eq!(task.workspace, "github://acme/petal");
        assert!(
            task.description
                .contains("Workers with a checkout of acme/petal: Ash, Sorrel.")
        );
        let intake = store
            .intake_for_tasks(&[task.id])
            .unwrap()
            .remove(&task.id)
            .unwrap();
        assert!(intake.recommended_worker.is_none());
        assert!(!intake.repository_named);
    }

    #[test]
    fn an_assigned_issue_arrives_beside_review_requests() {
        let store = TaskStore::in_memory().unwrap();
        let github = Fake::asking(Vec::new());
        *github.issues.borrow_mut() = Ok((
            vec![
                GithubAsk::new(
                    GithubAskKind::AssignedIssue,
                    "acme/stem",
                    7,
                    "Stem snaps",
                    None,
                    "",
                )
                .unwrap(),
            ],
            true,
        ));
        let pass = read_asks(
            &store,
            &github,
            &no_checkouts,
            &AskBackoff::default(),
            Instant::now(),
        );
        assert_eq!(pass.filed, 1);
        let task = only_task(&store);
        assert_eq!(task.title, "acme/stem#7: Stem snaps");
        assert_eq!(task.workspace, "github://acme/stem");
    }

    #[test]
    fn a_withdrawn_request_closes_its_draft_only_once_a_direct_read_says_so() {
        let store = TaskStore::in_memory().unwrap();
        let github = Fake::asking(vec![review(42)]);
        let backoff = AskBackoff::default();
        let now = Instant::now();
        read_asks(&store, &github, &no_checkouts, &backoff, now);

        // Gone from the search, but GitHub still asks when asked directly:
        // search lag, not an ending.
        *github.reviews.borrow_mut() = Ok((Vec::new(), true));
        let lagging = read_asks(&store, &github, &no_checkouts, &backoff, now);
        assert_eq!(github.direct_reads.get(), 1);
        assert_eq!(lagging.closed, 0);
        assert_eq!(only_task(&store).state, TaskState::Draft);

        *github.standing.borrow_mut() = Ok(AskStanding::Ended(AskEnded::ReviewNoLongerRequested));
        let ended = read_asks(&store, &github, &no_checkouts, &backoff, now);
        assert_eq!(ended.closed, 1);
        assert_eq!(only_task(&store).state, TaskState::Abandoned);
        // Settled: not read again.
        read_asks(&store, &github, &no_checkouts, &backoff, now);
        assert_eq!(github.direct_reads.get(), 2);
    }

    #[test]
    fn an_incomplete_search_ends_nothing() {
        let store = TaskStore::in_memory().unwrap();
        let github = Fake::asking(vec![review(42)]);
        let backoff = AskBackoff::default();
        let now = Instant::now();
        read_asks(&store, &github, &no_checkouts, &backoff, now);

        *github.reviews.borrow_mut() = Ok((Vec::new(), false));
        *github.standing.borrow_mut() = Ok(AskStanding::Ended(AskEnded::Closed));
        read_asks(&store, &github, &no_checkouts, &backoff, now);
        assert_eq!(
            github.direct_reads.get(),
            0,
            "an incomplete search proves no absence"
        );
        assert_eq!(only_task(&store).state, TaskState::Draft);
    }

    #[test]
    fn a_repository_the_login_lost_ends_nothing() {
        let store = TaskStore::in_memory().unwrap();
        let github = Fake::asking(vec![review(42)]);
        let backoff = AskBackoff::default();
        let now = Instant::now();
        read_asks(&store, &github, &no_checkouts, &backoff, now);

        *github.reviews.borrow_mut() = Ok((Vec::new(), true));
        *github.standing.borrow_mut() = Err(GhFailure::NotFound);
        let pass = read_asks(&store, &github, &no_checkouts, &backoff, now);
        assert_eq!(pass.closed, 0);
        assert_eq!(only_task(&store).state, TaskState::Draft);
        assert_eq!(store.standing_github_asks().unwrap().len(), 1);
        assert_eq!(
            store.github_ask_reading().unwrap().lapsed(),
            None,
            "one unseen ask is not GitHub failing"
        );

        // Access comes back, and the ending is read and settled.
        *github.standing.borrow_mut() = Ok(AskStanding::Ended(AskEnded::Merged));
        assert_eq!(
            read_asks(&store, &github, &no_checkouts, &backoff, now).closed,
            1
        );
    }

    #[test]
    fn a_revoked_login_is_reported_once_it_had_worked_waits_and_recovers() {
        let store = TaskStore::in_memory().unwrap();
        let github = Fake::asking(vec![review(42)]);
        let backoff = AskBackoff::default();
        let start = Instant::now();
        read_asks(&store, &github, &no_checkouts, &backoff, start);

        *github.login.borrow_mut() = Err(GhFailure::SignedOut);
        let failed_at = start + Duration::from_secs(300);
        read_asks(&store, &github, &no_checkouts, &backoff, failed_at);
        assert_eq!(
            store.github_ask_reading().unwrap().lapsed(),
            Some(swarm_domain::CI_GH_SIGNED_OUT)
        );
        assert_eq!(
            only_task(&store).state,
            TaskState::Draft,
            "a failed read ends nothing"
        );

        let calls = github.calls.get();
        let early = read_asks(
            &store,
            &github,
            &no_checkouts,
            &backoff,
            failed_at + Duration::from_secs(60),
        );
        assert!(early.waited);
        assert_eq!(
            github.calls.get(),
            calls,
            "a waiting pass does not call GitHub"
        );

        *github.login.borrow_mut() = Ok("miopea".into());
        let next_tick = failed_at + Duration::from_secs(300);
        assert!(!read_asks(&store, &github, &no_checkouts, &backoff, next_tick).waited);
        assert_eq!(store.github_ask_reading().unwrap().lapsed(), None);
    }

    #[test]
    fn a_hive_that_never_read_github_stays_quiet() {
        let store = TaskStore::in_memory().unwrap();
        let github = Fake::asking(Vec::new());
        *github.login.borrow_mut() = Err(GhFailure::Missing);
        read_asks(
            &store,
            &github,
            &no_checkouts,
            &AskBackoff::default(),
            Instant::now(),
        );
        assert_eq!(store.github_ask_reading().unwrap().lapsed(), None);
    }

    #[test]
    fn a_rate_limit_mid_settle_records_the_failure_and_waits() {
        let store = TaskStore::in_memory().unwrap();
        let github = Fake::asking(vec![review(42)]);
        let backoff = AskBackoff::default();
        let now = Instant::now();
        read_asks(&store, &github, &no_checkouts, &backoff, now);

        *github.reviews.borrow_mut() = Ok((Vec::new(), true));
        *github.standing.borrow_mut() =
            Err(GhFailure::RateLimited("gh: API rate limit exceeded".into()));
        read_asks(&store, &github, &no_checkouts, &backoff, now);
        assert_eq!(
            store.github_ask_reading().unwrap().lapsed(),
            Some("GitHub could not be read: gh: API rate limit exceeded")
        );
        assert_eq!(only_task(&store).state, TaskState::Draft);
        assert!(read_asks(&store, &github, &no_checkouts, &backoff, now).waited);
    }

    #[test]
    fn a_failing_read_waits_longer_each_time_up_to_an_hour() {
        let backoff = AskBackoff::default();
        let start = Instant::now();
        let mut expected = Vec::new();
        for failures in 1..=8_u32 {
            backoff.failed(start);
            let wait = backoff.state.lock().unwrap().resume_at.unwrap() - start;
            expected.push((failures, wait.as_secs()));
        }
        assert_eq!(
            expected,
            vec![
                (1, 270),
                (2, 570),
                (3, 1_170),
                (4, 2_370),
                (5, 3_570),
                (6, 3_570),
                (7, 3_570),
                (8, 3_570),
            ]
        );
        backoff.succeeded();
        assert!(backoff.due(start));
    }

    #[test]
    fn a_search_item_that_names_no_real_ask_makes_the_search_incomplete() {
        let item = |repository_url: &str| SearchItem {
            number: 1,
            title: "t".into(),
            body: None,
            author: Some("ash".into()),
            repository_url: repository_url.into(),
        };
        let whole = asks_from(
            GithubAskKind::ReviewRequest,
            SearchPage {
                total: 1,
                incomplete: false,
                items: vec![item("https://api.github.com/repos/acme/petal")],
            },
        );
        assert!(whole.complete);
        assert_eq!(whole.asks[0].repository(), "acme/petal");
        let odd = asks_from(
            GithubAskKind::ReviewRequest,
            SearchPage {
                total: 2,
                incomplete: false,
                items: vec![
                    item("https://api.github.com/repos/acme/petal"),
                    item("https://ghe.example/api/v3/repos/acme/petal"),
                ],
            },
        );
        assert!(!odd.complete);
        assert_eq!(odd.asks.len(), 1);
        let more_than_a_page = asks_from(
            GithubAskKind::ReviewRequest,
            SearchPage {
                total: 101,
                incomplete: false,
                items: vec![item("https://api.github.com/repos/acme/petal")],
            },
        );
        assert!(!more_than_a_page.complete);
    }

    /// The real reader against real GitHub. CI has no gh login, and a test that
    /// needs the network is not a unit test. Run with `--ignored` on a machine
    /// that has one.
    #[test]
    #[ignore = "reads real GitHub through this machine's gh login"]
    fn gh_asks_reads_real_github() {
        let operator = GhAsks.login().expect("this machine's gh login answers");
        assert!(!operator.is_empty());
        for kind in [GithubAskKind::ReviewRequest, GithubAskKind::AssignedIssue] {
            let search = GhAsks.search(kind).expect("the search answers");
            assert!(
                search.complete,
                "{kind}: fewer than a page of asks is all of them"
            );
        }
        // Merged in release 1.18.5, and stays merged.
        assert_eq!(
            GhAsks.standing(
                GithubAskKind::ReviewRequest,
                "miopea/swarm-next",
                147,
                &operator
            ),
            Ok(AskStanding::Ended(swarm_domain::AskEnded::Merged))
        );
        assert!(
            GhAsks
                .standing(
                    GithubAskKind::AssignedIssue,
                    "miopea/swarm-next",
                    73,
                    &operator
                )
                .is_ok(),
            "an issue's fields read in the shape expected"
        );
        assert_eq!(
            GhAsks.standing(
                GithubAskKind::AssignedIssue,
                "miopea/no-such-repository-here",
                1,
                &operator
            ),
            Err(GhFailure::NotFound)
        );

        // A whole pass, into a board of its own: every ask files one draft,
        // and the next pass files none.
        let asked = [GithubAskKind::ReviewRequest, GithubAskKind::AssignedIssue]
            .into_iter()
            .map(|kind| GhAsks.search(kind).unwrap().asks.len())
            .sum::<usize>();
        let store = TaskStore::in_memory().unwrap();
        let backoff = AskBackoff::default();
        let first = read_asks(&store, &GhAsks, &no_checkouts, &backoff, Instant::now());
        assert_eq!(first.filed, asked, "{first:?}");
        for task in store.list_tasks().unwrap() {
            eprintln!("{} | {}", task.title, task.workspace);
        }
        let second = read_asks(&store, &GhAsks, &no_checkouts, &backoff, Instant::now());
        assert_eq!(second, AskPass::default());
        assert!(store.github_ask_reading().unwrap().last_read_at.is_some());
    }

    #[tokio::test]
    async fn health_says_when_asks_stop_arriving_after_they_had_arrived() {
        use axum::{body::Body, http::Request};
        use tower::ServiceExt as _;

        let store = TaskStore::in_memory().unwrap();
        store.record_github_ask_reading(Ok(())).unwrap();
        store
            .record_github_ask_reading(Err(swarm_domain::CI_GH_SIGNED_OUT))
            .unwrap();
        let response = crate::router(AppState::default().with_task_store(store))
            .oneshot(
                Request::builder()
                    .uri("/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let body = axum::body::to_bytes(response.into_body(), 1024 * 1024)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        let degraded = json["degraded"].as_array().unwrap();
        assert_eq!(degraded.len(), 1, "{json}");
        assert_eq!(
            degraded[0]["subsystem"],
            "GitHub review requests and assigned issues"
        );
        assert_eq!(
            degraded[0]["reason"],
            format!(
                "They have stopped arriving: {}",
                swarm_domain::CI_GH_SIGNED_OUT
            )
        );
    }
}
