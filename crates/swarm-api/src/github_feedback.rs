//! Filing a dogfood report as a GitHub issue.
//!
//! WHY THIS EXISTS. Feedback saved to the Hive and stopped there, while the
//! operator believed it reached GitHub — "it should be automatic if it's
//! working, I've just never tested it". It was never built. A colleague filed a
//! report, saw a Save button, pressed it, and her words are still sitting in a
//! local database nobody reads.
//!
//! THE SCREENSHOT DOES NOT GO. It is labelled "kept on this device" where it is
//! taken, a GitHub issue is public, and a capture of somebody's screen can hold
//! anything that was on it. The issue says a screenshot exists and where to
//! find it; sending the image is a separate decision nobody has made.

use serde::Deserialize;
use swarm_persistence::DogfoodReport;

/// Where reports are filed, and the credential that may file them.
#[derive(Clone, Debug)]
pub(super) struct GithubFeedback {
    /// "owner/name", validated on the way in so a typo cannot become a URL.
    repository: String,
    token: String,
}

#[derive(Debug, Deserialize)]
struct CreatedIssue {
    html_url: String,
}

/// One open issue, as much of it as a draft task needs.
#[derive(Debug, Deserialize)]
pub(super) struct OpenIssue {
    pub(super) html_url: String,
    pub(super) title: String,
    #[serde(default)]
    pub(super) body: Option<String>,
    /// Present on pull requests and absent on issues. GitHub returns both from
    /// the issues endpoint, and a pull request is not a piece of work for this
    /// board.
    #[serde(default)]
    pub(super) pull_request: Option<serde_json::Value>,
}

/// The reasons filing can fail, kept apart so the caller can say which.
#[derive(Debug)]
pub(super) enum GithubError {
    Refused(String),
    Unreachable,
}

impl GithubFeedback {
    /// Rejects a repository that is not `owner/name`, so a misconfiguration
    /// fails here rather than becoming a request to somewhere unintended.
    pub(super) fn new(repository: &str, token: &str) -> Result<Self, String> {
        let repository = repository.trim();
        let token = token.trim();
        if token.is_empty() {
            return Err("the GitHub token is empty".to_owned());
        }
        let mut parts = repository.split('/');
        let owner = parts.next().unwrap_or_default();
        let name = parts.next().unwrap_or_default();
        let segment_ok = |value: &str| {
            !value.is_empty()
                && value.len() <= 100
                && value
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
        };
        if parts.next().is_some() || !segment_ok(owner) || !segment_ok(name) {
            return Err(format!(
                "SWARM_GITHUB_REPOSITORY must be owner/name, not {repository:?}"
            ));
        }
        Ok(Self {
            repository: repository.to_owned(),
            token: token.to_owned(),
        })
    }

    pub(super) fn repository(&self) -> &str {
        &self.repository
    }

    /// Opens the issue and returns its public URL.
    pub(super) async fn file(&self, report: &DogfoodReport) -> Result<String, GithubError> {
        let title = issue_title(report);
        let body = issue_body(report);
        let response = reqwest::Client::new()
            .post(format!(
                "https://api.github.com/repos/{}/issues",
                self.repository
            ))
            .header("authorization", format!("Bearer {}", self.token))
            .header("accept", "application/vnd.github+json")
            .header("user-agent", "swarm-next")
            .json(&serde_json::json!({ "title": title, "body": body }))
            .send()
            .await
            .map_err(|_| GithubError::Unreachable)?;
        let status = response.status();
        if !status.is_success() {
            // The status, never the body: a GitHub error can echo what was
            // sent, and the token is in the request.
            return Err(GithubError::Refused(format!(
                "GitHub refused the report with {status}"
            )));
        }
        response
            .json::<CreatedIssue>()
            .await
            .map(|issue| issue.html_url)
            .map_err(|_| GithubError::Unreachable)
    }
}

