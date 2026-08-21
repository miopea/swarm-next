//! Learning that a release exists, fetching it, and asking for it to be
//! installed. ADR 0050.
//!
//! Three things here are deliberate and load-bearing.
//!
//! Nothing is sent. The check is a plain GET of one static document: no
//! version, no Hive identity, no counts. The origin learns what any static
//! file host learns and nothing more, so an operator who turns this on is not
//! also turning on telemetry.
//!
//! Nothing is trusted before it is verified. The manifest's signature is
//! checked against the compiled key before any URL inside it is fetched, and
//! the artifact's digest is checked before anything is unpacked. An artifact
//! that fails either is discarded and reported, never installed and rolled
//! back — rollback is a worse position than never having installed.
//!
//! Nothing is installed by this process. Installing restarts `swarm-api`, so
//! the call would be killed mid-command and the result reported to nobody.
//! The API writes a request file and a systemd path unit does the work, which
//! is the mechanism the worker engine update and the development reload
//! already use.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use axum::Json;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use swarm_domain::{
    ReleaseOffer, SignedReleaseManifest, SwarmVersion, compiled_release_verifying_key,
    verify_release_manifest,
};
use tokio::io::AsyncWriteExt;

use crate::auth::authorize;
use crate::{ApiError, AppState, build_version, worker_engine_build_id};

/// Where releases are announced unless the operator points elsewhere.
const DEFAULT_MANIFEST_URL: &str =
    "https://raw.githubusercontent.com/miopea/swarm-next/main/releases.json";

/// A manifest is a small document. Anything larger is not one, and reading it
/// would be reading whatever the origin decided to send.
const MAX_MANIFEST_BYTES: u64 = 256 * 1024;

/// A ceiling on the artifact, so a hostile or broken origin cannot fill the
/// disk. Generous next to a real bundle.
const MAX_ARTIFACT_BYTES: u64 = 512 * 1024 * 1024;

const FETCH_TIMEOUT: Duration = Duration::from_secs(30);
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(600);

/// What the control room needs to describe the update situation.
#[derive(Debug, Serialize)]
pub(super) struct ReleaseStatusResponse {
    /// Whether this build can check at all: it needs a compiled verifying key
    /// and an origin. Without either the whole path is inert rather than
    /// failing repeatedly against something it was never meant to contact.
    available: bool,
    /// `unset` until the operator answers, which is not the same as `off`.
    mode: String,
    current_version: String,
    /// A working copy is told about releases and offered none of them.
    development_build: bool,
    last_checked_at: Option<i64>,
    last_outcome: Option<String>,
    /// The newest release this Hive could run, upgrade or not.
    offer: Option<ReleaseOffer>,
    /// Whether `offer` is something this Hive could actually install.
    upgrade_available: bool,
    /// Whether the release carries a different worker engine.
    ///
    /// This is NOT "installing stops your workers", which is what it used to
    /// say and is the opposite of what the product does. `swarm-package update`
    /// restarts the API and preserves the running terminal host, so workers
    /// stay online through the install. The engine is swapped later, when
    /// sessions are idle or the operator asks — and that is the part that
    /// restarts anything. ADR 0050 point 5 wants the consequence stated at the
    /// moment of consent; the consequence is a deferred engine update, not an
    /// immediate stop.
    carries_new_worker_engine: bool,
    /// The verified, unpacked release waiting to be installed, if any.
    downloaded_version: Option<String>,
    /// Why the install unit refused, when it did: `outside-downloads`,
    /// `missing`, or `not-a-release`.
    ///
    /// It writes one and nothing read it, so a refusal reached the operator as
    /// a bare "did not run" with the explanation sitting in a file.
    apply_reason: Option<String>,
    /// What the install unit last reported: `installing`, `installed`,
    /// `failed`, or `refused`.
    ///
    /// Without this the control room cannot tell "restarting, hold on" from
    /// "nothing happened", and both look like a card that goes back to
    /// offering the release it just accepted.
    apply_state: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(super) struct ReleaseModeRequest {
    mode: String,
}

/// Where a downloaded release is unpacked, and where the request naming it
/// goes. Absent on a Hive with no state directory configured.
fn download_root(state: &AppState) -> Option<PathBuf> {
    Some(state.release_state_root.as_ref()?.join("downloads"))
}

fn manifest_url(state: &AppState) -> Option<String> {
    if compiled_release_verifying_key().is_none() {
        return None;
    }
    state
        .release_manifest_url
        .as_ref()
        .map(|url| url.as_ref().clone())
        .or_else(|| Some(DEFAULT_MANIFEST_URL.to_owned()))
}

/// The current status, without contacting anything.
pub(super) async fn status(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    Ok((
        [(header::CACHE_CONTROL, "no-store")],
        Json(build_status(&state).await?),
    )
        .into_response())
}

/// Chooses whether this Hive contacts an origin at all.
pub(super) async fn set_mode(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(request): Json<ReleaseModeRequest>,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    crate::task_store(&state)?
        .set_release_check_mode(&request.mode)
        .map_err(|error| crate::task_store_error(&error))?;
    Ok((
        [(header::CACHE_CONTROL, "no-store")],
        Json(build_status(&state).await?),
    )
        .into_response())
}

/// Checks now, at the operator's request.
///
/// A poll you cannot force is one you do not trust, so this exists even
/// though the daily check would get there eventually.
pub(super) async fn check_now(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    check(&state).await;
    Ok((
        [(header::CACHE_CONTROL, "no-store")],
        Json(build_status(&state).await?),
    )
        .into_response())
}

/// Fetches and verifies the offered artifact, leaving it unpacked and ready.
///
/// Separate from installing on purpose: downloading is reversible and
/// installing is not, so they are two consents rather than one button that
/// does both.
pub(super) async fn download(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let status = build_status(&state).await?;
    if !status.upgrade_available {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            "no_release_offered",
            "there is no release to download",
        ));
    }
    let offer = status.offer.ok_or_else(|| {
        ApiError::new(
            StatusCode::CONFLICT,
            "no_release_offered",
            "there is no release to download",
        )
    })?;
    let root = download_root(&state).ok_or_else(|| {
        ApiError::new(
            StatusCode::CONFLICT,
            "release_downloads_unavailable",
            "this Hive has nowhere to put a release",
        )
    })?;
    fetch_artifact(&offer, &root).await.map_err(|reason| {
        ApiError::new(
            StatusCode::BAD_GATEWAY,
            "release_not_fetched",
            format!("release not fetched: {reason}"),
        )
    })?;
    Ok((
        [(header::CACHE_CONTROL, "no-store")],
        Json(build_status(&state).await?),
    )
        .into_response())
}

