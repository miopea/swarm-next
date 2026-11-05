//! Maintenance admission of a real session (ADR 0156): the turn ledger, the
//! input written to the terminal, and the screen, under the control guards.
use super::*;
use crate::{
    MaintenanceOutcome, MaintenanceRefusal, ProviderDenial, ProviderLifecycleAcceptance,
    ProviderLifecycleGate, ProviderSessionStartObservation, ProviderTurnBackground,
    ProviderTurnEvent, ProviderTurnFailure, ProviderTurnObservation, ProviderTurnWait,
};
use swarm_domain::{
    PresenceDeviceId, ProviderConversationId, ProviderSessionStartKind, TerminalViewId,
};

const CAPABILITY: [u8; 32] = [42; 32];
const A: &str = "8e58a9b3-ed7d-414a-97f0-8c22cf6a1e6e";
const B: &str = "2991629b-888d-4835-986d-9d2f78ee003d";
const C: &str = "0d4b8f5e-3c1a-4e7b-9a2d-5f6e7c8b9a01";
const Q: &str = "6f4525f4-a0f4-47ed-96bb-f2dcc114d0ca";

const RESTING: &str = "✻ Worked for 3s\r\n\r\n❯ \r\nauto mode on · ? for shortcuts · ← for agents";
/// Measured: after most turns Claude draws a dim suggested prompt in the
/// composer. Its dimness is gone from the text, so it reads like typed text.
const SUGGESTED: &str =
    "✻ Worked for 3s\r\n\r\n❯ Count to 600 again\r\nauto mode on · ? for shortcuts · ← for agents";
/// Measured: Claude keeps its working footer up until every Stop hook has
/// returned, including one that is about to continue the turn.
const STOP_HOOKS_RUNNING: &str = "✻ Running stop hooks… 1/2 · esc to interrupt\r\n\r\n❯ ";
const MENU: &str = "● Fictional earlier work.\r\n\r\n────────────────────\r\n ☐ Jar \r\n\
     Which fictional jar?\r\n\r\n❯ 1. Amber\r\n  2. Teal\r\n  3. Type something.\r\n\
     ────────────────────\r\n  4. Chat about this\r\n\r\n\
     Enter to select · ↑/↓ to navigate · Esc to cancel";

struct Claude {
    session: Arc<ProcessTerminalSession>,
    conversation: ProviderConversationId,
}

impl Claude {
    /// A Claude session whose hooks have run, resting on its prompt. The
    /// process echoes nothing, so the screen is only what a test draws.
    fn start(registry: &SessionRegistry) -> Self {
        let command = ProviderCommand {
            executable: PathBuf::from("/bin/sh"),
            arguments: vec![
                "-c".into(),
                "stty -echo && printf ready && exec cat >/dev/null".into(),
            ],
            working_directory: std::env::temp_dir(),
        };
        let session = registry
            .spawn_provider_session(
                &command,
                TerminalSize::new(24, 80),
                false,
                Some(ProviderKind::ClaudeCode),
            )
            .unwrap();
        wait_for_ready(&session);
        let conversation = ProviderConversationId::new();
        *session.provider_lifecycle.lock().unwrap() =
            Some(ProviderLifecycleGate::new(session.id(), CAPABILITY));
        assert_eq!(
            session
                .observe_provider_start(
                    &CAPABILITY,
                    ProviderSessionStartObservation {
                        conversation,
                        kind: ProviderSessionStartKind::New,
                    }
                )
                .unwrap(),
            ProviderLifecycleAcceptance::Accepted
        );
        let claude = Self {
            session,
            conversation,
        };
        claude.show(RESTING);
        claude
    }

    fn show(&self, screen: &str) {
        self.session
            .terminal_state
            .lock()
            .unwrap()
            .push(format!("\x1b[2J\x1b[H{screen}").into_bytes());
    }

