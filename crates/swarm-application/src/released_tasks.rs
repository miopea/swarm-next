//! Which release, if any, carries all of a task's work.
//!
//! ⚠️ THE BOARD UNDERSTATED WHAT HAD SHIPPED, and that is the worse direction
//! for it to be wrong in. On 2026-09-11 the operator asked why three tickets sat
//! in Awaiting Release; two of them had been in people's hands since v1.7.1 the
//! day before. Awaiting Release completes itself the moment a deployment is
//! recorded, and nobody recorded one, so the board said "waiting" about finished
//! work for a day.
//!
//! The remedy is not to remember harder. `git tag --contains` already answers
//! which release carried a commit, and the commits are already on the record, so
//! the answer can be computed rather than typed.
//!
//! The policy lives here, away from git and the database, because the judgement
//! it encodes -- what counts as "this release carried this task" -- is the part
//! worth being able to test exhaustively.

use std::fmt::Write as _;

use swarm_persistence::MAX_DEPLOYMENT_FIELD_BYTES;

/// The earliest tag that contains EVERY commit recorded for a task.
///
/// `tags_by_commit` holds one entry per recorded commit: the tags containing
/// that commit, in the order git reported them, which is version order. The
/// answer is the first tag common to all of them -- the release the work first
/// reached people in, not the newest one still carrying it.
///
/// ALL OR NOTHING, DELIBERATELY. A tag carrying some of a task's commits means
/// the task is not fully released, and this returning a tag closes the ticket.
/// Half-shipped work stays visibly in Awaiting Release until a release carries
/// the rest, which is what that state is for and what a reader of the board
/// would expect it to mean.
///
/// NO COMMITS IS NOT EVERY COMMIT. A task that reported an empty list built
/// nothing, so no release can carry it, and an empty slice answers `None` rather
/// than vacuously true -- which would auto-close every no-code task in Awaiting
/// Release the moment any tag existed.
#[must_use]
pub fn release_carrying_every_commit(tags_by_commit: &[Vec<String>]) -> Option<String> {
    let (first, rest) = tags_by_commit.split_first()?;
    first
        .iter()
        .find(|tag| rest.iter().all(|tags| tags.contains(tag)))
        .cloned()
}

/// Marks a reference the budget cut short, so a partial claim reads as one.
const ELLIPSIS: &str = "...";

/// How many commits the reference names before it says "and N more".
///
/// The reference has a hard byte budget and a task can carry dozens of commits,
/// so the list is the part that gives way. Five is enough to recognise the work
/// without the sentence becoming a manifest.
const NAMED_COMMITS: usize = 5;

/// The evidence sentence recorded against a task a release carried.
///
/// ⚠️ IT MUST FIT, AND OVERRUNNING IT FAILS SILENTLY. `record_task_deployment`
/// refuses a reference longer than [`MAX_DEPLOYMENT_FIELD_BYTES`], and the
/// caller here is a background sweep with nobody reading its return value: an
/// over-long reference would present as the ticket quietly never closing, which
/// is the exact failure this whole mechanism exists to end.
///
/// SAYS WHAT WAS CHECKED AND WHAT WAS NOT. A tag proves the work was released,
/// not that any particular machine is running it, and a reader six weeks from
/// now cannot tell those apart from a version number alone.
#[must_use]
pub fn release_deployment_reference(tag: &str, shas: &[String], workspace: &str) -> String {
    let mut named = shas
        .iter()
        .take(NAMED_COMMITS)
        .map(|sha| sha.chars().take(12).collect::<String>())
        .collect::<Vec<_>>()
        .join(", ");
    if shas.len() > NAMED_COMMITS {
        let _ = write!(named, " and {} more", shas.len() - NAMED_COMMITS);
    }
    let reference = format!(
        "{tag}: git tag --contains reports this release carries every commit recorded here \
         ({named}), and the tagged commit is on a remote branch, so the tag is pushed rather \
         than local only. Recorded automatically when the tag appeared in {workspace}. No \
         running version was read: this is evidence the work was RELEASED, not that a \
         particular machine is running it."
    );
    truncate_to_budget(reference)
}

