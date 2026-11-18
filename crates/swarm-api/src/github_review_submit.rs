//! Carries out the operator's press on an owed review (ADR 0176): post the
//! review under their GitHub login, then merge when GitHub allows it.
//!
//! Every step is a recorded move on the submission, made BEFORE the call it
//! leads to, so a restart in the middle leaves a known state. A call whose
//! outcome is unknown is settled by reading GitHub back, never by sending it
//! again: a second review under the operator's name, or a second merge, is
//! worse than a slow answer.
//!
//! The merge rule lives here, beside the writer, rather than in
//! `swarm_domain`. Changing that crate rebuilds every worker's engine, and this
//! rule concerns the API alone.

use serde::Deserialize;
use swarm_domain::{CiOutcome, classify_checks};
use swarm_persistence::{
    GithubReviewDispatch, GithubReviewSubmissionState as State, GithubReviewVerdict, TaskStore,
    TaskStoreError,
};

use crate::ci_checks::CheckReader as _;
use crate::gh_cli::{GhWriteFailure, gh_api, gh_api_write};

/// A pull request, as the merge rule reads it.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize)]
pub(crate) struct PullFacts {
    pub(crate) open: bool,
    pub(crate) merged: bool,
    pub(crate) draft: bool,
    pub(crate) head_sha: String,
    /// `None` while GitHub is still working it out.
    pub(crate) mergeable: Option<bool>,
}

/// The merge methods a repository allows.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Deserialize)]
pub(crate) struct MergeMethods {
    pub(crate) merge_commit: bool,
    pub(crate) squash: bool,
    pub(crate) rebase: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum MergeMethod {
    Merge,
    Squash,
    Rebase,
}

impl MergeMethod {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Merge => "merge",
            Self::Squash => "squash",
            Self::Rebase => "rebase",
        }
    }
}

/// A review on a pull request, as read back.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize)]
pub(crate) struct PostedReview {
    pub(crate) id: i64,
    pub(crate) author: Option<String>,
    pub(crate) commit_id: Option<String>,
    /// `APPROVED`, `CHANGES_REQUESTED`, `COMMENTED` and so on.
    pub(crate) state: String,
}

/// GitHub, as a press on an owed review uses it.
pub(crate) trait ReviewGithub {
    /// The login reviews post under.
    fn login(&self) -> Result<String, String>;
    /// Posts a review pinned to `commit`; returns its id.
    fn post_review(
        &self,
        repository: &str,
        number: u64,
        verdict: GithubReviewVerdict,
        body: &str,
        commit: &str,
    ) -> Result<i64, GhWriteFailure>;
    /// The pull request's reviews, to settle a post whose outcome is unknown.
    fn reviews(&self, repository: &str, number: u64) -> Result<Vec<PostedReview>, String>;
    fn pull(&self, repository: &str, number: u64) -> Result<PullFacts, String>;
    /// What CI says about one commit, judged by the check gate's rule, or
    /// `None` when it has no checks.
    fn checks(&self, repository: &str, sha: &str) -> Result<Option<CiOutcome>, String>;
    fn merge_methods(&self, repository: &str) -> Result<MergeMethods, String>;
    /// Merges only if the head is still `sha`.
    fn merge(
        &self,
        repository: &str,
        number: u64,
        sha: &str,
        method: MergeMethod,
    ) -> Result<(), GhWriteFailure>;
}

/// Through `gh api`, as the machine's logged-in GitHub user.
pub(crate) struct GhReviews;

#[derive(Deserialize)]
struct Id {
    id: i64,
}

#[derive(Deserialize)]
struct Login {
    login: String,
}

#[derive(Deserialize)]
struct RawPull {
    state: String,
    merged: bool,
    draft: Option<bool>,
    head: String,
    mergeable: Option<bool>,
}

#[derive(Deserialize)]
struct RawMethods {
    merge: Option<bool>,
    squash: Option<bool>,
    rebase: Option<bool>,
}

#[derive(Deserialize)]
struct Merged {
    merged: bool,
}

const REVIEW_PAGE: usize = 100;
const REVIEW_PAGES: u32 = 10;

/// GitHub lists a pull request's reviews oldest first, so the one just posted
/// is on the last page. Every page is read, up to a bound. Past the bound the
/// answer is unknown, not "it is not there", so a review is never posted twice
/// because Swarm stopped reading too early.
fn every_review(
    mut read_page: impl FnMut(u32) -> Result<Vec<PostedReview>, String>,
) -> Result<Vec<PostedReview>, String> {
    let mut reviews = Vec::new();
    for page in 1..=REVIEW_PAGES {
        let batch = read_page(page)?;
        let last = batch.len() < REVIEW_PAGE;
        reviews.extend(batch);
        if last {
            return Ok(reviews);
        }
    }
    Err(format!(
        "it has more than {} reviews, too many to read back",
        REVIEW_PAGE * REVIEW_PAGES as usize
    ))
}

