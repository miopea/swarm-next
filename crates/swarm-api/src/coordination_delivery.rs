//! Writing one coordination message into a provider terminal, and deciding
//! whether it actually landed.
//!
//! This is the hardest contract in the product to get right, because the only
//! evidence available is what the provider chose to draw. A message is written,
//! its render is waited for, and Enter is sent separately — a provider TUI
//! distinguishes a paste from a keypress, and collapses a long paste into a
//! numbered chip that hides the message entirely. Every state here exists
//! because one of those behaviours was observed.
//!
//! It fails closed. When the delivery cannot be confirmed it reports
//! uncertainty rather than writing again, because a replayed briefing is worse
//! than one the operator is told about.

use std::time::{Duration, Instant};

use swarm_domain::{ProviderKind, WorkerSessionId};
use swarm_persistence::{
    DecisionDispatch, QueenAutomationDelivery, TaskDispatch, TaskOutcomeDispatch, TaskStore,
};
use swarm_terminal::{HostRequest, HostResponse, ProviderActivity, snapshot_plain_text};
use tokio::time::sleep;

use crate::{
    AppState, HostClient, TerminalWriteProvenance, provider_activity, task_store, unix_timestamp,
};

#[derive(Debug, Eq, PartialEq)]
pub(super) enum TerminalSubmission {
    Acknowledged,
    Deferred,
    Rejected { code: String, message: String },
    Uncertain,
}

enum SubmissionObservation {
    Accepted,
    RetryAfter(u64),
    Uncertain,
}

enum MarkerObservation {
    Rendered {
        sequence: u64,
        paste_placeholder: Option<Vec<u8>>,
    },
    Rejected {
        code: String,
        message: String,
    },
    Uncertain,
}

#[derive(Debug, Default)]
struct SequenceStability {
    sequence: Option<u64>,
    unchanged_since: Option<Instant>,
}

impl SequenceStability {
    /// Whether the terminal has stopped producing output for `required`.
    ///
    /// Used after submission, where the question genuinely is whether the
    /// provider settled.
    fn observe(&mut self, sequence: u64, now: Instant, required: Duration) -> bool {
        if self.sequence != Some(sequence) {
            self.sequence = Some(sequence);
            self.unchanged_since = Some(now);
            return false;
        }
        self.unchanged_since
            .is_some_and(|unchanged_since| now.duration_since(unchanged_since) >= required)
    }

    fn reset(&mut self) {
        self.sequence = None;
        self.unchanged_since = None;
    }
}

/// How long the delivery must be continuously observable before it is accepted.
///
/// This used to require the terminal's whole output sequence to stop advancing,
/// which is a different claim: it asks the provider to be idle. A provider that
/// is thinking streams output the entire time, so the sequence never settled,
/// the delivery was never confirmed, and the message sat unsent in the prompt —
/// which is exactly when automation delivers, and exactly what the operator
/// kept finding.
///
/// What matters is that the written prompt is still there, not that nothing
/// else moved. So this measures how long the delivery itself has been visible.
#[derive(Debug, Default)]
struct RenderStability {
    visible_since: Option<Instant>,
}

impl RenderStability {
    fn observe(&mut self, now: Instant, required: Duration) -> bool {
        let since = *self.visible_since.get_or_insert(now);
        now.duration_since(since) >= required
    }

    fn reset(&mut self) {
        self.visible_since = None;
    }
}

/// Submits a coordination prompt after observing it in host-owned output.
///
/// Claude's interactive input can render a carriage return that arrives in the
/// same PTY read as a long prompt without accepting the prompt. This waits for
/// the host's output sequence to advance and confirms a bounded delivery marker
/// in the canonical snapshot before sending Enter. Ordering therefore depends
/// on observed terminal state, never an arbitrary delay.
pub(super) async fn submit_coordination_message(
    store: &TaskStore,
    client: &HostClient,
    session_id: WorkerSessionId,
    bytes: Vec<u8>,
    marker: &[u8],
) -> Result<TerminalSubmission, swarm_terminal::IpcError> {
    let provider = match store.provider_for_active_session(session_id) {
        Ok(provider) => provider,
        Err(error) => {
            return Ok(TerminalSubmission::Rejected {
                code: "provider_identity_unavailable".into(),
                message: error.to_string(),
            });
        }
    };
    submit_terminal_message(client, session_id, provider, bytes, marker).await
}

