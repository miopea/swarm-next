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
    HostRequest, HostResponse, ProviderActivity, ProviderTurn, ProviderTurnPhase, ProviderTurnWait,
    TerminalSnapshot, classify_provider_activity,
};

use crate::{ApiError, AppState, authorize, terminal_host::request_host};

#[derive(Clone, Debug, Serialize)]
pub(super) struct ProviderCapabilitiesView {
    claude_code: bool,
    codex: bool,
    experimental: Option<swarm_domain::ExperimentalProviderAvailability>,
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
    /// Of those, the workers Swarm moves onto the release itself with
    /// `/restart` (ADR 0122). The rest need stopping and starting.
    in_place_worker_ids: Vec<String>,
    /// Of those, the ones already asked and still on the older release.
    asked_worker_ids: Vec<String>,
}

/// The workers still running a provider release that disk has moved past.
///
/// A failed roster read is unavailable evidence, not an empty update list.
fn superseded_providers(
    state: &AppState,
    claude_release: Option<&swarm_terminal::ProviderRelease>,
    codex_release: Option<&swarm_terminal::ProviderRelease>,
    executables: &HashMap<WorkerSessionId, Option<String>>,
) -> Result<Vec<SupersededProviderView>, ApiError> {
    let sessions = crate::task_store(state)?
        .active_worker_sessions()
        .map_err(|error| crate::task_store_error(&error))?;
    let running = |session: &swarm_persistence::ActiveWorkerSession| {
        executables
            .get(&session.session_id)
            .and_then(Option::as_deref)
    };
    Ok([
        (
            "claude_code",
            swarm_domain::ProviderKind::ClaudeCode,
            claude_release,
        ),
        ("codex", swarm_domain::ProviderKind::Codex, codex_release),
    ]
    .into_iter()
    .filter_map(|(name, kind, release)| {
        let behind = sessions
            .iter()
            .filter(|session| {
                session.provider == kind
                    && swarm_terminal::provider_release_superseded(
                        release,
                        session.started_at,
                        running(session),
                    )
            })
            .collect::<Vec<_>>();
        let in_place = behind
            .iter()
            .filter(|session| {
                kind == swarm_domain::ProviderKind::ClaudeCode
                    && release
                        .zip(running(session))
                        .is_some_and(|(release, running)| {
                            swarm_terminal::running_release_current(release, running) == Some(false)
                        })
            })
            .collect::<Vec<_>>();
        let asked = release
            .map(|release| state.provider_updates.asked_onto(release))
            .unwrap_or_default();
        (!behind.is_empty()).then(|| SupersededProviderView {
            provider: name,
            version: release.and_then(|release| release.version.clone()),
            installed_at: release.and_then(|release| release.installed_at),
            worker_ids: behind
                .iter()
                .map(|session| session.worker_id.to_string())
                .collect(),
            in_place_worker_ids: in_place
                .iter()
                .map(|session| session.worker_id.to_string())
                .collect(),
            asked_worker_ids: in_place
                .iter()
                .filter(|session| asked.contains(&session.session_id))
                .map(|session| session.worker_id.to_string())
                .collect(),
        })
    })
    .collect())
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
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct ProviderSignals {
    pub(super) activity: ProviderActivity,
    pub(super) background_work: swarm_terminal::BackgroundWork,
    /// Why the latest turn ended on an API error, while it is the latest
    /// (ADR 0123). Routing still reads the turn as over; this is what the
    /// operator is told about it.
    pub(super) turn_failure: Option<swarm_terminal::ProviderTurnFailure>,
    /// The latest tool call auto mode refused, until Claude runs another tool
    /// or the next turn starts.
    pub(super) denial: Option<swarm_terminal::ProviderDenial>,
}

impl ProviderSignals {
    /// What the screen alone says, for a provider with no turn report.
    pub(super) const fn from_screen(
        activity: ProviderActivity,
        background_work: swarm_terminal::BackgroundWork,
    ) -> Self {
        Self {
            activity,
            background_work,
            turn_failure: None,
            denial: None,
        }
    }
}

pub(super) async fn observe_session(
    state: &AppState,
    session_id: WorkerSessionId,
    provider: ProviderKind,
) -> Option<ProviderSignals> {
    observe_session_snapshot(state, session_id, provider)
        .await
        .map(|(signals, _)| signals)
}

/// Same fenced observation, retained only for explicitly authorized recovery reads.
pub(super) async fn observe_session_snapshot(
    state: &AppState,
    session_id: WorkerSessionId,
    provider: ProviderKind,
) -> Option<(ProviderSignals, TerminalSnapshot)> {
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
            session_id: observed_session,
            resume: swarm_terminal::Resume::Snapshot { snapshot },
            running: true,
            provider_turn,
            last_output_at,
        }) if observed_session == session_id => Some((
            observed_signals(
                provider,
                &snapshot,
                provider_turn.as_ref(),
                last_output_at,
                crate::unix_timestamp(),
            ),
            snapshot,
        )),
        _ => None,
    }
}