impl ReviewGithub for GhReviews {
    fn login(&self) -> Result<String, String> {
        gh_api::<Login>("user", "{login}")
            .map(|user| user.login)
            .map_err(|failure| failure.describe())
    }

    fn post_review(
        &self,
        repository: &str,
        number: u64,
        verdict: GithubReviewVerdict,
        body: &str,
        commit: &str,
    ) -> Result<i64, GhWriteFailure> {
        let event = match verdict {
            GithubReviewVerdict::Approve => "APPROVE",
            GithubReviewVerdict::RequestChanges => "REQUEST_CHANGES",
        };
        let mut request = serde_json::json!({ "event": event, "commit_id": commit });
        if !body.trim().is_empty() {
            request["body"] = serde_json::Value::String(body.to_owned());
        }
        gh_api_write::<Id>(
            "POST",
            &format!("repos/{repository}/pulls/{number}/reviews"),
            &request,
            "{id}",
        )
        .map(|review| review.id)
    }

    fn reviews(&self, repository: &str, number: u64) -> Result<Vec<PostedReview>, String> {
        every_review(|page| {
            gh_api(
                &format!(
                    "repos/{repository}/pulls/{number}/reviews?per_page={REVIEW_PAGE}&page={page}"
                ),
                "[.[] | {id, author: .user.login, commit_id, state}]",
            )
            .map_err(|failure| failure.describe())
        })
    }

    fn pull(&self, repository: &str, number: u64) -> Result<PullFacts, String> {
        let raw: RawPull = gh_api(
            &format!("repos/{repository}/pulls/{number}"),
            "{state, merged, draft, head: .head.sha, mergeable}",
        )
        .map_err(|failure| failure.describe())?;
        Ok(PullFacts {
            open: raw.state == "open",
            merged: raw.merged,
            draft: raw.draft.unwrap_or(false),
            head_sha: raw.head,
            mergeable: raw.mergeable,
        })
    }

    fn checks(&self, repository: &str, sha: &str) -> Result<Option<CiOutcome>, String> {
        let (runs, statuses) = crate::ci_checks::GhCli.read(repository, sha)?;
        Ok(classify_checks(&runs, &statuses))
    }

    fn merge_methods(&self, repository: &str) -> Result<MergeMethods, String> {
        let raw: RawMethods = gh_api(
            &format!("repos/{repository}"),
            "{merge: .allow_merge_commit, squash: .allow_squash_merge, rebase: .allow_rebase_merge}",
        )
        .map_err(|failure| failure.describe())?;
        Ok(MergeMethods {
            merge_commit: raw.merge.unwrap_or(false),
            squash: raw.squash.unwrap_or(false),
            rebase: raw.rebase.unwrap_or(false),
        })
    }

    fn merge(
        &self,
        repository: &str,
        number: u64,
        sha: &str,
        method: MergeMethod,
    ) -> Result<(), GhWriteFailure> {
        let merged: Merged = gh_api_write(
            "PUT",
            &format!("repos/{repository}/pulls/{number}/merge"),
            &serde_json::json!({ "sha": sha, "merge_method": method.as_str() }),
            "{merged}",
        )?;
        if merged.merged {
            Ok(())
        } else {
            Err(GhWriteFailure::Refused("GitHub did not merge it".into()))
        }
    }
}