impl GithubFeedback {
    /// The open issues, newest first, bounded.
    ///
    /// Pull requests are dropped: GitHub returns them from the issues endpoint
    /// and they are not work for this board.
    pub(super) async fn open_issues(&self) -> Result<Vec<OpenIssue>, GithubError> {
        let response = reqwest::Client::new()
            .get(format!(
                "https://api.github.com/repos/{}/issues?state=open&per_page=20&sort=created&direction=desc",
                self.repository
            ))
            .header("authorization", format!("Bearer {}", self.token))
            .header("accept", "application/vnd.github+json")
            .header("user-agent", "swarm-next")
            .send()
            .await
            .map_err(|_| GithubError::Unreachable)?;
        let status = response.status();
        if !status.is_success() {
            return Err(GithubError::Refused(format!(
                "GitHub refused the issue list with {status}"
            )));
        }
        let issues = response
            .json::<Vec<OpenIssue>>()
            .await
            .map_err(|_| GithubError::Unreachable)?;
        Ok(issues_for_the_board(issues))
    }
}

/// Drops the pull requests GitHub returns from its issues endpoint.
///
/// Separate from the request so it can be tested without a credential: the
/// network leg of this client has never run, and this part need not wait on it.
fn issues_for_the_board(issues: Vec<OpenIssue>) -> Vec<OpenIssue> {
    issues
        .into_iter()
        .filter(|issue| issue.pull_request.is_none())
        .collect()
}

/// A title someone can scan in a list, from the words the reporter wrote.
fn issue_title(report: &DogfoodReport) -> String {
    let source = if report.observation.trim().is_empty() {
        report.expectation.trim()
    } else {
        report.observation.trim()
    };
    let first_line = source.lines().next().unwrap_or_default().trim();
    let mut title: String = first_line.chars().take(96).collect();
    if title.is_empty() {
        title.push_str("Dogfood report");
    } else if first_line.chars().count() > 96 {
        title.push('…');
    }
    title
}

fn issue_body(report: &DogfoodReport) -> String {
    let mut body = String::new();
    if !report.expectation.trim().is_empty() {
        body.push_str("**Expected**\n\n");
        body.push_str(report.expectation.trim());
        body.push_str("\n\n");
    }
    if !report.observation.trim().is_empty() {
        body.push_str("**What happened**\n\n");
        body.push_str(report.observation.trim());
        body.push_str("\n\n");
    }
    if report.attachment_name.is_some() {
        // NAMED, NOT SENT. The image stays on the reporter's device; saying so
        // is what stops a reader assuming there was nothing to see.
        body.push_str(
            "A screenshot was attached to this report and is kept privately on the reporter's Hive; it was deliberately not uploaded.\n\n",
        );
    }
    // ANONYMOUS, ON THE OPERATOR'S RULING, and it says the one thing a reader
    // has to know: there is nobody on the other end of this thread.
    //
    // "If someone has not connected, I don't care about their swarm name or
    // their username. They can be an anonymous issue submission. If they have
    // GitHub, then they can link their GitHub account to the issue, which would
    // be automatic in theory, and therefore they can get a response."
    //
    // So no name, no Hive, no operator id — publishing any of those to satisfy
    // routing nobody asked for would be a privacy cost with no buyer. What is
    // NOT optional is the second sentence: this issue is authored by the Hive's
    // credential, not by the person who hit the button, so closing it with
    // "fixed!" reaches nobody. A maintainer who does not know that is being
    // quietly misled by their own issue tracker.
    body.push_str(
        "_Filed from Swarm by an anonymous reporter, using this Hive's credential rather than their own account. Replies here will not reach them._",
    );
    body
}

#[cfg(test)]
mod tests {
    use super::*;

    fn report(expectation: &str, observation: &str, attachment: Option<&str>) -> DogfoodReport {
        DogfoodReport {
            id: "report-1".to_owned(),
            expectation: expectation.to_owned(),
            observation: observation.to_owned(),
            diagnostic_bundle: "{}".to_owned(),
            attachment_name: attachment.map(str::to_owned),
            github_issue_url: None,
            created_at: 1,
        }
    }

