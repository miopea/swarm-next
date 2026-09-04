# ADR 0068: Interactive continuation failure evidence

Status: Accepted implementation design under REC-01; fallback execution remains
in progress and real-provider acceptance remains required.

## Decision

The engine observes the actual startup stream of its Claude child. The official
[common workflows documentation](https://code.claude.com/docs/en/common-workflows)
(checked 2026-09-04) says native continuation prints
`No conversation found to continue` and exits when no conversation exists.
This is distinct from a print-mode probe, an old transcript, or a visible prompt.

Positive evidence requires all of these together: a valid Continue attempt,
completed PTY reader, child exit code 1, the exact missing-context message with
only line framing/styling/cursor-visibility controls, no submitted input, no
accepted provider startup, and no engine-requested stop or manual selection fence.
Any extra prose, unsupported control sequence, overflow, read failure, missing
status, authentication/configuration failure or timeout is inconclusive.

Capture is private to the process, capped at 4 KiB, discarded on disarming, and
never included in logs, diagnostics or IPC. Only normalized absence evidence may
leave this owner. Non-Claude sessions do not capture startup bytes. Observing this
fact does not itself start another process or change a saved conversation.

## Required execution contract

The subsequent Continue-to-Fresh operation must validate this exact session and
attempt, create a new immutable process identity, retain an idempotent successor
relationship across API interruption, and atomically reconcile the worker binding
without overriding a newer manual choice. Fresh context must be labeled and must
not receive automatic replay of the previous task. These requirements are not
satisfied merely by recognizing the startup error.

Existing lifecycle and startup ownership in ADRs 0011 and 0064 remain in force.
There is no timeout-driven fallback, permission bypass, provider substitution,
or replacement of a running PTY under the same Swarm session identity.