/// A turn the hooks call open, on a resting prompt whose terminal has drawn
/// nothing for this long, is over.
///
/// A working Claude redraws its spinner and elapsed time every second, so a
/// quiet minute is not a long tool call. This is the bound for a turn that
/// ended without a Stop AND without Claude's end-of-turn line, which has two
/// documented causes (an interrupt and a lost hook) and one undocumented one
/// (a Claude release that draws the line differently). It is never the first
/// answer: the hooks and the end-of-turn line are.
pub(super) const QUIET_TURN_SECONDS: i64 = 60;

/// Everything one read of a session says: the screen, and Claude's own report
/// of its turn when the session has one (ADR 0123).
pub(super) fn observed_signals(
    provider: ProviderKind,
    snapshot: &TerminalSnapshot,
    turn: Option<&ProviderTurn>,
    last_output_at: Option<i64>,
    now: i64,
) -> ProviderSignals {
    let screen = classify_observed_activity(provider, snapshot);
    let screen_background = swarm_terminal::background_work(provider, snapshot);
    let Some(turn) = turn.filter(|_| provider == ProviderKind::ClaudeCode) else {
        return ProviderSignals::from_screen(screen, screen_background);
    };
    let quiet = last_output_at.is_some_and(|at| now.saturating_sub(at) >= QUIET_TURN_SECONDS);
    ProviderSignals {
        activity: turn_activity(screen, turn, || turn_ended_on_screen(snapshot), quiet),
        // Claude's list is exact and typed, and every task that finishes
        // starts a short turn of its own whose Stop lists the rest. It is
        // read only from a turn that ENDED: a failed one never refreshed it,
        // and a running one may already have started more.
        background_work: match (turn.phase, turn.background) {
            (ProviderTurnPhase::Ended, Some(reported)) => reported.into(),
            _ => screen_background,
        },
        turn_failure: turn
            .failure
            .filter(|_| turn.phase == ProviderTurnPhase::Failed),
        denial: turn.denial.clone(),
    }
}

/// The screen and the hooks, combined (ADR 0123).
///
/// The hooks are exact about what they report and silent about the rest: Esc
/// and Ctrl+C fire nothing, a queued message starts a turn with no
/// `UserPromptSubmit` of its own, and a local command such as `/compact` is not
/// a turn at all. So:
///
/// - A menu on screen is a question, whatever the hooks last said.
/// - An open turn is working, even on a screen that looks idle. That screen
///   misread is what the hooks are for. Unless Claude's end-of-turn line sits
///   on the resting prompt, or the terminal has gone quiet: then the turn
///   ended without telling anyone.
/// - An ended turn adds nothing to the screen, which is right about the work
///   the hooks never report, except a usage limit Claude will not resume.
fn turn_activity(
    screen: ProviderActivity,
    turn: &ProviderTurn,
    ended_on_screen: impl FnOnce() -> bool,
    quiet: bool,
) -> ProviderActivity {
    if screen == ProviderActivity::AwaitingOperator {
        return ProviderActivity::AwaitingOperator;
    }
    match turn.phase {
        ProviderTurnPhase::Working => {
            if screen == ProviderActivity::Resting && (quiet || ended_on_screen()) {
                ProviderActivity::Resting
            } else if screen != ProviderActivity::Active && turn.waiting.is_some() {
                ProviderActivity::AwaitingOperator
            } else {
                ProviderActivity::Active
            }
        }
        ProviderTurnPhase::Ended | ProviderTurnPhase::Failed => {
            if screen != ProviderActivity::Active
                && turn.waiting == Some(ProviderTurnWait::UsageLimit)
            {
                ProviderActivity::AwaitingOperator
            } else {
                screen
            }
        }
    }
}

