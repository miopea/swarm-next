//! Signs the release manifest. Never shipped: this is the other half of the
//! key whose public part is compiled into every build, and it belongs on the
//! machine that cuts releases, not on the machines that install them.
//!
//! ```text
//! swarm-release keygen PRIVATE_KEY_PATH   writes the key, prints only the public half
//! swarm-release sign PRIVATE_KEY_PATH     unsigned payload on stdin, signed document on stdout
//! swarm-release notes REPO_ROOT VERSION   release notes JSON on stdout, for the bundle
//! ```

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
/// can still account for. Bounded because the file ships in every release and
/// nobody reads back further than this.
const NOTES_DEPTH: usize = 5;

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

    for (release_version, from, to) in ranges {
        let notes = notes_between(root, &from, &to)?;
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
    let mut notes = Vec::new();
    for line in listed.lines() {
        let Some((sha, subject)) = line.split_once('\u{1f}') else {
            continue;
        };
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
    use super::conventional_note;

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
