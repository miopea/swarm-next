# ADR 0011: Durable provider conversation recovery

Status: **Accepted**

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
  use workspace-scoped `claude --continue` as an explicit compatibility path.
- Fresh launches do not override Claude's configured permission mode.
- Browser and API restarts continue attaching to the live PTY and never launch
  another provider process.

The compatibility path is owned by the Claude provider adapter. It may be
removed after import tooling can identify and bind legacy Claude session IDs,
or after all active migrated profiles have been recreated with exact identity.

## Consequences

- Crash, reboot, and intentional stop/start preserve conversational context.
- New profiles cannot resume another worker merely because they share a
  workspace.
- The terminal-host protocol carries a typed conversation-start policy and
  therefore advances to version 6.
- A planned terminal-host replacement is required to deploy this protocol
  change; ordinary API/browser releases remain sidecar-preserving.
- Migrated profiles retain the same directory-level ambiguity as legacy
  `--continue` until exact identity is imported.

## Failure behavior

An exact resume failure remains visible in the terminal instead of silently
starting a blank conversation. Swarm does not fall back from a known UUID to a
different workspace conversation.
