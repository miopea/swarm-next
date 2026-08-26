use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use axum::{
    Json,
    extract::State,
    http::{HeaderMap, header},
    response::{IntoResponse, Response},
};
use futures_util::{StreamExt, stream};
use serde::Serialize;
use swarm_domain::ProviderKind;
use swarm_domain::{ControlRoomEventKind, WorkerProfile, WorkerSessionId};
use swarm_terminal::{
    HostRequest, HostResponse, ProviderActivity, TerminalSnapshot, classify_provider_activity,
};

use crate::{ApiError, AppState, authorize, terminal_host::request_host};

#[derive(Clone, Debug, Serialize)]
pub(super) struct ProviderCapabilitiesView {
    claude_code: bool,
    codex: bool,
    /// The release each provider resolves to now, and the workers still running
    /// something older.
    ///
    /// Claude and Codex update themselves and the running process keeps
    /// executing what it started with, so an update installed while workers are
    /// up is not running anywhere until each one restarts.
    #[serde(default)]
    superseded: Vec<SupersededProviderView>,
}

#[derive(Clone, Debug, Serialize)]
pub(super) struct SupersededProviderView {
    provider: &'static str,
    version: Option<String>,
    installed_at: Option<i64>,
    worker_ids: Vec<String>,
}

/// The workers still running a provider release that disk has moved past.
///
/// Reports nothing rather than guessing when the roster cannot be read: a
/// restart prompt that is not needed teaches the operator to ignore the one
/// that is.
fn superseded_providers(
    state: &AppState,
    claude_release: Option<&swarm_terminal::ProviderRelease>,
    codex_release: Option<&swarm_terminal::ProviderRelease>,
) -> Vec<SupersededProviderView> {
    let Ok(sessions) = crate::task_store(state).and_then(|store| {
        store
            .active_worker_sessions()
            .map_err(|error| crate::task_store_error(&error))
    }) else {
        return Vec::new();
    };
    [
        (
            "claude_code",
            swarm_domain::ProviderKind::ClaudeCode,
            claude_release,
        ),
        ("codex", swarm_domain::ProviderKind::Codex, codex_release),
    ]
    .into_iter()
    .filter_map(|(name, kind, release)| {
        let worker_ids = sessions
            .iter()
            .filter(|session| {
                session.provider == kind
                    && swarm_terminal::provider_release_superseded(release, session.started_at)
            })
            .map(|session| session.worker_id.to_string())
            .collect::<Vec<_>>();
        (!worker_ids.is_empty()).then(|| SupersededProviderView {
            provider: name,
            version: release.and_then(|release| release.version.clone()),
            installed_at: release.and_then(|release| release.installed_at),
            worker_ids,
        })
    })
    .collect()
}

async fn observe(
    state: &AppState,
    profiles: &[WorkerProfile],
    live: &HashSet<WorkerSessionId>,
) -> HashMap<WorkerSessionId, ProviderSignals> {
    let observations = profiles
        .iter()
        .filter_map(|profile| {
            let session_id = profile.active_session_id?;
            live.contains(&session_id)
                .then_some((session_id, profile.provider))
        })
        .collect::<Vec<_>>();
    stream::iter(observations)
        .map(|(session_id, provider)| async move {
            observe_session(state, session_id, provider)
                .await
                .map(|signals| (session_id, signals))
        })
        .buffer_unordered(8)
        .filter_map(async move |observation| observation)
        .collect()
        .await
}

/// Reads the exact current host-owned snapshot for one provider session.
///
/// Coordination delivery uses this immediately before writing so an open
/// provider question is an input-authority boundary rather than a visual hint.
/// What one observation of a session saw.
///
/// Activity decides routing; background work is reported beside it so an
/// operator can tell a worker with nothing to do from one that finished while
/// something it started is still running. Both classify as Resting, and that is
/// deliberate — the turn HAS ended — but they are not the same situation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct ProviderSignals {
    pub(super) activity: ProviderActivity,
    pub(super) background_work: bool,
}

pub(super) async fn observe_session(
    state: &AppState,
    session_id: WorkerSessionId,
    provider: ProviderKind,
) -> Option<ProviderSignals> {
    match request_host(
        state,
        HostRequest::Read {
            session_id,
            after_sequence: None,
        },
    )
    .await
    {
        Ok(HostResponse::Output {
            resume: swarm_terminal::Resume::Snapshot { snapshot },
            running: true,
            ..
        }) => Some(ProviderSignals {
            activity: classify_observed_activity(provider, &snapshot),
            background_work: swarm_terminal::background_work_running(provider, &snapshot),
        }),
        _ => None,
    }
}

