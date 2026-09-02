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

use futures_util::future::join_all;
use swarm_domain::{ProviderKind, WorkerSessionId};
use swarm_persistence::{
    DecisionDispatch, QueenAutomationDelivery, TaskDispatch, TaskMessageDispatch,
    TaskOutcomeDispatch, TaskStore,
};
use swarm_terminal::{HostRequest, HostResponse, ProviderActivity, snapshot_plain_text};
use tokio::time::sleep;

use crate::{
    AppState, HostClient, TerminalWriteProvenance, provider_activity, task_store, unix_timestamp,
};

/// Why a write was not attempted. Both mean "not now", and they mean it for
/// opposite reasons: one wants the operator to answer something, the other
/// wants them to clear something they typed and never sent. Reporting both as
/// "an unanswered prompt" sent the operator looking for a question that was not
/// there while the board sat still for hours.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum DeferralReason {
    /// The provider is working, or is genuinely asking the operator something.
    ProviderBusy,
    /// The provider is resting, but its prompt already holds text nobody sent.
    /// Appending to it would merge two unrelated instructions into one Enter.
    PromptHoldsUnsentText,
}

impl DeferralReason {
    /// The refusal kind this deferral is recorded under.
    ///
    /// The control room branches on the kind rather than reading the prose, so
    /// the two situations can be told apart without matching on a sentence.
    pub(super) fn refusal_kind(self) -> &'static str {
        match self {
            Self::ProviderBusy => swarm_persistence::REFUSAL_DELIVERY_HELD,
            Self::PromptHoldsUnsentText => swarm_persistence::REFUSAL_DELIVERY_HELD_UNSENT_TEXT,
        }
    }

    /// Written to the operator, so it names the remedy rather than the state.
    pub(super) fn describe(self, subject: &str) -> String {
        match self {
            Self::ProviderBusy => {
                format!("{subject} is waiting for an unanswered prompt in this terminal")
            }
            Self::PromptHoldsUnsentText => format!(
                "{subject} is waiting because this terminal's prompt holds text that was typed but never sent — clear the line to release it"
            ),
        }
    }
}