/// Whether Claude's own end-of-turn line is the last thing above its prompt.
///
/// Two forms, both measured on 2.1.310 and in this Hive's terminal history:
/// `⎿  Interrupted · What should Claude do instead?` after Esc or Ctrl+C, and
/// `✻ Baked for 3s · done 10:14 PM` after a turn that finished (the verb
/// varies; the shape does not). Directly above the input box only: once a new
/// prompt is echoed below it, the line belongs to a turn that is over.
fn turn_ended_on_screen(snapshot: &TerminalSnapshot) -> bool {
    let mut parser = vt100::Parser::new(snapshot.rows, snapshot.columns, 0);
    parser.process(&snapshot.bytes);
    let screen = parser.screen();
    let Some(composer) = provider_composer_row(ProviderKind::ClaudeCode, screen) else {
        return false;
    };
    let columns = screen.size().1;
    let Some(line) = (0..composer)
        .rev()
        .map(|row| screen.contents_between(row, 0, row, columns))
        .map(|line| line.trim().to_owned())
        .find(|line| {
            !line.is_empty() && !line.chars().all(|glyph| matches!(glyph, '─' | '━' | '═'))
        })
    else {
        return false;
    };
    let interrupted = line.starts_with('⎿') && line.contains("Interrupted");
    // A past tense and a number. The working spinner carries the same glyph
    // and a present participle, and can say "thought for 1s" in parentheses.
    let finished = line.starts_with('✻')
        && line.split_once(" for ").is_some_and(|(verb, rest)| {
            verb.ends_with("ed") && rest.starts_with(|glyph: char| glyph.is_ascii_digit())
        });
    interrupted || finished
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
    let Some(row) = provider_composer_row(provider, screen) else {
        return false;
    };
    let Some(column) = marker_column(screen, row, marker) else {
        return false;
    };
    let end = composer_end_row(screen, row);
    for input_row in row..end {
        let start = if input_row == row { column + 1 } else { 0 };
        for cell_column in start..snapshot.columns {
            let Some(cell) = screen.cell(input_row, cell_column) else {
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
                return true;
            }
        }
    }
    false
}

/// A styled input-box rule terminates the composer, not its first physical row.
/// With no known box boundary, retain the existing single-row interpretation
/// plus physical wrap continuations; unrelated footer text is not operator input.
fn composer_end_row(screen: &vt100::Screen, row: u16) -> u16 {
    let rows = screen.size().0;
    if let Some(border) = (row + 1..rows).find(|candidate| input_box_rule(screen, *candidate)) {
        return border;
    }
    let mut end = row + 1;
    while end < rows && screen.row_wrapped(end - 1) {
        end += 1;
    }
    end
}

fn input_box_rule(screen: &vt100::Screen, row: u16) -> bool {
    let mut glyphs = 0;
    for column in 0..screen.size().1 {
        let Some(cell) = screen.cell(row, column) else {
            continue;
        };
        let contents = cell.contents();
        if contents.trim().is_empty() {
            continue;
        }
        if !is_suggestion_styling(cell)
            || !contents
                .chars()
                .all(|glyph| matches!(glyph, '─' | '━' | '═'))
        {
            return false;
        }
        glyphs += contents.chars().count();
    }
    glyphs >= 3
}

/// The lowest provider prompt is the current composer, not submitted history.
/// Excerpt readers use the same boundary as the existing input guard so that
/// suggestions and unsent text cannot become apparent conversation history.
pub(super) fn provider_composer_row(provider: ProviderKind, screen: &vt100::Screen) -> Option<u16> {
    let marker = prompt_marker(provider);
    (0..screen.size().0)
        .rev()
        .find(|row| marker_column(screen, *row, marker).is_some())
}