/// Keeps a reference inside the store's limit on a character boundary.
///
/// The tail is what gives way rather than the head, because the tag and the
/// commits are the checkable part and the explanation is the part a reader can
/// reconstruct. An elision is marked so nobody reads a cut sentence as the whole
/// claim.
fn truncate_to_budget(reference: String) -> String {
    if reference.len() <= MAX_DEPLOYMENT_FIELD_BYTES {
        return reference;
    }
    let mut cut = MAX_DEPLOYMENT_FIELD_BYTES - ELLIPSIS.len();
    while cut > 0 && !reference.is_char_boundary(cut) {
        cut -= 1;
    }
    let mut truncated = reference[..cut].to_owned();
    truncated.push_str(ELLIPSIS);
    truncated
}

#[cfg(test)]
mod tests {
    use super::{
        MAX_DEPLOYMENT_FIELD_BYTES, release_carrying_every_commit, release_deployment_reference,
    };

    fn tags(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    #[test]
    fn the_release_a_task_first_reached_people_in_is_the_one_reported() {
        // Both commits are carried by v1.7.1 and still carried by v1.8.0. The
        // honest answer is the earlier one: that is when it shipped.
        let carried = release_carrying_every_commit(&[
            tags(&["v1.7.1", "v1.8.0"]),
            tags(&["v1.7.1", "v1.8.0"]),
        ]);
        assert_eq!(carried.as_deref(), Some("v1.7.1"));
    }

    #[test]
    fn a_release_carrying_only_some_of_the_work_is_passed_over_for_one_that_carries_all() {
        // The second commit landed after v1.7.1 was cut. Reporting v1.7.1 here
        // would close a ticket that still has unreleased work in it.
        let carried =
            release_carrying_every_commit(&[tags(&["v1.7.1", "v1.8.0"]), tags(&["v1.8.0"])]);
        assert_eq!(carried.as_deref(), Some("v1.8.0"));
    }

    #[test]
    fn a_commit_no_tag_contains_holds_the_whole_task_back() {
        let carried = release_carrying_every_commit(&[tags(&["v1.8.0"]), tags(&[])]);
        assert_eq!(carried, None);
    }

    #[test]
    fn a_task_that_built_nothing_is_carried_by_nothing() {
        // Vacuous truth would be a catastrophe here: every documentation task
        // parked in Awaiting Release would close itself against the next tag.
        assert_eq!(release_carrying_every_commit(&[]), None);
    }

    #[test]
    fn the_reference_names_the_tag_the_commits_and_what_was_not_checked() {
        let reference = release_deployment_reference(
            "v1.8.1",
            &["5d0b76bb".to_owned(), "44203afe".to_owned()],
            "/home/operator/projects/swarm-next",
        );
        assert!(reference.starts_with("v1.8.1:"), "{reference}");
        assert!(reference.contains("5d0b76bb"), "{reference}");
        assert!(reference.contains("44203afe"), "{reference}");
        assert!(
            reference.contains("/home/operator/projects/swarm-next"),
            "{reference}"
        );
        // The claim this record must never be mistaken for.
        assert!(reference.contains("not that a"), "{reference}");
        assert!(reference.len() <= MAX_DEPLOYMENT_FIELD_BYTES, "{reference}");
    }

    #[test]
    fn a_reference_stays_inside_the_store_limit_however_much_it_is_given() {
        // THE SILENT FAILURE THIS GUARDS: over the limit, record_task_deployment
        // refuses, and a background sweep with nobody reading its result shows
        // that as the ticket simply never closing.
        let shas = (0..200)
            .map(|index| format!("{index:040x}"))
            .collect::<Vec<_>>();
        let reference = release_deployment_reference(
            &"v1.8.1-with-an-unreasonable-name".repeat(20),
            &shas,
            &"/home/operator/a/very/deeply/nested/workspace".repeat(20),
        );
        assert!(
            reference.len() <= MAX_DEPLOYMENT_FIELD_BYTES,
            "{} bytes",
            reference.len()
        );
        assert!(reference.ends_with("..."), "an elision must be visible");
    }

    #[test]
    fn a_long_commit_list_is_summarised_rather_than_dropped() {
        let shas = (0..9)
            .map(|index| format!("{index:012x}"))
            .collect::<Vec<_>>();
        let reference = release_deployment_reference("v2.0.0", &shas, "/w");
        assert!(reference.contains("and 4 more"), "{reference}");
    }
}
