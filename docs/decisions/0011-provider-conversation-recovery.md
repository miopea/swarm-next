# ADR 0011: Durable provider conversation recovery

Status: **Accepted**, amended by the approved daily-driver maturity interview
(REC-01). Recovery ladder implementation and live acceptance remain incomplete.

## Context

A durable worker profile is not useful after a crash or reboot if restarting it
opens an empty provider conversation. A Swarm terminal session identifies one
PTY/process incarnation; it is not the same identity as Claude Code's saved
conversation. Directory-scoped `claude --continue` preserves legacy work but
can select the wrong conversation when more than one worker has used a
workspace.

## Decision

Swarm stores provider conversation identity separately from worker and terminal
session identity.

- A new Claude worker receives a durable conversation UUID before first launch
  and starts with `claude --session-id <uuid>`.
- Later process incarnations use `claude --resume <uuid>`.
- Profiles migrated with prior terminal history but no known conversation UUID
  use workspace-scoped `claude --continue`. Provider-native continuation is also
  the second recovery step when safe recovery of the chosen conversation fails.
- Fresh launches do not override Claude's configured permission mode.
- Browser and API restarts continue attaching to the live PTY and never launch
  another provider process.

Provider-specific recovery belongs to the provider adapter. The operator's
explicit conversation switch must become the default for future resumption;
filesystem timestamps are not authority to undo that choice. Provider-native
continue semantics may choose a different workspace conversation, so Swarm must
distinguish an exact restoration from that fallback in its recovery evidence.

The existing operator correction endpoint serializes with worker startup. The
saved choice and its worker-change event commit atomically, and connected views
are notified after commit. Repeating the same choice is a persistence no-op.
This changes the next startup target only; it must not move a running terminal.
It does not yet detect the provider's in-terminal conversation switch.

For Codex, an explicit saved conversation selects exact resume regardless of
whether Swarm has recorded a previous terminal session. Session history chooses
native continuation only when no exact identity is available. Without either,
startup is new. A history flag must not silently discard the selected identity.

## Consequences

- Crash, reboot, and intentional stop/start attempt to preserve the chosen
  conversation; fallback reporting distinguishes continuity from fresh context.
- New profiles start with their own identity. Exact recovery does not choose a
  different worker's conversation merely because they share a workspace;
  provider-native continue fallback retains the provider's workspace ambiguity.
- The terminal-host protocol carries a typed conversation-start policy and
  therefore advances to version 6.
- A planned terminal-host replacement is required to deploy this protocol
  change; ordinary API/browser releases remain sidecar-preserving.
- Migrated profiles retain the same directory-level ambiguity as legacy
  `--continue` until exact identity is imported.

## Failure behavior

The approved order is safe recovery of the chosen conversation, provider-native
continue (`--continue` for Claude or the supported provider equivalent), then a
fresh session only as the last attempt. Do not retry indefinitely or substitute
a provider. Uncertain errors are not proof that context is missing. Each attempt
has an owned resource/time bound and an explicit outcome; never use a timer alone
to declare a conversation restored or lost.

Fresh fallback must clearly say prior context was not restored and leave the
operator able to use the provider's resume command. It must not replay the prior
task's commands or claim the task resumed. If the provider has no supported
resumption capability, recovery is manual rather than an invented adapter path.

The host currently probes Claude in the same environment as the provider. The
probe's stderr is bounded and nonblocking; only the recognized exit-1 missing
result is evidence of absence. Other outcomes retain the exact resume attempt.
Confirmed missing exact context now selects native Continue through the domain
transition, rather than the former direct missing-to-New branch. This is only
startup selection: the full interactive ladder remains incomplete. The terminal
shows fallback startup provenance, not a claimed restoration outcome.

## Recovery operation contract (implementation in progress)

The domain model owns a unique recovery operation and numbered attempt tokens.
It advances Exact -> Continue -> Fresh only on positive context-unavailable
evidence, never on a PID, elapsed timer, or transport/authentication failure.
Late evidence from another attempt/operation is ignored. Exact restoration must
match the chosen identity; continuation and fresh results remain distinguishable.
An uncertain outcome requires resolution without starting a duplicate process.
This bounded model is implemented and tested. The host uses its first transition
for a confirmed missing exact conversation. Its attempt token is stored once on
the engine-owned process session and exposed as optional session-list metadata.
API/browser restarts can reread it without restarting the provider. Older hosts
omit it, meaning unknown, not successful restoration. It is startup provenance,
not mutable recovery outcome state. Persistence across engine replacement and
provider outcome evidence remain unimplemented.

Do not infer interactive continuation from a print-mode continuation probe:
[Claude's session documentation](https://code.claude.com/docs/en/sessions)
states that those modes can consider different sets of sessions. Preserve the
interactive provider contract and verify its outcome directly. The existing
exact-ID probe does not justify extending that technique to `--continue`.

For the pending Claude session-event integration, the official
[hooks reference](https://code.claude.com/docs/en/hooks) provides the current
conversation ID on `SessionStart`; `resume` covers command-line continuation and
in-terminal `/resume`. Events must be bound to the actual engine process
incarnation before updating the saved choice. Do not infer operator intent from
arbitrary terminal text, accept an old process's late report as current, or treat
a background fork as the foreground conversation. These are integration
requirements, not a claim that a hook has been installed.

Live disposable-worker acceptance on 2026-09-04 found that the API treated the
preassigned UUID of a never-launched Claude profile as a resume target. That sent
a first launch through exact-missing and `--continue`; after the repository trust
prompt, the submitted trust input correctly made absence evidence inconclusive
and the worker exited. The start selector now follows this ADR literally: a
preassigned identity with no session history is `New`, and the same identity is
`Resume` only after a process session has existed. This removes recovery from the
ordinary first-launch path without weakening the evidence guard.
