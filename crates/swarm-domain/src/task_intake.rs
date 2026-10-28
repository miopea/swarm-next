//! Work an outside tool files for Queen to route (ADR 0126).
//!
//! A connected tool such as Claude reads mail and Jira, not repositories, so it
//! often cannot name the repository the work belongs to. It files what it knows
//! and Queen routes from the description. What it said about where the work
//! came from, and who it thinks should do it, is kept beside the task rather
//! than written into its text, so nobody has to parse it back out.

use serde::Serialize;

use crate::{TaskId, TaskState, WorkerId};

/// The longest link to where the work came from, such as an email.
pub const MAX_INTAKE_LINK_BYTES: usize = 2048;
/// The longest Jira key.
pub const MAX_INTAKE_JIRA_KEY_BYTES: usize = 64;
/// The longest name of the person who asked.
pub const MAX_INTAKE_REQUESTER_BYTES: usize = 200;
/// The most tasks one search returns.
pub const MAX_TASK_SEARCH_HITS: usize = 20;
/// The longest search text.
pub const MAX_TASK_SEARCH_BYTES: usize = 200;
/// The most words one search matches on.
const MAX_TASK_SEARCH_WORDS: usize = 12;
/// Words that appear in most titles, so matching them finds nothing.
const SEARCH_STOP_WORDS: [&str; 9] = [
    "the", "and", "for", "with", "from", "this", "that", "into", "not",
];
/// The most worker names one refusal suggests.
pub const MAX_WORKER_SUGGESTIONS: usize = 3;
/// The most characters a name may have wrong and still be a typo of a worker.
const MAX_WORKER_NAME_TYPOS: usize = 3;

/// Where filed work came from, as the tool that filed it said.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
pub struct IntakeSource {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub link: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub jira_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub requested_by: Option<String>,
}

impl IntakeSource {
    /// Trims each field and drops empty ones. A Jira key is upper-cased, so
    /// `abc-12` and `ABC-12` are one key when work is searched for later.
    ///
    /// # Errors
    /// Returns the sentence the filer is told when a field is too long, spans
    /// lines, or is not a Jira key.
    pub fn normalized(
        link: Option<&str>,
        jira_key: Option<&str>,
        requested_by: Option<&str>,
    ) -> Result<Self, String> {
        let link = one_line("source.link", link, MAX_INTAKE_LINK_BYTES)?;
        let requested_by = one_line(
            "source.requested_by",
            requested_by,
            MAX_INTAKE_REQUESTER_BYTES,
        )?;
        let jira_key = one_line("source.jira_key", jira_key, MAX_INTAKE_JIRA_KEY_BYTES)?
            .map(|key| key.to_ascii_uppercase());
        if let Some(key) = &jira_key
            && !is_jira_key(key)
        {
            return Err(format!(
                "source.jira_key \"{key}\" is not a Jira key. A key is the project key, a \
                 hyphen and the issue number, such as ABC-123."
            ));
        }
        Ok(Self {
            link,
            jira_key,
            requested_by,
        })
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.link.is_none() && self.jira_key.is_none() && self.requested_by.is_none()
    }
}

fn one_line(field: &str, value: Option<&str>, limit: usize) -> Result<Option<String>, String> {
    let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(None);
    };
    if value.len() > limit {
        return Err(format!("{field} is longer than {limit} bytes."));
    }
    if value.contains(['\n', '\r']) {
        return Err(format!("{field} must be one line."));
    }
    Ok(Some(value.to_owned()))
}

/// A project key (a letter, then letters, digits or underscores), a hyphen,
/// and an issue number that does not start with zero.
fn is_jira_key(key: &str) -> bool {
    let Some((project, number)) = key.split_once('-') else {
        return false;
    };
    let mut project = project.chars();
    project
        .next()
        .is_some_and(|first| first.is_ascii_uppercase())
        && project.all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
        && !number.starts_with('0')
        && !number.is_empty()
        && number.chars().all(|c| c.is_ascii_digit())
}

/// A worker the filer suggested, read by id so a rename does not lose it.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct RecommendedWorker {
    pub id: WorkerId,
    pub name: String,
}

/// What an outside tool said about work it filed, beside the task itself.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct TaskIntake {
    /// Whether the filer named the repository. When it did not, the task holds
    /// a placeholder until Queen assigns it, and assigning sets the worker's.
    pub repository_named: bool,
    /// A suggestion, not an assignment. Absent when none was made, or when the
    /// worker suggested has since been removed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recommended_worker: Option<RecommendedWorker>,
    #[serde(skip_serializing_if = "IntakeSource::is_empty")]
    pub source: IntakeSource,
}

/// A worker work can be routed to, as an outside tool sees it: its name, what
/// it owns and its repository. Nothing about its sessions or what it is doing.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct RoutingCandidate {
    pub name: String,
    pub description: String,
    pub workspace: String,
}

