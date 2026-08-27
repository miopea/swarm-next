//! Signs the release manifest. Never shipped: this is the other half of the
//! key whose public part is compiled into every build, and it belongs on the
//! machine that cuts releases, not on the machines that install them.
//!
//! ```text
//! swarm-release keygen PRIVATE_KEY_PATH   writes the key, prints only the public half
//! swarm-release sign PRIVATE_KEY_PATH     unsigned payload on stdin, signed document on stdout
//! swarm-release notes REPO_ROOT VERSION   release notes JSON on stdout, for the bundle
//! ```

use std::collections::HashMap;
use std::env;
use std::fs;
use std::io::{self, Read, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;
use std::process::ExitCode;

use base64ct::{Base64UrlUnpadded, Encoding};
use ed25519_dalek::{Signer, SigningKey};
use swarm_domain::{
    RELEASE_MANIFEST_SCHEMA_VERSION, ReleaseManifest, ReleaseNote, ReleaseNotes,
    ReleaseVersionNotes, SignedReleaseManifest, canonical_release_manifest,
};

const USAGE: &str = "usage: swarm-release <keygen PRIVATE_KEY_PATH | sign PRIVATE_KEY_PATH | notes REPO_ROOT VERSION>";

/// How many past releases the notes carry, beyond the one being built.
///
/// A Hive that skipped versions has to find what it missed inside the ONE
/// artifact it downloads, so this is the depth of "away for a while" the modal
/// can still account for. Raised from 5 once the panel started anchoring on the
/// release an operator actually came from rather than on what their browser
/// remembered: the anchor can now reach back further than five, and notes that
/// stop short of it are a silent gap in the one thing the panel promises.
///
/// Still bounded -- the file ships inside every artifact, and a panel listing
/// forty releases is a changelog, which is what this exists instead of. When
/// the gap is genuinely deeper than this the panel SAYS so rather than
/// presenting a partial list as the whole story.
const NOTES_DEPTH: usize = 12;

fn main() -> ExitCode {
    let mut arguments = env::args().skip(1);
    let result = match (
        arguments.next().as_deref(),
        arguments.next(),
        arguments.next(),
    ) {
        (Some("keygen"), Some(path), None) => keygen(Path::new(&path)),
        (Some("sign"), Some(path), None) => sign(Path::new(&path)),
        (Some("notes"), Some(root), Some(version)) => notes(Path::new(&root), &version),
        _ => Err(USAGE.to_owned()),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("swarm-release: {message}");
            ExitCode::FAILURE
        }
    }
}

/// Writes a new signing key, and prints the verifying key and nothing else.
///
/// The private half is never printed, never logged, and never returned: the
/// only way to read it back is the file, which is created 0600 and refuses to
/// overwrite an existing one. Losing this key strands every install that
/// carries its public half, so it belongs in 1Password immediately.
fn keygen(path: &Path) -> Result<(), String> {
    if path.exists() {
        return Err(format!(
            "{} already exists; refusing to overwrite a signing key",
            path.display()
        ));
    }
    let mut seed = [0u8; 32];
    getrandom::fill(&mut seed).map_err(|error| format!("no secure randomness: {error}"))?;
    let signing_key = SigningKey::from_bytes(&seed);
    let verifying_key = Base64UrlUnpadded::encode_string(signing_key.verifying_key().as_bytes());

    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .map_err(|error| format!("could not create {}: {error}", path.display()))?;
    file.write_all(Base64UrlUnpadded::encode_string(&seed).as_bytes())
        .and_then(|()| file.write_all(b"\n"))
        .map_err(|error| format!("could not write {}: {error}", path.display()))?;

    println!("{verifying_key}");
    eprintln!(
        "Signing key written to {}, readable only by you.\n\
         Store it in 1Password now and delete the file. It cannot be recovered,\n\
         and every install carrying the key above verifies nothing without it.",
        path.display()
    );
    Ok(())
}