fn prompt_marker(provider: ProviderKind) -> char {
    match provider {
        ProviderKind::ClaudeCode => '\u{276f}',
        ProviderKind::Codex => '\u{203a}',
        // NUL, because this build knows no marker for an unrecognised provider
        // and a terminal never emits one — so nothing matches, which is the
        // honest result. Borrowing another provider's glyph would invent a
        // reading of a screen we cannot interpret.
        //
        // The alpha three take the same answer for the same reason: their
        // prompt markers are unknown here, and none of their CLIs is installed
        // to find out from.
        ProviderKind::Gemini
        | ProviderKind::Grok
        | ProviderKind::OpenCode
        | ProviderKind::Unsupported => '\0',
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
        ProviderKind::Gemini
        | ProviderKind::Grok
        | ProviderKind::OpenCode
        | ProviderKind::Unsupported => false,
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
    let (changed, busy) = {
        let mut previous = state.provider_activity.write().await;
        // The time since the last look goes to whoever was busy AT that look.
        let busy = previous
            .iter()
            .filter(|(_, signals)| signals.activity == ProviderActivity::Active)
            .map(|(session, _)| *session)
            .collect::<HashSet<_>>();
        if *previous == observed {
            (false, busy)
        } else {
            previous.clone_from(&observed);
            (true, busy)
        }
    };
    crate::apiary_usage::record_busy_time(state, profiles, &busy, crate::unix_timestamp());
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
    let capabilities = match request_host(&state, HostRequest::ProviderCapabilities).await? {
        HostResponse::ProviderCapabilities {
            claude_code,
            codex,
            experimental,
            claude_release,
            codex_release,
        } => ProviderCapabilitiesView {
            claude_code,
            codex,
            experimental,
            superseded: superseded_providers(
                &state,
                claude_release.as_ref(),
                codex_release.as_ref(),
                &crate::provider_updates::running_executables(&state)
                    .await
                    .unwrap_or_default(),
            )?,
        },
        _ => {
            return Err(ApiError::new(
                axum::http::StatusCode::SERVICE_UNAVAILABLE,
                "provider_status_unavailable",
                "The worker engine did not return provider availability. Existing workers are unchanged.",
            ));
        }
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

    const RULE: &str = "────────────────────────────────────────";

    /// A Claude screen at rest, with `above` as the last thing over its prompt.
    fn claude_at_rest(above: &str) -> TerminalSnapshot {
        snapshot(&format!(
            "● Earlier output.\r\n{above}\r\n{RULE}\r\n❯ \r\n{RULE}\r\n  ⏵⏵ auto mode on (shift+tab to cycle)"
        ))
    }

    fn turn(phase: ProviderTurnPhase) -> ProviderTurn {
        ProviderTurn {
            phase,
            ..ProviderTurn::default()
        }
    }

    fn read(
        snapshot: &TerminalSnapshot,
        turn: Option<&ProviderTurn>,
        quiet_for: i64,
    ) -> ProviderSignals {
        observed_signals(
            ProviderKind::ClaudeCode,
            snapshot,
            turn,
            Some(1_000),
            1_000 + quiet_for,
        )
    }

    /// The precondition every case below leans on: these screens read as
    /// Resting on their own. Without it a case could pass because the screen
    /// said something else, and prove nothing about the hooks.
    #[test]
    fn the_test_screens_rest_on_their_own() {
        for above in [
            "● Still working through the files.",
            "  ⎿  Interrupted · What should Claude do instead?",
            "✻ Cogitated for 1s · done 10:14 PM",
            "✻ Churned for 1m 4s · done 9:02 AM · 1 shell still running",
        ] {
            assert_eq!(
                classify_observed_activity(ProviderKind::ClaudeCode, &claude_at_rest(above)),
                ProviderActivity::Resting,
                "{above}"
            );
        }
    }

    /// THE MISREAD THE HOOKS EXIST FOR (ADR 0123). A turn Claude has opened
    /// and not closed is working, even on a screen that looks idle: the screen
    /// reader has read working workers as Resting before (2b631fba, cd9abd1c).
    #[test]
    fn an_open_turn_is_working_on_a_screen_that_looks_idle() {
        let screen = claude_at_rest("● Still working through the files.");
        let working = turn(ProviderTurnPhase::Working);
        assert_eq!(
            read(&screen, Some(&working), 5).activity,
            ProviderActivity::Active
        );
        assert_eq!(read(&screen, None, 5).activity, ProviderActivity::Resting);
    }

    /// Esc and Ctrl+C fire no hook, measured on 2.1.310. Claude prints its
    /// interrupt line over the prompt instead, and that ends the turn.
    #[test]
    fn an_interrupted_turn_rests_at_once() {
        let screen = claude_at_rest("  ⎿  Interrupted · What should Claude do instead?");
        let working = turn(ProviderTurnPhase::Working);
        assert_eq!(
            read(&screen, Some(&working), 1).activity,
            ProviderActivity::Resting
        );
    }

    /// A Stop that never arrived must not pin a worker busy forever. Claude's
    /// own end-of-turn line over the prompt says the turn finished.
    #[test]
    fn a_finished_turn_whose_stop_was_lost_rests_at_once() {
        let working = turn(ProviderTurnPhase::Working);
        for above in [
            "✻ Cogitated for 1s · done 10:14 PM",
            "✻ Churned for 1m 4s · done 9:02 AM · 1 shell still running",
        ] {
            assert_eq!(
                read(&claude_at_rest(above), Some(&working), 1).activity,
                ProviderActivity::Resting,
                "{above}"
            );
        }
    }

    /// The working spinner carries the same glyph and can say "thought for".
    #[test]
    fn the_working_spinner_is_not_an_end_of_turn_line() {
        let working = turn(ProviderTurnPhase::Working);
        for above in [
            "✻ Simmering… (4s · ↓ 339 tokens · thought for 1s)",
            "✻ Thought for 3s",
        ] {
            assert!(!turn_ended_on_screen(&claude_at_rest(above)), "{above}");
        }
        // And a finished line above a newly echoed prompt belongs to the turn
        // before: the one now open is working.
        let next = claude_at_rest("✻ Baked for 3s · done 10:14 PM\r\n❯ Next task, please.");
        assert!(!turn_ended_on_screen(&next));
        assert_eq!(
            read(&next, Some(&working), 1).activity,
            ProviderActivity::Active
        );
    }

    /// The last bound, for a turn that ended with neither a hook nor the line:
    /// a working Claude redraws every second, so a quiet minute is not work.
    #[test]
    fn an_open_turn_on_a_quiet_resting_screen_rests_after_a_minute() {
        let screen = claude_at_rest("● Still working through the files.");
        let working = turn(ProviderTurnPhase::Working);
        assert_eq!(
            read(&screen, Some(&working), QUIET_TURN_SECONDS - 1).activity,
            ProviderActivity::Active
        );
        assert_eq!(
            read(&screen, Some(&working), QUIET_TURN_SECONDS).activity,
            ProviderActivity::Resting
        );
        // Quiet on a screen that is not resting proves nothing.
        let busy = snapshot("✻ Brewing… (12s · esc to interrupt)\r\n");
        assert_eq!(
            read(&busy, Some(&working), QUIET_TURN_SECONDS * 10).activity,
            ProviderActivity::Active
        );
    }

    #[test]
    fn a_question_the_hooks_report_waits_on_the_operator_unless_the_screen_shows_work() {
        let mut asking = turn(ProviderTurnPhase::Working);
        asking.waiting = Some(ProviderTurnWait::Question);
        let unknown = snapshot("something this build cannot read\r\n");
        assert_eq!(
            read(&unknown, Some(&asking), 1).activity,
            ProviderActivity::AwaitingOperator
        );
        let busy = snapshot("✻ Brewing… (12s · esc to interrupt)\r\n");
        assert_eq!(
            read(&busy, Some(&asking), 1).activity,
            ProviderActivity::Active
        );
        // Esc on the question is an interrupt like any other.
        let interrupted = claude_at_rest("  ⎿  Interrupted · What should Claude do instead?");
        assert_eq!(
            read(&interrupted, Some(&asking), 1).activity,
            ProviderActivity::Resting
        );
    }

    /// An ended turn adds nothing to the screen: a queued message starts its
    /// turn with no `UserPromptSubmit`, and `/compact` is not a turn at all.
    #[test]
    fn an_ended_turn_leaves_the_screen_in_charge() {
        let ended = turn(ProviderTurnPhase::Ended);
        let busy = snapshot("✻ Brewing… (12s · esc to interrupt)\r\n");
        assert_eq!(
            read(&busy, Some(&ended), 1).activity,
            ProviderActivity::Active
        );
        let unknown = snapshot("something this build cannot read\r\n");
        assert_eq!(
            read(&unknown, Some(&ended), 1).activity,
            ProviderActivity::Unknown
        );
        let resting = claude_at_rest("● Done.");
        assert_eq!(
            read(&resting, Some(&ended), 1).activity,
            ProviderActivity::Resting
        );
    }

    #[test]
    fn a_failed_turn_is_reported_and_a_usage_limit_claude_will_not_resume_waits_on_someone() {
        let mut failed = turn(ProviderTurnPhase::Failed);
        failed.failure = Some(swarm_terminal::ProviderTurnFailure::RateLimit);
        let resting = claude_at_rest("● API Error: Rate limit reached");
        let seen = read(&resting, Some(&failed), 1);
        assert_eq!(seen.activity, ProviderActivity::Resting);
        assert_eq!(
            seen.turn_failure,
            Some(swarm_terminal::ProviderTurnFailure::RateLimit)
        );
        failed.waiting = Some(ProviderTurnWait::UsageLimit);
        assert_eq!(
            read(&resting, Some(&failed), 1).activity,
            ProviderActivity::AwaitingOperator
        );
        // A failure from an earlier turn is not reported once a turn ends well.
        let mut ended = turn(ProviderTurnPhase::Ended);
        ended.failure = Some(swarm_terminal::ProviderTurnFailure::RateLimit);
        assert_eq!(read(&resting, Some(&ended), 1).turn_failure, None);
    }

    /// The operator's reminder, 2026-10-23: a worker with shells running reads
    /// "Buzzing · N shells". Claude's own list counts them once the turn ends;
    /// any other turn falls back to the footer.
    #[test]
    fn shells_come_from_claude_s_list_after_a_finished_turn_and_from_the_screen_otherwise() {
        let footer = claude_at_rest("● Started the build.\r\n  2 shells still running");
        let mut ended = turn(ProviderTurnPhase::Ended);
        ended.background = Some(swarm_terminal::ProviderTurnBackground {
            shells: 3,
            others: 1,
            ..swarm_terminal::ProviderTurnBackground::default()
        });
        let seen = read(&footer, Some(&ended), 1);
        assert_eq!(seen.background_work.shells, 3);
        assert_eq!(seen.background_work.others, 1);
        // A finished turn that left nothing running says so, footer or not.
        ended.background = Some(swarm_terminal::ProviderTurnBackground::default());
        assert!(!read(&footer, Some(&ended), 1).background_work.is_running());
        for phase in [ProviderTurnPhase::Working, ProviderTurnPhase::Failed] {
            let mut other = turn(phase);
            other.background = Some(swarm_terminal::ProviderTurnBackground::default());
            assert_eq!(
                read(&footer, Some(&other), 1).background_work.shells,
                2,
                "{phase:?}"
            );
        }
        // No list (Claude could not reach its registry): the footer.
        ended.background = None;
        assert_eq!(read(&footer, Some(&ended), 1).background_work.shells, 2);
    }

    #[test]
    fn a_turn_report_is_ignored_for_providers_that_do_not_send_one() {
        let working = turn(ProviderTurnPhase::Working);
        let screen = snapshot("› \r\n  gpt-5.6 medium · ~/projects");
        let alone = classify_observed_activity(ProviderKind::Codex, &screen);
        assert_eq!(
            observed_signals(ProviderKind::Codex, &screen, Some(&working), None, 0).activity,
            alone
        );
    }

    /// The premise behind the stale-work false positives, measured rather than
    /// inferred.
    ///
    /// Queen flagged three workers as stale while each was waiting on a build
    /// or a measurement it had started, and reasoned that they must be
    /// classifying as Resting. They could equally have been Unknown, or absent
    /// from the activity map — which would have made the screen classifier a
    /// red herring and the real defect somewhere else entirely. This pins it:
    /// the screen a waiting worker actually shows classifies as Resting, and
    /// the background signal beside it is true at the same moment.
    #[test]
    fn a_prompt_resting_beside_a_running_build_reports_both_resting_and_background_work() {
        let snapshot = snapshot(
            "● Re-triggered Build Android AAB and I am watching the run.\r\n\r\n❯ \r\n  ? for shortcuts                              2 shells still running",
        );

        assert_eq!(
            classify_observed_activity(ProviderKind::ClaudeCode, &snapshot),
            ProviderActivity::Resting,
            "the turn HAS ended, so Resting is correct -- this is the premise the stale flag then misreads"
        );
        assert!(
            swarm_terminal::background_work(ProviderKind::ClaudeCode, &snapshot).is_running(),
            "and the signal that tells this apart from an idle worker is true at the same instant"
        );
    }

    /// The counter-case, without which the one above proves only that the
    /// marker matches a string.
    #[test]
    fn a_prompt_resting_with_nothing_running_reports_no_background_work() {
        let snapshot = snapshot("● Done.\r\n\r\n❯ \r\n  ? for shortcuts");

        assert_eq!(
            classify_observed_activity(ProviderKind::ClaudeCode, &snapshot),
            ProviderActivity::Resting
        );
        assert!(
            !swarm_terminal::background_work(ProviderKind::ClaudeCode, &snapshot).is_running(),
            "a genuinely idle worker must still read as having nothing running, or the stale flag goes blind"
        );
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
    fn multiline_composer_input_is_not_an_empty_delivery_target() {
        let draft = snapshot(
            "● Done.\r\n\x1b[38;5;244m────────────────────\x1b[39m\r\n❯ \r\n  private second line\r\n\x1b[38;5;244m────────────────────\x1b[39m\r\n⏵⏵ auto mode on",
        );
        assert!(has_open_provider_input(ProviderKind::ClaudeCode, &draft));
    }

    #[test]
    fn multiline_suggestions_and_bright_footer_do_not_hold_an_empty_composer() {
        let suggestion = snapshot(
            "● Done.\r\n❯ \x1b[2mstart the work\r\n  suggested second line\x1b[22m\r\n\x1b[38;5;244m────────────────────\x1b[39m\r\n⏵⏵ auto mode on",
        );
        assert!(!has_open_provider_input(
            ProviderKind::ClaudeCode,
            &suggestion
        ));
        let mixed = snapshot(
            "● Done.\r\n❯ \x1b[2msuggested first row\x1b[22m\r\n  actual typed second row\r\n\x1b[38;5;244m────────────────────\x1b[39m\r\n⏵⏵ auto mode on",
        );
        assert!(has_open_provider_input(ProviderKind::ClaudeCode, &mixed));
    }

    #[test]
    fn wrapped_input_without_a_box_is_checked_past_the_prompt_row() {
        let draft = snapshot(&format!(
            "❯ {}private wrapped input\r\n? for shortcuts",
            " ".repeat(99)
        ));
        assert!(has_open_provider_input(ProviderKind::ClaudeCode, &draft));
    }

    #[test]
    fn a_typed_rule_inside_the_composer_is_not_its_footer() {
        let draft = snapshot(
            "❯ \r\n────────────────────\r\n  private text below typed rule\r\n\x1b[38;5;244m────────────────────\x1b[39m\r\nfooter",
        );
        assert!(has_open_provider_input(ProviderKind::ClaudeCode, &draft));
    }

    #[test]
    fn an_empty_composer_is_still_empty() {
        let empty = snapshot("\u{276f}\r\n\u{23f5}\u{23f5} auto mode on");
        assert!(!has_open_provider_input(ProviderKind::ClaudeCode, &empty));
    }
}
