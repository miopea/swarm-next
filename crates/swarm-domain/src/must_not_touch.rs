//! Paths a task's work must not touch (ADR 0172).
//!
//! A task may carry path patterns, relative to its repository, that its work
//! must leave alone. At Review they are compared with the paths its recorded
//! commits changed, and any crossing is shown to Queen. Nothing is refused for
//! one: some crossings are legitimate, and Queen decides.
//!
//! The patterns are set by whoever files the task, then by Queen. The worker
//! holding the task cannot change them.

use std::borrow::Cow;

use serde::{Deserialize, Serialize};

use crate::TaskCommitReport;

/// The most patterns one task carries.
pub const MAX_MUST_NOT_TOUCH_PATTERNS: usize = 32;
/// The longest one pattern may be.
pub const MAX_MUST_NOT_TOUCH_PATTERN_CHARS: usize = 256;
/// The most crossed paths listed for one commit; the rest are counted.
pub const MAX_CROSSED_PATHS_PER_COMMIT: usize = 20;
/// The most commits with crossings listed; the rest are counted.
pub const MAX_CROSSING_COMMITS: usize = 50;

/// A task's patterns. New ones are made by [`validate_must_not_touch`], so
/// storage never takes a pattern that was not checked.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct MustNotTouchPatterns(Vec<String>);

impl MustNotTouchPatterns {
    /// Patterns read back from storage, as they were accepted when set.
    ///
    /// Not re-checked: a rule tightened later must not make a task filed
    /// before it unreadable. A pattern today's rules refuse still matches
    /// exactly what it matched when it was accepted.
    #[must_use]
    pub fn stored(patterns: Vec<String>) -> Self {
        Self(patterns)
    }