/// Signs an unsigned manifest payload read from stdin.
fn sign(path: &Path) -> Result<(), String> {
    let encoded = fs::read_to_string(path)
        .map_err(|error| format!("could not read {}: {error}", path.display()))?;
    let seed: [u8; 32] = Base64UrlUnpadded::decode_vec(encoded.trim())
        .map_err(|_| "signing key is not base64url".to_owned())?
        .try_into()
        .map_err(|_| "signing key is not 32 bytes".to_owned())?;
    let signing_key = SigningKey::from_bytes(&seed);

    let mut document = String::new();
    io::stdin()
        .read_to_string(&mut document)
        .map_err(|error| format!("could not read the payload: {error}"))?;
    let payload: ReleaseManifest = serde_json::from_str(&document)
        .map_err(|error| format!("payload is not a manifest: {error}"))?;
    if payload.schema_version != RELEASE_MANIFEST_SCHEMA_VERSION {
        return Err(format!(
            "payload declares schema {} but this tool signs schema {RELEASE_MANIFEST_SCHEMA_VERSION}",
            payload.schema_version
        ));
    }

    let canonical = canonical_release_manifest(&payload)
        .map_err(|_| "payload could not be encoded".to_owned())?;
    let signed = SignedReleaseManifest {
        signature: Base64UrlUnpadded::encode_string(&signing_key.sign(&canonical).to_bytes()),
        payload,
    };
    let rendered = serde_json::to_string_pretty(&signed)
        .map_err(|error| format!("signed manifest could not be encoded: {error}"))?;
    println!("{rendered}");
    Ok(())
}

/// Reads git and writes what changed, for the bundle rather than the manifest.
///
/// Generated at BUILD time because that is where the history is: the artifact
/// is what travels, and the manifest cannot carry this without breaking older
/// Hives' signature verification. See `ReleaseNotes` for why.
fn notes(root: &Path, version: &str) -> Result<(), String> {
    let tags = release_tags(root)?;
    let mut releases = Vec::new();

    // The newest entry ends at HEAD rather than at its own tag: build-release
    // runs while cutting the release, so the tag for it may not exist yet.
    let mut ranges: Vec<(String, String, String)> = Vec::new();
    let newest_previous = tags.first().cloned();
    ranges.push((
        version.to_owned(),
        newest_previous.clone().unwrap_or_default(),
        "HEAD".to_owned(),
    ));
    for pair in tags.windows(2).take(NOTES_DEPTH) {
        let (current, previous) = (&pair[0], &pair[1]);
        ranges.push((
            current.trim_start_matches('v').to_owned(),
            previous.clone(),
            current.clone(),
        ));
    }

    // WHAT THE RELEASER WROTE BEATS WHAT THE COMMITS SAY, per release.
    //
    // Applied to every release in the bundle rather than only the one being
    // cut: the artifact carries a dozen, so a Hive updating across several
    // gets the written notes for each release that has them and generated ones
    // for the rest.
    let written = written_notes(root);
    // ONE ENTRY PER VERSION, and this became load-bearing the moment written
    // notes existed. The first range ends at HEAD because the tag for the
    // release being cut may not exist yet -- but when it DOES exist, the
    // windows(2) pass below produces that same version a second time. That was
    // invisible while notes came only from commits: the HEAD range was empty
    // and the `is_empty` guard dropped it. A written section is never empty, so
    // both survived and the panel listed the release twice.
    let mut seen: Vec<String> = Vec::new();
    for (release_version, from, to) in ranges {
        if seen.contains(&release_version) {
            continue;
        }
        seen.push(release_version.clone());
        let notes = if let Some(written) = written.get(&release_version) {
            // SAID OUT LOUD, because the alternative failure is silent. A
            // releaser who writes a section for 0.8.20 and then cuts 0.9.0
            // gets generation, ships the rough notes, and nothing tells them
            // their prose was ignored -- the output looks the same either way.
            eprintln!(
                "swarm-release: {release_version} notes come from {WRITTEN_NOTES_FILE} ({} entries)",
                written.len()
            );
            written.clone()
        } else {
            let generated = notes_between(root, &from, &to)?;
            eprintln!(
                "swarm-release: {release_version} notes generated from commit subjects ({} entries); no {WRITTEN_NOTES_FILE} section",
                generated.len()
            );
            generated
        };
        if !notes.is_empty() {
            releases.push(ReleaseVersionNotes {
                version: release_version,
                notes,
            });
        }
    }

    let document = ReleaseNotes {
        schema: 1,
        releases,
    };
    let encoded = serde_json::to_vec(&document)
        .map_err(|error| format!("could not encode notes: {error}"))?;
    io::stdout()
        .write_all(&encoded)
        .map_err(|error| format!("could not write notes: {error}"))
}