/// What a read of the terminal says about writing to it now.
enum Baseline {
    Ready {
        sequence: u64,
        paste_placeholder: Option<Vec<u8>>,
    },
    Refused(TerminalSubmission),
}

/// Reads the terminal and decides whether coordination may write to it.
///
/// Coordination owns only a truly empty resting prompt. Active turns, provider
/// questions, unknown screens, and resting prompts carrying typed text all
/// defer durably rather than appending a message beneath whatever is there.
///
/// The paste placeholder already on screen is captured here so a later one can
/// be told apart from it, and is read from the replayed screen for the same
/// reason the later comparison is: the phrase is drawn with cursor moves and
/// does not exist in the stream as text.
async fn delivery_baseline(
    client: &HostClient,
    session_id: WorkerSessionId,
    provider: ProviderKind,
) -> Result<Baseline, swarm_terminal::IpcError> {
    Ok(
        match client
            .request(&HostRequest::Read {
                session_id,
                after_sequence: None,
            })
            .await?
        {
            HostResponse::Output {
                resume: swarm_terminal::Resume::Snapshot { snapshot },
                running: true,
                ..
            } => {
                let activity = provider_activity::classify_observed_activity(provider, &snapshot);
                if activity != ProviderActivity::Resting
                    || provider_activity::has_open_provider_input(provider, &snapshot)
                {
                    return Ok(Baseline::Refused(TerminalSubmission::Deferred));
                }
                Baseline::Ready {
                    sequence: snapshot.sequence,
                    paste_placeholder: latest_claude_paste_placeholder(
                        snapshot_plain_text(&snapshot.bytes, snapshot.rows, snapshot.columns)
                            .as_bytes(),
                    )
                    .map(<[u8]>::to_vec),
                }
            }
            HostResponse::Output { resume, .. } => Baseline::Ready {
                sequence: resume_sequence(&resume),
                paste_placeholder: None,
            },
            HostResponse::Error { code, message } => {
                Baseline::Refused(TerminalSubmission::Rejected { code, message })
            }
            _ => Baseline::Refused(TerminalSubmission::Uncertain),
        },
    )
}