/// Whether an approved pull request may be merged now, and how; or why not,
/// in words that finish "Not merged: ".
///
/// ⚠️ NOTHING UNREAD IS MERGED. The head must still be the commit the worker
/// read. And nothing unchecked is merged: at least one check, every one passed,
/// by the check gate's own rule (ADR 0115). The machine's login may be an
/// administrator whom GitHub would let past a failing check, so this does not
/// leave that to GitHub.
pub(crate) fn merge_decision(
    pull: &PullFacts,
    reviewed_head: &str,
    checks: Option<&CiOutcome>,
    methods: MergeMethods,
) -> Result<MergeMethod, String> {
    if pull.merged {
        return Err("it is already merged".into());
    }
    if !pull.open {
        return Err("it is closed".into());
    }
    if pull.draft {
        return Err(
            "it is still a draft. Merge it on GitHub once its author marks it ready".into(),
        );
    }
    if !pull.head_sha.eq_ignore_ascii_case(reviewed_head) {
        return Err(
            "commits were pushed after the review was written, and nobody has read them".into(),
        );
    }
    match checks {
        None => {
            return Err("it has no checks, and Swarm does not merge what nothing checked".into());
        }
        Some(CiOutcome::Passed) => {}
        Some(CiOutcome::Failed { failing }) => {
            return Err(format!("checks failed: {}", failing.join(", ")));
        }
        Some(CiOutcome::Pending { waiting }) => {
            return Err(format!("checks are still running: {}", waiting.join(", ")));
        }
        Some(CiOutcome::Unreadable { why }) => {
            return Err(format!("its checks could not be read: {why}"));
        }
    }
    // Unknown is left to GitHub: it works this out lazily, often just after a
    // review, and the merge call refuses in its own words when it cannot.
    if pull.mergeable == Some(false) {
        return Err(
            "GitHub says it cannot be merged, because of a conflict or a rule on the branch".into(),
        );
    }
    if methods.merge_commit {
        Ok(MergeMethod::Merge)
    } else if methods.squash {
        Ok(MergeMethod::Squash)
    } else if methods.rebase {
        Ok(MergeMethod::Rebase)
    } else {
        Err("the repository allows no merge method Swarm can use".into())
    }
}

/// Carries one submission as far as it can go now.
///
/// # Errors
/// Returns a persistence error; GitHub's answers are recorded, not returned.
pub(crate) fn carry_out(
    store: &TaskStore,
    github: &dyn ReviewGithub,
    id: i64,
    now: i64,
) -> Result<(), TaskStoreError> {
    let Some(submission) = store.github_review_submission(id)? else {
        return Ok(());
    };
    match submission.state {
        State::Queued => post(store, github, &submission, now),
        State::PostUncertain => settle_post(store, github, &submission, now),
        State::AwaitingMerge => merge(store, github, &submission, now),
        State::MergeUncertain => settle_merge(store, github, &submission, now),
        State::Posting | State::Merging | State::Failed | State::Merged | State::Done => Ok(()),
    }
}

/// Carries every press a restart or an unknown answer left unfinished.
///
/// # Errors
/// Returns a persistence error.
pub(crate) fn settle_unfinished(
    store: &TaskStore,
    github: &dyn ReviewGithub,
    now: i64,
) -> Result<usize, TaskStoreError> {
    let unfinished = store.unsettled_github_review_submissions()?;
    for submission in &unfinished {
        carry_out(store, github, submission.id, now)?;
    }
    Ok(unfinished.len())
}

fn post(
    store: &TaskStore,
    github: &dyn ReviewGithub,
    submission: &GithubReviewDispatch,
    now: i64,
) -> Result<(), TaskStoreError> {
    let moved = |to, outcome: Option<&str>, review: Option<i64>| {
        store.move_github_review_submission(submission.id, State::Posting, to, outcome, review, now)
    };
    if !store.move_github_review_submission(
        submission.id,
        State::Queued,
        State::Posting,
        None,
        None,
        now,
    )? {
        return Ok(());
    }
    match github.post_review(
        &submission.repository,
        submission.number,
        submission.verdict,
        &submission.comment,
        &submission.head_sha,
    ) {
        Ok(review) => posted(store, github, submission, State::Posting, review, now),
        Err(GhWriteFailure::Refused(why)) => {
            moved(State::Failed, Some(&why), None).map(|_moved| ())
        }
        Err(GhWriteFailure::Unknown(why)) => moved(
            State::PostUncertain,
            Some(&format!(
                "Swarm is not sure GitHub took the review ({why}), and is checking"
            )),
            None,
        )
        .map(|_moved| ()),
    }
}

fn posted(
    store: &TaskStore,
    github: &dyn ReviewGithub,
    submission: &GithubReviewDispatch,
    from: State,
    review: i64,
    now: i64,
) -> Result<(), TaskStoreError> {
    match submission.verdict {
        GithubReviewVerdict::RequestChanges => store
            .move_github_review_submission(
                submission.id,
                from,
                State::Done,
                Some("Changes requested on GitHub."),
                Some(review),
                now,
            )
            .map(|_moved| ()),
        GithubReviewVerdict::Approve => {
            if store.move_github_review_submission(
                submission.id,
                from,
                State::AwaitingMerge,
                None,
                Some(review),
                now,
            )? {
                merge(store, github, submission, now)?;
            }
            Ok(())
        }
    }
}