/// The file a releaser writes to say what a release means, in their words.
const WRITTEN_NOTES_FILE: &str = "RELEASE_NOTES.md";

/// The phrase that marks an entry as installed but not yet in effect.
///
/// It is the sentence the panel already renders, so what the releaser types is
/// what the operator reads and there is no second vocabulary to learn. Parsed
/// off the end and replaced by the flag, so it is not printed twice.
const ENGINE_MARKER: &str = "(after the worker engine update)";

/// Notes the releaser wrote, keyed by the version they describe.
///
/// REPLACES the generated list for a release rather than annotating it. A
/// releaser who writes notes has decided what the release says; interleaving
/// their lines with generated ones produces a list where nobody can tell which
/// is which, and the generated half is exactly what they were overriding.
///
/// A MISSING FILE IS NOT AN ERROR, and that is the load-bearing part. A
/// release that silently ships NO notes because someone forgot the file is
/// worse than one that ships rough ones, so every failure here has to fall
/// toward saying something. Unreadable and unparseable are the same as absent.
///
/// THE RETURN TYPE IS NOT A `Result`, deliberately. There is no failure this
/// function is allowed to have: every way of not getting notes -- no file, no
/// permission, malformed content -- has to end as "generate them instead",
/// because the one outcome worse than rough notes is a release that stops.
///
/// The format is the one the panel renders, so it reads as what it produces:
///
/// ```text
/// ## 0.8.20
///
/// ### New features
/// - A shell can be opened in a worker's workspace (after the worker engine update)
///
/// ### Fixes
/// - The blocked-escalation card lays out as one card
/// ```
fn written_notes(root: &Path) -> HashMap<String, Vec<ReleaseNote>> {
    let path = root.join(WRITTEN_NOTES_FILE);
    let Ok(raw) = fs::read_to_string(&path) else {
        return HashMap::new();
    };
    let mut sections: HashMap<String, Vec<ReleaseNote>> = HashMap::new();
    let mut version: Option<String> = None;
    // `feat` until a Fixes heading says otherwise, matching the generator's
    // vocabulary so both halves produce the same shape of entry.
    let mut kind = "feat";
    // A BULLET IS NOT A LINE. Markdown wraps, this repository wraps at 80, and
    // the first version of this read only the `- ` line -- so a releaser who
    // wrapped a sentence shipped its first fragment and lost the rest, with
    // nothing to show anything had gone missing. Continuations are gathered
    // until the next bullet, heading, or blank line closes the entry.
    let mut pending: Option<String> = None;

    for line in raw.lines() {
        let trimmed = line.trim();
        let is_heading = trimmed.starts_with('#');
        let is_bullet = trimmed.starts_with("- ");
        if (is_heading || is_bullet || trimmed.is_empty())
            && let Some(entry) = pending.take()
        {
            push_written_note(&mut sections, version.as_ref(), kind, &entry);
        }
        if let Some(heading) = trimmed.strip_prefix("## ") {
            version = Some(heading.trim().trim_start_matches('v').to_owned());
            kind = "feat";
        } else if let Some(heading) = trimmed.strip_prefix("### ") {
            kind = if heading.trim().eq_ignore_ascii_case("fixes") {
                "fix"
            } else {
                "feat"
            };
        } else if let Some(entry) = trimmed.strip_prefix("- ") {
            pending = Some(entry.trim().to_owned());
        } else if !trimmed.is_empty()
            && !is_heading
            && let Some(entry) = pending.as_mut()
        {
            entry.push(' ');
            entry.push_str(trimmed);
        }
    }
    if let Some(entry) = pending.take() {
        push_written_note(&mut sections, version.as_ref(), kind, &entry);
    }
    sections
}