    #[test]
    fn a_repository_that_is_not_owner_slash_name_is_refused() {
        // A typo must fail here rather than become a request somewhere else.
        for bad in [
            "swarm-next",
            "miopea/swarm-next/extra",
            "../../etc",
            "miopea/",
            "/swarm-next",
            "mio pea/swarm",
        ] {
            assert!(
                GithubFeedback::new(bad, "token").is_err(),
                "accepted {bad:?}"
            );
        }
        assert!(GithubFeedback::new("miopea/swarm-next", "token").is_ok());
    }

    #[test]
    fn an_empty_token_is_refused_rather_than_sent() {
        assert!(GithubFeedback::new("miopea/swarm-next", "   ").is_err());
    }

    #[test]
    fn the_body_says_a_screenshot_exists_without_sending_it() {
        let body = issue_body(&report("It saves", "It vanished", Some("shot.png")));
        assert!(body.contains("Expected"));
        assert!(body.contains("It vanished"));
        assert!(
            body.contains("deliberately not uploaded"),
            "a reader must know an image exists and that it stayed private: {body}"
        );
        assert!(
            !body.contains("shot.png"),
            "and the filename is not the point; the image is not there: {body}"
        );
    }

    #[test]
    fn a_title_comes_from_the_reporters_own_words_and_is_bounded() {
        let long = "x".repeat(200);
        let titled = issue_title(&report("", &long, None));
        assert!(titled.chars().count() <= 97, "{}", titled.chars().count());
        assert!(titled.ends_with('…'));
        assert_eq!(
            issue_title(&report("Expected only", "", None)),
            "Expected only",
            "falls back to the expectation when nothing was observed"
        );
        assert_eq!(issue_title(&report("", "", None)), "Dogfood report");
    }

    #[test]
    fn a_pull_request_is_not_a_piece_of_work_for_this_board() {
        // The shape GitHub actually returns: /issues carries pull requests too,
        // distinguished only by a `pull_request` key nobody would notice.
        let payload = serde_json::json!([
            {"html_url": "https://github.com/o/n/issues/1", "title": "Terminal drops a line", "body": "on my phone"},
            {"html_url": "https://github.com/o/n/pull/2", "title": "Fix the terminal", "body": "",
             "pull_request": {"url": "https://api.github.com/repos/o/n/pulls/2"}}
        ]);
        let issues: Vec<OpenIssue> = serde_json::from_value(payload).unwrap();
        assert_eq!(issues.len(), 2, "both arrive from GitHub");

        let kept = issues_for_the_board(issues);

        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].title, "Terminal drops a line");
    }

    /// An anonymous report must say that nobody can be replied to.
    ///
    /// The operator ruled that an un-connected reporter is anonymous — "I don't
    /// care about their swarm name or their username" — which removes the
    /// privacy question entirely and leaves one obligation behind it. The issue
    /// is authored by the HIVE's credential, so a maintainer closing it with
    /// "fixed!" is talking to themselves. Saying so is the whole job of that
    /// last line.
    #[test]
    fn an_anonymous_report_names_nobody_and_says_replies_reach_nobody() {
        let report = DogfoodReport {
            id: "report-1".to_owned(),
            expectation: "The terminal keeps its width".to_owned(),
            observation: "It jumps back on every scroll".to_owned(),
            diagnostic_bundle: String::new(),
            attachment_name: None,
            github_issue_url: None,
            created_at: 0,
        };

        let body = issue_body(&report);

        // The reporter's own words survive.
        assert!(body.contains("It jumps back on every scroll"), "{body}");
        // AND THE DEAD END IS STATED. Without this a maintainer closes the
        // issue believing the reporter will hear about it.
        assert!(body.contains("will not reach them"), "{body}");
        assert!(body.contains("anonymous"), "{body}");
    }
}
