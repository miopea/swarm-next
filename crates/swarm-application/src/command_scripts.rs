//! The local script a command card names: its hash, and the copy of its text
//! Swarm keeps for whoever answers the card (ADR 0129, ADR 0187).
//!
//! ⚠️ ONE READ PINS THE HASH AND THE TEXT. The text a card shows is taken from
//! the same bytes the hash is taken from, never from a second read, so a file
//! rewritten between the two cannot show one script and pin another.

use std::fmt::Write as _;
use std::fs::OpenOptions;
use std::io::Read as _;
use std::path::Path;

use sha2::{Digest as _, Sha256};
use swarm_persistence::{FiledScript, MAX_SCRIPT_TEXT_BYTES, ScriptSnapshot};

/// The largest script Swarm hashes for a command grant.
pub const MAX_GRANTED_SCRIPT_BYTES: u64 = 64 * 1024 * 1024;

/// Lowercase hex sha256 of a script a command grant names, or `None` when it
/// is not an absolute path to a file that can be read whole (ADR 0129).
#[must_use]
pub fn script_sha256(path: &str) -> Option<String> {
    read(path, 0).map(|(sha256, _, _)| sha256)
}

/// The script a card names, hashed, with what Swarm keeps of its text. `None`
/// when it is not an absolute path to a file that can be read whole.
#[must_use]
pub fn read_script(path: &str) -> Option<FiledScript> {
    let (sha256, head, len) = read(path, MAX_SCRIPT_TEXT_BYTES)?;
    Some(FiledScript {
        script: swarm_domain::RequestedScript {
            path: path.to_owned(),
            sha256,
        },
        snapshot: snapshot(&head, len),
    })
}

/// Hashes the whole file and keeps at most `keep + 1` of its first bytes, so
/// a caller can tell a file that fits from one that does not.
///
/// Opened without blocking: a worker names this path, and opening a FIFO to
/// read waits for a writer, which would hold an API thread indefinitely.
fn read(path: &str, keep: usize) -> Option<(String, Vec<u8>, u64)> {
    use std::os::unix::fs::OpenOptionsExt as _;
    if !Path::new(path).is_absolute() {
        return None;
    }
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(nix::libc::O_NONBLOCK)
        .open(path)
        .ok()?;
    if !file.metadata().ok()?.is_file() {
        return None;
    }
    let mut limited = file.take(MAX_GRANTED_SCRIPT_BYTES + 1);
    let mut hasher = Sha256::new();
    let mut head = Vec::new();
    let mut len: u64 = 0;
    let mut buffer = [0_u8; 16 * 1024];
    loop {
        let read = limited.read(&mut buffer).ok()?;
        if read == 0 {
            break;
        }
        let chunk = buffer.get(..read)?;
        hasher.update(chunk);
        let room = (keep + 1).saturating_sub(head.len());
        head.extend_from_slice(chunk.get(..room.min(read))?);
        len += u64::try_from(read).ok()?;
    }
    if len > MAX_GRANTED_SCRIPT_BYTES {
        return None;
    }
    let mut hex = String::with_capacity(64);
    for byte in hasher.finalize() {
        write!(&mut hex, "{byte:02x}").expect("writing to an in-memory String cannot fail");
    }
    Some((hex, head, len))
}

/// What a card keeps of a script it read in full: its text, or why none.
///
/// The value of anything that looks like a secret is never kept or shown,
/// only where it is and what it looks like, so the approver can still Deny.
fn snapshot(head: &[u8], len: u64) -> ScriptSnapshot {
    let withheld = |reason: String| ScriptSnapshot {
        text: None,
        withheld: Some(reason),
    };
    if usize::try_from(len).map_or(true, |len| len > MAX_SCRIPT_TEXT_BYTES) {
        return withheld(format!(
            "It is {len} bytes, over the {} KiB Swarm keeps, so no copy was kept. Read it at its path.",
            MAX_SCRIPT_TEXT_BYTES / 1024
        ));
    }
    let Ok(text) = std::str::from_utf8(head) else {
        return withheld("It is not UTF-8 text, so no copy was kept.".to_owned());
    };
    if let Some(line) = text
        .lines()
        .position(|line| line.chars().any(swarm_persistence::hides_command_text))
    {
        return withheld(format!(
            "Line {} holds characters that can hide or reorder text, so no copy was kept: what it shows could differ from what runs.",
            line + 1
        ));
    }
    if let Some((line, kind)) = find_secret(text) {
        return withheld(format!(
            "Line {line} looks like {kind}, so no copy was kept and the value is not shown. A script reads a secret from 1Password when it runs."
        ));
    }
    ScriptSnapshot {
        text: Some(text.to_owned()),
        withheld: None,
    }
}