/// Asks for the downloaded release to be installed.
///
/// Writes a request naming the verified directory. `swarm-package`
/// re-verifies that bundle's own `SHA256SUMS` before installing it, so the
/// integrity check does not depend on this having been honest.
pub(super) async fn apply(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let status = build_status(&state).await?;
    let request_path = state.release_apply_request_path.as_ref().ok_or_else(|| {
        ApiError::new(
            StatusCode::CONFLICT,
            "release_apply_unavailable",
            "this Hive cannot install a release itself",
        )
    })?;
    let root = download_root(&state).ok_or_else(|| {
        ApiError::new(
            StatusCode::CONFLICT,
            "no_release_downloaded",
            "no release has been downloaded",
        )
    })?;
    let expected = status
        .offer
        .as_ref()
        .map(|offer| offer.artifact_sha256.clone());
    let release = downloaded_release(&root, expected.as_deref()).ok_or_else(|| {
        ApiError::new(
            StatusCode::CONFLICT,
            "no_release_downloaded",
            "no release has been downloaded",
        )
    })?;
    tokio::fs::write(request_path.as_ref(), release.to_string_lossy().as_bytes())
        .await
        .map_err(|_| {
            ApiError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                "release_request_unwritable",
                "the install request could not be written",
            )
        })?;
    Ok(StatusCode::ACCEPTED.into_response())
}