    #[must_use]
    pub fn as_slice(&self) -> &[String] {
        &self.0
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// Checks and normalises a task's patterns: trimmed, a leading `./` dropped,
/// duplicates removed in the order given. An empty list is valid and means
/// the task has none.
///
/// # Errors
/// Says which pattern is unusable and why, in words a filer can act on.
pub fn validate_must_not_touch(patterns: &[String]) -> Result<MustNotTouchPatterns, String> {
    if patterns.len() > MAX_MUST_NOT_TOUCH_PATTERNS {
        return Err(format!(
            "A task can carry at most {MAX_MUST_NOT_TOUCH_PATTERNS} must-not-touch patterns; {} were given.",
            patterns.len()
        ));
    }
    let mut normalised: Vec<String> = Vec::with_capacity(patterns.len());
    for raw in patterns {
        let pattern = raw.trim();
        let pattern = pattern.strip_prefix("./").unwrap_or(pattern);
        if pattern.is_empty() {
            return Err("A must-not-touch pattern is empty.".to_owned());
        }
        if pattern.chars().count() > MAX_MUST_NOT_TOUCH_PATTERN_CHARS {
            return Err(format!(
                "The must-not-touch pattern \"{}…\" is longer than {MAX_MUST_NOT_TOUCH_PATTERN_CHARS} characters.",
                pattern.chars().take(40).collect::<String>()
            ));
        }
        if pattern.starts_with('/') || pattern.contains('\\') {
            return Err(format!(
                "The must-not-touch pattern \"{pattern}\" must be relative to the task's repository, with / between parts, like crates/swarm-terminal/**."
            ));
        }
        if pattern.split('/').any(|part| part == "..") {
            return Err(format!(
                "The must-not-touch pattern \"{pattern}\" climbs out of the repository with \"..\"."
            ));
        }
        // A changed path never has an empty part or a "." part, so a pattern
        // with one could never match and would read as a limit that holds.
        let parts = pattern.strip_suffix('/').unwrap_or(pattern);
        if parts.split('/').any(|part| part.is_empty() || part == ".") {
            return Err(format!(
                "The must-not-touch pattern \"{pattern}\" has an empty or \".\" part, so it could never match a changed path."
            ));
        }
        if pattern.chars().any(char::is_control) {
            return Err("A must-not-touch pattern contains a control character.".to_owned());
        }
        if !normalised.iter().any(|seen| seen == pattern) {
            normalised.push(pattern.to_owned());
        }
    }
    Ok(MustNotTouchPatterns(normalised))
}

/// Whether a changed path falls under a pattern.
///
/// - `*` matches any characters within one part of the path, `?` one.
/// - `**` as a whole part matches any number of parts, including none.
/// - A pattern ending in `/`, or with no `*` or `?` at all, also covers
///   everything under it: `web/node_modules` covers `web/node_modules/x/y.js`.
#[must_use]
pub fn path_matches(pattern: &str, path: &str) -> bool {
    let pattern = pattern.strip_prefix("./").unwrap_or(pattern);
    let path = path.strip_prefix("./").unwrap_or(path);
    if let Some(directory) = pattern.strip_suffix('/') {
        return path_matches(&format!("{directory}/**"), path);
    }
    if !pattern.contains(['*', '?']) {
        return path == pattern
            || path
                .strip_prefix(pattern)
                .is_some_and(|rest| rest.starts_with('/'));
    }
    let pattern: Vec<&str> = pattern.split('/').collect();
    let path: Vec<&str> = path.split('/').collect();
    parts_match(&pattern, &path)
}

/// Pattern parts against path parts, with `**` spanning parts. A table over
/// both, so a pattern full of `**` cannot make the match exponential.
fn parts_match(pattern: &[&str], path: &[&str]) -> bool {
    // reach[j]: the first i pattern parts can match the first j path parts.
    let mut reach = vec![false; path.len() + 1];
    reach[0] = true;
    for part in pattern {
        let mut next = vec![false; path.len() + 1];
        if *part == "**" {
            let mut any = false;
            for (j, slot) in next.iter_mut().enumerate() {
                any |= reach[j];
                *slot = any;
            }
        } else {
            for j in 1..=path.len() {
                next[j] = reach[j - 1] && part_matches(part, path[j - 1]);
            }
        }
        reach = next;
    }
    reach[path.len()]
}

/// One part, with `*` and `?` inside it.
fn part_matches(pattern: &str, text: &str) -> bool {
    let pattern: Vec<char> = pattern.chars().collect();
    let text: Vec<char> = text.chars().collect();
    let (mut p, mut t) = (0, 0);
    let mut star: Option<(usize, usize)> = None;
    while t < text.len() {
        // A `*` is tested first: as a wildcard it can always back off, and
        // taken as a literal against a `*` in the path it could not.
        if p < pattern.len() && pattern[p] == '*' {
            star = Some((p, t));
            p += 1;
        } else if p < pattern.len() && (pattern[p] == '?' || pattern[p] == text[t]) {
            p += 1;
            t += 1;
        } else if let Some((star_p, star_t)) = star {
            p = star_p + 1;
            t = star_t + 1;
            star = Some((star_p, star_t + 1));
        } else {
            return false;
        }
    }
    pattern[p..].iter().all(|c| *c == '*')
}

/// A changed path as git printed it, read back as the path itself.
///
/// Git prints a path holding a non-ASCII byte, a `"`, a `\` or a control
/// character inside double quotes, with C escapes, and a recorded commit keeps
/// it as printed. Compared as printed, it would match no pattern and hide a
/// crossing. A path git did not quote never starts with `"`, so it is returned
/// as it is. An escape git does not write is kept as written, and bytes that
/// are not UTF-8 read as U+FFFD, so the rest of the path still matches.
#[must_use]
pub fn unquoted_git_path(path: &str) -> Cow<'_, str> {
    let Some(inner) = path
        .strip_prefix('"')
        .and_then(|rest| rest.strip_suffix('"'))
    else {
        return Cow::Borrowed(path);
    };
    let mut bytes = Vec::with_capacity(inner.len());
    let mut rest = inner.as_bytes();
    while let Some((&byte, after)) = rest.split_first() {
        rest = after;
        if byte != b'\\' {
            bytes.push(byte);
            continue;
        }
        let named = match rest.first() {
            Some(b'a') => Some(0x07),
            Some(b'b') => Some(0x08),
            Some(b't') => Some(b'\t'),
            Some(b'n') => Some(b'\n'),
            Some(b'v') => Some(0x0b),
            Some(b'f') => Some(0x0c),
            Some(b'r') => Some(b'\r'),
            Some(b'"') => Some(b'"'),
            Some(b'\\') => Some(b'\\'),
            _ => None,
        };
        if let Some(named) = named {
            bytes.push(named);
            rest = &rest[1..];
            continue;
        }
        let digits = rest
            .iter()
            .take(3)
            .take_while(|digit| matches!(**digit, b'0'..=b'7'))
            .count();
        let value = rest[..digits]
            .iter()
            .fold(0_u32, |value, digit| value * 8 + u32::from(digit - b'0'));
        if digits > 0
            && let Ok(octal) = u8::try_from(value)
        {
            bytes.push(octal);
            rest = &rest[digits..];
        } else {
            bytes.push(b'\\');
        }
    }
    Cow::Owned(String::from_utf8_lossy(&bytes).into_owned())
}

/// One changed path that fell under a pattern.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CrossedPath {
    pub path: String,
    /// The first of the task's patterns it fell under.
    pub pattern: String,
}