/// Adds application-level provider UI recognition without changing the
/// independently deployed terminal engine. Claude can leave a slash-command
/// palette open below its input row; enough suggestions can push the prompt
/// outside the terminal crate's deliberately small recent-line window even
/// though the provider is visibly waiting for input.
pub(super) fn classify_observed_activity(
    provider: ProviderKind,
    snapshot: &TerminalSnapshot,
) -> ProviderActivity {
    let activity = classify_provider_activity(provider, snapshot);
    if activity != ProviderActivity::Unknown {
        return activity;
    }
    let mut parser = vt100::Parser::new(snapshot.rows, snapshot.columns, 0);
    parser.process(&snapshot.bytes);
    if input_palette_is_resting(provider, &parser.screen().contents()) {
        ProviderActivity::Resting
    } else {
        activity
    }
}

/// Returns true when the provider is idle but its current prompt already owns
/// unsubmitted text. A resting footer is not permission to append another
/// coordination message: doing so merges independent tasks and decisions into
/// one prompt and makes a later Enter ambiguous.
pub(super) fn has_open_provider_input(provider: ProviderKind, snapshot: &TerminalSnapshot) -> bool {
    let mut parser = vt100::Parser::new(snapshot.rows, snapshot.columns, 0);
    parser.process(&snapshot.bytes);
    let screen = parser.screen();
    let marker = prompt_marker(provider);
    // Only the lowest prompt marker is the composer. The ones above it are the
    // transcript — commands already submitted, echoed back with their answers.
    // Every row, bottom upwards. Windowing to the last twelve screen rows
    // looked equivalent to the old line-based window and is not: a short
    // transcript sits at the top of the screen, so the composer was never
    // examined at all and every answer was "nothing typed".
    let Some(row) = (0..snapshot.rows)
        .rev()
        .find(|row| marker_column(screen, *row, marker).is_some())
    else {
        return false;
    };
    let Some(column) = marker_column(screen, row, marker) else {
        return false;
    };
    let mut typed = false;
    for cell_column in column + 1..snapshot.columns {
        let Some(cell) = screen.cell(row, cell_column) else {
            continue;
        };
        let contents = cell.contents();
        if contents.trim().is_empty() {
            continue;
        }
        // Claude draws a suggested command into the empty composer in grey.
        // It is a proposal, not something anybody typed, and it disappears the
        // moment a key is pressed — so treating it as unsent input froze every
        // delivery to that worker for as long as the suggestion was on screen.
        // Measured 2026-08-23: Queen's composer showed "push the architecture
        // doc fix" that nobody had written, and her review was refused for
        // hours.
        if !is_suggestion_styling(cell) {
            typed = true;
            break;
        }
    }
    typed
}

fn prompt_marker(provider: ProviderKind) -> char {
    match provider {
        ProviderKind::ClaudeCode => '\u{276f}',
        ProviderKind::Codex => '\u{203a}',
        // NUL, because this build knows no marker for an unrecognised provider
        // and a terminal never emits one — so nothing matches, which is the
        // honest result. Borrowing another provider's glyph would invent a
        // reading of a screen we cannot interpret.
        ProviderKind::Unsupported => '\0',
    }
}

fn marker_column(screen: &vt100::Screen, row: u16, marker: char) -> Option<u16> {
    let mut buffer = [0u8; 4];
    let marker = marker.encode_utf8(&mut buffer);
    (0..screen.size().1).find(|column| {
        screen
            .cell(row, *column)
            .is_some_and(|cell| cell.contents() == marker)
    })
}

/// Whether this cell is drawn the way a provider draws a suggestion.
///
/// Deliberately narrow. A false positive here is Swarm typing over something
/// the operator wrote, which is the exact harm the open-input rule exists to
/// prevent, so anything that is not clearly muted counts as typed.
fn is_suggestion_styling(cell: &vt100::Cell) -> bool {
    cell.dim() || is_muted_foreground(cell.fgcolor())
}