async fn build_status(state: &Arc<AppState>) -> Result<ReleaseStatusResponse, ApiError> {
    let stored = crate::task_store(state)?
        .release_check_state()
        .map_err(|error| crate::task_store_error(&error))?;
    let current = SwarmVersion::parse(build_version());
    let development_build = current.as_ref().is_some_and(SwarmVersion::is_development);
    let offer: Option<ReleaseOffer> = stored
        .last_offer
        .as_deref()
        .and_then(|encoded| serde_json::from_str(encoded).ok());
    let upgrade_available = match (&current, &offer) {
        (Some(current), Some(offer)) => offer
            .parsed_version()
            .is_some_and(|offered| offered.supersedes(current)),
        _ => false,
    };
    Ok(ReleaseStatusResponse {
        available: manifest_url(state).is_some(),
        mode: stored.mode,
        current_version: build_version().to_owned(),
        development_build,
        last_checked_at: stored.last_checked_at,
        last_outcome: stored.last_outcome,
        carries_new_worker_engine: offer
            .as_ref()
            .is_some_and(|offer| offer.worker_engine_build_id != worker_engine_build_id()),
        upgrade_available,
        apply_state: apply_state(state, offer.as_ref().map(|offer| offer.version.as_str())),
        apply_reason: apply_field(
            state,
            offer.as_ref().map(|offer| offer.version.as_str()),
            "reason=",
        ),
        downloaded_version: download_root(state)
            .as_deref()
            .and_then(|root| {
                downloaded_release(
                    root,
                    offer.as_ref().map(|offer| offer.artifact_sha256.as_str()),
                )
            })
            .and_then(|path| {
                path.file_name()
                    .map(|name| name.to_string_lossy().into_owned())
            }),
        offer,
    })
}

/// How stale the last check has to be before the daily poll repeats it.
const CHECK_INTERVAL_SECONDS: i64 = 24 * 60 * 60;

/// Runs a check if the operator asked for one and the last is a day old.
///
/// Due-time is anchored to the last check rather than to a fixed schedule, so
/// a machine that was asleep overnight checks when it wakes rather than at a
/// moment every machine shares.
pub(super) async fn poll(state: &Arc<AppState>) {
    let Ok(store) = crate::task_store(state) else {
        return;
    };
    let Ok(stored) = store.release_check_state() else {
        return;
    };
    if stored.mode != "daily" {
        return;
    }
    let now = chrono::Utc::now().timestamp();
    if stored
        .last_checked_at
        .is_some_and(|last| now.saturating_sub(last) < CHECK_INTERVAL_SECONDS)
    {
        return;
    }
    check(state).await;
}

/// One check: fetch, verify, compare, record. Never throws away what was
/// previously known.
pub(super) async fn check(state: &Arc<AppState>) {
    let Some(url) = manifest_url(state) else {
        return;
    };
    let now = chrono::Utc::now().timestamp();
    let (outcome, offer) = match fetch_manifest(&url, now).await {
        Err(outcome) => (outcome, None),
        Ok(manifest) => {
            let protocol = swarm_terminal::PROTOCOL_VERSION.to_string();
            match manifest.newest_offer(&protocol) {
                None => ("current", None),
                Some(offer) => {
                    let current = SwarmVersion::parse(build_version());
                    let supersedes = current.as_ref().is_some_and(|current| {
                        offer
                            .parsed_version()
                            .is_some_and(|offered| offered.supersedes(current))
                    });
                    let encoded = serde_json::to_string(offer).ok();
                    (if supersedes { "offered" } else { "current" }, encoded)
                }
            }
        }
    };
    if let Ok(store) = crate::task_store(state) {
        let _ = store.record_release_check(outcome, offer.as_deref(), now);
    }
}

async fn fetch_manifest(
    url: &str,
    now: i64,
) -> Result<swarm_domain::ReleaseManifest, &'static str> {
    let client = reqwest::Client::builder()
        .timeout(FETCH_TIMEOUT)
        .https_only(true)
        .build()
        .map_err(|_| "unreachable")?;
    let response = client.get(url).send().await.map_err(|_| "unreachable")?;
    if !response.status().is_success() {
        return Err("unreachable");
    }
    if response
        .content_length()
        .is_some_and(|length| length > MAX_MANIFEST_BYTES)
    {
        return Err("rejected");
    }
    let body = response.text().await.map_err(|_| "unreachable")?;
    if body.len() as u64 > MAX_MANIFEST_BYTES {
        return Err("rejected");
    }
    let document: SignedReleaseManifest = serde_json::from_str(&body).map_err(|_| "rejected")?;
    verify_release_manifest(&document, compiled_release_verifying_key(), now)
        .map(Clone::clone)
        .map_err(|_| "rejected")
}

/// What the install unit last wrote about itself.
fn apply_state(state: &AppState, offered: Option<&str>) -> Option<String> {
    apply_field(state, offered, "state=")
}

/// One field of the install unit's status, when the status is about the
/// release in hand.
fn apply_field(state: &AppState, offered: Option<&str>, field: &str) -> Option<String> {
    let path = state
        .release_state_root
        .as_ref()?
        .join("release-apply.status");
    let contents = std::fs::read_to_string(path).ok()?;
    apply_field_from(&contents, offered, field)
}