/// Files one finished bullet under the release it belongs to.
///
/// A bullet before any version heading belongs to no release, and is dropped
/// rather than guessed at: attaching it to whichever release comes next would
/// put a line in front of operators that nobody meant for them.
fn push_written_note(
    sections: &mut HashMap<String, Vec<ReleaseNote>>,
    version: Option<&String>,
    kind: &str,
    entry: &str,
) {
    let Some(version) = version else { return };
    let entry = entry.trim();
    let (summary, needs_worker_engine_update) = entry
        .strip_suffix(ENGINE_MARKER)
        .map_or((entry, false), |trimmed| (trimmed.trim_end(), true));
    if summary.is_empty() {
        return;
    }
    sections
        .entry(version.clone())
        .or_default()
        .push(ReleaseNote {
            summary: summary.to_owned(),
            kind: kind.to_owned(),
            needs_worker_engine_update,
        });
}

/// Release tags newest first. Development and non-release tags are ignored.
fn release_tags(root: &Path) -> Result<Vec<String>, String> {
    let listed = git(root, &["tag", "--list", "v*", "--sort=-v:refname"])?;
    Ok(listed
        .lines()
        .map(str::trim)
        .filter(|tag| !tag.is_empty())
        .map(ToOwned::to_owned)
        .collect())
}

/// The operator-facing changes in a range.
///
/// Filtered to feat and fix because everything else -- release commits,
/// packaging churn, chores -- is true and not news. This repo uses the prefixes
/// consistently, so the filter is a real signal rather than a hopeful one.
fn notes_between(root: &Path, from: &str, to: &str) -> Result<Vec<ReleaseNote>, String> {
    let range = if from.is_empty() {
        to.to_owned()
    } else {
        format!("{from}..{to}")
    };
    // A record separator no commit subject contains, so a subject with a colon
    // or a pipe in it cannot split the line.
    let listed = git(root, &["log", "--no-merges", "--format=%H\x1f%s", &range])?;
    // A commit undone inside this range never reached anybody, so announcing it
    // would be a release advertising something it does not contain. 0.8.18
    // would have told every developer that alpha providers had shipped; they
    // were reverted three hours before the tag.
    //
    // Matched on the subject git itself writes -- `Revert "<original subject>"`
    // -- because that is the only link between the two commits that survives.
    // A revert whose subject was hand-edited is missed, and that is the honest
    // limit: it silences a note rather than inventing one.
    let reverted: std::collections::HashSet<String> = listed
        .lines()
        .filter_map(|line| line.split_once('\u{1f}'))
        .filter_map(|(_, subject)| {
            subject
                .strip_prefix("Revert \"")
                .and_then(|rest| rest.strip_suffix('"'))
                .map(ToOwned::to_owned)
        })
        .collect();

    let mut notes = Vec::new();
    for line in listed.lines() {
        let Some((sha, subject)) = line.split_once('\u{1f}') else {
            continue;
        };
        if reverted.contains(subject) {
            continue;
        }
        let Some((kind, summary)) = conventional_note(subject) else {
            continue;
        };
        notes.push(ReleaseNote {
            summary: summary.to_owned(),
            kind: kind.to_owned(),
            needs_worker_engine_update: touches_worker_engine(root, sha)?,
        });
    }
    Ok(notes)
}

