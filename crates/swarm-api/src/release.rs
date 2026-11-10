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
    AutoInstallHold, ReleaseNotes, ReleaseOffer, SignedReleaseManifest, SwarmVersion,
    compiled_release_verifying_key, verify_release_manifest,
};
use swarm_persistence::TaskStore;
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
#[allow(
    clippy::struct_excessive_bools,
    reason = "a response shape: each flag is an independent fact the control room renders"
)]
pub(super) struct ReleaseStatusResponse {
    /// Whether this build can check at all: it needs a compiled verifying key
    /// and an origin. Without either the whole path is inert rather than
    /// failing repeatedly against something it was never meant to contact.
    available: bool,
    /// `unset` until the operator answers, which is not the same as `off`.
    mode: String,
    /// Whether an ordinary release installs itself while the operator is away
    /// (ADR 0110) — this Hive's own choice, unless its Apiary requires one.
    auto_install: bool,
    /// Whether this Hive's Apiary requires `auto_install` (ADR 0113), in which
    /// case it is locked here and only the Keeper can change it.
    auto_install_required_by_apiary: bool,
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
    /// Whether installing this release ends every worker session.
    ///
    /// TRI-STATE ON PURPOSE. `Some(true)` swaps the API and the engine together
    /// and stops every session. `Some(false)` does not. `None` means this Hive
    /// could not tell — the host did not answer, or the offer carries no
    /// readable protocol. Before the download it is the manifest's protocol
    /// against the engine's; after it, the release's own answer, so a newer
    /// protocol whose API still drives the engine reads `Some(false)`
    /// (ADR 0167). The name predates that.
    ///
    /// A bool would have to fold "cannot tell" into one of the two answers, and
    /// folding it into `false` renders the unknown case as the reassuring one.
    /// That is the shape that put "your workers keep running" in front of an
    /// operator whose workers were about to stop.
    carries_protocol_change: Option<bool>,
    /// How an offer that swaps the engine gets swapped (ADR 0169):
    /// `waits_for_rest`, by itself once every worker rests; `waits_for_press`,
    /// by the operator's press; `attempted`, by nothing, because a swap of it
    /// stopped workers and did not finish. `None` for an offer that does not
    /// swap the engine, or when this Hive cannot tell.
    engine_swap: Option<AutomaticSwap>,
    /// How many commits this working copy is ahead of the released tag.
    ///
    /// The version strings alone say nothing: a working copy and the release it
    /// came from both read 0.7.0, so the card could not answer the only
    /// question being asked of it — is there enough here to cut a release.
    ///
    /// `None` when it cannot be counted rather than 0, because "the tag is not
    /// in this checkout" and "nothing has changed" are different answers.
    commits_ahead_of_release: Option<u32>,
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
    /// Which step the install failed at, in the operator's vocabulary:
    /// `accept`, `update`, or `migrate-protocol`.
    apply_step: Option<String>,
    /// What the failing bundle itself said, one bounded line.
    ///
    /// The install used to record a state and a version and nothing else, so
    /// the card said "The install did not run" and sent the reader to
    /// journalctl for a sentence the installer had already written down.
    apply_detail: Option<String>,
    /// What the failed install left behind: `nothing`, `partial`, `unknown`.
    ///
    /// READ BACK FROM THE INSTALLED VERSION, not concluded from the exit
    /// status. The card asserted "Nothing was changed" on every failure
    /// without ever looking, which is the same defect as a reload claiming a
    /// build did not compile when it compiled.
    apply_changed: Option<String>,
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
    compiled_release_verifying_key()?;
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
        Json(current_status(&state).await?),
    )
        .into_response())
}

/// The status, with the facts only the running engine can give.
async fn current_status(state: &Arc<AppState>) -> Result<ReleaseStatusResponse, ApiError> {
    // Asked of the RUNNING host rather than a symlink or this build's own
    // constant: the host is a separate process that deliberately outlives an
    // API restart, so it can be a release behind, and a symlink comparison is
    // how a staleness check once reported current while the host was two
    // releases back.
    let protocol_change = offered_protocol_change(state).await;
    let mut status = build_status(state, protocol_change)?;
    status.engine_swap = offered_engine_swap(state, &status).await;
    Ok(status)
}

/// How the offer's engine swap happens, when it swaps the engine (ADR 0169).
///
/// Before an automatic install has prepared it, the pass that would: this
/// Hive installs releases by itself, has not already tried this one, and its
/// engine's idle update would take the swap.
async fn offered_engine_swap(
    state: &Arc<AppState>,
    status: &ReleaseStatusResponse,
) -> Option<AutomaticSwap> {
    if status.carries_protocol_change != Some(true) || !status.upgrade_available {
        return None;
    }
    let store = crate::task_store(state).ok()?;
    let engine = crate::maintenance::host_status_snapshot(state).await.ok();
    let offered = status.offer.as_ref().map(|offer| offer.version.as_str());
    if let Some(swap) = automatic_swap(state, store, offered, engine.as_ref()) {
        return Some(swap);
    }
    let unattempted = status
        .apply_state
        .as_deref()
        .is_none_or(|outcome| outcome == "installed");
    let applies_itself = engine
        .as_ref()
        .is_some_and(|engine| crate::idle_engine_update::applies_itself(state, engine));
    Some(if status.auto_install && unattempted && applies_itself {
        AutomaticSwap::WaitsForRest
    } else {
        AutomaticSwap::WaitsForPress
    })
}

/// Whether installing the offer ends worker sessions, or `None` when this Hive
/// cannot say. Before the download that is the manifest's protocol against the
/// engine's; after it, the release's own answer.
///
/// UNKNOWN IS NOT "NO". Both halves have to be known: an offer with no readable
/// protocol, and a host that does not answer, each leave the question open. The
/// card says so rather than guessing, because guessing wrong in the reassuring
/// direction costs somebody their session.
///
/// Deliberately NOT `install_swaps_engine`, which answers a different question
/// for a different caller: that one decides whether to write down every
/// worker, so unknown must mean "do not", and folding unknown into `false` is
/// right there and wrong here. Same input, opposite cost of error.
async fn offered_protocol_change(state: &Arc<AppState>) -> Option<bool> {
    let offer = build_status(state, None).ok()?.offer?;
    let host = crate::maintenance::host_status_snapshot(state).await.ok()?;
    // ONCE DOWNLOADED, THE RELEASE SAYS ITSELF (ADR 0167). A protocol change
    // whose API still drives this engine installs the app alone, so its workers
    // keep running, and the manifest's number cannot say so.
    if let Some(release) = download_root(state)
        .and_then(|root| downloaded_release(&root, Some(&offer.artifact_sha256)))
    {
        return release_swaps_engine(&release, Some(host.protocol_version));
    }
    let offered = offer.protocol.trim().parse::<u16>().ok()?;
    Some(host.protocol_version != offered)
}

/// Chooses whether this Hive contacts an origin at all.
pub(super) async fn set_mode(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(request): Json<ReleaseModeRequest>,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    if request.mode == "off" && required_auto_install(&state) == Some(true) {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            "set_by_apiary",
            "your Apiary requires automatic install, which needs checking for releases; only its Keeper can change that",
        ));
    }
    crate::task_store(&state)?
        .set_release_check_mode(&request.mode)
        .map_err(|error| crate::task_store_error(&error))?;
    Ok((
        [(header::CACHE_CONTROL, "no-store")],
        Json(build_status(&state, None)?),
    )
        .into_response())
}

/// Checks now, at the operator's request.
///
/// A poll you cannot force is one you do not trust, so this exists even
/// though the daily check would get there eventually.
#[derive(serde::Deserialize)]
pub(super) struct AutoInstallRequest {
    enabled: bool,
}

pub(super) async fn set_auto_install(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(request): Json<AutoInstallRequest>,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    // ⚠️ AN APIARY REQUIREMENT IS NOT THIS HIVE'S TO CHANGE (ADR 0113). Refused
    // with words that say who can, rather than accepted and silently ignored.
    if required_auto_install(&state).is_some_and(|required| required != request.enabled) {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            "set_by_apiary",
            "your Apiary requires this setting; only its Keeper can change it",
        ));
    }
    crate::task_store(&state)?
        .set_release_auto_install(request.enabled)
        .map_err(|error| crate::task_store_error(&error))?;
    Ok((
        [(header::CACHE_CONTROL, "no-store")],
        Json(build_status(&state, None)?),
    )
        .into_response())
}

pub(super) async fn check_now(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    check(&state).await;
    Ok((
        [(header::CACHE_CONTROL, "no-store")],
        Json(build_status(&state, None)?),
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
    let status = build_status(&state, None)?;
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
        Json(build_status(&state, None)?),
    )
        .into_response())
}

/// Whether installing this downloaded release swaps the running engine, read
/// from the release itself (ADR 0167).
///
/// The engine's protocol is asked of the HOST rather than a symlink, because a
/// symlink comparison is how a staleness check reported current while the
/// running host was two releases behind.
///
/// UNKNOWN IS "NO SWAP" HERE, deliberately. This decides whether to write down
/// every running worker for a return; getting it wrong in the other direction
/// costs a needless roster wipe on an ordinary update.
async fn install_swaps_engine(state: &AppState, release: &Path) -> bool {
    let engine = crate::maintenance::host_status_snapshot(state)
        .await
        .ok()
        .map(|host| host.protocol_version);
    release_swaps_engine(release, engine) == Some(true)
}