/// One open task an outside tool's search found. Deliberately no description:
/// an outside tool reads other people's work by its title only (ADR 0126).
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct TaskSearchHit {
    pub id: TaskId,
    pub title: String,
    pub state: TaskState,
    pub workspace: String,
    /// Whether the searcher filed it, and so may read it in full.
    pub filed_by_you: bool,
}

/// The words a search matches on: lower-cased, stripped of the punctuation
/// around them, three bytes or longer, without the common ones, and at most
/// twelve.
///
/// # Errors
/// Returns the sentence the searcher is told when nothing is left to match.
pub fn task_search_words(query: &str) -> Result<Vec<String>, String> {
    if query.len() > MAX_TASK_SEARCH_BYTES {
        return Err(format!(
            "The search is longer than {MAX_TASK_SEARCH_BYTES} bytes. Search for the few words \
             a title would carry."
        ));
    }
    let mut words: Vec<String> = Vec::new();
    for word in query.split_whitespace() {
        let word = word
            .trim_matches(|c: char| !c.is_alphanumeric())
            .to_lowercase();
        if word.len() >= 3
            && !SEARCH_STOP_WORDS.contains(&word.as_str())
            && !words.contains(&word)
            && words.len() < MAX_TASK_SEARCH_WORDS
        {
            words.push(word);
        }
    }
    if words.is_empty() {
        return Err(
            "The search needs at least one word of three or more letters that is not a common \
             word such as \"the\"."
                .into(),
        );
    }
    Ok(words)
}

/// How well a title answers a search: one point per word it contains. A Jira
/// key the work was filed under counts as every word at once, because it names
/// the request exactly where words only resemble it.
#[must_use]
pub fn task_search_score(title: &str, jira_key: Option<&str>, words: &[String]) -> usize {
    if let Some(key) = jira_key
        && words.iter().any(|word| word.eq_ignore_ascii_case(key))
    {
        return words.len() + 1;
    }
    let title = title.to_lowercase();
    words
        .iter()
        .filter(|word| title.contains(word.as_str()))
        .count()
}

/// The workers a name that matched none was probably meant to be, closest
/// first and at most three.
///
/// A worker is close when the name is a typo of its name, at most one
/// character wrong in four and never more than three, or when every word of
/// the name begins a word of its name, as "public web" begins "Public
/// Website". Case, spacing and punctuation never count. A name nothing
/// resembles gets nothing, so a refusal never offers a guess as a correction.
#[must_use]
pub fn closest_worker_names<'a>(
    name: &str,
    workers: impl IntoIterator<Item = &'a str>,
) -> Vec<&'a str> {
    let typed = name_words(name);
    let typed_chars = typed.join(" ").chars().collect::<Vec<_>>();
    if typed_chars.is_empty() {
        return Vec::new();
    }
    let typos = (typed_chars.len() / 4).clamp(1, MAX_WORKER_NAME_TYPOS);
    let mut close = workers
        .into_iter()
        .filter_map(|worker| {
            let words = name_words(worker);
            // A word match ranks below every typo, which names the worker closer.
            within_typos(&typed_chars, &words.join(" "), typos)
                .or_else(|| begins_its_words(&typed, &words).then_some(typos + 1))
                .map(|rank| (rank, worker))
        })
        .collect::<Vec<_>>();
    // Stable, so equally close workers keep the roster's order.
    close.sort_by_key(|(rank, _)| *rank);
    close
        .into_iter()
        .take(MAX_WORKER_SUGGESTIONS)
        .map(|(_, worker)| worker)
        .collect()
}