async fn submit_terminal_message(
    client: &HostClient,
    session_id: WorkerSessionId,
    provider: ProviderKind,
    mut bytes: Vec<u8>,
    marker: &[u8],
) -> Result<TerminalSubmission, swarm_terminal::IpcError> {
    let submit = bytes.last() == Some(&b'\r');
    if submit {
        bytes.pop();
    }
    let (baseline, baseline_paste_placeholder) =
        match delivery_baseline(client, session_id, provider).await? {
            Baseline::Ready {
                sequence,
                paste_placeholder,
            } => (sequence, paste_placeholder),
            Baseline::Refused(outcome) => return Ok(outcome),
        };
    let response = client
        .request(&HostRequest::Write {
            session_id,
            bytes,
            provenance: TerminalWriteProvenance::coordination(),
        })
        .await?;
    match response {
        HostResponse::Acknowledged if submit => {}
        HostResponse::Acknowledged => return Ok(TerminalSubmission::Acknowledged),
        HostResponse::Error { code, message } => {
            return Ok(TerminalSubmission::Rejected { code, message });
        }
        _ => return Ok(TerminalSubmission::Uncertain),
    }
    // Claude can redraw a long paste dozens of times before the bounded marker
    // reaches the canonical screen. Observe elapsed time rather than frame
    // count, then require a short stable render window before sending Enter.
    let (rendered_sequence, rendered_paste_placeholder) = match observe_stable_marker(
        client,
        session_id,
        provider,
        marker,
        baseline,
        baseline_paste_placeholder.as_deref(),
    )
    .await?
    {
        MarkerObservation::Rendered {
            sequence,
            paste_placeholder,
        } => (sequence, paste_placeholder),
        MarkerObservation::Rejected { code, message } => {
            return Ok(TerminalSubmission::Rejected { code, message });
        }
        MarkerObservation::Uncertain => return Ok(TerminalSubmission::Uncertain),
    };
    // Claude may acknowledge Enter while it is still finalizing a long
    // bracketed-paste placeholder. Wait for an actually unchanged resting
    // render before retrying; a host acknowledgement alone is not proof that
    // the provider accepted the prompt. Three bounded Enter attempts cover the
    // observed placeholder behavior without creating an unowned retry loop.
    let mut observed_sequence = rendered_sequence;
    for attempt in 0..3 {
        let submit_response = client
            .request(&HostRequest::Write {
                session_id,
                bytes: vec![b'\r'],
                provenance: TerminalWriteProvenance::coordination(),
            })
            .await;
        if !matches!(submit_response, Ok(HostResponse::Acknowledged)) {
            return Ok(TerminalSubmission::Uncertain);
        }

        match observe_terminal_submission(
            client,
            session_id,
            provider,
            marker,
            rendered_paste_placeholder.as_deref(),
            observed_sequence,
        )
        .await?
        {
            SubmissionObservation::Accepted => return Ok(TerminalSubmission::Acknowledged),
            SubmissionObservation::RetryAfter(sequence) => observed_sequence = sequence,
            SubmissionObservation::Uncertain => return Ok(TerminalSubmission::Uncertain),
        }
        if attempt == 2 {
            return Ok(TerminalSubmission::Uncertain);
        }
    }
    Ok(TerminalSubmission::Uncertain)
}

async fn observe_stable_marker(
    client: &HostClient,
    session_id: WorkerSessionId,
    provider: ProviderKind,
    marker: &[u8],
    baseline: u64,
    baseline_paste_placeholder: Option<&[u8]>,
) -> Result<MarkerObservation, swarm_terminal::IpcError> {
    let render_deadline = Instant::now() + Duration::from_secs(10);
    let mut rendered_stability = RenderStability::default();
    loop {
        sleep(Duration::from_millis(50)).await;
        let snapshot = match client
            .request(&HostRequest::Read {
                session_id,
                after_sequence: None,
            })
            .await?
        {
            HostResponse::Output {
                resume: swarm_terminal::Resume::Snapshot { snapshot },
                running: true,
                ..
            } => snapshot,
            HostResponse::Error { code, message } => {
                return Ok(MarkerObservation::Rejected { code, message });
            }
            _ => return Ok(MarkerObservation::Uncertain),
        };
        let visible = snapshot_plain_text(&snapshot.bytes, snapshot.rows, snapshot.columns);
        let marker_is_visible = snapshot.sequence > baseline
            && visible
                .as_bytes()
                .windows(marker.len())
                .any(|part| part == marker);
        // Claude deliberately collapses a long paste into a numbered
        // `[Pasted text #N]` chip, hiding the delivery marker from the
        // canonical screen. A newly numbered, stable chip is equally strong
        // proof that this exact PTY write finished rendering. Comparing it to
        // the baseline prevents an older, operator-owned paste from being
        // mistaken for the current coordination message.
        let new_claude_paste = (provider == ProviderKind::ClaudeCode
            && snapshot.sequence > baseline)
            .then(|| latest_claude_paste_placeholder(visible.as_bytes()))
            .flatten()
            .filter(|placeholder| Some(*placeholder) != baseline_paste_placeholder);
        let new_claude_paste_is_visible = new_claude_paste.is_some();
        if (marker_is_visible || new_claude_paste_is_visible)
            && rendered_stability.observe(Instant::now(), Duration::from_millis(750))
        {
            return Ok(MarkerObservation::Rendered {
                sequence: snapshot.sequence,
                paste_placeholder: new_claude_paste.map(<[u8]>::to_vec),
            });
        }
        if !marker_is_visible && !new_claude_paste_is_visible {
            rendered_stability.reset();
        }
        if Instant::now() >= render_deadline {
            // A delivery that gives up here leaves its message sitting unsent in
            // the operator's prompt, and the reason has so far only been
            // reconstructed afterwards from terminal history. Say what was
            // actually observed. Content-free: sequences and whether the marker
            // was found, never what the terminal was showing.
            tracing::warn!(
                baseline_sequence = baseline,
                observed_sequence = snapshot.sequence,
                marker_is_visible,
                new_claude_paste_is_visible,
                had_baseline_paste = baseline_paste_placeholder.is_some(),
                snapshot_bytes = snapshot.bytes.len(),
                "coordination message render was not confirmed before the deadline"
            );
            return Ok(MarkerObservation::Uncertain);
        }
    }
}