/// The paths one commit changed that it must not have.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct PathCrossing {
    pub commit: String,
    pub paths: Vec<CrossedPath>,
    /// Crossed paths of this commit beyond those listed.
    pub more_paths: usize,
}

/// A task's patterns against what its commits changed.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct MustNotTouchReport {
    pub patterns: Vec<String>,
    /// How many recorded commits were compared.
    pub commits_compared: usize,
    pub crossings: Vec<PathCrossing>,
    /// Commits with crossings beyond those listed.
    pub more_commits: usize,
}

/// Compares a task's patterns with its recorded commits' changed paths, each
/// read back from git's quoting first, so a quoted name still crosses.
/// Nothing when the task has no patterns, so a task without them reads as it
/// always has.
#[must_use]
pub fn must_not_touch_report(
    patterns: &[String],
    report: Option<&TaskCommitReport>,
) -> Option<MustNotTouchReport> {
    if patterns.is_empty() {
        return None;
    }
    let commits = report.map_or(&[][..], |report| report.commits.as_slice());
    let mut crossings = Vec::new();
    let mut more_commits = 0;
    for commit in commits {
        let mut paths = Vec::new();
        let mut more_paths = 0;
        for recorded in &commit.changed_paths {
            let path = unquoted_git_path(recorded);
            let Some(pattern) = patterns.iter().find(|pattern| path_matches(pattern, &path)) else {
                continue;
            };
            if paths.len() < MAX_CROSSED_PATHS_PER_COMMIT {
                paths.push(CrossedPath {
                    path: path.into_owned(),
                    pattern: pattern.clone(),
                });
            } else {
                more_paths += 1;
            }
        }
        if paths.is_empty() {
            continue;
        }
        if crossings.len() < MAX_CROSSING_COMMITS {
            crossings.push(PathCrossing {
                commit: commit.sha.clone(),
                paths,
                more_paths,
            });
        } else {
            more_commits += 1;
        }
    }
    Some(MustNotTouchReport {
        patterns: patterns.to_vec(),
        commits_compared: commits.len(),
        crossings,
        more_commits,
    })
}

#[cfg(test)]
mod tests {
    use super::{
        MAX_CROSSED_PATHS_PER_COMMIT, MAX_MUST_NOT_TOUCH_PATTERNS, must_not_touch_report,
        path_matches, unquoted_git_path, validate_must_not_touch,
    };
    use crate::{CommitRepositoryState, CommitVerdict, TaskCommit, TaskCommitReport, TaskId};

    fn owned(patterns: &[&str]) -> Vec<String> {
        patterns
            .iter()
            .map(|pattern| (*pattern).to_owned())
            .collect()
    }

    fn commit(sha: &str, paths: &[&str]) -> TaskCommit {
        TaskCommit {
            sha: sha.to_owned(),
            verdict: CommitVerdict::Present,
            subject: "feat: x".to_owned(),
            changed_paths: owned(paths),
            found_in: None,
        }
    }

    fn report(commits: Vec<TaskCommit>) -> TaskCommitReport {
        TaskCommitReport {
            task_id: TaskId::new(),
            workspace: "/work".to_owned(),
            repository_state: CommitRepositoryState::Read,
            commits,
            reported_at: 1,
        }
    }