/// The first line, counted from 1, that looks like it holds a secret, and
/// what it looks like. A reference to one is not a secret: `op://`, `$VAR`,
/// `${VAR}` and `$(op read ...)` are how a script should name it.
fn find_secret(text: &str) -> Option<(usize, &'static str)> {
    text.lines()
        .enumerate()
        .find_map(|(index, line)| secret_in(line).map(|kind| (index + 1, kind)))
}

fn secret_in(line: &str) -> Option<&'static str> {
    if line.contains("-----BEGIN ") && line.contains("PRIVATE KEY") {
        return Some("a private key");
    }
    for (prefix, min, kind) in TOKEN_PREFIXES {
        if line
            .match_indices(prefix)
            .any(|(at, _)| token_run(line, at + prefix.len()) >= *min && starts_word(line, at))
        {
            return Some(kind);
        }
    }
    if aws_access_key(line) {
        return Some("an AWS access key");
    }
    if json_web_token(line) {
        return Some("a JSON web token");
    }
    if url_password(line) {
        return Some("a URL with a password in it");
    }
    if secret_assignment(line) {
        return Some("a secret written into the script");
    }
    None
}

/// Prefixes that begin a provider's token, the shortest run of token
/// characters after one that is taken as a token, and what to call it.
const TOKEN_PREFIXES: &[(&str, usize, &str)] = &[
    ("github_pat_", 22, "a GitHub token"),
    ("ghp_", 30, "a GitHub token"),
    ("gho_", 30, "a GitHub token"),
    ("ghu_", 30, "a GitHub token"),
    ("ghs_", 30, "a GitHub token"),
    ("ghr_", 30, "a GitHub token"),
    ("xoxb-", 10, "a Slack token"),
    ("xoxp-", 10, "a Slack token"),
    ("xoxa-", 10, "a Slack token"),
    ("xoxs-", 10, "a Slack token"),
    ("sk-ant-", 20, "an Anthropic API key"),
    ("sk-proj-", 20, "an OpenAI API key"),
    ("ops_", 40, "a 1Password service account token"),
];

fn token_char(character: char) -> bool {
    character.is_ascii_alphanumeric() || matches!(character, '_' | '-')
}

/// How many token characters follow byte `from`.
fn token_run(line: &str, from: usize) -> usize {
    line.get(from..).map_or(0, |rest| {
        rest.chars().take_while(|c| token_char(*c)).count()
    })
}

/// Whether byte `at` begins a word, so `ghp_` inside a longer name is not a token.
fn starts_word(line: &str, at: usize) -> bool {
    line.get(..at)
        .and_then(|before| before.chars().next_back())
        .is_none_or(|previous| !previous.is_ascii_alphanumeric() && previous != '_')
}