    fn report(&self, event: ProviderTurnEvent, prompt: Option<&str>, scheduled: Option<u16>) {
        assert!(
            self.session
                .observe_provider_turn(
                    &CAPABILITY,
                    &ProviderTurnObservation {
                        conversation: self.conversation,
                        event,
                        interview: false,
                        prompt: prompt.map(str::to_owned),
                        scheduled,
                    }
                )
                .unwrap()
        );
    }

    /// Typed and submitted the way Swarm's delivery does it, and taken.
    fn prompt(&self, prompt: &str) {
        self.session
            .write_input(b"Reply with exactly: OK\r")
            .unwrap();
        self.report(ProviderTurnEvent::Submitted, Some(prompt), None);
    }

    fn stop(&self, prompt: &str) {
        self.report(
            ProviderTurnEvent::Ended {
                background: Some(ProviderTurnBackground::default()),
            },
            Some(prompt),
            Some(0),
        );
    }

    fn refused(&self, reason: MaintenanceRefusal) -> MaintenanceOutcome {
        MaintenanceOutcome::refused(reason, Some(self.session.id))
    }
}

fn wait_for_ready(session: &ProcessTerminalSession) {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            let mut sequence = None;
            loop {
                let (resume, running) =
                    tokio::time::timeout(Duration::from_secs(5), session.wait_after(sequence))
                        .await
                        .expect("the fixture never started")
                        .unwrap();
                assert!(running, "the fixture exited");
                let snapshot = session.terminal_state.lock().unwrap().snapshot();
                if String::from_utf8_lossy(&snapshot.bytes).contains("ready") {
                    return;
                }
                sequence = match resume {
                    Resume::Snapshot { snapshot } => Some(snapshot.sequence),
                    Resume::Deltas { frames } => frames.last().map(|frame| frame.sequence),
                };
            }
        });
}

/// A registry for `sessions` sessions. Each test begins the drain itself, once
/// its sessions have started, since a draining registry starts none.
fn new_registry(sessions: usize) -> SessionRegistry {
    SessionRegistry::new(JournalLimits::default(), sessions, [std::env::temp_dir()]).unwrap()
}

fn admit(registry: &SessionRegistry, claude: &Claude) -> MaintenanceOutcome {
    registry.admit_maintenance(&[claude.session.id]).unwrap()
}

fn still_running(claude: &Claude) {
    assert!(claude.session.is_running().unwrap());
    assert!(!claude.session.stop_pending_release.load(Ordering::Acquire));
}

/// The case this exists for: every turn ended, nothing typed, nothing left
/// running, and the screen at rest. Admission stops it, and only it.
#[test]
fn a_resting_claude_session_is_stopped_with_its_exact_return() {
    let registry = new_registry(1);
    let claude = Claude::start(&registry);
    claude.prompt(A);
    claude.stop(A);
    registry.begin_drain().unwrap();
    assert_eq!(
        admit(&registry, &claude),
        MaintenanceOutcome::Stopped {
            session_ids: vec![claude.session.id]
        }
    );
    assert!(claude.session.stop_pending_release.load(Ordering::Acquire));
    assert!(
        claude.session.write_input(b"late\r").is_err(),
        "nothing reaches a session admission stopped"
    );
}

/// A suggested prompt on screen is not unsent input: what is unsent is judged
/// from what was written to the terminal, so a suggestion alone never holds
/// the update.
#[test]
fn a_suggested_prompt_on_screen_is_not_text_typed_and_unsent() {
    let registry = new_registry(1);
    let claude = Claude::start(&registry);
    claude.prompt(A);
    claude.stop(A);
    claude.show(SUGGESTED);
    registry.begin_drain().unwrap();
    assert_eq!(
        admit(&registry, &claude),
        MaintenanceOutcome::Stopped {
            session_ids: vec![claude.session.id]
        }
    );
}