fn latest_claude_paste_placeholder(snapshot: &[u8]) -> Option<&[u8]> {
    const PREFIX: &[u8] = b"[Pasted text #";
    let start = snapshot
        .windows(PREFIX.len())
        .rposition(|part| part == PREFIX)?;
    let suffix = &snapshot[start..];
    let end = suffix.iter().position(|byte| *byte == b']')?;
    Some(&suffix[..=end])
}

async fn observe_terminal_submission(
    client: &HostClient,
    session_id: WorkerSessionId,
    provider: ProviderKind,
    marker: &[u8],
    submitted_paste_placeholder: Option<&[u8]>,
    observed_sequence: u64,
) -> Result<SubmissionObservation, swarm_terminal::IpcError> {
    let acceptance_deadline = Instant::now() + Duration::from_secs(10);
    let mut resting_stability = SequenceStability::default();
    loop {
        sleep(Duration::from_millis(50)).await;
        let HostResponse::Output {
            resume: swarm_terminal::Resume::Snapshot { snapshot },
            running: true,
            ..
        } = client
            .request(&HostRequest::Read {
                session_id,
                after_sequence: None,
            })
            .await?
        else {
            return Ok(SubmissionObservation::Uncertain);
        };
        let activity = provider_activity::classify_observed_activity(provider, &snapshot);
        if activity == ProviderActivity::Active {
            return Ok(SubmissionObservation::Accepted);
        }
        let visible = snapshot_plain_text(&snapshot.bytes, snapshot.rows, snapshot.columns);
        let submitted_paste_is_still_open = submitted_claude_paste_is_still_open(
            provider,
            visible.as_bytes(),
            submitted_paste_placeholder,
        );
        let visible_marker_is_still_input = activity != ProviderActivity::Unknown
            && claude_input_marker_is_still_open(provider, visible.as_bytes(), marker);
        if submitted_paste_is_still_open || visible_marker_is_still_input {
            if resting_stability.observe(
                snapshot.sequence,
                Instant::now(),
                Duration::from_millis(1_500),
            ) {
                return Ok(SubmissionObservation::RetryAfter(snapshot.sequence));
            }
            continue;
        }
        match activity {
            ProviderActivity::Active => unreachable!("active submissions return above"),
            ProviderActivity::AwaitingOperator => {
                return Ok(SubmissionObservation::Accepted);
            }
            ProviderActivity::Unknown if snapshot.sequence > observed_sequence => {
                return Ok(SubmissionObservation::Accepted);
            }
            ProviderActivity::Resting
                if snapshot.sequence > observed_sequence
                    && resting_prompt_follows_marker(visible.as_bytes(), marker) =>
            {
                return Ok(SubmissionObservation::Accepted);
            }
            ProviderActivity::Resting => {
                if resting_stability.observe(
                    snapshot.sequence,
                    Instant::now(),
                    Duration::from_millis(1_500),
                ) {
                    return Ok(SubmissionObservation::RetryAfter(snapshot.sequence));
                }
            }
            ProviderActivity::Unknown => resting_stability.reset(),
        }
        if Instant::now() >= acceptance_deadline {
            return Ok(SubmissionObservation::Uncertain);
        }
    }
}