fn aws_access_key(line: &str) -> bool {
    ["AKIA", "ASIA"].iter().any(|prefix| {
        line.match_indices(prefix).any(|(at, _)| {
            starts_word(line, at)
                && line.get(at + 4..).is_some_and(|rest| {
                    rest.chars()
                        .take_while(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
                        .count()
                        == 16
                })
        })
    })
}

/// Three base64url segments, the first two opening a JSON object.
fn json_web_token(line: &str) -> bool {
    line.match_indices("eyJ").any(|(at, _)| {
        let token: String = line
            .get(at..)
            .unwrap_or_default()
            .chars()
            .take_while(|c| token_char(*c) || *c == '.')
            .collect();
        let parts: Vec<&str> = token.split('.').collect();
        parts.len() == 3
            && parts.get(1).is_some_and(|part| part.starts_with("eyJ"))
            && parts.iter().all(|part| part.len() >= 10)
    })
}

/// `scheme://user:password@host`, where the password is written out rather
/// than named by a variable.
fn url_password(line: &str) -> bool {
    line.match_indices("://").any(|(at, _)| {
        let rest = line.get(at + 3..).unwrap_or_default();
        let authority: &str = rest
            .split(|c: char| c == '/' || c.is_whitespace() || matches!(c, '"' | '\'' | '`'))
            .next()
            .unwrap_or_default();
        let Some((userinfo, _host)) = authority.rsplit_once('@') else {
            return false;
        };
        userinfo
            .split_once(':')
            .is_some_and(|(_, password)| literal(password) && password.len() >= 4)
    })
}

/// `PASSWORD=...`, `api_key: "..."` and the like, with a written-out value
/// of at least 12 characters.
fn secret_assignment(line: &str) -> bool {
    const NAMES: &[&str] = &[
        "password",
        "passwd",
        "secret",
        "token",
        "apikey",
        "api_key",
        "api-key",
        "access_key",
        "client_secret",
    ];
    let lower = line.to_ascii_lowercase();
    NAMES.iter().any(|name| {
        lower.match_indices(name).any(|(at, _)| {
            let after = lower.get(at + name.len()..).unwrap_or_default();
            let after = after.trim_start_matches(['"', '\'']).trim_start();
            let Some(value) = after.strip_prefix('=').or_else(|| after.strip_prefix(':')) else {
                return false;
            };
            let start = line.len() - value.len();
            let value = line.get(start..).unwrap_or_default().trim_start();
            let value = value.trim_start_matches(['"', '\'']);
            let value: String = value
                .chars()
                .take_while(|c| !c.is_whitespace() && !matches!(c, '"' | '\'' | ';' | ','))
                .collect();
            value.len() >= 12 && literal(&value)
        })
    })
}

/// Whether a value is written out, rather than a reference to one or a path.
fn literal(value: &str) -> bool {
    !value.is_empty()
        && !value.contains('$')
        && !value.contains('`')
        && !value.starts_with("op://")
        && !value.starts_with(['/', '~', '.', '<', '{', '%'])
        && !value
            .chars()
            .all(|c| c == value.chars().next().unwrap_or(c))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Test values are assembled at run time so this file holds no string a
    /// scanner would take for a real credential.
    fn fake(prefix: &str, body: &str, repeat: usize) -> String {
        format!("{prefix}{}", body.repeat(repeat))
    }

    #[test]
    fn each_kind_of_secret_is_found_by_line_and_never_shown() {
        let cases = [
            (
                format!("-----BEGIN OPENSSH {}", "PRIVATE KEY-----"),
                "a private key",
            ),
            (
                format!("export GH_TOKEN={}", fake("ghp_", "aB3d", 9)),
                "a GitHub token",
            ),
            (
                fake("curl -H token:github_pat_", "x1Y2", 8),
                "a GitHub token",
            ),
            (
                format!("SLACK={}", fake("xoxb-", "12ab-", 4)),
                "a Slack token",
            ),
            (
                format!("key={}", fake("sk-ant-api03-", "Zx9_", 8)),
                "an Anthropic API key",
            ),
            (
                format!("OPENAI={}", fake("sk-proj-", "Q1w2", 8)),
                "an OpenAI API key",
            ),
            (
                format!("OP_SERVICE_ACCOUNT={}", fake("ops_", "eyJh", 12)),
                "a 1Password service account token",
            ),
            (
                format!("aws_access_key_id = {}", fake("AKIA", "Q7", 8)),
                "an AWS access key",
            ),
            (
                format!(
                    "Authorization: Bearer {}.{}.{}",
                    fake("eyJ", "hbGc", 4),
                    fake("eyJ", "zdWI", 4),
                    "SflKxwRJSMeKKF2QT4fwpMeJf36P"
                ),
                "a JSON web token",
            ),
            (
                format!("psql postgres://admin:{}@db.example/app", "Tr0ub4dor-3"),
                "a URL with a password in it",
            ),
            (
                format!("PASSWORD=\"{}\"", "correct-horse-battery"),
                "a secret written into the script",
            ),
            (
                format!("  client_secret: {}", "f00dfeedcafe1234beef"),
                "a secret written into the script",
            ),
        ];
        for (line, kind) in cases {
            let text = format!("#!/bin/sh\nset -eu\n{line}\necho done\n");
            assert_eq!(find_secret(&text), Some((3, kind)), "{line}");
            let kept = snapshot(text.as_bytes(), text.len() as u64);
            assert_eq!(kept.text, None, "{line}");
            let reason = kept.withheld.unwrap();
            assert!(
                reason.starts_with(&format!("Line 3 looks like {kind}")),
                "{reason}"
            );
            let value = line
                .split(['=', ' ', ':', '@'])
                .max_by_key(|part| part.len())
                .unwrap();
            assert!(
                !reason.contains(value),
                "the value is never shown: {reason}"
            );
        }
    }

    #[test]
    fn a_reference_to_a_secret_is_not_one() {
        for line in [
            "TOKEN=\"$(op read 'op://BFG/Release/credential')\"",
            "export GH_TOKEN=${GITHUB_TOKEN}",
            "PASSWORD=$PGPASSWORD psql -h db",
            "api_key: op://WS Dev Secrets/v6-platform/api-key",
            "TOKEN_FILE=/run/user/1000/swarm-signing.key",
            "SECRET_PATH=/home/ops/.secrets/key",
            "token=<redacted>",
            "password=xxxxxxxxxxxxxxxx",
            "psql postgres://admin@db.example/app",
            "curl https://$USER:$PASS@host/x",
            "git clone https://github.com/miopea/swarm-next.git",
            "echo 'ghp_ is the prefix GitHub uses'",
            "my_ghp_counter=3",
            "gh api repos/x/y --jq '.sha'",
            "KEY=/run/user/$(id -u)/swarm-signing.key",
        ] {
            assert_eq!(secret_in(line), None, "{line}");
        }
    }

    #[test]
    fn a_script_that_fits_is_kept_whole_and_one_that_does_not_says_why() {
        let script = "#!/bin/sh\nset -eu\ncargo test --workspace\n";
        assert_eq!(
            snapshot(script.as_bytes(), script.len() as u64),
            ScriptSnapshot {
                text: Some(script.to_owned()),
                withheld: None
            }
        );
        let at_cap = "#".repeat(MAX_SCRIPT_TEXT_BYTES);
        assert_eq!(
            snapshot(at_cap.as_bytes(), at_cap.len() as u64)
                .text
                .as_deref(),
            Some(at_cap.as_str())
        );
        let over = MAX_SCRIPT_TEXT_BYTES as u64 + 1;
        let reason = snapshot(b"", over).withheld.unwrap();
        assert!(
            reason.contains(&format!("{over} bytes, over the 64 KiB")),
            "{reason}"
        );
        let binary = [0x7f, b'E', b'L', b'F', 0xff, 0xfe];
        assert_eq!(
            snapshot(&binary, binary.len() as u64).withheld.as_deref(),
            Some("It is not UTF-8 text, so no copy was kept.")
        );
        let hidden = "echo ok\necho \u{202e}gnp.exe\n";
        let reason = snapshot(hidden.as_bytes(), hidden.len() as u64)
            .withheld
            .unwrap();
        assert!(
            reason.starts_with("Line 2 holds characters that can hide"),
            "{reason}"
        );
    }

    #[test]
    fn one_read_pins_the_hash_and_the_text() {
        let directory = std::env::temp_dir().join(format!("swarm-script-{}", uuid::Uuid::now_v7()));
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("release.sh");
        let body = "#!/bin/sh\necho release\n";
        std::fs::write(&path, body).unwrap();
        let path = path.to_str().unwrap();
        let filed = read_script(path).unwrap();
        assert_eq!(filed.script.path, path);
        assert_eq!(
            filed.script.sha256,
            format!("{:x}", Sha256::digest(body.as_bytes()))
        );
        assert_eq!(Some(filed.script.sha256.clone()), script_sha256(path));
        assert_eq!(filed.snapshot.text.as_deref(), Some(body));

        let big = "#".repeat(MAX_SCRIPT_TEXT_BYTES * 3 + 7);
        std::fs::write(path, &big).unwrap();
        let filed = read_script(path).unwrap();
        assert_eq!(
            filed.script.sha256,
            format!("{:x}", Sha256::digest(big.as_bytes())),
            "a script too long to keep is still hashed whole"
        );
        assert!(filed.snapshot.withheld.is_some());

        assert_eq!(read_script("relative/release.sh"), None);
        assert_eq!(
            read_script(directory.to_str().unwrap()),
            None,
            "a folder is not a script"
        );
        std::fs::remove_dir_all(&directory).unwrap();
        assert_eq!(read_script(path), None, "a file that is gone");
    }
}