/// A post whose outcome was unknown: a review by this login, on the commit,
/// with this verdict, is the one Swarm sent. None at all means it never
/// landed, and the operator may press again.
fn settle_post(
    store: &TaskStore,
    github: &dyn ReviewGithub,
    submission: &GithubReviewDispatch,
    now: i64,
) -> Result<(), TaskStoreError> {
    let (Ok(login), Ok(reviews)) = (
        github.login(),
        github.reviews(&submission.repository, submission.number),
    ) else {
        // Still unknown. Tried again on a later pass.
        return Ok(());
    };
    let wanted = match submission.verdict {
        GithubReviewVerdict::Approve => "APPROVED",
        GithubReviewVerdict::RequestChanges => "CHANGES_REQUESTED",
    };
    let landed = reviews.iter().find(|review| {
        review
            .author
            .as_deref()
            .is_some_and(|author| author.eq_ignore_ascii_case(&login))
            && review
                .commit_id
                .as_deref()
                .is_some_and(|commit| commit.eq_ignore_ascii_case(&submission.head_sha))
            && review.state == wanted
    });
    match landed {
        Some(review) => posted(
            store,
            github,
            submission,
            State::PostUncertain,
            review.id,
            now,
        ),
        None => store
            .move_github_review_submission(
                submission.id,
                State::PostUncertain,
                State::Failed,
                Some("GitHub has no record of this review, so nothing was posted. Press again."),
                None,
                now,
            )
            .map(|_moved| ()),
    }
}

fn merge(
    store: &TaskStore,
    github: &dyn ReviewGithub,
    submission: &GithubReviewDispatch,
    now: i64,
) -> Result<(), TaskStoreError> {
    let not_merged = |why: &str| {
        store
            .move_github_review_submission(
                submission.id,
                State::AwaitingMerge,
                State::Done,
                Some(&format!("Approved. Not merged: {why}.")),
                None,
                now,
            )
            .map(|_moved| ())
    };
    let facts = github
        .pull(&submission.repository, submission.number)
        .and_then(|pull| {
            let checks = github.checks(&submission.repository, &submission.head_sha)?;
            let methods = github.merge_methods(&submission.repository)?;
            Ok((pull, checks, methods))
        });
    let method = match facts {
        Err(why) => return not_merged(&format!("its state on GitHub could not be read: {why}")),
        Ok((pull, checks, methods)) => {
            match merge_decision(&pull, &submission.head_sha, checks.as_ref(), methods) {
                Ok(method) => method,
                Err(why) => return not_merged(&why),
            }
        }
    };
    if !store.move_github_review_submission(
        submission.id,
        State::AwaitingMerge,
        State::Merging,
        None,
        None,
        now,
    )? {
        return Ok(());
    }
    let (to, outcome) = match github.merge(
        &submission.repository,
        submission.number,
        &submission.head_sha,
        method,
    ) {
        Ok(()) => (State::Merged, "Approved and merged.".to_owned()),
        Err(GhWriteFailure::Refused(why)) => (
            State::Done,
            format!("Approved. GitHub would not merge it: {why}."),
        ),
        Err(GhWriteFailure::Unknown(why)) => (
            State::MergeUncertain,
            format!("Approved. Swarm is not sure the merge went through ({why}), and is checking."),
        ),
    };
    store
        .move_github_review_submission(submission.id, State::Merging, to, Some(&outcome), None, now)
        .map(|_moved| ())
}