fn submitted_claude_paste_is_still_open(
    provider: ProviderKind,
    snapshot: &[u8],
    submitted_paste_placeholder: Option<&[u8]>,
) -> bool {
    provider == ProviderKind::ClaudeCode
        && submitted_paste_placeholder.is_some()
        && latest_claude_paste_placeholder(snapshot) == submitted_paste_placeholder
}

fn claude_input_marker_is_still_open(
    provider: ProviderKind,
    snapshot: &[u8],
    marker: &[u8],
) -> bool {
    provider == ProviderKind::ClaudeCode
        && snapshot.windows(marker.len()).any(|part| part == marker)
        && !resting_prompt_follows_marker(snapshot, marker)
}

fn resume_sequence(resume: &swarm_terminal::Resume) -> u64 {
    match resume {
        swarm_terminal::Resume::Snapshot { snapshot } => snapshot.sequence,
        swarm_terminal::Resume::Deltas { frames } => {
            frames.last().map_or(0, |frame| frame.sequence)
        }
    }
}

fn resting_prompt_follows_marker(snapshot: &[u8], marker: &[u8]) -> bool {
    let Some(marker_position) = snapshot
        .windows(marker.len())
        .rposition(|part| part == marker)
    else {
        return false;
    };
    ["❯".as_bytes(), "›".as_bytes()]
        .into_iter()
        .filter_map(|prompt| {
            snapshot
                .windows(prompt.len())
                .rposition(|part| part == prompt)
        })
        .max()
        .is_some_and(|prompt_position| prompt_position > marker_position)
}

pub(super) fn delivery_marker(id: impl std::fmt::Display) -> Vec<u8> {
    id.to_string().bytes().take(8).collect()
}

pub(super) fn decision_delivery_message(delivery: &DecisionDispatch) -> Vec<u8> {
    let action = terminal_safe_text(&delivery.action);
    let note = if delivery.note.is_empty() {
        "No additional note.".into()
    } else {
        terminal_safe_text(&delivery.note)
    };
    // An interview's substance is in the answers, so they are stated here
    // rather than left behind a tool call. A worker that has been holding its
    // session should not have to go and fetch what it was waiting for.
    let outcome = if delivery.answers.is_empty() {
        format!("Action: {action}.")
    } else {
        let answers = delivery
            .answers
            .iter()
            .map(|(header, given)| {
                format!(
                    "{}: {}",
                    terminal_safe_text(header),
                    terminal_safe_text(&given.join(", "))
                )
            })
            .collect::<Vec<_>>()
            .join(" | ");
        format!("Answers: {answers}.")
    };
    format!(
        "[Swarm decision {} resolved] {} Operator note: {} Use swarm_list_decisions for the full request context.\r",
        delivery.decision_id, outcome, note,
    )
    .into_bytes()
}