/// Write down every worker that is loaded now, so the supervisor brings them
/// back after the install has stopped everything.
async fn record_revival_intents(state: &AppState) -> Result<(), ApiError> {
    let swarm_terminal::HostResponse::Sessions { sessions } =
        crate::terminal_host::request_host(state, swarm_terminal::HostRequest::ListSessions)
            .await?
    else {
        return Err(ApiError::new(
            StatusCode::BAD_GATEWAY,
            "unexpected_host_response",
            "terminal host returned an unexpected session response",
        ));
    };
    let running = sessions
        .into_iter()
        .filter(|session| session.running)
        .map(|session| session.session_id)
        .collect::<std::collections::HashSet<_>>();
    let store = crate::task_store(state)?;
    let loaded = crate::maintenance::loaded_workers(
        &store
            .list_worker_profiles()
            .map_err(|error| crate::task_store_error(&error))?,
        &running,
    );
    store
        .record_worker_revival_intents(&loaded, crate::unix_timestamp())
        .map_err(|error| crate::task_store_error(&error))
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
    request_install(&state, InstallMode::Install).await?;
    Ok(StatusCode::ACCEPTED.into_response())
}

/// What the install unit is asked to do with the downloaded release.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum InstallMode {
    /// Install it now. A release that swaps the engine swaps it now, ending
    /// every worker session: the operator's press, and an ordinary release.
    Install,
    /// Prepare a release that swaps the engine, which stops nobody. The swap
    /// waits for every worker to rest (ADR 0169).
    PrepareMigration,
}

/// Asks the install unit to install the downloaded release, or to prepare it
/// (`mode`).
///
/// The operator's Install button and the automatic install (ADR 0110) both come
/// through here, so every guard — one install at a time, workers owed a return
/// recorded first — holds for both.
async fn request_install(state: &Arc<AppState>, mode: InstallMode) -> Result<(), ApiError> {
    let status = build_status(state, None)?;
    let request_path = state.release_apply_request_path.as_ref().ok_or_else(|| {
        ApiError::new(
            StatusCode::CONFLICT,
            "release_apply_unavailable",
            "this Hive cannot install a release itself",
        )
    })?;
    let root = download_root(state).ok_or_else(|| {
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
    // ⚠️ ONE INSTALL AT A TIME, AND THE SECOND PRESS IS REFUSED RATHER THAN
    // QUEUED. Nothing stopped Install being pressed again while an install was
    // pending or running, and a second attempt of the same release collides with
    // the first on the package lifecycle lock, fails, and writes ITS failure over
    // the status of the attempt that actually did the work. The operator was
    // then told "The install failed ... Nothing was changed and this Hive is
    // still on 1.13.0" about an install that succeeded — reported twice on
    // 2026-09-22 as "it said nothing changed ... I restarted the service and it
    // was updated".
    if install_in_flight(
        state,
        status.offer.as_ref().map(|offer| offer.version.as_str()),
        request_path,
    ) {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            "release_install_in_progress",
            "an install of this release is already underway; it finishes on its own without another press",
        ));
    }
    // A PROTOCOL CHANGE ENDS EVERY WORKER SESSION, so the workers owed a return
    // are written down BEFORE the install is asked for.
    //
    // The installer stops swarm.target and swaps the API and the host together;
    // it has no way to remember who was loaded, and on restart the supervisor
    // only revives workers set to start automatically. Without this, taking a
    // protocol release leaves every other worker asleep and the operator wakes
    // them one at a time — which is what happened on 2026-08-28.
    //
    // Recorded under the worker lifecycle lock, the same one maintain_worker_engine
    // holds, so a worker cannot start between the roster being read and the
    // intents being written. Recorded BEFORE the request rather than after,
    // because the install can begin the moment the file appears.
    //
    // A PREPARATION STOPS NOBODY, so it owes nobody a return. The swap that
    // follows records its own, for exactly the sessions it stops (ADR 0169).
    if mode == InstallMode::Install && install_swaps_engine(state, &release).await {
        let guard = state.worker_lifecycle.lock().await;
        let recorded = record_revival_intents(state).await;
        drop(guard);
        if let Err(error) = recorded {
            // Not fatal: an operator who accepted a protocol release should get
            // it. Losing the roster is worse than a wake-one-at-a-time morning,
            // so this is reported and the install proceeds.
            tracing::warn!(
                message = %error.message,
                "workers owed a return could not be recorded before a protocol install"
            );
        }
    }
    // The release on the first line, as every swarm-package reads it. An older
    // package reads the second line as part of the path and refuses the
    // request: refusing is the safe way for it to misread a preparation.
    let mut request = release.to_string_lossy().into_owned();
    if mode == InstallMode::PrepareMigration {
        request.push_str("\nmode=prepare-migration\n");
    }
    tokio::fs::write(request_path.as_ref(), request.as_bytes())
        .await
        .map_err(|_| {
            ApiError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                "release_request_unwritable",
                "the install request could not be written",
            )
        })?;
    Ok(())
}

/// What this Hive's Apiary requires of automatic install, if anything.
///
/// A Hive that is not a member, or cannot read its policy, has no requirement:
/// its operator's own choice stands.
/// Whether this Hive installs releases by itself: its operator's choice,
/// unless its Apiary requires one (ADR 0113). `None` when it cannot be read.
pub(crate) fn auto_install_enabled(state: &AppState) -> Option<bool> {
    let stored = crate::task_store(state).ok()?.release_check_state().ok()?;
    Some(required_auto_install(state).unwrap_or(stored.auto_install))
}

fn required_auto_install(state: &AppState) -> Option<bool> {
    crate::apiary_service(state)
        .ok()?
        .required_switch(swarm_domain::RequiredSetting::ReleaseAutoInstall)
        .ok()
        .flatten()
}