/// Queen's change (2): text the operator typed and did not send, and an
/// operator takeover in progress, always refuse, and stop nothing.
#[test]
fn maintenance_refuses_unsent_operator_input_and_a_takeover_in_progress() {
    let registry = new_registry(1);
    let claude = Claude::start(&registry);
    claude.prompt(A);
    claude.stop(A);
    registry.begin_drain().unwrap();

    let identity = TerminalControlIdentity {
        device: PresenceDeviceId::new(),
        view: TerminalViewId::new(),
    };
    let grant = registry
        .claim_control(claude.session.id, identity, None, TerminalSize::new(24, 80))
        .unwrap();
    registry
        .write_controlled(claude.session.id, identity, grant.generation, b"half a th")
        .unwrap();
    assert_eq!(
        admit(&registry, &claude),
        claude.refused(MaintenanceRefusal::InteractiveOwner),
        "the operator still holds the terminal"
    );
    claude
        .session
        .release_control(identity, grant.generation)
        .unwrap();
    assert_eq!(
        admit(&registry, &claude),
        claude.refused(MaintenanceRefusal::UnsentInput),
        "the operator let go, and the text is still in the composer"
    );
    still_running(&claude);

    // Sent, and taken as a prompt: the ledger accounts for it again.
    let grant = registry
        .claim_control(claude.session.id, identity, None, TerminalSize::new(24, 80))
        .unwrap();
    registry
        .write_controlled(claude.session.id, identity, grant.generation, b"ought\r")
        .unwrap();
    claude
        .session
        .release_control(identity, grant.generation)
        .unwrap();
    claude.report(ProviderTurnEvent::Submitted, Some(B), None);
    claude.stop(B);

    let lease = TerminalTakeoverLease {
        lease_id: FederationStewardTakeoverLeaseId::new(),
        revision: 1,
        expires_at: unix_timestamp() + 300,
    };
    registry.install_takeover(claude.session.id, lease).unwrap();
    assert_eq!(
        admit(&registry, &claude),
        claude.refused(MaintenanceRefusal::InteractiveOwner)
    );
    still_running(&claude);
    registry
        .release_takeover(claude.session.id, lease.lease_id, lease.revision)
        .unwrap();
    assert_eq!(
        admit(&registry, &claude),
        MaintenanceOutcome::Stopped {
            session_ids: vec![claude.session.id]
        }
    );
}

/// M2 and M3 through the session: a queued message and an interrupted turn
/// keep it busy until the hooks account for them.
#[test]
fn a_running_queued_or_interrupted_turn_refuses_until_its_turn_ends() {
    let registry = new_registry(1);
    let claude = Claude::start(&registry);
    registry.begin_drain().unwrap();

    claude.prompt(A);
    assert_eq!(
        admit(&registry, &claude),
        claude.refused(MaintenanceRefusal::TurnInProgress)
    );
    // Queued mid-turn: its submit names the running prompt.
    claude.session.write_input(b"and then this\r").unwrap();
    claude.report(ProviderTurnEvent::Submitted, Some(A), None);
    claude.stop(A);
    assert_eq!(
        admit(&registry, &claude),
        claude.refused(MaintenanceRefusal::TurnInProgress),
        "the queued message has not run yet"
    );
    claude.stop(Q);
    assert_eq!(claude.session.maintenance_settled(), Ok(()));

    // Interrupted: Esc fires no hook, and the prompt stays open until the
    // next fresh prompt, which a queued message can never be.
    claude.prompt(B);
    claude.session.write_input(b"\x1b").unwrap();
    assert_eq!(
        admit(&registry, &claude),
        claude.refused(MaintenanceRefusal::TurnInProgress)
    );
    claude.prompt(C);
    claude.stop(C);
    assert_eq!(claude.session.maintenance_settled(), Ok(()));

    claude.stop(Q);
    assert_eq!(
        admit(&registry, &claude),
        claude.refused(MaintenanceRefusal::TurnCountUnknown),
        "a turn ended that no submit started"
    );
    still_running(&claude);
}