/// The rule, separated so it can be tested against this function rather than
/// against a copy of it.
///
/// A status file outlives the attempt that wrote it, so a result has to name
/// the release it concerns or it cannot be told apart from a current one. One
/// with no version at all was written before stamping existed and is ignored
/// rather than guessed at.
fn apply_state_from(contents: &str, offered: Option<&str>) -> Option<String> {
    apply_field_from(contents, offered, "state=")
}

fn apply_field_from(contents: &str, offered: Option<&str>, wanted: &str) -> Option<String> {
    let field = |name: &str| {
        contents
            .lines()
            .find_map(|line| line.strip_prefix(name).map(str::to_owned))
    };
    let version = field("version=")?;
    (version == offered?).then(|| field(wanted))?
}

/// Records which signed artifact a download came from.
///
/// A release can be replaced at the same version — it happened, minutes after
/// 0.5.0 was published — and an already-unpacked copy of the old one is still
/// a structurally valid bundle. `swarm-package` re-verifies the bundle's own
/// SHA256SUMS, which a stale build satisfies perfectly well, so nothing
/// downstream can tell the two apart. The digest the manifest signed is what
/// distinguishes them, and it has to be written down at download time because
/// the artifact is deleted once unpacked.
const DOWNLOAD_DIGEST_FILE: &str = ".swarm-release-digest";

/// The unpacked release under the download root, if it is the one currently
/// offered.
///
/// A download that does not match the offer in hand is treated as absent, so
/// the operator is asked to fetch again rather than being handed a stale build
/// under a version number that now means something else.
fn downloaded_release(root: &Path, expected_digest: Option<&str>) -> Option<PathBuf> {
    let mut entries = std::fs::read_dir(root).ok()?.flatten();
    entries.find_map(|entry| {
        let path = entry.path();
        if !path.join("swarm-package").is_file() {
            return None;
        }
        let recorded = std::fs::read_to_string(path.join(DOWNLOAD_DIGEST_FILE)).ok()?;
        (recorded.trim() == expected_digest?).then_some(path)
    })
}

/// Fetches the artifact, checks its digest against the signed manifest, and
/// only then unpacks it.
async fn fetch_artifact(offer: &ReleaseOffer, root: &Path) -> Result<PathBuf, String> {
    if offer.artifact_bytes > MAX_ARTIFACT_BYTES {
        return Err("the artifact is larger than this product ships".to_owned());
    }
    tokio::fs::create_dir_all(root)
        .await
        .map_err(|error| error.to_string())?;
    let staging = root.join(format!(".incoming-{}", offer.version));
    let _ = tokio::fs::remove_dir_all(&staging).await;
    tokio::fs::create_dir_all(&staging)
        .await
        .map_err(|error| error.to_string())?;

    let client = reqwest::Client::builder()
        .timeout(DOWNLOAD_TIMEOUT)
        .https_only(true)
        .build()
        .map_err(|error| error.to_string())?;
    let mut response = client
        .get(&offer.artifact_url)
        .send()
        .await
        .map_err(|_| "the artifact could not be fetched".to_owned())?;
    if !response.status().is_success() {
        return Err("the artifact could not be fetched".to_owned());
    }

    let archive = staging.join("artifact.tar.gz");
    let mut file = tokio::fs::File::create(&archive)
        .await
        .map_err(|error| error.to_string())?;
    let mut digest = Sha256::new();
    let mut written: u64 = 0;
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| "the artifact could not be fetched".to_owned())?
    {
        written += chunk.len() as u64;
        if written > offer.artifact_bytes {
            let _ = tokio::fs::remove_dir_all(&staging).await;
            return Err("the artifact is not the size the manifest signed for".to_owned());
        }
        digest.update(&chunk);
        file.write_all(&chunk)
            .await
            .map_err(|error| error.to_string())?;
    }
    file.flush().await.map_err(|error| error.to_string())?;
    drop(file);

    if !artifact_matches(offer, written, &hex(&digest.finalize())) {
        // Discarded rather than kept: an artifact that fails this check is not
        // a candidate for anything, and keeping it invites installing it.
        let _ = tokio::fs::remove_dir_all(&staging).await;
        return Err("the artifact does not match the signed digest".to_owned());
    }

    let unpacked = unpack(
        &archive,
        &staging,
        root,
        &offer.version,
        &offer.artifact_sha256,
    )
    .await;
    let _ = tokio::fs::remove_dir_all(&staging).await;
    unpacked
}