/// A merge whose outcome was unknown: the pull request says whether it merged.
fn settle_merge(
    store: &TaskStore,
    github: &dyn ReviewGithub,
    submission: &GithubReviewDispatch,
    now: i64,
) -> Result<(), TaskStoreError> {
    let Ok(pull) = github.pull(&submission.repository, submission.number) else {
        return Ok(());
    };
    let (to, outcome) = if pull.merged {
        (State::Merged, "Approved and merged.")
    } else {
        (
            State::Done,
            "Approved. GitHub shows it was not merged, so merge it there.",
        )
    };
    store
        .move_github_review_submission(
            submission.id,
            State::MergeUncertain,
            to,
            Some(outcome),
            None,
            now,
        )
        .map(|_moved| ())
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::collections::VecDeque;

    use swarm_domain::{CiOutcome, GithubAsk, GithubAskKind, ProviderKind, TaskId, TaskState};
    use swarm_persistence::{
        GithubAskArrival, GithubAskFiling, GithubReviewSubmissionState as State,
        GithubReviewVerdict, TaskStore,
    };

    use super::{
        GhWriteFailure, MergeMethod, MergeMethods, PostedReview, PullFacts, ReviewGithub,
        carry_out, every_review, merge_decision, settle_unfinished,
    };

    const HEAD: &str = "888059e91c0ffee0123456789abcdef012345678";
    const COMMENT: &str = "Has a full sync been run with the read-only role?";

    fn ready_pull() -> PullFacts {
        PullFacts {
            open: true,
            merged: false,
            draft: false,
            head_sha: HEAD.into(),
            mergeable: Some(true),
        }
    }

    const ALL_METHODS: MergeMethods = MergeMethods {
        merge_commit: true,
        squash: true,
        rebase: true,
    };

    /// GitHub, scripted. Every call is written down, so a test can say what
    /// was NOT sent as plainly as what was.
    struct Fake {
        posts: RefCell<VecDeque<Result<i64, GhWriteFailure>>>,
        reviews: Result<Vec<PostedReview>, String>,
        pull: RefCell<Result<PullFacts, String>>,
        checks: Result<Option<CiOutcome>, String>,
        merges: RefCell<VecDeque<Result<(), GhWriteFailure>>>,
        calls: RefCell<Vec<String>>,
    }

    impl Fake {
        fn new() -> Self {
            Self {
                posts: RefCell::new(VecDeque::from([Ok(7)])),
                reviews: Ok(Vec::new()),
                pull: RefCell::new(Ok(ready_pull())),
                checks: Ok(Some(CiOutcome::Passed)),
                merges: RefCell::new(VecDeque::from([Ok(())])),
                calls: RefCell::new(Vec::new()),
            }
        }

        fn sent(&self, prefix: &str) -> Vec<String> {
            self.calls
                .borrow()
                .iter()
                .filter(|call| call.starts_with(prefix))
                .cloned()
                .collect()
        }
    }

    impl ReviewGithub for Fake {
        fn login(&self) -> Result<String, String> {
            Ok("operator".into())
        }
        fn post_review(
            &self,
            repository: &str,
            number: u64,
            verdict: GithubReviewVerdict,
            body: &str,
            commit: &str,
        ) -> Result<i64, GhWriteFailure> {
            self.calls.borrow_mut().push(format!(
                "post {repository}#{number} {} {commit} {body}",
                verdict.as_str()
            ));
            self.posts
                .borrow_mut()
                .pop_front()
                .expect("a post the test did not script")
        }
        fn reviews(&self, _: &str, _: u64) -> Result<Vec<PostedReview>, String> {
            self.calls.borrow_mut().push("reviews".into());
            self.reviews.clone()
        }
        fn pull(&self, _: &str, _: u64) -> Result<PullFacts, String> {
            self.calls.borrow_mut().push("pull".into());
            self.pull.borrow().clone()
        }
        fn checks(&self, _: &str, sha: &str) -> Result<Option<CiOutcome>, String> {
            self.calls.borrow_mut().push(format!("checks {sha}"));
            self.checks.clone()
        }
        fn merge_methods(&self, _: &str) -> Result<MergeMethods, String> {
            Ok(ALL_METHODS)
        }
        fn merge(
            &self,
            repository: &str,
            number: u64,
            sha: &str,
            method: MergeMethod,
        ) -> Result<(), GhWriteFailure> {
            self.calls
                .borrow_mut()
                .push(format!("merge {repository}#{number} {sha} {method:?}"));
            self.merges
                .borrow_mut()
                .pop_front()
                .expect("a merge the test did not script")
        }
    }

    /// An owed review on acme/petal#42 whose worker proposed `verdict`, and the
    /// operator's press on it.
    fn pressed(store: &TaskStore, verdict: GithubReviewVerdict) -> (TaskId, i64) {
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
        let ask = GithubAsk::new(
            GithubAskKind::ReviewRequest,
            "acme/petal",
            42,
            "Make the petals bloom",
            Some("sorrel"),
            "",
        )
        .unwrap();
        let GithubAskArrival::Filed(task) = store
            .receive_github_ask(&GithubAskFiling {
                ask: &ask,
                description: "Read it.",
                workspace: "/workspace/petal",
                repository_named: true,
                recommended_worker: Some(worker.id),
            })
            .unwrap()
        else {
            panic!("expected a filing");
        };
        store.transition_task(task, TaskState::Ready).unwrap();
        store.assign_task_to_worker(task, worker.id).unwrap();
        store.transition_task(task, TaskState::Active).unwrap();
        store
            .transition_task_with_note(task, TaskState::Review, "Notes.")
            .unwrap();
        store
            .propose_github_review(worker.id, task, verdict, COMMENT, HEAD, 1_000)
            .unwrap()
            .unwrap();
        let id = store
            .queue_github_review_submission(task, verdict, COMMENT, 1_100)
            .unwrap()
            .unwrap();
        (task, id)
    }

    fn outcome(store: &TaskStore, id: i64) -> (State, Option<String>) {
        let submission = store.github_review_submission(id).unwrap().unwrap();
        (submission.state, submission.outcome)
    }

    /// GitHub lists reviews oldest first: the one just posted is on the last
    /// page, and stopping early would read as "it never landed".
    #[test]
    fn the_read_back_reads_every_page_and_past_its_bound_is_unknown_not_absent() {
        let page_of = |count: usize, page: u32| -> Vec<PostedReview> {
            (0..count)
                .map(|n| PostedReview {
                    id: i64::from(page) * 1_000 + i64::try_from(n).unwrap(),
                    author: Some("someone".into()),
                    commit_id: Some(HEAD.into()),
                    state: "COMMENTED".into(),
                })
                .collect()
        };

        let mut asked = Vec::new();
        let reviews = every_review(|page| {
            asked.push(page);
            Ok(page_of(if page < 3 { 100 } else { 5 }, page))
        })
        .unwrap();
        assert_eq!(asked, [1, 2, 3]);
        assert_eq!(reviews.len(), 205);
        assert_eq!(reviews.last().map(|review| review.id), Some(3_004));

        let endless = every_review(|page| Ok(page_of(100, page)));
        assert!(endless.unwrap_err().contains("more than 1000 reviews"));

        let unread = every_review(|page| {
            if page == 2 {
                Err("no answer".into())
            } else {
                Ok(page_of(100, page))
            }
        });
        assert_eq!(unread.unwrap_err(), "no answer");
    }

    #[test]
    fn a_ready_pull_request_merges_by_the_first_method_the_repository_allows() {
        let passed = Some(CiOutcome::Passed);
        let decide = |pull: PullFacts, checks: Option<CiOutcome>, methods| {
            merge_decision(&pull, HEAD, checks.as_ref(), methods)
        };
        assert_eq!(
            decide(ready_pull(), passed.clone(), ALL_METHODS),
            Ok(MergeMethod::Merge)
        );
        assert_eq!(
            decide(
                PullFacts {
                    mergeable: None,
                    ..ready_pull()
                },
                passed.clone(),
                ALL_METHODS
            ),
            Ok(MergeMethod::Merge),
            "GitHub works mergeability out lazily; its own merge call decides"
        );
        let only = |squash, rebase| MergeMethods {
            merge_commit: false,
            squash,
            rebase,
        };
        assert_eq!(
            decide(ready_pull(), passed.clone(), only(true, true)),
            Ok(MergeMethod::Squash)
        );
        assert_eq!(
            decide(ready_pull(), passed.clone(), only(false, true)),
            Ok(MergeMethod::Rebase)
        );
        assert!(decide(ready_pull(), passed, only(false, false)).is_err());
    }

    #[test]
    fn nothing_unread_or_unchecked_is_merged() {
        let passed = Some(CiOutcome::Passed);
        let refused = |pull: PullFacts, checks: Option<CiOutcome>| {
            merge_decision(&pull, HEAD, checks.as_ref(), ALL_METHODS).unwrap_err()
        };
        assert!(
            refused(
                PullFacts {
                    draft: true,
                    ..ready_pull()
                },
                passed.clone()
            )
            .contains("draft")
        );
        assert!(
            refused(
                PullFacts {
                    head_sha: "f".repeat(40),
                    ..ready_pull()
                },
                passed.clone()
            )
            .contains("pushed after the review")
        );
        assert!(refused(ready_pull(), None).contains("no checks"));
        assert_eq!(
            refused(
                ready_pull(),
                Some(CiOutcome::Failed {
                    failing: vec!["rust".into()]
                })
            ),
            "checks failed: rust"
        );
        assert!(
            refused(
                ready_pull(),
                Some(CiOutcome::Pending {
                    waiting: vec!["web".into()]
                })
            )
            .contains("still running: web")
        );
        assert!(
            refused(
                ready_pull(),
                Some(CiOutcome::Unreadable {
                    why: "signed out".into()
                })
            )
            .contains("signed out")
        );
        assert!(
            refused(
                PullFacts {
                    mergeable: Some(false),
                    ..ready_pull()
                },
                passed.clone()
            )
            .contains("cannot be merged")
        );
        assert!(
            refused(
                PullFacts {
                    open: false,
                    ..ready_pull()
                },
                passed.clone()
            )
            .contains("closed")
        );
        assert!(
            refused(
                PullFacts {
                    merged: true,
                    ..ready_pull()
                },
                passed
            )
            .contains("already merged")
        );
    }

    /// THE PRESS THE OPERATOR ASKED FOR: their words, posted on the commit the
    /// worker read, then merged at that same commit.
    #[test]
    fn one_press_approves_with_the_comment_and_merges() {
        let store = TaskStore::in_memory().unwrap();
        let (_, id) = pressed(&store, GithubReviewVerdict::Approve);
        let github = Fake::new();
        carry_out(&store, &github, id, 1_200).unwrap();
        assert_eq!(
            github.sent("post"),
            vec![format!("post acme/petal#42 approve {HEAD} {COMMENT}")]
        );
        assert_eq!(github.sent("checks"), vec![format!("checks {HEAD}")]);
        assert_eq!(
            github.sent("merge"),
            vec![format!("merge acme/petal#42 {HEAD} Merge")]
        );
        assert_eq!(
            outcome(&store, id),
            (State::Merged, Some("Approved and merged.".into()))
        );
        let submission = store.github_review_submission(id).unwrap().unwrap();
        assert_eq!(submission.review_id, Some(7));
        assert!(store.github_reviews_owed().unwrap().is_empty());
    }

    #[test]
    fn a_draft_is_approved_and_left_unmerged_with_the_reason() {
        let store = TaskStore::in_memory().unwrap();
        let (_, id) = pressed(&store, GithubReviewVerdict::Approve);
        let github = Fake::new();
        *github.pull.borrow_mut() = Ok(PullFacts {
            draft: true,
            ..ready_pull()
        });
        carry_out(&store, &github, id, 1_200).unwrap();
        assert!(github.sent("merge").is_empty());
        let (state, said) = outcome(&store, id);
        assert_eq!(state, State::Done);
        assert!(
            said.unwrap()
                .starts_with("Approved. Not merged: it is still a draft")
        );
    }

    #[test]
    fn requesting_changes_posts_and_never_merges() {
        let store = TaskStore::in_memory().unwrap();
        let (_, id) = pressed(&store, GithubReviewVerdict::RequestChanges);
        let github = Fake::new();
        carry_out(&store, &github, id, 1_200).unwrap();
        assert_eq!(github.sent("post").len(), 1);
        assert!(github.sent("pull").is_empty() && github.sent("merge").is_empty());
        assert_eq!(
            outcome(&store, id),
            (State::Done, Some("Changes requested on GitHub.".into()))
        );
    }

    #[test]
    fn a_review_github_refuses_is_shown_in_its_words_and_stays_owed() {
        let store = TaskStore::in_memory().unwrap();
        let (task, id) = pressed(&store, GithubReviewVerdict::Approve);
        let github = Fake::new();
        *github.posts.borrow_mut() = VecDeque::from([Err(GhWriteFailure::Refused(
            "Can not approve your own pull request".into(),
        ))]);
        carry_out(&store, &github, id, 1_200).unwrap();
        assert!(github.sent("merge").is_empty());
        assert_eq!(
            outcome(&store, id),
            (
                State::Failed,
                Some("Can not approve your own pull request".into())
            )
        );
        let owed = store.github_reviews_owed().unwrap();
        assert_eq!(owed[0].task_id, task);
        assert_eq!(owed[0].submission.as_ref().unwrap().state, State::Failed);
    }

    /// AN UNKNOWN ANSWER IS NEVER SENT AGAIN. GitHub took the review but the
    /// answer was lost; reading back finds it and the merge goes ahead. A
    /// second post would be a second review under the operator's name.
    #[test]
    fn an_unanswered_post_is_settled_by_reading_back_not_by_posting_again() {
        let store = TaskStore::in_memory().unwrap();
        let (_, id) = pressed(&store, GithubReviewVerdict::Approve);
        let mut github = Fake::new();
        *github.posts.borrow_mut() = VecDeque::from([Err(GhWriteFailure::Unknown(
            "GitHub did not answer within 20 seconds".into(),
        ))]);
        carry_out(&store, &github, id, 1_200).unwrap();
        assert_eq!(outcome(&store, id).0, State::PostUncertain);
        assert!(github.sent("merge").is_empty());

        github.reviews = Ok(vec![
            PostedReview {
                id: 3,
                author: Some("operator".into()),
                commit_id: Some("a".repeat(40)),
                state: "APPROVED".into(),
            },
            PostedReview {
                id: 4,
                author: Some("sorrel".into()),
                commit_id: Some(HEAD.into()),
                state: "APPROVED".into(),
            },
            PostedReview {
                id: 9,
                author: Some("Operator".into()),
                commit_id: Some(HEAD.into()),
                state: "APPROVED".into(),
            },
        ]);
        assert_eq!(settle_unfinished(&store, &github, 1_300).unwrap(), 1);
        assert_eq!(github.sent("post").len(), 1, "posted once, ever");
        assert_eq!(outcome(&store, id).0, State::Merged);
        assert_eq!(
            store
                .github_review_submission(id)
                .unwrap()
                .unwrap()
                .review_id,
            Some(9)
        );
    }

    #[test]
    fn an_unanswered_post_github_has_no_record_of_can_be_pressed_again() {
        let store = TaskStore::in_memory().unwrap();
        let (task, id) = pressed(&store, GithubReviewVerdict::Approve);
        let mut github = Fake::new();
        *github.posts.borrow_mut() =
            VecDeque::from([Err(GhWriteFailure::Unknown("no answer".into()))]);
        carry_out(&store, &github, id, 1_200).unwrap();

        github.reviews = Err("GitHub could not be read".into());
        settle_unfinished(&store, &github, 1_300).unwrap();
        assert_eq!(
            outcome(&store, id).0,
            State::PostUncertain,
            "unreadable is still unknown"
        );

        github.reviews = Ok(Vec::new());
        settle_unfinished(&store, &github, 1_400).unwrap();
        let (state, said) = outcome(&store, id);
        assert_eq!(state, State::Failed);
        assert!(said.unwrap().contains("Press again"));
        assert_eq!(github.sent("post").len(), 1);
        assert!(
            store
                .queue_github_review_submission(task, GithubReviewVerdict::Approve, COMMENT, 1_500)
                .unwrap()
                .is_ok()
        );
    }

    #[test]
    fn an_unanswered_merge_is_settled_from_the_pull_request() {
        let store = TaskStore::in_memory().unwrap();
        let (_, id) = pressed(&store, GithubReviewVerdict::Approve);
        let github = Fake::new();
        *github.merges.borrow_mut() =
            VecDeque::from([Err(GhWriteFailure::Unknown("no answer".into()))]);
        carry_out(&store, &github, id, 1_200).unwrap();
        assert_eq!(outcome(&store, id).0, State::MergeUncertain);

        *github.pull.borrow_mut() = Ok(PullFacts {
            merged: true,
            open: false,
            ..ready_pull()
        });
        settle_unfinished(&store, &github, 1_300).unwrap();
        assert_eq!(
            outcome(&store, id),
            (State::Merged, Some("Approved and merged.".into()))
        );
        assert_eq!(github.sent("merge").len(), 1, "merged once, ever");
    }

    #[test]
    fn a_restart_mid_post_is_settled_without_posting_twice() {
        let store = TaskStore::in_memory().unwrap();
        let (_, id) = pressed(&store, GithubReviewVerdict::Approve);
        store
            .move_github_review_submission(id, State::Queued, State::Posting, None, None, 1_200)
            .unwrap();
        assert_eq!(
            store
                .recover_inflight_github_review_submissions(1_250)
                .unwrap(),
            1
        );
        let mut github = Fake::new();
        github.reviews = Ok(vec![PostedReview {
            id: 11,
            author: Some("operator".into()),
            commit_id: Some(HEAD.into()),
            state: "APPROVED".into(),
        }]);
        settle_unfinished(&store, &github, 1_300).unwrap();
        assert!(github.sent("post").is_empty());
        assert_eq!(outcome(&store, id).0, State::Merged);
    }

    #[test]
    fn commits_pushed_after_the_review_are_never_merged() {
        let store = TaskStore::in_memory().unwrap();
        let (_, id) = pressed(&store, GithubReviewVerdict::Approve);
        let github = Fake::new();
        *github.pull.borrow_mut() = Ok(PullFacts {
            head_sha: "b".repeat(40),
            ..ready_pull()
        });
        carry_out(&store, &github, id, 1_200).unwrap();
        assert_eq!(
            github.sent("post"),
            vec![format!("post acme/petal#42 approve {HEAD} {COMMENT}")],
            "the approval is pinned to what was read"
        );
        assert!(github.sent("merge").is_empty());
        assert!(
            outcome(&store, id)
                .1
                .unwrap()
                .contains("pushed after the review")
        );
    }
}