fn is_muted_foreground(color: vt100::Color) -> bool {
    match color {
        // Bright black, and the grey end of the 256-colour ramp.
        vt100::Color::Idx(index) => index == 8 || (232..=250).contains(&index),
        // Grey means the channels agree; dark means it is not the body text.
        vt100::Color::Rgb(red, green, blue) => {
            let spread = red.max(green).max(blue) - red.min(green).min(blue);
            spread <= 16 && red.max(green).max(blue) <= 160
        }
        vt100::Color::Default => false,
    }
}

fn input_palette_is_resting(provider: ProviderKind, visible: &str) -> bool {
    let lines = visible.lines().map(str::trim).collect::<Vec<_>>();
    let Some(prompt_index) = lines.iter().rposition(|line| match provider {
        ProviderKind::ClaudeCode => line == &"❯ /" || line.starts_with("❯ /"),
        ProviderKind::Codex => line == &"› /" || line.starts_with("› /"),
        ProviderKind::Unsupported => false,
    }) else {
        return false;
    };
    lines[prompt_index + 1..]
        .iter()
        .filter(|line| line.starts_with('/') && line.len() > 1)
        .take(2)
        .count()
        >= 2
}

pub(super) async fn refresh(
    state: &AppState,
    profiles: &[WorkerProfile],
    live: &HashSet<WorkerSessionId>,
) -> HashMap<WorkerSessionId, ProviderSignals> {
    let observed = observe(state, profiles, live).await;
    let changed = {
        let mut previous = state.provider_activity.write().await;
        if *previous == observed {
            false
        } else {
            previous.clone_from(&observed);
            true
        }
    };
    if changed {
        if let Some(store) = &state.task_store
            && let Err(error) =
                store.record_control_room_event(ControlRoomEventKind::RuntimeChanged)
        {
            tracing::warn!(%error, "provider activity change could not publish its runtime event");
        }
        state.control_room_notify.notify_waiters();
    }
    observed
}

pub(super) async fn capabilities(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let capabilities = match request_host(&state, HostRequest::ProviderCapabilities).await {
        Ok(HostResponse::ProviderCapabilities {
            claude_code,
            codex,
            claude_release,
            codex_release,
        }) => ProviderCapabilitiesView {
            claude_code,
            codex,
            superseded: superseded_providers(
                &state,
                claude_release.as_ref(),
                codex_release.as_ref(),
            ),
        },
        _ => ProviderCapabilitiesView {
            claude_code: true,
            codex: false,
            superseded: Vec::new(),
        },
    };
    Ok(([(header::CACHE_CONTROL, "no-store")], Json(capabilities)).into_response())
}

#[cfg(test)]
mod tests {
    use super::*;
    use swarm_terminal::{CanonicalTerminalState, JournalLimits, TerminalSize};

    fn snapshot(text: &str) -> TerminalSnapshot {
        let mut state = CanonicalTerminalState::new(
            JournalLimits::new(64, 64 * 1024),
            TerminalSize::new(24, 100),
        );
        state.push(text.as_bytes().to_vec());
        state.snapshot()
    }

    #[test]
    fn claude_slash_palette_remains_resting_when_suggestions_hide_the_prompt() {
        let suggestions = (0..18)
            .map(|index| format!("/command-{index}  Description {index}"))
            .collect::<Vec<_>>()
            .join("\r\n");
        let snapshot = snapshot(&format!("✻ Worked for 18s\r\n\r\n❯ /\r\n{suggestions}"));

        assert_eq!(
            classify_provider_activity(ProviderKind::ClaudeCode, &snapshot),
            ProviderActivity::Unknown,
            "the base classifier stays conservative once the prompt leaves its recent window"
        );
        assert_eq!(
            classify_observed_activity(ProviderKind::ClaudeCode, &snapshot),
            ProviderActivity::Resting
        );
    }

    #[test]
    fn typed_provider_input_is_not_an_idle_delivery_target() {
        assert!(has_open_provider_input(
            ProviderKind::ClaudeCode,
            &snapshot("❯ [Swarm decision 01ab23cd resolved]\r\nauto mode on"),
        ));
        assert!(!has_open_provider_input(
            ProviderKind::ClaudeCode,
            &snapshot("✻ Finished\r\n❯ \r\nauto mode on"),
        ));
    }