pub(super) fn task_dispatch_message(delivery: &TaskDispatch) -> Vec<u8> {
    let title = terminal_safe_text(&delivery.title);
    // The operator's instruction governs how the work is approached, so it is
    // stated before the work rather than left to be discovered in the record.
    let instruction = terminal_safe_text(&delivery.operator_instruction);
    let instruction = if instruction.trim().is_empty() {
        String::new()
    } else {
        format!(" Operator instruction for this task: {instruction}.")
    };
    format!(
        "[Swarm task {} assigned] {}.{} Call swarm_list_tasks now and work from its authoritative task details and linked evidence. If this task is not visible, stop; its assignment changed.\r",
        delivery.task_id, title, instruction,
    )
    .into_bytes()
}
/// Settles an uncertain Queen review by reading the terminal it was written to.
///
/// Uncertain means Swarm could not confirm the review reached Queen, not that
/// it failed. The prompt carries the run id, so finding it in that exact
/// terminal answers the question: Queen has it and can finish the run herself.
///
/// Only ever resolves uncertainty in the direction of "it landed". A marker
/// that is absent proves nothing — it may have scrolled out of the window —
/// and replaying a review that did land would double it, which is the failure
/// the uncertain state exists to prevent.
pub(super) async fn settle_uncertain_queen_review(state: &AppState) {
    let Ok(store) = task_store(state) else {
        return;
    };
    let Ok(Some((run_id, session_id))) = store.uncertain_queen_delivery() else {
        return;
    };
    let Some(client) = &state.terminal_host else {
        return;
    };
    let Ok(HostResponse::Output {
        resume: swarm_terminal::Resume::Snapshot { snapshot },
        ..
    }) = client
        .request(&HostRequest::Read {
            session_id,
            after_sequence: None,
        })
        .await
    else {
        return;
    };
    let marker = format!("[Swarm automation {run_id}]");
    let visible = snapshot_plain_text(&snapshot.bytes, snapshot.rows, snapshot.columns);
    if !visible.contains(&marker) {
        return;
    }
    match store.confirm_queen_automation_delivered(&run_id, session_id, unix_timestamp()) {
        Ok(true) => {
            state.control_room_notify.notify_waiters();
            tracing::info!(run_id = %run_id, "uncertain Queen review was found in its terminal and resumed");
        }
        Ok(false) => {}
        Err(error) => {
            tracing::warn!(run_id = %run_id, message = %error, "uncertain Queen review could not be settled");
        }
    }
}