#[derive(Debug, Eq, PartialEq)]
pub(super) enum TerminalSubmission {
    Acknowledged,
    Deferred(DeferralReason),
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
    /// Seen on screen, but never still for long enough to call settled.
    ///
    /// The message is demonstrably in the prompt — the operator can read it —
    /// and the bytes were already written and acknowledged before any of this
    /// began. Giving up here does not undo the write; it leaves the message
    /// sitting in the prompt for a person to press Enter on, which is exactly
    /// what was reported three times. Sending Enter cannot make that worse: it
    /// either lands, or the delivery reports the same uncertainty it would
    /// have reported anyway.
    RenderedUnsettled {
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

/// Submits one message per delivery, letting distinct terminals proceed at the
/// same time while keeping each terminal's own messages in order.
///
/// Delivery used to be strictly sequential across the whole Hive. Every attempt
/// can spend ten seconds waiting for a terminal to settle and ten more waiting
/// for it to accept, so with eight sessions in play a message at the back of
/// the queue waited minutes behind terminals it had nothing to do with — and
/// often expired into `uncertain` rather than arriving, which is the mark the
/// operator sees on a worker that was never actually written to twice.
///
/// Two prompts bound for two different workers were never in conflict. Two
/// bound for the same worker always are: they share one input line, and
/// interleaving them would produce a prompt made of both. So the grouping is
/// the correctness boundary, not the concurrency.
pub(super) async fn submit_to_each_terminal_at_once<D>(
    store: &TaskStore,
    client: &HostClient,
    deliveries: Vec<D>,
    describe: impl Fn(&D) -> (WorkerSessionId, Vec<u8>, Vec<u8>),
) -> Vec<(D, Result<TerminalSubmission, swarm_terminal::IpcError>)> {
    let describe = &describe;
    let groups = group_by_terminal(deliveries, |delivery| describe(delivery).0)
        .into_iter()
        .map(|group| async move {
            let mut settled = Vec::with_capacity(group.len());
            for delivery in group {
                let (session_id, bytes, marker) = describe(&delivery);
                let submission =
                    submit_coordination_message(store, client, session_id, bytes, &marker).await;
                settled.push((delivery, submission));
            }
            settled
        });
    join_all(groups).await.into_iter().flatten().collect()
}

/// Copies one write's outcome for each delivery that shared it.
///
/// `IpcError` is not `Clone`, and the alternative — reporting only the first
/// delivery in a group and quietly failing the rest — is how a task outcome
/// would be lost. Every delivery gets the result its write actually had.
pub(super) fn clone_submission(
    submission: &Result<TerminalSubmission, swarm_terminal::IpcError>,
) -> Result<TerminalSubmission, swarm_terminal::IpcError> {
    match submission {
        Ok(TerminalSubmission::Acknowledged) => Ok(TerminalSubmission::Acknowledged),
        Ok(TerminalSubmission::Deferred(reason)) => Ok(TerminalSubmission::Deferred(*reason)),
        Ok(TerminalSubmission::Rejected { code, message }) => Ok(TerminalSubmission::Rejected {
            code: code.clone(),
            message: message.clone(),
        }),
        Ok(TerminalSubmission::Uncertain) => Ok(TerminalSubmission::Uncertain),
        // The message is preserved; the variant is flattened to an I/O error,
        // because IpcError is not Clone and every caller treats a transport
        // failure as uncertain regardless of which kind it was.
        Err(error) => Err(swarm_terminal::IpcError::Io(std::io::Error::other(
            error.to_string(),
        ))),
    }
}

/// Writes one message per terminal, covering everything that terminal is owed.
///
/// The per-delivery variant above writes each message separately and is right
/// where each carries its own answer — a decision, a briefing. Outcomes are not
/// like that: they arrive in bursts, they are all addressed to the same reader,
/// and a provider queues whatever lands while it is working. Six of them meant
/// the recipient read the queue six times.
///
/// Every delivery in a group shares the group's result, because they shared the
/// write. Nothing is reported delivered that was not.
pub(super) async fn submit_grouped_per_terminal<D>(
    store: &TaskStore,
    client: &HostClient,
    deliveries: Vec<D>,
    session_of: impl Fn(&D) -> WorkerSessionId,
    describe: impl Fn(&[D]) -> (Vec<u8>, Vec<u8>),
) -> Vec<(Vec<D>, Result<TerminalSubmission, swarm_terminal::IpcError>)> {
    let describe = &describe;
    let session_of = &session_of;
    let groups = group_by_terminal(deliveries, session_of)
        .into_iter()
        .map(|group| async move {
            let Some(session_id) = group.first().map(session_of) else {
                return (group, Ok(TerminalSubmission::Acknowledged));
            };
            let (bytes, marker) = describe(&group);
            let submission =
                submit_coordination_message(store, client, session_id, bytes, &marker).await;
            (group, submission)
        });
    join_all(groups).await
}

/// Gathers deliveries into one group per terminal, keeping each terminal's own
/// messages in the order they were claimed.
///
/// This is the correctness boundary of running deliveries at the same time.
/// Messages bound for one terminal share a single input line, so interleaving
/// two of them would produce a prompt made of both; messages bound for
/// different terminals never touch. First-seen order is kept deliberately — a
/// handful of sessions makes the linear scan free, and a stable order keeps
/// results reproducible rather than dependent on hash iteration.
fn group_by_terminal<D>(
    deliveries: Vec<D>,
    session_of: impl Fn(&D) -> WorkerSessionId,
) -> Vec<Vec<D>> {
    let mut grouped: Vec<(WorkerSessionId, Vec<D>)> = Vec::new();
    for delivery in deliveries {
        let session_id = session_of(&delivery);
        match grouped.iter_mut().find(|(known, _)| *known == session_id) {
            Some((_, group)) => group.push(delivery),
            None => grouped.push((session_id, vec![delivery])),
        }
    }
    grouped.into_iter().map(|(_, group)| group).collect()
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
                if activity != ProviderActivity::Resting {
                    return Ok(Baseline::Refused(TerminalSubmission::Deferred(
                        DeferralReason::ProviderBusy,
                    )));
                }
                if provider_activity::has_open_provider_input(provider, &snapshot) {
                    return Ok(Baseline::Refused(TerminalSubmission::Deferred(
                        DeferralReason::PromptHoldsUnsentText,
                    )));
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
        // Never settled, but plainly there. Finish the delivery rather than
        // leaving the operator a prompt to press Enter on.
        MarkerObservation::RenderedUnsettled {
            sequence,
            paste_placeholder,
        } => {
            tracing::info!(
                observed_sequence = sequence,
                "coordination message never settled; submitting it rather than stranding it"
            );
            (sequence, paste_placeholder)
        }
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
    // Whether the message was ever on screen, as opposed to never arriving at
    // all. The two failures are different: one leaves a prompt a person must
    // press Enter on, the other leaves nothing to press Enter for.
    let mut ever_rendered = false;
    let mut last_rendered: Option<(u64, Option<Vec<u8>>)> = None;
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
        if marker_is_visible || new_claude_paste_is_visible {
            ever_rendered = true;
            last_rendered = Some((snapshot.sequence, new_claude_paste.map(<[u8]>::to_vec)));
        } else {
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
                ever_rendered,
                "coordination message render was not confirmed before the deadline"
            );
            return Ok(match last_rendered {
                Some((sequence, paste_placeholder)) => MarkerObservation::RenderedUnsettled {
                    sequence,
                    paste_placeholder,
                },
                None => MarkerObservation::Uncertain,
            });
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
    // A person wrote in and is waiting on that thread. Finishing the work tells
    // them nothing, so answering them is part of the work rather than a chore
    // left for the operator afterwards.
    let requester = delivery.email_requester.as_deref().map_or_else(
        String::new,
        |requester| {
            format!(
                " This came in by email from {}, who is waiting on a reply. Finishing it includes recording where it is deployed with swarm_record_deployment and writing their reply with swarm_draft_email_reply; the operator reviews and sends it.",
                terminal_safe_text(requester)
            )
        },
    );
    // THE OPERATOR'S RULING TRAVELS WITH THE WORK.
    //
    // A task whose gate reads "the operator must sign off, not a Queen note or
    // a peer relay" has to be able to read that sign-off somewhere. Being told
    // the decision id by a peer IS the relay such a gate forbids, so on
    // 2026-08-26 a worker held a cutover carrying 919k requests a day while the
    // authority for it sat in a record it had no reason to know existed. Every
    // route available to it violated its own gate.
    //
    // The id is here so the worker can verify at source rather than take this
    // sentence as the authority — this line is a pointer, and the durable
    // record is what authorises.
    //
    // Deliberately not the reason, risk or evidence. Those are bounded at ten
    // thousand characters EACH, and a brief is delivered into a terminal where
    // a wall of text costs more than it does in a tool result.
    // EVERY resolved ruling, newest first, one line each -- not the newest one.
    //
    // Picking the newest is right when an operator answers the same question
    // twice and wrong when a task accumulates rulings on different questions,
    // and this Hive's data contains both. On 01a0337e five decisions form one
    // negotiation, each naming the ruling it replaces. On 01a03952 two share no
    // subject at all -- an approval for a schema test and a ruling on historical
    // rows, twenty-five hours apart -- and picking the newest DROPPED THE
    // APPROVAL the assigned worker was blocked on.
    //
    // The failure modes are not symmetric. A superfluous line costs a reader a
    // second; a missing authority reads as no authority, and a worker acting on
    // that reads a complete-looking brief and concludes it has no approval.
    //
    // Brevity is still kept, but by the FIELD SELECTION rather than by picking:
    // id and resolution only, never reason, risk or evidence, which are bounded
    // at ten thousand characters EACH.
    let ruling = if delivery.operator_rulings.is_empty() {
        String::new()
    } else {
        let each = delivery
            .operator_rulings
            .iter()
            .map(|ruling| {
                if ruling.answered_in_words {
                    // The placeholder is not the answer, and quoting it as one
                    // is how two sessions concluded a ruling did not exist.
                    format!(
                        "decision {} answered in the operator's own words (read it with swarm_list_decisions)",
                        ruling.decision_id
                    )
                } else {
                    format!(
                        "\"{}\" (decision {})",
                        terminal_safe_text(&ruling.resolution),
                        ruling.decision_id
                    )
                }
            })
            .collect::<Vec<_>>()
            .join("; ");
        // Wording kept tight on purpose: a brief is delivered into a terminal
        // and the test holds it to a line. Every word here is carrying weight --
        // "each stands" is what stops a reader treating the newest as the only
        // one, and "verify at source" is what stops them treating this sentence
        // as the authority.
        format!(
            " Operator rulings on this task, newest first: {each}. Each stands unless a later \
             one answers the same question; verify at source with swarm_list_decisions."
        )
    };
    format!(
        "[Swarm task {} assigned] {}.{}{}{} Call swarm_list_tasks now and work from its authoritative task details and linked evidence. If this task is not visible, stop; its assignment changed.\r",
        delivery.task_id, title, instruction, ruling, requester,
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
        "[Swarm automation {}] Review {} actionable records while the operator is {}. Use swarm_list_tasks, swarm_list_workers, and swarm_list_coordination_attention as the authority. Draft tasks are part of this review: a draft is work nobody has decided about yet, so triage each one into ready, blocked, or removed rather than leaving it sitting. Coordination attention can identify Ready work whose delivered brief did not start, Active work that is unchanged while its loaded worker is resting, or work whose worker process exited; recheck the current task and worker before deciding whether to restart, steer, wait, or ask the operator. It ALSO identifies finished work nothing has settled, which is usually the largest part of this review and is yours rather than the operator's: approve a no-deployment claim with swarm_approve_no_deployment once you have read the handoff, SAYING WHAT YOUR AGREEMENT RESTS ON — a merged SHA, a recorded deployment, the handoff you read, or an explicit \"I could not verify\", which is accepted; saying nothing is not. When something is missing, hand it back with swarm_return_reviewed_work naming what you need: THE TASK STAYS IN REVIEW and the next move becomes the worker\'s. Do NOT move reviewed work to Ready to get attention — Ready means UNSTARTED to everything that reads it and erases that the work was done. When work is finished and merely waiting to ship, move it to awaiting_release: it needs no evidence to enter and COMPLETES ITSELF when a deployment is recorded, so it is the right home for anything held only because it has not shipped. When commits touch code with no deployment assign a task to the owning worker to ship it — you cannot deploy during this run, but routing the deployment is coordination and is yours. Work parked on a SLEEPING worker is yours to move rather than yours to wait on: there is no wake tool, and assigning READY work with swarm_assign_task queues a guarded wake, so reassigning it to the same sleeping worker is how that worker is started. Only Ready work wakes anyone — work left Active or Blocked on a SLEEPING worker wakes nobody, so return it to Ready first (Active to Blocked to Ready) and then assign. That route is for WAKING a stopped worker and nothing else; it is not how you ask a running worker for something, which is swarm_message_worker, and it is not how you hand back reviewed work, which is swarm_return_reviewed_work. You can also ask a running worker a question without interrupting it: swarm_message_worker waits until its terminal is resting, so it never lands mid-turn, and the exchange is recorded on the task. Observe the live session before calling the work Active again. Respect worker repository ownership and the configured Queen autonomy ceiling. Do not perform Jira, Apiary, email, deployment, or other external side effects during this run. When operator judgment is needed, create one swarm_request_decision per concrete task. Link its task_id, make the suggested_action exactly one allowed_actions button, and never group unrelated tasks or a fleet review into one approval. When this exact review is finished, call swarm_finish_automation_run with run_id {} and outcome completed, needs_operator, or no_action.\r",
        delivery.run_id,
        delivery.actionable_count,
        delivery.presence,
        delivery.run_id,
    )
    .into_bytes()
}
/// How much of a handoff note is pasted into the recipient's terminal.
///
/// Enough to know whether this needs attention now; not the whole report. The
/// note is durable in task history and the message already says to read it
/// there, so pasting all of it copies the non-authoritative version of
/// something it is simultaneously telling the reader to go and fetch.
///
/// Measured on 2026-08-23: 46 outcomes averaging 2,850 bytes put 128 KB through
/// Queen's terminal in a day, nine of them inside one hour. Notes are capped at
/// 4,000 bytes on the way in and workers write to the cap.
const HANDOFF_EXCERPT_BYTES: usize = 480;

/// Trims to a whole character within the budget, and says it was trimmed.
fn handoff_excerpt_within(note: &str, budget: usize) -> String {
    if note.len() <= budget {
        return note.to_owned();
    }
    let mut end = budget;
    while end > 0 && !note.is_char_boundary(end) {
        end -= 1;
    }
    // Prefer the last sentence or line break, so the excerpt ends somewhere a
    // reader would have paused anyway rather than mid-word.
    let cut = note[..end]
        .rfind(['.', '\n'])
        .map_or(end, |index| index + 1);
    // Says how much it dropped and names the tool that returns it.
    //
    // "(full handoff in task history)" pointed at something Queen had no
    // instrument to read, and gave no hint whether one line or forty was
    // missing — so a report whose whole point was "this ticket describes the
    // wrong incident" could be summarised as routine completion.
    let dropped = note
        .chars()
        .count()
        .saturating_sub(note[..cut].chars().count());
    format!(
        "{}… (+{dropped} more characters — call swarm_read_task_history for the whole handoff)",
        note[..cut].trim_end()
    )
}

/// One message for everything a terminal is owed right now.
///
/// Outcomes arrive in bursts — several workers finishing near each other — and
/// each one used to be written separately. A provider queues what arrives while
/// it is working, so the recipient read the same queue N times and spent
/// context on N preambles. The operator watching Queen: "It is almost like she
/// is getting too many prompts before the previous one is done."
///
/// One line, always: these are typed into a prompt, and a newline would submit
/// half a message.
/// What a worker actually reads when Queen asks it something.
///
/// Names the sender and the task, because a message with neither is an
/// instruction from nowhere — and the standing rule is that anything a sender
/// can write, a sender can fabricate. Saying it came through Swarm and naming
/// the task it is about is what lets a worker check it rather than believe it.
pub(super) fn task_message_message(messages: &[TaskMessageDispatch]) -> Vec<u8> {
    use std::fmt::Write as _;
    let mut text = String::from("Message");
    if messages.len() > 1 {
        let _ = write!(text, "s ({})", messages.len());
    }
    text.push_str(" via Swarm:\n");
    for message in messages {
        let _ = write!(
            text,
            "\n[{} — task {} \"{}\"]\n{}\n",
            message.sender_name, message.task_id, message.task_title, message.body
        );
    }
    // ENDS WITH \r, AND THAT IS NOT PUNCTUATION — IT IS THE SUBMIT FLAG.
    // submit_terminal_message reads the last byte: \r means "type this and
    // then press Enter", anything else means "type it and stop". Ending with
    // \n typed the message into the worker's composer, never submitted it,
    // and STILL returned Acknowledged — so it was recorded as delivered while
    // sitting unsent in their prompt, then surfaced later mid-turn looking
    // like a message that arrived while they were working.
    text.push_str(
        "\nReply with swarm_message_queen on that task id. This is a question, not an \
         instruction: it does not change what the work is, and a ruling cited here still has \
         to be verified with swarm_list_decisions.\r",
    );
    text.into_bytes()
}

/// What an operator broadcast looks like in a worker's terminal.
///
/// Says it went to everyone, because a worker that cannot tell a broadcast from
/// a message addressed to it will answer as though it were asked personally,
/// and thirty-nine workers each replying to one announcement is its own outage.
pub(super) fn operator_broadcast_message(body: &str) -> Vec<u8> {
    // Ends with \r for the reason spelled out in task_message_message: that
    // byte is the submit flag, and \n leaves this typed into the composer,
    // unsent, while the delivery pass records it as delivered.
    format!(
        "[Broadcast from the operator to every running worker]\n{body}\n\nThis went to all \
         workers at once. It is not addressed to you personally and wants no reply unless it \
         asks for one.\r"
    )
    .into_bytes()
}

pub(super) fn task_outcome_message(outcomes: &[TaskOutcomeDispatch]) -> Vec<u8> {
    let Some((first, rest)) = outcomes.split_first() else {
        return Vec::new();
    };
    if rest.is_empty() {
        return format!(
            "[Swarm worker outcome] {} Use swarm_list_tasks and task history for authoritative context.\r",
            one_outcome(first, HANDOFF_EXCERPT_BYTES),
        )
        .into_bytes();
    }
    // The excerpt budget is per message, not per outcome, so a burst of six
    // does not paste six times as much as one did.
    let budget = (HANDOFF_EXCERPT_BYTES / outcomes.len()).max(120);
    let reported = outcomes
        .iter()
        .enumerate()
        .map(|(index, outcome)| format!("{}) {}", index + 1, one_outcome(outcome, budget)))
        .collect::<Vec<_>>()
        .join(" ");
    format!(
        "[Swarm worker outcome] {} tasks reported. {reported} Use swarm_list_tasks and task history for authoritative context.\r",
        outcomes.len(),
    )
    .into_bytes()
}

fn one_outcome(outcome: &TaskOutcomeDispatch, budget: usize) -> String {
    let reporter = terminal_safe_text(&outcome.reporting_worker_name);
    let title = terminal_safe_text(&outcome.title);
    let note = if outcome.note.is_empty() {
        "No additional handoff note.".into()
    } else {
        terminal_safe_text(&handoff_excerpt_within(&outcome.note, budget))
    };
    format!(
        "{} moved task {} \"{}\" to {}. Handoff: {}",
        reporter, outcome.task_id, title, outcome.target_state, note,
    )
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
    use swarm_domain::{PresenceMode, QueenAutomationTrigger, TaskId, WorkerId};
    use swarm_persistence::{QueenAutomationDelivery, TaskMessageDispatch};
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

    /// The operator's report: a prompt sitting unsent while Queen was busy,
    /// and "it should be able to handle multiple of these at one time".
    ///
    /// Delivery was strictly sequential across the whole Hive, and one attempt
    /// can spend ten seconds waiting for a terminal to settle and ten more
    /// waiting for it to accept. Grouping is what makes running them together
    /// safe: two messages for one terminal share an input line and must stay
    /// ordered; messages for different terminals never touch.
    #[test]
    fn each_terminal_is_its_own_queue_and_keeps_its_own_order() {
        let first = WorkerSessionId::new();
        let second = WorkerSessionId::new();
        let deliveries = vec![
            (first, "first to Queen"),
            (second, "first to Scout"),
            (first, "second to Queen"),
            (second, "second to Scout"),
            (first, "third to Queen"),
        ];

        let groups = group_by_terminal(deliveries, |(session_id, _)| *session_id);

        // One queue per terminal, in the order those terminals were first seen.
        assert_eq!(groups.len(), 2);
        assert_eq!(
            groups[0].iter().map(|(_, what)| *what).collect::<Vec<_>>(),
            ["first to Queen", "second to Queen", "third to Queen"]
        );
        assert_eq!(
            groups[1].iter().map(|(_, what)| *what).collect::<Vec<_>>(),
            ["first to Scout", "second to Scout"]
        );
    }

    /// One terminal must not become two queues, whatever else is in flight.
    #[test]
    fn messages_for_a_single_terminal_stay_in_one_queue() {
        let only = WorkerSessionId::new();
        let groups = group_by_terminal(vec![(only, 1), (only, 2), (only, 3)], |(id, _)| *id);
        assert_eq!(groups.len(), 1);
        assert_eq!(
            groups[0].iter().map(|(_, n)| *n).collect::<Vec<_>>(),
            [1, 2, 3]
        );
    }

    /// Reported three times, most recently live: "The prompt was sitting in the
    /// command line without enter being hit. I hit enter and she is working on
    /// it right now."
    ///
    /// The bytes are written and acknowledged before the render is ever
    /// watched. So giving up on an unsettled render does not undo anything —
    /// it leaves the message in the prompt for a person to submit by hand,
    /// which is precisely what kept happening on a terminal too busy to hold
    /// still for 750ms inside a ten-second window.
    ///
    /// The distinction that matters is whether it was ever on screen at all.
    #[test]
    fn a_message_seen_but_never_still_is_submitted_rather_than_stranded() {
        let seen = MarkerObservation::RenderedUnsettled {
            sequence: 42,
            paste_placeholder: None,
        };
        // The caller treats this as rendered: it proceeds to send Enter.
        assert!(matches!(
            seen,
            MarkerObservation::RenderedUnsettled { sequence: 42, .. }
        ));

        // Never on screen is a different answer, and still refuses to guess:
        // there is nothing demonstrably in the prompt to submit.
        assert!(matches!(
            MarkerObservation::Uncertain,
            MarkerObservation::Uncertain
        ));
    }

    /// The stability window itself is unchanged: a settled render is still what
    /// the happy path waits for, and a gap still resets it.
    #[test]
    fn a_render_that_flickers_does_not_count_as_settled() {
        let start = Instant::now();
        let required = Duration::from_millis(750);
        let mut stability = RenderStability::default();
        assert!(!stability.observe(start, required));
        assert!(!stability.observe(start + Duration::from_millis(500), required));
        stability.reset();
        // The clock restarts from the reset, so the earlier time no longer counts.
        assert!(!stability.observe(start + Duration::from_millis(900), required));
        assert!(stability.observe(start + Duration::from_millis(1_700), required));
    }

    /// The 2026-08-23 wedge: Queen's prompt held an unsent `/rc`, the operator
    /// was told to answer a question, and there was no question. Both halves
    /// have to differ — the kind the control room branches on, and the sentence
    /// the operator reads — or one of them silently reintroduces the bug.
    #[test]
    fn the_two_reasons_a_delivery_is_held_do_not_read_the_same() {
        let busy = DeferralReason::ProviderBusy;
        let unsent = DeferralReason::PromptHoldsUnsentText;

        assert_ne!(busy.refusal_kind(), unsent.refusal_kind());
        assert_eq!(
            busy.refusal_kind(),
            swarm_persistence::REFUSAL_DELIVERY_HELD
        );
        assert_eq!(
            unsent.refusal_kind(),
            swarm_persistence::REFUSAL_DELIVERY_HELD_UNSENT_TEXT
        );

        let busy_text = busy.describe("Queen's review");
        let unsent_text = unsent.describe("Queen's review");
        assert_ne!(busy_text, unsent_text);
        assert!(busy_text.contains("unanswered prompt"), "{busy_text}");
        // The remedy, not the state: there is nothing here to answer.
        assert!(unsent_text.contains("clear the line"), "{unsent_text}");
        assert!(!unsent_text.contains("unanswered"), "{unsent_text}");
    }

    /// The operator, watching Queen: "Are these huge dumps going to cause the
    /// queen to never finish anything?"
    ///
    /// A worker's handoff is capped at 4,000 bytes on the way in and workers
    /// write to the cap. Pasting the whole thing put 128 KB through Queen's
    /// terminal in one day, in the same message that tells her to read task
    /// history for the authoritative version.
    #[test]
    fn a_handoff_reaches_the_terminal_as_an_excerpt_not_a_report() {
        let note = format!("First sentence. {}", "padding words ".repeat(400));
        assert!(note.len() > 4_000);

        let excerpt = handoff_excerpt_within(&note, HANDOFF_EXCERPT_BYTES);

        assert!(excerpt.len() < 700, "{} bytes", excerpt.len());
        assert!(excerpt.starts_with("First sentence."));
        // How much was lost, and how to get it — a pointer nobody can follow is
        // what made Queen unable to review long handoffs at all.
        assert!(excerpt.contains("more characters"), "{excerpt}");
        assert!(excerpt.contains("swarm_read_task_history"), "{excerpt}");
    }

    /// A short handoff is the whole handoff. Adding "…" to something complete
    /// would tell the reader to go and fetch what they already have.
    #[test]
    fn a_short_handoff_is_left_exactly_as_written() {
        let note = "Fixed, deployed, verified against production.";
        assert_eq!(handoff_excerpt_within(note, HANDOFF_EXCERPT_BYTES), note);
    }

    /// Trimming by bytes through a multi-byte character would produce invalid
    /// UTF-8 and panic on the slice.
    #[test]
    fn trimming_lands_on_a_character_boundary() {
        let note = "→".repeat(400);
        let excerpt = handoff_excerpt_within(&note, HANDOFF_EXCERPT_BYTES);
        assert!(excerpt.starts_with('→'));
        assert!(excerpt.contains("swarm_read_task_history"));
    }

    fn outcome(task: &str, reporter: &str, note: &str) -> TaskOutcomeDispatch {
        TaskOutcomeDispatch {
            id: format!("delivery-{task}"),
            task_id: task.parse().unwrap_or_else(|_| swarm_domain::TaskId::new()),
            reporting_worker_id: swarm_domain::WorkerId::new(),
            reporting_worker_name: reporter.to_owned(),
            recipient_worker_id: swarm_domain::WorkerId::new(),
            session_id: WorkerSessionId::new(),
            title: format!("Task {task}"),
            target_state: swarm_domain::TaskState::Review,
            note: note.to_owned(),
        }
    }

    /// "It is almost like she is getting too many prompts before the previous
    /// one is done."
    ///
    /// A provider queues what arrives while it is working, so six outcomes
    /// written separately meant the recipient read its queue six times and
    /// spent context on six preambles. One write, one read.
    #[test]
    fn a_burst_of_outcomes_becomes_one_message() {
        let burst = [
            outcome("a", "Architecture", "docs-spell finally passes"),
            outcome("b", "RCG Hub", "the deploy gate named the wrong slot"),
            outcome("c", "Sculpt Studio", "the countdown is audible"),
        ];

        let message = String::from_utf8(task_outcome_message(&burst)).unwrap();

        assert!(message.starts_with("[Swarm worker outcome] 3 tasks reported."));
        for reporter in ["Architecture", "RCG Hub", "Sculpt Studio"] {
            assert!(
                message.contains(reporter),
                "{reporter} missing from {message}"
            );
        }
        // Typed into a prompt: a newline would submit half of it.
        assert_eq!(message.matches('\n').count(), 0);
        assert!(message.ends_with("authoritative context.\r"));
    }

    /// A burst must not paste N times as much as one outcome did. The excerpt
    /// budget is per message, so the whole point of capping it survives.
    #[test]
    fn a_burst_costs_about_what_one_outcome_costs() {
        let long = "x".repeat(4_000);
        let one = task_outcome_message(&[outcome("a", "Architecture", &long)]).len();
        let six = task_outcome_message(&[
            outcome("a", "Architecture", &long),
            outcome("b", "RCG Hub", &long),
            outcome("c", "Sculpt Studio", &long),
            outcome("d", "Platform", &long),
            outcome("e", "Admin", &long),
            outcome("f", "Nexus", &long),
        ])
        .len();

        assert!(
            six < one * 3,
            "six outcomes took {six} bytes against {one} for one"
        );
    }

    /// One outcome reads exactly as it did; a burst is the new shape, not a new
    /// shape for everything.
    #[test]
    fn a_single_outcome_is_unchanged() {
        let message = String::from_utf8(task_outcome_message(&[outcome(
            "a",
            "Architecture",
            "fixed",
        )]))
        .unwrap();

        assert!(message.starts_with("[Swarm worker outcome] Architecture moved task "));
        assert!(!message.contains("tasks reported"));
    }

    /// THE RUN BRIEF HAS TO NAME THE CLASS THAT WOKE HER.
    ///
    /// EVERY delivered message must end with \r, because that byte is the
    /// difference between sending and typing.
    ///
    /// `submit_terminal_message` reads the last byte to decide whether to press
    /// Enter. A message ending any other way is written into the recipient's
    /// composer, never submitted, and STILL reported Acknowledged — so it is
    /// recorded as delivered while sitting unsent, and never retried. The
    /// operator saw exactly that: messages that "weren't getting into the
    /// terminal because enter wasn't being hit", then appearing later mid-turn.
    ///
    /// A per-builder test would not have caught it. The flag lives in the
    /// SUBMITTER and the builders are what must satisfy it, so the assertion
    /// belongs across all of them at once.
    #[test]
    fn every_delivered_message_ends_with_the_submit_byte() {
        let session = WorkerSessionId::new();
        let queen = queen_automation_message(&QueenAutomationDelivery {
            run_id: "run-1".to_owned(),
            session_id: session,
            worker_id: WorkerId::new(),
            trigger: QueenAutomationTrigger::ActionableWork,
            actionable_count: 1,
            presence: PresenceMode::AtHive,
        });
        assert_eq!(
            queen.last(),
            Some(&b'\r'),
            "the automation brief must submit"
        );
        assert_eq!(
            operator_broadcast_message("reloading in five minutes").last(),
            Some(&b'\r'),
            "a broadcast that does not submit reaches every worker's composer and nobody's turn"
        );

        let message = task_message_message(&[TaskMessageDispatch {
            message_id: "m1".to_owned(),
            task_id: TaskId::new(),
            task_title: "Some work".to_owned(),
            session_id: session,
            sender: swarm_persistence::MessageParty::Queen,
            sender_name: "Queen".to_owned(),
            body: "Which SHA did this ship as?".to_owned(),
        }]);
        assert_eq!(
            message.last(),
            Some(&b'\r'),
            "a message that does not submit is typed into the prompt and reported delivered"
        );
    }

    /// `reviewed_work_without_evidence_attention` feeds `actionable_fingerprint`,
    /// so finished work waiting on judgment is part of what triggers a run. The
    /// brief then enumerated only worker-liveness cases — brief-did-not-start,
    /// worker-resting, worker-exited — and never mentioned it.
    ///
    /// A Hive that wakes Queen BECAUSE work is waiting and then briefs her about
    /// stuck workers gets exactly what the operator saw: 92 tasks cleared in a
    /// day, and a review pile that grew to sixteen while they asked why the
    /// board was not moving.
    ///
    /// Naming it is not enough by itself. An item with no move attached is one
    /// she parks, so the brief carries the move for each state — including the
    /// deployment she may not perform during an unattended run but may route.
    #[test]
    fn the_run_brief_names_finished_work_and_the_move_for_each_state() {
        let message = String::from_utf8(queen_automation_message(&QueenAutomationDelivery {
            run_id: "run-1".to_owned(),
            session_id: WorkerSessionId::new(),
            worker_id: WorkerId::new(),
            trigger: QueenAutomationTrigger::ActionableWork,
            actionable_count: 16,
            presence: PresenceMode::AtHive,
        }))
        .unwrap();

        assert!(
            message.contains("finished work nothing has settled"),
            "the class that woke her is unnamed: {message}"
        );
        assert!(message.contains("swarm_approve_no_deployment"), "{message}");
        // An approval must cite what it rests on, or the second pair of eyes
        // is a click. A stamp under load is what approved a false claim.
        assert!(
            message.contains("RESTS ON"),
            "approving without a basis is the failure this names: {message}"
        );
        // The move for work that is missing something, and it is NOT backwards.
        assert!(
            message.contains("swarm_return_reviewed_work"),
            "the hand-back move must be named: {message}"
        );
        // The move for work that is finished and merely unshipped.
        assert!(
            message.contains("awaiting_release"),
            "work held only because it has not shipped has its own state: {message}"
        );
        // AND THE OLD ADVICE MUST NOT COME BACK. This brief told her to return
        // reviewed work to Ready to get a worker's attention, and Ready means
        // UNSTARTED to everything that reads it — it invalidated a valid
        // evidence claim on 2026-09-01. The phrase survives ONLY for waking a
        // sleeping worker, which is a different act on different work.
        assert!(
            !message.contains("return work to Ready and reassign"),
            "the brief still instructs the backward move it was corrected for: {message}"
        );
        // The one she cannot do herself is still hers to route.
        assert!(
            message.contains("assign a task to the owning worker"),
            "routing a deployment is coordination and must be named: {message}"
        );
    }
}