/// M4: the hooks cannot see a Stop hook continuing the turn. The screen can,
/// so the screen refuses even when every report says the turn is over.
#[test]
fn a_turn_a_stop_hook_is_continuing_refuses_on_the_screen() {
    let registry = new_registry(1);
    let claude = Claude::start(&registry);
    registry.begin_drain().unwrap();
    claude.prompt(A);
    claude.stop(A);
    claude.show(STOP_HOOKS_RUNNING);
    assert_eq!(
        admit(&registry, &claude),
        claude.refused(MaintenanceRefusal::ScreenNotResting)
    );
    claude.stop(A);
    claude.show(RESTING);
    assert_eq!(
        admit(&registry, &claude),
        MaintenanceOutcome::Stopped {
            session_ids: vec![claude.session.id]
        }
    );
}

#[test]
fn a_question_a_failure_or_work_left_running_refuses() {
    let registry = new_registry(1);
    let claude = Claude::start(&registry);
    registry.begin_drain().unwrap();

    claude.prompt(A);
    claude.stop(A);
    claude.report(
        ProviderTurnEvent::Waiting(ProviderTurnWait::UsageLimit),
        None,
        None,
    );
    assert_eq!(
        admit(&registry, &claude),
        claude.refused(MaintenanceRefusal::WaitingOnOperator),
        "a usage limit reset and Claude did not carry on"
    );
    claude.prompt(B);
    claude.report(
        ProviderTurnEvent::Denied(ProviderDenial {
            tool: "Bash".into(),
            reason: "fictional refusal".into(),
            call: None,
        }),
        None,
        None,
    );
    claude.stop(B);
    assert_eq!(
        admit(&registry, &claude),
        claude.refused(MaintenanceRefusal::WaitingOnOperator),
        "Claude stopped on a refused call"
    );
    claude.prompt(A);
    claude.stop(A);
    claude.show(MENU);
    assert_eq!(
        admit(&registry, &claude),
        claude.refused(MaintenanceRefusal::WaitingOnOperator),
        "a menu is on screen"
    );
    claude.show(RESTING);

    claude.prompt(B);
    claude.report(
        ProviderTurnEvent::Failed(ProviderTurnFailure::RateLimit),
        Some(B),
        None,
    );
    assert_eq!(
        admit(&registry, &claude),
        claude.refused(MaintenanceRefusal::TurnFailed)
    );

    claude.prompt(A);
    claude.report(
        ProviderTurnEvent::Ended {
            background: Some(ProviderTurnBackground {
                shells: 1,
                ..ProviderTurnBackground::default()
            }),
        },
        Some(A),
        Some(0),
    );
    assert_eq!(
        admit(&registry, &claude),
        claude.refused(MaintenanceRefusal::BackgroundWork)
    );
    claude.prompt(B);
    claude.report(
        ProviderTurnEvent::Ended {
            background: Some(ProviderTurnBackground::default()),
        },
        Some(B),
        Some(1),
    );
    assert_eq!(
        admit(&registry, &claude),
        claude.refused(MaintenanceRefusal::ScheduledWork)
    );
    still_running(&claude);
}

#[test]
fn a_session_whose_hooks_never_ran_refuses() {
    let registry = new_registry(1);
    let claude = Claude::start(&registry);
    *claude.session.provider_lifecycle.lock().unwrap() = None;
    registry.begin_drain().unwrap();
    assert_eq!(
        admit(&registry, &claude),
        claude.refused(MaintenanceRefusal::ProviderEvidenceUnavailable)
    );
    still_running(&claude);
}

/// One session that cannot be stopped safely keeps every other one running.
#[test]
fn one_busy_session_refuses_the_whole_set() {
    let registry = new_registry(2);
    let resting = Claude::start(&registry);
    let busy = Claude::start(&registry);
    resting.prompt(A);
    resting.stop(A);
    busy.prompt(B);
    registry.begin_drain().unwrap();
    let outcome = registry
        .admit_maintenance(&[resting.session.id, busy.session.id])
        .unwrap();
    assert_eq!(outcome, busy.refused(MaintenanceRefusal::TurnInProgress));
    still_running(&resting);
    still_running(&busy);
}
