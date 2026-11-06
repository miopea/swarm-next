//! What maintenance admission may believe about a Claude session's turns (ADR 0156).
//!
//! Separate from [`crate::ProviderTurn`] on purpose. That one decides how a
//! worker READS, and leans on the screen to recover when a hook is lost. This
//! one decides whether stopping the session can lose work, so it never recovers
//! by guessing: whatever it cannot account for, it refuses.
//!
//! Measured on Claude Code 2.1.318 on 2026-11-05, in a throwaway session:
//! - `UserPromptSubmit` and `Stop` carry `prompt_id`; a `Stop` names the prompt
//!   whose turn it ends.
//! - A message typed while a turn runs fires its submit at once, under the
//!   RUNNING prompt's id. Its own turn later runs under a new id with no submit
//!   and ends with a `Stop` under that id.
//! - Esc fires no hook. The next fresh prompt carries a new id while the
//!   interrupted one is still open, and a queued message can never carry a new
//!   id, so that is when the interrupted one can be closed.
//! - A `Stop` hook that continues the turn produces a second `Stop` under the
//!   same id. Nothing the hooks send says it is coming; the screen does (ADR
//!   0156), so that is checked there.
use crate::{MaintenanceRefusal, ProviderTurnBackground, ProviderTurnEvent};

/// What the latest `Stop` said was left running or scheduled.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct AfterStop {
    background: Option<ProviderTurnBackground>,
    scheduled: Option<u16>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct TurnLedger {
    /// The prompt whose submit was seen and whose `Stop` was not.
    open: Option<String>,
    /// The prompt the latest `Stop` closed, so a continued turn's second `Stop`
    /// is not mistaken for a queued one's.
    last_closed: Option<String>,
    /// Messages typed while a turn ran; each runs later as its own turn.
    queued: u16,
    /// Something happened that the measurements do not account for, and which
    /// case the first such report was. Nothing clears it but a new session.
    unaccounted: Option<&'static str>,
    /// Writes to the terminal, counted before each one is attempted.
    writes: u64,
    /// `writes` when the latest submit arrived: anything written after it has
    /// not been taken by Claude yet.
    writes_at_submit: u64,
    /// The latest write did not end with Enter, so the composer may hold text.
    half_typed: bool,
    after_stop: Option<AfterStop>,
}

/// The report that lost the count, for the engine's journal: which case, and
/// what the ledger held then. Prompt ids only, never what a prompt said.
#[derive(Debug, Eq, PartialEq)]
pub(crate) struct LostCount {
    pub(crate) case: &'static str,
    pub(crate) reported: Option<String>,
    pub(crate) open: Option<String>,
    pub(crate) last_closed: Option<String>,
    pub(crate) queued: u16,
}

impl TurnLedger {
    /// Applies one turn report. `prompt` and `scheduled` are as the hook gave
    /// them; a submit or `Stop` without a prompt id cannot be accounted for.
    ///
    /// Returns what lost the count when this report is the one that did, so
    /// the engine can say so once: until it does, nobody can tell which case
    /// holds a resting worker's update up (ADR 0162).
    pub(crate) fn observe(
        &mut self,
        event: &ProviderTurnEvent,
        prompt: Option<&str>,
        scheduled: Option<u16>,
    ) -> Option<LostCount> {
        let case = self.apply(event, prompt, scheduled)?;
        if self.unaccounted.is_some() {
            return None;
        }
        self.unaccounted = Some(case);
        Some(LostCount {
            case,
            reported: prompt.map(str::to_owned),
            open: self.open.clone(),
            last_closed: self.last_closed.clone(),
            queued: self.queued,
        })
    }