    #[test]
    fn historical_commands_do_not_turn_unknown_output_into_resting() {
        let output = (0..12)
            .map(|index| format!("provider output changed {index}"))
            .collect::<Vec<_>>()
            .join("\r\n");
        let snapshot = snapshot(&format!("❯ /status\r\n/one historical path\r\n{output}"));

        assert_eq!(
            classify_provider_activity(ProviderKind::ClaudeCode, &snapshot),
            ProviderActivity::Unknown
        );
        assert_eq!(
            classify_observed_activity(ProviderKind::ClaudeCode, &snapshot),
            ProviderActivity::Unknown
        );
    }

    /// The 2026-08-23 wedge, reproduced from Queen's actual screen.
    ///
    /// Her composer was empty. Two lines above it sat `❯ /login`, a command she
    /// had already run, with its answer underneath. Swarm read the transcript
    /// as unsent input and refused every delivery for three and a half hours —
    /// 466 refusals, a review queued 24 hours, and a board with nothing active
    /// on it.
    #[test]
    fn a_submitted_command_in_the_transcript_is_not_unsent_input() {
        let snapshot = snapshot(concat!(
            "● Remote Control disconnected — run /remote-control to start a session\r\n",
            "❯ /login\r\n",
            " ⎿ Login successful\r\n",
            "────────────────────────────\r\n",
            "❯\r\n",
            "────────────────────────────\r\n",
            "⏵⏵ auto mode on (shift+tab to cycle) · ← for agents",
        ));

        assert_eq!(
            classify_observed_activity(ProviderKind::ClaudeCode, &snapshot),
            ProviderActivity::Resting
        );
        assert!(
            !has_open_provider_input(ProviderKind::ClaudeCode, &snapshot),
            "the empty composer is the lowest prompt marker; the one above it is history"
        );
    }

    /// The protection this rule exists for still has to hold: text the operator
    /// typed and did not send must block a write, because a later Enter would
    /// submit theirs and Swarm's as one instruction.
    #[test]
    fn text_left_in_the_composer_still_blocks_a_delivery() {
        let snapshot = snapshot(concat!(
            "❯ /login\r\n",
            " ⎿ Login successful\r\n",
            "────────────────────────────\r\n",
            "❯ /rc\r\n",
            "────────────────────────────\r\n",
            "⏵⏵ auto mode on (shift+tab to cycle) · ← for agents",
        ));

        assert!(has_open_provider_input(ProviderKind::ClaudeCode, &snapshot));
    }

    /// The screen the operator photographed: Claude had drawn a suggested
    /// command into an empty composer in grey, and Swarm read it as unsent
    /// input. Every delivery to that worker was refused for as long as the
    /// suggestion was on screen, which on Queen meant the whole Hive stopped.
    ///
    /// Nobody typed it. It vanishes on the next keypress.
    #[test]
    fn a_greyed_out_suggestion_is_not_something_the_operator_typed() {
        // SGR 2 is dim; SGR 22 restores it.
        let dim = snapshot("\x1b[2m\u{276f} push the architecture doc fix\x1b[22m");
        assert!(!has_open_provider_input(ProviderKind::ClaudeCode, &dim));

        // The same suggestion drawn in a grey from the 256-colour ramp.
        let grey = snapshot("\u{276f} \x1b[38;5;244mpush the architecture doc fix\x1b[39m");
        assert!(!has_open_provider_input(ProviderKind::ClaudeCode, &grey));
    }

    /// The protection has to survive the fix. Text the operator actually typed
    /// is drawn in the ordinary foreground, and Swarm must still refuse to
    /// append to it — a later Enter would submit theirs and Swarm's as one.
    #[test]
    fn text_the_operator_typed_still_blocks_a_delivery() {
        let typed = snapshot("\u{276f} push the architecture doc fix");
        assert!(has_open_provider_input(ProviderKind::ClaudeCode, &typed));

        // Half-typed over a suggestion still counts: any ordinary cell wins.
        let mixed = snapshot("\u{276f} push\x1b[38;5;244m the architecture doc fix\x1b[39m");
        assert!(has_open_provider_input(ProviderKind::ClaudeCode, &mixed));
    }

    #[test]
    fn an_empty_composer_is_still_empty() {
        let empty = snapshot("\u{276f}\r\n\u{23f5}\u{23f5} auto mode on");
        assert!(!has_open_provider_input(ProviderKind::ClaudeCode, &empty));
    }
}