/// A name's words, lower-cased, with everything but letters and digits as a
/// break between them.
fn name_words(name: &str) -> Vec<String> {
    name.split(|c: char| !c.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .map(str::to_lowercase)
        .collect()
}

/// How many characters turn one name into the other, when that is at most
/// `limit`. Two swapped neighbours are one typo, not two.
///
/// Names further apart in length than the limit are never compared, so a long
/// name costs no more than a short one.
fn within_typos(typed: &[char], worker: &str, limit: usize) -> Option<usize> {
    let worker = worker.chars().collect::<Vec<_>>();
    if typed.len().abs_diff(worker.len()) > limit {
        return None;
    }
    let mut before = Vec::new();
    let mut previous = (0..=worker.len()).collect::<Vec<_>>();
    for (row, &typed_char) in typed.iter().enumerate() {
        let mut current = vec![row + 1; worker.len() + 1];
        for (column, &worker_char) in worker.iter().enumerate() {
            let mut cost = (previous[column] + usize::from(typed_char != worker_char))
                .min(previous[column + 1] + 1)
                .min(current[column] + 1);
            if row > 0
                && column > 0
                && typed_char == worker[column - 1]
                && typed[row - 1] == worker_char
            {
                cost = cost.min(before[column - 1] + 1);
            }
            current[column + 1] = cost;
        }
        before = std::mem::replace(&mut previous, current);
    }
    let distance = previous[worker.len()];
    (distance <= limit).then_some(distance)
}

/// Whether every typed word of three or more characters begins a word of the
/// worker's name. A name with more words than the worker's is not a shorter
/// form of it, and a name of only short words says too little to match on.
fn begins_its_words(typed: &[String], worker: &[String]) -> bool {
    if typed.len() > worker.len() {
        return false;
    }
    let meaningful = typed
        .iter()
        .filter(|word| word.chars().count() >= 3)
        .collect::<Vec<_>>();
    !meaningful.is_empty()
        && meaningful
            .iter()
            .all(|word| worker.iter().any(|own| own.starts_with(word.as_str())))
}

#[cfg(test)]
mod tests {
    use super::*;

    const ROSTER: [&str; 6] = [
        "BFG Solutions",
        "D365 Solutions",
        "Public Website",
        "RCG Networks",
        "Swarm Next",
        "Aria",
    ];

    /// ABLATION: count a swap as two typos and "Aira" suggests nothing.
    #[test]
    fn a_typo_suggests_the_worker_it_misspells() {
        assert_eq!(
            closest_worker_names("D365 Solution", ROSTER),
            ["D365 Solutions"]
        );
        assert_eq!(closest_worker_names("swarm-nxet", ROSTER), ["Swarm Next"]);
        assert_eq!(closest_worker_names("Aira", ROSTER), ["Aria"]);
        assert_eq!(closest_worker_names("Ari", ROSTER), ["Aria"]);
    }

    #[test]
    fn the_words_that_begin_a_name_suggest_it() {
        assert_eq!(
            closest_worker_names("public web", ROSTER),
            ["Public Website"]
        );
        assert_eq!(closest_worker_names("networks", ROSTER), ["RCG Networks"]);
        assert_eq!(
            closest_worker_names("Solutions", ROSTER),
            ["BFG Solutions", "D365 Solutions"]
        );
    }

    #[test]
    fn a_typo_ranks_above_a_word_match() {
        let roster = ["Probe Runner", "Prob"];
        assert_eq!(
            closest_worker_names("Probe", roster),
            ["Prob", "Probe Runner"]
        );
    }

    /// No suggestion is better than a wrong one: a name nothing resembles, one
    /// of only short words, and one with more words than any worker's name get
    /// none.
    #[test]
    fn a_name_nothing_resembles_suggests_nothing() {
        for name in [
            "Nonexistent Worker 9000",
            "Nobody",
            "a b",
            "--",
            "",
            "public website team lead",
        ] {
            assert!(
                closest_worker_names(name, ROSTER).is_empty(),
                "{name} resembles no worker"
            );
        }
        let long = "solutions ".repeat(10_000);
        assert!(closest_worker_names(&long, ROSTER).is_empty());
    }

    #[test]
    fn suggestions_are_bounded() {
        let roster = ["Probe 1", "Probe 2", "Probe 3", "Probe 4", "Probe 5"];
        assert_eq!(
            closest_worker_names("probe", roster),
            ["Probe 1", "Probe 2", "Probe 3"]
        );
    }

    #[test]
    fn a_source_is_trimmed_and_its_jira_key_upper_cased() {
        let source = IntakeSource::normalized(
            Some("  https://mail.example/1  "),
            Some(" abc-12 "),
            Some(""),
        )
        .unwrap();
        assert_eq!(source.link.as_deref(), Some("https://mail.example/1"));
        assert_eq!(source.jira_key.as_deref(), Some("ABC-12"));
        assert_eq!(source.requested_by, None);
    }

    #[test]
    fn a_source_refuses_what_is_not_a_jira_key_or_one_line() {
        for key in [
            "ABC", "ABC-", "-12", "1BC-12", "ABC-012", "ABC-1x", "AB C-1",
        ] {
            assert!(
                IntakeSource::normalized(None, Some(key), None).is_err(),
                "{key} is not a Jira key"
            );
        }
        assert!(IntakeSource::normalized(None, Some("AB_2-7"), None).is_ok());
        assert!(IntakeSource::normalized(None, None, Some("Ann\nBob")).is_err());
        let long = "x".repeat(MAX_INTAKE_REQUESTER_BYTES + 1);
        assert!(IntakeSource::normalized(None, None, Some(&long)).is_err());
    }

    #[test]
    fn search_words_drop_short_common_and_repeated_words() {
        let words =
            task_search_words("Fix the renewal e-mail, renewal TEMPLATE for ABC-12 a").unwrap();
        assert_eq!(words, ["fix", "renewal", "e-mail", "template", "abc-12"]);
        assert!(task_search_words("a an of the").is_err());
        assert!(task_search_words(&"word ".repeat(MAX_TASK_SEARCH_BYTES)).is_err());
    }

    #[test]
    fn a_title_scores_a_point_per_word_and_a_jira_key_scores_all_of_them() {
        let words = task_search_words("renewal email template").unwrap();
        assert_eq!(task_search_score("Renewal email is late", None, &words), 2);
        assert_eq!(task_search_score("Unrelated", None, &words), 0);
        let keyed = task_search_words("abc-12 renewal").unwrap();
        assert_eq!(task_search_score("Unrelated", Some("ABC-12"), &keyed), 3);
        assert_eq!(task_search_score("Unrelated", Some("ABC-13"), &keyed), 0);
    }
}