fn build_status(
    state: &Arc<AppState>,
    protocol_change: Option<bool>,
) -> Result<ReleaseStatusResponse, ApiError> {
    let stored = crate::task_store(state)?
        .release_check_state()
        .map_err(|error| crate::task_store_error(&error))?;
    let required_auto_install = required_auto_install(state);
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
        mode: effective_mode(&stored.mode, required_auto_install).to_owned(),
        auto_install: required_auto_install.unwrap_or(stored.auto_install),
        auto_install_required_by_apiary: required_auto_install.is_some(),
        current_version: build_version().to_owned(),
        development_build,
        last_checked_at: stored.last_checked_at,
        last_outcome: stored.last_outcome,
        carries_protocol_change: protocol_change,
        engine_swap: None,
        carries_new_worker_engine: offer
            .as_ref()
            .is_some_and(|offer| offer.worker_engine_build_id != worker_engine_build_id()),
        upgrade_available,
        commits_ahead_of_release: commits_ahead_of_release(
            state,
            offer.as_ref().map(|offer| offer.version.as_str()),
        ),
        apply_state: apply_state(state, offer.as_ref().map(|offer| offer.version.as_str())),
        apply_reason: apply_field(
            state,
            offer.as_ref().map(|offer| offer.version.as_str()),
            "reason=",
        ),
        apply_step: apply_field(
            state,
            offer.as_ref().map(|offer| offer.version.as_str()),
            "step=",
        ),
        apply_detail: apply_field(
            state,
            offer.as_ref().map(|offer| offer.version.as_str()),
            "detail=",
        ),
        apply_changed: apply_field(
            state,
            offer.as_ref().map(|offer| offer.version.as_str()),
            "changed=",
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
/// Four hours, not a day.
///
/// A day was chosen when releases were rare. They are not: 1.7.1 and 1.8.0 went
/// out within a day of each other, so a daily check could tell an operator they
/// were current while being two releases behind. The manifest is a few hundred
/// bytes from a CDN, so the cost of asking is nil; the cost of not asking is
/// somebody running a build with a fix they already paid for.
///
/// Not shorter than this on purpose. The published manifest takes up to about
/// twenty minutes to reach every edge, so checking every few minutes would
/// mostly re-read a cached answer.
const CHECK_INTERVAL_SECONDS: i64 = 4 * 60 * 60;

/// How soon after a restart a Hive may check again.
///
/// The startup pass exists so a Hive restarted just after a release notices it
/// now rather than hours later. This floor is what stops a restart LOOP from
/// turning that into a request per boot.
const STARTUP_RECHECK_FLOOR_SECONDS: i64 = 15 * 60;

/// Runs a check if the operator asked for one and the last is a day old.
///
/// Due-time is anchored to the last check rather than to a fixed schedule, so
/// a machine that was asleep overnight checks when it wakes rather than at a
/// moment every machine shares.
pub(super) async fn poll(state: &Arc<AppState>, startup: bool) {
    let Ok(store) = crate::task_store(state) else {
        return;
    };
    let Ok(stored) = store.release_check_state() else {
        return;
    };
    if check_is_due(
        effective_mode(&stored.mode, required_auto_install(state)),
        stored.last_checked_at,
        chrono::Utc::now().timestamp(),
        startup,
    ) {
        check(state).await;
    }
    auto_install(state).await;
}

impl AppState {
    /// Where this Hive's last automatic install pass stopped, for its health
    /// report (ADR 0167). `None` before the first pass and when nothing held
    /// the release.
    pub(crate) fn auto_install_hold(&self) -> Option<AutoInstallHold> {
        self.auto_install_hold.lock().ok().and_then(|hold| *hold)
    }

    pub(crate) fn record_auto_install_hold(&self, hold: Option<AutoInstallHold>) {
        if let Ok(mut recorded) = self.auto_install_hold.lock() {
            *recorded = hold;
        }
    }
}

/// What the automatic install is decided on, before the release is read.
///
/// Independent yes/no facts read from different places, not a state encoded in
/// flags, so the bools are each the honest type.
#[allow(clippy::struct_excessive_bools)]
#[derive(Clone, Copy, Debug, Default)]
struct AutoInstallFacts<'a> {
    enabled: bool,
    development_build: bool,
    upgrade_available: bool,
    presence: Option<swarm_domain::PresenceMode>,
    /// Every session is at rest by the engine's own count: none mid-turn and
    /// none it cannot read (ADR 0156). An engine that cannot answer is not
    /// idle.
    workers_idle: bool,
    in_flight: bool,
    last_outcome: Option<&'a str>,
    /// Where a swap of the offered release stands, when one has begun.
    swap: Option<AutomaticSwap>,
}

/// Where a swap of the offered release stands (ADR 0169).
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
enum AutomaticSwap {
    /// It swaps by itself once every worker rests: an automatic install has
    /// prepared it, or would.
    WaitsForRest,
    /// Nothing here would swap it by itself: the operator's press does.
    WaitsForPress,
    /// A swap of it stopped workers and did not finish, or was not heard from
    /// again. Nothing tries it again by itself.
    Attempted,
}

/// The rule, pure so each hold can be tested without a network or a clock.
///
/// Whether the release swaps the engine is decided after this, from the
/// downloaded release itself (`swaps_engine`).
fn auto_install_hold(facts: &AutoInstallFacts<'_>) -> Option<AutoInstallHold> {
    let AutoInstallFacts {
        enabled,
        development_build,
        upgrade_available,
        presence,
        workers_idle,
        in_flight,
        last_outcome,
        swap,
    } = *facts;
    if !enabled {
        return Some(AutoInstallHold::TurnedOff);
    }
    if development_build {
        return Some(AutoInstallHold::DevelopmentBuild);
    }
    if !upgrade_available {
        return Some(AutoInstallHold::NothingNewer);
    }
    // A swap under way is the reason, whatever the workers are doing now.
    match swap {
        Some(AutomaticSwap::Attempted) => return Some(AutoInstallHold::SwapAttempted),
        Some(AutomaticSwap::WaitsForRest) => return Some(AutoInstallHold::SwapWaitsForRest),
        Some(AutomaticSwap::WaitsForPress) => return Some(AutoInstallHold::ProtocolChange),
        None => {}
    }
    // AWAY, OR EVERY WORKER IDLE (ADR 0167). An install that keeps the engine
    // restarts only the API: workers keep running, and an open page reconnects
    // once. The operator ruled that idle workers are reason enough, as they
    // are for the engine, which is the more disruptive update. Unreadable
    // presence is treated as present, and an engine that cannot count its
    // workers as busy.
    let away = presence.is_some_and(|mode| mode != swarm_domain::PresenceMode::AtHive);
    if !away && !workers_idle {
        return Some(AutoInstallHold::InUse);
    }
    if in_flight {
        return Some(AutoInstallHold::InFlight);
    }
    // ⚠️ ONE ATTEMPT PER RELEASE. Without this, a release that cannot install
    // is retried on every pass for as long as the operator is away.
    if last_outcome.is_some_and(|outcome| outcome != "installed") {
        return Some(AutoInstallHold::AlreadyAttempted);
    }
    None
}

/// Whether installing a release swaps the running engine too, which ends every
/// worker session. `None` when that cannot be told.
///
/// ⚠️ READ FROM THE RELEASE, THE WAY ITS OWN `swarm-package update` READS IT
/// (ADR 0157): the same protocol, or an API that still drives the running
/// engine (`OLDEST_ENGINE_PROTOCOL` up to its `PROTOCOL`), installs the app
/// alone. The manifest's protocol number cannot say that. Trusting it held
/// every 1.18 release on Hives whose engine was on 17, although each would
/// have installed with every worker left running (ADR 0167).
///
/// A release without `OLDEST_ENGINE_PROTOCOL` predates app-first installs, and
/// one whose value is not a number cannot be trusted to drive anything. Both
/// swap the engine, as the installer does with them.
fn swaps_engine(
    release_protocol: Option<&str>,
    oldest_engine: Option<&str>,
    engine_protocol: Option<u16>,
) -> Option<bool> {
    let engine = engine_protocol?;
    let release = release_protocol?.trim().parse::<u16>().ok()?;
    if release == engine {
        return Some(false);
    }
    let drives = oldest_engine
        .and_then(|value| value.trim().parse::<u16>().ok())
        .is_some_and(|oldest| oldest <= engine && engine <= release);
    Some(!drives)
}

/// `swaps_engine` for a downloaded release directory.
fn release_swaps_engine(release: &Path, engine_protocol: Option<u16>) -> Option<bool> {
    let read = |name: &str| std::fs::read_to_string(release.join(name)).ok();
    swaps_engine(
        read("PROTOCOL").as_deref(),
        read("OLDEST_ENGINE_PROTOCOL").as_deref(),
        engine_protocol,
    )
}

/// Downloads and installs an ordinary release while the operator is away or
/// every worker is idle, and records where the pass stopped for this Hive's
/// health report (ADR 0167).
async fn auto_install(state: &Arc<AppState>) {
    // A Hive that cannot read its own store or status keeps its last answer
    // rather than inventing one.
    let Ok(store) = crate::task_store(state) else {
        return;
    };
    // The status carries the EFFECTIVE choice: this Hive's own, unless its
    // Apiary requires one (ADR 0113).
    let Ok(status) = build_status(state, None) else {
        return;
    };
    let hold = auto_install_pass(state, store, status).await;
    state.record_auto_install_hold(hold);
}

/// One automatic install pass. Returns where it stopped, or `None` when nothing
/// held the release: it was downloaded for the next pass, or its install was
/// asked for.
async fn auto_install_pass(
    state: &Arc<AppState>,
    store: &TaskStore,
    status: ReleaseStatusResponse,
) -> Option<AutoInstallHold> {
    let offered = status.offer.as_ref().map(|offer| offer.version.as_str());
    let in_flight = state
        .release_apply_request_path
        .as_ref()
        .is_some_and(|path| install_in_flight(state, offered, path));
    let presence = store
        .operator_presence(chrono::Utc::now().timestamp())
        .ok()
        .map(|presence| presence.mode);
    // One reading of the engine answers both questions asked of it here: are
    // its workers idle, and which protocol does it speak.
    let engine = crate::maintenance::host_status_snapshot(state).await.ok();
    let workers_idle = engine.as_ref().is_some_and(|engine| {
        engine.busy_sessions == Some(0) && engine.unreadable_sessions == Some(0)
    });
    if let Some(hold) = auto_install_hold(&AutoInstallFacts {
        enabled: status.auto_install,
        development_build: status.development_build,
        upgrade_available: status.upgrade_available,
        presence,
        workers_idle,
        in_flight,
        last_outcome: status.apply_state.as_deref(),
        swap: automatic_swap(state, store, offered, engine.as_ref()),
    }) {
        tracing::debug!(?hold, "an ordinary release is not installing itself yet");
        return Some(hold);
    }
    let Some(offer) = status.offer else {
        return Some(AutoInstallHold::NothingNewer);
    };
    let Some(root) = download_root(state) else {
        return Some(AutoInstallHold::InstallRefused);
    };
    // Downloaded before the protocol is judged, because only the release can
    // say whether it swaps the engine. A release that does is kept for the
    // operator's Install button.
    //
    // THE INSTALL WAITS FOR THE NEXT PASS AFTER A DOWNLOAD. A download can take
    // minutes, and "every worker is idle" was read before it began.
    if status.downloaded_version.as_deref() != Some(offer.version.as_str()) {
        return match fetch_artifact(&offer, &root).await {
            Ok(_) => {
                tracing::info!(version = %offer.version, "downloaded a release to install on the next pass");
                None
            }
            Err(reason) => {
                tracing::warn!(%reason, version = %offer.version, "an automatic install could not download its release");
                Some(AutoInstallHold::DownloadFailed)
            }
        };
    }
    let Some(release) = downloaded_release(&root, Some(&offer.artifact_sha256)) else {
        return Some(AutoInstallHold::DownloadFailed);
    };
    let mode = match release_swaps_engine(
        &release,
        engine.as_ref().map(|engine| engine.protocol_version),
    ) {
        Some(false) => InstallMode::Install,
        // ADR 0169: where the engine can stop only resting workers and would
        // swap by itself, the release is prepared now, stopping nobody, and
        // swapped once every worker rests.
        Some(true)
            if engine
                .as_ref()
                .is_some_and(|engine| crate::idle_engine_update::applies_itself(state, engine)) =>
        {
            InstallMode::PrepareMigration
        }
        swaps => {
            let hold = if swaps.is_some() {
                AutoInstallHold::ProtocolChange
            } else {
                AutoInstallHold::ProtocolUnknown
            };
            tracing::debug!(?hold, version = %offer.version, "an ordinary release is not installing itself yet");
            return Some(hold);
        }
    };
    match request_install(state, mode).await {
        Ok(()) if mode == InstallMode::PrepareMigration => {
            tracing::info!(
                version = %offer.version,
                "preparing a release that swaps the worker engine; it installs itself once every worker rests"
            );
            None
        }
        Ok(()) => {
            tracing::info!(
                version = %offer.version,
                "installing an ordinary release automatically while the operator is away or every worker is idle"
            );
            None
        }
        Err(error) => {
            tracing::warn!(
                message = %error.message,
                version = %offer.version,
                "an automatic install could not be requested"
            );
            Some(AutoInstallHold::InstallRefused)
        }
    }
}

/// Where a swap of the offered release stands, if one has begun (ADR 0169).
///
/// An attempt at it that stopped workers and did not finish comes first,
/// whoever started it: nothing tries it again by itself. Then a migration an
/// automatic install prepared for it, which swaps by itself only where the
/// engine's idle update would take it.
fn automatic_swap(
    state: &AppState,
    store: &TaskStore,
    offered: Option<&str>,
    engine: Option<&swarm_terminal::TerminalHostStatus>,
) -> Option<AutomaticSwap> {
    let offered = offered?;
    let unfinished = store
        .last_worker_engine_update()
        .ok()
        .flatten()
        .is_some_and(|last| {
            last.to_version == offered
                && last.to_protocol.is_some()
                && last.stopped_sessions.is_some()
                && last.outcome != Some(swarm_persistence::WorkerEngineUpdateOutcome::Succeeded)
        });
    if unfinished {
        return Some(AutomaticSwap::Attempted);
    }
    let (_, prepared) = crate::maintenance::automatic_protocol_target(state)
        .ok()
        .flatten()?;
    if prepared != offered {
        return None;
    }
    Some(
        if engine.is_some_and(|engine| crate::idle_engine_update::applies_itself(state, engine)) {
            AutomaticSwap::WaitsForRest
        } else {
            AutomaticSwap::WaitsForPress
        },
    )
}

/// The check mode in effect.
///
/// ⚠️ REQUIRING AUTOMATIC INSTALL REQUIRES CHECKING. A Hive that never asks for
/// releases has nothing to install, so "Stop checking" would quietly undo an
/// Apiary's requirement while every screen said it was in force (ADR 0113).
fn effective_mode(stored: &str, required_auto_install: Option<bool>) -> &str {
    if required_auto_install == Some(true) && stored == "off" {
        "daily"
    } else {
        stored
    }
}

/// Whether to ask the manifest anything right now. Pure so the policy is
/// testable without a network or a clock.
///
/// ⚠️ UNSET MEANS YES. It used to mean no, which made "I have not chosen"
/// indistinguishable from "do not tell me" -- a freshly built Hive never
/// checked, and sat on whatever release it was installed with until somebody
/// found the setting. Only an explicit `off` is silence.
fn check_is_due(mode: &str, last_checked_at: Option<i64>, now: i64, startup: bool) -> bool {
    if mode == "off" {
        return false;
    }
    let floor = if startup {
        STARTUP_RECHECK_FLOOR_SECONDS
    } else {
        CHECK_INTERVAL_SECONDS
    };
    // A stamp in the FUTURE reads as due, not as just-checked. i64 saturating_sub
    // saturates at i64::MIN, not at zero, so a clock that moved backwards -- or a
    // database copied from a machine ahead of this one -- would otherwise produce
    // a negative age that never reaches the floor and wedges checking forever.
    last_checked_at.is_none_or(|last| last > now || now.saturating_sub(last) >= floor)
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
            match manifest.newest_offer() {
                // `current` IS NOW TRUE HERE, which it was not before.
                //
                // This branch used to be reached whenever the newest release
                // declared a different protocol, so "you are up to date" was
                // said to Hives that were several releases behind and could
                // have installed every one of them.
                //
                // I tried to add an `incompatible` outcome for "the manifest
                // offered releases and none could be read" and then removed it:
                // verify_release_manifest refuses a manifest containing an
                // unparseable version outright, so a verified manifest cannot
                // reach here with releases listed. The branch could never have
                // fired, and a branch that cannot fire is what this whole
                // session has been deleting.
                None => ("current", None),
                Some(offer) => {
                    // ⚠️ RECORDED EVEN WHEN THIS HIVE IS ALREADY CURRENT. The
                    // Apiary needs to know which release is newest in order to
                    // judge its MEMBERS, and a Keeper that happens to be up to
                    // date is exactly the Keeper best placed to say so. Writing
                    // this only in the `offered` branch would leave a healthy
                    // Keeper unable to see anyone else fall behind.
                    if let Ok(store) = crate::task_store(state) {
                        let _ = store.note_expected_release(&offer.version, now);
                    }
                    // A new release appearing is the other moment the fleet's
                    // standing can change, so it is judged here too.
                    if let Ok(apiary) = crate::apiary_service(state) {
                        let _ = apiary.raise_hives_left_behind(now, build_version());
                    }
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
        .cloned()
        .map_err(|_| "rejected")
}

/// How far this working copy has moved past the released tag.
///
/// Counted against the tag rather than a recorded revision, because the tag is
/// what the release was cut from and it is already in the checkout that cut it.
/// A Hive that is not building from a working copy has nothing to count.
///
/// `None` when it cannot be counted rather than 0: "the tag is not in this
/// checkout" and "nothing has changed since it" are different answers, and
/// showing the second when the first is true would be a lie about whether
/// there is anything to release.
fn commits_ahead_of_release(state: &AppState, released: Option<&str>) -> Option<u32> {
    let checkout = state.development_checkout_path.as_ref()?;
    let released = released?;
    let range = format!("v{released}..HEAD");
    crate::runtime::git_output(checkout, &["rev-list", "--count", &range])?
        .parse()
        .ok()
}

/// How long an `installing` status is believed without being refreshed.
///
/// An install can wait up to five minutes for the package lifecycle lock and
/// then take its own time; past this, a status still saying `installing` is an
/// attempt that died without writing its ending, and trusting it would refuse
/// Install forever — a stuck button instead of a stuck install.
const INSTALL_IN_FLIGHT_WINDOW: std::time::Duration = std::time::Duration::from_secs(15 * 60);

/// Whether an install of the offered release is already queued or running.
fn install_in_flight(state: &AppState, offered: Option<&str>, request_path: &Path) -> bool {
    if request_path.exists() {
        return true;
    }
    let Some(status_path) = state
        .release_state_root
        .as_ref()
        .map(|root| root.join("release-apply.status"))
    else {
        return false;
    };
    let recent = std::fs::metadata(&status_path)
        .and_then(|metadata| metadata.modified())
        .ok()
        .and_then(|modified| modified.elapsed().ok())
        .is_some_and(|age| age < INSTALL_IN_FLIGHT_WINDOW);
    recent && apply_state(state, offered).as_deref() == Some("installing")
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
        .map_err(|error| format!("{} could not be reached ({error})", offer.artifact_url))?;
    // The status is named. A signed manifest can point at a URL that 404s, and
    // collapsing that into "could not be fetched" meant the operator saw a bare
    // 502 with nothing to act on — the manifest was wrong, not the network.
    if !response.status().is_success() {
        return Err(format!(
            "{} answered {}",
            offer.artifact_url,
            response.status().as_u16()
        ));
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

    use swarm_domain::PresenceMode::{AtHive, NightWatch, Reachable};
    use swarm_persistence::{WorkerEngineUpdateInitiator, WorkerEngineUpdateOutcome};

    /// Positional, so each case reads as one line of the rule's inputs.
    #[allow(clippy::fn_params_excessive_bools)]
    fn hold(
        enabled: bool,
        development_build: bool,
        upgrade_available: bool,
        presence: Option<swarm_domain::PresenceMode>,
        workers_idle: bool,
        in_flight: bool,
        last_outcome: Option<&str>,
    ) -> Option<AutoInstallHold> {
        auto_install_hold(&AutoInstallFacts {
            enabled,
            development_build,
            upgrade_available,
            presence,
            workers_idle,
            in_flight,
            last_outcome,
            swap: None,
        })
    }

    /// What the fake engine reports and what the downloaded release says
    /// about itself.
    struct Fixture {
        presence: swarm_domain::PresenceMode,
        /// The protocol the RUNNING engine speaks.
        engine_protocol: u16,
        /// Sessions the engine counts as mid-turn.
        busy: usize,
        /// The downloaded bundle's `PROTOCOL`, which the manifest repeats.
        release_protocol: u16,
        /// The downloaded bundle's `OLDEST_ENGINE_PROTOCOL`, absent from a
        /// bundle built before ADR 0157.
        oldest_engine: Option<u16>,
        /// Whether swarm-package replaces this Hive's engine, as every
        /// installed Hive's does.
        managed: bool,
        /// A worker whose session the engine lists as running.
        loaded_worker: bool,
    }

    impl Fixture {
        /// A release on this Hive's own protocol, nobody busy.
        fn ordinary(presence: swarm_domain::PresenceMode) -> Self {
            Self {
                presence,
                engine_protocol: swarm_terminal::PROTOCOL_VERSION,
                busy: 0,
                release_protocol: swarm_terminal::PROTOCOL_VERSION,
                oldest_engine: None,
                managed: true,
                loaded_worker: false,
            }
        }

        /// A release whose API cannot drive the running engine, which can
        /// stop its own resting workers (ADR 0156), with a worker loaded.
        fn swapping(presence: swarm_domain::PresenceMode) -> Self {
            Self {
                release_protocol: swarm_terminal::PROTOCOL_VERSION + 1,
                oldest_engine: Some(swarm_terminal::PROTOCOL_VERSION + 1),
                loaded_worker: true,
                ..Self::ordinary(presence)
            }
        }
    }

    /// An engine that answers its status with `protocol` and `busy`, and lists
    /// `running` as its running sessions. One request per connection, as the
    /// client sends them.
    fn fake_engine(
        socket: &Path,
        protocol: u16,
        busy: usize,
        running: Vec<swarm_domain::WorkerSessionId>,
    ) -> tokio::task::JoinHandle<()> {
        use swarm_terminal::{HostRequest, HostResponse, TerminalHostStatus};
        use tokio::io::{AsyncBufReadExt, BufReader};
        let listener = tokio::net::UnixListener::bind(socket).unwrap();
        tokio::spawn(async move {
            loop {
                let Ok((stream, _)) = listener.accept().await else {
                    return;
                };
                let mut reader = BufReader::new(stream);
                let mut line = String::new();
                if reader.read_line(&mut line).await.unwrap_or(0) == 0 {
                    continue;
                }
                let response = match serde_json::from_str::<HostRequest>(&line) {
                    Ok(HostRequest::HostStatus) => HostResponse::HostStatus {
                        status: TerminalHostStatus {
                            protocol_version: protocol,
                            host_version: "1.0.0-fixture".to_owned(),
                            host_build_id: None,
                            draining: false,
                            running_sessions: busy,
                            retained_sessions: busy,
                            busy_sessions: Some(busy),
                            unreadable_sessions: Some(0),
                            resources: None,
                            takeover_relay: false,
                        },
                    },
                    Ok(HostRequest::ListSessions) => HostResponse::Sessions {
                        sessions: running
                            .iter()
                            .map(|&session_id| swarm_terminal::HostSessionSummary {
                                session_id,
                                running: true,
                                stop_pending_release: false,
                                resources: None,
                                last_output_at: None,
                                recovery_attempt: None,
                                provider_start: None,
                                provider_selection: None,
                                continuation_unavailable: false,
                                continuation_recovery: None,
                                provider_executable: None,
                                provider_tools_relay: None,
                            })
                            .collect(),
                    },
                    _ => HostResponse::Error {
                        code: "fixture_unsupported".to_owned(),
                        message: "the fixture engine answers status and sessions only".to_owned(),
                    },
                };
                let mut bytes = serde_json::to_vec(&response).unwrap();
                bytes.push(b'\n');
                let _ = reader.get_mut().write_all(&bytes).await;
            }
        })
    }

    /// A Hive whose store holds a verified offer newer than this build, with the
    /// release already downloaded, and an engine as `fixture` describes. Returns
    /// the state, the request path, and the engine task.
    fn hive_with_a_downloaded_release(
        directory: &tempfile::TempDir,
        fixture: &Fixture,
    ) -> (
        Arc<AppState>,
        std::path::PathBuf,
        tokio::task::JoinHandle<()>,
    ) {
        let socket = directory.path().join("terminal.sock");
        let store = swarm_persistence::TaskStore::in_memory().unwrap();
        let mut running = Vec::new();
        if fixture.loaded_worker {
            let worker = store
                .create_worker(
                    "Ledger",
                    swarm_domain::ProviderKind::ClaudeCode,
                    "/workspace/fictional",
                    false,
                    1,
                )
                .unwrap();
            let session = swarm_domain::WorkerSessionId::new();
            store.bind_worker_session(worker.id, session).unwrap();
            running.push(session);
        }
        let host = fake_engine(&socket, fixture.engine_protocol, fixture.busy, running);
        let offer = swarm_domain::ReleaseOffer {
            version: "99.0.0".to_owned(),
            protocol: fixture.release_protocol.to_string(),
            artifact_url: "https://releases.invalid/swarm-99.0.0.tar.gz".to_owned(),
            artifact_sha256: "b".repeat(64),
            artifact_bytes: 4096,
            worker_engine_build_id: worker_engine_build_id().to_owned(),
            notes_url: None,
        };
        store
            .record_release_check("offered", Some(&serde_json::to_string(&offer).unwrap()), 1)
            .unwrap();
        store
            .set_manual_presence(Some(fixture.presence), 1)
            .unwrap();
        let downloaded = directory.path().join("downloads").join("99.0.0");
        std::fs::create_dir_all(&downloaded).unwrap();
        std::fs::write(downloaded.join("swarm-package"), "#!/bin/sh\n").unwrap();
        std::fs::write(
            downloaded.join(DOWNLOAD_DIGEST_FILE),
            &offer.artifact_sha256,
        )
        .unwrap();
        std::fs::write(
            downloaded.join("PROTOCOL"),
            format!("{}\n", fixture.release_protocol),
        )
        .unwrap();
        if let Some(oldest) = fixture.oldest_engine {
            std::fs::write(
                downloaded.join("OLDEST_ENGINE_PROTOCOL"),
                format!("{oldest}\n"),
            )
            .unwrap();
        }
        let request = directory.path().join("release-apply.request");
        let mut state = AppState::default()
            .with_terminal_host(swarm_terminal::HostClient::new(&socket), "secret")
            .with_task_store(store)
            .with_release_paths(directory.path().to_owned(), request.clone());
        if fixture.managed {
            state = state.with_maintenance_request_path(
                directory.path().join("worker-engine-maintenance.request"),
            );
        }
        (Arc::new(state), request, host)
    }

    /// ⚠️ THE PASS ITSELF, NOT ONLY ITS RULE: with the operator away it asks the
    /// install unit for the downloaded release, exactly as the button does,
    /// even while a worker is mid-turn, because the app install stops nobody.
    #[tokio::test]
    async fn an_ordinary_release_is_requested_for_install_while_the_operator_is_away() {
        let directory = tempfile::tempdir().unwrap();
        let fixture = Fixture {
            busy: 1,
            ..Fixture::ordinary(swarm_domain::PresenceMode::Reachable)
        };
        let (state, request, host) = hive_with_a_downloaded_release(&directory, &fixture);
        auto_install(&state).await;
        let requested = std::fs::read_to_string(&request).expect("an install was requested");
        assert!(requested.ends_with("downloads/99.0.0"), "{requested}");
        host.abort();
    }

    /// The operator's ruling of 2026-11-10: an app install restarts only the
    /// API, so it need not wait for the operator to leave. Waiting for every
    /// worker to be idle is enough, as it is for the engine.
    #[tokio::test]
    async fn an_ordinary_release_is_requested_while_the_operator_is_at_the_hive_and_every_worker_is_idle()
     {
        let directory = tempfile::tempdir().unwrap();
        let fixture = Fixture::ordinary(swarm_domain::PresenceMode::AtHive);
        let (state, request, host) = hive_with_a_downloaded_release(&directory, &fixture);
        auto_install(&state).await;
        let requested = std::fs::read_to_string(&request)
            .expect("the operator is at the Hive, but no worker is mid-turn");
        assert!(requested.ends_with("downloads/99.0.0"), "{requested}");
        host.abort();
    }

    #[tokio::test]
    async fn nothing_is_requested_while_the_operator_is_at_the_hive_and_a_worker_is_busy() {
        let directory = tempfile::tempdir().unwrap();
        let fixture = Fixture {
            busy: 1,
            ..Fixture::ordinary(swarm_domain::PresenceMode::AtHive)
        };
        let (state, request, host) = hive_with_a_downloaded_release(&directory, &fixture);
        auto_install(&state).await;
        assert!(
            !request.exists(),
            "restarting the API under a busy worker breaks its Swarm calls"
        );
        host.abort();
    }

    /// ⚠️ THE BUG. Every 1.18 release is protocol 19 and still drives an engine
    /// on 17 (ADR 0157), so its `update` installs the app alone and no worker
    /// stops. The manifest's number said "protocol change", and every member
    /// on 1.17.10 waited for a press that nobody was asked for.
    #[tokio::test]
    async fn a_release_on_a_newer_protocol_is_requested_when_it_drives_the_running_engine() {
        let directory = tempfile::tempdir().unwrap();
        let fixture = Fixture {
            engine_protocol: 17,
            release_protocol: 19,
            oldest_engine: Some(17),
            ..Fixture::ordinary(swarm_domain::PresenceMode::Reachable)
        };
        let (state, request, host) = hive_with_a_downloaded_release(&directory, &fixture);
        auto_install(&state).await;
        let requested = std::fs::read_to_string(&request)
            .expect("the release installs the app alone, so it is ordinary");
        assert!(requested.ends_with("downloads/99.0.0"), "{requested}");
        host.abort();
    }

    /// A release whose API cannot drive the running engine swaps both and ends
    /// every session. An engine on 17 cannot be asked to stop only resting
    /// workers, so there it stays the operator's call (ADR 0169).
    #[tokio::test]
    async fn a_release_that_cannot_drive_the_running_engine_waits_for_the_operator() {
        for oldest_engine in [Some(18), None] {
            let directory = tempfile::tempdir().unwrap();
            let fixture = Fixture {
                engine_protocol: 17,
                release_protocol: 19,
                oldest_engine,
                ..Fixture::ordinary(swarm_domain::PresenceMode::Reachable)
            };
            let (state, request, host) = hive_with_a_downloaded_release(&directory, &fixture);
            auto_install(&state).await;
            assert!(!request.exists(), "{oldest_engine:?}");
            assert_eq!(
                state.auto_install_hold(),
                Some(AutoInstallHold::ProtocolChange),
                "the member reports that its operator's press is what it waits for"
            );
            host.abort();
        }
    }

    /// A member reports where this pass stopped, so its Keeper can say why it
    /// is behind (ADR 0167). ⚠️ A LATER PASS REPLACES IT: a hold that has
    /// cleared must not go on explaining a Hive that has moved on.
    #[tokio::test]
    async fn the_pass_records_where_it_stopped_and_a_later_pass_replaces_it() {
        let directory = tempfile::tempdir().unwrap();
        let fixture = Fixture {
            busy: 1,
            ..Fixture::ordinary(AtHive)
        };
        let (state, request, host) = hive_with_a_downloaded_release(&directory, &fixture);
        assert_eq!(state.auto_install_hold(), None, "nothing before a pass");
        auto_install(&state).await;
        assert_eq!(state.auto_install_hold(), Some(AutoInstallHold::InUse));
        crate::task_store(&state)
            .unwrap()
            .set_manual_presence(Some(Reachable), 2)
            .unwrap();
        auto_install(&state).await;
        assert!(request.exists(), "the operator left, so it installs");
        assert_eq!(state.auto_install_hold(), None);
        host.abort();
    }

    /// A download that fails leaves the Hive behind with every rule satisfied,
    /// which is exactly the case nothing explained before.
    #[tokio::test]
    async fn a_release_that_cannot_be_downloaded_is_reported_as_held_by_the_download() {
        let directory = tempfile::tempdir().unwrap();
        let fixture = Fixture::ordinary(Reachable);
        let (state, request, host) = hive_with_a_downloaded_release(&directory, &fixture);
        std::fs::remove_dir_all(directory.path().join("downloads").join("99.0.0")).unwrap();
        // Refused before any network: larger than this product ships.
        let offer = swarm_domain::ReleaseOffer {
            version: "99.0.0".to_owned(),
            protocol: fixture.release_protocol.to_string(),
            artifact_url: "https://releases.invalid/swarm-99.0.0.tar.gz".to_owned(),
            artifact_sha256: "b".repeat(64),
            artifact_bytes: MAX_ARTIFACT_BYTES + 1,
            worker_engine_build_id: worker_engine_build_id().to_owned(),
            notes_url: None,
        };
        crate::task_store(&state)
            .unwrap()
            .record_release_check("offered", Some(&serde_json::to_string(&offer).unwrap()), 2)
            .unwrap();
        auto_install(&state).await;
        assert!(!request.exists());
        assert_eq!(
            state.auto_install_hold(),
            Some(AutoInstallHold::DownloadFailed)
        );
        host.abort();
    }

    /// ADR 0169: a release that swaps the engine is no longer kept for the
    /// operator's press where the engine can stop its own resting workers. The
    /// install unit is asked to PREPARE it, which stops nobody and owes nobody
    /// a return; the swap waits for every worker to rest.
    #[tokio::test]
    async fn an_engine_swapping_release_is_prepared_where_the_engine_can_stop_resting_workers() {
        let directory = tempfile::tempdir().unwrap();
        let fixture = Fixture::swapping(Reachable);
        let (state, request, host) = hive_with_a_downloaded_release(&directory, &fixture);
        auto_install(&state).await;
        let requested = std::fs::read_to_string(&request).expect("its preparation was asked for");
        let mut lines = requested.lines();
        assert!(
            lines
                .next()
                .is_some_and(|line| line.ends_with("downloads/99.0.0")),
            "{requested}"
        );
        assert_eq!(lines.next(), Some("mode=prepare-migration"), "{requested}");
        assert_eq!(lines.next(), None, "{requested}");
        assert_eq!(state.auto_install_hold(), None);
        assert!(
            crate::task_store(&state)
                .unwrap()
                .worker_revival_intents()
                .unwrap()
                .is_empty(),
            "preparing stops nobody, so nobody is owed a return yet"
        );
        host.abort();
    }

    /// The press installs it outright, as before, and writes the loaded
    /// workers down first. Only the automatic install prepares.
    #[tokio::test]
    async fn the_operators_press_still_swaps_the_engine_outright() {
        let directory = tempfile::tempdir().unwrap();
        let fixture = Fixture::swapping(AtHive);
        let (state, request, host) = hive_with_a_downloaded_release(&directory, &fixture);
        request_install(&state, InstallMode::Install).await.unwrap();
        let requested = std::fs::read_to_string(&request).unwrap();
        assert!(requested.ends_with("downloads/99.0.0"), "{requested}");
        assert!(!requested.contains("mode="), "{requested}");
        assert_eq!(
            crate::task_store(&state)
                .unwrap()
                .worker_revival_intents()
                .unwrap()
                .len(),
            1,
            "the loaded worker is owed a return"
        );
        host.abort();
    }

    /// Wherever the swap could not then apply by itself, the release waits for
    /// the press as it did: automatic engine updates off, an engine nothing
    /// replaces, or a migration its operator prepared.
    #[tokio::test]
    async fn an_engine_swapping_release_waits_for_the_press_where_nothing_would_swap_it_by_itself()
    {
        for case in ["engine switch off", "unmanaged", "operator prepared"] {
            let directory = tempfile::tempdir().unwrap();
            let fixture = Fixture {
                managed: case != "unmanaged",
                ..Fixture::swapping(Reachable)
            };
            let (state, request, host) = hive_with_a_downloaded_release(&directory, &fixture);
            match case {
                "engine switch off" => {
                    crate::task_store(&state)
                        .unwrap()
                        .set_worker_engine_auto_update(false, 1)
                        .unwrap();
                }
                "operator prepared" => {
                    prepared_migration(directory.path(), "98.0.0", "manual");
                }
                _ => {}
            }
            auto_install(&state).await;
            assert!(!request.exists(), "{case}");
            assert_eq!(
                state.auto_install_hold(),
                Some(AutoInstallHold::ProtocolChange),
                "{case}"
            );
            host.abort();
        }
    }

    /// Once prepared, the member says the swap waits for its workers to rest,
    /// and asks for nothing again. ⚠️ ONLY WHILE IT WOULD APPLY BY ITSELF: with
    /// automatic engine updates off it waits for the press, and says so.
    #[tokio::test]
    async fn a_swap_prepared_by_itself_is_reported_as_waiting_for_rest() {
        let directory = tempfile::tempdir().unwrap();
        let fixture = Fixture::swapping(Reachable);
        let (state, request, host) = hive_with_a_downloaded_release(&directory, &fixture);
        std::fs::remove_dir_all(directory.path().join("downloads").join("99.0.0")).unwrap();
        prepared_migration(directory.path(), "99.0.0", "automatic");
        std::fs::write(
            directory.path().join("release-apply.status"),
            "state=deferred\nversion=99.0.0\nprotocol_migration=1\n",
        )
        .unwrap();
        auto_install(&state).await;
        assert!(!request.exists());
        assert_eq!(
            state.auto_install_hold(),
            Some(AutoInstallHold::SwapWaitsForRest)
        );

        crate::task_store(&state)
            .unwrap()
            .set_worker_engine_auto_update(false, 2)
            .unwrap();
        auto_install(&state).await;
        assert!(!request.exists());
        assert_eq!(
            state.auto_install_hold(),
            Some(AutoInstallHold::ProtocolChange)
        );
        host.abort();
    }

    /// Queen's condition on ADR 0169: the member names an automatic swap that
    /// stopped its workers and did not finish, whether it failed or was never
    /// heard from again. It is not tried again by itself.
    #[tokio::test]
    async fn an_automatic_swap_that_did_not_finish_is_named_and_not_tried_again() {
        for outcome in [None, Some(WorkerEngineUpdateOutcome::Failed)] {
            let directory = tempfile::tempdir().unwrap();
            let fixture = Fixture::swapping(Reachable);
            let (state, request, host) = hive_with_a_downloaded_release(&directory, &fixture);
            let store = crate::task_store(&state).unwrap();
            let attempt = store
                .begin_worker_engine_update(
                    "1.0.0-fixture",
                    "99.0.0",
                    Some(fixture.release_protocol),
                    Some(1),
                    WorkerEngineUpdateInitiator::Automatic,
                    1,
                )
                .unwrap();
            if let Some(outcome) = outcome {
                store
                    .finish_worker_engine_update(&attempt, outcome, "", 2)
                    .unwrap();
            }
            auto_install(&state).await;
            assert!(!request.exists(), "{outcome:?}");
            assert_eq!(
                state.auto_install_hold(),
                Some(AutoInstallHold::SwapAttempted),
                "{outcome:?}"
            );
            host.abort();
        }
    }

    /// The Updates card is told how a swapping offer gets swapped (ADR 0169),
    /// so it does not say "nothing is installed until you say so" where the
    /// release installs itself once every worker rests, and does not send the
    /// operator to the maintenance action for a swap Swarm applies itself.
    #[tokio::test]
    async fn the_updates_card_is_told_how_a_swapping_offer_is_swapped() {
        for (case, expected) in [
            ("installs itself", Some(AutomaticSwap::WaitsForRest)),
            ("prepared by itself", Some(AutomaticSwap::WaitsForRest)),
            ("automatic install off", Some(AutomaticSwap::WaitsForPress)),
            ("engine switch off", Some(AutomaticSwap::WaitsForPress)),
            ("operator prepared", Some(AutomaticSwap::WaitsForPress)),
            ("already attempted", Some(AutomaticSwap::WaitsForPress)),
            ("attempted", Some(AutomaticSwap::Attempted)),
            ("no swap", None),
        ] {
            let directory = tempfile::tempdir().unwrap();
            let fixture = if case == "no swap" {
                Fixture::ordinary(Reachable)
            } else {
                Fixture::swapping(Reachable)
            };
            let (state, _request, host) = hive_with_a_downloaded_release(&directory, &fixture);
            let store = crate::task_store(&state).unwrap();
            match case {
                "prepared by itself" => prepared_migration(directory.path(), "99.0.0", "automatic"),
                "automatic install off" => {
                    store.set_release_auto_install(false).unwrap();
                }
                "engine switch off" => {
                    store.set_worker_engine_auto_update(false, 1).unwrap();
                }
                "operator prepared" => prepared_migration(directory.path(), "99.0.0", "manual"),
                "already attempted" => std::fs::write(
                    directory.path().join("release-apply.status"),
                    "state=failed\nversion=99.0.0\nstep=migrate-protocol\n",
                )
                .unwrap(),
                "attempted" => {
                    store
                        .begin_worker_engine_update(
                            "1.0.0-fixture",
                            "99.0.0",
                            Some(fixture.release_protocol),
                            Some(1),
                            WorkerEngineUpdateInitiator::Automatic,
                            1,
                        )
                        .unwrap();
                }
                _ => {}
            }
            let status = current_status(&state).await.unwrap();
            assert_eq!(status.engine_swap, expected, "{case}");
            host.abort();
        }
    }

    /// A migration prepared in this Hive's state root, as swarm-package leaves
    /// one: `protocol-migration.pending` and its `marker` (`manual` or
    /// `automatic`) naming the installed release.
    fn prepared_migration(root: &Path, version: &str, marker: &str) {
        let release = root.join("releases").join(version);
        std::fs::create_dir_all(&release).unwrap();
        std::fs::write(
            release.join("PROTOCOL"),
            format!("{}\n", swarm_terminal::PROTOCOL_VERSION + 1),
        )
        .unwrap();
        std::fs::write(release.join("VERSION"), format!("{version}\n")).unwrap();
        let named = format!("{}\n", release.display());
        std::fs::write(root.join("protocol-migration.pending"), &named).unwrap();
        std::fs::write(root.join(format!("protocol-migration.{marker}")), &named).unwrap();
    }

    /// The automatic swap's own holds come after the switch, a development
    /// build and having nothing newer, and before the workers and the
    /// one-attempt rule: a swap waiting for rest is not "in use".
    #[test]
    fn the_automatic_swap_holds_come_after_the_switch_and_before_the_workers() {
        let facts = |swap| AutoInstallFacts {
            enabled: true,
            upgrade_available: true,
            presence: Some(AtHive),
            workers_idle: false,
            last_outcome: Some("deferred"),
            swap,
            ..AutoInstallFacts::default()
        };
        assert_eq!(
            auto_install_hold(&facts(Some(AutomaticSwap::WaitsForRest))),
            Some(AutoInstallHold::SwapWaitsForRest)
        );
        assert_eq!(
            auto_install_hold(&facts(Some(AutomaticSwap::Attempted))),
            Some(AutoInstallHold::SwapAttempted)
        );
        assert_eq!(
            auto_install_hold(&facts(None)),
            Some(AutoInstallHold::InUse)
        );
        assert_eq!(
            auto_install_hold(&AutoInstallFacts {
                enabled: false,
                ..facts(Some(AutomaticSwap::WaitsForRest))
            }),
            Some(AutoInstallHold::TurnedOff)
        );
    }

    /// The ordinary case, which is the whole point of ADR 0110: a newer
    /// release, nobody at the Hive, whatever the workers are doing.
    #[test]
    fn an_ordinary_release_installs_while_the_operator_is_away() {
        for presence in [Reachable, NightWatch] {
            assert_eq!(
                hold(true, false, true, Some(presence), false, false, None),
                None,
                "{presence:?}"
            );
        }
    }

    /// ADR 0167: the operator at the Hive does not hold an install while every
    /// worker is idle. The engine's own update waits for exactly that.
    #[test]
    fn an_ordinary_release_installs_while_every_worker_is_idle() {
        assert_eq!(
            hold(true, false, true, Some(AtHive), true, false, None),
            None
        );
        assert_eq!(
            hold(true, false, true, None, true, false, None),
            None,
            "idle workers are enough even when presence cannot be read"
        );
    }

    #[test]
    fn nothing_installs_under_a_busy_worker_with_the_operator_at_the_hive() {
        assert_eq!(
            hold(true, false, true, Some(AtHive), false, false, None),
            Some(AutoInstallHold::InUse)
        );
        assert_eq!(
            hold(true, false, true, None, false, false, None),
            Some(AutoInstallHold::InUse),
            "presence that cannot be read is not evidence of absence"
        );
    }

    /// Read the way the release's own `update` reads it (ADR 0157).
    #[test]
    fn a_release_that_drives_the_running_engine_installs_the_app_alone() {
        assert_eq!(
            swaps_engine(Some("19\n"), Some("17\n"), Some(17)),
            Some(false)
        );
        assert_eq!(swaps_engine(Some("19"), Some("17"), Some(18)), Some(false));
        assert_eq!(swaps_engine(Some("19"), Some("17"), Some(19)), Some(false));
        assert_eq!(
            swaps_engine(Some("17"), None, Some(17)),
            Some(false),
            "the same protocol never swaps, with or without the file"
        );
    }

    /// A swap ends every worker session, so it stays the operator's call.
    #[test]
    fn a_release_that_cannot_drive_the_running_engine_swaps_it() {
        assert_eq!(swaps_engine(Some("19"), Some("18"), Some(17)), Some(true));
        assert_eq!(
            swaps_engine(Some("19"), None, Some(17)),
            Some(true),
            "a release from before ADR 0157 swaps both"
        );
        assert_eq!(
            swaps_engine(Some("19"), Some("seventeen"), Some(17)),
            Some(true),
            "an unreadable range drives nothing, as the installer reads it"
        );
        assert_eq!(
            swaps_engine(Some("19"), Some("17"), Some(20)),
            Some(true),
            "an engine newer than the release"
        );
    }

    /// A Hive that cannot tell does not guess.
    #[test]
    fn a_release_or_engine_that_cannot_be_read_is_unknown() {
        assert_eq!(swaps_engine(None, Some("17"), Some(17)), None);
        assert_eq!(swaps_engine(Some("x"), Some("17"), Some(17)), None);
        assert_eq!(swaps_engine(Some("19"), Some("17"), None), None);
    }

    /// ⚠️ ONE ATTEMPT PER RELEASE. A release that failed to install must not be
    /// retried every fifteen minutes for as long as the operator is away.
    #[test]
    fn a_release_that_did_not_install_is_not_tried_again_by_itself() {
        for outcome in ["failed", "refused", "deferred"] {
            assert_eq!(
                hold(
                    true,
                    false,
                    true,
                    Some(Reachable),
                    true,
                    false,
                    Some(outcome)
                ),
                Some(AutoInstallHold::AlreadyAttempted),
                "{outcome}"
            );
        }
        assert_eq!(
            hold(true, false, true, Some(Reachable), true, true, None),
            Some(AutoInstallHold::InFlight)
        );
    }

    #[test]
    fn the_switch_a_development_build_and_nothing_newer_each_hold_it() {
        assert_eq!(
            hold(false, false, true, Some(Reachable), true, false, None),
            Some(AutoInstallHold::TurnedOff)
        );
        assert_eq!(
            hold(true, true, true, Some(Reachable), true, false, None),
            Some(AutoInstallHold::DevelopmentBuild),
            "ADR 0050 §4: a working copy is never replaced by a release"
        );
        assert_eq!(
            hold(true, false, false, Some(Reachable), true, false, None),
            Some(AutoInstallHold::NothingNewer)
        );
    }

    /// A Hive nobody configured still learns there is an update.
    ///
    /// This is the whole behaviour change. `unset` is what every fresh install
    /// has, and it used to be treated as `off` -- so a new machine checked
    /// never, and the only way to find out was to open Settings and opt in.
    /// Reported 2026-09-11 on a box built from scratch that morning.
    #[test]
    fn an_unconfigured_hive_checks_and_only_an_explicit_off_is_silence() {
        assert!(check_is_due("unset", None, 1_000_000, false));
        assert!(check_is_due("daily", None, 1_000_000, false));
        // An Apiary requiring automatic install keeps checking on; nothing else does.
        assert_eq!(effective_mode("off", Some(true)), "daily");
        assert_eq!(effective_mode("off", Some(false)), "off");
        assert_eq!(effective_mode("off", None), "off");
        assert_eq!(effective_mode("unset", Some(true)), "unset");
        assert!(
            !check_is_due("off", None, 1_000_000, false),
            "someone who said off must never be contacted, including on startup"
        );
        assert!(!check_is_due("off", None, 1_000_000, true));
    }

    /// Four hours, and the startup pass has its own shorter floor.
    ///
    /// The floor is what stops a restart LOOP becoming one request per boot,
    /// which is the obvious way an eager startup check turns into a problem.
    #[test]
    fn the_interval_is_four_hours_and_a_restart_may_recheck_sooner() {
        const HOUR: i64 = 3_600;
        let now = 1_000_000;

        assert!(!check_is_due("unset", Some(now - 3 * HOUR), now, false));
        assert!(check_is_due("unset", Some(now - 4 * HOUR), now, false));

        // A restart 20 minutes after the last check may ask again; 10 may not.
        assert!(check_is_due("unset", Some(now - 20 * 60), now, true));
        assert!(!check_is_due("unset", Some(now - 10 * 60), now, true));
        // ...and that shorter floor applies ONLY to the startup pass.
        assert!(!check_is_due("unset", Some(now - 20 * 60), now, false));
    }

    /// A clock that moved backwards must not wedge checking forever.
    #[test]
    fn a_backwards_clock_does_not_stop_checks() {
        let now = 1_000_000;
        assert!(
            check_is_due("unset", Some(now + 99_999), now, false),
            "saturating_sub yields 0 for a future stamp, which must not read as recently checked"
        );
    }

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
    use super::apply_field_from;
    use super::release_swaps_engine;

    fn release(files: &[(&str, &str)]) -> tempfile::TempDir {
        let directory = tempfile::tempdir().unwrap();
        for (name, value) in files {
            std::fs::write(directory.path().join(name), value).unwrap();
        }
        directory
    }

    /// A swap ends every worker session, so this decides whether the roster is
    /// written down first. On 2026-08-28 it was not, and the operator woke
    /// workers one at a time.
    #[test]
    fn a_release_that_cannot_drive_the_engine_swaps_it() {
        let swaps = release(&[("PROTOCOL", "10\n")]);
        assert_eq!(release_swaps_engine(swaps.path(), Some(9)), Some(true));
    }

    /// The ordinary case, and the app-first one (ADR 0157), which must not wipe
    /// the roster: their workers keep running.
    #[test]
    fn a_release_on_the_engines_protocol_or_driving_it_does_not_swap_it() {
        let same = release(&[("PROTOCOL", "10\n")]);
        assert_eq!(release_swaps_engine(same.path(), Some(10)), Some(false));
        let drives = release(&[("PROTOCOL", "19\n"), ("OLDEST_ENGINE_PROTOCOL", "17\n")]);
        assert_eq!(release_swaps_engine(drives.path(), Some(17)), Some(false));
    }

    /// A release whose protocol cannot be read is not called a swap: treating
    /// absent as "swaps" would write down every worker on an ordinary update.
    #[test]
    fn an_absent_release_protocol_is_not_a_swap() {
        let absent = release(&[]);
        assert_eq!(release_swaps_engine(absent.path(), Some(9)), None);
    }

    /// And neither is a value that is not a number, or an engine that did not
    /// answer.
    #[test]
    fn an_unparseable_protocol_or_a_silent_engine_is_not_a_swap() {
        let unparseable = release(&[("PROTOCOL", "ten\n")]);
        assert_eq!(release_swaps_engine(unparseable.path(), Some(9)), None);
        let readable = release(&[("PROTOCOL", "10\n")]);
        assert_eq!(release_swaps_engine(readable.path(), None), None);
    }

    /// This test previously carried its own copy of the rule and passed while
    /// the real function was untouched — the version comparison never reached
    /// `apply_state`, shipped in 0.5.0 and 0.6.0, and the operator saw a
    /// failure from an earlier release reported against a later one. It calls
    /// the function now.
    #[test]
    fn only_a_status_about_the_release_in_hand_is_reported() {
        let failed_on_020 = "state=failed\nversion=0.2.0\n";
        assert_eq!(
            apply_field_from(failed_on_020, Some("0.2.0"), "state=").as_deref(),
            Some("failed")
        );
        // The defect: this used to report "failed" while offering 0.6.0.
        assert_eq!(
            apply_field_from(failed_on_020, Some("0.6.0"), "state="),
            None
        );
        assert_eq!(apply_field_from(failed_on_020, None, "state="), None);
        // Written before statuses were stamped; not guessed at.
        assert_eq!(
            apply_field_from("state=failed\n", Some("0.2.0"), "state="),
            None
        );
        // A current result still reads normally.
        assert_eq!(
            apply_field_from("state=installing\nversion=0.6.0\n", Some("0.6.0"), "state=")
                .as_deref(),
            Some("installing")
        );
    }
}

/// The notes shipped inside the running release, if this build came from one.
///
/// Read from the bundle rather than fetched, because the artifact is already on
/// disk and its hash is what the manifest signature covered. Located from the
/// running binary rather than from configuration so it cannot point at a
/// different release than the one executing.
///
/// A DEVELOPMENT BUILD HAS NO NOTES AND THAT IS CORRECT. A reload from the
/// checkout is not a release, has no version to be new since, and nothing about
/// it should greet the operator with a change list.
#[must_use]
pub(crate) fn installed_release_notes() -> ReleaseNotes {
    let Ok(executable) = std::env::current_exe() else {
        return ReleaseNotes::default();
    };
    // <bundle>/bin/<binary>, so the bundle is two levels up.
    let Some(bundle) = executable.parent().and_then(std::path::Path::parent) else {
        return ReleaseNotes::default();
    };
    let Ok(raw) = std::fs::read_to_string(bundle.join("NOTES")) else {
        return ReleaseNotes::default();
    };
    // Malformed notes are dropped rather than surfaced. Nothing depends on
    // them, and a half-parsed change list is worse than none.
    serde_json::from_str(&raw).unwrap_or_default()
}

/// The version this Hive was running before the release it is running now.
///
/// THE ANCHOR HAS TO SURVIVE A BROWSER, which is why it is not read from one.
/// The panel's own record of what an operator has seen lives in that browser's
/// local storage, so an operator who updates three releases and then opens the
/// control room somewhere else -- a second machine, a private window, cleared
/// site data -- looks to it exactly like a first-ever install, and a first-ever
/// install is deliberately shown nothing. They would be told about none of the
/// three.
///
/// Taken from the `previous` symlink the installer maintains rather than from
/// anything this process records: it IS the release that was replaced, it costs
/// no state and no migration, and it is already correct on every Hive that has
/// ever been updated. A checkout reload has no such link and answers `None`,
/// which correctly falls back to the browser's own record.
#[must_use]
pub(crate) fn previous_installed_version() -> Option<String> {
    let executable = std::env::current_exe().ok()?;
    // <install_root>/releases/<version>/bin/<binary>
    let install_root = executable.parent()?.parent()?.parent()?.parent()?;
    let raw = std::fs::read_to_string(install_root.join("previous").join("VERSION")).ok()?;
    let version = raw.trim();
    if version.is_empty() {
        return None;
    }
    Some(version.to_owned())
}

/// What is new for an operator who last saw `since`.
///
/// # Errors
/// Returns an error only when the caller is not authorized.
pub(super) async fn notes(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let notes = installed_release_notes();
    Ok((
        [(header::CACHE_CONTROL, "no-store")],
        Json(serde_json::json!({
            "running_version": crate::build_version(),
            "previous_version": previous_installed_version(),
            "releases": notes.releases,
        })),
    )
        .into_response())
}

#[cfg(test)]
mod install_in_flight_tests {
    use super::install_in_flight;
    use crate::AppState;
    use std::time::{Duration, SystemTime};

    fn hive() -> (tempfile::TempDir, AppState, std::path::PathBuf) {
        let directory = tempfile::tempdir().unwrap();
        let request = directory.path().join("release-apply.request");
        let state =
            AppState::default().with_release_paths(directory.path().to_owned(), request.clone());
        (directory, state, request)
    }

    fn status(directory: &tempfile::TempDir, contents: &str) -> std::path::PathBuf {
        let path = directory.path().join("release-apply.status");
        std::fs::write(&path, contents).unwrap();
        path
    }

    #[test]
    fn nothing_queued_and_nothing_running_allows_an_install() {
        let (_directory, state, request) = hive();
        assert!(!install_in_flight(&state, Some("1.14.1"), &request));
    }

    /// ⚠️ THE SECOND PRESS. A queued request is an install that has not started
    /// yet, and a second one of the same release collides with it on the
    /// lifecycle lock and writes its failure over the one doing the work.
    #[test]
    fn a_queued_request_refuses_another() {
        let (_directory, state, request) = hive();
        std::fs::write(&request, "/downloads/swarm-1.14.1").unwrap();
        assert!(install_in_flight(&state, Some("1.14.1"), &request));
    }

    #[test]
    fn a_running_install_of_this_release_refuses_another() {
        let (directory, state, request) = hive();
        status(&directory, "state=installing\nversion=1.14.1\n");
        assert!(install_in_flight(&state, Some("1.14.1"), &request));
    }

    /// A finished attempt, successful or not, is not in the way of trying again.
    #[test]
    fn a_finished_attempt_does_not_block_the_next() {
        let (directory, state, request) = hive();
        for ended in [
            "state=failed\nversion=1.14.1\n",
            "state=installed\nversion=1.14.1\n",
        ] {
            status(&directory, ended);
            assert!(
                !install_in_flight(&state, Some("1.14.1"), &request),
                "{ended}"
            );
        }
    }

    /// A status about a different release says nothing about this one.
    #[test]
    fn an_install_of_another_release_does_not_block_this_one() {
        let (directory, state, request) = hive();
        status(&directory, "state=installing\nversion=1.14.0\n");
        assert!(!install_in_flight(&state, Some("1.14.1"), &request));
    }

    /// ⚠️ A STUCK BUTTON IS NOT A FIX FOR A STUCK INSTALL. An attempt that died
    /// without writing its ending leaves `installing` behind forever; believing
    /// it indefinitely would refuse Install on this Hive until someone deleted a
    /// file by hand.
    #[test]
    fn an_installing_status_nobody_has_touched_for_a_long_time_is_not_believed() {
        let (directory, state, request) = hive();
        let path = status(&directory, "state=installing\nversion=1.14.1\n");
        std::fs::File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_modified(SystemTime::now() - Duration::from_secs(20 * 60))
            .unwrap();
        assert!(!install_in_flight(&state, Some("1.14.1"), &request));
    }
}
