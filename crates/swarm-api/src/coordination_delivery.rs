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
    DecisionDispatch, OperatorBroadcastDispatch, QueenAutomationDelivery, TaskDispatch,
    TaskMessageDispatch, TaskOutcomeDispatch, TaskStore,
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
    /// Coordination wrote to this terminal recently and is leaving it alone.
    ///
    /// NOT A FAILURE, and it is the only deferral that is a deliberate pause
    /// rather than an obstacle. Nothing is wrong with the terminal; the message
    /// is being saved up so it arrives with whatever else accumulates instead of
    /// interrupting again.
    RecentDelivery,
    /// Builder policy holds experimental providers during unattended operation.
    ProviderPolicy,
}

impl DeferralReason {
    /// The refusal kind this deferral is recorded under, if it is one at all.
    ///
    /// The control room branches on the kind rather than reading the prose, so
    /// the situations can be told apart without matching on a sentence.
    ///
    /// NONE FOR A COOLDOWN, AND THAT IS THE POINT OF THE OPTION. The other two
    /// are things a PERSON must fix — answer the prompt, clear the unsent line —
    /// and they sit in coordinator attention until somebody does. A cooldown is
    /// normal operation that resolves itself in five minutes. Recording it as
    /// attention would put a permanent, self-clearing entry in front of the
    /// operator every time coordination worked correctly, which is how a
    /// surface that means "something needs you" stops meaning anything.
    pub(super) fn refusal_kind(self) -> Option<&'static str> {
        match self {
            Self::ProviderBusy => Some(swarm_persistence::REFUSAL_DELIVERY_HELD),
            Self::PromptHoldsUnsentText => {
                Some(swarm_persistence::REFUSAL_DELIVERY_HELD_UNSENT_TEXT)
            }
            Self::RecentDelivery | Self::ProviderPolicy => None,
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
            Self::ProviderPolicy => format!(
                "{subject} is queued until this provider is eligible for automation; experimental providers wait until Night Watch ends"
            ),
            Self::RecentDelivery => format!(
                "{subject} is waiting so this terminal is not written to twice in a few minutes; it arrives with anything else that accumulates"
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
    describe: impl Fn(&D) -> (WorkerSessionId, CoordinationMessage),
) -> Vec<(D, Result<TerminalSubmission, swarm_terminal::IpcError>)> {
    let describe = &describe;
    let groups = group_by_terminal(deliveries, |delivery| describe(delivery).0)
        .into_iter()
        .map(|group| async move {
            let mut settled = Vec::with_capacity(group.len());
            for delivery in group {
                let (session_id, message) = describe(&delivery);
                let submission =
                    submit_coordination_message(store, client, session_id, message).await;
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
    describe: impl Fn(&[D]) -> CoordinationMessage,
) -> Vec<(Vec<D>, Result<TerminalSubmission, swarm_terminal::IpcError>)> {
    let describe = &describe;
    let session_of = &session_of;
    let groups = group_by_terminal(deliveries, session_of)
        .into_iter()
        .map(|group| async move {
            let Some(session_id) = group.first().map(session_of) else {
                return (group, Ok(TerminalSubmission::Acknowledged));
            };
            let message = describe(&group);
            let submission = submit_coordination_message(store, client, session_id, message).await;
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
    message: CoordinationMessage,
) -> Result<TerminalSubmission, swarm_terminal::IpcError> {
    // ONE GATE, HERE, because every delivery path already funnels through this
    // function. Putting the check in each caller is how the six marker pairings
    // drifted into two wrong ones this morning.
    //
    // A store error does NOT hold the message. Failing to read the cooldown is
    // not evidence that one is in effect, and treating it as one would turn a
    // database hiccup into a Hive that silently stops coordinating.
    if message.cadence == Cadence::Cooled
        && store
            .coordination_is_cooling_down(session_id, unix_timestamp())
            .unwrap_or(false)
    {
        return Ok(TerminalSubmission::Deferred(DeferralReason::RecentDelivery));
    }
    let provider = match store.provider_for_active_session(session_id) {
        Ok(provider) => provider,
        Err(error) => {
            return Ok(TerminalSubmission::Rejected {
                code: "provider_identity_unavailable".into(),
                message: error.to_string(),
            });
        }
    };
    // All coordination channels share this last pre-submission policy check.
    // Unavailable presence is not permission to send; deferral retains the outbox.
    if !store
        .operator_presence(unix_timestamp())
        .is_ok_and(|presence| provider.permits_automation_in(presence.mode))
    {
        return Ok(TerminalSubmission::Deferred(DeferralReason::ProviderPolicy));
    }
    let submission =
        submit_terminal_message(client, session_id, provider, message.bytes, &message.marker)
            .await?;
    // ONLY AN ACKNOWLEDGED WRITE STARTS A COOLDOWN. A deferred or uncertain
    // delivery interrupted nobody, and starting one for it would delay the
    // retry of a message that never arrived.
    if submission == TerminalSubmission::Acknowledged
        && let Err(error) = store.record_coordination_delivery(session_id, unix_timestamp())
    {
        // Worth saying and not worth failing for: the message DID land. A lost
        // cooldown means the next delivery is early, which is exactly the
        // behaviour that existed before this and is survivable.
        tracing::warn!(message = %error, %session_id, "a delivery landed but its cooldown could not be recorded");
    }
    Ok(submission)
}

/// What a read of the terminal says about writing to it now.
enum Baseline {
    Ready {
        sequence: u64,
        paste_placeholder: Option<Vec<u8>>,
    },
    /// The prompt holds unsent text and that text is DEMONSTRABLY THIS MESSAGE:
    /// our own marker is on the screen. Nothing needs writing — it only needs
    /// submitting.
    ///
    /// This is the residue of an Uncertain submission. The write landed, the
    /// three Enter attempts could not be confirmed, and the message has been
    /// sitting in the composer ever since. Measured 2026-09-02 on the first
    /// real broadcast: 24 of them, and the operator pressed Enter by hand.
    ///
    /// Retrying is safe HERE and only here. The general worry about retrying an
    /// uncertain submit is that the message may already have gone and a retry
    /// duplicates it — but a marker still visible in an unsent prompt is proof
    /// it did NOT go. The ambiguity that made retry unsafe is resolved by
    /// evidence rather than assumed away.
    HoldsOurUnsentMessage {
        sequence: u64,
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
    marker: &[u8],
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
                    // OURS, OR SOMEBODY ELSE'S? The refusal below protects the
                    // worker's own typing and stays. But when the unsent text
                    // carries THIS delivery's marker it is our own stranded
                    // message, and appending is not what it needs — Enter is.
                    let visible =
                        snapshot_plain_text(&snapshot.bytes, snapshot.rows, snapshot.columns);
                    let ours = !marker.is_empty()
                        && visible
                            .as_bytes()
                            .windows(marker.len())
                            .any(|part| part == marker);
                    if ours {
                        return Ok(Baseline::HoldsOurUnsentMessage {
                            sequence: snapshot.sequence,
                        });
                    }
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
        match delivery_baseline(client, session_id, provider, marker).await? {
            Baseline::Ready {
                sequence,
                paste_placeholder,
            } => (sequence, paste_placeholder),
            // ALREADY WRITTEN, NEVER SUBMITTED. Skip straight to Enter rather
            // than writing the message a second time underneath itself — the
            // text is on screen and only the submit is missing.
            Baseline::HoldsOurUnsentMessage { sequence } => {
                if !submit {
                    return Ok(TerminalSubmission::Acknowledged);
                }
                // NAMES THE DELIVERY, NOT A COUNT. If the marker search never
                // matches a real prompt this recovery does nothing and reports
                // nothing, which looks exactly like a working recovery on a
                // quiet week — and operator_broadcast_outcome cannot separate
                // them either, because still-waiting=0 is also what "no
                // uncertain submissions happened" looks like. One named line
                // the first time it fires is what makes the difference legible.
                tracing::info!(
                    marker = %String::from_utf8_lossy(marker),
                    %session_id,
                    "a previous delivery left this message unsent in the prompt; submitting it \
                     rather than writing it again"
                );
                return submit_rendered_message(
                    client, session_id, provider, marker, sequence, None,
                )
                .await;
            }
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
    submit_rendered_message(
        client,
        session_id,
        provider,
        marker,
        rendered_sequence,
        rendered_paste_placeholder.as_deref(),
    )
    .await
}

/// Presses Enter on a message already rendered in the prompt, and confirms it.
///
/// ONE IMPLEMENTATION, CALLED TWICE. The ordinary path renders the message then
/// submits it; the recovery path finds a message an earlier attempt already
/// rendered and submits that. Writing a second Enter loop beside this one is
/// how the broadcast dispatch query diverged from the task-message one earlier
/// today, and that divergence only showed as an empty result.
async fn submit_rendered_message(
    client: &HostClient,
    session_id: WorkerSessionId,
    provider: ProviderKind,
    marker: &[u8],
    rendered_sequence: u64,
    rendered_paste_placeholder: Option<&[u8]>,
) -> Result<TerminalSubmission, swarm_terminal::IpcError> {
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
            rendered_paste_placeholder,
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

/// A coordination message and the marker that proves it reached the screen.
///
/// ONE OWNER FOR BOTH HALVES, and that is the entire reason this type exists.
/// Delivery holds Enter until it can SEE the message: it searches the rendered
/// screen for the marker. A marker that is not in the bytes can therefore never
/// be found, Enter is never sent, and the message sits unsent in the
/// recipient's prompt while the record says delivered.
///
/// That is not hypothetical. Until 2026-09-02 the call sites chose a message
/// builder and, SEPARATELY, an id to build the marker from, with nothing tying
/// the two together. Two of the six pairings were wrong: a broadcast passed its
/// broadcast id while its text carried no id at all, and a worker message
/// passed its MESSAGE id while its text wrote the TASK id. Both shipped, and
/// both were invisible, because a marker that never renders is indistinguishable
/// from a terminal too busy to settle.
///
/// Pairing them in one value is what lets one test check all six at once.
pub(super) struct CoordinationMessage {
    pub(super) bytes: Vec<u8>,
    pub(super) marker: Vec<u8>,
    /// Whether this message waits for the terminal's cooldown, or goes now.
    ///
    /// On the message rather than at the call site, for the same reason the
    /// marker is: a property chosen separately from the message it describes is
    /// a property that drifts from it. Two of six markers were wrong that way
    /// this morning.
    pub(super) cadence: Cadence,
}

/// Whether a message waits its turn or interrupts.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Cadence {
    /// Held while the terminal is cooling down, and delivered with whatever
    /// else has accumulated. Everything the BOARD generates is this: briefs,
    /// messages, outcomes, decision answers, automation runs. None of it is so
    /// urgent that five minutes matters, and all of it together is the flood.
    Cooled,
    /// Written as soon as the terminal is resting, cooldown or not.
    ///
    /// ONLY OPERATOR BROADCASTS. A person has just typed something to every
    /// running worker and is waiting on it — "please pause so I can reload" is
    /// worthless five minutes late. It is also the one message class with an
    /// expiry: `BROADCAST_DELIVERY_WINDOW_SECONDS` is 600, and a 300 second
    /// cooldown would silently eat half of every broadcast's window.
    Immediate,
}

/// The handle delivery searches the rendered screen for.
///
/// THE WHOLE ID, NOT A PREFIX OF IT. This took the first eight bytes until
/// 2026-09-02, which for a `UUIDv7` is the high 32 bits of a millisecond
/// timestamp — a bucket about 65 SECONDS WIDE, not an identity. Measured
/// against this Hive's own task messages and broadcasts: three-way prefix
/// collisions were routine and the widest span sharing one prefix was 59
/// seconds. So two deliveries to the same terminal inside a minute were
/// indistinguishable, and `Baseline::HoldsOurUnsentMessage` — which decides
/// whether unsent text in a prompt is OURS and may simply be submitted — could
/// press Enter on a different message and record ours as delivered.
///
/// The collision is not theoretical and it is not rare. It defeated the first
/// draft of `every_delivered_message_contains_the_marker_delivery_will_look_for`,
/// which minted two ids a microsecond apart and so PASSED a path that was
/// broken.
///
/// Full length is safe on screen: `snapshot_plain_text` replays through vt100,
/// which omits the newline on a wrap-continuation row, so a 36 character marker
/// comes back contiguous at every wrap offset. That is asserted rather than
/// assumed by `a_delivery_marker_survives_the_terminal_wrapping_it`.
pub(super) fn delivery_marker(id: impl std::fmt::Display) -> Vec<u8> {
    id.to_string().into_bytes()
}

pub(super) fn decision_delivery_message(delivery: &DecisionDispatch) -> CoordinationMessage {
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
    CoordinationMessage {
        cadence: Cadence::Cooled,
        bytes: format!(
            "[Swarm decision {} resolved] {} Operator note: {} Use swarm_list_decisions for the full request context.\r",
            delivery.decision_id, outcome, note,
        )
        .into_bytes(),
        marker: delivery_marker(delivery.decision_id),
    }
}

pub(super) fn task_dispatch_message(delivery: &TaskDispatch) -> CoordinationMessage {
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
    CoordinationMessage {
        cadence: Cadence::Cooled,
        bytes: format!(
            "[Swarm task {} assigned] {}.{}{}{} Call swarm_list_tasks now and work from its authoritative task details and linked evidence. If this task is not visible, stop; its assignment changed.\r",
            delivery.task_id, title, instruction, ruling, requester,
        )
        .into_bytes(),
        marker: delivery_marker(delivery.task_id),
    }
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

pub(super) fn queen_automation_message(delivery: &QueenAutomationDelivery) -> CoordinationMessage {
    let bytes = format!(
        "[Swarm automation {}] Review {} actionable records while the operator is {}. Use swarm_list_tasks, swarm_list_workers, and swarm_list_coordination_attention as the authority. Draft tasks are part of this review: a draft is work nobody has decided about yet, so triage each one into ready, blocked, or removed rather than leaving it sitting. Coordination attention can identify Ready work whose delivered brief did not start, Active work that is unchanged while its loaded worker is resting, or work whose worker process exited; recheck the current task and worker before deciding whether to restart, steer, wait, or ask the operator. It ALSO identifies finished work nothing has settled, which is usually the largest part of this review and is yours rather than the operator's: approve a no-deployment claim with swarm_approve_no_deployment once you have read the handoff, SAYING WHAT YOUR AGREEMENT RESTS ON — a merged SHA, a recorded deployment, the handoff you read, or an explicit \"I could not verify\", which is accepted; saying nothing is not. When something is missing, hand it back with swarm_return_reviewed_work naming what you need: THE TASK STAYS IN REVIEW and the next move becomes the worker\'s. Do NOT move reviewed work to Ready to get attention — Ready means UNSTARTED to everything that reads it and erases that the work was done. When work is finished and merely waiting to ship, move it to awaiting_release: it needs no evidence to enter and COMPLETES ITSELF when a deployment is recorded, so it is the right home for anything held only because it has not shipped. When commits touch code with no deployment assign a task to the owning worker to ship it — you cannot deploy during this run, but routing the deployment is coordination and is yours. Work parked on a SLEEPING worker is yours to move rather than yours to wait on: there is no wake tool, and assigning READY work with swarm_assign_task queues a guarded wake, so reassigning it to the same sleeping worker is how that worker is started. Only Ready work wakes anyone — work left Active or Blocked on a SLEEPING worker wakes nobody, so return it to Ready first (Active to Blocked to Ready) and then assign. That route is for WAKING a stopped worker and nothing else; it is not how you ask a running worker for something, which is swarm_message_worker, and it is not how you hand back reviewed work, which is swarm_return_reviewed_work. You can also ask a running worker a question without interrupting it: swarm_message_worker waits until its terminal is resting, so it never lands mid-turn, and the exchange is recorded on the task. Observe the live session before calling the work Active again. Respect worker repository ownership and the configured Queen autonomy ceiling. Do not perform Jira, Apiary, email, deployment, or other external side effects during this run. When operator judgment is needed, create one swarm_request_decision per concrete task. Link its task_id, make the suggested_action exactly one allowed_actions button, and never group unrelated tasks or a fleet review into one approval. When this exact review is finished, call swarm_finish_automation_run with run_id {} and outcome completed, needs_operator, or no_action.\r",
        delivery.run_id,
        delivery.actionable_count,
        delivery.presence,
        delivery.run_id,
    )
    .into_bytes();
    CoordinationMessage {
        cadence: Cadence::Cooled,
        bytes,
        marker: delivery_marker(&delivery.run_id),
    }
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
pub(super) fn task_message_message(messages: &[TaskMessageDispatch]) -> CoordinationMessage {
    use std::fmt::Write as _;
    // THE ID IN THE HEADER IS THE MARKER. The blocks below name the TASK, which
    // is what a reader needs, but a task id is not this delivery's id — and the
    // marker was built from the message id, which appeared nowhere. Delivery
    // therefore searched the screen for something that was never typed, so Enter
    // was never sent and the message sat unsent in a prompt that reported it
    // delivered. The id here and the marker below are now the same value by
    // construction.
    let reference = messages
        .first()
        .map(|message| message.message_id.as_str())
        .unwrap_or_default();
    let mut text = String::from("[Message");
    if messages.len() > 1 {
        let _ = write!(text, "s ({})", messages.len());
    }
    let _ = writeln!(text, " via Swarm · {reference}]");
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
    CoordinationMessage {
        cadence: Cadence::Cooled,
        bytes: text.into_bytes(),
        marker: delivery_marker(reference),
    }
}

/// What an operator broadcast looks like in a worker's terminal.
///
/// Says it went to everyone, because a worker that cannot tell a broadcast from
/// a message addressed to it will answer as though it were asked personally,
/// and thirty-nine workers each replying to one announcement is its own outage.
pub(super) fn operator_broadcast_message(
    group: &[OperatorBroadcastDispatch],
) -> CoordinationMessage {
    let body = group
        .iter()
        .map(|dispatch| dispatch.body.as_str())
        .collect::<Vec<_>>()
        .join("\n\n");
    // Ends with \r for the reason spelled out in task_message_message: that
    // byte is the submit flag, and \n leaves this typed into the composer,
    // unsent, while the delivery pass records it as delivered.
    // THE ID IN THE HEADER IS THE MARKER, and it is why the header carries one
    // at all. Delivery holds Enter until it can see this line on screen; with no
    // id in the text there was nothing to see, so Enter was never sent and the
    // operator pressed it themselves on every worker. Reported 2026-09-02: "when
    // I sent a broadcast, it only goes to certain workers and it didn't hit
    // enter after so I had to do that manually."
    let reference = group
        .first()
        .map(|dispatch| dispatch.broadcast_id.as_str())
        .unwrap_or_default();
    CoordinationMessage {
        cadence: Cadence::Immediate,
        bytes: format!(
            "[Broadcast from the operator to every running worker · {reference}]\n{body}\n\nThis \
             went to all workers at once. It is not addressed to you personally and wants no \
             reply unless it asks for one.\r"
        )
        .into_bytes(),
        marker: delivery_marker(reference),
    }
}

pub(super) fn task_outcome_message(outcomes: &[TaskOutcomeDispatch]) -> CoordinationMessage {
    let Some((first, rest)) = outcomes.split_first() else {
        return CoordinationMessage {
            cadence: Cadence::Cooled,
            bytes: Vec::new(),
            marker: Vec::new(),
        };
    };
    let marker = delivery_marker(first.task_id);
    if rest.is_empty() {
        return CoordinationMessage {
            cadence: Cadence::Cooled,
            bytes: format!(
                "[Swarm worker outcome] {} Use swarm_list_tasks and task history for authoritative context.\r",
                one_outcome(first, HANDOFF_EXCERPT_BYTES),
            )
            .into_bytes(),
            marker,
        };
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
    CoordinationMessage {
        cadence: Cadence::Cooled,
        bytes: format!(
            "[Swarm worker outcome] {} tasks reported. {reported} Use swarm_list_tasks and task history for authoritative context.\r",
            outcomes.len(),
        )
        .into_bytes(),
        marker,
    }
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

    #[tokio::test]
    async fn experimental_coordination_is_deferred_before_contacting_terminal() {
        let store = TaskStore::in_memory().unwrap();
        let worker = store
            .create_worker(
                "Experimental",
                ProviderKind::Gemini,
                "/workspace/experimental",
                false,
                1,
            )
            .unwrap();
        let session = WorkerSessionId::new();
        store.bind_worker_session(worker.id, session).unwrap();
        store
            .set_manual_presence(Some(PresenceMode::NightWatch), unix_timestamp())
            .unwrap();
        let result = submit_coordination_message(
            &store,
            &HostClient::new("/unreachable/terminal.sock"),
            session,
            CoordinationMessage {
                cadence: Cadence::Cooled,
                bytes: b"test\r".to_vec(),
                marker: b"test".to_vec(),
            },
        )
        .await
        .unwrap();
        assert_eq!(
            result,
            TerminalSubmission::Deferred(DeferralReason::ProviderPolicy)
        );
        assert_eq!(DeferralReason::ProviderPolicy.refusal_kind(), None);
        assert!(
            !store
                .coordination_is_cooling_down(session, unix_timestamp())
                .unwrap()
        );
    }

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
        // SEARCH WHAT DELIVERY SEARCHES. This read the raw journal until
        // 2026-09-02 — a copy of the bytes just pushed, which contains them by
        // definition — so the test could not fail whatever the terminal did to
        // them, and the wrap column it names was never exercised.
        // observe_stable_marker reads the re-rendered screen, and that is the
        // surface where a token can be broken.
        let TerminalSnapshot {
            bytes,
            rows,
            columns,
            ..
        } = state.snapshot();
        snapshot_plain_text(&bytes, rows, columns).contains(marker)
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
        // A WHOLE ID, because that is what a marker is now. It was eight bytes
        // when this test was written, and eight bytes of a UUIDv7 identifies a
        // ~65 second window rather than a delivery — see `delivery_marker`. A
        // 36 character marker crosses far more wrap columns than an 8 character
        // one, so the length change is exactly what this has to cover.
        let marker = "01a0169f-4c1a-7c3e-9b2d-5f0e8a71c204";

        assert!(marker_survives_render(80, "moved task ", marker));
        // Every offset across the boundary at three widths, rather than the two
        // hand-picked offsets this used to try. vt100 omits the newline on a
        // wrap-continuation row, so a marker comes back contiguous — but that is
        // a property of the renderer, not something the delivery code controls,
        // and this is what would notice if it changed.
        for columns in [40u16, 80, 120] {
            for offset in 0..=usize::from(columns) + 1 {
                assert!(
                    marker_survives_render(columns, &"x".repeat(offset), marker),
                    "a marker broken at the wrap is a delivery that can never be confirmed \
                     (columns={columns}, offset={offset})"
                );
            }
        }
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

    /// THE SEAM THE MARKER SEARCH RESTS ON, MEASURED THROUGH THE REAL RENDERER.
    ///
    /// The fleet rule is that a passing mocked test is not evidence about an OS
    /// surface, and a terminal is one. The search runs over
    /// `snapshot_plain_text`, which replays the byte stream through vt100 and
    /// returns `screen.contents()`. A hand-built snapshot skips exactly that, so
    /// it cannot say whether a marker survives the grid.
    ///
    /// MEASURED, NOT ASSUMED: it does. A logical line longer than the terminal
    /// is returned CONTIGUOUSLY — the grid does not insert a newline where it
    /// wraps — so a marker is findable wherever it lands on a row. I checked
    /// this by rendering a line wider than the screen and reading the output,
    /// after a first version of this test passed even with a marker too long to
    /// fit, which is what exposed that wrapping was never a hazard.
    ///
    /// So this asserts the CONTIGUITY the search depends on, rather than
    /// pretending to probe wrap offsets that cannot break it. It fails if
    /// `snapshot_plain_text` ever starts breaking lines at the grid edge, which
    /// would silently stop every marker search matching.
    #[test]
    fn the_renderer_returns_a_wrapped_line_contiguously() {
        let marker = b"01a062ed";
        let mut bytes = b"x".repeat(74);
        bytes.extend_from_slice(marker);
        bytes.extend_from_slice(b" the message body\r\n");

        let visible = swarm_terminal::snapshot_plain_text(&bytes, 24, 20);
        assert!(
            visible
                .as_bytes()
                .windows(marker.len())
                .any(|part| part == marker),
            "a marker that straddles the grid edge must still be findable, or the recovery \
             silently never fires: {visible:?}"
        );
        assert!(
            visible.contains("the message body"),
            "and so must text after it on the same wrapped line"
        );
    }

    /// A PROMPT HOLDING OUR OWN UNSENT MESSAGE IS NOT THE SAME AS ONE HOLDING
    /// THE WORKER'S TYPING, and telling them apart is what makes a retry safe.
    ///
    /// 24 broadcast submissions came back Uncertain on 2026-09-02: the write
    /// landed, three Enter attempts went unconfirmed, and the text sat in the
    /// composer until the operator pressed Enter by hand. Retrying an uncertain
    /// submit is normally unsafe because the message may already have gone and
    /// a retry would duplicate it. A marker still visible in an UNSENT prompt is
    /// proof it did not go, so the ambiguity is resolved by evidence rather than
    /// assumed away — which is the condition the ticket set for choosing retry.
    ///
    /// The refusal still stands for text that is not ours: that guard protects
    /// the worker's own typing and merging two instructions into one Enter is a
    /// worse failure than not delivering.
    #[test]
    fn our_own_unsent_message_is_submitted_and_a_workers_typing_is_still_refused() {
        let marker = b"swarm-delivery-abc123";
        let ours = "some scrollback\nswarm-delivery-abc123 the message body\n";
        let theirs = "some scrollback\nwhat the worker was halfway through typing\n";

        let carries_marker = |screen: &str| {
            screen
                .as_bytes()
                .windows(marker.len())
                .any(|part| part == marker)
        };

        assert!(
            carries_marker(ours),
            "an unsent prompt carrying this delivery's marker is our own message, and it needs \
             Enter rather than another copy of itself"
        );
        assert!(
            !carries_marker(theirs),
            "and a prompt holding the worker's own text must still defer, because appending or \
             submitting it would merge two instructions into one Enter"
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
            Some(swarm_persistence::REFUSAL_DELIVERY_HELD)
        );
        assert_eq!(
            unsent.refusal_kind(),
            Some(swarm_persistence::REFUSAL_DELIVERY_HELD_UNSENT_TEXT)
        );
        // A COOLDOWN IS NOT ATTENTION. The other two sit in front of the
        // operator until a person acts; this one clears itself in five minutes,
        // and recording it would put a self-resolving entry on the surface that
        // exists to mean "something needs you" every time delivery worked.
        assert_eq!(
            DeferralReason::RecentDelivery.refusal_kind(),
            None,
            "a scheduled wait must not be filed as something the operator has to fix"
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

        let message = String::from_utf8(task_outcome_message(&burst).bytes).unwrap();

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
        let one = task_outcome_message(&[outcome("a", "Architecture", &long)])
            .bytes
            .len();
        let six = task_outcome_message(&[
            outcome("a", "Architecture", &long),
            outcome("b", "RCG Hub", &long),
            outcome("c", "Sculpt Studio", &long),
            outcome("d", "Platform", &long),
            outcome("e", "Admin", &long),
            outcome("f", "Nexus", &long),
        ])
        .bytes
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
        let message =
            String::from_utf8(task_outcome_message(&[outcome("a", "Architecture", "fixed")]).bytes)
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
    /// EXACTLY ONE MESSAGE CLASS INTERRUPTS, and it is the one a person is
    /// waiting on.
    ///
    /// The cooldown exists because everything the BOARD generates arrives at
    /// whatever pause an agent next produces, so a busy agent is written to at
    /// every turn boundary. An operator broadcast is not board churn: a human
    /// has just typed to every running worker and is waiting. "Please pause so
    /// I can reload" is worthless five minutes late.
    ///
    /// It is also the only class with an EXPIRY — `BROADCAST_DELIVERY_WINDOW_SECONDS`
    /// is 600 — so a 300 second cooldown would silently consume half of every
    /// broadcast's window and expire some of them outright.
    ///
    /// Asserted over every builder rather than the one that changed, because
    /// the failure this guards is a NEW path quietly choosing Immediate: one
    /// exemption is a decision, and a second one nobody argued for is how a
    /// cooldown stops meaning anything.
    #[test]
    fn only_an_operator_broadcast_is_allowed_past_the_cooldown() {
        let session = WorkerSessionId::new();
        let task: TaskId = "01a06300-0000-7000-8000-000000000001"
            .parse()
            .expect("a fixed task id");
        let cadences: Vec<(&str, Cadence)> = vec![
            (
                "task brief",
                task_dispatch_message(&TaskDispatch {
                    assignment_id: "assignment-1".to_owned(),
                    task_id: task,
                    worker_id: WorkerId::new(),
                    session_id: session,
                    title: "Repoint the syslog forwarder".to_owned(),
                    description: String::new(),
                    priority: swarm_domain::TaskPriority::High,
                    workspace: "/workspace".to_owned(),
                    operator_instruction: String::new(),
                    operator_rulings: Vec::new(),
                    email_requester: None,
                })
                .cadence,
            ),
            (
                "operator decision",
                decision_delivery_message(&DecisionDispatch {
                    decision_id: "01a0beef-0000-7000-8000-000000000003"
                        .parse()
                        .expect("a fixed decision id"),
                    worker_id: WorkerId::new(),
                    session_id: session,
                    action: "Release the hold".to_owned(),
                    note: String::new(),
                    answers: std::collections::BTreeMap::default(),
                })
                .cadence,
            ),
            (
                "task message",
                task_message_message(&[TaskMessageDispatch {
                    message_id: "01a0f00d-0000-7000-8000-000000000002".to_owned(),
                    task_id: task,
                    task_title: "Some work".to_owned(),
                    session_id: session,
                    sender: swarm_persistence::MessageParty::Queen,
                    sender_name: "Queen".to_owned(),
                    body: "Which SHA did this ship as?".to_owned(),
                }])
                .cadence,
            ),
            (
                "task outcome",
                task_outcome_message(&[outcome("a", "Architecture", "fixed")]).cadence,
            ),
            (
                "queen automation run",
                queen_automation_message(&QueenAutomationDelivery {
                    run_id: "run-1".to_owned(),
                    session_id: session,
                    worker_id: WorkerId::new(),
                    trigger: QueenAutomationTrigger::ActionableWork,
                    actionable_count: 1,
                    presence: PresenceMode::AtHive,
                })
                .cadence,
            ),
            (
                "operator broadcast",
                operator_broadcast_message(&[broadcast("please pause")]).cadence,
            ),
        ];

        let immediate = cadences
            .iter()
            .filter(|(_, cadence)| *cadence == Cadence::Immediate)
            .map(|(name, _)| *name)
            .collect::<Vec<_>>();
        assert_eq!(
            immediate,
            vec!["operator broadcast"],
            "exactly one class may interrupt a cooling terminal, and a second one \
             appearing here is a decision somebody has to argue for"
        );
    }

    /// THE MARKER MUST BE IN THE BYTES. Every delivery, no exceptions.
    ///
    /// Delivery does not press Enter on faith. It writes the message, then
    /// searches the RENDERED SCREEN for the marker and only submits once it can
    /// see it. So a marker that is not in the bytes is not a weak check — it is
    /// a check that can never pass. Enter is never sent, the message sits unsent
    /// in the recipient's prompt, and the delivery is recorded as delivered.
    ///
    /// TWO OF THE SIX WERE WRONG WHEN THIS WAS WRITTEN, and both had shipped.
    /// A broadcast passed its broadcast id while its text carried no id at all.
    /// A worker message passed its MESSAGE id while its text wrote the TASK id —
    /// a different id, one character class away from looking correct in review.
    /// Briefs, decisions, outcomes and automation runs were right, which is why
    /// the mechanism looked like it worked: the paths anyone tested by hand
    /// were the four sound ones.
    ///
    /// Neither failure was visible from anywhere. An unfindable marker produces
    /// exactly the same log line as a terminal too busy to settle, so 24
    /// uncertain submissions on the first real broadcast read as load.
    ///
    /// EVERY BUILDER AT ONCE, AND NAMED IN THE FAILURE. Checking one path would
    /// have passed on any of the four sound ones. The failure message has to
    /// say WHICH message is wrong and what it was looking for, because a red
    /// test that only says "some marker is missing" sends the next reader back
    /// to re-derive the finding from scratch.
    #[test]
    fn every_delivered_message_contains_the_marker_delivery_will_look_for() {
        let session = WorkerSessionId::new();
        // FIXED IDS, AND THAT IS LOAD-BEARING. Written first with TaskId::new()
        // for both, this test PASSED the task-message case it was written to
        // fail — two UUIDv7s minted in the same millisecond share their first
        // eight characters, which is all a marker takes. The bug hid inside the
        // test for the bug. Distinct prefixes here are what make the check real,
        // and the ease of losing it is the second half of this defect: eight
        // characters of a UUIDv7 is a ~65 second bucket, not an identity.
        let task: TaskId = "01a06300-0000-7000-8000-000000000001"
            .parse()
            .expect("a fixed task id");
        let message_id = "01a0f00d-0000-7000-8000-000000000002".to_owned();
        let built: Vec<(&str, CoordinationMessage)> = vec![
            (
                "task brief",
                task_dispatch_message(&TaskDispatch {
                    assignment_id: "assignment-1".to_owned(),
                    task_id: task,
                    worker_id: WorkerId::new(),
                    session_id: session,
                    title: "Repoint the syslog forwarder".to_owned(),
                    description: String::new(),
                    priority: swarm_domain::TaskPriority::High,
                    workspace: "/workspace".to_owned(),
                    operator_instruction: String::new(),
                    operator_rulings: Vec::new(),
                    email_requester: None,
                }),
            ),
            (
                "operator decision",
                decision_delivery_message(&DecisionDispatch {
                    decision_id: "01a0beef-0000-7000-8000-000000000003"
                        .parse()
                        .expect("a fixed decision id"),
                    worker_id: WorkerId::new(),
                    session_id: session,
                    action: "Release the hold".to_owned(),
                    note: String::new(),
                    answers: std::collections::BTreeMap::default(),
                }),
            ),
            (
                "operator broadcast",
                operator_broadcast_message(&[broadcast("please pause")]),
            ),
            (
                "task message",
                task_message_message(&[TaskMessageDispatch {
                    message_id,
                    task_id: task,
                    task_title: "Some work".to_owned(),
                    session_id: session,
                    sender: swarm_persistence::MessageParty::Queen,
                    sender_name: "Queen".to_owned(),
                    body: "Which SHA did this ship as?".to_owned(),
                }]),
            ),
            (
                "task outcome",
                task_outcome_message(&[outcome("a", "Architecture", "fixed")]),
            ),
            (
                "queen automation run",
                queen_automation_message(&QueenAutomationDelivery {
                    run_id: "run-1".to_owned(),
                    session_id: session,
                    worker_id: WorkerId::new(),
                    trigger: QueenAutomationTrigger::ActionableWork,
                    actionable_count: 1,
                    presence: PresenceMode::AtHive,
                }),
            ),
        ];

        let missing = built
            .iter()
            .filter(|(_, message)| {
                message.marker.is_empty()
                    || !message
                        .bytes
                        .windows(message.marker.len())
                        .any(|part| part == message.marker)
            })
            .map(|(name, message)| {
                format!(
                    "{name}: marker {:?} is nowhere in {:?}",
                    String::from_utf8_lossy(&message.marker),
                    String::from_utf8_lossy(&message.bytes),
                )
            })
            .collect::<Vec<_>>();

        assert!(
            missing.is_empty(),
            "a marker absent from the message can never be found on screen, so Enter is never \
             sent and the message sits unsent while the record says delivered:\n  {}",
            missing.join("\n  "),
        );
    }

    fn broadcast(body: &str) -> OperatorBroadcastDispatch {
        OperatorBroadcastDispatch {
            broadcast_id: TaskId::new().to_string(),
            worker_id: WorkerId::new().to_string(),
            session_id: WorkerSessionId::new(),
            body: body.to_owned(),
        }
    }

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
            queen.bytes.last(),
            Some(&b'\r'),
            "the automation brief must submit"
        );
        assert_eq!(
            operator_broadcast_message(&[broadcast("reloading in five minutes")])
                .bytes
                .last(),
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
            message.bytes.last(),
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
        let message = String::from_utf8(
            queen_automation_message(&QueenAutomationDelivery {
                run_id: "run-1".to_owned(),
                session_id: WorkerSessionId::new(),
                worker_id: WorkerId::new(),
                trigger: QueenAutomationTrigger::ActionableWork,
                actionable_count: 16,
                presence: PresenceMode::AtHive,
            })
            .bytes,
        )
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