async fn unpack(
    archive: &Path,
    staging: &Path,
    root: &Path,
    version: &str,
    digest: &str,
) -> Result<PathBuf, String> {
    let opened = staging.join("opened");
    tokio::fs::create_dir_all(&opened)
        .await
        .map_err(|error| error.to_string())?;
    let status = tokio::process::Command::new("tar")
        .arg("-xzf")
        .arg(archive)
        .arg("-C")
        .arg(&opened)
        .status()
        .await
        .map_err(|error| error.to_string())?;
    if !status.success() {
        return Err("the artifact could not be unpacked".to_owned());
    }
    let bundle = std::fs::read_dir(&opened)
        .map_err(|error| error.to_string())?
        .flatten()
        .map(|entry| entry.path())
        .find(|path| path.join("swarm-package").is_file())
        .ok_or_else(|| "the artifact is not a Swarm release".to_owned())?;

    let destination = root.join(version);
    let _ = tokio::fs::remove_dir_all(&destination).await;
    tokio::fs::rename(&bundle, &destination)
        .await
        .map_err(|error| error.to_string())?;
    tokio::fs::write(destination.join(DOWNLOAD_DIGEST_FILE), digest)
        .await
        .map_err(|error| error.to_string())?;
    Ok(destination)
}

/// Whether the bytes that arrived are the bytes the manifest signed for.
///
/// Both halves matter. The digest is what proves the content, and the length
/// is what stops a stream being read further than the signature covers.
fn artifact_matches(offer: &ReleaseOffer, written: u64, digest: &str) -> bool {
    written == offer.artifact_bytes && digest == offer.artifact_sha256
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().fold(String::new(), |mut rendered, byte| {
        use std::fmt::Write;
        let _ = write!(rendered, "{byte:02x}");
        rendered
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn offer() -> ReleaseOffer {
        ReleaseOffer {
            version: "0.2.0".to_owned(),
            protocol: "9".to_owned(),
            artifact_url: "https://releases.example/swarm-0.2.0.tar.gz".to_owned(),
            artifact_sha256: "a".repeat(64),
            artifact_bytes: 16_213_012,
            worker_engine_build_id: "engine-b".to_owned(),
            notes_url: None,
        }
    }

    /// The artifact is what actually runs on the machine. A digest that only
    /// nearly matches is a different program.
    #[test]
    fn an_artifact_is_taken_only_when_both_its_digest_and_length_match() {
        let offer = offer();
        assert!(artifact_matches(&offer, 16_213_012, &"a".repeat(64)));

        // A different program of exactly the right length.
        assert!(!artifact_matches(&offer, 16_213_012, &"b".repeat(64)));
        // The right program, truncated — the digest of what arrived cannot
        // match, but the length check catches it without hashing anything.
        assert!(!artifact_matches(&offer, 16_213_011, &"a".repeat(64)));
        // Padded past what the signature covers.
        assert!(!artifact_matches(&offer, 16_213_013, &"a".repeat(64)));
        // An empty response that hashes to something.
        assert!(!artifact_matches(&offer, 0, &"a".repeat(64)));
    }

    #[test]
    fn hex_renders_a_digest_the_way_a_manifest_writes_one() {
        assert_eq!(hex(&[0x00, 0x0f, 0xa0, 0xff]), "000fa0ff");
    }
}

#[cfg(test)]
mod apply_status_tests {
    use super::apply_state_from;

    /// This test previously carried its own copy of the rule and passed while
    /// the real function was untouched — the version comparison never reached
    /// `apply_state`, shipped in 0.5.0 and 0.6.0, and the operator saw a
    /// failure from an earlier release reported against a later one. It calls
    /// the function now.
    #[test]
    fn only_a_status_about_the_release_in_hand_is_reported() {
        let failed_on_020 = "state=failed\nversion=0.2.0\n";
        assert_eq!(
            apply_state_from(failed_on_020, Some("0.2.0")).as_deref(),
            Some("failed")
        );
        // The defect: this used to report "failed" while offering 0.6.0.
        assert_eq!(apply_state_from(failed_on_020, Some("0.6.0")), None);
        assert_eq!(apply_state_from(failed_on_020, None), None);
        // Written before statuses were stamped; not guessed at.
        assert_eq!(apply_state_from("state=failed\n", Some("0.2.0")), None);
        // A current result still reads normally.
        assert_eq!(
            apply_state_from("state=installing\nversion=0.6.0\n", Some("0.6.0")).as_deref(),
            Some("installing")
        );
    }
}