    #[test]
    fn patterns_cover_what_they_say() {
        let cases = [
            (
                "crates/swarm-terminal/**",
                "crates/swarm-terminal/src/lib.rs",
                true,
            ),
            (
                "crates/swarm-terminal/**",
                "crates/swarm-terminal-host/src/lib.rs",
                false,
            ),
            (
                "web/node_modules/**",
                "web/node_modules/react/index.js",
                true,
            ),
            ("web/node_modules", "web/node_modules/react/index.js", true),
            ("web/node_modules", "web/node_modules_extra/a.js", false),
            ("web/node_modules/", "web/node_modules/a.js", true),
            ("Cargo.lock", "Cargo.lock", true),
            ("Cargo.lock", "crates/x/Cargo.lock", false),
            ("**/Cargo.lock", "crates/x/Cargo.lock", true),
            ("**/Cargo.lock", "Cargo.lock", true),
            ("docs/*.md", "docs/a.md", true),
            ("docs/*.md", "docs/decisions/a.md", false),
            ("docs/**/*.md", "docs/decisions/a.md", true),
            ("src/?.rs", "src/a.rs", true),
            ("src/?.rs", "src/ab.rs", false),
            ("**/migrations/**", "crates/p/src/migrations/0001.sql", true),
            ("**", "anything/at/all", true),
            ("./packaging/**", "packaging/linux/x", true),
            // A `*` in the pattern is always a wildcard, even where the path
            // has a `*` of its own, so it never stops matching early.
            ("docs/*", "docs/*a.md", true),
            ("docs/a*", "docs/a*b.md", true),
            ("docs/*.md", "docs/*.md", true),
            ("docs/*b", "docs/*a", false),
        ];
        for (pattern, path, expected) in cases {
            assert_eq!(path_matches(pattern, path), expected, "{pattern} vs {path}");
        }
    }

    /// Many `**` parts against a long path stay quick: the match is a table,
    /// not a search.
    #[test]
    fn a_pattern_full_of_double_stars_is_not_exponential() {
        let pattern = format!("{}x", "**/".repeat(60));
        let path = format!("{}y", "a/".repeat(500));
        // No timing assertion: a matcher that backtracked over sixty `**`
        // parts would not return at all, so finishing is the evidence.
        assert!(!path_matches(&pattern, &path));
    }

    #[test]
    fn unusable_patterns_are_refused_with_a_reason() {
        for (bad, says) in [
            ("/etc/passwd", "relative"),
            ("crates\\x", "relative"),
            ("../other", "climbs out"),
            ("  ", "empty"),
            ("crates//x", "empty or"),
            ("crates/./x", "empty or"),
            (".", "empty or"),
            ("crates//", "empty or"),
        ] {
            let error = validate_must_not_touch(&owned(&[bad])).unwrap_err();
            assert!(error.contains(says), "{bad}: {error}");
        }
        let long = "a".repeat(300);
        assert!(
            validate_must_not_touch(&[long])
                .unwrap_err()
                .contains("longer")
        );
        let many: Vec<String> = (0..=MAX_MUST_NOT_TOUCH_PATTERNS)
            .map(|n| format!("p{n}"))
            .collect();
        assert!(
            validate_must_not_touch(&many)
                .unwrap_err()
                .contains("at most")
        );
    }

    #[test]
    fn patterns_are_trimmed_and_deduplicated_in_order() {
        assert_eq!(
            validate_must_not_touch(&owned(&[" web/** ", "./web/**", "Cargo.lock"]))
                .unwrap()
                .as_slice(),
            owned(&["web/**", "Cargo.lock"])
        );
        assert!(validate_must_not_touch(&[]).unwrap().is_empty());
    }

    #[test]
    fn no_patterns_reads_as_nothing_at_all() {
        let commits = report(vec![commit("aaa", &["web/a.ts"])]);
        assert_eq!(must_not_touch_report(&[], Some(&commits)), None);
    }