/// Splits a conventional-commit subject into kind and summary.
///
/// Returns None for anything that is not feat or fix, including a subject that
/// merely starts with those letters -- "feature flags removed" is not a feat.
fn conventional_note(subject: &str) -> Option<(&'static str, &str)> {
    let (prefix, rest) = subject.split_once(':')?;
    let kind = match prefix.split_once('(').map_or(prefix, |(head, _)| head) {
        "feat" => "feature",
        "fix" => "fix",
        _ => return None,
    };
    let summary = rest.trim();
    (!summary.is_empty()).then_some((kind, summary))
}

/// Whether this commit changed code that only a worker engine update replaces.
///
/// The terminal host is a separate service that survives an API restart on
/// purpose, so a change to it is installed and NOT running until that update
/// happens. Computed from the paths the commit touched rather than guessed from
/// its wording, because the wording is written by whoever made the change and
/// the paths are not.
fn touches_worker_engine(root: &Path, sha: &str) -> Result<bool, String> {
    let changed = git(
        root,
        &["show", "--name-only", "--format=", "--no-renames", sha],
    )?;
    Ok(changed.lines().map(str::trim).any(|path| {
        path.starts_with("crates/swarm-terminal-host/")
            || path == "crates/swarm-terminal/src/ipc.rs"
            || path.starts_with("crates/swarm-terminal/src/provider")
    }))
}