    /// The case a report cannot be accounted for by, if it cannot.
    fn apply(
        &mut self,
        event: &ProviderTurnEvent,
        prompt: Option<&str>,
        scheduled: Option<u16>,
    ) -> Option<&'static str> {
        match event {
            ProviderTurnEvent::Submitted => {
                self.writes_at_submit = self.writes;
                let Some(prompt) = prompt else {
                    return Some("submit_without_prompt_id");
                };
                match (&self.open, self.queued) {
                    // Typed while this prompt's turn runs: it will run next.
                    (Some(open), _) if open == prompt => {
                        self.queued = self.queued.saturating_add(1);
                    }
                    // A fresh prompt. If one was open, it was interrupted:
                    // nothing queued can carry a new id.
                    (_, 0) => self.open = Some(prompt.to_owned()),
                    // Typed during a queued turn (whose id no submit named), or
                    // a fresh prompt after that turn was interrupted. The two
                    // look the same, and they leave different work behind.
                    _ => return Some("fresh_prompt_while_queued"),
                }
            }
            ProviderTurnEvent::Ended { background } => {
                self.after_stop = Some(AfterStop {
                    background: *background,
                    scheduled,
                });
                let Some(prompt) = prompt else {
                    return Some("stop_without_prompt_id");
                };
                if self.open.as_deref() == Some(prompt) {
                    self.last_closed = self.open.take();
                } else if self.last_closed.as_deref() == Some(prompt) {
                    // A Stop hook continued that turn; this is its second Stop.
                } else if self.queued > 0 {
                    self.queued -= 1;
                    self.last_closed = Some(prompt.to_owned());
                } else {
                    // A turn ended that no submit started.
                    return Some("stop_without_submit");
                }
            }
            // The turn is over but said nothing about what it left. The
            // session's phase refuses until another turn ends normally.
            ProviderTurnEvent::Failed(_) => {
                if prompt.is_some() && self.open.as_deref() == prompt {
                    self.last_closed = self.open.take();
                }
            }
            ProviderTurnEvent::Waiting(_)
            | ProviderTurnEvent::Resolved
            | ProviderTurnEvent::BatchResolved { .. }
            | ProviderTurnEvent::Denied(_) => {}
        }
        None
    }

    /// Counts one write to the terminal, before it is attempted.
    ///
    /// A focus report is not a keystroke: a terminal sends one when it gains or
    /// loses focus, and it never puts text in the composer.
    pub(crate) fn observe_write(&mut self, bytes: &[u8]) {
        if matches!(bytes, b"\x1b[I" | b"\x1b[O") {
            return;
        }
        self.writes = self.writes.saturating_add(1);
        self.half_typed = bytes.last() != Some(&b'\r');
    }

    /// Whether the hooks and the writes account for every turn as finished,
    /// with nothing typed, running or scheduled after it.
    pub(crate) fn settled(&self) -> Result<(), MaintenanceRefusal> {
        if self.unaccounted.is_some() {
            return Err(MaintenanceRefusal::TurnCountUnknown);
        }
        if self.open.is_some() || self.queued > 0 {
            return Err(MaintenanceRefusal::TurnInProgress);
        }
        if self.half_typed || self.writes != self.writes_at_submit {
            return Err(MaintenanceRefusal::UnsentInput);
        }
        if let Some(after) = self.after_stop {
            // No list is unknown, not empty.
            if after
                .background
                .is_none_or(ProviderTurnBackground::is_running)
            {
                return Err(MaintenanceRefusal::BackgroundWork);
            }
            if after.scheduled != Some(0) {
                return Err(MaintenanceRefusal::ScheduledWork);
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ProviderTurnFailure;

    const A: &str = "8e58a9b3-ed7d-414a-97f0-8c22cf6a1e6e";
    const B: &str = "2991629b-888d-4835-986d-9d2f78ee003d";
    const Q: &str = "6f4525f4-a0f4-47ed-96bb-f2dcc114d0ca";

    /// A Stop that reports nothing running in the background.
    fn clear() -> ProviderTurnBackground {
        ProviderTurnBackground::default()
    }

    fn submit(ledger: &mut TurnLedger, prompt: &str) {
        ledger.observe(&ProviderTurnEvent::Submitted, Some(prompt), None);
    }

    fn stop(ledger: &mut TurnLedger, prompt: &str) {
        ledger.observe(
            &ProviderTurnEvent::Ended {
                background: Some(clear()),
            },
            Some(prompt),
            Some(0),
        );
    }

    /// Typed and submitted the way the operator and Swarm's delivery do it.
    fn type_and_submit(ledger: &mut TurnLedger, prompt: &str) {
        ledger.observe_write(b"Reply with exactly: BASELINE");
        ledger.observe_write(b"\r");
        submit(ledger, prompt);
    }

    #[test]
    fn a_fresh_session_and_a_finished_turn_are_settled_and_a_running_one_is_not() {
        let mut ledger = TurnLedger::default();
        assert_eq!(ledger.settled(), Ok(()));
        type_and_submit(&mut ledger, A);
        assert_eq!(ledger.settled(), Err(MaintenanceRefusal::TurnInProgress));
        stop(&mut ledger, A);
        assert_eq!(ledger.settled(), Ok(()));
    }

    /// M2: the queued message's submit names the RUNNING prompt, and its own
    /// turn ends under an id no submit named.
    #[test]
    fn a_message_queued_mid_turn_keeps_the_session_busy_until_its_own_turn_ends() {
        let mut ledger = TurnLedger::default();
        type_and_submit(&mut ledger, A);
        ledger.observe_write(b"Reply with exactly: QUEUED-OK\r");
        submit(&mut ledger, A);
        stop(&mut ledger, A);
        assert_eq!(ledger.settled(), Err(MaintenanceRefusal::TurnInProgress));
        stop(&mut ledger, Q);
        assert_eq!(ledger.settled(), Ok(()));
    }

    /// M3: Esc fires nothing, and the next fresh prompt's new id closes it.
    #[test]
    fn an_interrupted_turn_stays_open_until_the_next_fresh_prompt() {
        let mut ledger = TurnLedger::default();
        type_and_submit(&mut ledger, A);
        ledger.observe_write(b"\x1b");
        assert_eq!(ledger.settled(), Err(MaintenanceRefusal::TurnInProgress));
        type_and_submit(&mut ledger, B);
        assert_eq!(ledger.settled(), Err(MaintenanceRefusal::TurnInProgress));
        stop(&mut ledger, B);
        assert_eq!(ledger.settled(), Ok(()));
    }

    /// M3: Esc with a message queued runs that message at once, under a new id
    /// and with no submit. The interrupted prompt is still open after it.
    #[test]
    fn esc_with_a_message_queued_stays_open_until_the_next_fresh_prompt() {
        let mut ledger = TurnLedger::default();
        type_and_submit(&mut ledger, A);
        ledger.observe_write(b"Reply with exactly: QUEUED-THEN-ESC\r");
        submit(&mut ledger, A);
        ledger.observe_write(b"\x1b");
        stop(&mut ledger, Q);
        assert_eq!(ledger.settled(), Err(MaintenanceRefusal::TurnInProgress));
        type_and_submit(&mut ledger, B);
        stop(&mut ledger, B);
        assert_eq!(ledger.settled(), Ok(()));
    }

    /// M4: the hooks cannot see a continuation coming, but its second `Stop`
    /// must not be counted as a queued message's.
    #[test]
    fn a_continued_turn_s_second_stop_is_not_a_queued_turn_ending() {
        let mut ledger = TurnLedger::default();
        type_and_submit(&mut ledger, A);
        ledger.observe_write(b"queued\r");
        submit(&mut ledger, A);
        stop(&mut ledger, A);
        stop(&mut ledger, A);
        assert_eq!(ledger.settled(), Err(MaintenanceRefusal::TurnInProgress));
        stop(&mut ledger, Q);
        assert_eq!(ledger.settled(), Ok(()));
    }

    #[test]
    fn what_the_measurements_do_not_explain_refuses_for_the_rest_of_the_session() {
        // A submit with a new id while a queued turn is outstanding: typed
        // during that turn, or a fresh prompt after it was interrupted.
        let mut ledger = TurnLedger::default();
        type_and_submit(&mut ledger, A);
        submit(&mut ledger, A);
        stop(&mut ledger, A);
        type_and_submit(&mut ledger, B);
        assert_eq!(ledger.settled(), Err(MaintenanceRefusal::TurnCountUnknown));
        stop(&mut ledger, B);
        stop(&mut ledger, Q);
        type_and_submit(&mut ledger, "c");
        stop(&mut ledger, "c");
        assert_eq!(ledger.settled(), Err(MaintenanceRefusal::TurnCountUnknown));

        // A turn ending that no submit started.
        let mut ledger = TurnLedger::default();
        stop(&mut ledger, Q);
        assert_eq!(ledger.settled(), Err(MaintenanceRefusal::TurnCountUnknown));

        // A report without a prompt id.
        for event in [
            ProviderTurnEvent::Submitted,
            ProviderTurnEvent::Ended {
                background: Some(clear()),
            },
        ] {
            let mut ledger = TurnLedger::default();
            ledger.observe(&event, None, Some(0));
            assert_eq!(ledger.settled(), Err(MaintenanceRefusal::TurnCountUnknown));
        }
    }

    /// ADR 0162: the report that loses the count says which case it was, once,
    /// so the engine's journal can name it. Later reports say nothing.
    #[test]
    fn the_report_that_loses_the_count_names_its_case_once() {
        let mut ledger = TurnLedger::default();
        type_and_submit(&mut ledger, A);
        submit(&mut ledger, A);
        stop(&mut ledger, A);
        assert_eq!(
            ledger.observe(&ProviderTurnEvent::Submitted, Some(B), None),
            Some(LostCount {
                case: "fresh_prompt_while_queued",
                reported: Some(B.to_owned()),
                open: None,
                last_closed: Some(A.to_owned()),
                queued: 1,
            })
        );
        let stopped = ProviderTurnEvent::Ended {
            background: Some(clear()),
        };
        assert_eq!(ledger.observe(&stopped, Some("z"), Some(0)), None, "once");
        assert_eq!(ledger.settled(), Err(MaintenanceRefusal::TurnCountUnknown));

        let mut ledger = TurnLedger::default();
        let lost = ledger.observe(&stopped, Some(Q), Some(0)).unwrap();
        assert_eq!(lost.case, "stop_without_submit");
        let lost = TurnLedger::default()
            .observe(&ProviderTurnEvent::Submitted, None, None)
            .unwrap();
        assert_eq!(lost.case, "submit_without_prompt_id");
        let lost = TurnLedger::default()
            .observe(&stopped, None, Some(0))
            .unwrap();
        assert_eq!(lost.case, "stop_without_prompt_id");
        assert_eq!(
            TurnLedger::default().observe(&ProviderTurnEvent::Submitted, Some(A), None),
            None,
            "an ordinary report loses nothing"
        );
    }

    /// Queen's change (2): unsent operator input always refuses.
    #[test]
    fn text_typed_and_not_submitted_refuses_until_a_submit_takes_it() {
        let mut ledger = TurnLedger::default();
        type_and_submit(&mut ledger, A);
        stop(&mut ledger, A);
        ledger.observe_write(b"half a thought");
        assert_eq!(ledger.settled(), Err(MaintenanceRefusal::UnsentInput));
        ledger.observe_write(b"\r");
        assert_eq!(
            ledger.settled(),
            Err(MaintenanceRefusal::UnsentInput),
            "submitted, but Claude has not taken it yet"
        );
        submit(&mut ledger, B);
        stop(&mut ledger, B);
        assert_eq!(ledger.settled(), Ok(()));

        // Typed while the turn ran, then the turn ended: still in the composer.
        type_and_submit(&mut ledger, A);
        ledger.observe_write(b"for later");
        stop(&mut ledger, A);
        assert_eq!(ledger.settled(), Err(MaintenanceRefusal::UnsentInput));
    }

    #[test]
    fn a_focus_report_is_not_typing() {
        let mut ledger = TurnLedger::default();
        type_and_submit(&mut ledger, A);
        stop(&mut ledger, A);
        ledger.observe_write(b"\x1b[I");
        ledger.observe_write(b"\x1b[O");
        assert_eq!(ledger.settled(), Ok(()));
    }

    #[test]
    fn background_work_or_a_cron_left_by_the_last_turn_refuses() {
        for (background, scheduled, expected) in [
            (
                Some(ProviderTurnBackground {
                    shells: 1,
                    ..ProviderTurnBackground::default()
                }),
                Some(0),
                Err(MaintenanceRefusal::BackgroundWork),
            ),
            (
                Some(ProviderTurnBackground {
                    workflows: 1,
                    ..ProviderTurnBackground::default()
                }),
                Some(0),
                Err(MaintenanceRefusal::BackgroundWork),
            ),
            // Claude could not list its tasks: unknown, not none.
            (None, Some(0), Err(MaintenanceRefusal::BackgroundWork)),
            (
                Some(clear()),
                Some(1),
                Err(MaintenanceRefusal::ScheduledWork),
            ),
            (Some(clear()), None, Err(MaintenanceRefusal::ScheduledWork)),
            (Some(clear()), Some(0), Ok(())),
        ] {
            let mut ledger = TurnLedger::default();
            type_and_submit(&mut ledger, A);
            ledger.observe(&ProviderTurnEvent::Ended { background }, Some(A), scheduled);
            assert_eq!(ledger.settled(), expected, "{background:?} {scheduled:?}");
        }
    }

    #[test]
    fn a_failed_turn_closes_its_prompt_and_leaves_the_refusal_to_its_phase() {
        let mut ledger = TurnLedger::default();
        type_and_submit(&mut ledger, A);
        ledger.observe(
            &ProviderTurnEvent::Failed(ProviderTurnFailure::RateLimit),
            Some(A),
            None,
        );
        assert_eq!(ledger.settled(), Ok(()));
    }
}