    #[test]
    fn crossings_name_the_commit_the_path_and_the_pattern() {
        let patterns = owned(&["crates/swarm-terminal/**", "Cargo.lock"]);
        let commits = report(vec![
            commit("aaa", &["web/a.ts"]),
            commit(
                "bbb",
                &["crates/swarm-terminal/src/ipc.rs", "web/b.ts", "Cargo.lock"],
            ),
        ]);

        let found = must_not_touch_report(&patterns, Some(&commits)).unwrap();

        assert_eq!(found.commits_compared, 2);
        assert_eq!(found.crossings.len(), 1);
        assert_eq!(found.crossings[0].commit, "bbb");
        let crossed: Vec<(&str, &str)> = found.crossings[0]
            .paths
            .iter()
            .map(|crossed| (crossed.path.as_str(), crossed.pattern.as_str()))
            .collect();
        assert_eq!(
            crossed,
            [
                (
                    "crates/swarm-terminal/src/ipc.rs",
                    "crates/swarm-terminal/**"
                ),
                ("Cargo.lock", "Cargo.lock")
            ]
        );
    }

    /// Git prints a path holding a non-ASCII byte, a `"`, a `\` or a control
    /// character inside quotes, with C escapes, and the record keeps it so.
    /// Read back, it is the path itself. A path git did not quote is untouched.
    #[test]
    fn a_path_git_quoted_reads_back_as_the_path_itself() {
        let cases = [
            ("docs/a.md", "docs/a.md"),
            (r#""docs/caf\303\251.md""#, "docs/caf\u{e9}.md"),
            (r#""web/say \"hi\".rs""#, "web/say \"hi\".rs"),
            (r#""web/back\\slash.rs""#, "web/back\\slash.rs"),
            (r#""web/tab\tname.rs""#, "web/tab\tname.rs"),
            (r#""a\a\b\f\n\r\v""#, "a\u{7}\u{8}\u{c}\n\r\u{b}"),
            // Bytes that are not UTF-8 leave the rest of the path readable.
            (r#""docs/caf\351.md""#, "docs/caf\u{fffd}.md"),
            // Not an escape git writes: kept as written rather than guessed at.
            (r#""a\qb""#, "a\\qb"),
            ("\"", "\""),
        ];
        for (recorded, expected) in cases {
            assert_eq!(unquoted_git_path(recorded), expected, "{recorded}");
        }
    }

    /// A crossing behind git's quoting is still a crossing, and it is shown as
    /// the path rather than as git printed it (ADR 0172).
    #[test]
    fn a_quoted_path_crosses_like_any_other() {
        let commits = report(vec![commit(
            "aaa",
            &[
                r#""crates/swarm-terminal/caf\303\251.rs""#,
                r#""crates/swarm-terminal/tab\tname.rs""#,
                "web/a.ts",
            ],
        )]);

        let found =
            must_not_touch_report(&owned(&["crates/swarm-terminal/**"]), Some(&commits)).unwrap();

        let crossed: Vec<&str> = found.crossings[0]
            .paths
            .iter()
            .map(|crossed| crossed.path.as_str())
            .collect();
        assert_eq!(
            crossed,
            [
                "crates/swarm-terminal/caf\u{e9}.rs",
                "crates/swarm-terminal/tab\tname.rs"
            ]
        );
    }

    #[test]
    fn patterns_with_no_commits_yet_compare_nothing_and_cross_nothing() {
        let found = must_not_touch_report(&owned(&["web/**"]), None).unwrap();
        assert_eq!(found.commits_compared, 0);
        assert!(found.crossings.is_empty());
    }

    #[test]
    fn a_huge_crossing_is_listed_within_bounds_and_counted() {
        let paths: Vec<String> = (0..500).map(|n| format!("vendor/{n}.js")).collect();
        let refs: Vec<&str> = paths.iter().map(String::as_str).collect();
        let commits = report(vec![commit("aaa", &refs)]);
        let found = must_not_touch_report(&owned(&["vendor/**"]), Some(&commits)).unwrap();
        assert_eq!(found.crossings[0].paths.len(), MAX_CROSSED_PATHS_PER_COMMIT);
        assert_eq!(
            found.crossings[0].more_paths,
            500 - MAX_CROSSED_PATHS_PER_COMMIT
        );
    }
}