fn git(root: &Path, arguments: &[&str]) -> Result<String, String> {
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(root)
        .args(arguments)
        .output()
        .map_err(|error| format!("could not run git: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "git {} failed: {}",
            arguments.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    String::from_utf8(output.stdout).map_err(|_| "git returned invalid UTF-8".to_owned())
}

#[cfg(test)]
mod tests {
    use super::{conventional_note, written_notes};

    /// Writes a `RELEASE_NOTES.md` into a throwaway directory and parses it.
    fn parse(body: &str) -> std::collections::HashMap<String, Vec<swarm_domain::ReleaseNote>> {
        let root = std::env::temp_dir().join(format!("swarm-release-notes-{}", body.len()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("RELEASE_NOTES.md"), body).unwrap();
        let parsed = written_notes(&root);
        let _ = std::fs::remove_dir_all(&root);
        parsed
    }

    /// A releaser's sentence survives being wrapped.
    ///
    /// THE ABLATION IS THE POINT. Reading only the `- ` line parses without
    /// error and produces a note -- just a truncated one -- so nothing looks
    /// wrong anywhere. This repository wraps prose at 80 columns, so the first
    /// person to use the feature as intended would have shipped a fragment.
    #[test]
    fn a_wrapped_bullet_keeps_the_whole_sentence() {
        let parsed = parse(
            "## 0.9.0\n\n### Fixes\n- A Hive with Outlook registered and no public address\n  starts again, instead of exiting on boot and being\n  restart-looped\n",
        );
        let notes = &parsed["0.9.0"];
        assert_eq!(notes.len(), 1, "one bullet, however many lines it took");
        assert_eq!(
            notes[0].summary,
            "A Hive with Outlook registered and no public address starts again, instead of exiting on boot and being restart-looped"
        );
    }

    /// The marker the panel already renders is the one the releaser types.
    #[test]
    fn the_engine_marker_sets_the_flag_and_is_not_printed_twice() {
        let parsed = parse(
            "## 0.9.0\n\n### New features\n- A shell opens in a worker's workspace (after the worker engine update)\n- An ordinary change\n",
        );
        let notes = &parsed["0.9.0"];
        assert!(notes[0].needs_worker_engine_update);
        assert_eq!(notes[0].summary, "A shell opens in a worker's workspace");
        assert!(!notes[1].needs_worker_engine_update);
    }

    /// Headings decide the section a bullet lands in.
    #[test]
    fn new_features_and_fixes_are_separated_by_their_headings() {
        let parsed = parse(
            "## 0.9.0\n\n### New features\n- Something new\n\n### Fixes\n- Something repaired\n",
        );
        let notes = &parsed["0.9.0"];
        assert_eq!(notes[0].kind, "feat");
        assert_eq!(notes[1].kind, "fix");
    }

    /// Several releases can be described in one file.
    #[test]
    fn each_version_gets_only_the_bullets_under_its_own_heading() {
        let parsed = parse("## 0.9.0\n\n- Newer\n\n## 0.8.19\n\n- Older\n");
        assert_eq!(parsed["0.9.0"].len(), 1);
        assert_eq!(parsed["0.9.0"][0].summary, "Newer");
        assert_eq!(parsed["0.8.19"].len(), 1);
        assert_eq!(parsed["0.8.19"][0].summary, "Older");
    }

    /// Prose above the first version heading is not somebody's release note.
    #[test]
    fn a_bullet_before_any_version_heading_reaches_no_operator() {
        let parsed = parse("# Release notes\n\n- How to use this file\n\n## 0.9.0\n\n- Real\n");
        assert_eq!(parsed["0.9.0"].len(), 1);
        assert_eq!(parsed["0.9.0"][0].summary, "Real");
    }

    /// Absent has to keep working, because the failure must fall toward
    /// saying something. A release that ships NO notes because someone forgot
    /// the file is worse than one that ships rough ones.
    #[test]
    fn a_missing_file_is_not_an_error_and_falls_back_to_generation() {
        let root = std::env::temp_dir().join("swarm-release-notes-absent");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        assert!(written_notes(&root).is_empty());
        let _ = std::fs::remove_dir_all(&root);
    }

    /// A commit undone inside the same release is not news.
    ///
    /// 0.8.18 would have announced alpha providers to every developer. They
    /// were reverted three hours before the tag, so the release did not contain
    /// them -- a release advertising what it does not carry is worse than one
    /// that says less.
    #[test]
    fn a_reverted_commit_is_not_announced() {
        // The shape git writes, which is the only link between the pair that
        // survives into the log.
        let subject = "feat: gemini, grok and opencode ship as alpha providers";
        let revert = format!("Revert \"{subject}\"");
        assert_eq!(
            revert
                .strip_prefix("Revert \"")
                .and_then(|rest| rest.strip_suffix('"')),
            Some(subject),
            "the revert must name the subject it undoes"
        );

        // And a revert is itself not a note: it carries no conventional prefix.
        assert_eq!(conventional_note(&revert), None);
    }

    /// The filter reads the conventional-commit PREFIX, not the first letters.
    #[test]
    fn only_a_real_conventional_prefix_becomes_a_note() {
        assert_eq!(
            conventional_note("feat: a shell opens from the menu"),
            Some(("feature", "a shell opens from the menu"))
        );
        assert_eq!(
            conventional_note("fix(orchestration): surface delivered work"),
            Some(("fix", "surface delivered work"))
        );

        // Not news, and each is a real subject shape from this repo's history.
        assert_eq!(conventional_note("chore: bump deps"), None);
        assert_eq!(conventional_note("docs: record the ruling"), None);
        assert_eq!(conventional_note("release: 0.8.17"), None);
        assert_eq!(conventional_note("style: cargo fmt"), None);

        // The trap this function exists for: a subject that merely STARTS with
        // the letters of a kind is not that kind.
        assert_eq!(conventional_note("feature flags are gone"), None);
        assert_eq!(conventional_note("fixture cleanup: drop the stub"), None);

        // A prefix with nothing after it says nothing, so it is not a note.
        assert_eq!(conventional_note("feat:   "), None);
        assert_eq!(conventional_note("no colon here at all"), None);
    }
}