pub(super) fn queen_automation_message(delivery: &QueenAutomationDelivery) -> Vec<u8> {
    format!(
        "[Swarm automation {}] Review {} actionable records while the operator is {}. Use swarm_list_tasks, swarm_list_workers, and swarm_list_coordination_attention as the authority. Coordination attention can identify Ready work whose delivered brief did not start, Active work that is unchanged while its loaded worker is resting, or work whose worker process exited; recheck the current task and worker before deciding whether to restart, steer, wait, or ask the operator. Respect worker repository ownership and the configured Queen autonomy ceiling. Do not perform Jira, Apiary, email, deployment, or other external side effects during this run. When operator judgment is needed, create one swarm_request_decision per concrete task. Link its task_id, make the suggested_action exactly one allowed_actions button, and never group unrelated tasks or a fleet review into one approval. When this exact review is finished, call swarm_finish_automation_run with run_id {} and outcome completed, needs_operator, or no_action.\r",
        delivery.run_id,
        delivery.actionable_count,
        delivery.presence,
        delivery.run_id,
    )
    .into_bytes()
}
pub(super) fn task_outcome_message(outcome: &TaskOutcomeDispatch) -> Vec<u8> {
    let reporter = terminal_safe_text(&outcome.reporting_worker_name);
    let title = terminal_safe_text(&outcome.title);
    let note = if outcome.note.is_empty() {
        "No additional handoff note.".into()
    } else {
        terminal_safe_text(&outcome.note)
    };
    format!(
        "[Swarm worker outcome] {} moved task {} \"{}\" to {}. Handoff: {} Use swarm_list_tasks and task history for authoritative context.\r",
        reporter, outcome.task_id, title, outcome.target_state, note,
    )
    .into_bytes()
}
fn terminal_safe_text(value: &str) -> String {
    value
        .chars()
        .map(|character| {
            if character.is_control() {
                ' '
            } else {
                character
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use swarm_terminal::{CanonicalTerminalState, JournalLimits, TerminalSize, TerminalSnapshot};

    #[test]
    fn a_delivery_is_confirmed_while_the_provider_keeps_working() {
        // Observed 2026-08-20 02:47:38: marker visible, paste chip visible, and
        // the delivery still went unconfirmed — so Enter was withheld and the
        // message sat in the operator's prompt. The old rule waited for the
        // terminal's sequence to stop advancing, which asks the provider to be
        // idle. A provider that is thinking streams output the whole time,
        // which is exactly when automation delivers.
        let start = Instant::now();
        let mut stability = RenderStability::default();

        // Seen for the first time: not yet long enough to trust.
        assert!(!stability.observe(start, Duration::from_millis(750)));
        // Still there a moment later, while output keeps flowing around it.
        assert!(!stability.observe(
            start + Duration::from_millis(500),
            Duration::from_millis(750)
        ));
        assert!(stability.observe(
            start + Duration::from_millis(800),
            Duration::from_millis(750)
        ));
    }

    #[test]
    fn losing_sight_of_a_delivery_starts_the_clock_again() {
        // Reset is what keeps this honest: it measures how long the delivery
        // has been continuously visible, not how long ago it first appeared.
        let start = Instant::now();
        let mut stability = RenderStability::default();
        assert!(!stability.observe(start, Duration::from_millis(750)));

        stability.reset();

        assert!(!stability.observe(
            start + Duration::from_millis(800),
            Duration::from_millis(750)
        ));
        assert!(stability.observe(
            start + Duration::from_millis(1_600),
            Duration::from_millis(750)
        ));
    }

    #[test]
    fn acceptance_still_waits_for_the_terminal_to_settle() {
        // The check after submission asks a genuinely different question —
        // whether the provider came to rest — and keeps measuring the sequence.
        let start = Instant::now();
        let mut stability = SequenceStability::default();

        assert!(!stability.observe(10, start, Duration::from_millis(750)));
        assert!(!stability.observe(
            11,
            start + Duration::from_millis(800),
            Duration::from_millis(750)
        ));
        assert!(stability.observe(
            11,
            start + Duration::from_millis(1_600),
            Duration::from_millis(750)
        ));
    }

    /// Renders a message through the same canonical state a real terminal uses,
    /// at a given width, and reports whether the delivery marker survives.
    fn marker_survives_render(columns: u16, prefix: &str, marker: &str) -> bool {
        let mut state = CanonicalTerminalState::new(
            JournalLimits::new(64 * 1024, 64),
            TerminalSize::new(24, columns),
        );
        state.push(format!("{prefix}{marker} and the rest of the briefing").into_bytes());
        let TerminalSnapshot { bytes, .. } = state.snapshot();
        let marker = marker.as_bytes();
        bytes.windows(marker.len()).any(|part| part == marker)
    }

    #[test]
    fn a_paste_chip_drawn_with_cursor_moves_is_still_recognised() {
        // Claude does not draw the chip with spaces. Captured from a live Queen
        // terminal: it writes "[Pasted", moves the cursor right, writes "text",
        // moves again, then "#1]". The literal bytes "[Pasted text #" never
        // reach the stream, so a search for them cannot match however long it
        // waits — which is why a delivery sat unsubmitted for ten seconds and
        // then reported uncertainty.
        let mut state = CanonicalTerminalState::new(
            JournalLimits::new(64 * 1024, 64),
            TerminalSize::new(24, 80),
        );
        state.push(b"\xe2\x9d\xaf [Pasted\x1b[Ctext\x1b[C#1]".to_vec());
        let TerminalSnapshot {
            bytes,
            rows,
            columns,
            ..
        } = state.snapshot();

        // Searching the stream directly cannot work: the phrase is not in it.
        assert!(latest_claude_paste_placeholder(&bytes).is_none());

        // Replaying it into a screen restores what the operator can read.
        let visible = snapshot_plain_text(&bytes, rows, columns);
        assert!(
            latest_claude_paste_placeholder(visible.as_bytes()).is_some(),
            "the chip is on screen, so it must be recognisable once replayed"
        );
    }

    #[test]
    fn a_delivery_marker_survives_the_terminal_wrapping_it() {
        // Confirmation searches the snapshot for eight bytes taken from the head
        // of an identifier, and the snapshot is a re-rendered screen rather than
        // the raw stream. If that render ever broke a token at the wrap column,
        // or coloured inside one, delivery would stop being confirmable while
        // every byte still reached the screen — and the message would sit unsent
        // in the operator's prompt with nothing to explain it.
        //
        // Written while eliminating that as the cause of a real unconfirmed
        // delivery. It is not the cause, and this keeps it from becoming one.
        let marker = "01a0169f";

        assert!(marker_survives_render(80, "moved task ", marker));
        // Starting at column 37 of a 40 column terminal, so it crosses the wrap.
        assert!(marker_survives_render(40, &"x".repeat(36), marker));
        // And immediately before the boundary, the other side of the same edge.
        assert!(marker_survives_render(40, &"x".repeat(32), marker));
    }

    #[test]
    fn terminal_render_stability_resets_on_every_output_advance() {
        let started = Instant::now();
        let required = Duration::from_millis(300);
        let mut stability = SequenceStability::default();

        assert!(!stability.observe(10, started, required));
        assert!(!stability.observe(11, started + Duration::from_millis(500), required));
        assert!(!stability.observe(11, started + Duration::from_millis(799), required));
        assert!(stability.observe(11, started + Duration::from_millis(800), required));

        assert!(!stability.observe(12, started + Duration::from_secs(2), required));
        stability.reset();
        assert!(!stability.observe(12, started + Duration::from_secs(3), required));
    }

    #[test]
    fn a_new_resting_prompt_proves_the_submitted_marker_left_input() {
        let marker = b"01ab23cd";
        assert!(resting_prompt_follows_marker(
            b"manual mode\n\xe2\x9d\xaf [Swarm automation 01ab23cd]\nworked\n\xe2\x9d\xaf ",
            marker,
        ));
        assert!(!resting_prompt_follows_marker(
            b"manual mode\n\xe2\x9d\xaf [Swarm automation 01ab23cd]",
            marker,
        ));
        assert!(!resting_prompt_follows_marker(
            b"manual mode\n\xe2\x9d\xaf [Pasted text #1]",
            marker,
        ));
    }

    #[test]
    fn claude_paste_placeholder_identity_distinguishes_a_new_rendered_paste() {
        assert_eq!(
            latest_claude_paste_placeholder(b"manual mode\n\xe2\x9d\xaf [Pasted text #4]"),
            Some(&b"[Pasted text #4]"[..])
        );
        assert_eq!(
            latest_claude_paste_placeholder(b"old [Pasted text #3]\nnew prompt [Pasted text #4]"),
            Some(&b"[Pasted text #4]"[..])
        );
        assert_eq!(latest_claude_paste_placeholder(b"ordinary prompt"), None);
        assert_eq!(latest_claude_paste_placeholder(b"[Pasted text #4"), None);
    }

    #[test]
    fn exact_submitted_claude_paste_must_leave_input_before_acknowledgement() {
        let submitted = Some(&b"[Pasted text #4]"[..]);
        assert!(submitted_claude_paste_is_still_open(
            ProviderKind::ClaudeCode,
            b"manual mode\n\xe2\x9d\xaf [Pasted text #4]",
            submitted,
        ));
        assert!(!submitted_claude_paste_is_still_open(
            ProviderKind::ClaudeCode,
            b"worked\n\xe2\x9d\xaf ",
            submitted,
        ));
        assert!(!submitted_claude_paste_is_still_open(
            ProviderKind::ClaudeCode,
            b"manual mode\n\xe2\x9d\xaf [Pasted text #5]",
            submitted,
        ));
        assert!(!submitted_claude_paste_is_still_open(
            ProviderKind::Codex,
            b"[Pasted text #4]",
            submitted,
        ));
    }

    #[test]
    fn visible_claude_marker_must_leave_the_current_input_before_acknowledgement() {
        let marker = b"01ab23cd";
        assert!(claude_input_marker_is_still_open(
            ProviderKind::ClaudeCode,
            b"manual mode\n\xe2\x9d\xaf [Swarm task 01ab23cd]",
            marker,
        ));
        assert!(!claude_input_marker_is_still_open(
            ProviderKind::ClaudeCode,
            b"\xe2\x9d\xaf [Swarm task 01ab23cd]\nworked\n\xe2\x9d\xaf ",
            marker,
        ));
    }
}
